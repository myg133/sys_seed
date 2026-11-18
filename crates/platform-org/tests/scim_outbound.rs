//! SCIM 2.0 **出站**的实测（REQ-008）。
//!
//! 跑的是**真的 HTTP**：本文件起一个 SCIM mock 服务（`tests/common/mod.rs`），
//! 用自己实现的 [`HttpScimGateway`]（不引 SCIM 库，D6）跟它对话。
//!
//! | 用例 | 验收 |
//! |------|------|
//! | `put_semantics_do_not_duplicate_entries` | AC-S-012：幂等 |
//! | `the_field_mapping_is_applied_to_the_payload` | AC-S-013：字段映射 |
//! | `a_slow_target_does_not_block_the_local_request` | AC-S-013：推送失败/慢不阻塞本地 |
//! | `a_failed_push_is_retried` | 失败重试（指数退避） |

mod common;

use std::time::{Duration, Instant};

use common::start_mock_scim_server;
use platform_org::scim::{
    HttpScimGateway, ScimGateway, ScimJob, ScimJobEntry, ScimOutbox, ScimProviderConfig,
    ScimUserDraft,
};

/// 造一条用户草稿。
fn draft(id: &str) -> ScimUserDraft {
    ScimUserDraft {
        id: id.to_string(),
        username: format!("{id}-name"),
        email: Some(format!("{id}@example.com")),
        display_name: Some(format!("{id} display")),
        active: true,
        group_ids: vec!["dept-1".to_string()],
    }
}

/// AC-S-012：幂等 —— 同一个 id 推两次，目标端只有**一条**记录。
#[tokio::test]
async fn put_semantics_do_not_duplicate_entries() {
    let server = start_mock_scim_server().await;
    let gateway = HttpScimGateway::new();
    let config = ScimProviderConfig::new("corp-scim", server.base_url.clone());
    let payload = draft("u1");

    gateway
        .put_user(&config, &payload)
        .await
        .expect("第一次推送应成功");
    gateway
        .put_user(&config, &payload)
        .await
        .expect("第二次推送应成功");

    assert_eq!(server.user_count(), 1, "PUT 两次不应产生重复条目");
    assert_eq!(server.calls().len(), 2, "两次推送都该发出去了");
}

/// AC-S-012：Group 同样幂等。
#[tokio::test]
async fn groups_are_pushed_idempotently_with_their_members() {
    let server = start_mock_scim_server().await;
    let gateway = HttpScimGateway::new();
    let config = ScimProviderConfig::new("corp-scim", server.base_url.clone());
    let group = platform_org::scim::ScimGroupDraft {
        id: "dept-1".to_string(),
        display_name: "Engineering".to_string(),
        member_ids: vec!["u1".to_string()],
    };

    gateway
        .put_group(&config, &group)
        .await
        .expect("第一次推送应成功");
    gateway
        .put_group(&config, &group)
        .await
        .expect("第二次推送应成功");

    assert_eq!(server.group_count(), 1);
    let stored = server.user("dept-1");
    assert!(stored.is_none(), "Group 不该出现在 Users 里");
}

/// AC-S-013：字段映射 —— 用户名 / 邮箱 / 显示名来自**不同属性**时，
/// 目标端收到的是**按映射规则转换后**的值。
#[tokio::test]
async fn the_field_mapping_is_applied_to_the_payload() {
    let server = start_mock_scim_server().await;
    let gateway = HttpScimGateway::new();
    let config = ScimProviderConfig::from_json(&format!(
        r#"{{
          "provider": "corp-scim",
          "baseUrl": "{}",
          "userFields": {{
            "username": "login",
            "email": "workEmail",
            "displayName": "cn",
            "emailShape": "single"
          }}
        }}"#,
        server.base_url
    ))
    .expect("配置应能解析");

    gateway
        .put_user(&config, &draft("u2"))
        .await
        .expect("推送应成功");

    let stored = server.user("u2").expect("目标端应有这条记录");
    assert_eq!(stored["login"], serde_json::json!("u2-name"));
    assert_eq!(stored["workEmail"], serde_json::json!("u2@example.com"));
    assert_eq!(stored["cn"], serde_json::json!("u2 display"));
    assert!(stored.get("userName").is_none(), "不该用默认字段名");
    assert!(stored.get("emails").is_none(), "不该用默认字段名");
}

/// AC-S-013：**推送慢 / 失败不阻塞本地请求**（D6 / R3）。
///
/// 本地操作只**入队**就返回 —— 第三方慢不影响本地。
#[tokio::test]
async fn a_slow_target_does_not_block_the_local_request() {
    let server = start_mock_scim_server().await;
    server.set_delay(Duration::from_millis(800));
    let gateway = HttpScimGateway::new();
    let config = ScimProviderConfig::new("corp-scim", server.base_url.clone());
    let outbox = ScimOutbox::new();

    // 本地动作：入队（**不等第三方**）。
    let started = Instant::now();
    outbox.enqueue(ScimJobEntry {
        tenant_id: "tenant-a".to_string(),
        config: config.clone(),
        job: ScimJob::PutUser(draft("u3")),
        attempt: 0,
        ready_at: None,
    });
    let enqueue_cost = started.elapsed();
    assert!(
        enqueue_cost < Duration::from_millis(100),
        "入队不应等第三方（耗时 {enqueue_cost:?}）"
    );
    assert_eq!(outbox.pending(), 1);

    // 后台 worker 慢慢推（本地请求早就返回了）。
    let started = Instant::now();
    let outcomes = outbox
        .drain_once(&gateway, time::OffsetDateTime::now_utc())
        .await;
    let push_cost = started.elapsed();
    assert_eq!(outcomes.len(), 1);
    assert!(outcomes[0].ok);
    assert!(
        push_cost >= Duration::from_millis(700),
        "推送本身确实等了第三方（耗时 {push_cost:?}）"
    );
    assert_eq!(server.user_count(), 1);
}

/// 推送失败 → 进重试（指数退避），超过上限才丢弃。
#[tokio::test]
async fn a_failed_push_is_retried() {
    let server = start_mock_scim_server().await;
    server.fail_next(1);
    let gateway = HttpScimGateway::new();
    let config = ScimProviderConfig::new("corp-scim", server.base_url.clone());
    let outbox = ScimOutbox::new();
    let now = time::OffsetDateTime::now_utc();

    outbox.enqueue(ScimJobEntry {
        tenant_id: "tenant-a".to_string(),
        config,
        job: ScimJob::PutUser(draft("u4")),
        attempt: 0,
        ready_at: None,
    });

    let outcomes = outbox.drain_once(&gateway, now).await;
    assert_eq!(outcomes.len(), 1);
    assert!(!outcomes[0].ok, "第一次应失败");
    assert!(outcomes[0].will_retry);
    assert_eq!(outbox.pending(), 1, "失败的任务应留在队列里");

    // 退避到期后重试成功。
    let later = now + platform_org::scim::backoff_delay(1);
    let outcomes = outbox.drain_once(&gateway, later).await;
    assert_eq!(outcomes.len(), 1);
    assert!(outcomes[0].ok);
    assert_eq!(outbox.pending(), 0);
    assert_eq!(server.user_count(), 1);
}

/// 查询：按 SCIM 过滤器取 Users。
#[tokio::test]
async fn users_can_be_queried_with_a_filter() {
    let server = start_mock_scim_server().await;
    let gateway = HttpScimGateway::new();
    let config = ScimProviderConfig::new("corp-scim", server.base_url.clone());

    gateway
        .put_user(&config, &draft("alice"))
        .await
        .expect("推送应成功");
    gateway
        .put_user(&config, &draft("bob"))
        .await
        .expect("推送应成功");

    let all = gateway.list_users(&config, "").await.expect("查询应成功");
    assert_eq!(all.len(), 2);

    let filtered = gateway
        .list_users(&config, "userName eq \"alice-name\"")
        .await
        .expect("查询应成功");
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].user_name, "alice-name");
}

/// HTTPS 明确拒绝（本期只支持明文），**不静默降级**。
#[tokio::test]
async fn https_is_rejected_instead_of_silently_downgraded() {
    let gateway = HttpScimGateway::new();
    let config = ScimProviderConfig::new("corp-scim", "https://scim.example/v2");
    let error = gateway
        .put_user(&config, &draft("u5"))
        .await
        .expect_err("https 应被拒绝");
    assert!(
        matches!(error, platform_org::scim::ScimError::Config(_)),
        "错误类型不对：{error:?}"
    );
}
