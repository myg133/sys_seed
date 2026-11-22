//! 请求上下文骨架。
//!
//! 只放当前已确定的维度：租户、空间、追踪 ID、进入时刻，
//! 以及 REQ-014（D140）新增的来源 IP 与设备。
//! REQ-006（多租户）会在此基础上扩展隔离维度与校验规则，
//! 平台中间件负责注入，业务模块不得手工解析请求头（`api-conventions v1` C3）。

use time::OffsetDateTime;
use uuid::Uuid;

/// 单次请求的上下文。
///
/// # 用户身份
///
/// `user_id` 是**已认证的调用方**（REQ-007 的令牌主体）。
/// 权限判定（REQ-009）需要它 —— 「谁能做什么」里的「谁」。
///
/// 它由**认证中间件**写入（认证之前是 `None`）。
/// 业务模块**不得**手工设置它 —— 那等于自己声明「我是谁」。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestContext {
    tenant_id: Option<String>,
    space_id: Option<String>,
    user_id: Option<String>,
    trace_id: String,
    entered_at: OffsetDateTime,
    /// 来源 IP（客户端可控的 `x-forwarded-for` 第一跳）。
    client_ip: Option<String>,
    /// 设备（`user-agent`）。
    user_agent: Option<String>,
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
            user_id: None,
            trace_id: new_trace_id(),
            entered_at: OffsetDateTime::now_utc(),
            client_ip: None,
            user_agent: None,
        }
    }

    /// 绑定租户；空值视为未绑定。
    pub fn with_tenant(mut self, tenant_id: impl Into<String>) -> Self {
        self.tenant_id = normalize(tenant_id.into());
        self
    }

    /// 绑定 trace_id（REQ-019：入站 `X-Request-Id` 沿用时用）。
    ///
    /// ⚠️ 只接受**已校验**的值 —— 校验在 `platform-observability::trace`
    /// 里做（`normalize_trace_id`）。空值忽略（保留自动生成的）。
    ///
    /// 为什么不在 core 里做校验：core 是**所有 crate 的依赖**且**只依赖 uuid**，
    /// 校验规则属于可观测性域的语义 → 放在 `platform-observability`（D343）。
    pub fn with_trace_id(mut self, trace_id: impl Into<String>) -> Self {
        let value = trace_id.into();
        if !value.is_empty() {
            self.trace_id = value;
        }
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

    /// 绑定已认证的调用方；空值视为未认证。
    ///
    /// **只应由认证中间件调用** —— 业务模块自己调它等于自封身份。
    pub fn with_user(mut self, user_id: impl Into<String>) -> Self {
        self.user_id = normalize(user_id.into());
        self
    }

    /// 已认证的调用方 ID；未认证时为 `None`。
    pub fn user_id(&self) -> Option<&str> {
        self.user_id.as_deref()
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

    /// 绑定来源 IP；空值视为未知。
    ///
    /// # ⚠️ 这个值**不可信**（REQ-014 D149）
    ///
    /// 它来自**客户端可控的请求头**（`x-forwarded-for` 的第一跳），
    /// 本仓**无法**记录可信的物理来源。它的用途是**关联与排查**，
    /// **不是取证** —— 代码注释与规格都**不得**把它写成「真实客户端地址」。
    ///
    /// 由平台中间件填充；业务模块**不得**手工解析请求头（契约 C3）。
    pub fn with_client_ip(mut self, client_ip: impl Into<String>) -> Self {
        self.client_ip = normalize(client_ip.into());
        self
    }

    /// 来源 IP；未知时为 `None`（审计记录里落**空串**，不是 `NULL`）。
    pub fn client_ip(&self) -> Option<&str> {
        self.client_ip.as_deref()
    }

    /// 绑定设备（`user-agent`）；空值视为未知。
    pub fn with_user_agent(mut self, user_agent: impl Into<String>) -> Self {
        self.user_agent = normalize(user_agent.into());
        self
    }

    /// 设备；未知时为 `None`（审计记录里落**空串**，不是 `NULL`）。
    pub fn user_agent(&self) -> Option<&str> {
        self.user_agent.as_deref()
    }
}

/// 生成 trace_id：**32 位小写 hex**（16 字节随机）。
///
/// # ⚠️ REQ-019 D340：格式从「36 字符 uuid」改为「32 hex」
///
/// 理由：与 OpenTelemetry 的 `TraceId`（16 字节 / 32 hex）**同构** ——
/// 否则每次对接都要做一次字符串转换，而且日志里带连字符会让
/// 运维 `grep` trace 变麻烦。
///
/// 这是**对外可见的格式变化**（响应头 `X-Request-Id`、`ProblemDetails.trace_id`、
/// `/health/*` 的 `traceId` 都会跟着变）→ 属于跨需求改动（D109）。
fn new_trace_id() -> String {
    // ⚠️ 取 16 字节随机再转 hex（不用 `to_string()` —— 那是带连字符的 36 字符形态）。
    let bytes = Uuid::new_v4().into_bytes();
    let mut out = String::with_capacity(32);
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
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
        // ⚠️ REQ-019 D340：格式从 36 字符 uuid 改为 32 位小写 hex。
        // 这条断言**必须**同步改 —— 否则改了生成器它就会红（design §12 已点名）。
        assert_eq!(ctx.trace_id().len(), 32, "trace_id 必须是 32 位 hex");
        assert!(
            ctx.trace_id().bytes().all(|b| b.is_ascii_hexdigit()),
            "trace_id 必须全是 hex：{}",
            ctx.trace_id()
        );
        assert!(
            !ctx.trace_id().contains('-'),
            "trace_id 不得含连字符（uuid 形态）：{}",
            ctx.trace_id()
        );
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

    #[test]
    fn a_fresh_context_has_no_authenticated_user() {
        let ctx = RequestContext::new();
        assert_eq!(ctx.user_id(), None);
    }

    #[test]
    fn the_authenticated_user_is_bound_and_trimmed() {
        let ctx = RequestContext::new().with_user("  u-1001 ");
        assert_eq!(ctx.user_id(), Some("u-1001"));
        let blank = RequestContext::new().with_user("   ");
        assert_eq!(blank.user_id(), None, "空值视为未认证");
    }

    /// REQ-014 D140：新增的两个字段默认是 `None`，既有行为不变。
    #[test]
    fn a_fresh_context_has_no_client_ip_or_user_agent() {
        let ctx = RequestContext::new();
        assert_eq!(ctx.client_ip(), None);
        assert_eq!(ctx.user_agent(), None);
    }

    #[test]
    fn the_client_ip_and_user_agent_are_bound_and_trimmed() {
        let ctx = RequestContext::new()
            .with_client_ip(" 203.0.113.9 ")
            .with_user_agent(" audit-test-agent ");
        assert_eq!(ctx.client_ip(), Some("203.0.113.9"));
        assert_eq!(ctx.user_agent(), Some("audit-test-agent"));
        let blank = RequestContext::new()
            .with_client_ip("   ")
            .with_user_agent("");
        assert_eq!(blank.client_ip(), None);
        assert_eq!(blank.user_agent(), None);
    }
}
