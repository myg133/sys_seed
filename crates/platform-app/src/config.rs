//! 应用接入的**运行期配置**（REQ-016，design §6 / D274 / D276 / D277）。
//!
//! # 3 个配置键（**键名 lowerCamelCase**，契约 C9）
//!
//! | 键 | 类型 | 默认 | 说明 |
//! |----|------|------|------|
//! | `app.recentCount` | number | 6 | 「最近访问」与「常用应用」各取前 N |
//! | `app.workspace.maxApps` | number | 60 | 工作台最多展示多少应用（防一屏塞爆） |
//! | `app.icon.allowHttp` | boolean | false | URL 是否允许 `http`（仅本地开发设 true） |
//!
//! # ⚠️ 三个键**必须**是 `scope: "platform"`（D276 / QA 的 S-01）
//!
//! `ConfigSpec.scope` 的 serde 默认值是 **`Tenant`**
//! （`crates/platform-core/src/extension/manifest.rs` 的 `default_scope`）→
//! **不写 `"scope": "platform"` 就落到租户作用域** → 平台级读写端点直接拒
//! （`platform-config/src/service.rs` 的 `read_platform_entries` 只接受 `Platform`）
//! → AC-S-160 / AC-S-161 的「配置生效」**永不可达**。
//!
//! ⚠️ 这个失败**完全静默**：manifest 能解析、迁移能写库、编译能过 ——
//! 只有运行时读配置的路径在过滤 → 现象是「配置改了不生效」，最像业务 bug。
//! → AC-S-160 步骤 0 显式断言
//! `SELECT count(*) FROM platform_config WHERE key LIKE 'app.%' AND scope = 'platform'` == 3。
//!
//! # 读取路径：**每请求读**（不是装配时读一次）
//!
//! 配置由 `platform_config_upsert` 写进 `platform_config` →
//! 运行期由**装配层**注入的 [`AppConfigSource`] 读（实现是
//! `platform-config` 的 `ConfigService::read_platform_entries`，
//! 它自带 epoch 失效的缓存）。
//!
//! **每请求读**的语义是「配置改了**下一个请求**就生效」——
//! 装配时读一次 = 配置改了**永远不生效**（除非重启）。
//!
//! ⚠️ 本 crate **不依赖** `platform-config`（依赖方向单向）→
//! 用 trait 接缝（与 `platform-oauth` 的 `OAuthConfigSource` 同款）。

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, RwLock};

use platform_core::RequestContext;

/// 装箱的 future（让 trait 保持**可对象化** —— `async fn` 在 trait 上不可 `dyn`）。
pub type AppBoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// 「最近访问」与「常用应用」各取前 N 条。
pub const CONFIG_KEY_RECENT_COUNT: &str = "app.recentCount";
/// 工作台最多展示的应用数。
pub const CONFIG_KEY_WORKSPACE_MAX_APPS: &str = "app.workspace.maxApps";
/// URL 是否允许 `http`（仅本地开发）。
pub const CONFIG_KEY_ALLOW_HTTP: &str = "app.icon.allowHttp";

/// 全部配置键（**逐字**，顺序固定）。
pub const CONFIG_KEYS: [&str; 3] = [
    CONFIG_KEY_RECENT_COUNT,
    CONFIG_KEY_WORKSPACE_MAX_APPS,
    CONFIG_KEY_ALLOW_HTTP,
];

/// 配置读取失败（**不含任何配置值**）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AppConfigError;

/// 应用接入的运行期配置。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppSettings {
    /// 「最近访问」与「常用应用」各取前 N 条。
    pub recent_count: i64,
    /// 工作台最多展示的应用数。
    pub workspace_max_apps: i64,
    /// URL 是否允许 `http`（仅本地开发）。
    pub allow_http: bool,
}

impl Default for AppSettings {
    /// 默认值与 manifest 的 `config` 段**逐字一致**。
    fn default() -> Self {
        Self {
            recent_count: 6,
            workspace_max_apps: 60,
            allow_http: false,
        }
    }
}

impl AppSettings {
    /// 生效的「前 N 条」上限（**负数 / 0 归一成 1**，避免「配置写 0 就永远空」）。
    pub fn recent_limit(&self) -> usize {
        usize::try_from(self.recent_count.max(1)).unwrap_or(1)
    }

    /// 生效的工作台上限（同上）。
    pub fn workspace_limit(&self) -> usize {
        usize::try_from(self.workspace_max_apps.max(1)).unwrap_or(1)
    }
}

/// 配置的**读取源**（由装配层实现；本 crate **不依赖** `platform-config`）。
///
/// ⚠️ 读取失败必须返回 `Err`，**不得**降级成默认值 ——
/// 「读不出来就用默认」会让「改了配置不生效」变成一件看不见的事。
pub trait AppConfigSource: Send + Sync + 'static {
    /// 读取当前的有效配置。
    ///
    /// ⚠️ `self` 与 `ctx` 的生命周期**统一**成同一个 `'a` ——
    /// 实现里要把 `ctx` **跨 await** 传下去（读库），返回的 future
    /// 必须**同时**活得比两者久 → 不统一就编译不过。
    fn read<'a>(
        &'a self,
        ctx: &'a RequestContext,
    ) -> AppBoxFuture<'a, Result<AppSettings, AppConfigError>>;
}

/// 内存里的配置源（测试与「从别的来源已读好」的装配场景）。
#[derive(Debug, Clone, Default)]
pub struct StaticAppConfigSource {
    inner: Arc<RwLock<AppSettings>>,
}

impl StaticAppConfigSource {
    /// 用一份配置构造。
    pub fn new(settings: AppSettings) -> Self {
        Self {
            inner: Arc::new(RwLock::new(settings)),
        }
    }

    /// 替换配置。
    pub fn set(&self, settings: AppSettings) {
        if let Ok(mut guard) = self.inner.write() {
            *guard = settings;
        }
    }

    /// 读一份快照。
    pub fn snapshot(&self) -> AppSettings {
        self.inner
            .read()
            .map(|guard| guard.clone())
            .unwrap_or_default()
    }
}

impl AppConfigSource for StaticAppConfigSource {
    fn read<'a>(
        &'a self,
        _ctx: &'a RequestContext,
    ) -> AppBoxFuture<'a, Result<AppSettings, AppConfigError>> {
        Box::pin(async move { Ok(self.snapshot()) })
    }
}

/// **永远读失败**的配置源（fail closed 的测试夹具）。
#[derive(Debug, Clone, Copy, Default)]
pub struct FailingAppConfigSource;

impl AppConfigSource for FailingAppConfigSource {
    fn read<'a>(
        &'a self,
        _ctx: &'a RequestContext,
    ) -> AppBoxFuture<'a, Result<AppSettings, AppConfigError>> {
        Box::pin(async move { Err(AppConfigError) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_key_set_is_exactly_the_contract_set() {
        assert_eq!(CONFIG_KEYS.len(), 3);
        for key in CONFIG_KEYS {
            assert!(
                platform_core::config::ConfigKey::parse(key).is_ok(),
                "配置键必须是平台接受的形态（每段 lowerCamelCase）：{key}"
            );
            assert!(
                key.starts_with("app."),
                "本 crate 的配置键必须以 app. 开头：{key}"
            );
        }
    }

    /// 默认值与 manifest 的 `config` 段**逐字一致**（design §6 / D274）。
    #[test]
    fn the_defaults_are_pinned() {
        let settings = AppSettings::default();
        assert_eq!(settings.recent_count, 6);
        assert_eq!(settings.workspace_max_apps, 60);
        assert!(!settings.allow_http);
    }

    /// 上限**至少为 1** —— 配置写 0 / 负数也不让端点半条都不返回。
    #[test]
    fn the_limits_never_go_below_one() {
        let settings = AppSettings {
            recent_count: 0,
            workspace_max_apps: -5,
            allow_http: false,
        };
        assert_eq!(settings.recent_limit(), 1);
        assert_eq!(settings.workspace_limit(), 1);
    }

    /// 内存配置源可以替换配置（模拟「配置改了要生效」）。
    #[test]
    fn the_static_source_can_be_replaced() {
        let source = StaticAppConfigSource::default();
        assert_eq!(source.snapshot().recent_count, 6);
        source.set(AppSettings {
            recent_count: 1,
            ..AppSettings::default()
        });
        assert_eq!(source.snapshot().recent_count, 1);
    }
}
