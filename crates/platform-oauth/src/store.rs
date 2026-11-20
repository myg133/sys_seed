//! 数据访问（REQ-022，design §2）。
//!
//! # 租户作用域
//!
//! - 4 张带 `tenant_id` 的表：语句里**必须**有 `tenant_id = {tenant}`
//!   （RLS 兜底 + 应用层兜底）；
//! - `platform_oauth_client` 是**平台级**表（**无** `tenant_id`、**不启用** RLS，
//!   D219）→ 沿用 `platform-config` / `platform-db::fx` 的**伪租户**模式：
//!   上下文租户固定为 [`PLATFORM_SCOPE_TENANT`]，语句里写 `{tenant} IS NOT NULL`
//!   （恒真）以满足 `TenantQuery` 的「必须出现租户占位符」规则。
//!
//! # 事务
//!
//! ⚠️ 每一次访问都由 `TenantDb` **自己**开事务 → 设租户 → 执行 → 提交，
//! 租户上下文**只在事务内有效**（D148）。
//!
//! 「多步必须原子」的地方（换令牌：删授权码 + 写 grant + 写 token）由**服务层**
//! 编排：先**原子地**删掉授权码（单条 DELETE → 天然 exactly-once），
//! 再写 grant 与令牌。

use platform_core::RequestContext;
use platform_db::{DbResult, TenantDb, TenantQuery};
use time::OffsetDateTime;

use crate::model::{
    CLIENT_TABLE, CODE_TABLE, DEVICE_CODE_TABLE, GRANT_TABLE, OAuthClientRow, OAuthCodeRow,
    OAuthDeviceCodeRow, OAuthGrantRow, OAuthTokenRow, TOKEN_TABLE,
};

/// 访问平台级表时用的伪租户（与 `platform-config` / `platform-db::fx` 同一约定）。
pub const PLATFORM_SCOPE_TENANT: &str = "platform";

/// OAuth 自己的**保留伪租户** —— 「这一行还没有租户绑定」时的落点。
///
/// # 为什么不用 `PLATFORM_SCOPE_TENANT`（`"platform"`）
///
/// `platform-config` 把 `"platform"` 列为**保留值**（D72）：用它读配置会被
/// `ensure_tenant_not_reserved` 拒掉 → 协议端点读不到 OAuth 配置 → 500。
///
/// # 谁会落在这个租户里
///
/// - `client_credentials` 签发的令牌（**无用户、无授权** → 是**平台级**资产，D219）；
/// - 设备码行在**批准之前**（还不知道用户属于哪个租户 → 批准时**迁到用户的租户**）。
///
/// # ⚠️ 它是一个**保留值**，不是真实租户
///
/// 真实租户不可能叫这个名字 → 不会与任何真实租户的数据混淆。
pub const OAUTH_PLATFORM_TENANT: &str = "oauth-platform";

/// 跨租户定位授权码的 **SECURITY DEFINER** 函数（迁移 `20261120000500`）。
pub const CODE_BY_HASH_FUNCTION: &str = "platform_oauth_code_by_hash";

/// 跨租户定位设备码（按 `device_code` 摘要）的 **SECURITY DEFINER** 函数。
pub const DEVICE_CODE_BY_HASH_FUNCTION: &str = "platform_oauth_device_code_by_hash";

/// 跨租户定位设备码（按 `user_code` 摘要）的 **SECURITY DEFINER** 函数。
pub const DEVICE_CODE_BY_USER_CODE_HASH_FUNCTION: &str =
    "platform_oauth_device_code_by_user_code_hash";

/// 设备码**批准 / 拒绝**的 **SECURITY DEFINER** 函数（跨租户写 + 绑定租户）。
pub const DEVICE_CODE_DECIDE_FUNCTION: &str = "platform_oauth_device_code_decide";

/// 待写入的 OAuth 客户端。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewClient {
    /// client ID。
    pub client_id: String,
    /// secret 的**摘要**（public client 为 `None`）。
    pub client_secret_hash: Option<String>,
    /// client 类型。
    pub client_type: String,
    /// 展示名。
    pub name: String,
    /// 回调白名单。
    pub redirect_uris: Vec<String>,
    /// 允许的 grant type。
    pub allowed_grant_types: Vec<String>,
    /// 允许的 scope。
    pub allowed_scopes: Vec<String>,
    /// back-channel 登出回调。
    pub backchannel_logout_uri: Option<String>,
    /// front-channel 登出回调。
    pub frontchannel_logout_uri: Option<String>,
    /// 状态。
    pub status: String,
    /// 过期时刻。
    pub expires_at: Option<OffsetDateTime>,
    /// 创建人。
    pub created_by: Option<String>,
}

/// 待写入的授权码。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewCode {
    /// 记录 ID。
    pub id: String,
    /// client ID。
    pub client_id: String,
    /// 用户 ID。
    pub user_id: String,
    /// 授权码的**摘要**。
    pub code_hash: String,
    /// 回调地址。
    pub redirect_uri: String,
    /// scope（**空格分隔**）。
    pub scope: String,
    /// PKCE 挑战。
    pub code_challenge: String,
    /// PKCE 挑战方法。
    pub code_challenge_method: String,
    /// 过期时刻。
    pub expires_at: OffsetDateTime,
}

/// 待写入的设备码。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewDeviceCode {
    /// 记录 ID。
    pub id: String,
    /// client ID。
    pub client_id: String,
    /// 设备码的**摘要**。
    pub device_code_hash: String,
    /// 用户短码的**摘要**。
    pub user_code_hash: String,
    /// scope（**空格分隔**）。
    pub scope: String,
    /// 设备码过期时刻。
    pub device_code_expires_at: OffsetDateTime,
    /// 用户短码过期时刻。
    pub user_code_expires_at: OffsetDateTime,
    /// 初始轮询间隔（秒）。
    pub poll_interval_seconds: i32,
}

/// 待写入的 OAuth 令牌状态行。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewToken {
    /// 记录 ID。
    pub id: String,
    /// client ID。
    pub client_id: String,
    /// 用户 ID（`client_credentials` 没有）。
    pub user_id: Option<String>,
    /// 令牌的 `jti`。
    pub token_id: String,
    /// 授予的 scope。
    pub scopes: Vec<String>,
    /// 关联的授权记录。
    pub grant_id: Option<String>,
    /// 过期时刻。
    pub expires_at: OffsetDateTime,
}

/// 出站 IdP 的数据访问。
#[derive(Debug, Clone)]
pub struct OAuthStore {
    db: TenantDb,
}

impl OAuthStore {
    /// 用平台数据访问层构造。
    pub fn new(db: TenantDb) -> Self {
        Self { db }
    }

    /// 平台级访问用的请求上下文（伪租户）。
    fn platform_ctx() -> RequestContext {
        RequestContext::new().with_tenant(PLATFORM_SCOPE_TENANT)
    }

    // -----------------------------------------------------------------------
    // client（**平台级**）
    // -----------------------------------------------------------------------

    /// 插入一个 client。
    pub async fn insert_client(&self, client: &NewClient) -> DbResult<OAuthClientRow> {
        let ctx = Self::platform_ctx();
        let query = TenantQuery::new(format!(
            "INSERT INTO {table} (client_id, client_secret_hash, client_type, name, \
             redirect_uris, allowed_grant_types, allowed_scopes, backchannel_logout_uri, \
             frontchannel_logout_uri, status, expires_at, created_at, created_by) \
             SELECT ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, now(), ? WHERE {{tenant}} IS NOT NULL \
             RETURNING client_id, client_secret_hash, client_type, name, redirect_uris, \
             allowed_grant_types, allowed_scopes, backchannel_logout_uri, frontchannel_logout_uri, \
             status, expires_at, created_at, created_by",
            table = self.db.table(CLIENT_TABLE)
        ))
        .bind(client.client_id.clone())
        .bind(client.client_secret_hash.clone())
        .bind(client.client_type.clone())
        .bind(client.name.clone())
        .bind(client.redirect_uris.clone())
        .bind(client.allowed_grant_types.clone())
        .bind(client.allowed_scopes.clone())
        .bind(client.backchannel_logout_uri.clone())
        .bind(client.frontchannel_logout_uri.clone())
        .bind(client.status.clone())
        .bind(client.expires_at)
        .bind(client.created_by.clone());

        self.db.fetch_one(&ctx, &query).await
    }

    /// 列出**全部** client（平台级 → 所有租户共享同一批）。
    pub async fn list_clients(&self) -> DbResult<Vec<OAuthClientRow>> {
        let ctx = Self::platform_ctx();
        let query = TenantQuery::new(format!(
            "SELECT client_id, client_secret_hash, client_type, name, redirect_uris, \
             allowed_grant_types, allowed_scopes, backchannel_logout_uri, frontchannel_logout_uri, \
             status, expires_at, created_at, created_by \
             FROM {table} WHERE {{tenant}} IS NOT NULL ORDER BY created_at, client_id",
            table = self.db.table(CLIENT_TABLE)
        ));
        self.db.fetch_all(&ctx, &query).await
    }

    /// 按 client ID 取一个。
    pub async fn get_client(&self, client_id: &str) -> DbResult<Option<OAuthClientRow>> {
        let ctx = Self::platform_ctx();
        let query = TenantQuery::new(format!(
            "SELECT client_id, client_secret_hash, client_type, name, redirect_uris, \
             allowed_grant_types, allowed_scopes, backchannel_logout_uri, frontchannel_logout_uri, \
             status, expires_at, created_at, created_by \
             FROM {table} WHERE {{tenant}} IS NOT NULL AND client_id = ?",
            table = self.db.table(CLIENT_TABLE)
        ))
        .bind(client_id.to_string());
        self.db.fetch_optional(&ctx, &query).await
    }

    /// 按 client ID 取**若干个**（登出时按 grant 里的 client_id 批量取）。
    pub async fn get_clients(&self, client_ids: &[String]) -> DbResult<Vec<OAuthClientRow>> {
        if client_ids.is_empty() {
            return Ok(Vec::new());
        }
        let ctx = Self::platform_ctx();
        let query = TenantQuery::new(format!(
            "SELECT client_id, client_secret_hash, client_type, name, redirect_uris, \
             allowed_grant_types, allowed_scopes, backchannel_logout_uri, frontchannel_logout_uri, \
             status, expires_at, created_at, created_by \
             FROM {table} WHERE {{tenant}} IS NOT NULL AND client_id = ANY(?)",
            table = self.db.table(CLIENT_TABLE)
        ))
        .bind(client_ids.to_vec());
        self.db.fetch_all(&ctx, &query).await
    }

    /// 改 client 的状态（停用 / 启用）。
    pub async fn set_client_status(&self, client_id: &str, status: &str) -> DbResult<u64> {
        let ctx = Self::platform_ctx();
        let query = TenantQuery::new(format!(
            "UPDATE {table} SET status = ? WHERE {{tenant}} IS NOT NULL AND client_id = ?",
            table = self.db.table(CLIENT_TABLE)
        ))
        .bind(status.to_string())
        .bind(client_id.to_string());
        self.db.execute(&ctx, &query).await
    }

    /// 删除 client。
    pub async fn delete_client(&self, client_id: &str) -> DbResult<u64> {
        let ctx = Self::platform_ctx();
        let query = TenantQuery::new(format!(
            "DELETE FROM {table} WHERE {{tenant}} IS NOT NULL AND client_id = ?",
            table = self.db.table(CLIENT_TABLE)
        ))
        .bind(client_id.to_string());
        self.db.execute(&ctx, &query).await
    }

    // -----------------------------------------------------------------------
    // grant（租户数据）
    // -----------------------------------------------------------------------

    /// 写 / 更新一条同意记录（**幂等**：`(tenant, client, user)` 唯一）。
    ///
    /// ⚠️ 每次访问都在**自己的事务**里执行（`TenantDb` 内部开事务 → 设租户 →
    /// 执行 → 提交），租户上下文**只在事务内有效**（D148）。
    pub async fn upsert_grant(
        &self,
        ctx: &RequestContext,
        client_id: &str,
        user_id: &str,
        scopes: &[String],
        consent_at: OffsetDateTime,
    ) -> DbResult<()> {
        let query = TenantQuery::new(format!(
            "INSERT INTO {table} (tenant_id, id, client_id, user_id, scopes, status, \
             granted_at, consent_at) \
             VALUES ({{tenant}}, ?, ?, ?, ?, 'active', now(), ?) \
             ON CONFLICT (tenant_id, client_id, user_id) DO UPDATE \
             SET scopes = EXCLUDED.scopes, status = 'active', granted_at = now(), \
                 consent_at = EXCLUDED.consent_at, revoked_at = NULL",
            table = self.db.table(GRANT_TABLE)
        ))
        .bind(uuid::Uuid::new_v4().to_string())
        .bind(client_id.to_string())
        .bind(user_id.to_string())
        .bind(scopes.to_vec())
        .bind(consent_at);
        self.db.execute(ctx, &query).await?;
        Ok(())
    }

    /// 列出某用户的全部同意记录。
    pub async fn list_grants_for_user(
        &self,
        ctx: &RequestContext,
        user_id: &str,
    ) -> DbResult<Vec<OAuthGrantRow>> {
        let query = TenantQuery::new(format!(
            "SELECT tenant_id, id, client_id, user_id, scopes, status, granted_at, consent_at, \
             revoked_at FROM {table} WHERE tenant_id = {{tenant}} AND user_id = ? \
             ORDER BY granted_at, client_id",
            table = self.db.table(GRANT_TABLE)
        ))
        .bind(user_id.to_string());
        self.db.fetch_all(ctx, &query).await
    }

    /// 取某用户对某 client 的同意记录。
    pub async fn get_grant(
        &self,
        ctx: &RequestContext,
        client_id: &str,
        user_id: &str,
    ) -> DbResult<Option<OAuthGrantRow>> {
        let query = TenantQuery::new(format!(
            "SELECT tenant_id, id, client_id, user_id, scopes, status, granted_at, consent_at, \
             revoked_at FROM {table} WHERE tenant_id = {{tenant}} AND client_id = ? AND user_id = ?",
            table = self.db.table(GRANT_TABLE)
        ))
        .bind(client_id.to_string())
        .bind(user_id.to_string());
        self.db.fetch_optional(ctx, &query).await
    }

    /// 撤销某用户对某 client 的授权（**grant 级**）。
    pub async fn revoke_grant(
        &self,
        ctx: &RequestContext,
        client_id: &str,
        user_id: &str,
    ) -> DbResult<u64> {
        let query = TenantQuery::new(format!(
            "UPDATE {table} SET status = 'revoked', revoked_at = now() \
             WHERE tenant_id = {{tenant}} AND client_id = ? AND user_id = ? AND status = 'active'",
            table = self.db.table(GRANT_TABLE)
        ))
        .bind(client_id.to_string())
        .bind(user_id.to_string());
        self.db.execute(ctx, &query).await
    }

    /// 撤销某用户的**全部**授权（全局登出）。
    pub async fn revoke_all_grants_for_user(
        &self,
        ctx: &RequestContext,
        user_id: &str,
    ) -> DbResult<u64> {
        let query = TenantQuery::new(format!(
            "UPDATE {table} SET status = 'revoked', revoked_at = now() \
             WHERE tenant_id = {{tenant}} AND user_id = ? AND status = 'active'",
            table = self.db.table(GRANT_TABLE)
        ))
        .bind(user_id.to_string());
        self.db.execute(ctx, &query).await
    }

    // -----------------------------------------------------------------------
    // code（租户数据）
    // -----------------------------------------------------------------------

    /// 插入授权码（**只存摘要**）。
    pub async fn insert_code(&self, ctx: &RequestContext, code: &NewCode) -> DbResult<()> {
        let query = TenantQuery::new(format!(
            "INSERT INTO {table} (tenant_id, id, client_id, user_id, code_hash, redirect_uri, \
             scope, code_challenge, code_challenge_method, expires_at) \
             VALUES ({{tenant}}, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            table = self.db.table(CODE_TABLE)
        ))
        .bind(code.id.clone())
        .bind(code.client_id.clone())
        .bind(code.user_id.clone())
        .bind(code.code_hash.clone())
        .bind(code.redirect_uri.clone())
        .bind(code.scope.clone())
        .bind(code.code_challenge.clone())
        .bind(code.code_challenge_method.clone())
        .bind(code.expires_at);
        self.db.execute(ctx, &query).await?;
        Ok(())
    }

    /// 按**摘要**取授权码。
    pub async fn get_code_by_hash(
        &self,
        ctx: &RequestContext,
        code_hash: &str,
    ) -> DbResult<Option<OAuthCodeRow>> {
        let query = TenantQuery::new(format!(
            "SELECT tenant_id, id, client_id, user_id, code_hash, redirect_uri, scope, \
             code_challenge, code_challenge_method, expires_at FROM {table} \
             WHERE tenant_id = {{tenant}} AND code_hash = ?",
            table = self.db.table(CODE_TABLE)
        ))
        .bind(code_hash.to_string());
        self.db.fetch_optional(ctx, &query).await
    }

    /// **删除**授权码（删除即消费 → 天然一次性，D205）。
    pub async fn delete_code_by_hash(
        &self,
        ctx: &RequestContext,
        code_hash: &str,
    ) -> DbResult<u64> {
        let query = TenantQuery::new(format!(
            "DELETE FROM {table} WHERE tenant_id = {{tenant}} AND code_hash = ?",
            table = self.db.table(CODE_TABLE)
        ))
        .bind(code_hash.to_string());
        self.db.execute(ctx, &query).await
    }

    // -----------------------------------------------------------------------
    // device code（租户数据）
    // -----------------------------------------------------------------------

    /// 插入设备码（**两个码都只存摘要**）。
    pub async fn insert_device_code(
        &self,
        ctx: &RequestContext,
        device_code: &NewDeviceCode,
    ) -> DbResult<()> {
        let query = TenantQuery::new(format!(
            "INSERT INTO {table} (tenant_id, id, client_id, device_code_hash, user_code_hash, \
             scope, status, device_code_expires_at, user_code_expires_at, poll_interval_seconds) \
             VALUES ({{tenant}}, ?, ?, ?, ?, ?, 'pending', ?, ?, ?)",
            table = self.db.table(DEVICE_CODE_TABLE)
        ))
        .bind(device_code.id.clone())
        .bind(device_code.client_id.clone())
        .bind(device_code.device_code_hash.clone())
        .bind(device_code.user_code_hash.clone())
        .bind(device_code.scope.clone())
        .bind(device_code.device_code_expires_at)
        .bind(device_code.user_code_expires_at)
        .bind(device_code.poll_interval_seconds);
        self.db.execute(ctx, &query).await?;
        Ok(())
    }

    /// 按设备码**摘要**取设备码。
    pub async fn get_device_code_by_hash(
        &self,
        ctx: &RequestContext,
        device_code_hash: &str,
    ) -> DbResult<Option<OAuthDeviceCodeRow>> {
        let query = TenantQuery::new(format!(
            "SELECT tenant_id, id, client_id, device_code_hash, user_code_hash, scope, status, \
             user_id, device_code_expires_at, user_code_expires_at, poll_interval_seconds, \
             last_polled_at FROM {table} \
             WHERE tenant_id = {{tenant}} AND device_code_hash = ?",
            table = self.db.table(DEVICE_CODE_TABLE)
        ))
        .bind(device_code_hash.to_string());
        self.db.fetch_optional(ctx, &query).await
    }

    /// 按用户短码**摘要**取设备码（用户是在设备码页输入短码的）。
    pub async fn get_device_code_by_user_code_hash(
        &self,
        ctx: &RequestContext,
        user_code_hash: &str,
    ) -> DbResult<Option<OAuthDeviceCodeRow>> {
        let query = TenantQuery::new(format!(
            "SELECT tenant_id, id, client_id, device_code_hash, user_code_hash, scope, status, \
             user_id, device_code_expires_at, user_code_expires_at, poll_interval_seconds, \
             last_polled_at FROM {table} \
             WHERE tenant_id = {{tenant}} AND user_code_hash = ?",
            table = self.db.table(DEVICE_CODE_TABLE)
        ))
        .bind(user_code_hash.to_string());
        self.db.fetch_optional(ctx, &query).await
    }

    /// 记录一次轮询（`last_polled_at` + 当前间隔）。
    pub async fn mark_device_code_polled(
        &self,
        ctx: &RequestContext,
        id: &str,
        last_polled_at: OffsetDateTime,
        poll_interval_seconds: i32,
    ) -> DbResult<u64> {
        let query = TenantQuery::new(format!(
            "UPDATE {table} SET last_polled_at = ?, poll_interval_seconds = ? \
             WHERE tenant_id = {{tenant}} AND id = ?",
            table = self.db.table(DEVICE_CODE_TABLE)
        ))
        .bind(last_polled_at)
        .bind(poll_interval_seconds)
        .bind(id.to_string());
        self.db.execute(ctx, &query).await
    }

    /// 用户**批准**：置状态 + 写入用户 ID。
    pub async fn approve_device_code(
        &self,
        ctx: &RequestContext,
        id: &str,
        user_id: &str,
    ) -> DbResult<u64> {
        let query = TenantQuery::new(format!(
            "UPDATE {table} SET status = 'approved', user_id = ? \
             WHERE tenant_id = {{tenant}} AND id = ? AND status = 'pending'",
            table = self.db.table(DEVICE_CODE_TABLE)
        ))
        .bind(user_id.to_string())
        .bind(id.to_string());
        self.db.execute(ctx, &query).await
    }

    /// 用户**拒绝**。
    pub async fn deny_device_code(&self, ctx: &RequestContext, id: &str) -> DbResult<u64> {
        let query = TenantQuery::new(format!(
            "UPDATE {table} SET status = 'denied' \
             WHERE tenant_id = {{tenant}} AND id = ? AND status = 'pending'",
            table = self.db.table(DEVICE_CODE_TABLE)
        ))
        .bind(id.to_string());
        self.db.execute(ctx, &query).await
    }

    /// 删除设备码行（换到令牌后删除 → 一次性）。
    pub async fn delete_device_code(&self, ctx: &RequestContext, id: &str) -> DbResult<u64> {
        let query = TenantQuery::new(format!(
            "DELETE FROM {table} WHERE tenant_id = {{tenant}} AND id = ?",
            table = self.db.table(DEVICE_CODE_TABLE)
        ))
        .bind(id.to_string());
        self.db.execute(ctx, &query).await
    }

    // -----------------------------------------------------------------------
    // 跨租户定位（协议端点）
    // -----------------------------------------------------------------------

    /// 按**摘要**取授权码（**跨租户**）。
    ///
    /// # 为什么需要跨租户
    ///
    /// 协议端点（`POST /api/oauth/token`）**没有用户会话** → 调用方是 client 的
    /// 后端服务，它**不知道**授权码落在哪个租户；而 4 张租户表都**启用 RLS** →
    /// 上下文里没有正确租户就**一行都看不见**（fail closed）。
    ///
    /// → 走 SECURITY DEFINER 函数 `platform_oauth_code_by_hash`：
    ///
    /// - 调用方必须知道 **43 字符的摘要**（SHA-256 的不可枚举摘要）→ 不是越权；
    /// - 函数**只返回那一行**（不给列表、不给统计）；
    /// - 拿到的 `tenant_id` 只用于**后续在该租户内**读写 —— 后续每一步仍然过 RLS。
    pub async fn get_code_by_hash_any_tenant(
        &self,
        code_hash: &str,
    ) -> DbResult<Option<OAuthCodeRow>> {
        let ctx = Self::platform_ctx();
        let query = TenantQuery::new(format!(
            "SELECT tenant_id, id, client_id, user_id, code_hash, redirect_uri, scope, \
             code_challenge, code_challenge_method, expires_at \
             FROM {}(?) WHERE {{tenant}} IS NOT NULL",
            self.db.table(CODE_BY_HASH_FUNCTION)
        ))
        .bind(code_hash.to_string());
        self.db.fetch_optional(&ctx, &query).await
    }

    /// 按 **device_code** 摘要取设备码（**跨租户**）。
    ///
    /// ⚠️ 设备码行在**批准之前**落在 [`OAUTH_PLATFORM_TENANT`] 里 → 轮询端点
    /// （没有用户会话）看不到它 → 必须跨租户定位。
    pub async fn get_device_code_by_hash_any_tenant(
        &self,
        device_code_hash: &str,
    ) -> DbResult<Option<OAuthDeviceCodeRow>> {
        let ctx = Self::platform_ctx();
        let query = TenantQuery::new(format!(
            "SELECT tenant_id, id, client_id, device_code_hash, user_code_hash, scope, status, \
             user_id, device_code_expires_at, user_code_expires_at, poll_interval_seconds, \
             last_polled_at FROM {}(?) WHERE {{tenant}} IS NOT NULL",
            self.db.table(DEVICE_CODE_BY_HASH_FUNCTION)
        ))
        .bind(device_code_hash.to_string());
        self.db.fetch_optional(&ctx, &query).await
    }

    /// 按 **user_code** 摘要取设备码（**跨租户**）。
    ///
    /// ⚠️ 用户是在设备码页**手输**短码的 → 批准端点用的是**另一个摘要**。
    pub async fn get_device_code_by_user_code_hash_any_tenant(
        &self,
        user_code_hash: &str,
    ) -> DbResult<Option<OAuthDeviceCodeRow>> {
        let ctx = Self::platform_ctx();
        let query = TenantQuery::new(format!(
            "SELECT tenant_id, id, client_id, device_code_hash, user_code_hash, scope, status, \
             user_id, device_code_expires_at, user_code_expires_at, poll_interval_seconds, \
             last_polled_at FROM {}(?) WHERE {{tenant}} IS NOT NULL",
            self.db.table(DEVICE_CODE_BY_USER_CODE_HASH_FUNCTION)
        ))
        .bind(user_code_hash.to_string());
        self.db.fetch_optional(&ctx, &query).await
    }

    /// 设备码的**批准 / 拒绝**（**跨租户**写 + 批准时**绑定租户**）。
    ///
    /// ⚠️ 正常 UPDATE 会被 RLS 挡住（`USING` 看的是**旧行**的 `tenant_id`，
    /// 它不在用户的租户里）→ 必须走 SECURITY DEFINER 函数。
    ///
    /// 返回 `true` = 有一行被改了；`false` = 没有 `pending` 且未过期的行
    /// （已被别人用过 / 已过期 / 不存在）。
    pub async fn decide_device_code(
        &self,
        user_code_hash: &str,
        approve: bool,
        user_id: &str,
        tenant_id: &str,
    ) -> DbResult<bool> {
        let ctx = Self::platform_ctx();
        let query = TenantQuery::new(format!(
            "SELECT {}(?, ?, ?, ?) AS decided WHERE {{tenant}} IS NOT NULL",
            self.db.table(DEVICE_CODE_DECIDE_FUNCTION)
        ))
        .bind(user_code_hash.to_string())
        .bind(if approve { "approve" } else { "deny" })
        .bind(user_id.to_string())
        .bind(tenant_id.to_string());
        let row: Option<(bool,)> = self.db.fetch_optional(&ctx, &query).await?;
        Ok(row.map(|value| value.0).unwrap_or(false))
    }

    // -----------------------------------------------------------------------
    // token（租户数据）
    // -----------------------------------------------------------------------

    /// 插入令牌状态行。
    pub async fn insert_token(&self, ctx: &RequestContext, token: &NewToken) -> DbResult<()> {
        let query = TenantQuery::new(format!(
            "INSERT INTO {table} (tenant_id, id, client_id, user_id, token_id, scopes, grant_id, \
             status, issued_at, expires_at) \
             VALUES ({{tenant}}, ?, ?, ?, ?, ?, ?, 'active', now(), ?)",
            table = self.db.table(TOKEN_TABLE)
        ))
        .bind(token.id.clone())
        .bind(token.client_id.clone())
        .bind(token.user_id.clone())
        .bind(token.token_id.clone())
        .bind(token.scopes.to_vec())
        .bind(token.grant_id.clone())
        .bind(token.expires_at);
        self.db.execute(ctx, &query).await?;
        Ok(())
    }

    /// 按 `jti` 取令牌状态行。
    pub async fn get_token(
        &self,
        ctx: &RequestContext,
        token_id: &str,
    ) -> DbResult<Option<OAuthTokenRow>> {
        let query = TenantQuery::new(format!(
            "SELECT tenant_id, id, client_id, user_id, token_id, scopes, grant_id, status, \
             issued_at, expires_at, revoked_at FROM {table} \
             WHERE tenant_id = {{tenant}} AND token_id = ?",
            table = self.db.table(TOKEN_TABLE)
        ))
        .bind(token_id.to_string());
        self.db.fetch_optional(ctx, &query).await
    }

    /// 撤销**一枚**令牌。
    pub async fn revoke_token(&self, ctx: &RequestContext, token_id: &str) -> DbResult<u64> {
        let query = TenantQuery::new(format!(
            "UPDATE {table} SET status = 'revoked', revoked_at = now() \
             WHERE tenant_id = {{tenant}} AND token_id = ? AND status = 'active'",
            table = self.db.table(TOKEN_TABLE)
        ))
        .bind(token_id.to_string());
        self.db.execute(ctx, &query).await
    }

    /// 撤销某个 grant 下的**全部**令牌（grant 级撤销，D210 / D254）。
    pub async fn revoke_tokens_for_grant(
        &self,
        ctx: &RequestContext,
        grant_id: &str,
    ) -> DbResult<u64> {
        let query = TenantQuery::new(format!(
            "UPDATE {table} SET status = 'revoked', revoked_at = now() \
             WHERE tenant_id = {{tenant}} AND grant_id = ? AND status = 'active'",
            table = self.db.table(TOKEN_TABLE)
        ))
        .bind(grant_id.to_string());
        self.db.execute(ctx, &query).await
    }

    /// 撤销某用户的**全部**令牌（全局登出）。
    pub async fn revoke_tokens_for_user(
        &self,
        ctx: &RequestContext,
        user_id: &str,
    ) -> DbResult<u64> {
        let query = TenantQuery::new(format!(
            "UPDATE {table} SET status = 'revoked', revoked_at = now() \
             WHERE tenant_id = {{tenant}} AND user_id = ? AND status = 'active'",
            table = self.db.table(TOKEN_TABLE)
        ))
        .bind(user_id.to_string());
        self.db.execute(ctx, &query).await
    }
}
