//! 数据访问：账号 / 会话 / 令牌 / MFA / 幂等键。
//!
//! # 全部走 `platform-db` 的租户作用域入口
//!
//! - 每条语句都带 **`{tenant}` 占位符**（平台强制：少了直接报错）；
//! - 租户**只能**从 [`RequestContext`] 取；
//! - 公开 API 里**拿不到裸连接** —— 想绕过租户条件也绕不过；
//! - 数据库层还有 RLS 兜底（fail closed）。
//!
//! # 为什么不用「跨租户查询」
//!
//! 账号是**租户内**的身份：`username` / `email` 都在租户内唯一。
//! 登录时按 `(tenant_id, username)` 查 —— 租户来自请求头
//! （`X-Tenant-Id` → 租户上下文中间件），**不是**从账号记录里反推。

use platform_core::RequestContext;
use platform_db::{DbResult, TenantDb, TenantQuery, TenantTx};
use time::OffsetDateTime;

/// 账号表。
pub const USER_TABLE: &str = "platform_iam_user";
/// 密码历史表。
pub const PASSWORD_HISTORY_TABLE: &str = "platform_iam_password_history";
/// 设备会话表。
pub const SESSION_TABLE: &str = "platform_iam_session";
/// refresh token 表。
pub const REFRESH_TOKEN_TABLE: &str = "platform_iam_refresh_token";
/// TOTP secret 表。
pub const MFA_SECRET_TABLE: &str = "platform_iam_mfa_secret";
/// 恢复码表。
pub const RECOVERY_CODE_TABLE: &str = "platform_iam_recovery_code";
/// 密码重置令牌表。
pub const PASSWORD_RESET_TABLE: &str = "platform_iam_password_reset";
/// 登录尝试表（审计流水；**带租户维度**）。
pub const LOGIN_ATTEMPT_TABLE: &str = "platform_iam_login_attempt";
/// 登录失败计数表（**IP 全局限流**用；**不带租户维度**，只存 IP 摘要）。
pub const LOGIN_RATE_TABLE: &str = "platform_iam_login_rate";
/// 幂等键表。
pub const IDEMPOTENCY_TABLE: &str = "platform_iam_idempotency";
/// OIDC 授权请求的一次性凭证（REQ-013）。
pub const OIDC_STATE_TABLE: &str = "platform_iam_oidc_state";
/// SSO 登录票据（REQ-013）。
pub const SSO_TICKET_TABLE: &str = "platform_iam_sso_ticket";
/// 外部身份映射（REQ-008 建表；SSO 认证路径**只读 + DO NOTHING 写**）。
pub const EXTERNAL_IDENTITY_TABLE: &str = "external_identity";

/// 无租户过滤查询用的**占位租户**。
///
/// `platform_iam_oidc_state` / `platform_iam_sso_ticket` **不启用 RLS**
/// （回调 / 换票时还没有权威租户来源），查询语句里用 `{tenant} IS NOT NULL`
/// 满足 `TenantQuery` 的「必须有一个租户占位符」约束。
///
/// ⚠️ 这个值**只用于把事务开起来**，不参与任何行过滤 ——
/// 真正的租户校验在应用层：`state.tenant_id` / `ticket.tenant_id` 必须与请求租户一致
/// （无请求头时才采纳它）。**绝不要**拿这个 ctx 去碰带 RLS 的表。
const LOOKUP_TENANT: &str = "__sso_lookup__";

/// 无租户过滤查询的上下文（见 [`LOOKUP_TENANT`]）。
fn lookup_context() -> RequestContext {
    RequestContext::new().with_tenant(LOOKUP_TENANT)
}

/// 账号状态：正常。
pub const STATUS_ACTIVE: &str = "active";
/// 账号状态：已禁用。
pub const STATUS_DISABLED: &str = "disabled";

/// 可由同步裁决写入的账号档案字段（**白名单** —— 不允许拼 SQL 片段）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserProfileField {
    /// 登录名。
    Username,
    /// 邮箱。
    Email,
    /// 显示名。
    DisplayName,
}

/// 一个账号。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserRecord {
    /// 账号 ID。
    pub id: String,
    /// 归属租户。
    pub tenant_id: String,
    /// 登录名（租户内唯一）。
    pub username: String,
    /// 邮箱（租户内唯一，可空）。
    pub email: Option<String>,
    /// Argon2id 的 PHC 串（**绝不是明文**）。
    pub password_hash: String,
    /// 账号状态（`active` / `disabled`）。
    pub status: String,
    /// 是否已绑定 MFA。
    pub mfa_enabled: bool,
    /// 是否必须改密（首次登录）。
    pub must_change_password: bool,
    /// 连续登录失败次数。
    pub failed_attempts: i32,
    /// 锁定到期时间。
    pub locked_until: Option<OffsetDateTime>,
    /// 最近一次改密时间（有效期判定用）。
    pub password_changed_at: OffsetDateTime,
    /// 显示名（REQ-008 的组织侧属性：SCIM / LDAP 同步的对象）。
    pub display_name: Option<String>,
    /// 直属上级（**汇报关系是人的属性**，REQ-008 D3）。
    pub manager_id: Option<String>,
    /// 离职时间（`None` = 在职）。
    pub departed_at: Option<OffsetDateTime>,
    /// 创建时间。
    pub created_at: OffsetDateTime,
    /// 更新时间。
    pub updated_at: OffsetDateTime,
}

platform_db::tenant_row!(UserRecord {
    id: "id",
    tenant_id: "tenant_id",
    username: "username",
    email: "email",
    password_hash: "password_hash",
    status: "status",
    mfa_enabled: "mfa_enabled",
    must_change_password: "must_change_password",
    failed_attempts: "failed_attempts",
    locked_until: "locked_until",
    password_changed_at: "password_changed_at",
    display_name: "display_name",
    manager_id: "manager_id",
    departed_at: "departed_at",
    created_at: "created_at",
    updated_at: "updated_at",
});

/// 一个设备会话（**每个会话一个 refresh family**）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionRecord {
    /// 会话 ID。
    pub id: String,
    /// 归属租户。
    pub tenant_id: String,
    /// 账号 ID。
    pub user_id: String,
    /// 设备标签。
    pub device_label: String,
    /// 登录 IP。
    pub ip: String,
    /// 客户端 UA。
    pub user_agent: String,
    /// 该会话的 refresh token 家族。
    pub family_id: String,
    /// 登录来源（`local` / `oidc` / `ldap`）。
    ///
    /// ⚠️ 加了 struct 字段就**必须**同时把它加进两条 SELECT ——
    /// `tenant_row!` 按列名 `try_get`，漏了是**运行期**报列不存在（不是编译错）。
    pub auth_provider: String,
    /// 创建时间。
    pub created_at: OffsetDateTime,
    /// 最近活跃时间。
    pub last_used_at: OffsetDateTime,
    /// 撤销时间。
    pub revoked_at: Option<OffsetDateTime>,
}

platform_db::tenant_row!(SessionRecord {
    id: "id",
    tenant_id: "tenant_id",
    user_id: "user_id",
    device_label: "device_label",
    ip: "ip",
    user_agent: "user_agent",
    family_id: "family_id",
    auth_provider: "auth_provider",
    created_at: "created_at",
    last_used_at: "last_used_at",
    revoked_at: "revoked_at",
});

/// 一个 refresh token（**只存 hash**）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefreshRecord {
    /// 记录 ID。
    pub id: String,
    /// 归属租户。
    pub tenant_id: String,
    /// 账号 ID。
    pub user_id: String,
    /// 会话 ID。
    pub session_id: String,
    /// 令牌家族（重放 → 整个 family 作废）。
    pub family_id: String,
    /// 令牌摘要（base64url 的 SHA-256）。
    pub token_hash: String,
    /// 签发时间。
    pub issued_at: OffsetDateTime,
    /// 过期时间。
    pub expires_at: OffsetDateTime,
    /// 已轮换时间（**非空 = 旧令牌已作废**）。
    pub used_at: Option<OffsetDateTime>,
    /// 撤销时间。
    pub revoked_at: Option<OffsetDateTime>,
    /// 轮换后的新令牌 ID。
    pub replaced_by: Option<String>,
}

platform_db::tenant_row!(RefreshRecord {
    id: "id",
    tenant_id: "tenant_id",
    user_id: "user_id",
    session_id: "session_id",
    family_id: "family_id",
    token_hash: "token_hash",
    issued_at: "issued_at",
    expires_at: "expires_at",
    used_at: "used_at",
    revoked_at: "revoked_at",
    replaced_by: "replaced_by",
});

/// TOTP secret（**静态加密**）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MfaSecretRecord {
    /// 归属租户。
    pub tenant_id: String,
    /// 账号 ID。
    pub user_id: String,
    /// 密文（base64url）。
    pub secret_cipher: String,
    /// 最近一次成功使用的**时间步**（防重放）。
    pub last_used_step: Option<i64>,
    /// 创建时间。
    pub created_at: OffsetDateTime,
}

platform_db::tenant_row!(MfaSecretRecord {
    tenant_id: "tenant_id",
    user_id: "user_id",
    secret_cipher: "secret_cipher",
    last_used_step: "last_used_step",
    created_at: "created_at",
});

/// 一条恢复码（**只存 hash**）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryCodeRecord {
    /// 记录 ID。
    pub id: String,
    /// 恢复码摘要。
    pub code_hash: String,
    /// 使用时间。
    pub used_at: Option<OffsetDateTime>,
}

platform_db::tenant_row!(RecoveryCodeRecord {
    id: "id",
    code_hash: "code_hash",
    used_at: "used_at",
});

/// 一个密码重置令牌（**只存 hash**）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PasswordResetRecord {
    /// 记录 ID。
    pub id: String,
    /// 账号 ID。
    pub user_id: String,
    /// 令牌摘要。
    pub token_hash: String,
    /// 过期时间。
    pub expires_at: OffsetDateTime,
    /// 使用时间。
    pub used_at: Option<OffsetDateTime>,
}

platform_db::tenant_row!(PasswordResetRecord {
    id: "id",
    user_id: "user_id",
    token_hash: "token_hash",
    expires_at: "expires_at",
    used_at: "used_at",
});

/// 一条幂等键记录。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdempotencyRecord {
    /// 幂等键。
    pub idempotency_key: String,
    /// 方法。
    pub method: String,
    /// 路径。
    pub path: String,
    /// 状态码（0 = 处理中）。
    pub status: i32,
    /// 首次响应体。
    pub body: Option<String>,
    /// 创建时间。
    pub created_at: OffsetDateTime,
}

platform_db::tenant_row!(IdempotencyRecord {
    idempotency_key: "idempotency_key",
    method: "method",
    path: "path",
    status: "status",
    body: "body",
    created_at: "created_at",
});

/// 账号 / 会话 / 令牌 / MFA / 幂等键的数据访问。
#[derive(Debug, Clone)]
pub struct IamStore {
    db: TenantDb,
}

impl IamStore {
    /// 用平台数据访问层构造。
    pub fn new(db: TenantDb) -> Self {
        Self { db }
    }

    /// 表名（带 schema 前缀）。
    fn table(&self, name: &str) -> String {
        self.db.table(name)
    }

    // -----------------------------------------------------------------------
    // 账号
    // -----------------------------------------------------------------------

    /// 按登录名查账号。
    pub async fn find_user_by_username(
        &self,
        ctx: &RequestContext,
        username: &str,
    ) -> DbResult<Option<UserRecord>> {
        let query = TenantQuery::new(format!(
            "SELECT id, tenant_id, username, email, password_hash, status, mfa_enabled, \
             must_change_password, failed_attempts, locked_until, password_changed_at, \
             display_name, manager_id, departed_at, created_at, updated_at FROM {table} \
             WHERE tenant_id = {{tenant}} AND username = ?",
            table = self.table(USER_TABLE)
        ))
        .bind(username);
        self.db.fetch_optional(ctx, &query).await
    }

    /// 按邮箱查账号。
    pub async fn find_user_by_email(
        &self,
        ctx: &RequestContext,
        email: &str,
    ) -> DbResult<Option<UserRecord>> {
        let query = TenantQuery::new(format!(
            "SELECT id, tenant_id, username, email, password_hash, status, mfa_enabled, \
             must_change_password, failed_attempts, locked_until, password_changed_at, \
             display_name, manager_id, departed_at, created_at, updated_at FROM {table} \
             WHERE tenant_id = {{tenant}} AND email = ?",
            table = self.table(USER_TABLE)
        ))
        .bind(email);
        self.db.fetch_optional(ctx, &query).await
    }

    /// 按 ID 查账号。
    pub async fn find_user_by_id(
        &self,
        ctx: &RequestContext,
        user_id: &str,
    ) -> DbResult<Option<UserRecord>> {
        let query = TenantQuery::new(format!(
            "SELECT id, tenant_id, username, email, password_hash, status, mfa_enabled, \
             must_change_password, failed_attempts, locked_until, password_changed_at, \
             display_name, manager_id, departed_at, created_at, updated_at FROM {table} \
             WHERE tenant_id = {{tenant}} AND id = ?",
            table = self.table(USER_TABLE)
        ))
        .bind(user_id);
        self.db.fetch_optional(ctx, &query).await
    }

    /// 列出租户内的账号（SCIM 出站全量对账用）。
    ///
    /// 带上限：全量对账一次拉太多会把内存吃满 —— 超出部分由调用方分页处理。
    pub async fn list_users(&self, ctx: &RequestContext, limit: i64) -> DbResult<Vec<UserRecord>> {
        let query = TenantQuery::new(format!(
            "SELECT id, tenant_id, username, email, password_hash, status, mfa_enabled, \
             must_change_password, failed_attempts, locked_until, password_changed_at, \
             display_name, manager_id, departed_at, created_at, updated_at FROM {table} \
             WHERE tenant_id = {{tenant}} ORDER BY id LIMIT {limit}",
            table = self.table(USER_TABLE)
        ));
        self.db.fetch_all(ctx, &query).await
    }

    /// 新建账号。
    pub async fn insert_user(&self, ctx: &RequestContext, user: &UserRecord) -> DbResult<()> {
        let query = TenantQuery::new(format!(
            "INSERT INTO {table} \
             (tenant_id, id, username, email, password_hash, status, mfa_enabled, \
              must_change_password, failed_attempts, password_changed_at) \
             VALUES ({{tenant}}, ?, ?, ?, ?, ?, ?, ?, 0, now())",
            table = self.table(USER_TABLE)
        ))
        .bind(user.id.clone())
        .bind(user.username.clone())
        .bind(user.email.clone())
        .bind(user.password_hash.clone())
        .bind(user.status.clone())
        .bind(user.mfa_enabled)
        .bind(user.must_change_password);
        self.db.execute(ctx, &query).await?;
        Ok(())
    }

    /// 禁用账号（离职 / 封禁）。
    ///
    /// 同时写下 `departed_at` —— 「为什么被禁用」要能查（离职 vs 封禁）。
    pub async fn disable_user(&self, ctx: &RequestContext, user_id: &str) -> DbResult<u64> {
        let query = TenantQuery::new(format!(
            "UPDATE {table} SET status = ?, departed_at = now(), updated_at = now() \
             WHERE tenant_id = {{tenant}} AND id = ?",
            table = self.table(USER_TABLE)
        ))
        .bind(STATUS_DISABLED)
        .bind(user_id.to_string());
        self.db.execute(ctx, &query).await
    }

    /// 设置直属上级（**汇报关系是人的属性**，REQ-008 D3）。
    pub async fn set_user_manager(
        &self,
        ctx: &RequestContext,
        user_id: &str,
        manager_id: Option<&str>,
    ) -> DbResult<u64> {
        let query = TenantQuery::new(format!(
            "UPDATE {table} SET manager_id = ?, updated_at = now() \
             WHERE tenant_id = {{tenant}} AND id = ?",
            table = self.table(USER_TABLE)
        ))
        .bind(manager_id.map(str::to_string))
        .bind(user_id.to_string());
        self.db.execute(ctx, &query).await
    }

    /// 写一个账号的档案字段（**白名单**，不拼 SQL 片段）。
    ///
    /// 供同步冲突的人工裁决使用（REQ-008 D8）：裁决结果按字段落到本地。
    pub async fn update_user_profile_field(
        &self,
        ctx: &RequestContext,
        user_id: &str,
        field: UserProfileField,
        value: Option<&str>,
    ) -> DbResult<u64> {
        let column = match field {
            UserProfileField::Username => "username",
            UserProfileField::Email => "email",
            UserProfileField::DisplayName => "display_name",
        };
        let query = TenantQuery::new(format!(
            "UPDATE {table} SET {column} = ?, updated_at = now() \
             WHERE tenant_id = {{tenant}} AND id = ?",
            table = self.table(USER_TABLE)
        ))
        .bind(value.map(str::to_string))
        .bind(user_id.to_string());
        self.db.execute(ctx, &query).await
    }

    /// 记录一次登录失败（累加失败次数，可选锁定）。
    pub async fn record_login_failure(
        &self,
        ctx: &RequestContext,
        user_id: &str,
        failed_attempts: i32,
        locked_until: Option<OffsetDateTime>,
    ) -> DbResult<()> {
        let query = TenantQuery::new(format!(
            "UPDATE {table} SET failed_attempts = ?, locked_until = ?, updated_at = now() \
             WHERE tenant_id = {{tenant}} AND id = ?",
            table = self.table(USER_TABLE)
        ))
        .bind(failed_attempts)
        .bind(locked_until)
        .bind(user_id);
        self.db.execute(ctx, &query).await?;
        Ok(())
    }

    /// 登录成功：失败计数清零、解除锁定。
    pub async fn reset_login_failures(&self, ctx: &RequestContext, user_id: &str) -> DbResult<()> {
        let query = TenantQuery::new(format!(
            "UPDATE {table} SET failed_attempts = 0, locked_until = NULL, updated_at = now() \
             WHERE tenant_id = {{tenant}} AND id = ?",
            table = self.table(USER_TABLE)
        ))
        .bind(user_id);
        self.db.execute(ctx, &query).await?;
        Ok(())
    }

    /// 改密：更新哈希、改密时间、清除「必须改密」标记。
    pub async fn set_password(
        &self,
        ctx: &RequestContext,
        user_id: &str,
        password_hash: &str,
    ) -> DbResult<()> {
        let query = TenantQuery::new(format!(
            "UPDATE {table} SET password_hash = ?, password_changed_at = now(), \
             must_change_password = FALSE, updated_at = now() \
             WHERE tenant_id = {{tenant}} AND id = ?",
            table = self.table(USER_TABLE)
        ))
        .bind(password_hash)
        .bind(user_id);
        self.db.execute(ctx, &query).await?;
        Ok(())
    }

    /// 设置 MFA 是否启用。
    pub async fn set_mfa_enabled(
        &self,
        ctx: &RequestContext,
        user_id: &str,
        enabled: bool,
    ) -> DbResult<()> {
        let query = TenantQuery::new(format!(
            "UPDATE {table} SET mfa_enabled = ?, updated_at = now() \
             WHERE tenant_id = {{tenant}} AND id = ?",
            table = self.table(USER_TABLE)
        ))
        .bind(enabled)
        .bind(user_id);
        self.db.execute(ctx, &query).await?;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // 密码历史
    // -----------------------------------------------------------------------

    /// 追加一条密码历史。
    pub async fn insert_password_history(
        &self,
        ctx: &RequestContext,
        id: &str,
        user_id: &str,
        password_hash: &str,
    ) -> DbResult<()> {
        let query = TenantQuery::new(format!(
            "INSERT INTO {table} (tenant_id, id, user_id, password_hash) \
             VALUES ({{tenant}}, ?, ?, ?)",
            table = self.table(PASSWORD_HISTORY_TABLE)
        ))
        .bind(id)
        .bind(user_id)
        .bind(password_hash);
        self.db.execute(ctx, &query).await?;
        Ok(())
    }

    /// 取最近 N 条密码历史（**按时间倒序**）。
    pub async fn recent_password_hashes(
        &self,
        ctx: &RequestContext,
        user_id: &str,
        limit: i64,
    ) -> DbResult<Vec<String>> {
        let query = TenantQuery::new(format!(
            "SELECT password_hash FROM {table} \
             WHERE tenant_id = {{tenant}} AND user_id = ? \
             ORDER BY created_at DESC LIMIT ?",
            table = self.table(PASSWORD_HISTORY_TABLE)
        ))
        .bind(user_id)
        .bind(limit);
        let rows: Vec<HashRow> = self.db.fetch_all(ctx, &query).await?;
        Ok(rows.into_iter().map(|row| row.password_hash).collect())
    }

    // -----------------------------------------------------------------------
    // 设备会话
    // -----------------------------------------------------------------------

    /// 新建会话。
    pub async fn insert_session(
        &self,
        ctx: &RequestContext,
        session: &SessionRecord,
    ) -> DbResult<()> {
        let query = TenantQuery::new(format!(
            "INSERT INTO {table} \
             (tenant_id, id, user_id, device_label, ip, user_agent, family_id, auth_provider) \
             VALUES ({{tenant}}, ?, ?, ?, ?, ?, ?, ?)",
            table = self.table(SESSION_TABLE)
        ))
        .bind(session.id.clone())
        .bind(session.user_id.clone())
        .bind(session.device_label.clone())
        .bind(session.ip.clone())
        .bind(session.user_agent.clone())
        .bind(session.family_id.clone())
        .bind(session.auth_provider.clone());
        self.db.execute(ctx, &query).await?;
        Ok(())
    }

    /// 列出会话（**按最近活跃倒序**）。
    pub async fn list_sessions(
        &self,
        ctx: &RequestContext,
        user_id: &str,
    ) -> DbResult<Vec<SessionRecord>> {
        let query = TenantQuery::new(format!(
            "SELECT id, tenant_id, user_id, device_label, ip, user_agent, family_id, \
             auth_provider, created_at, last_used_at, revoked_at FROM {table} \
             WHERE tenant_id = {{tenant}} AND user_id = ? AND revoked_at IS NULL \
             ORDER BY last_used_at DESC",
            table = self.table(SESSION_TABLE)
        ))
        .bind(user_id);
        self.db.fetch_all(ctx, &query).await
    }

    /// 按 ID 查会话。
    pub async fn find_session(
        &self,
        ctx: &RequestContext,
        session_id: &str,
    ) -> DbResult<Option<SessionRecord>> {
        let query = TenantQuery::new(format!(
            "SELECT id, tenant_id, user_id, device_label, ip, user_agent, family_id, \
             auth_provider, created_at, last_used_at, revoked_at FROM {table} \
             WHERE tenant_id = {{tenant}} AND id = ?",
            table = self.table(SESSION_TABLE)
        ))
        .bind(session_id);
        self.db.fetch_optional(ctx, &query).await
    }

    /// 刷新会话的「最近活跃」时间。
    pub async fn touch_session(&self, ctx: &RequestContext, session_id: &str) -> DbResult<()> {
        let query = TenantQuery::new(format!(
            "UPDATE {table} SET last_used_at = now() WHERE tenant_id = {{tenant}} AND id = ?",
            table = self.table(SESSION_TABLE)
        ))
        .bind(session_id);
        self.db.execute(ctx, &query).await?;
        Ok(())
    }

    /// 撤销一个会话（**下线**）。
    pub async fn revoke_session(&self, ctx: &RequestContext, session_id: &str) -> DbResult<u64> {
        let query = TenantQuery::new(format!(
            "UPDATE {table} SET revoked_at = now() \
             WHERE tenant_id = {{tenant}} AND id = ? AND revoked_at IS NULL",
            table = self.table(SESSION_TABLE)
        ))
        .bind(session_id);
        self.db.execute(ctx, &query).await
    }

    /// 撤销某账号的**全部**会话（登出全端）。
    pub async fn revoke_all_sessions(&self, ctx: &RequestContext, user_id: &str) -> DbResult<u64> {
        let query = TenantQuery::new(format!(
            "UPDATE {table} SET revoked_at = now() \
             WHERE tenant_id = {{tenant}} AND user_id = ? AND revoked_at IS NULL",
            table = self.table(SESSION_TABLE)
        ))
        .bind(user_id);
        self.db.execute(ctx, &query).await
    }

    /// 撤销某账号**除指定会话外**的全部会话（改密后踢出其他设备）。
    pub async fn revoke_other_sessions(
        &self,
        ctx: &RequestContext,
        user_id: &str,
        keep_session_id: &str,
    ) -> DbResult<u64> {
        let query = TenantQuery::new(format!(
            "UPDATE {table} SET revoked_at = now() \
             WHERE tenant_id = {{tenant}} AND user_id = ? AND id <> ? AND revoked_at IS NULL",
            table = self.table(SESSION_TABLE)
        ))
        .bind(user_id)
        .bind(keep_session_id);
        self.db.execute(ctx, &query).await
    }

    // -----------------------------------------------------------------------
    // refresh token
    // -----------------------------------------------------------------------

    /// 写入 refresh token（**只存 hash**）。
    pub async fn insert_refresh_token(
        &self,
        ctx: &RequestContext,
        record: &RefreshRecord,
    ) -> DbResult<()> {
        let query = TenantQuery::new(format!(
            "INSERT INTO {table} \
             (tenant_id, id, user_id, session_id, family_id, token_hash, expires_at) \
             VALUES ({{tenant}}, ?, ?, ?, ?, ?, ?)",
            table = self.table(REFRESH_TOKEN_TABLE)
        ))
        .bind(record.id.clone())
        .bind(record.user_id.clone())
        .bind(record.session_id.clone())
        .bind(record.family_id.clone())
        .bind(record.token_hash.clone())
        .bind(record.expires_at);
        self.db.execute(ctx, &query).await?;
        Ok(())
    }

    /// 按摘要查 refresh token（**加行锁**：轮换与重放判定必须是原子的）。
    pub async fn find_refresh_by_hash_for_update(
        &self,
        ctx: &RequestContext,
        token_hash: &str,
    ) -> DbResult<Option<RefreshRecord>> {
        let query = TenantQuery::new(format!(
            "SELECT id, tenant_id, user_id, session_id, family_id, token_hash, issued_at, \
             expires_at, used_at, revoked_at, replaced_by FROM {table} \
             WHERE tenant_id = {{tenant}} AND token_hash = ? FOR UPDATE",
            table = self.table(REFRESH_TOKEN_TABLE)
        ))
        .bind(token_hash);
        self.db.fetch_optional(ctx, &query).await
    }

    /// 标记 refresh token **已轮换**（旧的立即作废）。
    pub async fn mark_refresh_used(
        &self,
        ctx: &RequestContext,
        token_id: &str,
        replaced_by: &str,
    ) -> DbResult<()> {
        let query = TenantQuery::new(format!(
            "UPDATE {table} SET used_at = now(), replaced_by = ? \
             WHERE tenant_id = {{tenant}} AND id = ?",
            table = self.table(REFRESH_TOKEN_TABLE)
        ))
        .bind(replaced_by)
        .bind(token_id);
        self.db.execute(ctx, &query).await?;
        Ok(())
    }

    /// 作废**整个 family**（检测到重放 / 踢下线 / 登出全端）。
    pub async fn revoke_family(&self, ctx: &RequestContext, family_id: &str) -> DbResult<u64> {
        let query = TenantQuery::new(format!(
            "UPDATE {table} SET revoked_at = now() \
             WHERE tenant_id = {{tenant}} AND family_id = ? AND revoked_at IS NULL",
            table = self.table(REFRESH_TOKEN_TABLE)
        ))
        .bind(family_id);
        self.db.execute(ctx, &query).await
    }

    /// 作废某账号的**全部** refresh token（登出全端 / 离职）。
    pub async fn revoke_all_refresh_tokens(
        &self,
        ctx: &RequestContext,
        user_id: &str,
    ) -> DbResult<u64> {
        let query = TenantQuery::new(format!(
            "UPDATE {table} SET revoked_at = now() \
             WHERE tenant_id = {{tenant}} AND user_id = ? AND revoked_at IS NULL",
            table = self.table(REFRESH_TOKEN_TABLE)
        ))
        .bind(user_id);
        self.db.execute(ctx, &query).await
    }

    /// 作废某账号**除指定 family 外**的全部 refresh token（改密后踢出其它设备）。
    ///
    /// design-summary §7：「改密后踢出其他设备」= 作废**除当前以外**的全部 family ——
    /// 当前设备自己的 refresh 必须**继续可用**（否则用户改完密就得重新登录）。
    pub async fn revoke_other_refresh_tokens(
        &self,
        ctx: &RequestContext,
        user_id: &str,
        keep_family_id: &str,
    ) -> DbResult<u64> {
        let query = TenantQuery::new(format!(
            "UPDATE {table} SET revoked_at = now() \
             WHERE tenant_id = {{tenant}} AND user_id = ? AND family_id <> ? AND revoked_at IS NULL",
            table = self.table(REFRESH_TOKEN_TABLE)
        ))
        .bind(user_id)
        .bind(keep_family_id);
        self.db.execute(ctx, &query).await
    }

    // -----------------------------------------------------------------------
    // MFA
    // -----------------------------------------------------------------------

    /// 写入（或覆盖）TOTP secret（**密文**）。
    pub async fn upsert_mfa_secret(
        &self,
        ctx: &RequestContext,
        user_id: &str,
        secret_cipher: &str,
    ) -> DbResult<()> {
        let query = TenantQuery::new(format!(
            "INSERT INTO {table} (tenant_id, user_id, secret_cipher) \
             VALUES ({{tenant}}, ?, ?) \
             ON CONFLICT (tenant_id, user_id) DO UPDATE \
             SET secret_cipher = EXCLUDED.secret_cipher, last_used_step = NULL, created_at = now()",
            table = self.table(MFA_SECRET_TABLE)
        ))
        .bind(user_id)
        .bind(secret_cipher);
        self.db.execute(ctx, &query).await?;
        Ok(())
    }

    /// 读 TOTP secret。
    pub async fn find_mfa_secret(
        &self,
        ctx: &RequestContext,
        user_id: &str,
    ) -> DbResult<Option<MfaSecretRecord>> {
        let query = TenantQuery::new(format!(
            "SELECT tenant_id, user_id, secret_cipher, last_used_step, created_at \
             FROM {table} WHERE tenant_id = {{tenant}} AND user_id = ?",
            table = self.table(MFA_SECRET_TABLE)
        ))
        .bind(user_id);
        self.db.fetch_optional(ctx, &query).await
    }

    /// 记录最近一次成功使用的**时间步**（防重放）。
    ///
    /// # ⚠️ 这是**条件更新**，返回值表示「是否真的抢到了这次时间步」
    ///
    /// # 为什么必须条件化（QA 复查 M4 的修复）
    ///
    /// 原实现是**无条件** `UPDATE ... SET last_used_step = ?` —— 调用方先读
    /// `last_used_step` 校验，再无条件写回 → 经典的 **check-then-act (TOCTOU)**：
    ///
    /// ```text
    /// 请求 A: 读 last_used_step = 5 ──────┐
    /// 请求 B: 读 last_used_step = 5 ──┐   │   两边都通过校验
    /// 请求 A: UPDATE ... = 10 ◄───────┼───┘
    /// 请求 B: UPDATE ... = 10 ◄───────┘       同一个 TOTP 码用了两次
    /// ```
    ///
    /// QA **实测**：同一 TOTP 码 12 路并发 → **12 次全部成功**。
    /// 而现有测试是**串行**的 → 全绿却测不到这个面。
    ///
    /// # 修法
    ///
    /// 把「校验」和「写入」合并成**一条**条件 UPDATE：
    /// 只有 `last_used_step` 仍是我们读到的那个旧值时才更新（乐观锁）。
    /// 两个并发请求里只有一个能命中条件，另一个影响 0 行 → 调用方据此拒绝。
    ///
    /// # 返回
    ///
    /// - `Ok(true)`：抢到了（本次时间步被记录）
    /// - `Ok(false)`：**已被别人抢先**（同一个时间步被并发使用了）
    pub async fn set_mfa_last_step(
        &self,
        ctx: &RequestContext,
        user_id: &str,
        step: i64,
        previous_step: Option<i64>,
    ) -> DbResult<bool> {
        // ⚠️ `last_used_step` 可能是 NULL（从未用过 TOTP）——
        // `IS NOT DISTINCT FROM` 能正确处理 NULL = NULL 的情形，
        // 而 `= ?` 在 NULL 上恒为 NULL（永不命中）。
        let query = TenantQuery::new(format!(
            "UPDATE {table} SET last_used_step = ? \
              WHERE tenant_id = {{tenant}} AND user_id = ? \
                AND last_used_step IS NOT DISTINCT FROM ?",
            table = self.table(MFA_SECRET_TABLE)
        ))
        .bind(step)
        .bind(user_id)
        .bind(previous_step);
        let affected = self.db.execute(ctx, &query).await?;
        Ok(affected > 0)
    }

    /// 批量写入恢复码（**只存 hash**）。
    pub async fn insert_recovery_codes(
        &self,
        ctx: &RequestContext,
        user_id: &str,
        codes: &[(String, String)],
    ) -> DbResult<()> {
        for (id, hash) in codes {
            let query = TenantQuery::new(format!(
                "INSERT INTO {table} (tenant_id, id, user_id, code_hash) \
                 VALUES ({{tenant}}, ?, ?, ?)",
                table = self.table(RECOVERY_CODE_TABLE)
            ))
            .bind(id.clone())
            .bind(user_id)
            .bind(hash.clone());
            self.db.execute(ctx, &query).await?;
        }
        Ok(())
    }

    /// 读**未使用**的恢复码。
    pub async fn unused_recovery_codes(
        &self,
        ctx: &RequestContext,
        user_id: &str,
    ) -> DbResult<Vec<RecoveryCodeRecord>> {
        let query = TenantQuery::new(format!(
            "SELECT id, code_hash, used_at FROM {table} \
             WHERE tenant_id = {{tenant}} AND user_id = ? AND used_at IS NULL",
            table = self.table(RECOVERY_CODE_TABLE)
        ))
        .bind(user_id);
        self.db.fetch_all(ctx, &query).await
    }

    /// 标记恢复码**已使用**（一次性）。
    pub async fn mark_recovery_code_used(
        &self,
        ctx: &RequestContext,
        code_id: &str,
    ) -> DbResult<u64> {
        let query = TenantQuery::new(format!(
            "UPDATE {table} SET used_at = now() \
             WHERE tenant_id = {{tenant}} AND id = ? AND used_at IS NULL",
            table = self.table(RECOVERY_CODE_TABLE)
        ))
        .bind(code_id);
        self.db.execute(ctx, &query).await
    }

    // -----------------------------------------------------------------------
    // 密码重置
    // -----------------------------------------------------------------------

    /// 写入密码重置令牌（**只存 hash**）。
    pub async fn insert_password_reset(
        &self,
        ctx: &RequestContext,
        id: &str,
        user_id: &str,
        token_hash: &str,
        expires_at: OffsetDateTime,
    ) -> DbResult<()> {
        let query = TenantQuery::new(format!(
            "INSERT INTO {table} (tenant_id, id, user_id, token_hash, expires_at) \
             VALUES ({{tenant}}, ?, ?, ?, ?)",
            table = self.table(PASSWORD_RESET_TABLE)
        ))
        .bind(id)
        .bind(user_id)
        .bind(token_hash)
        .bind(expires_at);
        self.db.execute(ctx, &query).await?;
        Ok(())
    }

    /// 按摘要查密码重置令牌。
    pub async fn find_password_reset(
        &self,
        ctx: &RequestContext,
        token_hash: &str,
    ) -> DbResult<Option<PasswordResetRecord>> {
        let query = TenantQuery::new(format!(
            "SELECT id, user_id, token_hash, expires_at, used_at FROM {table} \
             WHERE tenant_id = {{tenant}} AND token_hash = ?",
            table = self.table(PASSWORD_RESET_TABLE)
        ))
        .bind(token_hash);
        self.db.fetch_optional(ctx, &query).await
    }

    /// 标记密码重置令牌**已使用**（一次性）。
    pub async fn mark_password_reset_used(
        &self,
        ctx: &RequestContext,
        reset_id: &str,
    ) -> DbResult<u64> {
        let query = TenantQuery::new(format!(
            "UPDATE {table} SET used_at = now() \
             WHERE tenant_id = {{tenant}} AND id = ? AND used_at IS NULL",
            table = self.table(PASSWORD_RESET_TABLE)
        ))
        .bind(reset_id);
        self.db.execute(ctx, &query).await
    }

    /// 作废某账号的全部密码重置令牌（改密后旧链接立即失效）。
    pub async fn revoke_password_resets(
        &self,
        ctx: &RequestContext,
        user_id: &str,
    ) -> DbResult<u64> {
        let query = TenantQuery::new(format!(
            "UPDATE {table} SET used_at = now() \
             WHERE tenant_id = {{tenant}} AND user_id = ? AND used_at IS NULL",
            table = self.table(PASSWORD_RESET_TABLE)
        ))
        .bind(user_id);
        self.db.execute(ctx, &query).await
    }

    // -----------------------------------------------------------------------
    // 登录尝试（限流与审计）
    // -----------------------------------------------------------------------

    /// 记一条登录尝试。
    pub async fn record_login_attempt(
        &self,
        ctx: &RequestContext,
        id: &str,
        ip: &str,
        username: &str,
        succeeded: bool,
    ) -> DbResult<()> {
        let query = TenantQuery::new(format!(
            "INSERT INTO {table} (tenant_id, id, ip, username, succeeded) \
             VALUES ({{tenant}}, ?, ?, ?, ?)",
            table = self.table(LOGIN_ATTEMPT_TABLE)
        ))
        .bind(id)
        .bind(ip)
        .bind(username)
        .bind(succeeded);
        self.db.execute(ctx, &query).await?;
        Ok(())
    }

    /// 统计窗口内的**失败**登录次数（**按 IP 全局**，跨租户）。
    ///
    /// 限流预算必须**跨租户共享**：同一个 IP 换租户不应该拿到一份新的额度 ——
    /// 否则攻击者只要把请求散到多个租户，限流预算就被放大成「租户数倍」。
    ///
    /// ⚠️ 这张表**故意不带租户维度**（只存 IP 摘要与时间戳，没有任何租户数据），
    ///    所以 RLS 不生效，计数天然是跨租户的。
    ///    但 [`TenantQuery`] 强制每条语句都带 `{tenant}` 占位符 —— 这里用恒真的
    ///    `{tenant} IS NOT NULL` 占位（租户 ID 永远非空），查询本身不受租户维度影响。
    pub async fn count_login_failures_since(
        &self,
        ctx: &RequestContext,
        ip_hash: &str,
        since: OffsetDateTime,
    ) -> DbResult<i64> {
        let query = TenantQuery::new(format!(
            "SELECT COUNT(*) AS attempts FROM {table} \
             WHERE ip_hash = ? AND occurred_at >= ? AND {{tenant}} IS NOT NULL",
            table = self.table(LOGIN_RATE_TABLE)
        ))
        .bind(ip_hash)
        .bind(since);
        let rows: Vec<CountRow> = self.db.fetch_all(ctx, &query).await?;
        Ok(rows.first().map(|row| row.attempts).unwrap_or(0))
    }

    /// 在事务内记一次**失败**的登录尝试（全局 IP 限流的计数依据）。
    ///
    /// ⚠️ 这张表**没有 `tenant_id` 列**，所以插入语句用 `INSERT ... SELECT ... WHERE`
    ///    的形式挂上 `{tenant}` 占位符（恒真）—— [`TenantQuery`] 要求每条语句
    ///    都带租户占位符，而这张表**不按租户切分**。
    pub async fn record_login_rate_in(
        &self,
        tx: &mut TenantTx,
        id: &str,
        ip_hash: &str,
    ) -> DbResult<()> {
        let query = TenantQuery::new(format!(
            "INSERT INTO {table} (id, ip_hash) SELECT ?, ? WHERE {{tenant}} IS NOT NULL",
            table = self.table(LOGIN_RATE_TABLE)
        ))
        .bind(id)
        .bind(ip_hash);
        tx.execute(&query).await?;
        Ok(())
    }

    /// 清理过期的登录尝试（防止流水无限增长）。
    pub async fn purge_login_attempts_before(
        &self,
        ctx: &RequestContext,
        before: OffsetDateTime,
    ) -> DbResult<u64> {
        let query = TenantQuery::new(format!(
            "DELETE FROM {table} WHERE tenant_id = {{tenant}} AND occurred_at < ?",
            table = self.table(LOGIN_ATTEMPT_TABLE)
        ))
        .bind(before);
        self.db.execute(ctx, &query).await
    }

    // -----------------------------------------------------------------------
    // 幂等键（契约 C8）
    // -----------------------------------------------------------------------

    /// 认领一个幂等键。
    ///
    /// 返回 `true` = 认领成功（本次请求是第一个）；`false` = 已被别的请求认领。
    pub async fn claim_idempotency(
        &self,
        ctx: &RequestContext,
        key: &str,
        method: &str,
        path: &str,
    ) -> DbResult<bool> {
        let query = TenantQuery::new(format!(
            "INSERT INTO {table} (tenant_id, idempotency_key, method, path, status) \
             VALUES ({{tenant}}, ?, ?, ?, 0) ON CONFLICT (tenant_id, idempotency_key) DO NOTHING",
            table = self.table(IDEMPOTENCY_TABLE)
        ))
        .bind(key)
        .bind(method)
        .bind(path);
        let affected = self.db.execute(ctx, &query).await?;
        Ok(affected > 0)
    }

    /// 读幂等键记录。
    pub async fn find_idempotency(
        &self,
        ctx: &RequestContext,
        key: &str,
    ) -> DbResult<Option<IdempotencyRecord>> {
        let query = TenantQuery::new(format!(
            "SELECT idempotency_key, method, path, status, body, created_at FROM {table} \
             WHERE tenant_id = {{tenant}} AND idempotency_key = ?",
            table = self.table(IDEMPOTENCY_TABLE)
        ))
        .bind(key);
        self.db.fetch_optional(ctx, &query).await
    }

    /// 写回幂等键的首次响应（状态码 + 响应体）。
    pub async fn complete_idempotency(
        &self,
        ctx: &RequestContext,
        key: &str,
        status: u16,
        body: &str,
    ) -> DbResult<()> {
        let query = TenantQuery::new(format!(
            "UPDATE {table} SET status = ?, body = ?, updated_at = now() \
             WHERE tenant_id = {{tenant}} AND idempotency_key = ?",
            table = self.table(IDEMPOTENCY_TABLE)
        ))
        .bind(i32::from(status))
        .bind(body)
        .bind(key);
        self.db.execute(ctx, &query).await?;
        Ok(())
    }

    /// 清理过期（24 小时）的幂等键记录。
    pub async fn purge_expired_idempotency(
        &self,
        ctx: &RequestContext,
        before: OffsetDateTime,
    ) -> DbResult<u64> {
        let query = TenantQuery::new(format!(
            "DELETE FROM {table} WHERE tenant_id = {{tenant}} AND created_at < ?",
            table = self.table(IDEMPOTENCY_TABLE)
        ))
        .bind(before);
        self.db.execute(ctx, &query).await
    }

    /// 在事务内记录一次登录失败（失败计数 + 锁定）。
    pub async fn record_login_failure_in(
        &self,
        tx: &mut TenantTx,
        user_id: &str,
        failed_attempts: i32,
        locked_until: Option<OffsetDateTime>,
    ) -> DbResult<()> {
        let query = TenantQuery::new(format!(
            "UPDATE {table} SET failed_attempts = ?, locked_until = ?, updated_at = now() \
             WHERE tenant_id = {{tenant}} AND id = ?",
            table = self.table(USER_TABLE)
        ))
        .bind(failed_attempts)
        .bind(locked_until)
        .bind(user_id);
        tx.execute(&query).await?;
        Ok(())
    }

    /// 在事务内记一条登录尝试。
    pub async fn record_login_attempt_in(
        &self,
        tx: &mut TenantTx,
        id: &str,
        ip: &str,
        username: &str,
        succeeded: bool,
    ) -> DbResult<()> {
        let query = TenantQuery::new(format!(
            "INSERT INTO {table} (tenant_id, id, ip, username, succeeded) \
             VALUES ({{tenant}}, ?, ?, ?, ?)",
            table = self.table(LOGIN_ATTEMPT_TABLE)
        ))
        .bind(id)
        .bind(ip)
        .bind(username)
        .bind(succeeded);
        tx.execute(&query).await?;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // 事务内的操作（轮换与重放判定必须原子）
    // -----------------------------------------------------------------------

    /// 开一个**租户作用域**事务。
    ///
    /// refresh token 的轮换与重放判定**必须**在同一个事务里完成：
    /// 「读旧令牌 → 作废旧令牌 → 签发新令牌」要么全成、要么全不成。
    pub async fn begin(&self, ctx: &RequestContext) -> DbResult<TenantTx> {
        self.db.begin(ctx).await
    }

    /// 在事务内按摘要查 refresh token（**加行锁**）。
    pub async fn find_refresh_by_hash_for_update_in(
        &self,
        tx: &mut TenantTx,
        token_hash: &str,
    ) -> DbResult<Option<RefreshRecord>> {
        let query = TenantQuery::new(format!(
            "SELECT id, tenant_id, user_id, session_id, family_id, token_hash, issued_at, \
             expires_at, used_at, revoked_at, replaced_by FROM {table} \
             WHERE tenant_id = {{tenant}} AND token_hash = ? FOR UPDATE",
            table = self.table(REFRESH_TOKEN_TABLE)
        ))
        .bind(token_hash);
        tx.fetch_optional(&query).await
    }

    /// 在事务内标记 refresh token **已轮换**。
    pub async fn mark_refresh_used_in(
        &self,
        tx: &mut TenantTx,
        token_id: &str,
        replaced_by: &str,
    ) -> DbResult<()> {
        let query = TenantQuery::new(format!(
            "UPDATE {table} SET used_at = now(), replaced_by = ? \
             WHERE tenant_id = {{tenant}} AND id = ?",
            table = self.table(REFRESH_TOKEN_TABLE)
        ))
        .bind(replaced_by)
        .bind(token_id);
        tx.execute(&query).await?;
        Ok(())
    }

    /// 在事务内作废**整个 family**。
    pub async fn revoke_family_in(&self, tx: &mut TenantTx, family_id: &str) -> DbResult<u64> {
        let query = TenantQuery::new(format!(
            "UPDATE {table} SET revoked_at = now() \
             WHERE tenant_id = {{tenant}} AND family_id = ? AND revoked_at IS NULL",
            table = self.table(REFRESH_TOKEN_TABLE)
        ))
        .bind(family_id);
        tx.execute(&query).await
    }

    /// 在事务内写入 refresh token。
    pub async fn insert_refresh_token_in(
        &self,
        tx: &mut TenantTx,
        record: &RefreshRecord,
    ) -> DbResult<()> {
        let query = TenantQuery::new(format!(
            "INSERT INTO {table} \
             (tenant_id, id, user_id, session_id, family_id, token_hash, expires_at) \
             VALUES ({{tenant}}, ?, ?, ?, ?, ?, ?)",
            table = self.table(REFRESH_TOKEN_TABLE)
        ))
        .bind(record.id.clone())
        .bind(record.user_id.clone())
        .bind(record.session_id.clone())
        .bind(record.family_id.clone())
        .bind(record.token_hash.clone())
        .bind(record.expires_at);
        tx.execute(&query).await?;
        Ok(())
    }

    /// 在事务内新建会话。
    pub async fn insert_session_in(
        &self,
        tx: &mut TenantTx,
        session: &SessionRecord,
    ) -> DbResult<()> {
        let query = TenantQuery::new(format!(
            "INSERT INTO {table} \
             (tenant_id, id, user_id, device_label, ip, user_agent, family_id, auth_provider) \
             VALUES ({{tenant}}, ?, ?, ?, ?, ?, ?, ?)",
            table = self.table(SESSION_TABLE)
        ))
        .bind(session.id.clone())
        .bind(session.user_id.clone())
        .bind(session.device_label.clone())
        .bind(session.ip.clone())
        .bind(session.user_agent.clone())
        .bind(session.family_id.clone())
        .bind(session.auth_provider.clone());
        tx.execute(&query).await?;
        Ok(())
    }

    /// 在事务内撤销一个会话。
    pub async fn revoke_session_in(&self, tx: &mut TenantTx, session_id: &str) -> DbResult<u64> {
        let query = TenantQuery::new(format!(
            "UPDATE {table} SET revoked_at = now() \
             WHERE tenant_id = {{tenant}} AND id = ? AND revoked_at IS NULL",
            table = self.table(SESSION_TABLE)
        ))
        .bind(session_id);
        tx.execute(&query).await
    }

    /// 在事务内刷新会话的「最近活跃」时间。
    pub async fn touch_session_in(&self, tx: &mut TenantTx, session_id: &str) -> DbResult<()> {
        let query = TenantQuery::new(format!(
            "UPDATE {table} SET last_used_at = now() WHERE tenant_id = {{tenant}} AND id = ?",
            table = self.table(SESSION_TABLE)
        ))
        .bind(session_id);
        tx.execute(&query).await?;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // SSO（REQ-013）：oidc_state / ticket / external_identity
    // -----------------------------------------------------------------------

    /// 写入一条 `oidc_state`（授权时，**有租户上下文**）。
    pub async fn insert_oidc_state(
        &self,
        ctx: &RequestContext,
        record: &OidcStateRecord,
    ) -> DbResult<()> {
        let query = TenantQuery::new(format!(
            "INSERT INTO {table} \
             (tenant_id, state, provider, nonce, code_verifier, code_challenge, redirect_uri, expires_at) \
             VALUES ({{tenant}}, ?, ?, ?, ?, ?, ?, ?)",
            table = self.table(OIDC_STATE_TABLE)
        ))
        .bind(record.state.clone())
        .bind(record.provider.clone())
        .bind(record.nonce.clone())
        .bind(record.code_verifier.clone())
        .bind(record.code_challenge.clone())
        .bind(record.redirect_uri.clone())
        .bind(record.expires_at);
        self.db.execute(ctx, &query).await?;
        Ok(())
    }

    /// 按 `state` 读一条记录（**不带租户过滤**，见 [`LOOKUP_TENANT`]）。
    ///
    /// 安全性由 `state` 是 256 位不可猜串保证；租户校验在应用层做。
    pub async fn find_oidc_state(&self, state: &str) -> DbResult<Option<OidcStateRecord>> {
        let query = TenantQuery::new(format!(
            "SELECT state, tenant_id, provider, nonce, code_verifier, code_challenge, \
             redirect_uri, created_at, expires_at, consumed_at FROM {table} \
             WHERE state = ? AND {{tenant}} IS NOT NULL",
            table = self.table(OIDC_STATE_TABLE)
        ))
        .bind(state);
        self.db.fetch_optional(&lookup_context(), &query).await
    }

    /// 原子消费一条 `oidc_state`（**租户作用域**：跨租户连消费都做不到）。
    ///
    /// 返回 `0` = 已被并发回调消费（或不存在）。
    pub async fn consume_oidc_state(&self, tenant_id: &str, state: &str) -> DbResult<u64> {
        let ctx = RequestContext::new().with_tenant(tenant_id.to_string());
        let query = TenantQuery::new(format!(
            "UPDATE {table} SET consumed_at = now() \
             WHERE tenant_id = {{tenant}} AND state = ? AND consumed_at IS NULL",
            table = self.table(OIDC_STATE_TABLE)
        ))
        .bind(state);
        self.db.execute(&ctx, &query).await
    }

    /// 写入一条 ticket（回调建会话之后）。
    pub async fn insert_sso_ticket(
        &self,
        ctx: &RequestContext,
        record: &SsoTicketRecord,
    ) -> DbResult<()> {
        let query = TenantQuery::new(format!(
            "INSERT INTO {table} (tenant_id, ticket, user_id, session_id, expires_at) \
             VALUES ({{tenant}}, ?, ?, ?, ?)",
            table = self.table(SSO_TICKET_TABLE)
        ))
        .bind(record.ticket.clone())
        .bind(record.user_id.clone())
        .bind(record.session_id.clone())
        .bind(record.expires_at);
        self.db.execute(ctx, &query).await?;
        Ok(())
    }

    /// 按 ticket 读一条记录（**不带租户过滤**，见 [`LOOKUP_TENANT`]）。
    pub async fn find_sso_ticket(&self, ticket: &str) -> DbResult<Option<SsoTicketRecord>> {
        let query = TenantQuery::new(format!(
            "SELECT ticket, tenant_id, user_id, session_id, created_at, expires_at, consumed_at \
             FROM {table} WHERE ticket = ? AND {{tenant}} IS NOT NULL",
            table = self.table(SSO_TICKET_TABLE)
        ))
        .bind(ticket);
        self.db.fetch_optional(&lookup_context(), &query).await
    }

    /// 原子消费一条 ticket（**租户作用域**）。
    pub async fn consume_sso_ticket(&self, tenant_id: &str, ticket: &str) -> DbResult<u64> {
        let ctx = RequestContext::new().with_tenant(tenant_id.to_string());
        let query = TenantQuery::new(format!(
            "UPDATE {table} SET consumed_at = now() \
             WHERE tenant_id = {{tenant}} AND ticket = ? AND consumed_at IS NULL",
            table = self.table(SSO_TICKET_TABLE)
        ))
        .bind(ticket);
        self.db.execute(&ctx, &query).await
    }

    /// 按 `(provider, external_id)` 读绑定。
    pub async fn find_external_identity(
        &self,
        ctx: &RequestContext,
        provider: &str,
        external_id: &str,
    ) -> DbResult<Option<ExternalIdentityRecord>> {
        let query = TenantQuery::new(format!(
            "SELECT id, tenant_id, provider, external_id, user_id FROM {table} \
             WHERE tenant_id = {{tenant}} AND provider = ? AND external_id = ?",
            table = self.table(EXTERNAL_IDENTITY_TABLE)
        ))
        .bind(provider)
        .bind(external_id);
        self.db.fetch_optional(ctx, &query).await
    }

    /// **认证路径**的绑定写入：`ON CONFLICT DO NOTHING`（**绝不**改指）。
    ///
    /// 与 `platform-org` 的 `upsert_external_identity`（`DO UPDATE SET user_id`）
    /// **语义相反**，是**有意的**：组织同步场景「改映射」合理，
    /// 认证路径上「把绑定静默改到另一个本地用户」= 账号接管（P7 / S19）。
    ///
    /// 返回 `1` = 新建；`0` = 已存在（调用方**必须**读回比对 `user_id`）。
    pub async fn insert_external_identity_do_nothing(
        &self,
        ctx: &RequestContext,
        id: &str,
        provider: &str,
        external_id: &str,
        user_id: &str,
    ) -> DbResult<u64> {
        let query = TenantQuery::new(format!(
            "INSERT INTO {table} (tenant_id, id, provider, external_id, user_id) \
             VALUES ({{tenant}}, ?, ?, ?, ?) ON CONFLICT DO NOTHING",
            table = self.table(EXTERNAL_IDENTITY_TABLE)
        ))
        .bind(id)
        .bind(provider)
        .bind(external_id)
        .bind(user_id);
        self.db.execute(ctx, &query).await
    }

    /// 某个本地账号在**同一 provider** 下的绑定（用于「mail 撞他人绑定」判定）。
    pub async fn find_binding_for_user(
        &self,
        ctx: &RequestContext,
        user_id: &str,
        provider: &str,
    ) -> DbResult<Vec<ExternalIdentityRecord>> {
        let query = TenantQuery::new(format!(
            "SELECT id, tenant_id, provider, external_id, user_id FROM {table} \
             WHERE tenant_id = {{tenant}} AND user_id = ? AND provider = ?",
            table = self.table(EXTERNAL_IDENTITY_TABLE)
        ))
        .bind(user_id)
        .bind(provider);
        self.db.fetch_all(ctx, &query).await
    }

    /// JIT 建号：`ON CONFLICT DO NOTHING`（**任意唯一约束冲突都不报错**）。
    ///
    /// 返回 `1` = 新建；`0` = 已存在（`username` 或 `email` 撞了）——
    /// 调用方重查后自己判断是「并发同一邮箱」还是「username 被占」（→ 409）。
    /// 这样并发下**不会**产生 `5xx`（AC-S-103 步骤 6 的判据）。
    pub async fn insert_user_on_conflict_do_nothing(
        &self,
        ctx: &RequestContext,
        user: &UserRecord,
    ) -> DbResult<u64> {
        let query = TenantQuery::new(format!(
            "INSERT INTO {table} \
             (tenant_id, id, username, email, password_hash, status, mfa_enabled, \
              must_change_password, password_changed_at) \
             VALUES ({{tenant}}, ?, ?, ?, ?, ?, ?, ?, ?) ON CONFLICT DO NOTHING",
            table = self.table(USER_TABLE)
        ))
        .bind(user.id.clone())
        .bind(user.username.clone())
        .bind(user.email.clone())
        .bind(user.password_hash.clone())
        .bind(user.status.clone())
        .bind(user.mfa_enabled)
        .bind(user.must_change_password)
        .bind(user.password_changed_at);
        self.db.execute(ctx, &query).await
    }
}

/// 一条 `oidc_state`（**一次性凭证**）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OidcStateRecord {
    /// base64url(32B) 一次性串（主键）。
    pub state: String,
    /// 归属租户。
    pub tenant_id: String,
    /// 协议名（`oidc`）。
    pub provider: String,
    /// 重放防护。
    pub nonce: String,
    /// PKCE 的明文 verifier。
    pub code_verifier: String,
    /// PKCE 的 challenge（S256）。
    pub code_challenge: String,
    /// 回调时必须逐字一致的 redirect URI。
    pub redirect_uri: String,
    /// 创建时间。
    pub created_at: OffsetDateTime,
    /// 过期时间。
    pub expires_at: OffsetDateTime,
    /// 消费时间（非空 = 一次性已用）。
    pub consumed_at: Option<OffsetDateTime>,
}

platform_db::tenant_row!(OidcStateRecord {
    state: "state",
    tenant_id: "tenant_id",
    provider: "provider",
    nonce: "nonce",
    code_verifier: "code_verifier",
    code_challenge: "code_challenge",
    redirect_uri: "redirect_uri",
    created_at: "created_at",
    expires_at: "expires_at",
    consumed_at: "consumed_at",
});

/// 一条 SSO 登录票据（**只存 `session_id`，不存 token**）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SsoTicketRecord {
    /// base64url(32B) 一次性串（主键）。
    pub ticket: String,
    /// 归属租户。
    pub tenant_id: String,
    /// 账号 ID。
    pub user_id: String,
    /// 已建好的会话。
    pub session_id: String,
    /// 创建时间。
    pub created_at: OffsetDateTime,
    /// 过期时间（30 秒）。
    pub expires_at: OffsetDateTime,
    /// 消费时间（非空 = 已换过）。
    pub consumed_at: Option<OffsetDateTime>,
}

platform_db::tenant_row!(SsoTicketRecord {
    ticket: "ticket",
    tenant_id: "tenant_id",
    user_id: "user_id",
    session_id: "session_id",
    created_at: "created_at",
    expires_at: "expires_at",
    consumed_at: "consumed_at",
});

/// 一条外部身份绑定。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalIdentityRecord {
    /// 记录 ID。
    pub id: String,
    /// 归属租户。
    pub tenant_id: String,
    /// 协议（`oidc` / `ldap` / 组织同步的 source 名）。
    pub provider: String,
    /// 外部标识（OIDC 的 `sub` / LDAP 的 DN）。
    pub external_id: String,
    /// 本地账号 ID。
    pub user_id: String,
}

platform_db::tenant_row!(ExternalIdentityRecord {
    id: "id",
    tenant_id: "tenant_id",
    provider: "provider",
    external_id: "external_id",
    user_id: "user_id",
});

/// 密码历史的一行（只取哈希）。
struct HashRow {
    password_hash: String,
}

platform_db::tenant_row!(HashRow {
    password_hash: "password_hash",
});

/// 计数的一行。
struct CountRow {
    attempts: i64,
}

platform_db::tenant_row!(CountRow {
    attempts: "attempts",
});
