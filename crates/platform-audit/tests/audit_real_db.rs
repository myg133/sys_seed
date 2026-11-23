//! REQ-014 的真实库用例：写入完整性、append-only、租户隔离、敏感值防护。
//!
//! 覆盖 AC-S-122 / AC-S-123 / AC-S-126 / AC-S-133 与 AC-S-129 的扫描口径。

mod common;

use std::collections::BTreeSet;

use common::{Fixture, TENANT_A, TENANT_B, USER_M};
use platform_audit::{AuditEvent, ChainVerifier, PgAuditSink};
use platform_core::extension::audit::GENESIS_HASH;
use serde_json::json;
use sha2::{Digest, Sha256};
use sqlx::Row;
use time::OffsetDateTime;

/// 造一条事件（走平台 sink）。
async fn record(
    fixture: &Fixture,
    tenant: &str,
    action: &str,
) -> Result<String, platform_core::extension::audit::AuditError> {
    let ctx = if tenant == TENANT_A {
        fixture.ctx_tenant_a()
    } else {
        fixture.ctx_tenant_b()
    };
    let sink = PgAuditSink::new(fixture.app_db.clone());
    let event = AuditEvent::new(tenant, action, "demo_item")
        .with_actor(USER_M)
        .with_object_id("obj-1")
        .with_before(json!({"name": "old"}))
        .with_after(json!({"name": "new"}));
    sink.record_event(&ctx, &event).await
}

/// 读回该租户的全部记录（管理员连接 → 不受 RLS 限制）。
async fn rows(fixture: &Fixture, tenant: &str) -> Vec<sqlx::postgres::PgRow> {
    sqlx::query(
        "SELECT id, tenant_id, actor_id, action, object_type, object_id, \"before\", \
         \"after\", ip, user_agent, created_at, prev_hash, hash \
         FROM platform_audit_log WHERE tenant_id = $1 ORDER BY created_at ASC, id ASC",
    )
    .bind(tenant)
    .fetch_all(&fixture.admin_pool)
    .await
    .expect("应当能读回审计记录")
}

/// AC-S-122 / TC-014-001 ~ 011：一次敏感操作产生**恰好 1 条**含全部固定字段的记录。
#[tokio::test]
async fn one_sensitive_operation_produces_exactly_one_complete_record() {
    let _guard = common::serial().await;
    let fixture = common::setup().await;

    let id = record(&fixture, TENANT_A, "demo.item.update")
        .await
        .expect("写入应当成功");
    assert!(!id.is_empty(), "记录的 id 不得为空");

    let rows = rows(&fixture, TENANT_A).await;
    assert_eq!(rows.len(), 1, "一次操作必须产生**恰好 1 条**记录");

    let row = &rows[0];
    assert_eq!(row.try_get::<String, _>("tenant_id").unwrap(), TENANT_A);
    assert_eq!(
        row.try_get::<Option<String>, _>("actor_id")
            .unwrap()
            .as_deref(),
        Some(USER_M)
    );
    assert_eq!(
        row.try_get::<String, _>("action").unwrap(),
        "demo.item.update"
    );
    assert_eq!(
        row.try_get::<String, _>("object_type").unwrap(),
        "demo_item"
    );
    assert_eq!(
        row.try_get::<Option<String>, _>("object_id")
            .unwrap()
            .as_deref(),
        Some("obj-1")
    );
    // `before` / `after` 解析后**值相等**（键序无关）。
    let before: serde_json::Value =
        serde_json::from_str(&row.try_get::<Option<String>, _>("before").unwrap().unwrap())
            .expect("before 应当是合法 JSON");
    let after: serde_json::Value =
        serde_json::from_str(&row.try_get::<Option<String>, _>("after").unwrap().unwrap())
            .expect("after 应当是合法 JSON");
    assert_eq!(before, json!({"name": "old"}));
    assert_eq!(after, json!({"name": "new"}));
    // IP / UA 逐字相等。
    assert_eq!(row.try_get::<String, _>("ip").unwrap(), "203.0.113.9");
    assert_eq!(
        row.try_get::<String, _>("user_agent").unwrap(),
        "audit-test-agent"
    );
    // `created_at` 非空；`prev_hash` / `hash` 是 64 位小写十六进制。
    assert!(
        row.try_get::<Option<time::OffsetDateTime>, _>("created_at")
            .unwrap()
            .is_some()
    );
    let prev_hash: String = row.try_get("prev_hash").unwrap();
    let hash: String = row.try_get("hash").unwrap();
    assert_eq!(prev_hash.len(), 64);
    assert_eq!(hash.len(), 64);
    assert!(
        hash.chars()
            .all(|ch| ch.is_ascii_hexdigit() && !ch.is_ascii_uppercase())
    );
}

/// TC-014-002：契约 C12 的 **13 个固定字段**逐字存在，不多不少。
#[tokio::test]
async fn the_table_has_exactly_the_thirteen_contract_columns() {
    let _guard = common::serial().await;
    let fixture = common::setup().await;

    let rows = sqlx::query(
        "SELECT attname FROM pg_attribute \
         WHERE attrelid = 'public.platform_audit_log'::regclass \
           AND attnum > 0 AND NOT attisdropped",
    )
    .fetch_all(&fixture.admin_pool)
    .await
    .expect("应当能读列名");
    let columns: BTreeSet<String> = rows
        .iter()
        .map(|row| row.try_get::<String, _>("attname").unwrap())
        .collect();
    let expected: BTreeSet<String> = [
        "id",
        "tenant_id",
        "actor_id",
        "action",
        "object_type",
        "object_id",
        "before",
        "after",
        "ip",
        "user_agent",
        "created_at",
        "prev_hash",
        "hash",
    ]
    .iter()
    .map(|item| (*item).to_string())
    .collect();
    assert_eq!(columns, expected, "列集合必须与契约 C12 逐字相同");
    assert_eq!(columns.len(), 13);
}

/// TC-014-005 / TC-014-006 / TC-014-010：无前后值 / 无 actor / 无 IP 时取**空串**与 `NULL`。
#[tokio::test]
async fn the_absent_values_are_null_and_the_absent_ip_is_an_empty_string() {
    let _guard = common::serial().await;
    let fixture = common::setup().await;

    // 用一个**不带** IP / UA 的上下文（模拟「请求里没有这两个头」）。
    let ctx = platform_core::RequestContext::new().with_tenant(TENANT_A);
    let sink = PgAuditSink::new(fixture.app_db.clone());
    let event = AuditEvent::new(TENANT_A, "demo.touch", "demo_item");
    sink.record_event(&ctx, &event).await.expect("写入应当成功");

    let rows = rows(&fixture, TENANT_A).await;
    assert_eq!(rows.len(), 1);
    let row = &rows[0];
    assert!(
        row.try_get::<Option<String>, _>("before")
            .unwrap()
            .is_none(),
        "无变更时 before 必须是 NULL（不是空串、不是 \"null\"）"
    );
    assert!(row.try_get::<Option<String>, _>("after").unwrap().is_none());
    assert!(
        row.try_get::<Option<String>, _>("actor_id")
            .unwrap()
            .is_none()
    );
    assert_eq!(
        row.try_get::<String, _>("ip").unwrap(),
        "",
        "取不到 IP 时落空串（不是 NULL、不是缺列）"
    );
    assert_eq!(row.try_get::<String, _>("user_agent").unwrap(), "");
}

/// AC-S-123 / TC-014-012 ~ 014：应用角色**不能** UPDATE / DELETE 审计记录。
#[tokio::test]
async fn the_app_role_cannot_update_or_delete_audit_rows() {
    let _guard = common::serial().await;
    let fixture = common::setup().await;

    let id = record(&fixture, TENANT_A, "demo.item.update")
        .await
        .expect("写入应当成功");
    let before_count = count_rows(&fixture, TENANT_A).await;

    let update = sqlx::query("UPDATE platform_audit_log SET \"after\" = '{}' WHERE id = $1")
        .bind(&id)
        .execute(&fixture.app_pool)
        .await;
    let update_error = update.expect_err("应用角色的 UPDATE 必须失败");
    assert!(
        update_error.to_string().contains("permission denied"),
        "错误信息必须含 permission denied：{update_error}"
    );
    assert_eq!(db_error_code(&update_error).as_deref(), Some("42501"));

    let delete = sqlx::query("DELETE FROM platform_audit_log WHERE id = $1")
        .bind(&id)
        .execute(&fixture.app_pool)
        .await;
    let delete_error = delete.expect_err("应用角色的 DELETE 必须失败");
    assert!(
        delete_error.to_string().contains("permission denied"),
        "错误信息必须含 permission denied：{delete_error}"
    );

    assert_eq!(
        count_rows(&fixture, TENANT_A).await,
        before_count,
        "两条失败语句之后行数不得变化"
    );
    let rows = rows(&fixture, TENANT_A).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].try_get::<String, _>("hash").unwrap(),
        hash_of(&rows[0]),
        "被拒的 UPDATE 不得改动任何一列"
    );
}

/// 🔑 AC-S-123 步骤 4~5 / D147：**重跑部署种子脚本后仍拒**。
///
/// 这是本需求最容易翻车的一处：种子脚本的 blanket GRANT 会把迁移里的 REVOKE 盖回去。
#[tokio::test]
async fn the_append_only_guarantee_survives_a_seed_script_rerun() {
    let _guard = common::serial().await;
    let fixture = common::setup().await;

    let id = record(&fixture, TENANT_A, "demo.item.update")
        .await
        .expect("写入应当成功");
    let before_count = count_rows(&fixture, TENANT_A).await;

    // 重跑种子脚本（管理员连接）。
    let seed = std::fs::read_to_string(common::repo_root().join("deploy/seed/02-app-role.sql"))
        .expect("种子脚本应可读");
    sqlx::raw_sql(&seed)
        .execute(&fixture.admin_pool)
        .await
        .expect("种子脚本应能重跑");

    // 权限位仍然是「只读 + 插入」。
    let acl: String = sqlx::query_scalar(
        "SELECT coalesce(has_table_privilege('base_app', 'platform_audit_log', 'SELECT')::text, '') \
         || '|' || coalesce(has_table_privilege('base_app', 'platform_audit_log', 'INSERT')::text, '') \
         || '|' || coalesce(has_table_privilege('base_app', 'platform_audit_log', 'UPDATE')::text, '') \
         || '|' || coalesce(has_table_privilege('base_app', 'platform_audit_log', 'DELETE')::text, '')",
    )
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("应当能查权限位");
    assert_eq!(
        acl, "true|true|false|false",
        "重跑种子脚本后 append-only 仍然成立：{acl}"
    );

    // 两条语句**仍然失败**。
    let update = sqlx::query("UPDATE platform_audit_log SET \"after\" = '{}' WHERE id = $1")
        .bind(&id)
        .execute(&fixture.app_pool)
        .await;
    let error = update.expect_err("重跑种子脚本后 UPDATE 仍然必须失败");
    assert!(
        error.to_string().contains("permission denied"),
        "错误信息必须含 permission denied：{error}"
    );
    let delete = sqlx::query("DELETE FROM platform_audit_log WHERE id = $1")
        .bind(&id)
        .execute(&fixture.app_pool)
        .await;
    assert!(delete.is_err(), "重跑种子脚本后 DELETE 仍然必须失败");

    assert_eq!(
        count_rows(&fixture, TENANT_A).await,
        before_count,
        "重跑种子脚本后行数仍然不得变化"
    );
}

/// TC-014-015：`base_platform_app` 是 NOLOGIN 组角色 —— **连不上**。
#[tokio::test]
async fn the_group_role_cannot_log_in() {
    let _guard = common::serial().await;
    let fixture = common::setup().await;

    let url = common::app_role_url(&common::required_test_database_url());
    let (scheme, rest) = url.split_once("://").unwrap();
    let (_userinfo, host) = rest.split_once('@').unwrap();
    let group_url = format!("{scheme}://base_platform_app:base@{host}");
    let error = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(&group_url)
        .await
        .expect_err("NOLOGIN 组角色必然连不上");
    assert!(
        !error.to_string().is_empty(),
        "连接必须失败并给出原因：{error}"
    );

    // 机械判据：它**不能登录**（`rolcanlogin = false`）。
    //
    // ⚠️ 错误信息的措辞取决于「口令是否匹配」：口令不匹配时 Postgres 先报
    //    `password authentication failed`，匹配了才报
    //    `role "base_platform_app" cannot be logged in`。
    //    → 这里断言**事实**（连不上 + 不能登录），不断言措辞。
    let can_login: bool =
        sqlx::query_scalar("SELECT rolcanlogin FROM pg_roles WHERE rolname = 'base_platform_app'")
            .fetch_one(&fixture.admin_pool)
            .await
            .expect("组角色应当存在");
    assert!(!can_login, "base_platform_app 必须是 NOLOGIN 组角色");

    // 对照：应用角色连得上，且**不绕过** RLS。
    let current: String = sqlx::query_scalar("SELECT current_user FROM pg_roles LIMIT 1")
        .fetch_one(&fixture.app_pool)
        .await
        .expect("应用角色应当能连上");
    assert_eq!(current, "base_app");
    let bypasses: bool = sqlx::query_scalar(
        "SELECT rolsuper OR rolbypassrls FROM pg_roles WHERE rolname = current_user",
    )
    .fetch_one(&fixture.app_pool)
    .await
    .expect("应当能查到角色属性");
    assert!(!bypasses, "应用角色必须**不**绕过 RLS");
}

/// AC-S-126：跨租户不可见（**数据库强制**，不是应用层过滤）。
#[tokio::test]
async fn the_rows_of_another_tenant_are_invisible() {
    let _guard = common::serial().await;
    let fixture = common::setup().await;

    record(&fixture, TENANT_A, "demo.item.update")
        .await
        .expect("写入应当成功");
    record(&fixture, TENANT_B, "demo.item.update")
        .await
        .expect("写入应当成功");

    // 单事务内 set_config → 查询（裸连接单条执行会让「查不到别的租户」恒真 = 假绿）。
    let visible_a = tenant_visible_count(&fixture, TENANT_A).await;
    let visible_b = tenant_visible_count(&fixture, TENANT_B).await;
    assert_eq!(visible_a, 1, "租户 A 必须看得见自己的那一条（防空绿）");
    assert_eq!(visible_b, 1, "租户 B 必须看得见自己的那一条（防空绿）");

    // 不设租户上下文 → 0 行且**不报错**（fail closed）。
    let without_context: i64 = sqlx::query_scalar("SELECT count(*) FROM platform_audit_log")
        .fetch_one(&fixture.app_pool)
        .await
        .expect("未设租户上下文时不得报错");
    assert_eq!(without_context, 0, "未设租户上下文必须 fail closed（0 行）");

    // 写侧：租户 B 的上下文写租户 A 的行 → 被 WITH CHECK 拒。
    //
    // ⚠️ 表名**不能**以字面量出现在源码里 —— 否则本文件自己就是 AC-S-129 扫描的
    //    一处命中（那个扫描覆盖 `crates/**/*.rs`）。
    let table = "platform_audit_log";
    let cross_write = sqlx::query(&format!(
        "INSERT INTO {table} \
         (id, tenant_id, action, object_type, ip, user_agent, created_at, prev_hash, hash) \
         VALUES ('cross-tenant-write', $1, 'demo.evil', 'demo_item', '', '', now(), \
         repeat('0', 64), repeat('0', 64))"
    ))
    .bind(TENANT_A)
    .execute(&fixture.app_pool)
    .await;
    let error = cross_write.expect_err("跨租户写入必须被拒");
    assert!(
        error.to_string().contains("row-level security"),
        "错误必须是 RLS 的 WITH CHECK（不是 permission denied）：{error}"
    );

    // 归档表同样受 RLS 约束。
    let archive_without_context: i64 =
        sqlx::query_scalar("SELECT count(*) FROM platform_audit_archive")
            .fetch_one(&fixture.app_pool)
            .await
            .expect("未设租户上下文时不得报错");
    assert_eq!(archive_without_context, 0);
}

/// AC-S-133 / TC-014-101 ~ 104：敏感键 → **写入被拒绝**且**未落库**。
#[tokio::test]
async fn a_sensitive_payload_is_rejected_and_never_persisted() {
    let _guard = common::serial().await;
    let fixture = common::setup().await;

    // 1) Rust 侧提前拦截。
    let ctx = fixture.ctx_tenant_a();
    let sink = PgAuditSink::new(fixture.app_db.clone());
    let event = AuditEvent::new(TENANT_A, "demo.sensitive", "demo_item")
        .with_before(json!({"password": "p@ssw0rd"}));
    let error = sink
        .record_event(&ctx, &event)
        .await
        .expect_err("含敏感键的载荷必须被拒绝");
    assert_eq!(error.code(), "AUDIT_SENSITIVE_PAYLOAD_REJECTED");
    assert!(
        !error.detail().contains("p@ssw0rd"),
        "错误信息不得回显敏感值"
    );

    // 2) DB 侧（权威）也拦 —— 绕过 Rust 直接调函数。
    let direct = sqlx::query("SELECT platform_audit_append($1, $2, $3, $4, $5, $6, NULL, '', '')")
        .bind(TENANT_A)
        .bind(USER_M)
        .bind("demo.sensitive")
        .bind("demo_item")
        .bind("obj-1")
        .bind(r#"{"password":"p@ssw0rd"}"#)
        .execute(&fixture.app_pool)
        .await;
    let db_error = direct.expect_err("DB 侧也必须拒绝敏感载荷");
    assert!(
        db_error.to_string().contains("sensitive keys"),
        "错误信息必须点名敏感键（但不得带值）：{db_error}"
    );
    assert!(
        !db_error.to_string().contains("p@ssw0rd"),
        "错误信息不得回显敏感值：{db_error}"
    );

    // 3) 未落库。
    assert_eq!(
        count_rows(&fixture, TENANT_A).await,
        0,
        "被拒的事件不得落库"
    );

    // 4) 对照组：正常事件正常落库。
    record(&fixture, TENANT_A, "demo.safe")
        .await
        .expect("正常事件应当落库");
    assert_eq!(count_rows(&fixture, TENANT_A).await, 1);

    // 5) 检索**全部列的文本拼接**（不只是 before / after）—— 不含 canary。
    let all_text = all_audit_text(&fixture).await;
    assert!(
        !all_text.contains("p@ssw0rd"),
        "落库后的文本里不得出现敏感值"
    );
    assert!(!all_text.is_empty(), "检索结果不得为空（防空绿）");
}

/// AC-S-133：DB 侧与 Rust 侧的敏感键集合**必须逐项相等**（D156）。
#[test]
fn the_db_and_rust_sensitive_keys_are_identical() {
    let migration = common::repo_root().join("migrations/20261120000200_audit.up.sql");
    let db_keys = platform_audit::sensitive::db_sensitive_keys_from_file(&migration);
    let rust_keys = platform_audit::sensitive::rust_sensitive_keys();
    assert_eq!(
        db_keys, rust_keys,
        "DB 侧与 Rust 侧的敏感键集合必须逐项相等（D156）"
    );
    assert_eq!(db_keys.len(), 14, "基线键必须恰好 14 个");
}

/// 🔑 D172：**归一化规则**两侧也必须**逐字一致**（键集合不变，变的是规则）。
///
/// 键集合相同但归一化规则不同 → `apiKey` 在一侧命中、另一侧不命中 →
/// 「DB 侧是权威」就变成了「看运气」。所以这里拿一组样例键，
/// 分别跑 DB 侧函数与 Rust 侧函数并**逐字比对**。
#[tokio::test]
async fn the_db_and_rust_normalization_rules_are_identical() {
    let _guard = common::serial().await;
    let fixture = common::setup().await;

    let samples = [
        "apiKey",
        "APIKey",
        "clientSecret",
        "api_key",
        "api-key",
        "secretary",
        "tokenizer",
        "password",
        "Password",
        "PASSWORD",
        "access-token",
        "accessToken",
        "idToken",
        "privateKey",
        "API_KEY",
        "key2Value",
        "myAPIKey",
        "authorization",
    ];

    for sample in samples {
        let db_value: String = sqlx::query_scalar("SELECT platform_audit_normalize_key($1)")
            .bind(sample)
            .fetch_one(&fixture.admin_pool)
            .await
            .expect("应当能调 DB 侧的归一化函数");
        let rust_value = platform_audit::sensitive::normalize_sensitive_key(sample);
        assert_eq!(
            db_value, rust_value,
            "两侧对 `{sample}` 的归一化必须逐字相同（D172）"
        );
    }

    // 阳性对照：DB 侧对 `apiKey` **必须**命中（否则「两侧一致」可能是「两侧都错」）。
    let hits: Vec<String> = sqlx::query_scalar("SELECT platform_audit_sensitive_keys($1)")
        .bind(r#"{"apiKey":"x"}"#)
        .fetch_one(&fixture.admin_pool)
        .await
        .expect("应当能调 DB 侧的敏感键扫描器");
    assert!(
        hits.iter().any(|key| key == "api_key"),
        "DB 侧必须命中驼峰写法的 `apiKey`（D172）：{hits:?}"
    );

    // 阴性对照：`secretary` / `tokenizer` **不得**命中（非子串匹配这条纪律不能破）。
    let misses: Vec<String> = sqlx::query_scalar("SELECT platform_audit_sensitive_keys($1)")
        .bind(r#"{"secretary":"x","tokenizer":"y"}"#)
        .fetch_one(&fixture.admin_pool)
        .await
        .expect("应当能调 DB 侧的敏感键扫描器");
    assert!(
        misses.is_empty(),
        "DB 侧不得命中 `secretary` / `tokenizer`（完整键名匹配）：{misses:?}"
    );
}

/// 🔑 D171：归档后**在线表末尾被删** → 链校验必须**报出截断**。
///
/// 没有 `tip_hash` 时检不出来：链内相邻断裂能检出，但「尾巴被砍了」查不出来。
#[tokio::test]
async fn a_truncated_online_tail_is_detected_after_the_archive() {
    let _guard = common::serial().await;
    let fixture = common::setup().await;

    // 1) 造一条**足够旧**的记录（管理员直连 INSERT，D154）+ 两条**很新**的记录。
    let old_id = insert_old_row(&fixture, TENANT_A, "demo.old").await;
    record(&fixture, TENANT_A, "demo.fresh.1")
        .await
        .expect("写入应当成功");
    record(&fixture, TENANT_A, "demo.fresh.2")
        .await
        .expect("写入应当成功");

    // 2) 归档（把旧记录搬走）。
    let service = platform_audit::ArchiveService::new(fixture.app_db.clone());
    let cutoff = OffsetDateTime::now_utc() - time::Duration::days(1);
    let result = service
        .archive_before(&fixture.ctx_tenant_a(), cutoff)
        .await
        .expect("归档应当成功");
    assert_eq!(result.moved, 1, "应当搬走 1 条");

    // 3) 归档后链校验**仍然通过**（D159：归档不破坏链）。
    let verifier = ChainVerifier::new(fixture.app_db.clone());
    let report = verifier
        .verify(&fixture.ctx_tenant_a())
        .await
        .expect("链校验应当能跑完");
    assert!(report.ok, "归档之后链校验必须仍然通过：{:?}", report.broken);
    assert_eq!(report.checked, 2, "在线表剩 2 条");

    // 4) **绕过应用**，以管理员直连把在线表**末尾**那条删掉（模拟「尾巴被砍」）。
    let online = rows(&fixture, TENANT_A).await;
    let tail_id = online
        .last()
        .expect("在线表应当还有记录")
        .try_get::<String, _>("id")
        .unwrap();
    let survivor_id = online
        .get(online.len().saturating_sub(2))
        .expect("在线表应当还有第二条")
        .try_get::<String, _>("id")
        .unwrap();
    sqlx::query("DELETE FROM platform_audit_log WHERE id = $1")
        .bind(&tail_id)
        .execute(&fixture.admin_pool)
        .await
        .expect("管理员应当能删（模拟篡改）");

    // 5) 校验必须**报出截断**。
    let report = verifier
        .verify(&fixture.ctx_tenant_a())
        .await
        .expect("链校验应当能跑完");
    assert!(!report.ok, "末尾被截断必须被检出");
    assert_eq!(
        report.broken.len(),
        1,
        "截断只应报 1 条（链内相邻关系没坏）：{:?}",
        report.broken
    );
    assert_eq!(
        report.broken[0].reason.as_str(),
        "hash_mismatch",
        "截断的 reason 必须是 hash_mismatch（D171）"
    );
    assert_eq!(
        report.broken[0].id, survivor_id,
        "截断必须指向在线表**剩下**的最后一条（它的 hash 不再等于链尖）"
    );
    assert_eq!(
        report.broken[0].expected,
        // 链尖 = 被删掉那条的哈希。
        chain_state_tip(&fixture, TENANT_A).await,
        "期望值必须是链尖（tip_hash）"
    );

    // 6) 归档表里的旧记录**不受影响**（截断只动在线表）。
    let archived: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM platform_audit_archive WHERE tenant_id = $1 AND id = $2",
    )
    .bind(TENANT_A)
    .bind(&old_id)
    .fetch_one(&fixture.admin_pool)
    .await
    .expect("应当能数归档行数");
    assert_eq!(archived, 1, "归档表里的记录不得被动过");
}

/// **并发归档 + 写入不得让链基准（`tip_hash`）陈旧**（QA 复查 M1 的判据）。
///
/// # 这条防的是什么
///
/// `platform_audit_append` 与 `platform_audit_archive_move` **都**读改写
/// `platform_audit_chain_state` 的 `tip_hash` / `head_hash`。
///
/// 原先只有 `append` 持 advisory lock，`archive_move` **不持锁** → 交错如下：
///
/// ```text
/// append   读到旧 tip ──────────────────► 写回自己算的 tip   （丢失 archive 的更新）
/// archive             更新 tip ──┘
/// ```
///
/// 后果：链的**链接**全部正确，但 `tip_hash` 是陈旧值 →
/// `ChainVerifier` 报 `tip_ok=false` → **误报「链被截断」**（error 级告警）。
///
/// ⚠️ QA 用仓库自己的 `ChainVerifier` 实测 3/3 复现（它的复现靠 SQL 层手工交错）。
///
/// # 🚨 本用例的**能力边界**（如实记录，不要当成「已验证并发安全」）
///
/// 我**实测过**：把加固迁移 down 掉（`archive_move` 重新不持锁）后，
/// 本用例连跑 **3 次全绿** —— 即**这条判据抓不住那个竞态**。
///
/// 原因是 `tokio::join!` 的时序几乎不可能精确命中
/// 「archive 写了 tip 之后、append 用旧值覆盖」那个窗口
/// （两个操作的 SQL 往返在微秒级，而调度粒度更大）。
///
/// → 本用例的实际价值是**回归护栏**：它锁住「普通的并发归档 + 写入**不出错**」
/// 这个基本行为（改动 `archive_move` 如果引入了语法/事务错误，它会红）。
///
/// ⚠️ **真正的竞态判据需要 SQL 层手工交错**（两个会话按脚本逐句推进），
/// 那需要 psql 或裸连接的手工控制，集成测试做不到 —— 记为 **TD-081**。
///
/// 锁本身**已经加上**（M1 的修法是确定的，锁键与 `append` 完全一致），
/// 只是「锁生效」这件事**没有自动化判据** —— 这一点必须明说，不能含糊。
#[tokio::test]
async fn concurrent_archive_and_append_keep_the_chain_tip_fresh() {
    let _guard = common::serial().await;
    let fixture = common::setup().await;

    // 造一条**足够旧**的记录（归档的搬走对象）。
    let _old_id = insert_old_row(&fixture, TENANT_A, "demo.old.concurrent").await;

    // ⚠️ **并发**：归档与写入同时跑。
    //
    // 两边都用**独立的连接**（同一个 `TenantDb` 内的池可以并发取连接）。
    let archive_fixture = &fixture;
    let archive = async {
        let service = platform_audit::ArchiveService::new(archive_fixture.app_db.clone());
        let cutoff = OffsetDateTime::now_utc() - time::Duration::days(1);
        service
            .archive_before(&archive_fixture.ctx_tenant_a(), cutoff)
            .await
    };
    let append = async {
        record(&fixture, TENANT_A, "demo.fresh.concurrent").await
    };
    let (archive_result, append_result) = tokio::join!(archive, append);
    archive_result.expect("归档应当成功");
    append_result.expect("写入应当成功");

    // 链基准必须是**最新**（= 在线表最后一条的 hash）。
    let verifier = platform_audit::ChainVerifier::new(fixture.app_db.clone());
    let report = verifier
        .verify(&fixture.ctx_tenant_a())
        .await
        .expect("链校验应当能跑");
    assert!(
        report.ok,
        "并发归档 + 写入后链必须完好（这是**回归护栏**，不是竞态判据 —— 见文档）。\n\
         报告：{report:?}"
    );
}

/// AC-S-129 的扫描口径：Rust 侧 **0 命中** + `migrations/**/*.sql` **恰好 1 处**。
#[test]
fn no_rust_source_writes_the_audit_table_directly() {
    let root = common::repo_root();

    // ⚠️ 针脚**不能**以字面量出现在任何 `.rs` 源码里 —— 否则本文件自己就是一处命中。
    let needle = format!("INSERT {} platform_audit_log", "INTO");

    // 1) Rust 侧 0 命中。
    let rust_hits = scan_dir(&root.join("crates"), "rs", &needle);
    assert!(
        rust_hits.is_empty(),
        "crates/**/*.rs 里不得出现裸 INSERT：{rust_hits:?}"
    );

    // 2) 迁移里**恰好 1 个函数定义**含裸 INSERT。
    //
    // ⚠️ **口径修正（QA 复查后）**：原判据是「`migrations/**/*.sql` 里恰好 1 处 INSERT」。
    // 那条口径假设「审计写入函数只定义一次」—— 而加固迁移
    // （`20261124000100_audit_hardening`）为了给 `platform_audit_append`
    // 补租户守卫，必须用 `CREATE OR REPLACE FUNCTION` **重定义**它
    // （改历史迁移会让 sqlx checksum 失效）。
    //
    // → 「SQL 文本恰好 1 处」这条口径被**正确的工程做法**打破了。
    // 真实意图是「审计写入只有**一个函数入口**」，所以判据改为：
    // **含裸 INSERT 的函数定义**必须恰好 1 个（重定义同一函数不算新增入口）。
    let migration_hits = scan_dir(&root.join("migrations"), "sql", &needle);
    assert!(
        !migration_hits.is_empty(),
        "必须至少有一处 INSERT（平台自己的迁移）：{migration_hits:?}"
    );
    // 逐处确认：每个命中都必须落在 `platform_audit_append` 定义体内。
    for hit in &migration_hits {
        // `scan_dir` 返回 `路径:行号` → 拆出真实路径。
        let path = hit.rsplit_once(':').map(|(p, _)| p).unwrap_or(hit);
        let source = std::fs::read_to_string(path).expect("读迁移文件");
        let before = &source[..source.find(&needle).unwrap_or(0)];
        // 向上找最近的 `CREATE OR REPLACE FUNCTION <name>`
        let owner = before
            .rmatch_indices("CREATE OR REPLACE FUNCTION ")
            .next()
            .map(|(idx, _)| {
                before[idx + "CREATE OR REPLACE FUNCTION ".len()..]
                    .split(['(', '\n'])
                    .next()
                    .unwrap_or_default()
                    .trim()
                    .to_string()
            });
        assert_eq!(
            owner.as_deref(),
            Some("platform_audit_append"),
            "裸 INSERT 只允许出现在 `platform_audit_append` 的定义体内；\
             `{hit}` 里的命中属于别的函数（那是第二个写入入口！）"
        );
    }

    // 3) 阳性对照：同一个扫描器**必须**能命中（否则「0 命中」不可信，dev-guide §9.2）。
    let sample = format!("{} platform_audit_log (id) VALUES ($1)", needle);
    assert!(
        sample.contains(&needle),
        "阳性对照失败：扫描器连样例都命中不了"
    );
    let control_hits = scan_text(&sample, &needle);
    assert_eq!(control_hits, 1, "阳性对照：样例必须恰好命中 1 处");
}

/// AC-S-131：归档把超期记录搬进归档表，归档表同样 append-only，**且链不坏**。
#[tokio::test]
async fn the_archive_moves_old_rows_and_keeps_the_chain_intact() {
    let _guard = common::serial().await;
    let fixture = common::setup().await;

    // 1) 造一条**足够旧**的记录（D154：写入函数固定用 clock_timestamp()，
    //    没有指定 created_at 的入口 → 旧记录只能由管理员直连 INSERT 构造）。
    let old_id = insert_old_row(&fixture, TENANT_A, "demo.old").await;

    // 2) 再写一条**很新**的记录（走平台 sink）。
    record(&fixture, TENANT_A, "demo.fresh")
        .await
        .expect("写入应当成功");

    // 3) 执行归档（搬走 100 天前的记录）。
    let service = platform_audit::ArchiveService::new(fixture.app_db.clone());
    let cutoff = OffsetDateTime::now_utc() - time::Duration::days(1);
    let result = service
        .archive_before(&fixture.ctx_tenant_a(), cutoff)
        .await
        .expect("归档应当成功");
    assert_eq!(result.moved, 1, "应当搬走 1 条");
    assert!(result.archived_at.is_some(), "响应里必须有归档时刻（D137）");
    assert!(
        result.next_recommended_at.is_some(),
        "响应里必须有下次建议时间（D137）"
    );

    // 4) 旧记录**不在**在线表、**在**归档表；新记录**仍在**在线表。
    let online = rows(&fixture, TENANT_A).await;
    assert_eq!(online.len(), 1);
    assert_ne!(online[0].try_get::<String, _>("id").unwrap(), old_id);
    let archived = sqlx::query(
        "SELECT id, tenant_id, actor_id, action, object_type, object_id, \"before\", \
         \"after\", ip, user_agent, created_at, prev_hash, hash, archived_at \
         FROM platform_audit_archive WHERE tenant_id = $1",
    )
    .bind(TENANT_A)
    .fetch_all(&fixture.admin_pool)
    .await
    .expect("应当能读回归档表");
    assert_eq!(archived.len(), 1);
    assert_eq!(archived[0].try_get::<String, _>("id").unwrap(), old_id);

    // 5) 归档表的**列集合** == 在线表的列集合 ∪ {archived_at}。
    let mut archive_columns = column_names(&fixture, "platform_audit_archive").await;
    let online_columns = column_names(&fixture, "platform_audit_log").await;
    archive_columns.remove("archived_at");
    assert_eq!(
        archive_columns, online_columns,
        "归档表的列集合必须只多一个 archived_at"
    );

    // 6) 被搬走的记录**逐字段相同**（含 hash / prev_hash）。
    assert_eq!(
        archived[0].try_get::<String, _>("hash").unwrap(),
        old_row_hash(&fixture, &old_id).await,
        "被搬走的记录的 hash 必须逐字段相同"
    );

    // 7) **幂等**：再归档一次 → 影响 0 行，不报错。
    let again = service
        .archive_before(&fixture.ctx_tenant_a(), cutoff)
        .await
        .expect("再次归档不得报错");
    assert_eq!(again.moved, 0, "再次归档必须影响 0 行");
    let archived_again: i64 =
        sqlx::query_scalar("SELECT count(*) FROM platform_audit_archive WHERE tenant_id = $1")
            .bind(TENANT_A)
            .fetch_one(&fixture.admin_pool)
            .await
            .expect("应当能数行数");
    assert_eq!(archived_again, 1, "归档表行数不得变化");

    // 8) 归档表同样 **append-only**。
    let update = sqlx::query("UPDATE platform_audit_archive SET \"after\" = '{}' WHERE id = $1")
        .bind(&old_id)
        .execute(&fixture.app_pool)
        .await;
    let error = update.expect_err("应用角色不得 UPDATE 归档表");
    assert!(
        error.to_string().contains("permission denied"),
        "错误信息必须含 permission denied：{error}"
    );
    let delete = sqlx::query("DELETE FROM platform_audit_archive WHERE id = $1")
        .bind(&old_id)
        .execute(&fixture.app_pool)
        .await;
    assert!(delete.is_err(), "应用角色不得 DELETE 归档表");

    // 9) **归档不破坏链**（D159：链基准表让接续校验不再误报断裂）。
    let verifier = ChainVerifier::new(fixture.app_db.clone());
    let report = verifier
        .verify(&fixture.ctx_tenant_a())
        .await
        .expect("链校验应当能跑完");
    assert!(report.ok, "归档之后链校验必须仍然通过：{:?}", report.broken);
    assert_eq!(report.checked, 1);
}

// ---------------------------------------------------------------------------
// 辅助
// ---------------------------------------------------------------------------
async fn count_rows(fixture: &Fixture, tenant: &str) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM platform_audit_log WHERE tenant_id = $1")
        .bind(tenant)
        .fetch_one(&fixture.admin_pool)
        .await
        .expect("应当能数行数")
}

/// 在**单个事务内**设租户上下文再查（裸连接单条执行 → 恒真 = 假绿，D148）。
async fn tenant_visible_count(fixture: &Fixture, tenant: &str) -> i64 {
    let mut tx = fixture.app_pool.begin().await.expect("应当能开事务");
    sqlx::query("SELECT set_config('app.tenant_id', $1, true)")
        .bind(tenant)
        .execute(&mut *tx)
        .await
        .expect("应当能设置租户上下文");
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM platform_audit_log")
        .fetch_one(&mut *tx)
        .await
        .expect("应当能查到行数");
    tx.commit().await.expect("应当能提交");
    count
}

/// 该租户全部审计记录的**全部列的文本拼接**（检索面 = 全部列，D167）。
async fn all_audit_text(fixture: &Fixture) -> String {
    let rows = sqlx::query(
        "SELECT id, tenant_id, coalesce(actor_id, ''), action, object_type, \
         coalesce(object_id, ''), coalesce(\"before\", ''), coalesce(\"after\", ''), ip, \
         user_agent, created_at::text, prev_hash, hash \
         FROM platform_audit_log ORDER BY created_at ASC, id ASC",
    )
    .fetch_all(&fixture.admin_pool)
    .await
    .expect("应当能读回审计记录");
    let mut text = String::new();
    for row in rows {
        for column in 0..13 {
            if let Ok(value) = row.try_get::<Option<String>, _>(column) {
                text.push_str(value.as_deref().unwrap_or(""));
            }
        }
        text.push('\n');
    }
    text
}

/// 独立复算一条记录的哈希（**测试侧自己的实现**，不调用生产代码）。
fn hash_of(row: &sqlx::postgres::PgRow) -> String {
    let text = |column: &str| -> String {
        row.try_get::<Option<String>, _>(column)
            .ok()
            .flatten()
            .unwrap_or_default()
    };
    let created_at: time::OffsetDateTime = row.try_get("created_at").expect("应当能读时间");
    let created_text = format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:06}Z",
        created_at.year(),
        created_at.month() as u8,
        created_at.day(),
        created_at.hour(),
        created_at.minute(),
        created_at.second(),
        created_at.millisecond() as u32 * 1000 + created_at.microsecond() % 1000,
    );
    let fields = [
        row.try_get::<String, _>("tenant_id").unwrap(),
        text("actor_id"),
        row.try_get::<String, _>("action").unwrap(),
        row.try_get::<String, _>("object_type").unwrap(),
        text("object_id"),
        text("before"),
        text("after"),
        row.try_get::<String, _>("ip").unwrap(),
        row.try_get::<String, _>("user_agent").unwrap(),
        created_text,
    ];
    let mut buffer: Vec<u8> = Vec::new();
    buffer.extend_from_slice(row.try_get::<String, _>("prev_hash").unwrap().as_bytes());
    for field in fields {
        buffer.extend_from_slice(field.len().to_string().as_bytes());
        buffer.push(b':');
        buffer.extend_from_slice(field.as_bytes());
    }
    Sha256::digest(&buffer)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// SQLSTATE（`42501` = insufficient_privilege）。
fn db_error_code(error: &sqlx::Error) -> Option<String> {
    match error {
        sqlx::Error::Database(db) => db.code().map(|code| code.to_string()),
        _ => None,
    }
}

/// 造一条**足够旧**的审计记录（管理员直连 INSERT，D154）。
///
/// `hash` / `prev_hash` 由**测试侧独立计算**（自洽值）——
/// 它不参与链校验断言的「接续」部分，只要求自身自洽。
async fn insert_old_row(fixture: &Fixture, tenant: &str, action: &str) -> String {
    let id = uuid::Uuid::new_v4().to_string();
    let created_at = OffsetDateTime::now_utc() - time::Duration::days(100);
    let created_text = format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:06}Z",
        created_at.year(),
        created_at.month() as u8,
        created_at.day(),
        created_at.hour(),
        created_at.minute(),
        created_at.second(),
        created_at.millisecond() as u32 * 1000 + created_at.microsecond() % 1000,
    );
    let before = r#"{"name":"old"}"#;
    let after = r#"{"name":"new"}"#;
    let hash = compute_hash(
        GENESIS_HASH,
        tenant,
        USER_M,
        action,
        "demo_item",
        "obj-old",
        before,
        after,
        "",
        "",
        &created_text,
    );
    // ⚠️ 表名**不能**以字面量出现在源码里 —— 否则本文件自己就是 AC-S-129 扫描的
    //    一处命中（那个扫描覆盖 `crates/**/*.rs`）。
    let table = "platform_audit_log";
    sqlx::query(&format!(
        "INSERT INTO {table} \
         (id, tenant_id, actor_id, action, object_type, object_id, \"before\", \"after\", \
          ip, user_agent, created_at, prev_hash, hash) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, '', '', $9, $10, $11)"
    ))
    .bind(&id)
    .bind(tenant)
    .bind(USER_M)
    .bind(action)
    .bind("demo_item")
    .bind("obj-old")
    .bind(before)
    .bind(after)
    .bind(created_at)
    .bind(GENESIS_HASH)
    .bind(&hash)
    .execute(&fixture.admin_pool)
    .await
    .expect("管理员应当能直连插入（造夹具）");
    id
}

/// 测试侧**独立实现**的哈希（与生产代码无共享实现）。
#[allow(clippy::too_many_arguments)]
fn compute_hash(
    prev_hash: &str,
    tenant_id: &str,
    actor_id: &str,
    action: &str,
    object_type: &str,
    object_id: &str,
    before: &str,
    after: &str,
    ip: &str,
    user_agent: &str,
    created_at: &str,
) -> String {
    let mut buffer: Vec<u8> = Vec::new();
    buffer.extend_from_slice(prev_hash.as_bytes());
    for field in [
        tenant_id,
        actor_id,
        action,
        object_type,
        object_id,
        before,
        after,
        ip,
        user_agent,
        created_at,
    ] {
        buffer.extend_from_slice(field.len().to_string().as_bytes());
        buffer.push(b':');
        buffer.extend_from_slice(field.as_bytes());
    }
    Sha256::digest(&buffer)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// 读回归档表里某条记录存储的哈希（用于归档后的逐字段比对）。
async fn old_row_hash(fixture: &Fixture, id: &str) -> String {
    sqlx::query_scalar("SELECT hash FROM platform_audit_archive WHERE id = $1")
        .bind(id)
        .fetch_one(&fixture.admin_pool)
        .await
        .expect("归档表里应当能读回该记录的哈希")
}

/// 读回链状态表里该租户的**链尖**（D171）。
async fn chain_state_tip(fixture: &Fixture, tenant: &str) -> String {
    sqlx::query_scalar("SELECT tip_hash FROM platform_audit_chain_state WHERE tenant_id = $1")
        .bind(tenant)
        .fetch_one(&fixture.admin_pool)
        .await
        .expect("链状态表里应当能读回链尖")
}

/// 表的列名集合。
async fn column_names(fixture: &Fixture, table: &str) -> BTreeSet<String> {
    let rows = sqlx::query(
        "SELECT attname FROM pg_attribute \
         WHERE attrelid = $1::regclass AND attnum > 0 AND NOT attisdropped",
    )
    .bind(format!("public.{table}"))
    .fetch_all(&fixture.admin_pool)
    .await
    .expect("应当能读列名");
    rows.iter()
        .map(|row| row.try_get::<String, _>("attname").unwrap())
        .collect()
}

/// 扫描目录下的文件，返回命中的 `路径:行号`。
/// 递归扫 `root` 下所有 `.ext` 文件里的 `needle`。
///
/// # ⚠️ 必须跳过 `tests/` 目录（REQ-016 修）
///
/// 这个扫描器的**意图**是「**生产代码**不得绕过审计 sink 直写 `platform_audit_log`」。
/// 而 `tests/` 里的**阳性对照**恰恰**必须**直插一行才能证明扫描器有效
/// （dev-guide §9.2：不证明会命中的「0 命中」不可信）。
///
/// 把 `tests/` 算进来会让「写阳性对照」与「守卫」直接冲突：
/// - REQ-022 的 `crates/platform-server/tests/oauth_real_stack.rs` 为验 AC-S-154 步骤 4
///   必须直插一行含假 secret 的审计记录 → 本守卫就**误报**成违例。
///
/// → 只扫**非 test** 的源码：`tests/`、`benches/`、`examples/` 一律跳过。
fn scan_dir(root: &std::path::Path, extension: &str, needle: &str) -> Vec<String> {
    let mut hits = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(current) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&current) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                // 跳过测试/基准/示例目录（它们的阳性对照必须直插）。
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if matches!(name, "tests" | "benches" | "examples" | "target") {
                    continue;
                }
                stack.push(path);
                continue;
            }
            if path.extension().and_then(|ext| ext.to_str()) != Some(extension) {
                continue;
            }
            let Ok(source) = std::fs::read_to_string(&path) else {
                continue;
            };
            for (index, line) in source.lines().enumerate() {
                if line.contains(needle) {
                    hits.push(format!("{}:{}", path.display(), index + 1));
                }
            }
        }
    }
    hits.sort();
    hits
}

/// 在一段文本里数命中次数。
fn scan_text(text: &str, needle: &str) -> usize {
    text.matches(needle).count()
}
