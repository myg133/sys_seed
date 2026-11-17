//! 请求上下文骨架。
//!
//! 只放当前已确定的维度：租户、空间、追踪 ID、进入时刻。
//! REQ-006（多租户）会在此基础上扩展隔离维度与校验规则，
//! 平台中间件负责注入，业务模块不得手工解析请求头（`api-conventions v1` C3）。

use time::OffsetDateTime;
use uuid::Uuid;

/// 单次请求的上下文。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestContext {
    tenant_id: Option<String>,
    space_id: Option<String>,
    trace_id: String,
    entered_at: OffsetDateTime,
}

impl Default for RequestContext {
    fn default() -> Self {
        Self::new()
    }
}

impl RequestContext {
    /// 新建上下文：追踪 ID 自动生成，租户与空间留空。
    pub fn new() -> Self {
        Self {
            tenant_id: None,
            space_id: None,
            trace_id: new_trace_id(),
            entered_at: OffsetDateTime::now_utc(),
        }
    }

    /// 绑定租户；空值视为未绑定。
    pub fn with_tenant(mut self, tenant_id: impl Into<String>) -> Self {
        self.tenant_id = normalize(tenant_id.into());
        self
    }

    /// 绑定空间；空值视为不限空间（`api-conventions v1` C3）。
    pub fn with_space(mut self, space_id: impl Into<String>) -> Self {
        self.space_id = normalize(space_id.into());
        self
    }

    /// 租户 ID，未绑定时为 `None`。
    pub fn tenant_id(&self) -> Option<&str> {
        self.tenant_id.as_deref()
    }

    /// 空间 ID，不限空间时为 `None`。
    pub fn space_id(&self) -> Option<&str> {
        self.space_id.as_deref()
    }

    /// 追踪 ID，串联日志用。
    pub fn trace_id(&self) -> &str {
        &self.trace_id
    }

    /// 进入请求的时刻（UTC）。
    pub fn entered_at(&self) -> OffsetDateTime {
        self.entered_at
    }

    /// 从进入时刻算起的耗时。
    pub fn elapsed(&self) -> time::Duration {
        OffsetDateTime::now_utc() - self.entered_at
    }
}

fn new_trace_id() -> String {
    Uuid::new_v4().to_string()
}

fn normalize(value: String) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_context_has_trace_id_but_no_tenant() {
        let ctx = RequestContext::new();
        assert_eq!(ctx.tenant_id(), None);
        assert_eq!(ctx.space_id(), None);
        assert_eq!(ctx.trace_id().len(), 36, "UUID v4 字符串长度应为 36");
    }

    #[test]
    fn tenant_and_space_are_trimmed() {
        let ctx = RequestContext::new()
            .with_tenant("  tenant-a ")
            .with_space(" space-1 ");
        assert_eq!(ctx.tenant_id(), Some("tenant-a"));
        assert_eq!(ctx.space_id(), Some("space-1"));
    }

    #[test]
    fn blank_identifiers_mean_unbound() {
        let ctx = RequestContext::new().with_tenant("   ").with_space("");
        assert_eq!(ctx.tenant_id(), None);
        assert_eq!(ctx.space_id(), None);
    }

    #[test]
    fn trace_ids_are_unique_per_context() {
        let a = RequestContext::new();
        let b = RequestContext::new();
        assert_ne!(a.trace_id(), b.trace_id());
    }
}
