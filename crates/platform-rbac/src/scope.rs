//! 数据范围的求值（design-summary §4）。
//!
//! # 五种语义
//!
//! | scope | 语义 | 实现 |
//! |-------|------|------|
//! | `ALL` | 全部 | 无过滤 |
//! | `OWN_ORG` | 本部门 | 用户**主部门**节点 |
//! | `OWN_ORG_AND_BELOW` | 本部门及下级 | 主部门节点的**子树**（递归 CTE） |
//! | `SELF` | 仅本人 | `created_by = 当前用户` |
//! | `CUSTOM` | 自定义过滤 | 受限表达式 DSL |
//!
//! # 三条容易算错的口径
//!
//! 1. **兼岗默认按主部门**（`user_org.is_primary = true`）。
//!    取并集必须**显式开启**（`platform_role.use_all_memberships`）。
//!    默认行为一旦算错就是数据泄漏 —— 并集会让「本部门及下级」的可见范围
//!    被兼职部门撑大。
//! 2. **主部门缺失或歧义 → fail closed 到 `SELF`**（**不是** `ALL`）。
//!    DB 的部分唯一索引是**预防层**，这里的判定是**防御层** —— 两层都要。
//! 3. **多角色取最宽松**（**并集**语义）：多角色的可见范围过滤条件取并集 ——
//!    与权限点「任一满足即通过」一致（见 [`combine_scopes`]）。
//!    `CUSTOM` **不**被排除在并集之外：自定义条件与其它角色的范围取 **OR**，
//!    多个 `CUSTOM` 条件之间也是 **OR** —— **加一个角色不会缩小可见范围**。

use std::collections::BTreeSet;

use platform_core::RequestContext;
use platform_core::extension::expr::{
    CompiledCondition, EvalContext, FieldWhitelist, ValueKind, compile,
};
use platform_core::extension::permission::{DataScope, ScopeFilter};

use crate::error::RbacError;
use crate::store::RbacStore;

/// 组织节点列的约定名（业务表用它表达「这条记录归属哪个组织节点」）。
pub const ORG_NODE_COLUMN: &str = "org_node_id";
/// 创建人列的约定名（`SELF` 范围用它）。
pub const CREATED_BY_COLUMN: &str = "created_by";

/// 字段白名单：属性路径 → 绑定方式。
///
/// **白名单是「越界写法」的第一道闸** —— 不在白名单里的属性一律拒绝编译。
#[derive(Debug, Clone)]
pub struct ScopePolicy {
    /// 组织节点列名。
    pub org_node_column: String,
    /// 创建人列名。
    pub created_by_column: String,
    /// 记录字段：`(属性路径, 列名, 类型)`。
    pub record_fields: Vec<(String, String, ValueKind)>,
}

impl Default for ScopePolicy {
    fn default() -> Self {
        Self {
            org_node_column: ORG_NODE_COLUMN.to_string(),
            created_by_column: CREATED_BY_COLUMN.to_string(),
            record_fields: vec![
                ("record.id".to_string(), "id".to_string(), ValueKind::Text),
                (
                    "record.amount".to_string(),
                    "amount".to_string(),
                    ValueKind::Number,
                ),
                (
                    "record.dept_id".to_string(),
                    "dept_id".to_string(),
                    ValueKind::Text,
                ),
                (
                    "record.org_node_id".to_string(),
                    "org_node_id".to_string(),
                    ValueKind::Text,
                ),
                (
                    "record.created_by".to_string(),
                    "created_by".to_string(),
                    ValueKind::Text,
                ),
            ],
        }
    }
}

impl ScopePolicy {
    /// 构造字段白名单。
    pub fn whitelist(&self) -> FieldWhitelist {
        let mut whitelist = FieldWhitelist::empty()
            .with_user_id("user.id")
            .with_tenant_id("ctx.tenant_id");
        for (path, column, kind) in &self.record_fields {
            whitelist = whitelist.with_column(path, column, *kind);
        }
        whitelist
    }
}

/// 一个角色对最终范围的贡献。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeContribution {
    /// 角色级 scope。
    pub scope: DataScope,
    /// `CUSTOM` 的表达式。
    pub custom_filter: Option<String>,
    /// 是否取所有成员节点的并集。
    pub use_all_memberships: bool,
}

impl ScopeContribution {
    /// 构造一个贡献。
    pub fn new(scope: DataScope) -> Self {
        Self {
            scope,
            custom_filter: None,
            use_all_memberships: false,
        }
    }

    /// 带上 `CUSTOM` 表达式。
    pub fn with_filter(mut self, filter: impl Into<String>) -> Self {
        self.custom_filter = Some(filter.into());
        self
    }

    /// 带上「取并集」开关。
    pub fn with_all_memberships(mut self, enabled: bool) -> Self {
        self.use_all_memberships = enabled;
        self
    }
}

/// 多个角色合并后的范围。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CombinedScope {
    /// 全部。
    All,
    /// 本部门。
    OwnOrg {
        /// 是否取所有成员节点的并集。
        use_all_memberships: bool,
    },
    /// 本部门及下级。
    OwnOrgAndBelow {
        /// 是否取所有成员节点的并集。
        use_all_memberships: bool,
    },
    /// 仅本人。
    OnlySelf,
    /// 自定义条件 **OR** 包含型范围（**并集**语义）。
    Custom {
        /// 自定义表达式（每个角色一条；彼此 **OR** —— 不是交集）。
        filters: Vec<String>,
        /// 另一侧的范围（非 `CUSTOM` 角色里最宽松的那个）；`None` = 只有自定义条件。
        plus: Option<ContainmentScope>,
    },
}

/// 包含型范围（本部门 / 本部门及下级 / 仅本人）。
///
/// 与自定义条件的区别：包含型范围是「**某一组**组织节点（或本人）」，
/// 自定义条件是「任意表达式」—— 两者取**并集**（OR）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContainmentScope {
    /// 本部门（用户**主部门**节点）。
    OwnOrg {
        /// 是否取所有成员节点的并集。
        use_all_memberships: bool,
    },
    /// 本部门及下级（主部门节点的**子树**）。
    OwnOrgAndBelow {
        /// 是否取所有成员节点的并集。
        use_all_memberships: bool,
    },
    /// 仅本人（`created_by = 当前用户`）。
    OnlySelf,
}

impl CombinedScope {
    /// 是否使用了自定义范围（响应里要标明）。
    pub fn is_custom(&self) -> bool {
        matches!(self, Self::Custom { .. })
    }

    /// 合并后的 scope 语义名（用于响应里的 `dataScope` 字段）。
    pub fn scope_name(&self) -> &'static str {
        match self {
            Self::All => "ALL",
            Self::OwnOrg { .. } => "OWN_ORG",
            Self::OwnOrgAndBelow { .. } => "OWN_ORG_AND_BELOW",
            Self::OnlySelf => "SELF",
            Self::Custom { .. } => "CUSTOM",
        }
    }
}

/// 合并多个角色的范围（design-summary §4.4，**并集语义**）。
///
/// # 语义：多角色的可见范围取**并集**
///
/// 每个角色给一份授权（权限 + 一份可见范围过滤条件），多角色的过滤条件取**并集** ——
/// 与权限点「任一满足即通过」完全一致。
///
/// | 规则 | 结果 |
/// |------|------|
/// | 任一角色是 `ALL` | `ALL`（最宽松，其他都不必看） |
/// | 否则有 `CUSTOM` 角色 | **自定义条件 OR 其它（非 `CUSTOM`）角色的最宽松范围** |
/// | 否则有 `OWN_ORG_AND_BELOW` | `OWN_ORG_AND_BELOW`（最宽松） |
/// | 否则有 `OWN_ORG` | `OWN_ORG` |
/// | 否则 | `SELF` |
///
/// # 多个 `CUSTOM` 角色
///
/// 条件之间取 **OR**（并集），**不是 AND** —— 加一个角色**不会**缩小可见范围。
///
/// | 规则 | 说明 |
/// |------|------|
/// | `ALL` 最宽松 | 任何含 `ALL` 的角色组合 → `ALL` |
/// | `SELF` 最严格 | 只有 `SELF` → `SELF` |
/// | `OWN_ORG` ⊂ `OWN_ORG_AND_BELOW` | 同为「本部门」时，后者更宽松 |
/// | `CUSTOM` 与其它 scope | 取**并集**（`CUSTOM` 条件 **OR** 其它角色的范围） |
/// | 多 `CUSTOM` | 条件之间取 **OR**（并集），**不是**交集 |
pub fn combine_scopes(contributions: &[ScopeContribution]) -> CombinedScope {
    if contributions.is_empty() {
        return CombinedScope::OnlySelf;
    }

    // `ALL` 最宽松 —— 一个角色给了全部，整体就是全部。
    if contributions
        .iter()
        .any(|item| item.scope == DataScope::All)
    {
        return CombinedScope::All;
    }

    // 自定义条件（每个角色一条；彼此 **OR** —— 并集）。
    let filters: Vec<String> = contributions
        .iter()
        .filter(|item| item.scope == DataScope::Custom)
        .filter_map(|item| item.custom_filter.clone())
        .collect();

    // 非 `CUSTOM` 角色里**最宽松**的那个包含型范围。
    let plus = most_permissive_containment(contributions);

    if !filters.is_empty() {
        // 自定义条件 **OR** 其它角色的范围（并集）。
        return CombinedScope::Custom { filters, plus };
    }

    match plus {
        Some(ContainmentScope::OwnOrgAndBelow {
            use_all_memberships,
        }) => CombinedScope::OwnOrgAndBelow {
            use_all_memberships,
        },
        Some(ContainmentScope::OwnOrg {
            use_all_memberships,
        }) => CombinedScope::OwnOrg {
            use_all_memberships,
        },
        Some(ContainmentScope::OnlySelf) | None => CombinedScope::OnlySelf,
    }
}

/// 非 `CUSTOM` 贡献里**最宽松**的包含型范围。
///
/// 宽松顺序：`OWN_ORG_AND_BELOW` ⊃ `OWN_ORG` ⊃ `SELF`。
///
/// 「取所有成员节点的并集」的开关取自**所有**贡献（任一角色要求并集 → 整体并集）——
/// 与「取最宽松」的口径一致。
fn most_permissive_containment(contributions: &[ScopeContribution]) -> Option<ContainmentScope> {
    let use_all_memberships = contributions.iter().any(|item| item.use_all_memberships);

    if contributions
        .iter()
        .any(|item| item.scope == DataScope::OwnOrgAndBelow)
    {
        return Some(ContainmentScope::OwnOrgAndBelow {
            use_all_memberships,
        });
    }
    if contributions
        .iter()
        .any(|item| item.scope == DataScope::OwnOrg)
    {
        return Some(ContainmentScope::OwnOrg {
            use_all_memberships,
        });
    }
    if contributions
        .iter()
        .any(|item| item.scope == DataScope::OnlySelf)
    {
        return Some(ContainmentScope::OnlySelf);
    }
    None
}

/// 主部门判定结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrimaryDepartment {
    /// 唯一的主部门。
    Found(String),
    /// **没有**主部门 → fail closed 到 `SELF`。
    Missing,
    /// **多个**主部门（歧义）→ fail closed 到 `SELF`。
    Ambiguous,
}

/// 从成员关系里判定主部门。
///
/// **缺失与歧义都 fail closed**（返回 `Missing` / `Ambiguous`）——
/// 判定层是**防御层**，DB 的部分唯一索引是**预防层**，两层都要。
pub fn primary_department(memberships: &[crate::store::Membership]) -> PrimaryDepartment {
    let primaries: Vec<&str> = memberships
        .iter()
        .filter(|item| item.is_primary)
        .map(|item| item.org_node_id.as_str())
        .collect();
    match primaries.len() {
        0 => PrimaryDepartment::Missing,
        1 => PrimaryDepartment::Found(primaries[0].to_string()),
        _ => PrimaryDepartment::Ambiguous,
    }
}

/// 数据范围求值器。
pub struct ScopeResolver<'a> {
    store: &'a RbacStore,
    policy: &'a ScopePolicy,
}

impl<'a> ScopeResolver<'a> {
    /// 构造求值器。
    pub fn new(store: &'a RbacStore, policy: &'a ScopePolicy) -> Self {
        Self { store, policy }
    }

    /// 求值一个合并后的范围 → 可执行的过滤条件。
    pub async fn resolve(
        &self,
        ctx: &RequestContext,
        combined: &CombinedScope,
    ) -> Result<ScopeFilter, RbacError> {
        match combined {
            CombinedScope::All => Ok(ScopeFilter::All),
            CombinedScope::OnlySelf => Ok(self.self_filter(ctx)),
            CombinedScope::OwnOrg {
                use_all_memberships,
            } => {
                self.resolve_containment(
                    ctx,
                    &ContainmentScope::OwnOrg {
                        use_all_memberships: *use_all_memberships,
                    },
                )
                .await
            }
            CombinedScope::OwnOrgAndBelow {
                use_all_memberships,
            } => {
                self.resolve_containment(
                    ctx,
                    &ContainmentScope::OwnOrgAndBelow {
                        use_all_memberships: *use_all_memberships,
                    },
                )
                .await
            }
            CombinedScope::Custom { filters, plus } => {
                let conditions = self.compile_custom(ctx, filters)?;
                match plus {
                    // 只有自定义条件（多个条件之间是 **OR**）。
                    None => Ok(ScopeFilter::Custom { conditions }),
                    // 自定义条件 **OR** 包含型范围（并集）。
                    Some(containment) => {
                        let scope = self.resolve_containment(ctx, containment).await?;
                        Ok(ScopeFilter::CustomOr {
                            conditions,
                            scope: Box::new(scope),
                        })
                    }
                }
            }
        }
    }

    /// 求值一个包含型范围（本部门 / 本部门及下级 / 仅本人）。
    pub async fn resolve_containment(
        &self,
        ctx: &RequestContext,
        containment: &ContainmentScope,
    ) -> Result<ScopeFilter, RbacError> {
        match containment {
            ContainmentScope::OwnOrg {
                use_all_memberships,
            } => match self.org_nodes(ctx, *use_all_memberships, false).await? {
                OrgNodes::Nodes(nodes) => Ok(ScopeFilter::OrgNodes {
                    column: self.policy.org_node_column.clone(),
                    nodes,
                }),
                // 主部门缺失 / 歧义 → **退化为 SELF**（不是「什么都看不到」）。
                OrgNodes::DegradeToSelf => Ok(self.self_filter(ctx)),
            },
            ContainmentScope::OwnOrgAndBelow {
                use_all_memberships,
            } => match self.org_nodes(ctx, *use_all_memberships, true).await? {
                OrgNodes::Nodes(nodes) => Ok(ScopeFilter::OrgNodes {
                    column: self.policy.org_node_column.clone(),
                    nodes,
                }),
                OrgNodes::DegradeToSelf => Ok(self.self_filter(ctx)),
            },
            ContainmentScope::OnlySelf => Ok(self.self_filter(ctx)),
        }
    }

    /// 编译自定义条件（每个角色一条；彼此 **OR** —— 并集）。
    ///
    /// 白名单外的属性 → **编译失败** → fail closed（不放行）。
    fn compile_custom(
        &self,
        ctx: &RequestContext,
        filters: &[String],
    ) -> Result<Vec<CompiledCondition>, RbacError> {
        let whitelist = self.policy.whitelist();
        let eval = EvalContext {
            whitelist: &whitelist,
            user_id: ctx.user_id(),
            tenant_id: ctx.tenant_id(),
        };
        let mut conditions = Vec::with_capacity(filters.len());
        for filter in filters {
            // 白名单外的属性 → **编译失败** → fail closed（不放行）。
            let compiled = compile(filter, &eval).map_err(|error| RbacError::InvalidArgument {
                field: "customFilter".to_string(),
                reason: error.reason,
            })?;
            conditions.push(compiled);
        }
        Ok(conditions)
    }

    /// 求值单个 scope（[`PermissionChecker::scope`](platform_core::extension::permission::PermissionChecker::scope) 的实现体）。
    pub async fn resolve_one(
        &self,
        ctx: &RequestContext,
        scope: DataScope,
    ) -> Result<ScopeFilter, RbacError> {
        let contribution = ScopeContribution::new(scope);
        self.resolve(ctx, &combine_scopes(&[contribution])).await
    }

    /// `SELF` 的过滤条件（`created_by = 当前用户`）。
    fn self_filter(&self, ctx: &RequestContext) -> ScopeFilter {
        ScopeFilter::Owner {
            column: self.policy.created_by_column.clone(),
            user_id: ctx.user_id().unwrap_or_default().to_string(),
        }
    }

    /// 求值「本部门 / 本部门及下级」的组织节点集合。
    ///
    /// - 默认只取**主部门**；
    /// - `use_all_memberships = true` 时取**所有**成员节点（兼岗并集）；
    /// - 主部门**缺失或歧义** → **退化为 `SELF`**（不是 `ALL`，也不是「什么都看不到」）。
    async fn org_nodes(
        &self,
        ctx: &RequestContext,
        use_all_memberships: bool,
        include_subtree: bool,
    ) -> Result<OrgNodes, RbacError> {
        let user_id = ctx.user_id().unwrap_or_default();
        let memberships = self
            .store
            .list_memberships(ctx, user_id)
            .await
            .map_err(|error| RbacError::Internal {
                reason: format!("the organization membership could not be read: {error}"),
            })?;

        if memberships.is_empty() {
            // 没有成员关系 → 看不到任何部门数据（fail closed）。
            return Ok(OrgNodes::DegradeToSelf);
        }

        let roots: Vec<String> = if use_all_memberships {
            memberships
                .iter()
                .map(|item| item.org_node_id.clone())
                .collect()
        } else {
            match primary_department(&memberships) {
                PrimaryDepartment::Found(node) => vec![node],
                // 缺失 / 歧义 → **退化为 SELF**。
                PrimaryDepartment::Missing | PrimaryDepartment::Ambiguous => {
                    return Ok(OrgNodes::DegradeToSelf);
                }
            }
        };

        if roots.is_empty() {
            return Ok(OrgNodes::DegradeToSelf);
        }

        if !include_subtree {
            let mut unique: Vec<String> = roots;
            unique.sort();
            unique.dedup();
            return Ok(OrgNodes::Nodes(unique));
        }

        let mut nodes: BTreeSet<String> = BTreeSet::new();
        for root in roots {
            let subtree =
                self.store
                    .subtree(ctx, &root)
                    .await
                    .map_err(|error| RbacError::Internal {
                        reason: format!("the organization subtree could not be read: {error}"),
                    })?;
            nodes.extend(subtree);
        }
        Ok(OrgNodes::Nodes(nodes.into_iter().collect()))
    }
}

/// 「本部门」类范围的求值结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OrgNodes {
    /// 节点集合。
    Nodes(Vec<String>),
    /// 主部门缺失 / 歧义 → **退化为 `SELF`**。
    DegradeToSelf,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Membership;

    fn membership(node: &str, primary: bool) -> Membership {
        Membership {
            org_node_id: node.to_string(),
            is_primary: primary,
        }
    }

    #[test]
    fn the_primary_department_is_the_one_flagged_primary() {
        let memberships = vec![membership("dept-a", true), membership("dept-b", false)];
        assert_eq!(
            primary_department(&memberships),
            PrimaryDepartment::Found("dept-a".to_string())
        );
    }

    #[test]
    fn a_missing_primary_department_is_detected() {
        let memberships = vec![membership("dept-a", false)];
        assert_eq!(primary_department(&memberships), PrimaryDepartment::Missing);
    }

    #[test]
    fn an_ambiguous_primary_department_is_detected() {
        let memberships = vec![membership("dept-a", true), membership("dept-c", true)];
        assert_eq!(
            primary_department(&memberships),
            PrimaryDepartment::Ambiguous
        );
    }

    #[test]
    fn all_is_the_most_permissive_scope() {
        let combined = combine_scopes(&[
            ScopeContribution::new(DataScope::OwnOrg),
            ScopeContribution::new(DataScope::All),
        ]);
        assert_eq!(combined, CombinedScope::All);
    }

    #[test]
    fn own_org_and_below_is_more_permissive_than_own_org() {
        let combined = combine_scopes(&[
            ScopeContribution::new(DataScope::OwnOrg),
            ScopeContribution::new(DataScope::OwnOrgAndBelow),
        ]);
        assert_eq!(
            combined,
            CombinedScope::OwnOrgAndBelow {
                use_all_memberships: false
            }
        );
    }

    #[test]
    fn self_is_the_narrowest_scope() {
        let combined = combine_scopes(&[
            ScopeContribution::new(DataScope::OnlySelf),
            ScopeContribution::new(DataScope::OwnOrg),
        ]);
        assert_eq!(
            combined,
            CombinedScope::OwnOrg {
                use_all_memberships: false
            }
        );
    }

    #[test]
    fn multiple_customs_are_unioned_not_intersected() {
        let combined = combine_scopes(&[
            ScopeContribution::new(DataScope::Custom).with_filter("$record.amount <= 10000"),
            ScopeContribution::new(DataScope::Custom).with_filter("$record.dept_id == \"dept-a\""),
        ]);
        match combined {
            CombinedScope::Custom { filters, plus } => {
                assert_eq!(
                    filters.len(),
                    2,
                    "多个 CUSTOM 的条件都要参与（并集 / OR）：{filters:?}"
                );
                assert!(
                    plus.is_none(),
                    "只有 CUSTOM 角色时，另一侧没有包含型范围：{plus:?}"
                );
            }
            other => panic!("应当是 CUSTOM：{other:?}"),
        }
    }

    /// **本次修复的核心**：自定义条件 **OR** 其它角色的范围（并集）。
    ///
    /// 旧实现（有 CUSTOM 就只返回 CUSTOM）会丢掉 `OWN_ORG` 的贡献 ——
    /// 加一个角色反而**缩小**了可见范围。
    #[test]
    fn a_custom_scope_is_unioned_with_the_other_roles_scope() {
        let combined = combine_scopes(&[
            ScopeContribution::new(DataScope::OwnOrg),
            ScopeContribution::new(DataScope::Custom).with_filter("$record.amount <= 10000"),
        ]);
        match combined {
            CombinedScope::Custom { filters, plus } => {
                assert_eq!(filters, vec!["$record.amount <= 10000".to_string()]);
                assert_eq!(
                    plus,
                    Some(ContainmentScope::OwnOrg {
                        use_all_memberships: false
                    }),
                    "自定义条件必须与本部门范围取**并集**（OR）"
                );
            }
            other => panic!("应当是 CUSTOM + 包含型范围：{other:?}"),
        }
    }

    /// `OWN_ORG_AND_BELOW` 比 `OWN_ORG` 更宽松 → 取它。
    #[test]
    fn a_custom_scope_prefers_the_most_permissive_other_scope() {
        let combined = combine_scopes(&[
            ScopeContribution::new(DataScope::OwnOrg),
            ScopeContribution::new(DataScope::OwnOrgAndBelow),
            ScopeContribution::new(DataScope::Custom).with_filter("$record.amount <= 10000"),
        ]);
        match combined {
            CombinedScope::Custom { plus, .. } => assert_eq!(
                plus,
                Some(ContainmentScope::OwnOrgAndBelow {
                    use_all_memberships: false
                })
            ),
            other => panic!("应当是 CUSTOM + 包含型范围：{other:?}"),
        }
    }

    /// `ALL` 最宽松 —— 有 `ALL` 时不看别的。
    #[test]
    fn all_wins_over_custom() {
        let combined = combine_scopes(&[
            ScopeContribution::new(DataScope::All),
            ScopeContribution::new(DataScope::Custom).with_filter("$record.amount <= 10000"),
        ]);
        assert_eq!(combined, CombinedScope::All);
    }

    /// 没有自定义条件时，行为与旧实现一致（`OWN_ORG` + `OWN_ORG_AND_BELOW` → 后者）。
    #[test]
    fn the_union_flag_is_carried_over() {
        let combined =
            combine_scopes(&[ScopeContribution::new(DataScope::OwnOrg).with_all_memberships(true)]);
        assert_eq!(
            combined,
            CombinedScope::OwnOrg {
                use_all_memberships: true
            }
        );
    }

    #[test]
    fn no_role_means_self_not_all() {
        assert_eq!(combine_scopes(&[]), CombinedScope::OnlySelf);
    }
}
