//! 业务编排：注册 / 登录 / 刷新 / 登出 / 会话 / 密码 / MFA。
//!
//! # 防账号枚举是**结构性的**，不是靠「记得小心」
//!
//! 「账号不存在」与「密码错误」走**同一段代码**、返回**同一个**错误：
//!
//! | 输入 | 账号不存在 | 密码错误 |
//! |------|-----------|---------|
//! | 账号查询 | 查一次（查不到） | 查一次（查得到） |
//! | 哈希校验 | **执行一次假哈希校验** | 执行一次真哈希校验 |
//! | 错误码 | `INVALID_CREDENTIALS` | `INVALID_CREDENTIALS` |
//! | 响应体 | 完全相同 | 完全相同 |
//!
//! 假哈希用**与真密码相同的 Argon2 参数**生成 → 耗时相当（design-summary §5.1）。
//!
//! # refresh token 的轮换与重放（design-summary §2.1 / D2）
//!
//! ```text
//! refresh_1 → 刷新 → refresh_1 立即作废，签发 refresh_2（同一个 family）
//! refresh_1 再次使用 → 判定为泄露 → **整个 family 全部作废**
//! ```
//!
//! 「读旧令牌 → 作废旧令牌 → 签发新令牌」在**同一个事务**里完成，
//! 否则并发刷新会出现「同一个旧令牌签发两次」。

use platform_core::RequestContext;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::config::{IamConfig, check_password_policy};
use crate::crypto;
use crate::error::IamError;
use crate::jwt::{AccessClaims, JwtKeySet};
use crate::password::{self, DummyHash};
use crate::store::{self, IamStore, RefreshRecord, STATUS_ACTIVE, SessionRecord, UserRecord};
use crate::totp;

/// 请求来源信息（限流、审计、会话展示用）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RequestMeta {
    /// 客户端 IP。
    pub ip: String,
    /// 客户端 UA。
    pub user_agent: String,
    /// 设备标签（默认由 UA 推断，可被 `X-Device-Label` 覆盖）。
    pub device_label: String,
}

/// 注册请求。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisterRequest {
    /// 登录名（租户内唯一）。
    pub username: String,
    /// 明文密码（**只在这一刻存在**，落库的是 Argon2id 的 PHC 串）。
    pub password: String,
    /// 邮箱（可选；用于密码找回）。
    #[serde(default)]
    pub email: Option<String>,
}

/// 登录请求。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginRequest {
    /// 登录名。
    pub username: String,
    /// 明文密码。
    pub password: String,
    /// MFA 验证码（账号已绑定 MFA 时必填）。
    #[serde(default)]
    pub mfa_code: Option<String>,
}

/// 刷新请求。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RefreshRequest {
    /// refresh token（不透明随机串）。
    pub refresh_token: String,
}

/// 密码重置请求。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResetPasswordRequest {
    /// 重置令牌。
    pub token: String,
    /// 新密码。
    pub new_password: String,
}

/// 改密请求（已认证）。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangePasswordRequest {
    /// 当前密码。
    pub current_password: String,
    /// 新密码。
    pub new_password: String,
}

/// 账号视图（登录响应里给前端的那一份）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserView {
    /// 账号 ID。
    pub id: String,
    /// 登录名。
    pub username: String,
    /// 邮箱。
    pub email: Option<String>,
    /// 归属租户。
    pub tenant_id: String,
    /// 是否已绑定 MFA。
    pub mfa_enabled: bool,
    /// 是否必须改密（首次登录）。
    pub must_change_password: bool,
}

/// 会话视图（设备会话列表）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionView {
    /// 会话 ID。
    pub id: String,
    /// 设备标签。
    pub device_label: String,
    /// 登录 IP。
    pub ip: String,
    /// 客户端 UA。
    pub user_agent: String,
    /// 创建时间（RFC 3339，UTC）。
    pub created_at: String,
    /// 最近活跃时间（RFC 3339，UTC）。
    pub last_used_at: String,
}

/// 登录 / 刷新响应。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginResponse {
    /// access token（JWT，15 分钟）。
    pub access_token: String,
    /// refresh token（不透明随机串，30 天）。
    pub refresh_token: String,
    /// 令牌类型（`Bearer`）。
    pub token_type: String,
    /// access token 的剩余有效期（秒）。
    pub expires_in: u64,
    /// 账号信息。
    pub user: UserView,
    /// 权限点列表。
    ///
    /// 本期恒为空 —— RBAC 属 REQ-009（`api-conventions v1` C4）。
    pub permissions: Vec<String>,
}

/// MFA 绑定响应。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MfaEnrollResponse {
    /// base32 的 secret（用户可手输）。
    pub secret: String,
    /// otpauth URI（扫码用）。
    pub otpauth_uri: String,
    /// 一次性恢复码（**只在绑定时给一次**）。
    pub recovery_codes: Vec<String>,
}

/// 认证与账号体系的服务。
#[derive(Debug, Clone)]
pub struct IamService {
    store: IamStore,
    config: IamConfig,
    keys: JwtKeySet,
    dummy_hash: DummyHash,
}

impl IamService {
    /// 构造服务（构建密钥集与假哈希）。
    pub fn new(store: IamStore, config: IamConfig) -> Result<Self, IamError> {
        let keys = JwtKeySet::new(&config.jwt)?;
        let dummy_hash = DummyHash::generate(&config.argon2)?;
        Ok(Self {
            store,
            config,
            keys,
            dummy_hash,
        })
    }

    /// 配置（只读）。
    pub fn config(&self) -> &IamConfig {
        &self.config
    }

    /// JWKS（**只暴露公钥**）。
    pub fn jwks(&self) -> jsonwebtoken::jwk::JwkSet {
        self.keys.jwks()
    }

    /// 数据访问（供 HTTP 层做幂等键等编排）。
    pub fn store(&self) -> &IamStore {
        &self.store
    }

    // -----------------------------------------------------------------------
    // 注册
    // -----------------------------------------------------------------------

    /// 注册账号。
    pub async fn register(
        &self,
        ctx: &RequestContext,
        req: &RegisterRequest,
    ) -> Result<UserView, IamError> {
        let tenant = ctx.tenant_id().ok_or(IamError::TenantContextMissing)?;
        let username = req.username.trim();
        if username.is_empty() {
            return Err(IamError::InvalidCredentials);
        }
        check_password_policy(&req.password, &self.config.password_policy)?;

        if self
            .store
            .find_user_by_username(ctx, username)
            .await
            .map_err(internal)?
            .is_some()
        {
            return Err(IamError::Conflict {
                reason: "the username is already taken",
            });
        }

        let password_hash = password::hash_password(&req.password, &self.config.argon2)?;
        let user = UserRecord {
            id: Uuid::new_v4().to_string(),
            tenant_id: tenant.to_string(),
            username: username.to_string(),
            email: req.email.clone().map(|value| value.trim().to_string()),
            password_hash: password_hash.clone(),
            status: store::STATUS_ACTIVE.to_string(),
            mfa_enabled: false,
            must_change_password: false,
            failed_attempts: 0,
            locked_until: None,
            password_changed_at: OffsetDateTime::now_utc(),
            created_at: OffsetDateTime::now_utc(),
            updated_at: OffsetDateTime::now_utc(),
        };
        self.store.insert_user(ctx, &user).await.map_err(internal)?;
        self.store
            .insert_password_history(ctx, &Uuid::new_v4().to_string(), &user.id, &password_hash)
            .await
            .map_err(internal)?;
        Ok(user_view(&user))
    }

    // -----------------------------------------------------------------------
    // 登录
    // -----------------------------------------------------------------------

    /// 登录。
    pub async fn login(
        &self,
        ctx: &RequestContext,
        req: &LoginRequest,
        meta: &RequestMeta,
    ) -> Result<LoginResponse, IamError> {
        let tenant = ctx.tenant_id().ok_or(IamError::TenantContextMissing)?;

        // 1) 按 IP 的滑动窗口限流（瞬时打爆）。
        self.check_ip_rate_limit(ctx, &meta.ip).await?;

        // 2) 查账号。
        let user = self
            .store
            .find_user_by_username(ctx, req.username.trim())
            .await
            .map_err(internal)?;

        // 3) **账号不存在**：执行一次假哈希校验，让耗时与「密码错误」相当，
        //    然后返回**完全相同**的错误（design-summary §5.1 / D6）。
        let Some(user) = user else {
            // 假哈希校验（对齐**哈希**耗时）。
            self.dummy_hash.verify(&req.password);
            // 再对齐**数据库**耗时：走与「密码错误」路径**完全相同**的两条语句
            // （一次 UPDATE + 一次 INSERT，同一个事务），只是 UPDATE 命中 0 行。
            // 这样两条失败路径的耗时差只剩「行数」那点噪声 —— 时序枚举拿不到信号。
            let mut tx = self.store.begin(ctx).await.map_err(internal)?;
            self.store
                .record_login_failure_in(&mut tx, &Uuid::new_v4().to_string(), 0, None)
                .await
                .map_err(internal)?;
            self.store
                .record_login_attempt_in(
                    &mut tx,
                    &Uuid::new_v4().to_string(),
                    &meta.ip,
                    &req.username,
                    false,
                )
                .await
                .map_err(internal)?;
            tx.commit().await.map_err(internal)?;
            return Err(IamError::InvalidCredentials);
        };

        // 4) 账号锁定：锁定期内**即使密码正确也拒**（AC-S-017）。
        if is_locked(&user) {
            self.record_attempt(ctx, meta, &req.username, false).await?;
            return Err(IamError::AccountLocked);
        }

        // 5) 校验密码。
        if !password::verify_password(&req.password, &user.password_hash) {
            // 失败计数 + 尝试流水放**同一个事务**：两条失败路径的数据库往返次数一致，
            // 耗时差只剩「多一条 UPDATE」—— 时序枚举的信号被压到最小。
            self.on_password_failure(ctx, &user, meta, &req.username)
                .await?;
            return Err(IamError::InvalidCredentials);
        }

        // 6) 账号已禁用：密码正确之后才提示（减少账号枚举面）。
        if user.status != STATUS_ACTIVE {
            return Err(IamError::AccountDisabled);
        }

        // 7) MFA。
        if user.mfa_enabled {
            let code = req
                .mfa_code
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty());
            let Some(code) = code else {
                return Err(IamError::MfaRequired);
            };
            self.verify_mfa(ctx, &user, code).await?;
        }

        // 8) 成功：失败计数清零 + 建会话 + 签发令牌。
        self.store
            .reset_login_failures(ctx, &user.id)
            .await
            .map_err(internal)?;
        self.record_attempt(ctx, meta, &req.username, true).await?;

        let response = self.issue_session(ctx, tenant, &user, meta).await?;
        Ok(response)
    }

    /// 登录失败：累加失败次数，达到阈值则锁定；并记一条尝试流水。
    ///
    /// 两件事在**同一个事务**里 —— 见 [`IamService::login`] 的说明。
    async fn on_password_failure(
        &self,
        ctx: &RequestContext,
        user: &UserRecord,
        meta: &RequestMeta,
        username: &str,
    ) -> Result<(), IamError> {
        let attempts = user.failed_attempts + 1;
        let locked_until = if attempts >= self.config.login.account_failure_limit as i32 {
            Some(OffsetDateTime::now_utc() + self.config.login.lockout_duration)
        } else {
            None
        };
        let mut tx = self.store.begin(ctx).await.map_err(internal)?;
        self.store
            .record_login_failure_in(&mut tx, &user.id, attempts, locked_until)
            .await
            .map_err(internal)?;
        self.store
            .record_login_attempt_in(
                &mut tx,
                &Uuid::new_v4().to_string(),
                &meta.ip,
                username,
                false,
            )
            .await
            .map_err(internal)?;
        tx.commit().await.map_err(internal)
    }

    /// 记一条登录尝试（限流与审计）。
    async fn record_attempt(
        &self,
        ctx: &RequestContext,
        meta: &RequestMeta,
        username: &str,
        succeeded: bool,
    ) -> Result<(), IamError> {
        self.store
            .record_login_attempt(
                ctx,
                &Uuid::new_v4().to_string(),
                &meta.ip,
                username,
                succeeded,
            )
            .await
            .map_err(internal)
    }

    /// 按 IP 的滑动窗口限流。
    async fn check_ip_rate_limit(&self, ctx: &RequestContext, ip: &str) -> Result<(), IamError> {
        if ip.is_empty() {
            return Ok(());
        }
        let since = OffsetDateTime::now_utc() - self.config.login.ip_window;
        let attempts = self
            .store
            .count_login_attempts_since(ctx, ip, since)
            .await
            .map_err(internal)?;
        if attempts >= i64::from(self.config.login.ip_limit) {
            return Err(IamError::LoginRateLimited {
                retry_after_seconds: self.config.login.ip_window.as_secs(),
            });
        }
        Ok(())
    }

    // -----------------------------------------------------------------------
    // 令牌
    // -----------------------------------------------------------------------

    /// 建会话 + 签发令牌（access + refresh）。
    async fn issue_session(
        &self,
        ctx: &RequestContext,
        tenant: &str,
        user: &UserRecord,
        meta: &RequestMeta,
    ) -> Result<LoginResponse, IamError> {
        let session_id = Uuid::new_v4().to_string();
        let family_id = Uuid::new_v4().to_string();
        let refresh_token = crypto::new_refresh_token();
        let refresh_hash = crypto::digest_base64url(refresh_token.as_bytes());
        let expires_at = OffsetDateTime::now_utc() + self.config.tokens.refresh_ttl;

        let session = SessionRecord {
            id: session_id.clone(),
            tenant_id: tenant.to_string(),
            user_id: user.id.clone(),
            device_label: device_label(meta),
            ip: meta.ip.clone(),
            user_agent: meta.user_agent.clone(),
            family_id: family_id.clone(),
            created_at: OffsetDateTime::now_utc(),
            last_used_at: OffsetDateTime::now_utc(),
            revoked_at: None,
        };
        let refresh = RefreshRecord {
            id: Uuid::new_v4().to_string(),
            tenant_id: tenant.to_string(),
            user_id: user.id.clone(),
            session_id: session_id.clone(),
            family_id,
            token_hash: refresh_hash,
            issued_at: OffsetDateTime::now_utc(),
            expires_at,
            used_at: None,
            revoked_at: None,
            replaced_by: None,
        };

        // 会话与 refresh token **同生共死** —— 同一个事务。
        let mut tx = self.store.begin(ctx).await.map_err(internal)?;
        self.store
            .insert_session_in(&mut tx, &session)
            .await
            .map_err(internal)?;
        self.store
            .insert_refresh_token_in(&mut tx, &refresh)
            .await
            .map_err(internal)?;
        tx.commit().await.map_err(internal)?;

        let access_token = self.issue_access_token(&user.id, tenant, &session_id)?;
        Ok(LoginResponse {
            access_token,
            refresh_token,
            token_type: "Bearer".to_string(),
            expires_in: self.config.tokens.access_ttl.as_secs(),
            user: user_view(user),
            permissions: Vec::new(),
        })
    }

    /// 签发 access token（JWT）。
    fn issue_access_token(
        &self,
        user_id: &str,
        tenant: &str,
        session_id: &str,
    ) -> Result<String, IamError> {
        let now = OffsetDateTime::now_utc().unix_timestamp();
        let claims = AccessClaims {
            subject: user_id.to_string(),
            tenant_id: tenant.to_string(),
            session_id: session_id.to_string(),
            issuer: self.config.jwt.issuer.clone(),
            audience: self.config.jwt.audience.clone(),
            issued_at: now,
            not_before: now,
            expires_at: now + self.config.tokens.access_ttl.as_secs() as i64,
            token_id: Uuid::new_v4().to_string(),
        };
        self.keys.sign(&claims)
    }

    /// 验签 access token。
    pub fn verify_access_token(&self, token: &str) -> Result<AccessClaims, IamError> {
        self.keys
            .verify(token, &self.config.jwt.issuer, &self.config.jwt.audience)
    }

    // -----------------------------------------------------------------------
    // 刷新（轮换 + 重放检测）
    // -----------------------------------------------------------------------

    /// 用 refresh token 换新的令牌对。
    pub async fn refresh(
        &self,
        ctx: &RequestContext,
        req: &RefreshRequest,
    ) -> Result<LoginResponse, IamError> {
        let tenant = ctx.tenant_id().ok_or(IamError::TenantContextMissing)?;
        let token_hash = crypto::digest_base64url(req.refresh_token.as_bytes());

        let mut tx = self.store.begin(ctx).await.map_err(internal)?;
        let record = self
            .store
            .find_refresh_by_hash_for_update_in(&mut tx, &token_hash)
            .await
            .map_err(internal)?;

        let Some(record) = record else {
            let _ = tx.rollback().await;
            return Err(IamError::TokenRevoked);
        };

        let now = OffsetDateTime::now_utc();

        // 过期。
        if record.expires_at <= now {
            self.revoke_family(&mut tx, &record).await?;
            // ⚠️ 必须 **提交**：family 的作废要真的落库（回滚会把作废一起丢掉）。
            tx.commit().await.map_err(internal)?;
            return Err(IamError::TokenExpired);
        }

        // **已轮换** = 旧令牌被再次使用 = 泄露 → 整个 family 全部作废。
        // （这个判定必须排在 `revoked_at` 之前：family 被作废时旧令牌也会带上
        //   revoked_at，但「用过又再用」是更严重的信号。）
        if record.used_at.is_some() {
            self.revoke_family(&mut tx, &record).await?;
            // ⚠️ 必须 **提交**：整个 family 的作废是这次检测的**结果**，
            //    回滚等于「检测到了泄露却什么都没作废」—— 那是致命的。
            tx.commit().await.map_err(internal)?;
            return Err(IamError::TokenReuseDetected);
        }

        // 已撤销（登出 / 踢下线 / 全端失效）。
        if record.revoked_at.is_some() {
            let _ = tx.rollback().await;
            return Err(IamError::TokenRevoked);
        }

        // 正常轮换：旧令牌立即作废，签发新的（**同一个 family**）。
        let new_token = crypto::new_refresh_token();
        let new_record = RefreshRecord {
            id: Uuid::new_v4().to_string(),
            tenant_id: record.tenant_id.clone(),
            user_id: record.user_id.clone(),
            session_id: record.session_id.clone(),
            family_id: record.family_id.clone(),
            token_hash: crypto::digest_base64url(new_token.as_bytes()),
            issued_at: now,
            expires_at: now + self.config.tokens.refresh_ttl,
            used_at: None,
            revoked_at: None,
            replaced_by: None,
        };
        self.store
            .insert_refresh_token_in(&mut tx, &new_record)
            .await
            .map_err(internal)?;
        self.store
            .mark_refresh_used_in(&mut tx, &record.id, &new_record.id)
            .await
            .map_err(internal)?;
        self.store
            .touch_session_in(&mut tx, &record.session_id)
            .await
            .map_err(internal)?;
        tx.commit().await.map_err(internal)?;

        let access_token = self.issue_access_token(&record.user_id, tenant, &record.session_id)?;
        let user = self
            .store
            .find_user_by_id(ctx, &record.user_id)
            .await
            .map_err(internal)?
            .ok_or(IamError::TokenRevoked)?;
        Ok(LoginResponse {
            access_token,
            refresh_token: new_token,
            token_type: "Bearer".to_string(),
            expires_in: self.config.tokens.access_ttl.as_secs(),
            user: user_view(&user),
            permissions: Vec::new(),
        })
    }

    /// 作废整个 family + 撤销会话（重放检测 / 过期时的清理）。
    async fn revoke_family(
        &self,
        tx: &mut platform_db::TenantTx,
        record: &RefreshRecord,
    ) -> Result<(), IamError> {
        self.store
            .revoke_family_in(tx, &record.family_id)
            .await
            .map_err(internal)?;
        self.store
            .revoke_session_in(tx, &record.session_id)
            .await
            .map_err(internal)?;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // 登出与会话
    // -----------------------------------------------------------------------

    /// 登出当前会话（撤销该会话的 family）。
    pub async fn logout(&self, ctx: &RequestContext, session_id: &str) -> Result<(), IamError> {
        let session = self
            .store
            .find_session(ctx, session_id)
            .await
            .map_err(internal)?
            .ok_or(IamError::NotFound {
                resource: "session",
            })?;
        self.store
            .revoke_family(ctx, &session.family_id)
            .await
            .map_err(internal)?;
        self.store
            .revoke_session(ctx, session_id)
            .await
            .map_err(internal)?;
        Ok(())
    }

    /// 登出全端：撤销该用户的**全部**会话与 refresh family。
    pub async fn logout_all(&self, ctx: &RequestContext, user_id: &str) -> Result<u64, IamError> {
        self.store
            .revoke_all_refresh_tokens(ctx, user_id)
            .await
            .map_err(internal)?;
        let revoked = self
            .store
            .revoke_all_sessions(ctx, user_id)
            .await
            .map_err(internal)?;
        Ok(revoked)
    }

    /// 列出会话（设备会话列表）。
    pub async fn list_sessions(
        &self,
        ctx: &RequestContext,
        user_id: &str,
    ) -> Result<Vec<SessionView>, IamError> {
        let sessions = self
            .store
            .list_sessions(ctx, user_id)
            .await
            .map_err(internal)?;
        Ok(sessions.iter().map(session_view).collect())
    }

    /// 踢下线（撤销指定会话）—— **只能踢自己的会话**（别人的会话返回 404，不泄露存在性）。
    pub async fn revoke_session(
        &self,
        ctx: &RequestContext,
        user_id: &str,
        session_id: &str,
    ) -> Result<(), IamError> {
        let session = self
            .store
            .find_session(ctx, session_id)
            .await
            .map_err(internal)?
            .filter(|session| session.user_id == user_id)
            .ok_or(IamError::NotFound {
                resource: "session",
            })?;
        self.store
            .revoke_family(ctx, &session.family_id)
            .await
            .map_err(internal)?;
        self.store
            .revoke_session(ctx, session_id)
            .await
            .map_err(internal)?;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // 密码
    // -----------------------------------------------------------------------

    /// 发起密码找回。
    ///
    /// 返回 `Some(token)` = 该邮箱已注册（**调用方不得把它放进响应体**）；
    /// 返回 `None` = 未注册。两者对调用方的响应**完全一样**（design-summary §5.2 / D7）。
    pub async fn forgot_password(
        &self,
        ctx: &RequestContext,
        email: &str,
    ) -> Result<Option<String>, IamError> {
        let user = self
            .store
            .find_user_by_email(ctx, email.trim())
            .await
            .map_err(internal)?;
        let Some(user) = user else {
            // 未注册：**什么都不做**，响应与「已注册」完全相同。
            return Ok(None);
        };
        let token = crypto::new_reset_token();
        let expires_at = OffsetDateTime::now_utc() + self.config.tokens.reset_ttl;
        self.store
            .insert_password_reset(
                ctx,
                &Uuid::new_v4().to_string(),
                &user.id,
                &crypto::digest_base64url(token.as_bytes()),
                expires_at,
            )
            .await
            .map_err(internal)?;
        Ok(Some(token))
    }

    /// 用重置令牌改密码。
    pub async fn reset_password(
        &self,
        ctx: &RequestContext,
        req: &ResetPasswordRequest,
    ) -> Result<(), IamError> {
        check_password_policy(&req.new_password, &self.config.password_policy)?;
        let hash = crypto::digest_base64url(req.token.as_bytes());
        let record = self
            .store
            .find_password_reset(ctx, &hash)
            .await
            .map_err(internal)?
            .ok_or(IamError::TokenRevoked)?;
        if record.used_at.is_some() {
            return Err(IamError::TokenRevoked);
        }
        if record.expires_at <= OffsetDateTime::now_utc() {
            return Err(IamError::TokenExpired);
        }
        let user = self
            .store
            .find_user_by_id(ctx, &record.user_id)
            .await
            .map_err(internal)?
            .ok_or(IamError::TokenRevoked)?;

        self.apply_new_password(ctx, &user, &req.new_password, None)
            .await?;
        self.store
            .mark_password_reset_used(ctx, &record.id)
            .await
            .map_err(internal)?;
        Ok(())
    }

    /// 已认证用户改密（需校验当前密码）。
    pub async fn change_password(
        &self,
        ctx: &RequestContext,
        user_id: &str,
        current_session_id: &str,
        req: &ChangePasswordRequest,
    ) -> Result<(), IamError> {
        let user = self
            .store
            .find_user_by_id(ctx, user_id)
            .await
            .map_err(internal)?
            .ok_or(IamError::NotFound { resource: "user" })?;
        if !password::verify_password(&req.current_password, &user.password_hash) {
            return Err(IamError::InvalidCredentials);
        }
        check_password_policy(&req.new_password, &self.config.password_policy)?;
        self.apply_new_password(ctx, &user, &req.new_password, Some(current_session_id))
            .await
    }

    /// 应用新密码：历史不重复 → 落库 → **撤销 refresh family**。
    ///
    /// `keep_session_id` = `Some` 时保留当前会话（改密后踢出**其它**设备）；
    /// `None` 时撤销**全部**（密码重置场景）。
    async fn apply_new_password(
        &self,
        ctx: &RequestContext,
        user: &UserRecord,
        new_password: &str,
        keep_session_id: Option<&str>,
    ) -> Result<(), IamError> {
        // 历史不重复（与最近 N 条比对）。
        if self.config.password_policy.history > 0 {
            let history = self
                .store
                .recent_password_hashes(ctx, &user.id, self.config.password_policy.history as i64)
                .await
                .map_err(internal)?;
            for hash in history {
                if password::verify_password(new_password, &hash) {
                    return Err(IamError::PasswordReused);
                }
            }
        }

        let new_hash = password::hash_password(new_password, &self.config.argon2)?;
        self.store
            .set_password(ctx, &user.id, &new_hash)
            .await
            .map_err(internal)?;
        self.store
            .insert_password_history(ctx, &Uuid::new_v4().to_string(), &user.id, &new_hash)
            .await
            .map_err(internal)?;

        // 改密码 → **撤销全部 refresh family**（design-summary §7 / D10）。
        self.store
            .revoke_all_refresh_tokens(ctx, &user.id)
            .await
            .map_err(internal)?;
        match keep_session_id {
            Some(session_id) => {
                self.store
                    .revoke_other_sessions(ctx, &user.id, session_id)
                    .await
                    .map_err(internal)?;
            }
            None => {
                self.store
                    .revoke_all_sessions(ctx, &user.id)
                    .await
                    .map_err(internal)?;
            }
        }
        // 旧的重置链接立即失效。
        self.store
            .revoke_password_resets(ctx, &user.id)
            .await
            .map_err(internal)?;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // MFA
    // -----------------------------------------------------------------------

    /// 开始绑定 TOTP：生成 secret（**密文入库**）与一次性恢复码。
    pub async fn mfa_enroll(
        &self,
        ctx: &RequestContext,
        user_id: &str,
    ) -> Result<MfaEnrollResponse, IamError> {
        let (raw, base32) = totp::generate_secret();
        let cipher = crypto::encrypt(&raw, &self.config.secret_key)?;
        self.store
            .upsert_mfa_secret(ctx, user_id, &cipher)
            .await
            .map_err(internal)?;

        // 恢复码：只在绑定时给一次，**只存 hash**。
        let codes = totp::generate_recovery_codes(self.config.totp.recovery_code_count);
        let hashed: Vec<(String, String)> = codes
            .iter()
            .map(|code| {
                (
                    Uuid::new_v4().to_string(),
                    crypto::digest_base64url(totp::normalize_recovery_code(code).as_bytes()),
                )
            })
            .collect();
        self.store
            .insert_recovery_codes(ctx, user_id, &hashed)
            .await
            .map_err(internal)?;

        let otpauth_uri =
            totp::otpauth_uri(&self.config.jwt.issuer, user_id, &base32, &self.config.totp)?;
        Ok(MfaEnrollResponse {
            secret: base32,
            otpauth_uri,
            recovery_codes: codes,
        })
    }

    /// 校验并绑定 TOTP（绑定成功才把 `mfa_enabled` 打开）。
    pub async fn mfa_verify(
        &self,
        ctx: &RequestContext,
        user_id: &str,
        code: &str,
    ) -> Result<(), IamError> {
        self.verify_mfa_code(ctx, user_id, code).await?;
        self.store
            .set_mfa_enabled(ctx, user_id, true)
            .await
            .map_err(internal)?;
        Ok(())
    }

    /// 校验 MFA 验证码（TOTP 或恢复码）。
    async fn verify_mfa(
        &self,
        ctx: &RequestContext,
        user: &UserRecord,
        code: &str,
    ) -> Result<(), IamError> {
        self.verify_mfa_code(ctx, &user.id, code).await
    }

    /// 校验 MFA 验证码的公共实现。
    ///
    /// 纯数字的短码走 TOTP；其它走恢复码（一次性）。
    async fn verify_mfa_code(
        &self,
        ctx: &RequestContext,
        user_id: &str,
        code: &str,
    ) -> Result<(), IamError> {
        let secret = self
            .store
            .find_mfa_secret(ctx, user_id)
            .await
            .map_err(internal)?
            .ok_or(IamError::MfaInvalid)?;

        // 纯数字 → TOTP。
        if code.chars().all(|ch| ch.is_ascii_digit()) {
            let raw = crypto::decrypt(&secret.secret_cipher, &self.config.secret_key)?;
            let now = OffsetDateTime::now_utc().unix_timestamp();
            let step =
                totp::verify_code(&raw, code, secret.last_used_step, now, &self.config.totp)?;
            // 记录时间步 → 同一时间步的验证码**不能**用第二次（AC-S-015）。
            self.store
                .set_mfa_last_step(ctx, user_id, step)
                .await
                .map_err(internal)?;
            return Ok(());
        }

        // 恢复码：一次性。
        let normalized = totp::normalize_recovery_code(code);
        let candidates = self
            .store
            .unused_recovery_codes(ctx, user_id)
            .await
            .map_err(internal)?;
        for candidate in candidates {
            let expected = crypto::digest_base64url(normalized.as_bytes());
            if candidate.code_hash == expected {
                self.store
                    .mark_recovery_code_used(ctx, &candidate.id)
                    .await
                    .map_err(internal)?;
                return Ok(());
            }
        }
        Err(IamError::MfaInvalid)
    }
}

/// 账号是否处于锁定期。
fn is_locked(user: &UserRecord) -> bool {
    user.locked_until
        .is_some_and(|until| until > OffsetDateTime::now_utc())
}

/// 账号视图。
fn user_view(user: &UserRecord) -> UserView {
    UserView {
        id: user.id.clone(),
        username: user.username.clone(),
        email: user.email.clone(),
        tenant_id: user.tenant_id.clone(),
        mfa_enabled: user.mfa_enabled,
        must_change_password: user.must_change_password,
    }
}

/// 会话视图（时间统一转成 RFC 3339 的 UTC 字符串）。
fn session_view(session: &SessionRecord) -> SessionView {
    SessionView {
        id: session.id.clone(),
        device_label: session.device_label.clone(),
        ip: session.ip.clone(),
        user_agent: session.user_agent.clone(),
        created_at: rfc3339(session.created_at),
        last_used_at: rfc3339(session.last_used_at),
    }
}

/// 时间 → RFC 3339（UTC）。
fn rfc3339(moment: OffsetDateTime) -> String {
    use time::format_description::well_known::Rfc3339;
    moment
        .format(&Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string())
}

/// 设备标签：优先用请求头给的，否则从 UA 粗略推断。
fn device_label(meta: &RequestMeta) -> String {
    if !meta.device_label.trim().is_empty() {
        return meta.device_label.trim().to_string();
    }
    let agent = meta.user_agent.trim();
    if agent.is_empty() {
        return "unknown".to_string();
    }
    if agent.contains("Windows") {
        "Windows".to_string()
    } else if agent.contains("Mac OS") || agent.contains("Macintosh") {
        "macOS".to_string()
    } else if agent.contains("Android") {
        "Android".to_string()
    } else if agent.contains("iPhone") || agent.contains("iPad") {
        "iOS".to_string()
    } else if agent.contains("Linux") {
        "Linux".to_string()
    } else {
        "unknown".to_string()
    }
}

/// 数据库错误 → 内部错误（**不把内部细节漏给调用方**）。
fn internal(err: platform_db::DbError) -> IamError {
    tracing::error!(error = %err, "the iam data access failed");
    IamError::Internal {
        reason: "the data access layer failed".to_string(),
    }
}
