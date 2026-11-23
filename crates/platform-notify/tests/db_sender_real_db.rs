//! **落库适配器**的真实库判据（REQ-020 D354 / AC-S-189 的通知计数）。
//!
//! # 这里验的是「真的写了表」，不是「调用了方法」
//!
//! `NotificationSender` 的实现可以只把消息塞进内存 `Vec`（`channels/in_app.rs` 就是
//! 这么干的，那是**通道**的职责）。业务要的「通知能查真表」只有一条路：
//! 适配器内部走 `NotifyService::enqueue` → 插进 `platform_notify_delivery`。
//!
//! → 所以每条用例都**从库里读回来**断字段（租户 / 通道 / 收件人 / 渲染快照）。
//!
//! # ⚠️ 两条**实测的**限制（用例如实钉住，不粉饰）
//!
//! 1. 找不到模板 → `Err`（`NOTIFY_TEMPLATE_NOT_FOUND`）——适配器**不**凭空造模板；
//! 2. 收件人为空 → `delivered: false` 且**不落库**——适配器**不**按
//!    `payload["audience"]` 猜收件人（平台没有「按权限点解析收件人」的能力）。
//!
//! 跑法：
//!
//! ```bash
//! DATABASE_URL_TEST=postgres://base:base@127.0.0.1:5432/base_test \
//!   cargo test -p platform-notify --test db_sender_real_db -- --test-threads=1
//! ```

mod common;

use std::collections::BTreeMap;
use std::sync::Arc;

use common::*;
use platform_core::RequestContext;
use platform_core::extension::notification::{NotificationMessage, NotificationSender};
use platform_notify::db_sender::DbNotificationSender;
use platform_notify::model::{category, channel};
use platform_notify::service::NotifyService;

/// 让 `tracing::error!` 的细节在失败时可见。
fn init_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::ERROR)
        .with_test_writer()
        .try_init();
}

/// 模板编码（与 `seed_template` 的 code 一致）。
const TEMPLATE_CODE: &str = "leave.reviewed";

/// 造一个适配器（模板已按 `tenant-a` + `zh-CN` 种好）。
async fn sender_for(fixture: &Fixture, ctx: &RequestContext) -> Arc<DbNotificationSender> {
    seed_template(
        fixture,
        ctx,
        TEMPLATE_CODE,
        channel::IN_APP,
        "zh-CN",
        category::TRANSACTIONAL,
    )
    .await;
    Arc::new(
        DbNotificationSender::new(fixture.service.clone(), TEMPLATE_CODE)
            .with_locale("zh-CN"),
    )
}

/// 符合 `{{name}}` 的 payload。
fn payload() -> BTreeMap<String, String> {
    let mut payload = BTreeMap::new();
    payload.insert("name".to_string(), "张三".to_string());
    payload
}

/// 统计某租户下本模板的投递条数。
async fn delivery_count(fixture: &Fixture, tenant: &str) -> i64 {
    fixture
        .admin
        .query_text(&format!(
            "SELECT count(*)::text FROM platform_notify_delivery \
             WHERE tenant_id = '{tenant}' AND template_code = '{TEMPLATE_CODE}'"
        ))
        .await
        .first()
        .and_then(|value| value.parse().ok())
        .expect("count 应当能解析")
}

/// **主判据**：`send()` 真的插进 `platform_notify_delivery`，且字段与模板/消息一致。
///
/// 删掉 `DbNotificationSender` 的实现（或改成只写内存），这条必红 ——
/// 这就是 AC-S-189「查通知投递（真表）」的判据落点。
#[tokio::test]
#[ignore = "需要 base_test 库：用 --include-ignored 跑到"]
async fn send_writes_a_delivery_row_for_the_tenant() {
    init_tracing();
    let fixture = setup().await;
    let ctx = fixture.ctx_user("u-leave-1");
    let sender = sender_for(&fixture, &fixture.ctx_a()).await;

    let message = NotificationMessage::new("ignored title", "ignored body")
        .with_recipient("u-leave-1");
    let mut message = message;
    message.payload = payload();

    let result = sender
        .send(&ctx, &message)
        .await
        .expect("入队应当成功（模板已种好）");
    assert!(result.delivered, "必须报告投递已入队：{result:?}");
    let delivery_id = result
        .provider_message_id
        .as_deref()
        .expect("provider_message_id 是投递记录主键");
    assert!(!delivery_id.is_empty());

    // ① 真的落库了。
    assert_eq!(delivery_count(&fixture, TENANT_A).await, 1);

    // ② 字段逐个核对（不是「有一行就算」）。
    for (sql, expected) in [
        (
            format!("SELECT channel FROM platform_notify_delivery WHERE id = '{delivery_id}'"),
            channel::IN_APP.to_string(),
        ),
        (
            format!("SELECT recipient FROM platform_notify_delivery WHERE id = '{delivery_id}'"),
            "u-leave-1".to_string(),
        ),
        (
            format!("SELECT status FROM platform_notify_delivery WHERE id = '{delivery_id}'"),
            "pending".to_string(),
        ),
    ] {
        let actual = fixture
            .admin
            .query_text(&sql)
            .await
            .first()
            .cloned()
            .unwrap_or_default();
        assert_eq!(actual, expected, "SQL：{sql}");
    }

    // ③ 渲染快照来自**模板**（`{{name}}` 被 payload 替换），不是 message 的 title/body。
    let rendered_body = fixture
        .admin
        .query_text(&format!(
            "SELECT rendered_body FROM platform_notify_delivery WHERE id = '{delivery_id}'"
        ))
        .await
        .first()
        .cloned()
        .unwrap_or_default();
    assert!(
        rendered_body.contains("张三"),
        "渲染结果必须来自模板 + payload：{rendered_body}"
    );
    assert!(
        !rendered_body.contains("ignored body"),
        "message.body 不参与渲染（模板才是渲染来源）：{rendered_body}"
    );
}

/// 多个收件人 → **每个一条**投递记录（不是只给第一个发）。
#[tokio::test]
#[ignore = "需要 base_test 库：用 --include-ignored 跑到"]
async fn every_recipient_gets_its_own_row() {
    init_tracing();
    let fixture = setup().await;
    let ctx = fixture.ctx_user("u-leave-1");
    let sender = sender_for(&fixture, &fixture.ctx_a()).await;

    let mut message = NotificationMessage::new("t", "b")
        .with_recipient("u-A")
        .with_recipient("u-B");
    message.payload = payload();

    let result = sender.send(&ctx, &message).await.expect("入队应当成功");
    assert!(result.delivered);
    assert_eq!(delivery_count(&fixture, TENANT_A).await, 2);

    let recipients: Vec<String> = fixture
        .admin
        .query_text(&format!(
            "SELECT recipient FROM platform_notify_delivery \
             WHERE tenant_id = '{TENANT_A}' AND template_code = '{TEMPLATE_CODE}' ORDER BY recipient"
        ))
        .await;
    assert_eq!(recipients, vec!["u-A".to_string(), "u-B".to_string()]);
}

/// 空收件人 → `delivered: false`，**一行都不落**。
///
/// ⚠️ 这是「业务只给了 `payload["audience"]`」的真实形态（`modules/leave` 的
/// 提交通知就是这么发的）：平台没有「按权限点解析收件人」的能力 →
/// 适配器**不得**猜，只能如实报告没有投递对象。
#[tokio::test]
#[ignore = "需要 base_test 库：用 --include-ignored 跑到"]
async fn an_empty_recipient_list_is_a_legal_failure_with_no_row() {
    init_tracing();
    let fixture = setup().await;
    let ctx = fixture.ctx_a();
    let sender = sender_for(&fixture, &fixture.ctx_a()).await;

    let mut message = NotificationMessage::new("t", "b");
    message.payload = payload();
    message
        .payload
        .insert("audience".to_string(), "leave:request:approve".to_string());

    let result = sender.send(&ctx, &message).await.expect("不是异常，是合法失败");
    assert!(!result.delivered, "没有收件人不得报告投递成功：{result:?}");
    assert!(result.provider_message_id.is_none());
    assert_eq!(
        delivery_count(&fixture, TENANT_A).await,
        0,
        "没有收件人时不得落库"
    );
}

/// 模板缺失 → `Err`（**不得**凭空造一条「无模板」的投递）。
///
/// 删掉这条判据的严格性（比如吞掉错误返回失败），业务会以为通知排上了。
#[tokio::test]
#[ignore = "需要 base_test 库：用 --include-ignored 跑到"]
async fn a_missing_template_is_an_error_and_writes_nothing() {
    init_tracing();
    let fixture = setup().await;
    let ctx = fixture.ctx_a();
    // ⚠️ 故意**不**种模板。
    let sender = DbNotificationSender::new(fixture.service.clone(), "no.such.template");

    let mut message = NotificationMessage::new("t", "b").with_recipient("u-1");
    message.payload = payload();

    let error = sender
        .send(&ctx, &message)
        .await
        .expect_err("没有模板必须失败，不能静默成功");
    let text = format!("{error:#}");
    assert!(
        text.contains("NOTIFY_TEMPLATE_NOT_FOUND"),
        "错误里必须带稳定码：{text}"
    );
    assert_eq!(delivery_count(&fixture, TENANT_A).await, 0);
}

/// 适配器只做 `in_app`（`channel()` 是路由表的键，写错就发错通道）。
#[tokio::test]
#[ignore = "需要 base_test 库：用 --include-ignored 跑到"]
async fn the_channel_is_in_app() {
    init_tracing();
    let fixture = setup().await;
    let service: Arc<NotifyService> = fixture.service.clone();
    let sender = DbNotificationSender::new(service, "any");
    assert_eq!(sender.channel(), channel::IN_APP);
    assert_eq!(sender.template_code(), "any");
}
