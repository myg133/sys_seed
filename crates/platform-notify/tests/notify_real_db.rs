//! REQ-015 **真实库**集成用例：死信重投 / 多语言回退 / 订阅偏好 / 投递快照与 RLS。
//!
//! 覆盖：AC-S-023、AC-S-168、AC-S-169、AC-S-173。
//!
//! # ⚠️ 缺 `DATABASE_URL_TEST` → **失败**，不是跳过
//!
//! 跳过也算通过 = 假绿（dev-guide §9.14）。
//!
//! # ⚠️ 判据写法（§9.15.2）
//!
//! - 「不重复投递已成功的」→ 断言 provider **调用计数**，不是只看库里的状态；
//! - 「三级回退取对了哪一行」→ 断言**具体文案**，不是「有结果」；
//! - 「退订留痕」→ 断言 `attempt_count == 0` 且 `status == dead_letter`。

mod common;

use common::*;

/// 让 `tracing::error!` 的细节在失败时可见（否则只剩 `DATABASE_ERROR`）。
fn init_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::ERROR)
        .with_test_writer()
        .try_init();
}
use platform_notify::model::{category, status};
use platform_notify::store::{TemplateUpdate, NewTemplate};

/// **AC-S-023**：重投只重置死信那一条，**已成功的不重复投递**。
///
/// ⚠️ 这条判据的核心是 `CountingSender::call_count()`：
/// 只查数据库状态的话，「发了两次但状态只记一次」的实现也会绿。
#[tokio::test]
async fn resending_a_dead_letter_does_not_redeliver_the_delivered_one() {
    let fixture = setup().await;
    let ctx = fixture.ctx_a();
    seed_template(&fixture, &ctx, "code", "in_app", "zh-CN", category::TRANSACTIONAL).await;

    let sender = CountingSender::new();

    // 入队 2 条（都发同一收件人）。
    let mut req = platform_notify::service::EnqueueRequest {
        template_code: "code".to_string(),
        channel: "in_app".to_string(),
        recipient: "u1".to_string(),
        locale: Some("zh-CN".to_string()),
        payload: std::collections::BTreeMap::new(),
    };
    req.payload.insert("name".to_string(), "张三".to_string());

    let first = fixture.service.enqueue(&ctx, &req).await.expect("入队 1");
    let second = fixture.service.enqueue(&ctx, &req).await.expect("入队 2");

    // 第一条：投递成功。
    let mut registry = platform_notify::channels::ChannelRegistry::new();
    registry.insert(
        "in_app",
        std::sync::Arc::new(platform_notify::worker::PlatformChannel::new(
            std::sync::Arc::new(sender.clone()),
        )),
    );
    let stats = platform_notify::worker::tick(&fixture.service, &ctx, &registry, 10)
        .await
        .expect("tick 1");
    assert_eq!(stats.delivered, 2, "两条都应成功");
    assert_eq!(sender.call_count(), 2, "provider 应被调用 2 次");

    let ok_id = first.delivery.id.clone();
    let ok_delivered_at = delivery_field(&fixture, &ok_id, "delivered_at")
        .await
        .expect("第一条应有 delivered_at");
    let ok_attempts = delivery_field(&fixture, &ok_id, "attempt_count").await;

    // 把第二条强制改成「次数用尽 + 死信」。
    let dead_id = second.delivery.id.clone();
    fixture
        .admin
        .execute(&format!(
            "UPDATE platform_notify_delivery SET status = 'dead_letter', attempt_count = 5 \
             WHERE id = '{dead_id}'"
        ))
        .await;

    // 重投死信。
    let resent = fixture
        .service
        .resend_dead_letter(&ctx, &dead_id)
        .await
        .expect("重投应成功");
    assert_eq!(resent.status, status::PENDING, "重投后应是 pending");
    assert_eq!(resent.attempt_count, 0, "重投必须重置计数为 0");
    assert!(resent.last_error.is_none(), "重投必须清空 last_error");

    // 再 tick 一次：只有死信那条被重新投递。
    sender.reset();
    let stats2 = platform_notify::worker::tick(&fixture.service, &ctx, &registry, 10)
        .await
        .expect("tick 2");
    assert_eq!(stats2.delivered, 1, "只有重投的那一条");
    assert_eq!(
        sender.call_count(),
        1,
        "provider 只应被多调用 1 次（已成功的那条**不得**重复投递）"
    );

    // 已成功那条的三个字段**逐字段不变**。
    assert_eq!(
        delivery_field(&fixture, &ok_id, "delivered_at").await,
        Some(ok_delivered_at),
        "已成功那条的 delivered_at 不得变"
    );
    assert_eq!(
        delivery_field(&fixture, &ok_id, "attempt_count").await,
        ok_attempts,
        "已成功那条的 attempt_count 不得变"
    );
    assert_eq!(
        delivery_field(&fixture, &ok_id, "status").await,
        Some("delivered".to_string()),
        "已成功那条的状态不得回退"
    );

    // 重投那条的计数**恰好 1**（不是 5、不是 6）。
    assert_eq!(
        delivery_field(&fixture, &dead_id, "attempt_count").await,
        Some("1".to_string()),
        "重投后第一次投递，计数应恰好为 1"
    );
}

/// **AC-S-172 步骤 2/3**：重投**非死信** → `NOTIFY_NOT_DEAD_LETTER`，且字段**逐字段不变**。
#[tokio::test]
async fn resending_a_non_dead_letter_is_rejected_and_changes_nothing() {
    let fixture = setup().await;
    let ctx = fixture.ctx_a();

    for (status_value, label) in [("delivered", "已成功"), ("pending", "待投递")] {
        let id = seed_delivery(&fixture, TENANT_A, status_value, 2, "now()").await;
        let before_status = delivery_field(&fixture, &id, "status").await;
        let before_attempts = delivery_field(&fixture, &id, "attempt_count").await;

        let err = fixture
            .service
            .resend_dead_letter(&ctx, &id)
            .await
            .expect_err(&format!("{label}的记录不得重投"));
        assert_eq!(err.code(), "NOTIFY_NOT_DEAD_LETTER", "{label}");

        assert_eq!(
            delivery_field(&fixture, &id, "status").await,
            before_status,
            "{label}：status 不得变"
        );
        assert_eq!(
            delivery_field(&fixture, &id, "attempt_count").await,
            before_attempts,
            "{label}：attempt_count 不得变"
        );
    }
}

/// **AC-S-172 步骤 5**：并发两个重投 → **恰好 1 成功 1 失败**。
///
/// ⚠️ 这条必须真并发：写成串行的话第一个已改成 `pending`，
/// 第二个自然 409 → **看起来测了并发，其实测了串行**（D314 / S-07）。
///
/// 判据落在 **`rows_affected` 的集合 == {1, 0}** 上。
#[tokio::test]
async fn two_concurrent_resends_yield_exactly_one_success() {
    let fixture = setup().await;
    let ctx = fixture.ctx_a();
    let id = seed_delivery(&fixture, TENANT_A, "dead_letter", 5, "now()").await;

    // 两个独立事务，各自执行同一条带守卫的 UPDATE —— 这才是并发判据的落点。
    let url = app_role_url(&required_test_database_url());
    let db1 = platform_db::TenantDb::connect(&url, platform_db::DbOptions::default())
        .await
        .expect("连接 1");
    let db2 = platform_db::TenantDb::connect(&url, platform_db::DbOptions::default())
        .await
        .expect("连接 2");

    let barrier = std::sync::Arc::new(tokio::sync::Barrier::new(2));
    let make = |db: platform_db::TenantDb, barrier: std::sync::Arc<tokio::sync::Barrier>, ctx: platform_core::RequestContext, id: String| async move {
        barrier.wait().await;
        let store = platform_notify::store::NotifyStore::new(db);
        store.resend_dead_letter(&ctx, &id).await
    };

    let (r1, r2) = tokio::join!(
        make(db1, barrier.clone(), ctx.clone(), id.clone()),
        make(db2, barrier, ctx.clone(), id.clone())
    );

    let a = r1.expect("查询 1 不应报错");
    let b = r2.expect("查询 2 不应报错");
    let mut affected = vec![a, b];
    affected.sort_unstable();
    assert_eq!(
        affected,
        vec![0, 1],
        "并发两个重投必须**恰好**一个影响 1 行、另一个影响 0 行（得到 {affected:?}）"
    );
}

/// **AC-S-168**：三级回退**取对了哪一行**（断言具体文案，不是「有结果」）。
#[tokio::test]
async fn the_locale_fallback_picks_the_right_row() {
    let fixture = setup().await;
    let ctx = fixture.ctx_a();

    // 同一 (code, channel) 写 zh-CN 与 en-US 两行（不同文案）。
    for (locale, text) in [("zh-CN", "中文正文"), ("en-US", "English body")] {
        let new = NewTemplate {
            id: uuid::Uuid::new_v4().to_string(),
            code: "multi".to_string(),
            channel: "in_app".to_string(),
            locale: locale.to_string(),
            category: category::TRANSACTIONAL.to_string(),
            title_template: format!("{locale} 标题"),
            body_template: text.to_string(),
        };
        fixture.service.create_template(&ctx, &new).await.expect("建模板");
    }

    // 用户 locale = en-US → 取 en-US 那行。
    let en = fixture
        .service
        .resolve_template(&ctx, "multi", "in_app", Some("en-US"))
        .await
        .expect("en-US 应命中");
    assert_eq!(en.body_template, "English body", "必须取 en-US 那行");

    // 用户 locale = ja-JP（没有这行）→ 回退到租户默认 zh-CN。
    let ja = fixture
        .service
        .resolve_template(&ctx, "multi", "in_app", Some("ja-JP"))
        .await
        .expect("ja-JP 应回退到 zh-CN");
    assert_eq!(
        ja.body_template, "中文正文",
        "ja-JP 必须回退到**租户默认** zh-CN（不是 en-US）"
    );
    assert_eq!(ja.locale, "zh-CN", "回退后的 locale 必须是 zh-CN");

    // 删掉 zh-CN → 用户 ja-JP **失败**（不得回退到任意存在的 locale）。
    fixture
        .admin
        .execute("DELETE FROM platform_notify_template WHERE locale = 'zh-CN'")
        .await;
    let err = fixture
        .service
        .resolve_template(&ctx, "multi", "in_app", Some("ja-JP"))
        .await
        .expect_err("zh-CN 删掉后 ja-JP 必须失败");
    assert_eq!(
        err.code(),
        "NOTIFY_TEMPLATE_NOT_FOUND",
        "不得静默回退到 en-US（那会让用户收到看不懂的语言）"
    );
}

/// **AC-S-168 步骤 1 的库层判据**：`UNIQUE (tenant, code, channel, locale)` 里**含 locale**
/// → 同一 `(code, channel)` 的**两个 locale 能共存**（漏了 locale 就会 409）。
#[tokio::test]
async fn two_locales_of_the_same_template_coexist() {
    let fixture = setup().await;
    let ctx = fixture.ctx_a();
    seed_template(&fixture, &ctx, "dup", "in_app", "zh-CN", category::TRANSACTIONAL).await;

    // 同 code+channel，不同 locale → 必须成功（不是 409）。
    let other = NewTemplate {
        id: uuid::Uuid::new_v4().to_string(),
        code: "dup".to_string(),
        channel: "in_app".to_string(),
        locale: "en-US".to_string(),
        category: category::TRANSACTIONAL.to_string(),
        title_template: "t".to_string(),
        body_template: "b".to_string(),
    };
    fixture
        .service
        .create_template(&ctx, &other)
        .await
        .expect("不同 locale 必须能共存（唯一约束含 locale）");

    // 同 (code, channel, locale) 再来一次 → 必须失败（唯一冲突）。
    let duplicate = NewTemplate {
        id: uuid::Uuid::new_v4().to_string(),
        code: "dup".to_string(),
        channel: "in_app".to_string(),
        locale: "zh-CN".to_string(),
        category: category::TRANSACTIONAL.to_string(),
        title_template: "t2".to_string(),
        body_template: "b2".to_string(),
    };
    let err = fixture
        .service
        .create_template(&ctx, &duplicate)
        .await
        .expect_err("同 (code, channel, locale) 必须失败");
    // 唯一冲突在 http 层翻成 NOTIFY_TEMPLATE_DUPLICATED；服务层这里是 500。
    assert!(
        err.code() == "INTERNAL_ERROR" || err.code() == "NOTIFY_TEMPLATE_DUPLICATED",
        "唯一冲突应报出来，实际 {}",
        err.code()
    );
}

/// **AC-S-169**：退订后投递进死信且 **`attempt_count == 0`**（D312 钉死）。
///
/// ⚠️ 判据要求「退订检查在**投递尝试之前**」——
/// 若留空间给 1，「退订后还重试 5 次」这种实现也能过（它的 last_error 同样是退订）。
#[tokio::test]
async fn an_opted_out_recipient_goes_straight_to_dead_letter_with_zero_attempts() {
    init_tracing();
    let fixture = setup().await;
    let ctx = fixture.ctx_a();
    seed_template(
        &fixture,
        &ctx,
        "promo",
        "in_app",
        "zh-CN",
        category::MARKETING,
    )
    .await;

    // 退订 marketing。
    fixture
        .service
        .set_preference(&ctx, "u1", category::MARKETING, true)
        .await
        .expect("退订应成功");

    let mut payload = std::collections::BTreeMap::new();
    payload.insert("name".to_string(), "张三".to_string());
    let outcome = fixture
        .service
        .enqueue(
            &ctx,
            &platform_notify::service::EnqueueRequest {
                template_code: "promo".to_string(),
                channel: "in_app".to_string(),
                recipient: "u1".to_string(),
                locale: Some("zh-CN".to_string()),
                payload,
            },
        )
        .await
        .expect("入队应成功");

    assert!(outcome.opted_out, "应标记为已退订");
    assert_eq!(outcome.delivery.status, status::DEAD_LETTER, "必须进死信");
    assert_eq!(
        outcome.delivery.attempt_count, 0,
        "**attempt_count 必须是 0**（D312：退订检查在投递尝试之前）"
    );
    let last_error = outcome.delivery.last_error.clone().unwrap_or_default();
    assert!(
        last_error.contains("opted out"),
        "last_error 必须写明退订原因（可查）：{last_error}"
    );
}

/// **AC-S-169 步骤 5**：退订 `system` → `NOTIFY_CATEGORY_MANDATORY`，
/// 且偏好表里**不得出现 `system` 行**（先校验后写）。
#[tokio::test]
async fn the_system_category_cannot_be_unsubscribed_and_leaves_no_row() {
    let fixture = setup().await;
    let ctx = fixture.ctx_a();

    let err = fixture
        .service
        .set_preference(&ctx, "u1", category::SYSTEM, true)
        .await
        .expect_err("system 类不得退订");
    assert_eq!(err.code(), "NOTIFY_CATEGORY_MANDATORY");

    // ⚠️ 关键判据：偏好表里**不得出现 system 行**
    //（若「先写后报错」，用户的 system 就真的被关掉了）。
    let rows = fixture
        .admin
        .query_text(
            "SELECT count(*)::text FROM platform_notify_preference WHERE category = 'system'",
        )
        .await;
    let count = rows.first().map(String::as_str).unwrap_or("?");
    assert_eq!(count, "0", "偏好表里不得出现 system 行（先校验后写）");
}

/// **AC-S-169 步骤 6/7**：`system` 类不受 marketing 退订影响；退订可逆。
#[tokio::test]
async fn the_system_category_is_unaffected_and_unsubscribe_is_reversible() {
    let fixture = setup().await;
    let ctx = fixture.ctx_a();
    seed_template(&fixture, &ctx, "alert", "in_app", "zh-CN", category::SYSTEM).await;
    seed_template(&fixture, &ctx, "promo", "in_app", "zh-CN", category::MARKETING).await;

    // 退订 marketing。
    fixture
        .service
        .set_preference(&ctx, "u1", category::MARKETING, true)
        .await
        .expect("退订 marketing");

    let payload = {
        let mut map = std::collections::BTreeMap::new();
        map.insert("name".to_string(), "张三".to_string());
        map
    };

    // system 类通知：**仍应投递**（不受 marketing 退订影响）。
    let system_outcome = fixture
        .service
        .enqueue(
            &ctx,
            &platform_notify::service::EnqueueRequest {
                template_code: "alert".to_string(),
                channel: "in_app".to_string(),
                recipient: "u1".to_string(),
                locale: Some("zh-CN".to_string()),
                payload: payload.clone(),
            },
        )
        .await
        .expect("入队 system");
    assert!(!system_outcome.opted_out, "system 类不该被 marketing 退订影响");
    assert_eq!(system_outcome.delivery.status, status::PENDING);

    // 重新订阅 marketing → 投递应成功（不 opt_out）。
    fixture
        .service
        .set_preference(&ctx, "u1", category::MARKETING, false)
        .await
        .expect("重新订阅");
    let promo_outcome = fixture
        .service
        .enqueue(
            &ctx,
            &platform_notify::service::EnqueueRequest {
                template_code: "promo".to_string(),
                channel: "in_app".to_string(),
                recipient: "u1".to_string(),
                locale: Some("zh-CN".to_string()),
                payload,
            },
        )
        .await
        .expect("入队 promo");
    assert!(!promo_outcome.opted_out, "重新订阅后不该再判定为退订");
    assert_eq!(promo_outcome.delivery.status, status::PENDING, "退订可逆");
}

/// **AC-S-173 步骤 2**：`rendered_*` 是**快照** —— 模板改了不影响已入队的记录。
#[tokio::test]
async fn the_rendered_body_is_a_snapshot() {
    let fixture = setup().await;
    let ctx = fixture.ctx_a();
    let tpl = seed_template(
        &fixture,
        &ctx,
        "snap",
        "in_app",
        "zh-CN",
        category::TRANSACTIONAL,
    )
    .await;

    let mut payload = std::collections::BTreeMap::new();
    payload.insert("name".to_string(), "张三".to_string());
    let outcome = fixture
        .service
        .enqueue(
            &ctx,
            &platform_notify::service::EnqueueRequest {
                template_code: "snap".to_string(),
                channel: "in_app".to_string(),
                recipient: "u1".to_string(),
                locale: Some("zh-CN".to_string()),
                payload,
            },
        )
        .await
        .expect("入队");

    let before = outcome.delivery.rendered_body.clone();
    assert!(before.contains("张三"), "渲染结果应含变量值：{before}");

    // 改模板正文成完全不同的文案。
    fixture
        .service
        .update_template(
            &ctx,
            &tpl.id,
            &TemplateUpdate {
                title_template: None,
                body_template: Some("完全不同的文案".to_string()),
                category: None,
            },
        )
        .await
        .expect("改模板");

    // 已入队那条的 rendered_body **仍是旧文案**。
    let after = delivery_field(&fixture, &outcome.delivery.id, "rendered_body")
        .await
        .expect("应有 rendered_body");
    assert_eq!(after, before, "rendered_body 是快照，模板改了不得追溯");
    assert!(
        !after.contains("完全不同的文案"),
        "不得被新模板覆盖：{after}"
    );
}

/// **AC-S-173 步骤 3/8-③**：投递记录的 RLS 真的挡住跨租户（**3 张表各验一次**）。
///
/// ⚠️ 这是 D311 / S-04 的核心判据：原先只有 delivery 一张表有判据。
#[tokio::test]
async fn all_three_tables_are_protected_by_rls() {
    let fixture = setup().await;

    // 两个租户各造一行（管理员连接绕过 RLS）。
    seed_delivery(&fixture, TENANT_A, "pending", 0, "now()").await;
    seed_delivery(&fixture, TENANT_B, "pending", 0, "now()").await;
    for tenant in [TENANT_A, TENANT_B] {
        fixture
            .admin
            .execute(&format!(
                "INSERT INTO platform_notify_template \
                   (id, tenant_id, code, channel, locale, category, title_template, body_template) \
                 VALUES (gen_random_uuid(), '{tenant}', 'c-{tenant}', 'in_app', 'zh-CN', \
                         'system', 't', 'b')"
            ))
            .await;
        fixture
            .admin
            .execute(&format!(
                "INSERT INTO platform_notify_preference \
                   (tenant_id, user_id, category, opted_out) \
                 VALUES ('{tenant}', 'u1', 'marketing', false)"
            ))
            .await;
    }

    // 应用角色连的是同一个库 —— 用 `service` 的 store 读（**RLS 生效**）。
    let deliveries_a = fixture.service.list_deliveries(&fixture.ctx_a()).await.expect("读 delivery");
    assert_eq!(deliveries_a.len(), 1, "tenant-a 只应看到自己的 1 条投递记录");
    assert!(
        deliveries_a.iter().all(|row| row.tenant_id == TENANT_A),
        "tenantId 必须全部等于 tenant-a"
    );

    let templates_a = fixture.service.list_templates(&fixture.ctx_a()).await.expect("读 template");
    assert_eq!(templates_a.len(), 1, "tenant-a 只应看到自己的 1 个模板");
    assert!(
        templates_a.iter().all(|row| row.tenant_id == TENANT_A),
        "template 表的 RLS 必须生效"
    );

    let prefs_a = fixture
        .service
        .list_preferences(&fixture.ctx_a(), "u1")
        .await
        .expect("读 preference");
    assert_eq!(prefs_a.len(), 1, "tenant-a 只应看到自己的 1 条偏好");
    assert!(
        prefs_a.iter().all(|row| row.tenant_id == TENANT_A),
        "preference 表的 RLS 必须生效"
    );
}

/// **AC-S-173 步骤 7**：投递表列集合**恰好 18 列**，且 `payload` 是 `jsonb`。
///
/// ⚠️ 判据写成「与显式列出的 18 元素集合相等」，**不是**「列数 = 18」
/// （列数对但列错，那种断言会放过改名列）。
#[tokio::test]
async fn the_delivery_table_has_exactly_the_eighteen_documented_columns() {
    let fixture = setup().await;
    let rows = fixture
        .admin
        .query_text(
            "SELECT string_agg(column_name, ',' ORDER BY column_name)::text \
             FROM information_schema.columns WHERE table_name = 'platform_notify_delivery'",
        )
        .await;
    let actual = rows.first().expect("应能取到列清单").clone();

    let mut actual_columns: Vec<&str> = actual.split(',').collect();
    let mut expected: Vec<&str> = platform_notify::model::DELIVERY_COLUMNS.to_vec();
    actual_columns.sort_unstable();
    expected.sort_unstable();
    assert_eq!(
        actual_columns, expected,
        "列集合必须与 DELIVERY_COLUMNS **双向相等**（不是只比数量）"
    );

    // `payload` 必须是 jsonb。
    let types = fixture
        .admin
        .query_text(
            "SELECT data_type::text FROM information_schema.columns \
             WHERE table_name = 'platform_notify_delivery' AND column_name = 'payload'",
        )
        .await;
    assert_eq!(
        types.first().map(String::as_str),
        Some("jsonb"),
        "payload 的类型必须是 jsonb"
    );
}

/// **AC-S-173 步骤 4/5**：敏感键拒收；`secretary` 不误伤（阳性对照）。
#[tokio::test]
async fn sensitive_payload_keys_are_rejected() {
    let fixture = setup().await;
    let ctx = fixture.ctx_a();
    seed_template(&fixture, &ctx, "sec", "in_app", "zh-CN", category::TRANSACTIONAL).await;

    for key in ["password", "clientSecret", "apiKey", "authorization"] {
        let mut payload = std::collections::BTreeMap::new();
        // 模板正文要 `{{name}}` —— 必须先让渲染能过，才验得到敏感键这一层。
        payload.insert("name".to_string(), "张三".to_string());
        payload.insert(key.to_string(), "x".to_string());
        let err = fixture
            .service
            .enqueue(
                &ctx,
                &platform_notify::service::EnqueueRequest {
                    template_code: "sec".to_string(),
                    channel: "in_app".to_string(),
                    recipient: "u1".to_string(),
                    locale: Some("zh-CN".to_string()),
                    payload,
                },
            )
            .await
            .expect_err(&format!("{key} 必须被拒"));
        assert_eq!(err.code(), "NOTIFY_SENSITIVE_PAYLOAD", "键 {key}");
    }

    // 阳性对照：secretary 不命中。
    let mut innocent = std::collections::BTreeMap::new();
    innocent.insert("name".to_string(), "张三".to_string());
    innocent.insert("secretary".to_string(), "x".to_string());
    fixture
        .service
        .enqueue(
            &ctx,
            &platform_notify::service::EnqueueRequest {
                template_code: "sec".to_string(),
                channel: "in_app".to_string(),
                recipient: "u1".to_string(),
                locale: Some("zh-CN".to_string()),
                payload: innocent,
            },
        )
        .await
        .expect("secretary 不含敏感键，不得误伤");
}
