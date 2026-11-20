//! 组织架构的**审计埋点**。
//!
//! # 平台的约定：模块**只声明**事件，不手写审计表
//!
//! `AGENTS.md` 的能力目录里写着：「`Auditable` + `AuditEventSpec` 提供审计事件声明面，
//! 平台据此自动埋点，**勿自建审计表**」。
//! 审计的**落库与查询**在 REQ-014 —— 那时平台会提供真正的 sink。
//!
//! 本期没有平台 sink 可调，所以本模块提供一层**接缝** [`AuditRecorder`]：
//!
//! - 默认实现是 [`NoopAuditRecorder`]（什么都不做）；
//! - 测试用 [`RecordingAuditRecorder`] 断言「事件确实产生了」；
//! - REQ-014 的平台 sink 落地后，换一个实现即可，**模块代码不用改**。

use std::sync::{Arc, LazyLock, Mutex};

use anyhow::Result;
use platform_core::RequestContext;
use platform_core::extension::audit::{AuditEvent, AuditSink};
use platform_core::extension::auditable::AuditEventSpec;

/// 装箱的 future（让接缝保持**可对象化**）。
pub type BoxFuture<'a, T> = std::pin::Pin<Box<dyn std::future::Future<Output = T> + Send + 'a>>;

/// 一次组织架构的审计事件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrgAuditEvent {
    /// 事件码（与 [`AUDIT_EVENTS`] 里的声明对应）。
    pub code: String,
    /// 动作名。
    pub action: String,
    /// 对象类型（如 `org_node`）。
    pub object_type: String,
    /// 对象 ID。
    pub object_id: String,
    /// 面向开发者的英文明细。
    pub detail: String,
}

impl OrgAuditEvent {
    /// 造一个事件。
    pub fn new(
        code: impl Into<String>,
        action: impl Into<String>,
        object_type: impl Into<String>,
        object_id: impl Into<String>,
    ) -> Self {
        Self {
            code: code.into(),
            action: action.into(),
            object_type: object_type.into(),
            object_id: object_id.into(),
            detail: String::new(),
        }
    }

    /// 补充明细。
    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = detail.into();
        self
    }
}

/// 审计事件接收端（可对象化）。
pub trait AuditRecorder: Send + Sync {
    /// 记录一个事件。
    fn record<'a>(
        &'a self,
        ctx: &'a RequestContext,
        event: &'a OrgAuditEvent,
    ) -> BoxFuture<'a, Result<()>>;
}

/// 什么都不记的接收端（**默认**）。
#[derive(Debug, Clone, Copy, Default)]
pub struct NoopAuditRecorder;

impl AuditRecorder for NoopAuditRecorder {
    fn record<'a>(
        &'a self,
        _ctx: &'a RequestContext,
        _event: &'a OrgAuditEvent,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async { Ok(()) })
    }
}

/// 把事件记在内存里的接收端（**测试用**）。
#[derive(Debug, Default)]
pub struct RecordingAuditRecorder {
    events: Mutex<Vec<OrgAuditEvent>>,
}

impl RecordingAuditRecorder {
    /// 造一个记录器。
    pub fn new() -> Self {
        Self::default()
    }

    /// 已记录的事件。
    pub fn events(&self) -> Vec<OrgAuditEvent> {
        self.events
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .clone()
    }

    /// 是否记录过某个事件码。
    pub fn has_code(&self, code: &str) -> bool {
        self.events().iter().any(|event| event.code == code)
    }
}

impl AuditRecorder for RecordingAuditRecorder {
    fn record<'a>(
        &'a self,
        _ctx: &'a RequestContext,
        event: &'a OrgAuditEvent,
    ) -> BoxFuture<'a, Result<()>> {
        let mut events = self.events.lock().unwrap_or_else(|err| err.into_inner());
        events.push(event.clone());
        Box::pin(async { Ok(()) })
    }
}

/// 把既有 `OrgAuditEvent` **桥接**到平台 sink 的实现（REQ-014 D135 / D150）。
///
/// # 为什么不改既有 trait 的签名
///
/// D135 钉死：`platform-org` 的 `AuditRecorder`（绑死 `OrgAuditEvent`）
/// **签名一律不改** —— REQ-008 已合并，改签名会连带破坏既有测试。
/// → 这里实现**同一个 trait**，把事件翻译成通用 `AuditEvent` 后交给平台 sink。
///
/// 装配层（`platform-server`）把本实现传给 `OrgService::new` 的第 4 个参数
/// （**既有注入点，签名不变**）。
#[derive(Clone)]
pub struct PlatformSinkAuditRecorder {
    sink: Arc<dyn AuditSink>,
}

impl PlatformSinkAuditRecorder {
    /// 用平台 sink 构造。
    pub fn new(sink: Arc<dyn AuditSink>) -> Self {
        Self { sink }
    }
}

impl std::fmt::Debug for PlatformSinkAuditRecorder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PlatformSinkAuditRecorder").finish()
    }
}

impl AuditRecorder for PlatformSinkAuditRecorder {
    fn record<'a>(
        &'a self,
        ctx: &'a RequestContext,
        event: &'a OrgAuditEvent,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let translated = translate(ctx, event);
            self.sink.record(ctx, &translated).await.map_err(|error| {
                anyhow::anyhow!(
                    "the platform audit sink could not persist an org audit event: {}",
                    error.detail()
                )
            })
        })
    }
}

/// 把 `OrgAuditEvent` 翻译成通用 `AuditEvent`。
///
/// # 形状
///
/// | 通用事件 | 取自 |
/// |----------|------|
/// | `action` | [`OrgAuditEvent::code`]（**事件码**，如 `org.node.create`） |
/// | `object_type` | [`OrgAuditEvent::object_type`] |
/// | `object_id` | [`OrgAuditEvent::object_id`] |
/// | `before` | `{"detail": …}` —— org 事件**没有**前后值，明细放这里 |
///
/// ⚠️ 明细是**面向开发者的英文**（既有约定）→ 不含凭据；
/// 敏感键扫描器也只拦**敏感键名**（`detail` 不在其中）。
fn translate(ctx: &RequestContext, event: &OrgAuditEvent) -> AuditEvent {
    let mut translated = AuditEvent::new(
        ctx.tenant_id().unwrap_or_default(),
        event.code.clone(),
        event.object_type.clone(),
    )
    .with_object_id(event.object_id.clone());
    if let Some(user_id) = ctx.user_id() {
        translated = translated.with_actor(user_id);
    }
    if !event.detail.is_empty() {
        translated = translated.with_before(serde_json::json!({ "detail": event.detail }));
    }
    translated
}

/// 本模块声明的审计事件（平台的 `Auditable` 消费这份声明）。
pub static AUDIT_EVENTS: LazyLock<Vec<AuditEventSpec>> = LazyLock::new(|| {
    vec![
        AuditEventSpec::new("org.node.create", "create", "org_node")
            .with_description("create an organization node"),
        AuditEventSpec::new("org.node.move", "move", "org_node")
            .with_description("move an organization node under another parent"),
        AuditEventSpec::new("org.node.rename", "rename", "org_node")
            .with_description("rename an organization node"),
        AuditEventSpec::new("org.node.delete", "delete", "org_node")
            .with_description("delete an organization node"),
        AuditEventSpec::new("org.membership.add", "create", "user_org")
            .with_description("add a user to an organization node"),
        AuditEventSpec::new("org.membership.remove", "delete", "user_org")
            .with_description("remove a user from an organization node"),
        AuditEventSpec::new("org.user.departed", "disable", "user")
            .with_description("disable a departed user and revoke every session"),
        AuditEventSpec::new("org.sync.conflict", "detect", "sync_conflict")
            .with_description("record a field level synchronization conflict"),
        AuditEventSpec::new("org.sync.conflict.resolved", "resolve", "sync_conflict")
            .with_description("adjudicate a field level synchronization conflict"),
    ]
});

#[cfg(test)]
mod tests {
    use super::*;
    use platform_core::extension::audit::AuditError;

    /// 记录事件的**假 sink**（测试用）。
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
        ) -> BoxFuture<'a, Result<(), AuditError>> {
            self.0
                .lock()
                .unwrap_or_else(|err| err.into_inner())
                .push(event.clone());
            Box::pin(async { Ok(()) })
        }
    }

    #[test]
    fn every_declared_event_has_a_stable_code() {
        for spec in AUDIT_EVENTS.iter() {
            assert!(spec.code.starts_with("org."));
            assert!(!spec.object_type.is_empty());
            assert!(!spec.description.is_empty());
        }
    }

    #[tokio::test]
    async fn the_recording_recorder_keeps_the_events() {
        let recorder = RecordingAuditRecorder::new();
        let ctx = RequestContext::new().with_tenant("tenant-a");
        let event = OrgAuditEvent::new("org.node.create", "create", "org_node", "n1");
        recorder.record(&ctx, &event).await.expect("记录应成功");
        assert_eq!(recorder.events().len(), 1);
        assert!(recorder.has_code("org.node.create"));
    }

    #[tokio::test]
    async fn the_noop_recorder_never_fails() {
        let recorder = NoopAuditRecorder;
        let ctx = RequestContext::new().with_tenant("tenant-a");
        let event = OrgAuditEvent::new("org.node.create", "create", "org_node", "n1");
        recorder.record(&ctx, &event).await.expect("不应失败");
    }

    /// 桥接**不改**既有 trait 的形状，但**真的**把事件送进平台 sink（REQ-014 D150）。
    #[tokio::test]
    async fn the_platform_sink_recorder_forwards_the_event() {
        let sink = Arc::new(RecordingSink::default());
        let recorder = PlatformSinkAuditRecorder::new(sink.clone());
        let ctx = RequestContext::new()
            .with_tenant("tenant-a")
            .with_user("u-2001");
        let event = OrgAuditEvent::new("org.node.create", "create", "org_node", "n-1")
            .with_detail("created the node n-1");
        recorder.record(&ctx, &event).await.expect("桥接应当成功");

        let events = sink.events();
        assert_eq!(events.len(), 1, "一次事件 = 一条记录");
        let recorded = &events[0];
        assert_eq!(recorded.action, "org.node.create", "action = 事件码");
        assert_eq!(recorded.object_type, "org_node");
        assert_eq!(recorded.object_id.as_deref(), Some("n-1"));
        assert_eq!(recorded.actor_id.as_deref(), Some("u-2001"));
        assert_eq!(recorded.tenant_id, "tenant-a");
        // org 事件**没有**前后值 → 明细放 `before` 的 `detail` 键里。
        assert_eq!(recorded.after, None);
        assert_eq!(
            recorded.before,
            Some(serde_json::json!({ "detail": "created the node n-1" }))
        );
    }

    /// 桥接**失败**必须**报错**（不得静默吞掉 —— 「以为在审计其实没有」）。
    #[tokio::test]
    async fn the_platform_sink_recorder_reports_a_failure() {
        struct FailingSink;
        impl AuditSink for FailingSink {
            fn record<'a>(
                &'a self,
                _ctx: &'a RequestContext,
                _event: &'a AuditEvent,
            ) -> BoxFuture<'a, Result<(), AuditError>> {
                Box::pin(async {
                    Err(AuditError::SinkUnavailable {
                        reason: "boom".to_string(),
                    })
                })
            }
        }
        let recorder = PlatformSinkAuditRecorder::new(Arc::new(FailingSink));
        let ctx = RequestContext::new().with_tenant("tenant-a");
        let event = OrgAuditEvent::new("org.node.create", "create", "org_node", "n-1");
        let result = recorder.record(&ctx, &event).await;
        assert!(result.is_err(), "写失败必须报出来");
    }
}
