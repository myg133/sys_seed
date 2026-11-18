//! 角色与权限的业务逻辑。
//!
//! # 三条「不能算错」的口径
//!
//! 1. **内置角色受保护**：`platform-admin` / `platform-user` 不可删除、不可降权
//!    （改 `data_scope` 或改权限点都被拒）。
//! 2. **角色继承不得成环**：写入时检测，错误信息**指出是哪两个角色构成的环**。
//! 3. **通配只有两种形态**：`module:*` 与 `module:resource:*`；
//!    `*:*` 与跨模块通配一律拒绝。
//!
//! # `platform-admin` 的权限集合是「注册表全集」
//!
//! **不是**通配 `*`，**也不是**写死的一组权限点 —— 查询时**动态**取注册表当前的全部权限点。
//! 新增一个权限点，它**自动**拥有（不靠人工补授权行）。

use std::collections::{BTreeMap, BTreeSet};

use platform_core::RequestContext;
use platform_core::extension::permission::DataScope;

use crate::entity::{BUILTIN_ADMIN, BUILTIN_USER, PermissionPattern, Role, RolePermission};
use crate::error::RbacError;
use crate::scope::{CombinedScope, ScopeContribution, ScopePolicy};
use crate::store::{RbacStore, RoleRow};

/// 角色与权限的业务服务。
#[derive(Debug, Clone)]
pub struct RbacService {
    store: RbacStore,
    policy: ScopePolicy,
}

/// 一个角色的视角（供 HTTP 层返回）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoleView {
    /// 角色 ID。
    pub id: String,
    /// 角色编码。
    pub code: String,
    /// 角色名称。
    pub name: String,
    /// 角色级数据范围。
    pub data_scope: String,
    /// `CUSTOM` 的过滤表达式。
    pub custom_filter: Option<String>,
    /// 是否取所有成员节点的并集。
    pub use_all_memberships: bool,
    /// 是否内置角色。
    pub builtin: bool,
    /// 展开后的权限点编码（**逐个列出**，不只给数量）。
    pub permissions: Vec<String>,
    /// 展开后的数据范围（内置管理员 = 注册表全集）。
    pub effective_permissions: Vec<String>,
}

/// 一个用户的有效权限视角。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserPermissionView {
    /// 用户 ID。
    pub user_id: String,
    /// 有效权限点编码（已展开，逐个列出）。
    pub permissions: Vec<String>,
    /// 某个权限点的有效数据范围。
    pub data_scope: String,
    /// 是否使用了自定义范围（响应里要标明）。
    pub uses_custom_scope: bool,
}

/// 角色的继承图（租户内）。
#[derive(Debug, Clone, Default)]
pub struct RoleGraph {
    /// 角色（按 ID 索引）。
    pub roles: BTreeMap<String, RoleRow>,
    /// 每个角色的授权行。
    pub permissions: BTreeMap<String, Vec<crate::store::RolePermissionRow>>,
    /// 每个角色的父角色（被继承的角色）。
    pub inherits: BTreeMap<String, Vec<String>>,
    /// 用户 → 直接绑定的角色 ID。
    pub role_bindings: BTreeMap<String, Vec<String>>,
}

impl RoleGraph {
    /// 角色及其**全部**祖先（传递继承）。
    pub fn ancestors(&self, role_id: &str) -> BTreeSet<String> {
        let mut seen = BTreeSet::new();
        let mut stack = vec![role_id.to_string()];
        while let Some(current) = stack.pop() {
            if !seen.insert(current.clone()) {
                continue;
            }
            if let Some(parents) = self.inherits.get(&current) {
                stack.extend(parents.iter().cloned());
            }
        }
        seen
    }

    /// 某个角色的**全部**授权行（含继承来的）。
    pub fn grants_of(&self, role_id: &str) -> Vec<crate::store::RolePermissionRow> {
        let mut rows = Vec::new();
        for ancestor in self.ancestors(role_id) {
            if let Some(grants) = self.permissions.get(&ancestor) {
                rows.extend(grants.iter().cloned());
            }
        }
        rows
    }

    /// 角色的编码（错误信息里要用）。
    pub fn code_of(&self, role_id: &str) -> String {
        self.roles
            .get(role_id)
            .map(|role| role.code.clone())
            .unwrap_or_else(|| role_id.to_string())
    }
}

impl RbacService {
    /// 构造服务。
    pub fn new(store: RbacStore, policy: ScopePolicy) -> Self {
        Self { store, policy }
    }

    /// 底层的数据访问。
    pub fn store(&self) -> &RbacStore {
        &self.store
    }

    /// 范围策略。
    pub fn policy(&self) -> &ScopePolicy {
        &self.policy
    }

    // -- 权限点注册表 ----------------------------------------------------

    /// 读权限点注册表（**任何已认证用户都可读** —— 前端 `can()` 靠它）。
    pub async fn list_permissions(&self) -> Result<Vec<crate::entity::Permission>, RbacError> {
        let rows = self
            .store
            .list_permissions()
            .await
            .map_err(|error| RbacError::Internal {
                reason: format!("the permission registry could not be read: {error}"),
            })?;
        Ok(rows.iter().filter_map(|row| row.to_permission()).collect())
    }

    /// 注册表里的权限点编码（已排序）。
    pub async fn registry_codes(&self) -> Result<Vec<String>, RbacError> {
        self.store
            .permission_codes()
            .await
            .map_err(|error| RbacError::Internal {
                reason: format!("the permission registry could not be read: {error}"),
            })
    }

    // -- 角色 ------------------------------------------------------------

    /// 列出本租户的角色。
    pub async fn list_roles(&self, ctx: &RequestContext) -> Result<Vec<RoleView>, RbacError> {
        // 内置角色**按租户物化** —— 第一次用到本租户时补齐（幂等）。
        self.ensure_builtin_roles(ctx).await?;
        let rows = self
            .store
            .list_roles(ctx)
            .await
            .map_err(|error| RbacError::Internal {
                reason: format!("the roles could not be read: {error}"),
            })?;
        let registry = self.registry_codes().await?;
        let graph = self.load_graph(ctx).await?;
        let mut views = Vec::new();
        for row in &rows {
            views.push(self.to_role_view(row, &graph, &registry).await?);
        }
        Ok(views)
    }

    /// 按 ID 读角色（**跨租户 id → 404**：不泄露存在性）。
    pub async fn get_role(
        &self,
        ctx: &RequestContext,
        role_id: &str,
    ) -> Result<RoleView, RbacError> {
        let row = self
            .store
            .find_role(ctx, role_id)
            .await
            .map_err(|error| RbacError::Internal {
                reason: format!("the role could not be read: {error}"),
            })?
            .ok_or(RbacError::NotFound { resource: "role" })?;
        let registry = self.registry_codes().await?;
        let graph = self.load_graph(ctx).await?;
        self.to_role_view(&row, &graph, &registry).await
    }

    /// 角色的视角（展开权限点）。
    async fn to_role_view(
        &self,
        row: &RoleRow,
        graph: &RoleGraph,
        registry: &[String],
    ) -> Result<RoleView, RbacError> {
        let mut permissions: Vec<String> = Vec::new();
        for grant in graph.grants_of(&row.id) {
            let pattern = PermissionPattern::parse(&grant.permission_code).map_err(|error| {
                RbacError::WildcardForbidden {
                    code: grant.permission_code.clone(),
                    reason: error.reason,
                }
            })?;
            for code in pattern.expand(registry) {
                if !permissions.contains(&code) {
                    permissions.push(code);
                }
            }
        }
        permissions.sort();

        // 内置管理员 = **注册表全集**（动态得出，不靠人工补授权）。
        let effective_permissions = if row.builtin && row.code == BUILTIN_ADMIN {
            registry.to_vec()
        } else {
            permissions.clone()
        };

        Ok(RoleView {
            id: row.id.clone(),
            code: row.code.clone(),
            name: row.name.clone(),
            data_scope: row.data_scope.clone(),
            custom_filter: row.custom_filter.clone(),
            use_all_memberships: row.use_all_memberships,
            builtin: row.builtin,
            permissions,
            effective_permissions,
        })
    }

    /// 建角色。
    pub async fn create_role(
        &self,
        ctx: &RequestContext,
        role: &Role,
    ) -> Result<RoleView, RbacError> {
        if role.data_scope == DataScope::Custom && role.custom_filter.is_none() {
            return Err(RbacError::InvalidArgument {
                field: "customFilter".to_string(),
                reason: "a CUSTOM data scope requires a filter expression".to_string(),
            });
        }
        if role.data_scope != DataScope::Custom && role.custom_filter.is_some() {
            return Err(RbacError::InvalidArgument {
                field: "customFilter".to_string(),
                reason: "a filter expression is only allowed with the CUSTOM data scope"
                    .to_string(),
            });
        }
        if role.builtin {
            return Err(RbacError::InvalidArgument {
                field: "builtin".to_string(),
                reason: "built-in roles are created by the platform, not by the API".to_string(),
            });
        }

        let mut tx = self
            .store
            .db()
            .begin(ctx)
            .await
            .map_err(|error| RbacError::Internal {
                reason: format!("the transaction could not be started: {error}"),
            })?;
        self.store
            .insert_role(&mut tx, role)
            .await
            .map_err(|error| RbacError::Conflict {
                reason: format!("the role could not be created: {error}"),
            })?;
        // 写操作 → **同一事务里** epoch + 1（主动失效）。
        self.store
            .bump_epoch(&mut tx)
            .await
            .map_err(|error| RbacError::Internal {
                reason: format!("the cache epoch could not be bumped: {error}"),
            })?;
        tx.commit().await.map_err(|error| RbacError::Internal {
            reason: format!("the transaction could not be committed: {error}"),
        })?;

        self.get_role(ctx, &role.id).await
    }

    /// 删角色（**内置角色受保护**）。
    pub async fn delete_role(&self, ctx: &RequestContext, role_id: &str) -> Result<(), RbacError> {
        let row = self.require_role(ctx, role_id).await?;
        if row.builtin {
            return Err(RbacError::BuiltinRoleProtected {
                operation: "deleted",
                role_code: row.code.clone(),
            });
        }
        let mut tx = self
            .store
            .db()
            .begin(ctx)
            .await
            .map_err(|error| RbacError::Internal {
                reason: format!("the transaction could not be started: {error}"),
            })?;
        self.store
            .delete_role(&mut tx, role_id)
            .await
            .map_err(|error| RbacError::Internal {
                reason: format!("the role could not be deleted: {error}"),
            })?;
        self.store
            .bump_epoch(&mut tx)
            .await
            .map_err(|error| RbacError::Internal {
                reason: format!("the cache epoch could not be bumped: {error}"),
            })?;
        tx.commit().await.map_err(|error| RbacError::Internal {
            reason: format!("the transaction could not be committed: {error}"),
        })?;
        Ok(())
    }

    /// 改角色的数据范围 / 自定义条件 / 并集开关（**内置角色不可降权**）。
    pub async fn update_role_scope(
        &self,
        ctx: &RequestContext,
        role_id: &str,
        data_scope: DataScope,
        custom_filter: Option<&str>,
        use_all_memberships: bool,
    ) -> Result<RoleView, RbacError> {
        let row = self.require_role(ctx, role_id).await?;
        if row.builtin {
            return Err(RbacError::BuiltinRoleProtected {
                operation: "downgraded",
                role_code: row.code.clone(),
            });
        }
        if data_scope == DataScope::Custom && custom_filter.is_none() {
            return Err(RbacError::InvalidArgument {
                field: "customFilter".to_string(),
                reason: "a CUSTOM data scope requires a filter expression".to_string(),
            });
        }

        let mut tx = self
            .store
            .db()
            .begin(ctx)
            .await
            .map_err(|error| RbacError::Internal {
                reason: format!("the transaction could not be started: {error}"),
            })?;
        self.store
            .update_role_scope(
                &mut tx,
                role_id,
                data_scope,
                custom_filter,
                use_all_memberships,
            )
            .await
            .map_err(|error| RbacError::Internal {
                reason: format!("the role could not be updated: {error}"),
            })?;
        self.store
            .bump_epoch(&mut tx)
            .await
            .map_err(|error| RbacError::Internal {
                reason: format!("the cache epoch could not be bumped: {error}"),
            })?;
        tx.commit().await.map_err(|error| RbacError::Internal {
            reason: format!("the transaction could not be committed: {error}"),
        })?;
        self.get_role(ctx, role_id).await
    }

    // -- 授权 ------------------------------------------------------------

    /// 给角色授权。
    pub async fn grant_permission(
        &self,
        ctx: &RequestContext,
        role_id: &str,
        permission_code: &str,
        data_scope: Option<DataScope>,
    ) -> Result<(), RbacError> {
        let row = self.require_role(ctx, role_id).await?;
        if row.builtin {
            // 内置管理员的权限 = 注册表全集（动态），**不能**手工增删。
            return Err(RbacError::BuiltinRoleProtected {
                operation: "granted",
                role_code: row.code.clone(),
            });
        }

        // 通配形态校验：`*:*` 与跨模块通配一律拒绝。
        let pattern = PermissionPattern::parse(permission_code).map_err(|error| {
            RbacError::WildcardForbidden {
                code: permission_code.to_string(),
                reason: error.reason,
            }
        })?;

        // 精确权限点必须在注册表里；通配必须有匹配项（否则是「假权限」）。
        let registry = self.registry_codes().await?;
        if pattern.expand(&registry).is_empty() {
            return Err(RbacError::InvalidArgument {
                field: "permissionCode".to_string(),
                reason: format!("the permission `{permission_code}` is not in the registry"),
            });
        }

        let mut tx = self
            .store
            .db()
            .begin(ctx)
            .await
            .map_err(|error| RbacError::Internal {
                reason: format!("the transaction could not be started: {error}"),
            })?;
        self.store
            .grant_permission(&mut tx, role_id, permission_code, data_scope)
            .await
            .map_err(|error| RbacError::Internal {
                reason: format!("the permission could not be granted: {error}"),
            })?;
        self.store
            .bump_epoch(&mut tx)
            .await
            .map_err(|error| RbacError::Internal {
                reason: format!("the cache epoch could not be bumped: {error}"),
            })?;
        tx.commit().await.map_err(|error| RbacError::Internal {
            reason: format!("the transaction could not be committed: {error}"),
        })?;
        Ok(())
    }

    /// 撤销授权。
    pub async fn revoke_permission(
        &self,
        ctx: &RequestContext,
        role_id: &str,
        permission_code: &str,
    ) -> Result<(), RbacError> {
        let row = self.require_role(ctx, role_id).await?;
        if row.builtin {
            return Err(RbacError::BuiltinRoleProtected {
                operation: "revoked",
                role_code: row.code.clone(),
            });
        }
        let mut tx = self
            .store
            .db()
            .begin(ctx)
            .await
            .map_err(|error| RbacError::Internal {
                reason: format!("the transaction could not be started: {error}"),
            })?;
        self.store
            .revoke_permission(&mut tx, role_id, permission_code)
            .await
            .map_err(|error| RbacError::Internal {
                reason: format!("the permission could not be revoked: {error}"),
            })?;
        self.store
            .bump_epoch(&mut tx)
            .await
            .map_err(|error| RbacError::Internal {
                reason: format!("the cache epoch could not be bumped: {error}"),
            })?;
        tx.commit().await.map_err(|error| RbacError::Internal {
            reason: format!("the transaction could not be committed: {error}"),
        })?;
        Ok(())
    }

    /// 角色的授权明细。
    pub async fn list_role_permissions(
        &self,
        ctx: &RequestContext,
        role_id: &str,
    ) -> Result<Vec<RolePermission>, RbacError> {
        self.require_role(ctx, role_id).await?;
        let rows = self
            .store
            .list_permissions_of_role(ctx, role_id)
            .await
            .map_err(|error| RbacError::Internal {
                reason: format!("the role permissions could not be read: {error}"),
            })?;
        Ok(rows
            .into_iter()
            .map(|row| RolePermission {
                tenant_id: row.tenant_id,
                role_id: row.role_id,
                permission_code: row.permission_code,
                data_scope: row.data_scope.and_then(|scope| DataScope::parse(&scope)),
            })
            .collect())
    }

    // -- 继承 ------------------------------------------------------------

    /// 设置角色继承（**成环则拒绝**，并指出是哪两个角色构成的环）。
    pub async fn set_inherit(
        &self,
        ctx: &RequestContext,
        role_id: &str,
        parent_role_id: &str,
    ) -> Result<(), RbacError> {
        let child = self.require_role(ctx, role_id).await?;
        let parent = self.require_role(ctx, parent_role_id).await?;
        if child.id == parent.id {
            return Err(RbacError::InheritanceCycle {
                roles: vec![child.code.clone(), parent.code.clone()],
            });
        }

        let graph = self.load_graph(ctx).await?;
        if let Some(cycle) = detect_cycle(&graph, &child.id, &parent.id) {
            return Err(RbacError::InheritanceCycle {
                roles: cycle.into_iter().map(|id| graph.code_of(&id)).collect(),
            });
        }

        let mut tx = self
            .store
            .db()
            .begin(ctx)
            .await
            .map_err(|error| RbacError::Internal {
                reason: format!("the transaction could not be started: {error}"),
            })?;
        self.store
            .add_inherit(&mut tx, role_id, parent_role_id)
            .await
            .map_err(|error| RbacError::Internal {
                reason: format!("the inheritance could not be created: {error}"),
            })?;
        self.store
            .bump_epoch(&mut tx)
            .await
            .map_err(|error| RbacError::Internal {
                reason: format!("the cache epoch could not be bumped: {error}"),
            })?;
        tx.commit().await.map_err(|error| RbacError::Internal {
            reason: format!("the transaction could not be committed: {error}"),
        })?;
        Ok(())
    }

    /// 删除角色继承。
    pub async fn remove_inherit(
        &self,
        ctx: &RequestContext,
        role_id: &str,
        parent_role_id: &str,
    ) -> Result<(), RbacError> {
        self.require_role(ctx, role_id).await?;
        let mut tx = self
            .store
            .db()
            .begin(ctx)
            .await
            .map_err(|error| RbacError::Internal {
                reason: format!("the transaction could not be started: {error}"),
            })?;
        self.store
            .remove_inherit(&mut tx, role_id, parent_role_id)
            .await
            .map_err(|error| RbacError::Internal {
                reason: format!("the inheritance could not be removed: {error}"),
            })?;
        self.store
            .bump_epoch(&mut tx)
            .await
            .map_err(|error| RbacError::Internal {
                reason: format!("the cache epoch could not be bumped: {error}"),
            })?;
        tx.commit().await.map_err(|error| RbacError::Internal {
            reason: format!("the transaction could not be committed: {error}"),
        })?;
        Ok(())
    }

    // -- 用户-角色绑定 ---------------------------------------------------

    /// 绑定用户角色。
    pub async fn bind_user_role(
        &self,
        ctx: &RequestContext,
        user_id: &str,
        role_id: &str,
    ) -> Result<(), RbacError> {
        // 内置角色按租户物化：绑定 `platform-admin` 之前先把它们建出来。
        self.ensure_builtin_roles(ctx).await?;
        self.require_role(ctx, role_id).await?;
        let mut tx = self
            .store
            .db()
            .begin(ctx)
            .await
            .map_err(|error| RbacError::Internal {
                reason: format!("the transaction could not be started: {error}"),
            })?;
        self.store
            .bind_user_role(&mut tx, user_id, role_id)
            .await
            .map_err(|error| RbacError::Conflict {
                reason: format!("the role binding could not be created: {error}"),
            })?;
        self.store
            .bump_epoch(&mut tx)
            .await
            .map_err(|error| RbacError::Internal {
                reason: format!("the cache epoch could not be bumped: {error}"),
            })?;
        tx.commit().await.map_err(|error| RbacError::Internal {
            reason: format!("the transaction could not be committed: {error}"),
        })?;
        Ok(())
    }

    /// 解绑用户角色。
    pub async fn unbind_user_role(
        &self,
        ctx: &RequestContext,
        user_id: &str,
        role_id: &str,
    ) -> Result<(), RbacError> {
        let mut tx = self
            .store
            .db()
            .begin(ctx)
            .await
            .map_err(|error| RbacError::Internal {
                reason: format!("the transaction could not be started: {error}"),
            })?;
        self.store
            .unbind_user_role(&mut tx, user_id, role_id)
            .await
            .map_err(|error| RbacError::Internal {
                reason: format!("the role binding could not be removed: {error}"),
            })?;
        self.store
            .bump_epoch(&mut tx)
            .await
            .map_err(|error| RbacError::Internal {
                reason: format!("the cache epoch could not be bumped: {error}"),
            })?;
        tx.commit().await.map_err(|error| RbacError::Internal {
            reason: format!("the transaction could not be committed: {error}"),
        })?;
        Ok(())
    }

    // -- 内置角色 --------------------------------------------------------

    /// 确保本租户有两条内置角色（**按租户物化**）。
    ///
    /// 幂等 —— 每次调用都安全。租户初始化时调用一次即可。
    pub async fn ensure_builtin_roles(&self, ctx: &RequestContext) -> Result<(), RbacError> {
        for code in [BUILTIN_ADMIN, BUILTIN_USER] {
            let existing = self
                .store
                .find_role_by_code(ctx, code)
                .await
                .map_err(|error| RbacError::Internal {
                    reason: format!("the built-in roles could not be read: {error}"),
                })?;
            if existing.is_some() {
                continue;
            }
            let id = format!("builtin-{code}");
            let role = if code == BUILTIN_ADMIN {
                Role::builtin_admin(ctx.tenant_id().unwrap_or_default(), &id)
            } else {
                Role::builtin_user(ctx.tenant_id().unwrap_or_default(), &id)
            };
            let mut tx = self
                .store
                .db()
                .begin(ctx)
                .await
                .map_err(|error| RbacError::Internal {
                    reason: format!("the transaction could not be started: {error}"),
                })?;
            self.store
                .insert_role_if_absent(&mut tx, &role)
                .await
                .map_err(|error| RbacError::Internal {
                    reason: format!("the built-in role could not be created: {error}"),
                })?;
            self.store
                .bump_epoch(&mut tx)
                .await
                .map_err(|error| RbacError::Internal {
                    reason: format!("the cache epoch could not be bumped: {error}"),
                })?;
            tx.commit().await.map_err(|error| RbacError::Internal {
                reason: format!("the transaction could not be committed: {error}"),
            })?;
        }
        Ok(())
    }

    // -- 有效权限与范围 --------------------------------------------------

    /// 读租户内的角色继承图。
    pub async fn load_graph(&self, ctx: &RequestContext) -> Result<RoleGraph, RbacError> {
        let roles = self
            .store
            .list_roles(ctx)
            .await
            .map_err(|error| RbacError::Internal {
                reason: format!("the roles could not be read: {error}"),
            })?;
        let permissions = self
            .store
            .list_role_permissions(ctx)
            .await
            .map_err(|error| RbacError::Internal {
                reason: format!("the role permissions could not be read: {error}"),
            })?;
        let inherits =
            self.store
                .list_role_inherits(ctx)
                .await
                .map_err(|error| RbacError::Internal {
                    reason: format!("the role inheritance could not be read: {error}"),
                })?;
        let bindings =
            self.store
                .list_all_user_roles(ctx)
                .await
                .map_err(|error| RbacError::Internal {
                    reason: format!("the role bindings could not be read: {error}"),
                })?;

        let mut graph = RoleGraph::default();
        for role in roles {
            graph.roles.insert(role.id.clone(), role);
        }
        for grant in permissions {
            graph
                .permissions
                .entry(grant.role_id.clone())
                .or_default()
                .push(grant);
        }
        for inherit in inherits {
            graph
                .inherits
                .entry(inherit.role_id.clone())
                .or_default()
                .push(inherit.parent_role_id);
        }
        for binding in bindings {
            graph
                .role_bindings
                .entry(binding.user_id)
                .or_default()
                .push(binding.role_id);
        }
        Ok(graph)
    }

    /// 用户的有效权限点集合（**展开通配**）。
    pub async fn effective_permissions(
        &self,
        ctx: &RequestContext,
        user_id: &str,
    ) -> Result<BTreeSet<String>, RbacError> {
        let graph = self.load_graph(ctx).await?;
        let registry = self.registry_codes().await?;
        Ok(expand_user_permissions(&graph, user_id, &registry))
    }

    /// 用户对某个权限点的有效数据范围（**多角色取最宽松**）。
    pub async fn effective_scope(
        &self,
        ctx: &RequestContext,
        user_id: &str,
        permission: &str,
    ) -> Result<(CombinedScope, Vec<ScopeContribution>), RbacError> {
        let graph = self.load_graph(ctx).await?;
        let registry = self.registry_codes().await?;
        let contributions = contributions_for(&graph, user_id, permission, &registry);
        Ok((combine_contributions(&contributions), contributions))
    }

    /// 用户对某个权限点的视角（权限点集合 + 数据范围）。
    pub async fn describe_user_permission(
        &self,
        ctx: &RequestContext,
        user_id: &str,
        permission: &str,
    ) -> Result<UserPermissionView, RbacError> {
        let permissions = self.effective_permissions(ctx, user_id).await?;
        let (combined, _) = self.effective_scope(ctx, user_id, permission).await?;
        Ok(UserPermissionView {
            user_id: user_id.to_string(),
            permissions: permissions.into_iter().collect(),
            data_scope: combined.scope_name().to_string(),
            uses_custom_scope: combined.is_custom(),
        })
    }

    /// 按 ID 读角色，不存在 → 404。
    async fn require_role(
        &self,
        ctx: &RequestContext,
        role_id: &str,
    ) -> Result<RoleRow, RbacError> {
        self.store
            .find_role(ctx, role_id)
            .await
            .map_err(|error| RbacError::Internal {
                reason: format!("the role could not be read: {error}"),
            })?
            .ok_or(RbacError::NotFound { resource: "role" })
    }
}

/// 把多个角色的贡献合并成一个范围。
pub fn combine_contributions(contributions: &[ScopeContribution]) -> CombinedScope {
    crate::scope::combine_scopes(contributions)
}

/// 展开用户的有效权限点集合（**通配按注册表展开**）。
pub fn expand_user_permissions(
    graph: &RoleGraph,
    user_id: &str,
    registry: &[String],
) -> BTreeSet<String> {
    let mut permissions = BTreeSet::new();
    for role_id in user_role_ids(graph, user_id) {
        for grant in graph.grants_of(&role_id) {
            let Ok(pattern) = PermissionPattern::parse(&grant.permission_code) else {
                continue;
            };
            for code in pattern.expand(registry) {
                permissions.insert(code);
            }
        }
        // 内置管理员 = **注册表全集**（动态得出）。
        if let Some(role) = graph.roles.get(&role_id) {
            if role.builtin && role.code == BUILTIN_ADMIN {
                for code in registry {
                    permissions.insert(code.clone());
                }
            }
        }
    }
    permissions
}

/// 用户的有效权限点的**范围贡献**（每个角色一条）。
///
/// # 同一个角色内多条 grant 命中同一权限点时：**精确优先于通配**（D43）
///
/// | 维度 | 优先级 |
/// |---|---|
/// | **匹配具体度** | 精确 `module:resource:action` > `module:resource:*` > `module:*` |
/// | 同具体度 | 取**更严格**的 scope（`ALL` < `OWN_ORG_AND_BELOW` < `OWN_ORG` < `SELF`） |
///
/// ⚠️ **不能靠「取排序里最后一条匹配的」**：直接授权时精确的恰好赢
/// （`'*' < 小写字母`），但那只是**排序的副产物**；经角色继承时胜者由
/// **角色 ID 的字典序**决定（角色 ID 是 UUID → 实际等同随机）。
/// **随机是不合格的** —— 同一份配置两次部署可能给出不同的可见范围。
///
/// 跨角色仍然是**并集**（D37）—— 这条只管**同一个角色内部**的多条 grant。
pub fn contributions_for(
    graph: &RoleGraph,
    user_id: &str,
    permission: &str,
    registry: &[String],
) -> Vec<ScopeContribution> {
    let mut contributions = Vec::new();
    for role_id in user_role_ids(graph, user_id) {
        let Some(role) = graph.roles.get(&role_id) else {
            continue;
        };
        let grants = graph.grants_of(&role_id);
        // 角色级 scope（**权限点级 scope 覆盖它**）。
        let role_scope = DataScope::parse(&role.data_scope).unwrap_or(DataScope::OnlySelf);
        // 该角色是否授予了这个权限点（含通配与继承）；命中多条时按 D43 选一条。
        let mut matched: Option<MatchedGrant> = None;
        for grant in &grants {
            let Ok(pattern) = PermissionPattern::parse(&grant.permission_code) else {
                continue;
            };
            if !pattern.matches(permission) {
                continue;
            }
            let candidate = MatchedGrant {
                specificity: pattern_specificity(&pattern),
                override_scope: grant.data_scope.as_deref().and_then(DataScope::parse),
            };
            matched = Some(match matched {
                None => candidate,
                Some(best) => {
                    if candidate.specificity > best.specificity {
                        // 更具体 → 赢（**显式意图覆盖宽泛默认**）。
                        candidate
                    } else if candidate.specificity == best.specificity {
                        // 同具体度 → 取**更严格**的 scope。
                        stricter(best, candidate, role_scope)
                    } else {
                        best
                    }
                }
            });
        }
        if matched.is_none() {
            // 内置管理员仍然拥有注册表全集。
            if !(role.builtin
                && role.code == BUILTIN_ADMIN
                && registry.iter().any(|c| c == permission))
            {
                continue;
            }
        }
        // 权限点级 scope 覆盖角色级。
        let scope = match matched {
            Some(MatchedGrant {
                override_scope: Some(override_scope),
                ..
            }) => override_scope,
            _ => role_scope,
        };
        contributions.push(ScopeContribution {
            scope,
            custom_filter: role.custom_filter.clone(),
            use_all_memberships: role.use_all_memberships,
        });
    }
    contributions
}

/// 命中的那一条 grant（D43 的选取结果）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct MatchedGrant {
    /// 匹配具体度（越大越具体）。
    specificity: u8,
    /// 权限点级 scope（`None` = 不覆盖角色级）。
    override_scope: Option<DataScope>,
}

/// 一个 grant 形态的**匹配具体度**（D43）。
///
/// 管理员写精确的那条是**显式意图**，写通配的是**宽泛默认** → 显式覆盖默认。
fn pattern_specificity(pattern: &PermissionPattern) -> u8 {
    match pattern {
        PermissionPattern::Exact(_) => 3,
        PermissionPattern::ResourceWildcard { .. } => 2,
        PermissionPattern::ModuleWildcard(_) => 1,
    }
}

/// 包含型 scope 的**严格度**（D43）：`ALL` < `OWN_ORG_AND_BELOW` < `OWN_ORG` < `SELF`。
///
/// 越靠后越严格。`CUSTOM` **不参与**这个比较（它不是包含型 scope）→ `None`。
fn containment_strictness(scope: DataScope) -> Option<u8> {
    match scope {
        DataScope::All => Some(0),
        DataScope::OwnOrgAndBelow => Some(1),
        DataScope::OwnOrg => Some(2),
        DataScope::OnlySelf => Some(3),
        DataScope::Custom => None,
    }
}

/// 同具体度时**取更严格的 scope**（D43）。
///
/// - 两条都是包含型 → 取更严格的；
/// - 一条包含型一条 `CUSTOM` → **按包含型那条**（`CUSTOM` 不参与比较；
///   它与包含型的 OR 是**跨角色**的事，D37）；
/// - 两条都是 `CUSTOM` → 保留先命中的（顺序确定 → 结果确定）。
fn stricter(best: MatchedGrant, candidate: MatchedGrant, role_scope: DataScope) -> MatchedGrant {
    let best_scope = best.override_scope.unwrap_or(role_scope);
    let candidate_scope = candidate.override_scope.unwrap_or(role_scope);
    match (
        containment_strictness(best_scope),
        containment_strictness(candidate_scope),
    ) {
        (Some(best_rank), Some(candidate_rank)) => {
            if candidate_rank > best_rank {
                candidate
            } else {
                best
            }
        }
        (Some(_), None) => best,
        (None, Some(_)) => candidate,
        (None, None) => best,
    }
}

/// 用户直接绑定的角色 ID。
fn user_role_ids(graph: &RoleGraph, user_id: &str) -> Vec<String> {
    // `RoleGraph` 不直接持有绑定关系；绑定由调用方通过 `platform_user_role` 查。
    // 这里由 `graph` 的 `role_bindings` 字段提供（见 `load_graph` 的调用方）。
    graph
        .role_bindings
        .get(user_id)
        .cloned()
        .unwrap_or_default()
}

/// 检测「加这条继承边会不会成环」。
///
/// 返回环上的角色 ID 序列（至少两个），成环时 `Some`。
pub fn detect_cycle(graph: &RoleGraph, child: &str, parent: &str) -> Option<Vec<String>> {
    if child == parent {
        return Some(vec![child.to_string(), parent.to_string()]);
    }
    // 从 parent 往上走：若能走到 child，就成环。
    let mut path = vec![parent.to_string()];
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut stack = vec![parent.to_string()];
    while let Some(current) = stack.pop() {
        if !seen.insert(current.clone()) {
            continue;
        }
        if current == child {
            // 环：child → parent → … → child
            let mut cycle = vec![child.to_string()];
            cycle.extend(path.iter().cloned());
            return Some(cycle);
        }
        if let Some(parents) = graph.inherits.get(&current) {
            for next in parents {
                if !path.contains(next) {
                    path.push(next.clone());
                }
                stack.push(next.clone());
            }
        }
    }
    None
}

/// 构造一个继承图（测试用）。
pub fn build_graph(roles: Vec<RoleRow>, bindings: BTreeMap<String, Vec<String>>) -> RoleGraph {
    let mut graph = RoleGraph::default();
    for role in roles {
        graph.roles.insert(role.id.clone(), role);
    }
    graph.role_bindings = bindings;
    graph
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::RolePermissionRow;

    fn role_row(id: &str, code: &str, scope: DataScope, builtin: bool) -> RoleRow {
        RoleRow {
            tenant_id: "tenant-a".to_string(),
            id: id.to_string(),
            code: code.to_string(),
            name: code.to_string(),
            data_scope: scope.as_str().to_string(),
            custom_filter: None,
            use_all_memberships: false,
            builtin,
        }
    }

    fn grant(role_id: &str, code: &str) -> RolePermissionRow {
        RolePermissionRow {
            tenant_id: "tenant-a".to_string(),
            role_id: role_id.to_string(),
            permission_code: code.to_string(),
            data_scope: None,
        }
    }

    /// 带**权限点级 scope** 的授权行。
    fn grant_with_scope(role_id: &str, code: &str, scope: DataScope) -> RolePermissionRow {
        RolePermissionRow {
            data_scope: Some(scope.as_str().to_string()),
            ..grant(role_id, code)
        }
    }

    fn graph_with() -> RoleGraph {
        let mut graph = build_graph(
            vec![
                role_row("r-a", "rbac-inh-a", DataScope::OwnOrg, false),
                role_row("r-b", "rbac-inh-b", DataScope::OwnOrg, false),
                role_row("r-c", "rbac-inh-c", DataScope::OwnOrg, false),
            ],
            BTreeMap::from([("u-1005".to_string(), vec!["r-a".to_string()])]),
        );
        graph
            .permissions
            .insert("r-a".to_string(), vec![grant("r-a", "sandbox:demo:read")]);
        graph
            .permissions
            .insert("r-b".to_string(), vec![grant("r-b", "sandbox:demo:write")]);
        graph.permissions.insert(
            "r-c".to_string(),
            vec![grant("r-c", "sandbox:scope-item:read")],
        );
        graph
    }

    #[test]
    fn inheritance_expands_transitively() {
        let mut graph = graph_with();
        graph
            .inherits
            .insert("r-a".to_string(), vec!["r-b".to_string()]);
        graph
            .inherits
            .insert("r-b".to_string(), vec!["r-c".to_string()]);
        let ancestors = graph.ancestors("r-a");
        assert!(ancestors.contains("r-a"));
        assert!(ancestors.contains("r-b"));
        assert!(ancestors.contains("r-c"));
    }

    #[test]
    fn a_role_inherits_its_parents_permissions() {
        let mut graph = graph_with();
        graph
            .inherits
            .insert("r-a".to_string(), vec!["r-b".to_string()]);
        let permissions = expand_user_permissions(
            &graph,
            "u-1005",
            &[
                "sandbox:demo:read".to_string(),
                "sandbox:demo:write".to_string(),
                "sandbox:scope-item:read".to_string(),
            ],
        );
        assert!(permissions.contains("sandbox:demo:read"));
        assert!(permissions.contains("sandbox:demo:write"));
    }

    #[test]
    fn the_builtin_admin_owns_the_whole_registry() {
        let mut graph = build_graph(
            vec![role_row("r-admin", BUILTIN_ADMIN, DataScope::All, true)],
            BTreeMap::from([("u-1006".to_string(), vec!["r-admin".to_string()])]),
        );
        graph.permissions.insert("r-admin".to_string(), vec![]);
        let registry = vec![
            "sandbox:demo:read".to_string(),
            "rbac:role:write".to_string(),
        ];
        let permissions = expand_user_permissions(&graph, "u-1006", &registry);
        assert_eq!(permissions.len(), 2, "内置管理员 = 注册表全集");
        assert!(permissions.contains("rbac:role:write"));
    }

    #[test]
    fn a_cycle_is_detected_and_named() {
        let mut graph = graph_with();
        graph
            .inherits
            .insert("r-b".to_string(), vec!["r-c".to_string()]);
        graph
            .inherits
            .insert("r-c".to_string(), vec!["r-a".to_string()]);
        let cycle = detect_cycle(&graph, "r-a", "r-b").expect("应当检测到成环");
        assert!(cycle.len() >= 2, "环上至少两个角色：{cycle:?}");
        assert!(cycle.contains(&"r-a".to_string()));
    }

    #[test]
    fn a_self_inheritance_is_a_cycle() {
        let graph = graph_with();
        let cycle = detect_cycle(&graph, "r-a", "r-a").expect("自继承就是环");
        assert_eq!(cycle.len(), 2);
    }

    #[test]
    fn a_non_cyclic_inheritance_is_allowed() {
        let mut graph = graph_with();
        graph
            .inherits
            .insert("r-b".to_string(), vec!["r-c".to_string()]);
        assert!(detect_cycle(&graph, "r-a", "r-b").is_none());
    }

    #[test]
    fn the_permission_level_scope_overrides_the_role_level_one() {
        let mut graph = graph_with();
        let mut row = grant("r-a", "sandbox:demo:read");
        row.data_scope = Some("SELF".to_string());
        graph.permissions.insert("r-a".to_string(), vec![row]);
        let contributions = contributions_for(
            &graph,
            "u-1005",
            "sandbox:demo:read",
            &["sandbox:demo:read".to_string()],
        );
        assert_eq!(contributions.len(), 1);
        assert_eq!(contributions[0].scope, DataScope::OnlySelf);
    }

    #[test]
    fn a_wildcard_grant_covers_the_whole_module() {
        let mut graph = graph_with();
        graph
            .permissions
            .insert("r-a".to_string(), vec![grant("r-a", "sandbox:*")]);
        let permissions = expand_user_permissions(
            &graph,
            "u-1005",
            &[
                "sandbox:demo:read".to_string(),
                "sandbox:demo:write".to_string(),
                "org:org-node:read".to_string(),
            ],
        );
        assert!(permissions.contains("sandbox:demo:read"));
        assert!(permissions.contains("sandbox:demo:write"));
        assert!(
            !permissions.contains("org:org-node:read"),
            "`sandbox:*` 不得覆盖 `org:`：{permissions:?}"
        );
    }

    // -- D43：同一个角色内多条 grant 命中同一权限点 → 精确优先于通配 -------

    /// 精确授权与通配授权同时命中 → **精确的赢**（显式意图覆盖宽泛默认）。
    #[test]
    fn an_exact_grant_wins_over_a_wildcard_grant() {
        let mut graph = build_graph(
            vec![role_row("r-a", "rbac-d43", DataScope::OwnOrg, false)],
            BTreeMap::from([("u-d43".to_string(), vec!["r-a".to_string()])]),
        );
        graph.permissions.insert(
            "r-a".to_string(),
            vec![
                grant_with_scope("r-a", "sandbox:*", DataScope::All),
                grant_with_scope("r-a", "sandbox:scope-item:read", DataScope::OnlySelf),
            ],
        );
        let contributions = contributions_for(
            &graph,
            "u-d43",
            "sandbox:scope-item:read",
            &["sandbox:scope-item:read".to_string()],
        );
        assert_eq!(contributions.len(), 1);
        assert_eq!(
            contributions[0].scope,
            DataScope::OnlySelf,
            "精确授权必须赢过通配授权（D43）"
        );
    }

    /// **经继承**：两条 grant 都在父角色上 → 子角色继承后仍是**精确的赢**。
    #[test]
    fn an_exact_grant_inherited_from_a_parent_role_wins_over_a_wildcard_grant() {
        let mut graph = build_graph(
            vec![
                role_row("r-child", "rbac-d43-child", DataScope::OwnOrg, false),
                role_row("r-parent", "rbac-d43-parent", DataScope::OwnOrg, false),
            ],
            BTreeMap::from([("u-d43".to_string(), vec!["r-child".to_string()])]),
        );
        graph.permissions.insert(
            "r-parent".to_string(),
            vec![
                grant_with_scope("r-parent", "sandbox:*", DataScope::All),
                grant_with_scope("r-parent", "sandbox:scope-item:read", DataScope::OnlySelf),
            ],
        );
        graph
            .inherits
            .insert("r-child".to_string(), vec!["r-parent".to_string()]);
        let contributions = contributions_for(
            &graph,
            "u-d43",
            "sandbox:scope-item:read",
            &["sandbox:scope-item:read".to_string()],
        );
        assert_eq!(contributions.len(), 1);
        assert_eq!(
            contributions[0].scope,
            DataScope::OnlySelf,
            "经继承也必须由精确授权说了算（D43）"
        );
    }

    /// ⚠️ **这条专门证伪「取排序里最后一条匹配的」**。
    ///
    /// 通配在**子角色**上、精确在**父角色**上：祖先是按**角色 ID**排序的
    /// （角色 ID 是 UUID → 字典序等同随机）→ 「取最后一个」的结果取决于
    /// 角色 ID 的字典序，**不是**取决于具体度。
    #[test]
    fn an_exact_grant_on_a_parent_role_wins_over_a_wildcard_grant_on_the_child_role() {
        let mut graph = build_graph(
            vec![
                role_row("r-narrow", "rbac-d43-narrow", DataScope::OwnOrg, false),
                role_row("r-wide", "rbac-d43-wide", DataScope::OwnOrg, false),
            ],
            BTreeMap::from([("u-d43".to_string(), vec!["r-wide".to_string()])]),
        );
        graph.permissions.insert(
            "r-narrow".to_string(),
            vec![grant_with_scope(
                "r-narrow",
                "sandbox:scope-item:read",
                DataScope::OnlySelf,
            )],
        );
        graph.permissions.insert(
            "r-wide".to_string(),
            vec![grant_with_scope("r-wide", "sandbox:*", DataScope::All)],
        );
        graph
            .inherits
            .insert("r-wide".to_string(), vec!["r-narrow".to_string()]);
        let contributions = contributions_for(
            &graph,
            "u-d43",
            "sandbox:scope-item:read",
            &["sandbox:scope-item:read".to_string()],
        );
        assert_eq!(contributions.len(), 1);
        assert_eq!(
            contributions[0].scope,
            DataScope::OnlySelf,
            "精确授权必须赢过通配授权 —— 不能由角色 ID 的字典序决定（D43）"
        );
    }
}
