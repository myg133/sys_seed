//! 角色与权限的数据访问。
//!
//! # 租户维度
//!
//! | 表 | 租户维度 | 访问方式 |
//! |----|----------|----------|
//! | `platform_permission` | **无**（平台级） | 读全部（对账用） |
//! | `platform_role` / `platform_role_permission` / `platform_role_inherit` / `platform_user_role` | 有 | 全部走 `TenantDb`（`{tenant}` 强制） |
//! | `platform_permission_epoch` | 有（主键 `tenant_id`） | 走 `TenantDb` |
//!
//! # 写操作一律在同一事务里 `epoch = epoch + 1`
//!
//! 缓存的主动失效信号来自**共享的数据库**（epoch）——
//! 纯进程内广播只在单实例下正确（design-summary §7.1）。

use std::collections::BTreeSet;

use platform_core::RequestContext;
use platform_core::extension::manifest::PermissionType;
use platform_core::extension::permission::DataScope;
use platform_db::{DbError, TenantDb, TenantQuery, TenantTx};

use crate::entity::{Permission, Role, RoleInherit, RolePermission, UserRole};

/// 权限点注册表表名（**平台级**，不带 `tenant_id`）。
pub const PERMISSION_TABLE: &str = "platform_permission";
/// 角色表名。
pub const ROLE_TABLE: &str = "platform_role";
/// 角色-权限表名。
pub const ROLE_PERMISSION_TABLE: &str = "platform_role_permission";
/// 角色继承表名。
pub const ROLE_INHERIT_TABLE: &str = "platform_role_inherit";
/// 用户-角色表名。
pub const USER_ROLE_TABLE: &str = "platform_user_role";
/// 权限缓存 epoch 表名。
pub const EPOCH_TABLE: &str = "platform_permission_epoch";
/// 组织节点表名（REQ-008 的表 —— 数据范围的输入）。
pub const ORG_NODE_TABLE: &str = "org_node";
/// 用户成员关系表名（REQ-008 的表 —— 主部门 / 兼岗的输入）。
pub const USER_ORG_TABLE: &str = "user_org";

/// 平台级表的「伪租户」—— `platform_permission` 不带 `tenant_id`，
/// 但 `TenantDb` 的每次访问都要一个租户（RLS 的 `SET LOCAL`）。
/// 用它读平台级表**不会**泄露任何东西（该表没有 RLS，也没有租户列）。
pub const PLATFORM_SCOPE_TENANT: &str = "platform";

/// 一行角色。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoleRow {
    /// 归属租户。
    pub tenant_id: String,
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
}

impl RoleRow {
    /// 转成领域实体。
    pub fn to_role(&self) -> Option<Role> {
        Some(Role {
            tenant_id: self.tenant_id.clone(),
            id: self.id.clone(),
            code: self.code.clone(),
            name: self.name.clone(),
            data_scope: DataScope::parse(&self.data_scope)?,
            custom_filter: self.custom_filter.clone(),
            use_all_memberships: self.use_all_memberships,
            builtin: self.builtin,
        })
    }
}

/// 一行角色-权限。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RolePermissionRow {
    /// 归属租户。
    pub tenant_id: String,
    /// 角色 ID。
    pub role_id: String,
    /// 权限点编码（可能是通配）。
    pub permission_code: String,
    /// 权限点级数据范围（可空 = 不覆盖角色级）。
    pub data_scope: Option<String>,
}

/// 一行角色继承。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoleInheritRow {
    /// 归属租户。
    pub tenant_id: String,
    /// 角色 ID。
    pub role_id: String,
    /// 被继承的角色 ID。
    pub parent_role_id: String,
}

/// 一行用户-角色。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserRoleRow {
    /// 归属租户。
    pub tenant_id: String,
    /// 用户 ID。
    pub user_id: String,
    /// 角色 ID。
    pub role_id: String,
}

/// 一行权限点。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionRow {
    /// 权限点编码。
    pub code: String,
    /// 权限点名称。
    pub name: String,
    /// 权限点类型。
    pub kind: String,
    /// 属性级校验条件。
    pub condition: Option<String>,
    /// 来源 manifest 路径。
    pub source: String,
}

impl PermissionRow {
    /// 转成领域实体。
    pub fn to_permission(&self) -> Option<Permission> {
        let kind = match self.kind.as_str() {
            "menu" => PermissionType::Menu,
            "button" => PermissionType::Button,
            "field" => PermissionType::Field,
            "data" => PermissionType::Data,
            _ => return None,
        };
        Some(Permission {
            code: self.code.clone(),
            name: self.name.clone(),
            kind,
            condition: self.condition.clone(),
            source: self.source.clone(),
        })
    }
}

/// 一个用户在某个组织节点的成员关系。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Membership {
    /// 组织节点 ID。
    pub org_node_id: String,
    /// 是否主部门。
    pub is_primary: bool,
}

// 手写 `FromRow`（`platform-db` 的声明宏）：不需要 sqlx 的 `macros` feature。
platform_db::tenant_row!(PermissionRow {
    code: "code",
    name: "name",
    kind: "kind",
    condition: "condition",
    source: "source",
});

platform_db::tenant_row!(RoleRow {
    tenant_id: "tenant_id",
    id: "id",
    code: "code",
    name: "name",
    data_scope: "data_scope",
    custom_filter: "custom_filter",
    use_all_memberships: "use_all_memberships",
    builtin: "builtin",
});

platform_db::tenant_row!(RolePermissionRow {
    tenant_id: "tenant_id",
    role_id: "role_id",
    permission_code: "permission_code",
    data_scope: "data_scope",
});

platform_db::tenant_row!(RoleInheritRow {
    tenant_id: "tenant_id",
    role_id: "role_id",
    parent_role_id: "parent_role_id",
});

platform_db::tenant_row!(UserRoleRow {
    tenant_id: "tenant_id",
    user_id: "user_id",
    role_id: "role_id",
});

platform_db::tenant_row!(Membership {
    org_node_id: "org_node_id",
    is_primary: "is_primary",
});

/// 角色与权限的数据访问。
#[derive(Debug, Clone)]
pub struct RbacStore {
    db: TenantDb,
}

impl RbacStore {
    /// 用平台数据访问层构造。
    pub fn new(db: TenantDb) -> Self {
        Self { db }
    }

    /// 底层的数据访问层（供服务层开事务用）。
    pub fn db(&self) -> &TenantDb {
        &self.db
    }

    // -- 权限点注册表（平台级） -------------------------------------------

    /// 读全部已登记的权限点（**平台级表**，对账用）。
    ///
    /// ⚠️ `platform_permission` **没有 `tenant_id` 列**（权限点来自代码，全平台一致），
    /// 但语句里**仍然要带 `{tenant}`** ——
    /// `{tenant}` 的唯一性是 `TenantQuery` 对**所有**语句的硬约束；
    /// 少一个就绕过了租户条件。这里用 `IS NOT NULL` 表达「条件恒真」，
    /// 租户上下文照样绑定，语义不变。
    pub async fn list_permissions(&self) -> Result<Vec<PermissionRow>, DbError> {
        let ctx = RequestContext::new().with_tenant(PLATFORM_SCOPE_TENANT);
        let query = TenantQuery::new(format!(
            "SELECT code, name, kind, condition, source FROM {} \
             WHERE {{tenant}} IS NOT NULL ORDER BY code",
            self.db.table(PERMISSION_TABLE)
        ));
        self.db.fetch_all(&ctx, &query).await
    }

    /// 权限点编码集合（已排序）。
    pub async fn permission_codes(&self) -> Result<Vec<String>, DbError> {
        Ok(self
            .list_permissions()
            .await?
            .into_iter()
            .map(|row| row.code)
            .collect())
    }

    // -- 角色（租户级） ---------------------------------------------------

    /// 列出本租户的全部角色。
    pub async fn list_roles(&self, ctx: &RequestContext) -> Result<Vec<RoleRow>, DbError> {
        let query = TenantQuery::new(format!(
            "SELECT tenant_id, id, code, name, data_scope, custom_filter, \
                    use_all_memberships, builtin FROM {} \
             WHERE tenant_id = {{tenant}} ORDER BY code",
            self.db.table(ROLE_TABLE)
        ));
        self.db.fetch_all(ctx, &query).await
    }

    /// 按 ID 查角色。
    pub async fn find_role(
        &self,
        ctx: &RequestContext,
        role_id: &str,
    ) -> Result<Option<RoleRow>, DbError> {
        let query = TenantQuery::new(format!(
            "SELECT tenant_id, id, code, name, data_scope, custom_filter, \
                    use_all_memberships, builtin FROM {} \
             WHERE tenant_id = {{tenant}} AND id = ?",
            self.db.table(ROLE_TABLE)
        ))
        .bind(role_id.to_string());
        self.db.fetch_optional(ctx, &query).await
    }

    /// 按编码查角色。
    pub async fn find_role_by_code(
        &self,
        ctx: &RequestContext,
        code: &str,
    ) -> Result<Option<RoleRow>, DbError> {
        let query = TenantQuery::new(format!(
            "SELECT tenant_id, id, code, name, data_scope, custom_filter, \
                    use_all_memberships, builtin FROM {} \
             WHERE tenant_id = {{tenant}} AND code = ?",
            self.db.table(ROLE_TABLE)
        ))
        .bind(code.to_string());
        self.db.fetch_optional(ctx, &query).await
    }

    /// 插入角色（**幂等**：已存在则不动）—— 内置角色按租户物化时会并发。
    pub async fn insert_role_if_absent(
        &self,
        tx: &mut TenantTx,
        role: &Role,
    ) -> Result<(), DbError> {
        let query = TenantQuery::new(format!(
            "INSERT INTO {} (tenant_id, id, code, name, data_scope, custom_filter, \
                    use_all_memberships, builtin) \
             VALUES ({{tenant}}, ?, ?, ?, ?, ?, ?, ?) \
             ON CONFLICT (tenant_id, id) DO NOTHING",
            self.db.table(ROLE_TABLE)
        ))
        .bind(role.id.clone())
        .bind(role.code.clone())
        .bind(role.name.clone())
        .bind(role.data_scope.as_str())
        .bind(role.custom_filter.clone())
        .bind(role.use_all_memberships)
        .bind(role.builtin);
        tx.execute(&query).await?;
        Ok(())
    }

    /// 插入角色。
    pub async fn insert_role(&self, tx: &mut TenantTx, role: &Role) -> Result<(), DbError> {
        let query = TenantQuery::new(format!(
            "INSERT INTO {} (tenant_id, id, code, name, data_scope, custom_filter, \
                    use_all_memberships, builtin) \
             VALUES ({{tenant}}, ?, ?, ?, ?, ?, ?, ?)",
            self.db.table(ROLE_TABLE)
        ))
        .bind(role.id.clone())
        .bind(role.code.clone())
        .bind(role.name.clone())
        .bind(role.data_scope.as_str())
        .bind(role.custom_filter.clone())
        .bind(role.use_all_memberships)
        .bind(role.builtin);
        tx.execute(&query).await?;
        Ok(())
    }

    /// 更新角色的数据范围 / 自定义条件 / 并集开关。
    pub async fn update_role_scope(
        &self,
        tx: &mut TenantTx,
        role_id: &str,
        data_scope: DataScope,
        custom_filter: Option<&str>,
        use_all_memberships: bool,
    ) -> Result<u64, DbError> {
        let query = TenantQuery::new(format!(
            "UPDATE {} SET data_scope = ?, custom_filter = ?, use_all_memberships = ? \
             WHERE tenant_id = {{tenant}} AND id = ?",
            self.db.table(ROLE_TABLE)
        ))
        .bind(data_scope.as_str())
        .bind(custom_filter.map(str::to_string))
        .bind(use_all_memberships)
        .bind(role_id.to_string());
        tx.execute(&query).await
    }

    /// 删除角色（外键会连带删掉它的授权与继承行）。
    pub async fn delete_role(&self, tx: &mut TenantTx, role_id: &str) -> Result<u64, DbError> {
        let query = TenantQuery::new(format!(
            "DELETE FROM {} WHERE tenant_id = {{tenant}} AND id = ?",
            self.db.table(ROLE_TABLE)
        ))
        .bind(role_id.to_string());
        tx.execute(&query).await
    }

    // -- 角色-权限 --------------------------------------------------------

    /// 列出本租户的全部授权行。
    pub async fn list_role_permissions(
        &self,
        ctx: &RequestContext,
    ) -> Result<Vec<RolePermissionRow>, DbError> {
        let query = TenantQuery::new(format!(
            "SELECT tenant_id, role_id, permission_code, data_scope FROM {} \
             WHERE tenant_id = {{tenant}} ORDER BY role_id, permission_code",
            self.db.table(ROLE_PERMISSION_TABLE)
        ));
        self.db.fetch_all(ctx, &query).await
    }

    /// 列出某个角色的授权行。
    pub async fn list_permissions_of_role(
        &self,
        ctx: &RequestContext,
        role_id: &str,
    ) -> Result<Vec<RolePermissionRow>, DbError> {
        let query = TenantQuery::new(format!(
            "SELECT tenant_id, role_id, permission_code, data_scope FROM {} \
             WHERE tenant_id = {{tenant}} AND role_id = ? ORDER BY permission_code",
            self.db.table(ROLE_PERMISSION_TABLE)
        ))
        .bind(role_id.to_string());
        self.db.fetch_all(ctx, &query).await
    }

    /// 给角色授权（幂等：已存在则更新权限点级 scope）。
    pub async fn grant_permission(
        &self,
        tx: &mut TenantTx,
        role_id: &str,
        permission_code: &str,
        data_scope: Option<DataScope>,
    ) -> Result<(), DbError> {
        let query = TenantQuery::new(format!(
            "INSERT INTO {} (tenant_id, role_id, permission_code, data_scope) \
             VALUES ({{tenant}}, ?, ?, ?) \
             ON CONFLICT (tenant_id, role_id, permission_code) \
             DO UPDATE SET data_scope = EXCLUDED.data_scope",
            self.db.table(ROLE_PERMISSION_TABLE)
        ))
        .bind(role_id.to_string())
        .bind(permission_code.to_string())
        .bind(data_scope.map(|scope| scope.as_str().to_string()));
        tx.execute(&query).await?;
        Ok(())
    }

    /// 撤销授权。
    pub async fn revoke_permission(
        &self,
        tx: &mut TenantTx,
        role_id: &str,
        permission_code: &str,
    ) -> Result<u64, DbError> {
        let query = TenantQuery::new(format!(
            "DELETE FROM {} WHERE tenant_id = {{tenant}} AND role_id = ? AND permission_code = ?",
            self.db.table(ROLE_PERMISSION_TABLE)
        ))
        .bind(role_id.to_string())
        .bind(permission_code.to_string());
        tx.execute(&query).await
    }

    // -- 角色继承 --------------------------------------------------------

    /// 列出本租户的全部继承行。
    pub async fn list_role_inherits(
        &self,
        ctx: &RequestContext,
    ) -> Result<Vec<RoleInheritRow>, DbError> {
        let query = TenantQuery::new(format!(
            "SELECT tenant_id, role_id, parent_role_id FROM {} WHERE tenant_id = {{tenant}}",
            self.db.table(ROLE_INHERIT_TABLE)
        ));
        self.db.fetch_all(ctx, &query).await
    }

    /// 添加继承关系。
    pub async fn add_inherit(
        &self,
        tx: &mut TenantTx,
        role_id: &str,
        parent_role_id: &str,
    ) -> Result<(), DbError> {
        let query = TenantQuery::new(format!(
            "INSERT INTO {} (tenant_id, role_id, parent_role_id) \
             VALUES ({{tenant}}, ?, ?) \
             ON CONFLICT (tenant_id, role_id, parent_role_id) DO NOTHING",
            self.db.table(ROLE_INHERIT_TABLE)
        ))
        .bind(role_id.to_string())
        .bind(parent_role_id.to_string());
        tx.execute(&query).await?;
        Ok(())
    }

    /// 删除继承关系。
    pub async fn remove_inherit(
        &self,
        tx: &mut TenantTx,
        role_id: &str,
        parent_role_id: &str,
    ) -> Result<u64, DbError> {
        let query = TenantQuery::new(format!(
            "DELETE FROM {} WHERE tenant_id = {{tenant}} AND role_id = ? AND parent_role_id = ?",
            self.db.table(ROLE_INHERIT_TABLE)
        ))
        .bind(role_id.to_string())
        .bind(parent_role_id.to_string());
        tx.execute(&query).await
    }

    // -- 用户-角色 --------------------------------------------------------

    /// 列出某个用户的角色绑定。
    pub async fn list_user_roles(
        &self,
        ctx: &RequestContext,
        user_id: &str,
    ) -> Result<Vec<UserRoleRow>, DbError> {
        let query = TenantQuery::new(format!(
            "SELECT tenant_id, user_id, role_id FROM {} \
             WHERE tenant_id = {{tenant}} AND user_id = ? ORDER BY role_id",
            self.db.table(USER_ROLE_TABLE)
        ))
        .bind(user_id.to_string());
        self.db.fetch_all(ctx, &query).await
    }

    /// 绑定用户角色（幂等）。
    pub async fn bind_user_role(
        &self,
        tx: &mut TenantTx,
        user_id: &str,
        role_id: &str,
    ) -> Result<(), DbError> {
        let query = TenantQuery::new(format!(
            "INSERT INTO {} (tenant_id, user_id, role_id) VALUES ({{tenant}}, ?, ?) \
             ON CONFLICT (tenant_id, user_id, role_id) DO NOTHING",
            self.db.table(USER_ROLE_TABLE)
        ))
        .bind(user_id.to_string())
        .bind(role_id.to_string());
        tx.execute(&query).await?;
        Ok(())
    }

    /// 解绑用户角色。
    pub async fn unbind_user_role(
        &self,
        tx: &mut TenantTx,
        user_id: &str,
        role_id: &str,
    ) -> Result<u64, DbError> {
        let query = TenantQuery::new(format!(
            "DELETE FROM {} WHERE tenant_id = {{tenant}} AND user_id = ? AND role_id = ?",
            self.db.table(USER_ROLE_TABLE)
        ))
        .bind(user_id.to_string())
        .bind(role_id.to_string());
        tx.execute(&query).await
    }

    // -- 缓存 epoch -------------------------------------------------------

    /// 读本租户的当前 epoch（**每次判定前都要读** —— 主动失效的信号来源）。
    pub async fn current_epoch(&self, ctx: &RequestContext) -> Result<i64, DbError> {
        let query = TenantQuery::new(format!(
            "SELECT epoch FROM {} WHERE tenant_id = {{tenant}}",
            self.db.table(EPOCH_TABLE)
        ));
        let row: Option<(i64,)> = self.db.fetch_optional(ctx, &query).await?;
        Ok(row.map(|(epoch,)| epoch).unwrap_or(0))
    }

    /// `epoch = epoch + 1`（**与写操作同一事务**）。
    pub async fn bump_epoch(&self, tx: &mut TenantTx) -> Result<(), DbError> {
        let query = TenantQuery::new(format!(
            "INSERT INTO {} (tenant_id, epoch, updated_at) \
             VALUES ({{tenant}}, 1, now()) \
             ON CONFLICT (tenant_id) DO UPDATE \
             SET epoch = {}.epoch + 1, updated_at = now()",
            self.db.table(EPOCH_TABLE),
            self.db.table(EPOCH_TABLE)
        ));
        tx.execute(&query).await?;
        Ok(())
    }

    // -- 组织结构（数据范围的输入，REQ-008 的表） --------------------------

    /// 用户的全部成员关系（**主部门判定 + 兼岗判定**）。
    ///
    /// 每次判定都重读（不缓存）—— 组织成员关系一改，范围**立即**生效
    /// （AC-S-047：撤销数据范围同样立即生效）。
    pub async fn list_memberships(
        &self,
        ctx: &RequestContext,
        user_id: &str,
    ) -> Result<Vec<Membership>, DbError> {
        let query = TenantQuery::new(format!(
            "SELECT org_node_id, is_primary FROM {} \
             WHERE tenant_id = {{tenant}} AND user_id = ? ORDER BY org_node_id",
            self.db.table(USER_ORG_TABLE)
        ))
        .bind(user_id.to_string());
        self.db.fetch_all(ctx, &query).await
    }

    /// 组织节点的**子树**（含本节点）—— 递归 CTE。
    pub async fn subtree(
        &self,
        ctx: &RequestContext,
        node_id: &str,
    ) -> Result<Vec<String>, DbError> {
        let tenant = ctx.tenant_id().unwrap_or_default().to_string();
        let query = TenantQuery::new(format!(
            "WITH RECURSIVE sub AS ( \
                 SELECT id FROM {nodes} WHERE tenant_id = {{tenant}} AND id = ? \
                 UNION ALL \
                 SELECT child.id FROM {nodes} child \
                   JOIN sub ON child.parent_id = sub.id AND child.tenant_id = ? \
             ) SELECT id FROM sub ORDER BY id",
            nodes = self.db.table(ORG_NODE_TABLE)
        ))
        .bind(node_id.to_string())
        .bind(tenant);
        let rows: Vec<(String,)> = self.db.fetch_all(ctx, &query).await?;
        Ok(rows.into_iter().map(|(id,)| id).collect())
    }

    /// 本租户的全部用户-角色绑定（用户 ID → 角色 ID）。
    pub async fn list_all_user_roles(
        &self,
        ctx: &RequestContext,
    ) -> Result<Vec<UserRoleRow>, DbError> {
        let query = TenantQuery::new(format!(
            "SELECT tenant_id, user_id, role_id FROM {} \
             WHERE tenant_id = {{tenant}} ORDER BY user_id, role_id",
            self.db.table(USER_ROLE_TABLE)
        ));
        self.db.fetch_all(ctx, &query).await
    }

    /// 一个用户绑定的全部角色 ID。
    pub async fn role_ids_of_user(
        &self,
        ctx: &RequestContext,
        user_id: &str,
    ) -> Result<BTreeSet<String>, DbError> {
        Ok(self
            .list_user_roles(ctx, user_id)
            .await?
            .into_iter()
            .map(|row| row.role_id)
            .collect())
    }
}

/// 把领域实体转成存储行（测试与种子用）。
pub fn role_to_row(role: &Role) -> RoleRow {
    RoleRow {
        tenant_id: role.tenant_id.clone(),
        id: role.id.clone(),
        code: role.code.clone(),
        name: role.name.clone(),
        data_scope: role.data_scope.as_str().to_string(),
        custom_filter: role.custom_filter.clone(),
        use_all_memberships: role.use_all_memberships,
        builtin: role.builtin,
    }
}

/// 把领域实体转成存储行。
pub fn permission_to_row(permission: &Permission) -> RolePermission {
    RolePermission {
        tenant_id: String::new(),
        role_id: String::new(),
        permission_code: permission.code.clone(),
        data_scope: None,
    }
}

/// 把领域实体转成存储行。
pub fn inherit_to_row(inherit: &RoleInherit) -> RoleInheritRow {
    RoleInheritRow {
        tenant_id: inherit.tenant_id.clone(),
        role_id: inherit.role_id.clone(),
        parent_role_id: inherit.parent_role_id.clone(),
    }
}

/// 把领域实体转成存储行。
pub fn user_role_to_row(binding: &UserRole) -> UserRoleRow {
    UserRoleRow {
        tenant_id: binding.tenant_id.clone(),
        user_id: binding.user_id.clone(),
        role_id: binding.role_id.clone(),
    }
}
