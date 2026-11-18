//! 配置解析能力面（`kind = "platform-capability"`，**不是**扩展点）。
//!
//! # 定位（REQ-010 design-summary §1 / §6.3）
//!
//! 与 [`crate::extension::permission::PermissionChecker`] 同理：**平台实现、业务调用**，
//! 方向与 5 个扩展点相反。它住在 `extension/` 目录只是为了被能力索引收录
//! （`capabilities/extension-points.json` 的 `kind` 标成 `platform-capability`），
//! `extension_point_count` 仍然是 **5**。
//!
//! | | 5 个扩展点 | `ConfigResolver` |
//! |---|---|---|
//! | 谁实现 | 业务模块 | **平台**（`platform-config`） |
//! | 谁调用 | 平台 | **业务模块** |
//! | 能力索引分类 | `extension-point` | `platform-capability` |
//!
//! # 两个枚举，不是一个大枚举（D60）
//!
//! [`crate::config::ConfigScope`] 同时是 **manifest 的登记作用域**与**写入作用域**
//! （`platform` / `tenant` / `space`，**三值**）。
//! 响应里的 `source` 需要第四个取值 [`ConfigSource::Default`]
//! （谁都没配，用的是 manifest 登记的默认值）。
//!
//! 如果直接给 `ConfigScope` 加 `Default`：
//!
//! - manifest 就能写 `"scope": "default"`（serde 的 `lowercase` 直接放行）；
//! - 写入接口会接受「第 4 种作用域」—— 等于凭空开出一个**可写层**。
//!
//! 所以 `source` 用独立的 [`ConfigSource`]（**四值**），`ConfigScope` 保持三值。
//!
//! # 为什么方法是同步的
//!
//! 业务模块拿到的实现是 `Arc<dyn ConfigResolver>` —— 同步方法才能 dyn 兼容
//! （RPITIT 的 `async fn` 在 trait 上不可对象化，见 [`super`] 的模块文档）。
//! 数据访问（读 `platform_config_epoch` 判新鲜、必要时重算）由平台侧在**调用前**
//! 用异步的 `refresh` 完成（`platform-config` 的 `DbConfigResolver`）；
//! 这里只暴露「从已刷新的快照里同步取值」这一面。

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::config::{ConfigKey, ConfigScope};
use crate::context::RequestContext;

/// 配置值的**来源**（响应 `source` 字段，四值）。
///
/// 与 [`ConfigScope`] 的区别见模块文档：本枚举只用于「值是从哪一层来的」，
/// **不是**可写作用域 —— `Default` 永远不可写。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ConfigSource {
    /// 平台级显式配置。
    Platform,
    /// 租户级显式配置。
    Tenant,
    /// 空间级显式配置。
    Space,
    /// 谁都没配，用的是 manifest 登记的默认值。
    Default,
}

impl ConfigSource {
    /// 落库 / API 用的名字。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Platform => "platform",
            Self::Tenant => "tenant",
            Self::Space => "space",
            Self::Default => "default",
        }
    }

    /// 优先级权重，数值越大越优先（空间 > 租户 > 平台 > 默认）。
    ///
    /// `Default` 是**兜底**，权重比平台更低 —— 它只出现在前三层都没命中时。
    pub fn rank(self) -> u8 {
        match self {
            Self::Default => 0,
            Self::Platform => 1,
            Self::Tenant => 2,
            Self::Space => 3,
        }
    }
}

impl From<ConfigScope> for ConfigSource {
    fn from(scope: ConfigScope) -> Self {
        match scope {
            ConfigScope::Platform => Self::Platform,
            ConfigScope::Tenant => Self::Tenant,
            ConfigScope::Space => Self::Space,
        }
    }
}

/// 一条解析结果：键 + 值 + 命中层级。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedConfig {
    /// 配置键（与请求里的写法一致）。
    pub key: String,
    /// JSON 原生类型的值。
    pub value: Value,
    /// 命中的层级（四值）。
    pub source: ConfigSource,
}

/// 一个功能开关的判定结果（契约 C10）。
///
/// ⚠️ 关闭时也返回 `200` + `enabled=false`（不是 404）—— 客户端要能区分
/// 「功能关闭」与「接口不存在」，前者应优雅降级。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FlagDecision {
    /// flag 名（与请求一致，不做归一化）。
    pub flag: String,
    /// 是否启用。
    pub enabled: bool,
    /// 总开关取值的来源（`default` = 未配置、用 manifest 默认值）。
    pub source: ConfigSource,
}

impl FlagDecision {
    /// 未登记 / 无配置时的默认判定：关闭 + `default`。
    pub fn disabled(flag: impl Into<String>) -> Self {
        Self {
            flag: flag.into(),
            enabled: false,
            source: ConfigSource::Default,
        }
    }
}

/// 配置解析能力（**平台实现，业务模块调用**）。
///
/// 方法全是同步的（见模块文档）→ 可以 `Arc<dyn ConfigResolver>`。
pub trait ConfigResolver: Send + Sync + 'static {
    /// 解析一个配置键在当前上下文（租户 / 空间）下的有效值与其来源作用域。
    ///
    /// 键**未登记**时返回 `None`（HTTP 层据此返回 404，不是空值）。
    fn resolve(&self, ctx: &RequestContext, key: &ConfigKey) -> Option<ResolvedConfig>;

    /// 解析一个功能开关（含灰度比例）。
    ///
    /// 未登记的 flag **不返回 404**：返回 `enabled=false` + `source=default`
    /// （契约 C10 §10.4），由实现侧记录一条结构化 WARN。
    fn flag(&self, ctx: &RequestContext, flag: &str) -> FlagDecision;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_names_are_the_contract_four() {
        let names: Vec<&str> = [
            ConfigSource::Platform,
            ConfigSource::Tenant,
            ConfigSource::Space,
            ConfigSource::Default,
        ]
        .iter()
        .map(|source| source.as_str())
        .collect();
        assert_eq!(names, vec!["platform", "tenant", "space", "default"]);
    }

    /// `default` **不是**可写作用域：`ConfigScope` 只有三值。
    #[test]
    fn default_is_a_source_but_never_a_scope() {
        assert_eq!(
            serde_json::to_value(ConfigSource::Default).expect("应能序列化"),
            serde_json::json!("default")
        );
        assert!(
            serde_json::from_str::<ConfigScope>("\"default\"").is_err(),
            "ConfigScope 不得接受 default（否则开出第 4 个可写层）"
        );
    }

    #[test]
    fn source_rank_orders_space_over_tenant_over_platform_over_default() {
        assert!(ConfigSource::Space.rank() > ConfigSource::Tenant.rank());
        assert!(ConfigSource::Tenant.rank() > ConfigSource::Platform.rank());
        assert!(ConfigSource::Platform.rank() > ConfigSource::Default.rank());
    }

    #[test]
    fn a_disabled_decision_uses_the_default_source() {
        let decision = FlagDecision::disabled("beta");
        assert!(!decision.enabled);
        assert_eq!(decision.source, ConfigSource::Default);
        assert_eq!(decision.flag, "beta");
    }
}
