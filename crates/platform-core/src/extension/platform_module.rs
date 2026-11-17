//! 扩展点 1：模块注册。
//!
//! 业务模块实现 [`PlatformModule`]，并在编译期登记到平台的模块注册表
//! （见 [`crate::extension::registry`]）。

use anyhow::Result;

use crate::context::RequestContext;

/// 模块启动上下文。
///
/// `setup` 在模块启动时调用一次；模块级上下文复用平台的 [`RequestContext`]，
/// 因此平台注入的租户 / 空间 / 追踪 ID 与请求链路保持一致。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleContext {
    module_id: String,
    request: RequestContext,
}

impl ModuleContext {
    /// 新建模块上下文：追踪 ID 自动生成，租户与空间留空。
    pub fn new(module_id: impl Into<String>) -> Self {
        Self {
            module_id: module_id.into(),
            request: RequestContext::new(),
        }
    }

    /// 绑定租户。
    pub fn with_tenant(mut self, tenant_id: impl Into<String>) -> Self {
        self.request = self.request.with_tenant(tenant_id);
        self
    }

    /// 绑定空间。
    pub fn with_space(mut self, space_id: impl Into<String>) -> Self {
        self.request = self.request.with_space(space_id);
        self
    }

    /// 模块 ID。
    pub fn module_id(&self) -> &str {
        &self.module_id
    }

    /// 租户 ID，未绑定时为 `None`。
    pub fn tenant_id(&self) -> Option<&str> {
        self.request.tenant_id()
    }

    /// 空间 ID，不限空间时为 `None`。
    pub fn space_id(&self) -> Option<&str> {
        self.request.space_id()
    }

    /// 追踪 ID。
    pub fn trace_id(&self) -> &str {
        self.request.trace_id()
    }
}

/// 模块注册扩展点。
///
/// 契约测试 C-1 会逐项核对实现与 `manifest.json` 是否一致：
/// `id` / `version` / `enabled` / `dependencies`。
pub trait PlatformModule: Send + Sync + 'static {
    /// 必须与 `manifest.json` 的 `id` 一致，否则契约测试失败。
    fn id(&self) -> &'static str;

    /// 语义化版本，必须与 `manifest.json` 的 `version` 一致。
    fn version(&self) -> &'static str;

    /// 依赖的其他模块 ID。
    fn dependencies(&self) -> &[&'static str] {
        &[]
    }

    /// 启动时调用一次。
    fn setup(&self, ctx: &ModuleContext) -> Result<()> {
        let _ = ctx;
        Ok(())
    }

    /// 是否启用（对应 `manifest.json` 的 `enabled`）。
    fn enabled(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn module_context_exposes_module_id_and_trace_id() {
        let ctx = ModuleContext::new("sandbox")
            .with_tenant("tenant-a")
            .with_space("space-1");
        assert_eq!(ctx.module_id(), "sandbox");
        assert_eq!(ctx.tenant_id(), Some("tenant-a"));
        assert_eq!(ctx.space_id(), Some("space-1"));
        assert_eq!(ctx.trace_id().len(), 36);
    }

    #[test]
    fn default_trait_implementation_is_enabled_without_dependencies() {
        struct Demo;
        impl PlatformModule for Demo {
            fn id(&self) -> &'static str {
                "demo"
            }
            fn version(&self) -> &'static str {
                "0.1.0"
            }
        }

        let demo = Demo;
        assert!(demo.enabled());
        assert!(demo.dependencies().is_empty());
        assert!(demo.setup(&ModuleContext::new("demo")).is_ok());
    }
}
