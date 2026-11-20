//! 配置写入的**审计埋点**用例（REQ-014 D163 / AC-S-130）。
//!
//! # 覆盖的事件码与形状（**照抄** D163）
//!
//! | 事件 | 事件码 | object_type | before | after |
//! |------|--------|-------------|--------|-------|
//! | 配置条目写入 | `config.entries.write` | `config_entry` | 变更前 | 变更后 |
//! | 配置 flag 写入 | `config.flag.write` | `config_flag` | 变更前 | 变更后 |
//!
//! # `before` 取的是「**被写入那一层**」的值
//!
//! 审计要记的是「**这次写入**把哪一层的什么值换成了什么」——
//! 生效值会混进「平台默认 + 租户覆盖 + 空间覆盖」的解析结果，
//! 读**那一层**才是这次写入的直接对象。
//!
//! # 跑法
//!
//! ```bash
//! DATABASE_URL_TEST=postgres://base:base@127.0.0.1:5432/base_test \
//!   cargo test -p platform-config --test audit_events -- --test-threads=1 --include-ignored
//! ```
//!
//! 缺 `DATABASE_URL_TEST` → **失败**（不是「跳过」—— 跳过也算通过 = 假绿）。

mod common;

use std::sync::{Arc, Mutex};

use common::*;
use platform_config::service::{FlagWriteRequest, WriteEntry, WriteRequest, WriteRoute};
use platform_core::RequestContext;
use platform_core::extension::audit::{AuditError, AuditEvent, AuditSink};
use serde_json::json;

/// 记录事件的**假 sink**（不落库 —— 本用例只断言**埋点契约**）。
#[derive(Debug, Default)]
struct RecordingSink(Mutex<Vec<AuditEvent>>);

impl RecordingSink {
    fn events(&self) -> Vec<AuditEvent> {
        self.0.lock().unwrap_or_else(|err| err.into_inner()).clone()
    }
}

impl AuditSink for RecordingSink {
    fn record<'a>(
        &'a self,
        _ctx: &'a RequestContext,
        event: &'a AuditEvent,
    ) -> platform_core::extension::audit::BoxFuture<'a, Result<(), AuditError>> {
        self.0
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .push(event.clone());
        Box::pin(async { Ok(()) })
    }
}

/// 注入假 sink（**先注入再写**）。
fn install_sink(fixture: &Fixture) -> Arc<RecordingSink> {
    let sink = Arc::new(RecordingSink::default());
    assert!(
        fixture
            .service
            .with_audit_sink(sink.clone() as Arc<dyn AuditSink>),
        "sink 应当能注入一次"
    );
    sink
}

/// 取**最后一条**某事件码的记录。
fn last_of(sink: &RecordingSink, action: &str) -> AuditEvent {
    sink.events()
        .into_iter()
        .rev()
        .find(|event| event.action == action)
        .unwrap_or_else(|| panic!("应当有一条 `{action}` 事件"))
}

/// 写配置条目 → `config.entries.write`（`before` = 那一层的旧值）。
#[tokio::test]
async fn writing_entries_produces_one_event_with_both_sides() {
    let fixture = setup().await;
    let sink = install_sink(&fixture);
    let ctx = ctx_user(TENANT_A, USER_M);

    fixture
        .service
        .write_entries(
            &ctx,
            &WriteRequest {
                scope: "tenant".to_string(),
                entries: vec![WriteEntry {
                    key: "title".to_string(),
                    value: json!("审计标题"),
                }],
            },
            WriteRoute::Scoped,
        )
        .await
        .expect("写入应当成功");

    let recorded = sink.events();
    let events: Vec<&AuditEvent> = recorded
        .iter()
        .filter(|event| event.action == "config.entries.write")
        .collect();
    assert_eq!(events.len(), 1, "一批写 = **一条**审计事件");
    let event = events[0];
    assert_eq!(event.object_type, "config_entry");
    assert_eq!(event.object_id.as_deref(), Some("tenant"), "对象 = 作用域");
    assert_eq!(event.tenant_id, TENANT_A);
    assert_eq!(event.actor_id.as_deref(), Some(USER_M));
    // 夹具里租户层的 `title` 是 `T1平台` → 变更前是它。
    assert_eq!(
        event.before,
        Some(json!({ "title": "T1平台" })),
        "变更前 = 被写入那一层的旧值"
    );
    assert_eq!(
        event.after,
        Some(json!({ "title": "审计标题" })),
        "变更后 = 这次写入的值"
    );
}

/// 写 flag → `config.flag.write`。
#[tokio::test]
async fn writing_a_flag_produces_a_flag_event() {
    let fixture = setup().await;
    let sink = install_sink(&fixture);
    let ctx = ctx_user(TENANT_A, USER_M);

    fixture
        .service
        .write_flag(
            &ctx,
            "newInvoice",
            &FlagWriteRequest {
                enabled: Some(json!(false)),
                rollout_percent: None,
            },
        )
        .await
        .expect("写 flag 应当成功");

    let event = last_of(&sink, "config.flag.write");
    assert_eq!(event.object_type, "config_flag");
    assert_eq!(
        event.object_id.as_deref(),
        Some("newInvoice"),
        "对象 = flag 名"
    );
    assert_eq!(event.tenant_id, TENANT_A);
    assert_eq!(
        event.after,
        Some(json!({ "newInvoice": false })),
        "变更后 = 这次写入的值"
    );
    // 变更前取租户层的值（夹具里是 `true`）。
    assert_eq!(
        event.before,
        Some(json!({ "newInvoice": true })),
        "变更前 = 租户层的旧值"
    );
}

/// **没有装配 sink** → 不埋点，但**操作照常成功**（不得因为审计而拖垮业务）。
#[tokio::test]
async fn writing_without_a_sink_still_succeeds() {
    let fixture = setup().await;
    let ctx = ctx_user(TENANT_A, USER_M);
    let written = fixture
        .service
        .write_entries(
            &ctx,
            &WriteRequest {
                scope: "tenant".to_string(),
                entries: vec![WriteEntry {
                    key: "title".to_string(),
                    value: json!("无 sink 的写入"),
                }],
            },
            WriteRoute::Scoped,
        )
        .await
        .expect("没有 sink 也应当写成功");
    assert_eq!(written, 1);
}
