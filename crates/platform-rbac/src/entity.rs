//! 角色模型的实体与权限点编码规则。
//!
//! # 角色模型（design-summary §3）
//!
//! | 概念 | 表 | 租户维度 |
//! |------|----|----------|
//! | 权限点注册 | `platform_permission` | **无**（平台级，权限点来自代码） |
//! | 角色 | `platform_role` | 有 |
//! | 角色-权限 | `platform_role_permission` | 有 |
//! | 角色继承 | `platform_role_inherit` | 有 |
//! | 用户-角色 | `platform_user_role` | 有 |
//!
//! # 权限点通配（design-summary §3.1）
//!
//! 允许的通配形态**只有两种**：`module:*` 与 `module:resource:*`。
//!
//! | 写法 | 判定 |
//! |------|------|
//! | `order:invoice:read` | 精确权限点 |
//! | `order:*` | 模块通配：匹配 `order:` 下的**全部**权限点 |
//! | `order:invoice:*` | 资源通配：匹配 `order:invoice:` 下的全部 |
//! | `*:*` | **拒绝** —— 不得出现「一个角色拿到全平台权限」的路径 |
//! | `*:invoice:read` | **拒绝** —— 跨模块通配 |

use platform_core::extension::manifest::PermissionType;
use platform_core::extension::permission::DataScope;

/// 内置角色：平台管理员（权限集合 = **注册表当前全部权限点**，动态得出）。
pub const BUILTIN_ADMIN: &str = "platform-admin";
/// 内置角色：普通用户（无特殊权限，数据范围 `SELF`）。
pub const BUILTIN_USER: &str = "platform-user";

/// 一个角色。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Role {
    /// 归属租户。
    pub tenant_id: String,
    /// 角色 ID（租户内唯一）。
    pub id: String,
    /// 角色编码（租户内唯一）。
    pub code: String,
    /// 角色名称。
    pub name: String,
    /// 角色级数据范围。
    pub data_scope: DataScope,
    /// `CUSTOM` 的过滤表达式（仅 `data_scope = CUSTOM` 时有效）。
    pub custom_filter: Option<String>,
    /// 是否取**所有**成员节点的并集（默认 `false` = 只按主部门）。
    pub use_all_memberships: bool,
    /// 是否内置角色（内置角色不可删除、不可降权）。
    pub builtin: bool,
}

impl Role {
    /// 内置管理员角色（数据范围 `ALL`）。
    pub fn builtin_admin(tenant_id: &str, id: &str) -> Self {
        Self {
            tenant_id: tenant_id.to_string(),
            id: id.to_string(),
            code: BUILTIN_ADMIN.to_string(),
            name: "平台管理员".to_string(),
            data_scope: DataScope::All,
            custom_filter: None,
            use_all_memberships: false,
            builtin: true,
        }
    }

    /// 内置普通用户角色（数据范围 `SELF`）。
    pub fn builtin_user(tenant_id: &str, id: &str) -> Self {
        Self {
            tenant_id: tenant_id.to_string(),
            id: id.to_string(),
            code: BUILTIN_USER.to_string(),
            name: "平台用户".to_string(),
            data_scope: DataScope::OnlySelf,
            custom_filter: None,
            use_all_memberships: false,
            builtin: true,
        }
    }
}

/// 角色-权限授权行。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RolePermission {
    /// 归属租户。
    pub tenant_id: String,
    /// 角色 ID。
    pub role_id: String,
    /// 权限点编码（**可以是通配**，如 `sandbox:*`）。
    pub permission_code: String,
    /// 权限点级数据范围（**可空 = 不覆盖角色级**）。
    pub data_scope: Option<DataScope>,
}

/// 角色继承行（`role_id` 继承 `parent_role_id`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoleInherit {
    /// 归属租户。
    pub tenant_id: String,
    /// 角色 ID。
    pub role_id: String,
    /// 被继承的角色 ID。
    pub parent_role_id: String,
}

/// 用户-角色绑定行。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserRole {
    /// 归属租户。
    pub tenant_id: String,
    /// 用户 ID。
    pub user_id: String,
    /// 角色 ID。
    pub role_id: String,
}

/// 注册表里的一条权限点。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Permission {
    /// 权限点编码。
    pub code: String,
    /// 权限点名称。
    pub name: String,
    /// 权限点类型。
    pub kind: PermissionType,
    /// 属性级校验条件（**可空** = 无条件）。
    pub condition: Option<String>,
    /// 来源 manifest 的路径（对账报错时要**逐条列出**来源）。
    pub source: String,
}

/// 一个权限点编码的形态。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PermissionPattern {
    /// 精确权限点：`module:resource:action`。
    Exact(String),
    /// 模块通配：`module:*`。
    ModuleWildcard(String),
    /// 资源通配：`module:resource:*`。
    ResourceWildcard {
        /// 模块段。
        module: String,
        /// 资源段。
        resource: String,
    },
}

/// 通配被拒时返回的原因。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatternError {
    /// 面向开发者的英文原因。
    pub reason: String,
}

impl PatternError {
    fn new(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
        }
    }
}

impl PermissionPattern {
    /// 解析一个权限点编码（含通配）。
    ///
    /// `*:*` 与跨模块通配（`*:resource:action`）**一律拒绝**。
    pub fn parse(value: &str) -> Result<Self, PatternError> {
        let segments: Vec<&str> = value.split(':').collect();
        match segments.as_slice() {
            [module, "*"] => {
                if *module == "*" {
                    return Err(PatternError::new(
                        "the `*:*` wildcard grants every permission on the platform and is not allowed",
                    ));
                }
                if !is_segment(module) {
                    return Err(PatternError::new(
                        "the module segment must match ^[a-z][a-z0-9-]*$",
                    ));
                }
                Ok(Self::ModuleWildcard((*module).to_string()))
            }
            [module, resource, action] => {
                if *module == "*" {
                    return Err(PatternError::new(
                        "a cross-module wildcard (for example `*:demo:read`) is not allowed",
                    ));
                }
                if !is_segment(module) || !is_segment(resource) {
                    return Err(PatternError::new(
                        "the module and resource segments must match ^[a-z][a-z0-9-]*$",
                    ));
                }
                if *action == "*" {
                    return Ok(Self::ResourceWildcard {
                        module: (*module).to_string(),
                        resource: (*resource).to_string(),
                    });
                }
                if !is_segment(action) {
                    return Err(PatternError::new(
                        "the action segment must match ^[a-z][a-z0-9-]*$",
                    ));
                }
                Ok(Self::Exact(value.to_string()))
            }
            _ => Err(PatternError::new(
                "a permission code must be `module:resource:action`, `module:*` or `module:resource:*`",
            )),
        }
    }

    /// 是否是通配。
    pub fn is_wildcard(&self) -> bool {
        !matches!(self, Self::Exact(_))
    }

    /// 该形态覆盖的**精确**权限点（对着注册表求值）。
    pub fn matches(&self, code: &str) -> bool {
        match self {
            Self::Exact(exact) => exact == code,
            Self::ModuleWildcard(module) => code.starts_with(&format!("{module}:")),
            Self::ResourceWildcard { module, resource } => {
                code.starts_with(&format!("{module}:{resource}:"))
            }
        }
    }

    /// 该形态覆盖注册表里的哪些权限点。
    pub fn expand(&self, registry: &[String]) -> Vec<String> {
        let mut matched: Vec<String> = registry
            .iter()
            .filter(|code| self.matches(code))
            .cloned()
            .collect();
        matched.sort();
        matched.dedup();
        matched
    }
}

/// 一段权限点编码：`^[a-z][a-z0-9-]*$`。
fn is_segment(value: &str) -> bool {
    let mut chars = value.chars();
    match chars.next() {
        Some(first) if first.is_ascii_lowercase() => {
            chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registry() -> Vec<String> {
        vec![
            "org:org-node:read".to_string(),
            "org:org-node:write".to_string(),
            "sandbox:demo:read".to_string(),
            "sandbox:demo:write".to_string(),
            "sandbox:scope-item:read".to_string(),
        ]
    }

    #[test]
    fn an_exact_code_is_recognised() {
        let pattern = PermissionPattern::parse("sandbox:demo:read").expect("应当能解析");
        assert_eq!(
            pattern,
            PermissionPattern::Exact("sandbox:demo:read".to_string())
        );
        assert!(!pattern.is_wildcard());
    }

    #[test]
    fn the_module_wildcard_covers_only_its_own_module() {
        let pattern = PermissionPattern::parse("sandbox:*").expect("应当能解析");
        assert!(pattern.is_wildcard());
        let matched = pattern.expand(&registry());
        assert!(matched.contains(&"sandbox:demo:read".to_string()));
        assert!(matched.contains(&"sandbox:scope-item:read".to_string()));
        assert!(
            !matched.contains(&"org:org-node:read".to_string()),
            "`sandbox:*` 不得覆盖 `org:` 下的权限点：{matched:?}"
        );
    }

    #[test]
    fn the_resource_wildcard_covers_only_its_own_resource() {
        let pattern = PermissionPattern::parse("sandbox:demo:*").expect("应当能解析");
        let matched = pattern.expand(&registry());
        assert_eq!(
            matched,
            vec![
                "sandbox:demo:read".to_string(),
                "sandbox:demo:write".to_string()
            ]
        );
    }

    #[test]
    fn the_all_wildcard_is_rejected() {
        let error = PermissionPattern::parse("*:*").expect_err("`*:*` 必须被拒");
        assert!(
            error.reason.contains("not allowed"),
            "错误原因要说清是禁止全通过：{}",
            error.reason
        );
    }

    #[test]
    fn a_cross_module_wildcard_is_rejected() {
        let error = PermissionPattern::parse("*:demo:read").expect_err("跨模块通配必须被拒");
        assert!(
            error.reason.contains("cross-module"),
            "错误原因要说清是跨模块通配：{}",
            error.reason
        );
    }

    #[test]
    fn a_malformed_code_is_rejected() {
        assert!(PermissionPattern::parse("sandbox").is_err());
        assert!(PermissionPattern::parse("sandbox:demo").is_err());
        assert!(PermissionPattern::parse("Sandbox:demo:read").is_err());
        assert!(PermissionPattern::parse("sandbox:demo:read:extra").is_err());
    }

    #[test]
    fn the_builtin_roles_have_the_specified_scopes() {
        let admin = Role::builtin_admin("tenant-a", "r-admin");
        assert_eq!(admin.data_scope, DataScope::All);
        assert!(admin.builtin);
        let user = Role::builtin_user("tenant-a", "r-user");
        assert_eq!(user.data_scope, DataScope::OnlySelf);
        assert!(user.builtin);
        assert!(!user.use_all_memberships, "取并集必须显式开启");
    }
}
