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
//! # 锁定判定排在**密码校验之后**（否则它就是一个枚举预言机）
//!
//! `ACCOUNT_LOCKED` 只有在**调用方已经证明知道密码**之后才可能出现：
//!
//! | 请求 | 账号不存在 | 账号存在 + 密码错 | 账号存在 + 密码对 + 已锁 |
//! |------|-----------|------------------|----------------------|
//! | 错误码 | `INVALID_CREDENTIALS` | `INVALID_CREDENTIALS` | `ACCOUNT_LOCKED` |
//!
//! 如果锁定判定排在密码校验**之前**，那么「第 6 次错密码返回 403」本身就泄露了
//! 「这个账号存在」—— **6 个请求就能枚举一个租户内的账号**（CWE-204）。
//!
//! 排在密码校验之后，两个约束**同时**成立：
//!
//! - 攻击者（不知道密码）**永远**只拿到 `INVALID_CREDENTIALS`，与不存在的账号完全一致；
//! - **知道密码**的被锁用户仍然拿到 `ACCOUNT_LOCKED`（前端要靠它提示「你被锁了」）。
//!
//! 顺带把锁定路径也纳入了 Argon2 校验 → 三条失败路径的耗时天然对齐。
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

use std::sync::{Arc, OnceLock};
use std::time::Duration;

use platform_core::RequestContext;
use platform_core::extension::audit::{AuditEvent, AuditSink, AuditSinkSlot};
use platform_core::extension::auditable::{AuditEventSpec, Auditable};
use platform_core::extension::permission::PermissionProvider;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::audit::login_event;
use crate::config::{IamConfig, check_password_policy};
use crate::crypto;
use crate::error::IamError;
use crate::jwt::{AccessClaims, JwtKeySet};
use crate::password::{self, DummyHash};
use crate::sso::audit::{
    self as sso_audit, NoopSsoAuditRecorder, SsoAuditEvent, SsoAuditPayload, SsoAuditRecorder,
};
use crate::sso::binding::{self, LDAP_PROVIDER};
use crate::sso::config::{OIDC_CLIENT_SECRET_ENV, SsoConfig, SsoConfigOutcome, SsoConfigSource};
use crate::sso::ldap::{self, SsoLdapClient};
use crate::sso::oidc::{self, OidcRuntime, PROVIDER as OIDC_PROVIDER};
use crate::sso::ticket;
use crate::sso::{AuthProvider, SsoProviderView};
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

/// LDAP 登录请求。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LdapLoginRequest {
    /// 目录里的用户名（会按 RFC 4515 转义后代入 filter）。
    pub username: String,
    /// 明文密码（**只在内存里**；LDAP 用户的密码永不落库）。
    pub password: String,
}

/// 票据交换请求（P4）。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExchangeTicketRequest {
    /// 一次性 login ticket。
    pub ticket: String,
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
    /// 登录来源：`local` / `oidc` / `ldap`（AC-S-107）。
    ///
    /// **两条会话唯一允许取值不同**的字段 —— 其余键必须完全一致（同一视图类型）。
    pub auth_provider: String,
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
    /// 权限点列表（**升序、去重**；已展开通配）。
    ///
    /// - **登录**时由 [`PermissionProvider`] 填充 —— 与
    ///   `GET /api/rbac/me/permissions` **同一份**数据（两个入口不得给出不同答案）；
    /// - **未注入**实现时是**空数组**（不是 `null`、不是省略字段）——
    ///   fail closed 的是「判定」，**不是**「登录」；
    /// - **刷新**响应恒为空数组（本期只改登录路径 —— 见 AC-S-065）。
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

/// 权限点提供者的**可选**槽位。
///
/// # 为什么要「后设注入」
///
/// 依赖方向是 `platform-rbac → platform-iam`；反过来**成环**
/// （且违反 AGENTS.md 规则 2）。所以 `platform-iam` 只认 `platform-core` 的
/// [`PermissionProvider`] trait，**由装配层在 RBAC 就绪后注入**。
///
/// # 为什么是 `OnceLock`
///
/// 注入只发生在**启动期一次**；`OnceLock` 正好表达「一次性、可并发读、线程安全」。
#[derive(Clone, Default)]
struct PermissionProviderSlot(OnceLock<Arc<dyn PermissionProvider>>);

impl PermissionProviderSlot {
    /// 注入实现；**重复注入返回 `false`**（不覆盖已装好的实现）。
    fn set(&self, provider: Arc<dyn PermissionProvider>) -> bool {
        self.0.set(provider).is_ok()
    }

    /// 读实现；**未注入 → `None`**。
    fn get(&self) -> Option<&Arc<dyn PermissionProvider>> {
        self.0.get()
    }
}

impl std::fmt::Debug for PermissionProviderSlot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PermissionProviderSlot")
            .field("installed", &self.0.get().is_some())
            .finish()
    }
}

/// SSO 的**后设槽**集合（与 [`PermissionProviderSlot`] 同一套模式）。
///
/// # 为什么都是「后设」而不是构造参数
///
/// 加进 `IamService::new` 会破 4 处既有调用点（含 `platform-rbac` 的 2 处测试，
/// 属已合并需求产物 —— D109 / S35）。后设槽既不改签名，也让「未装配」成为可表达状态。
///
/// # 未装配时的行为（**逐项不同，都是有意的**）
///
/// | 槽 | 未装配时 |
/// |----|---------|
/// | 配置源 | `sso.enforce` 视为 `false`（**本部署没装配 SSO**，不是「读了但失败」） |
/// | 审计 recorder | [`NoopSsoAuditRecorder`]：**每次 warn**（「以为在审计其实没有」比没有更危险） |
/// | LDAP 客户端 | 现造一个 [`crate::sso::ldap::Ldap3SsoClient`]（生产默认） |
/// | OIDC 出站客户端 | 懒建生产客户端（webpki 根） |
/// | 前端基址 | `""` → 302 用相对路径 `/sso/callback?ticket=…` |
/// | `client_secret` | 读环境变量 `BASE_SSO_OIDC_CLIENT_SECRET` |
/// | LDAP 服务账号 | 无（匿名 bind，**仍是一次 bind**） |
#[derive(Clone, Default)]
struct SsoState {
    config_source: OnceLock<Arc<dyn SsoConfigSource>>,
    audit_recorder: OnceLock<Arc<dyn SsoAuditRecorder>>,
    ldap_client: OnceLock<Arc<dyn SsoLdapClient>>,
    oidc: OidcRuntime,
    frontend_base_url: OnceLock<String>,
    oidc_client_secret: OnceLock<String>,
    ldap_service_bind: OnceLock<(String, String)>,
}

impl std::fmt::Debug for SsoState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SsoState")
            .field("config_source", &self.config_source.get().is_some())
            .field("audit_recorder", &self.audit_recorder.get().is_some())
            .field("ldap_client", &self.ldap_client.get().is_some())
            .finish()
    }
}

impl SsoState {
    /// 审计 recorder（未注入 → noop，但**会 warn**）。
    fn recorder(&self) -> Arc<dyn SsoAuditRecorder> {
        self.audit_recorder
            .get()
            .cloned()
            .unwrap_or_else(|| Arc::new(NoopSsoAuditRecorder))
    }

    /// `client_secret`：注入优先，其次环境变量（**唯一来源** —— AC-S-106）。
    fn client_secret(&self) -> String {
        self.oidc_client_secret
            .get()
            .cloned()
            .or_else(|| std::env::var(OIDC_CLIENT_SECRET_ENV).ok())
            .unwrap_or_default()
    }

    /// 前端基址（302 的目标前缀）。
    fn frontend_base_url(&self) -> String {
        self.frontend_base_url.get().cloned().unwrap_or_default()
    }
}

/// 认证与账号体系的服务。
#[derive(Debug, Clone)]
pub struct IamService {
    store: IamStore,
    config: IamConfig,
    keys: JwtKeySet,
    dummy_hash: DummyHash,
    /// 权限点提供者（**可选** —— 未注入时登录响应的 `permissions` 是空数组）。
    permission_provider: PermissionProviderSlot,
    /// SSO 入站（REQ-013）的后设槽。
    sso: SsoState,
    /// 平台审计 sink（REQ-014；**可选** —— 未注入时不埋点，但**每次 warn**）。
    audit: AuditSinkSlot,
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
            permission_provider: PermissionProviderSlot::default(),
            sso: SsoState::default(),
            audit: AuditSinkSlot::default(),
        })
    }

    // -----------------------------------------------------------------------
    // SSO 后设槽（REQ-013）
    // -----------------------------------------------------------------------

    /// 注入 SSO 配置源（由装配层用 `ConfigService::read_entries` 实现）。
    ///
    /// 返回 `false` = 已注入过（不覆盖）。
    pub fn set_sso_config_source(&self, source: Arc<dyn SsoConfigSource>) -> bool {
        self.sso.config_source.set(source).is_ok()
    }

    /// 注入审计 recorder（**后设槽**，不改 `new` 签名 —— S35）。
    ///
    /// 返回 `false` = 已注入过（不覆盖）。
    pub fn with_sso_audit_recorder(&self, recorder: Arc<dyn SsoAuditRecorder>) -> bool {
        self.sso.audit_recorder.set(recorder).is_ok()
    }

    /// 注入**平台审计 sink**（REQ-014 D150）。
    ///
    /// 装配层（`platform-server`）构造 `PgAuditSink` 后以
    /// `Arc<dyn AuditSink>` 注入到这里 —— `platform-iam` **只认 core 的 trait**，
    /// 不依赖 `platform-audit` → **不成环**。
    ///
    /// 返回 `false` = 已注入过（不覆盖）。
    pub fn with_audit_sink(&self, sink: Arc<dyn AuditSink>) -> bool {
        self.audit.set(sink)
    }

    /// 记一条**登录审计**事件（REQ-014 AC-S-127 / D157）。
    ///
    /// # 失败语义
    ///
    /// - **未装配 sink** → 跳过（并记一条 **warn**：「以为在审计其实没有」
    ///   比「没有审计」更危险）→ 登录**照常**（单测与裸用服务不装 sink）；
    /// - **装配了但写失败** → **拒绝登录**，返回 **503**
    ///   `AUDIT_SINK_UNAVAILABLE`（「宁可不登录，不可无审计地登录」，D157）。
    async fn record_audit(&self, ctx: &RequestContext, event: AuditEvent) -> Result<(), IamError> {
        let Some(sink) = self.audit.get() else {
            tracing::warn!(
                event = %event.action,
                "a login audit event was produced but no platform audit sink is installed; \
                 the event is NOT persisted"
            );
            return Ok(());
        };
        sink.record(ctx, &event).await.map_err(|error| {
            tracing::error!(
                event = %event.action,
                code = error.code(),
                "the platform audit sink could not persist a login audit event; \
                 the login is rejected (REQ-014 D157)"
            );
            IamError::AuditUnavailable {
                reason: error.detail(),
            }
        })
    }

    /// 注入 LDAP 客户端（生产用默认 [`crate::sso::ldap::Ldap3SsoClient`]；测试注入可计数实现）。
    pub fn set_sso_ldap_client(&self, client: Arc<dyn SsoLdapClient>) -> bool {
        self.sso.ldap_client.set(client).is_ok()
    }

    /// 注入 OIDC 出站 HTTP 客户端（测试注入带测试根的客户端 —— P10 / S13）。
    ///
    /// 返回 `false` = 已注入过（不覆盖）。
    pub fn set_sso_http_client(&self, client: Arc<oidc::OidcHttpClient>) -> bool {
        self.sso.oidc.install_http(client)
    }

    /// 设置前端基址（回调 302 的目标前缀，P4）。
    pub fn set_sso_frontend_base_url(&self, base_url: impl Into<String>) -> bool {
        self.sso.frontend_base_url.set(base_url.into()).is_ok()
    }

    /// 设置 OIDC `client_secret`（**只存在内存里** —— 不进配置 / 不进日志）。
    pub fn set_sso_oidc_client_secret(&self, secret: impl Into<String>) -> bool {
        self.sso.oidc_client_secret.set(secret.into()).is_ok()
    }

    /// 设置 LDAP 服务账号凭据（用于 search；不设则匿名 bind）。
    pub fn set_sso_ldap_service_bind(
        &self,
        dn: impl Into<String>,
        password: impl Into<String>,
    ) -> bool {
        self.sso
            .ldap_service_bind
            .set((dn.into(), password.into()))
            .is_ok()
    }

    /// 前端回调页的地址（P4）：`{前端基址}/sso/callback?<query>`。
    pub fn sso_frontend_callback_location(&self, query: &str) -> String {
        format!(
            "{}/sso/callback?{query}",
            self.sso.frontend_base_url().trim_end_matches('/')
        )
    }

    /// **失败**交接的地址：`{前端基址}/sso/callback?error=<CODE>`。
    ///
    /// `error` **只放错误码** —— 不放 `detail`、不放任何配置值（rev 3.1）。
    pub fn sso_callback_error_location(&self, code: &str) -> String {
        self.sso_frontend_callback_location(&format!("error={}", oidc::percent_encode(code)))
    }

    /// **成功**交接的地址：`{前端基址}/sso/callback?ticket=<ticket>`。
    pub fn sso_callback_ticket_location(&self, ticket: &str) -> String {
        self.sso_frontend_callback_location(&format!("ticket={}", oidc::percent_encode(ticket)))
    }

    /// 读一次 SSO 配置（区分「未装配 / 读到 / 读失败」）。
    async fn sso_config(&self, ctx: &RequestContext) -> SsoConfigOutcome {
        let Some(source) = self.sso.config_source.get() else {
            return SsoConfigOutcome::NotInstalled;
        };
        match source.read(ctx).await {
            Ok(config) => SsoConfigOutcome::Loaded(config),
            Err(_) => SsoConfigOutcome::Unreadable,
        }
    }

    /// 读配置并要求「已装配且可读」，否则 `SSO_PROVIDER_NOT_CONFIGURED`。
    async fn require_sso_config(&self, ctx: &RequestContext) -> Result<SsoConfig, IamError> {
        match self.sso_config(ctx).await {
            SsoConfigOutcome::Loaded(config) => Ok(config),
            // 未装配 / 读失败 —— 对未认证调用者都表现为「这个租户没有 SSO」。
            SsoConfigOutcome::NotInstalled | SsoConfigOutcome::Unreadable => {
                Err(IamError::SsoProviderNotConfigured)
            }
        }
    }

    /// 注入权限点提供者（**启动期一次**）。
    ///
    /// # 为什么是「后设」接口
    ///
    /// 依赖方向是 `platform-rbac → platform-iam`；反过来成环。
    /// 所以 `platform-iam` 只认 `platform-core` 的 [`PermissionProvider`] trait，
    /// 由装配层（`platform-server`）在 RBAC 就绪之后注入。
    ///
    /// # 未注入时
    ///
    /// 登录**仍然成功**，响应的 `permissions` 是**空数组** ——
    /// fail closed 的是「判定」，**不是**「登录」（design-summary §2.2）。
    ///
    /// 返回 `false` 表示**已经有实现**（不覆盖）。
    pub fn set_permission_provider(&self, provider: Arc<dyn PermissionProvider>) -> bool {
        self.permission_provider.set(provider)
    }

    /// 当前用户的有效权限点集合（**降级**：未注入 provider → 空数组）。
    ///
    /// # provider 读库失败怎么办
    ///
    /// 登录**仍然成功**，权限点集合降级为**空数组**
    /// （fail closed 的是「判定」，不是「登录」），
    /// 并记一条 **warn** 日志 —— 静默降级也是**静默放行**的一种
    /// （与 D27「告警 = 静默放行的一种」同一价值观）。
    async fn effective_permissions(&self, ctx: &RequestContext, user_id: &str) -> Vec<String> {
        let Some(provider) = self.permission_provider.get() else {
            return Vec::new();
        };
        match provider.effective_permissions(ctx, user_id).await {
            Ok(permissions) => permissions,
            Err(reason) => {
                tracing::warn!(
                    %reason,
                    user_id,
                    "the permission provider failed; the login response carries an empty permission set",
                );
                Vec::new()
            }
        }
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
            display_name: None,
            manager_id: None,
            departed_at: None,
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

        // 0) **强制 SSO 判定**（AC-S-021 / demand §2）。
        //
        // 必须在**加载用户 / 校验密码之前**：
        // - 先查用户再拒 → 不存在用户会走「查无此人」分支 → 403 vs 401 就是枚举预言机；
        // - 也避免在强制 SSO 期间白跑一次 Argon2id（低配机上是真实 CPU 成本）。
        self.enforce_sso(ctx).await?;

        // 1) 按 IP 的滑动窗口限流（瞬时打爆）：**只算失败尝试**，且**跨租户共享**。
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
            // 再对齐**数据库**耗时：走与「密码错误」路径**完全相同**的三条语句
            // （一次 UPDATE + 一次 INSERT 流水 + 一次 INSERT 失败计数，同一个事务），
            // 只是 UPDATE 命中 0 行。
            // 这样三条失败路径的耗时差只剩「行数」那点噪声 —— 时序枚举拿不到信号。
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
            self.store
                .record_login_rate_in(&mut tx, &Uuid::new_v4().to_string(), &ip_hash(&meta.ip))
                .await
                .map_err(internal)?;
            tx.commit().await.map_err(internal)?;
            // 🔑 登录失败**也要审计**（AC-S-127）—— 事件**不含账号名**。
            self.record_audit(
                ctx,
                login_event(tenant, None, false, false, &meta.ip, &meta.user_agent),
            )
            .await?;
            return Err(IamError::InvalidCredentials);
        };

        // 4) 校验密码 —— **必须排在锁定判定之前**。
        //    否则 `ACCOUNT_LOCKED` 就是一个账号枚举预言机：未注册账号永远拿不到它，
        //    已注册账号第 6 次就拿到 → 6 个请求即可枚举（CWE-204）。
        let now = OffsetDateTime::now_utc();
        if !password::verify_password(&req.password, &user.password_hash) {
            // 失败计数 + 尝试流水放**同一个事务**：两条失败路径的数据库往返次数一致，
            // 耗时差只剩「多一条 UPDATE」—— 时序枚举的信号被压到最小。
            self.record_failed_login(ctx, &user, meta, &req.username, now)
                .await?;
            // 🔑 失败审计 —— **不含账号名**（AC-S-127 步骤 5 的不可枚举判据）。
            //
            // ⚠️ 与「账号不存在」那条路径**完全对称**（各 1 条）——
            //    失败记录的条数不因账号是否存在而变化。
            self.record_audit(
                ctx,
                login_event(tenant, None, false, false, &meta.ip, &meta.user_agent),
            )
            .await?;
            return Err(IamError::InvalidCredentials);
        }

        // 5) 密码正确 → 这时才可以说「你的账号被锁了」。
        //    （走到这里的人**已经证明知道密码**，所以这不是枚举信号。）
        if is_locked(&user, now) {
            // 记一条失败流水，但**不续期、不增计数**（见 `record_failed_login`）。
            self.record_failed_login(ctx, &user, meta, &req.username, now)
                .await?;
            // 🔑 失败审计（与上面两条失败路径**对称**：各 1 条）。
            self.record_audit(
                ctx,
                login_event(tenant, None, false, false, &meta.ip, &meta.user_agent),
            )
            .await?;
            return Err(IamError::AccountLocked);
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
        // 🔑 成功审计（AC-S-127 步骤 1：产生 1 条 `iam.login.succeeded`）。
        self.record_audit(
            ctx,
            login_event(
                tenant,
                Some(&user.id),
                true,
                false,
                &meta.ip,
                &meta.user_agent,
            ),
        )
        .await?;
        Ok(response)
    }

    /// 记一次**失败的登录**（密码错误 **或** 账号被锁）并更新锁定状态。
    ///
    /// 三件事在**同一个事务**里 —— 见 [`IamService::login`] 的说明：
    /// 两条失败路径的数据库往返次数必须一致，否则耗时差会泄露账号是否存在。
    async fn record_failed_login(
        &self,
        ctx: &RequestContext,
        user: &UserRecord,
        meta: &RequestMeta,
        username: &str,
        now: OffsetDateTime,
    ) -> Result<(), IamError> {
        let (attempts, locked_until) = next_lock_state(
            user,
            now,
            self.config.login.account_failure_limit as i32,
            self.config.login.lockout_duration,
        );
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
        self.store
            .record_login_rate_in(&mut tx, &Uuid::new_v4().to_string(), &ip_hash(&meta.ip))
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
    ///
    /// 两条口径（QA 报告 LOW 6）：
    ///
    /// - **只算失败尝试**：成功登录**不进入**窗口 —— 否则共用一个 NAT 出口的
    ///   办公网在 5 分钟内 20 次正常登录就会被 429 误伤。
    /// - **跨租户共享**同一个 IP 的预算：同一个 IP 换租户不应该拿到一份新的额度，
    ///   否则限流预算会被放大成「租户数倍」。
    async fn check_ip_rate_limit(&self, ctx: &RequestContext, ip: &str) -> Result<(), IamError> {
        if ip.is_empty() {
            return Ok(());
        }
        let since = OffsetDateTime::now_utc() - self.config.login.ip_window;
        let failures = self
            .store
            .count_login_failures_since(ctx, &ip_hash(ip), since)
            .await
            .map_err(internal)?;
        if failures >= i64::from(self.config.login.ip_limit) {
            return Err(IamError::LoginRateLimited {
                retry_after_seconds: self.config.login.ip_window.as_secs(),
            });
        }
        Ok(())
    }

    // -----------------------------------------------------------------------
    // SSO（REQ-013）
    // -----------------------------------------------------------------------

    /// 强制 SSO 判定（**密码校验之前**）。
    ///
    /// 三种状态的处理（AC-S-021 / demand §2）：
    ///
    /// | 状态 | 行为 |
    /// |------|------|
    /// | 未装配配置源 | 放行（本部署没装配 SSO） |
    /// | 读到且 `enforce = false` | 放行 |
    /// | 读到且 `enforce = true` | `403 SSO_REQUIRED` + `params.ssoProviders` |
    /// | 注入过但**读失败** | `403 SSO_REQUIRED`（**fail closed**）+ warn |
    async fn enforce_sso(&self, ctx: &RequestContext) -> Result<(), IamError> {
        let outcome = self.sso_config(ctx).await;
        if !outcome.enforcement() {
            return Ok(());
        }
        let providers = match &outcome {
            SsoConfigOutcome::Loaded(config) => config.providers(),
            // 读失败时拿不到「可用 provider」，但不能因此放行 —— 空列表 + 403。
            SsoConfigOutcome::Unreadable | SsoConfigOutcome::NotInstalled => Vec::new(),
        };
        if matches!(outcome, SsoConfigOutcome::Unreadable) {
            // 静默把 enforce 当 false = 强制 SSO 悄悄失效（S16）。
            tracing::warn!(
                tenant = ctx.tenant_id().unwrap_or_default(),
                "the sso configuration could not be read; local password login is rejected (fail closed)"
            );
        }
        Err(IamError::SsoRequired { providers })
    }

    /// 当前租户可用的 SSO provider 列表（登录页用；**不需要认证**）。
    pub async fn sso_providers(
        &self,
        ctx: &RequestContext,
    ) -> Result<Vec<SsoProviderView>, IamError> {
        match self.sso_config(ctx).await {
            SsoConfigOutcome::Loaded(config) => Ok(config.providers()),
            // 未装配 / 读失败 → 空列表（200）。
            // 「查了但没有」不是「没有这个接口」，也不是 500。
            SsoConfigOutcome::NotInstalled => Ok(Vec::new()),
            SsoConfigOutcome::Unreadable => {
                tracing::warn!(
                    tenant = ctx.tenant_id().unwrap_or_default(),
                    "the sso configuration could not be read; the provider list is reported as empty"
                );
                Ok(Vec::new())
            }
        }
    }

    /// 发起 OIDC 授权：落一条 `oidc_state`，返回 302 的 `Location`。
    pub async fn begin_oidc_authorize(&self, ctx: &RequestContext) -> Result<String, IamError> {
        let config = self.require_sso_config(ctx).await?;
        config
            .validate()
            .map_err(|reason| IamError::SsoConfigInvalid { reason })?;
        if !config.oidc.enabled {
            return Err(IamError::SsoProviderNotConfigured);
        }

        let discovery = self.sso.oidc.discovery(&config.oidc.issuer, false).await?;

        let state = oidc::random_token();
        let nonce = oidc::random_token();
        let verifier = oidc::new_code_verifier();
        let challenge = oidc::code_challenge(&verifier);

        let record = store::OidcStateRecord {
            state: state.clone(),
            tenant_id: ctx
                .tenant_id()
                .ok_or(IamError::TenantContextMissing)?
                .to_string(),
            provider: OIDC_PROVIDER.to_string(),
            nonce: nonce.clone(),
            code_verifier: verifier,
            code_challenge: challenge.clone(),
            redirect_uri: config.oidc.redirect_uri.clone(),
            created_at: OffsetDateTime::now_utc(),
            expires_at: OffsetDateTime::now_utc() + oidc::OIDC_STATE_TTL,
            consumed_at: None,
        };
        self.store
            .insert_oidc_state(ctx, &record)
            .await
            .map_err(internal)?;

        Ok(oidc::authorization_url(
            &discovery.authorization_endpoint,
            &config.oidc,
            &state,
            &nonce,
            &challenge,
        ))
    }

    /// 完成 OIDC 回调：校验 → 换 token → 验签 → 绑定 → 建会话 → 落 ticket。
    ///
    /// 返回 `302` 的目标（`{前端基址}/sso/callback?ticket=…`）。
    pub async fn complete_oidc_callback(
        &self,
        header_tenant: Option<&str>,
        code: &str,
        state: &str,
        meta: &RequestMeta,
    ) -> Result<String, IamError> {
        let outcome = self
            .do_oidc_callback(header_tenant, code, state, meta)
            .await;
        match &outcome {
            Ok((tenant, user, sub, _session_id)) => {
                self.record_sso_success("oidc", sub, &user.id, Some(tenant), meta);
            }
            Err(error) => {
                self.record_sso_failure("oidc", error.code(), header_tenant, meta);
            }
        }
        // 🔑 SSO 登录审计（AC-S-127 步骤 3：产生 1 条 `iam.sso.login.succeeded`）。
        //
        // ⚠️ **失败时可能拿不到租户**（回调还没解析出 state 的租户）→
        //    审计记录**必须**属于某个租户 → 拿不到就**跳过并 warn**。
        let tenant = match &outcome {
            Ok((tenant, _user, _sub, _session_id)) => Some(tenant.clone()),
            Err(_error) => header_tenant.map(|tenant| tenant.to_string()),
        };
        if let Some(tenant) = tenant {
            let ctx = RequestContext::new().with_tenant(tenant.clone());
            let event = match &outcome {
                Ok((_tenant, user, _sub, _session_id)) => login_event(
                    &tenant,
                    Some(&user.id),
                    true,
                    true,
                    &meta.ip,
                    &meta.user_agent,
                ),
                Err(_error) => login_event(&tenant, None, false, true, &meta.ip, &meta.user_agent),
            };
            // 🔑 审计写不进去 → **拒绝登录**（D157）。
            self.record_audit(&ctx, event).await?;
        } else {
            tracing::warn!(
                "an SSO login audit event was produced but the tenant is unknown; \
                 the event is NOT persisted"
            );
        }
        outcome.map(|(_, _, _, location)| location)
    }

    #[allow(clippy::type_complexity)]
    async fn do_oidc_callback(
        &self,
        header_tenant: Option<&str>,
        code: &str,
        state: &str,
        meta: &RequestMeta,
    ) -> Result<(String, UserRecord, String, String), IamError> {
        // 1) 读 state（**不带租户过滤**：回调时还没有权威租户来源）。
        let record = self
            .store
            .find_oidc_state(state)
            .await
            .map_err(internal)?
            .ok_or(IamError::SsoOidcStateInvalid)?;
        if record.consumed_at.is_some() {
            return Err(IamError::SsoOidcStateInvalid);
        }
        if record.expires_at <= OffsetDateTime::now_utc() {
            return Err(IamError::SsoOidcStateExpired);
        }
        if record.provider != OIDC_PROVIDER {
            return Err(IamError::SsoOidcStateInvalid);
        }

        // 2) 租户归属（P3）：有头必须相等，无头**采纳 state 租户**。
        let tenant = oidc::resolve_callback_tenant(header_tenant, &record.tenant_id)?;
        let ctx = RequestContext::new().with_tenant(tenant.clone());

        // 3) 配置（此时租户已确定）。
        let config = self.require_sso_config(&ctx).await?;
        config
            .validate()
            .map_err(|reason| IamError::SsoConfigInvalid { reason })?;
        if !config.oidc.enabled {
            return Err(IamError::SsoProviderNotConfigured);
        }

        // 4) 原子消费（**租户作用域**：跨租户连「消费掉」都做不到）。
        let consumed = self
            .store
            .consume_oidc_state(&tenant, state)
            .await
            .map_err(internal)?;
        if consumed == 0 {
            return Err(IamError::SsoOidcStateInvalid);
        }

        // 5) PKCE：**先本地重算** challenge 比对（AC-S-101 步骤 6 的观测面）。
        if oidc::code_challenge(&record.code_verifier) != record.code_challenge {
            return Err(IamError::SsoOidcPkceMismatch);
        }

        // 6) 发现文档 → 换 token。
        let discovery = self.sso.oidc.discovery(&config.oidc.issuer, false).await?;
        let client_secret = self.sso.client_secret();
        let http = self.sso.oidc.http()?;
        let token = oidc::exchange_code(
            &http,
            &discovery.token_endpoint,
            &config.oidc,
            &client_secret,
            code,
            &record.code_verifier,
            &record.redirect_uri,
        )
        .await?;

        // 7) 验签（未知 kid → **强制刷新一次** JWKS 再试，AC-S-102 步骤 7）。
        let mut keys = self
            .sso
            .oidc
            .signing_keys(&config.oidc.issuer, &discovery.jwks_uri, false)
            .await?;
        let claims = match oidc::verify_id_token_with(
            &token.id_token,
            &keys,
            &config.oidc.issuer,
            &config.oidc.client_id,
            Some(&record.nonce),
        ) {
            Ok(claims) => claims,
            Err(IamError::SsoOidcTokenInvalid) => {
                keys = self
                    .sso
                    .oidc
                    .signing_keys(&config.oidc.issuer, &discovery.jwks_uri, true)
                    .await?;
                oidc::verify_id_token_with(
                    &token.id_token,
                    &keys,
                    &config.oidc.issuer,
                    &config.oidc.client_id,
                    Some(&record.nonce),
                )?
            }
            Err(other) => return Err(other),
        };

        // 8) 绑定 / JIT。
        let display_name = claims
            .name
            .clone()
            .or_else(|| claims.preferred_username.clone());
        let email = claims
            .email
            .clone()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        let binding = binding::resolve(
            &self.store,
            &ctx,
            &config,
            OIDC_PROVIDER,
            &claims.sub,
            email.as_deref(),
            display_name.as_deref(),
        )
        .await?;

        if binding.user.status != STATUS_ACTIVE {
            return Err(IamError::AccountDisabled);
        }

        // 9) 会话（`auth_provider = oidc`）+ 一次性 ticket。
        let session = self
            .create_session(&ctx, &tenant, &binding.user, meta, AuthProvider::Oidc)
            .await?;
        let ticket = ticket::new_ticket();
        ticket::issue(&self.store, &ctx, &ticket, &binding.user.id, &session.id).await?;

        if binding.created_user || binding.created_link {
            self.sso.recorder().record(&SsoAuditEvent::new(
                sso_audit::EVENT_LINK_CREATED,
                SsoAuditPayload::link_created(OIDC_PROVIDER, &claims.sub, &binding.user.id)
                    .with_tenant(&tenant),
            ));
        }

        let location = self.sso_callback_ticket_location(&ticket);
        Ok((tenant, binding.user, claims.sub, location))
    }

    /// 用一次性 ticket 换会话（P4）。
    ///
    /// `header_tenant` 有值时必须是票据的租户（否则 `401 SSO_TICKET_INVALID`）。
    pub async fn exchange_sso_ticket(
        &self,
        header_tenant: Option<&str>,
        ticket: &str,
    ) -> Result<LoginResponse, IamError> {
        let record = ticket::consume(&self.store, ticket, header_tenant).await?;
        let ctx = RequestContext::new().with_tenant(record.tenant_id.clone());

        let session = self
            .store
            .find_session(&ctx, &record.session_id)
            .await
            .map_err(internal)?
            .ok_or(IamError::SsoTicketInvalid)?;
        let user = self
            .store
            .find_user_by_id(&ctx, &record.user_id)
            .await
            .map_err(internal)?
            .ok_or(IamError::SsoTicketInvalid)?;
        if user.status != STATUS_ACTIVE {
            return Err(IamError::AccountDisabled);
        }

        self.issue_tokens_for_session(&ctx, &record.tenant_id, &user, &session)
            .await
    }

    /// LDAP 登录（bind 认证）。
    pub async fn login_via_ldap(
        &self,
        ctx: &RequestContext,
        req: &LdapLoginRequest,
        meta: &RequestMeta,
    ) -> Result<LoginResponse, IamError> {
        let outcome = self.do_ldap_login(ctx, req, meta).await;
        match &outcome {
            Ok((sub, user_id, _response)) => {
                self.record_sso_success(LDAP_PROVIDER, sub, user_id, ctx.tenant_id(), meta)
            }
            Err(error) => {
                self.record_sso_failure(LDAP_PROVIDER, error.code(), ctx.tenant_id(), meta)
            }
        }
        // 🔑 LDAP 登录按 D163 记 `iam.login.*`（**不是** `iam.sso.*`）。
        //
        // ⚠️ 审计**失败 → 拒绝登录**（D157）—— 这里的错误**替换**原错误。
        let tenant = ctx.tenant_id().unwrap_or_default().to_string();
        let audit = match &outcome {
            Ok((_sub, user_id, _response)) => login_event(
                &tenant,
                Some(user_id.as_str()),
                true,
                false,
                &meta.ip,
                &meta.user_agent,
            ),
            Err(_error) => login_event(&tenant, None, false, false, &meta.ip, &meta.user_agent),
        };
        self.record_audit(ctx, audit).await?;
        outcome.map(|(_, _, response)| response)
    }

    async fn do_ldap_login(
        &self,
        ctx: &RequestContext,
        req: &LdapLoginRequest,
        meta: &RequestMeta,
    ) -> Result<(String, String, LoginResponse), IamError> {
        let tenant = ctx.tenant_id().ok_or(IamError::TenantContextMissing)?;
        let config = self.require_sso_config(ctx).await?;
        config
            .validate()
            .map_err(|reason| IamError::SsoConfigInvalid { reason })?;
        if !config.ldap.enabled {
            return Err(IamError::SsoProviderNotConfigured);
        }

        // 限流：与本地登录共用同一套按 IP 的预算。
        self.check_ip_rate_limit(ctx, &meta.ip).await?;

        // **空密码在连接之前拦截**（AC-S-104 步骤 4/5/8：connect == 0、bind == 0）。
        if ldap::is_empty_password(&req.password) {
            return Err(IamError::SsoLdapEmptyPassword);
        }

        let client = self.ldap_client(&config.ldap.url);
        let service_bind = self
            .sso
            .ldap_service_bind
            .get()
            .map(|(dn, password)| (dn.as_str(), password.as_str()));

        let identity = match ldap::authenticate(
            client.as_ref(),
            &config.ldap,
            service_bind,
            req.username.trim(),
            &req.password,
        )
        .await
        {
            Ok(identity) => identity,
            Err(ldap::SsoLdapError::Bind) => {
                // 失败也要计入限流（AC-S-104 步骤 7）。
                self.record_sso_failure_rate(ctx, meta, &req.username)
                    .await?;
                return Err(IamError::SsoLdapBindFailed);
            }
            Err(ldap::SsoLdapError::Connection)
            | Err(ldap::SsoLdapError::Search)
            | Err(ldap::SsoLdapError::Config) => return Err(IamError::SsoLdapUnreachable),
        };

        let binding = binding::resolve(
            &self.store,
            ctx,
            &config,
            LDAP_PROVIDER,
            &identity.dn,
            identity.email.as_deref(),
            None,
        )
        .await?;

        if binding.user.status != STATUS_ACTIVE {
            return Err(IamError::AccountDisabled);
        }

        self.store
            .reset_login_failures(ctx, &binding.user.id)
            .await
            .map_err(internal)?;
        self.record_attempt(ctx, meta, &req.username, true).await?;

        let session = self
            .create_session(ctx, tenant, &binding.user, meta, AuthProvider::Ldap)
            .await?;
        if binding.created_user || binding.created_link {
            self.sso.recorder().record(&SsoAuditEvent::new(
                sso_audit::EVENT_LINK_CREATED,
                SsoAuditPayload::link_created(LDAP_PROVIDER, &identity.dn, &binding.user.id)
                    .with_tenant(tenant),
            ));
        }
        let response = self
            .issue_tokens_for_session(ctx, tenant, &binding.user, &session)
            .await?;
        Ok((identity.dn, binding.user.id, response))
    }

    /// LDAP 客户端：注入优先；否则按配置现造一个 `ldap3` 实现（每次登录新建连接）。
    fn ldap_client(&self, url: &str) -> Arc<dyn SsoLdapClient> {
        self.sso
            .ldap_client
            .get()
            .cloned()
            .unwrap_or_else(|| Arc::new(ldap::Ldap3SsoClient::new(url)))
    }

    /// 发一条「成功」审计事件。
    ///
    /// ⚠️ **既有 recorder**（REQ-013 的接缝）**照旧**发出 —— 签名与行为都不变；
    /// 平台 sink 的落库由**服务自己**做（见 `complete_oidc_callback`）。
    fn record_sso_success(
        &self,
        provider: &str,
        external_id: &str,
        user_id: &str,
        tenant_id: Option<&str>,
        meta: &RequestMeta,
    ) {
        let mut payload =
            SsoAuditPayload::succeeded(provider, external_id, user_id, &meta.ip, &meta.user_agent);
        if let Some(tenant) = tenant_id {
            payload = payload.with_tenant(tenant);
        }
        self.sso.recorder().record(&SsoAuditEvent::new(
            sso_audit::EVENT_LOGIN_SUCCEEDED,
            payload,
        ));
    }

    /// 发一条「失败」审计事件（**只带 provider / code / ip**）。
    fn record_sso_failure(
        &self,
        provider: &str,
        code: &str,
        tenant_id: Option<&str>,
        meta: &RequestMeta,
    ) {
        let mut payload = SsoAuditPayload::failed(provider, code, &meta.ip);
        if let Some(tenant) = tenant_id {
            payload = payload.with_tenant(tenant);
        }
        self.sso
            .recorder()
            .record(&SsoAuditEvent::new(sso_audit::EVENT_LOGIN_FAILED, payload));
    }

    /// 记一次 SSO 失败（只进限流桶与尝试流水，不动账号锁定 —— 目录里的账号不在本地）。
    async fn record_sso_failure_rate(
        &self,
        ctx: &RequestContext,
        meta: &RequestMeta,
        username: &str,
    ) -> Result<(), IamError> {
        let mut tx = self.store.begin(ctx).await.map_err(internal)?;
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
        self.store
            .record_login_rate_in(&mut tx, &Uuid::new_v4().to_string(), &ip_hash(&meta.ip))
            .await
            .map_err(internal)?;
        tx.commit().await.map_err(internal)
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
        let session = SessionRecord {
            id: Uuid::new_v4().to_string(),
            tenant_id: tenant.to_string(),
            user_id: user.id.clone(),
            device_label: device_label(meta),
            ip: meta.ip.clone(),
            user_agent: meta.user_agent.clone(),
            family_id: Uuid::new_v4().to_string(),
            auth_provider: AuthProvider::Local.as_str().to_string(),
            created_at: OffsetDateTime::now_utc(),
            last_used_at: OffsetDateTime::now_utc(),
            revoked_at: None,
        };
        let refresh_token = crypto::new_refresh_token();
        let refresh = RefreshRecord {
            id: Uuid::new_v4().to_string(),
            tenant_id: tenant.to_string(),
            user_id: user.id.clone(),
            session_id: session.id.clone(),
            family_id: session.family_id.clone(),
            token_hash: crypto::digest_base64url(refresh_token.as_bytes()),
            issued_at: OffsetDateTime::now_utc(),
            expires_at: OffsetDateTime::now_utc() + self.config.tokens.refresh_ttl,
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

        self.finish_login_response(ctx, tenant, user, &session.id, refresh_token)
            .await
    }

    /// 建一条会话（**不签 refresh token**）。
    ///
    /// 拆成两步是为了 ticket 交接（P4）：回调只建会话 + 落 ticket，
    /// token 在 `POST /sso/exchange` 的响应体里才出现。
    async fn create_session(
        &self,
        ctx: &RequestContext,
        tenant: &str,
        user: &UserRecord,
        meta: &RequestMeta,
        provider: AuthProvider,
    ) -> Result<SessionRecord, IamError> {
        let session = SessionRecord {
            id: Uuid::new_v4().to_string(),
            tenant_id: tenant.to_string(),
            user_id: user.id.clone(),
            device_label: device_label(meta),
            ip: meta.ip.clone(),
            user_agent: meta.user_agent.clone(),
            family_id: Uuid::new_v4().to_string(),
            auth_provider: provider.as_str().to_string(),
            created_at: OffsetDateTime::now_utc(),
            last_used_at: OffsetDateTime::now_utc(),
            revoked_at: None,
        };
        self.store
            .insert_session(ctx, &session)
            .await
            .map_err(internal)?;
        Ok(session)
    }

    /// 给一条**已存在**的会话签发 refresh token，并组装 `LoginResponse`。
    async fn issue_tokens_for_session(
        &self,
        ctx: &RequestContext,
        tenant: &str,
        user: &UserRecord,
        session: &SessionRecord,
    ) -> Result<LoginResponse, IamError> {
        let refresh_token = crypto::new_refresh_token();
        let refresh = RefreshRecord {
            id: Uuid::new_v4().to_string(),
            tenant_id: tenant.to_string(),
            user_id: user.id.clone(),
            session_id: session.id.clone(),
            family_id: session.family_id.clone(),
            token_hash: crypto::digest_base64url(refresh_token.as_bytes()),
            issued_at: OffsetDateTime::now_utc(),
            expires_at: OffsetDateTime::now_utc() + self.config.tokens.refresh_ttl,
            used_at: None,
            revoked_at: None,
            replaced_by: None,
        };
        self.store
            .insert_refresh_token(ctx, &refresh)
            .await
            .map_err(internal)?;

        self.finish_login_response(ctx, tenant, user, &session.id, refresh_token)
            .await
    }

    /// 组装登录响应（access token + 权限点集合 + 账号视图）。
    async fn finish_login_response(
        &self,
        ctx: &RequestContext,
        tenant: &str,
        user: &UserRecord,
        session_id: &str,
        refresh_token: String,
    ) -> Result<LoginResponse, IamError> {
        let access_token = self.issue_access_token(&user.id, tenant, session_id)?;
        // 权限点集合：与 `GET /api/rbac/me/permissions` **同一份**数据
        // （两个入口不得给出不同答案 —— 否则前端 `can()` 与后端 403 会打架）。
        let permissions = self.effective_permissions(ctx, &user.id).await;
        Ok(LoginResponse {
            access_token,
            refresh_token,
            token_type: "Bearer".to_string(),
            expires_in: self.config.tokens.access_ttl.as_secs(),
            user: user_view(user),
            permissions,
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
            // 刷新响应**本期不带**权限点（AC-S-065 只要求登录响应带）。
            // 客户端拿权限点走 `GET /api/rbac/me/permissions`，或用登录时的那一份。
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
    /// `keep_session_id` = `Some` 时**保留当前会话的 family**（改密后踢出**其它**设备）；
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

        // 改密码 → **撤销除当前 family 外的全部 refresh family**（design-summary §7 / D10）。
        //
        // ⚠️ **当前设备自己的 family 必须保留** —— 否则用户在自己设备上改完密，
        //    当前设备的 refresh 立刻失效，等于逼他重新登录（偏离设计摘要 §7：
        //    「改密后踢出**其他**设备」）。
        match keep_session_id {
            Some(session_id) => {
                let session = self
                    .store
                    .find_session(ctx, session_id)
                    .await
                    .map_err(internal)?
                    .ok_or(IamError::NotFound {
                        resource: "session",
                    })?;
                self.store
                    .revoke_other_refresh_tokens(ctx, &user.id, &session.family_id)
                    .await
                    .map_err(internal)?;
                self.store
                    .revoke_other_sessions(ctx, &user.id, session_id)
                    .await
                    .map_err(internal)?;
            }
            None => {
                self.store
                    .revoke_all_refresh_tokens(ctx, &user.id)
                    .await
                    .map_err(internal)?;
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

/// 平台契约要求的**声明面**（与 payload 接缝是两件不同的事 —— P14）。
///
/// `platform_core` 只有 [`Auditable`] + [`AuditEventSpec`]（**没有 payload 字段**），
/// 所以「声明」走这里，「发出」走 [`SsoAuditRecorder`]。
impl Auditable for IamService {
    fn audit_events(&self) -> &[AuditEventSpec] {
        sso_audit::audit_events()
    }
}

/// 账号是否处于锁定期（`now` 由调用方传入 → 判定与状态更新用**同一个时刻**）。
fn is_locked(user: &UserRecord, now: OffsetDateTime) -> bool {
    user.locked_until.is_some_and(|until| until > now)
}

/// 一次失败登录之后的**失败计数**与**锁定到期时间**。
///
/// | 登录前的状态 | 失败计数 | `locked_until` |
/// |-------------|---------|----------------|
/// | 未锁定 | +1 | 达阈值则 `now + 锁定时长`，否则 `NULL` |
/// | **锁定期内** | **不变**（不增） | **不变**（**不续期**） |
/// | 锁定**已到期** | **从头开始**（= 1） | `NULL` |
///
/// 「不续期」是必须的：否则攻击者每 15 分钟发一次错密码就能把受害者的锁
/// **无限延长** —— 受害者连**正确密码**都登不进去（未认证的账号锁死 DoS）。
fn next_lock_state(
    user: &UserRecord,
    now: OffsetDateTime,
    limit: i32,
    lockout: Duration,
) -> (i32, Option<OffsetDateTime>) {
    // 已经在锁定期 → **不续期、不增计数**。
    if let Some(until) = user.locked_until.filter(|until| *until > now) {
        return (user.failed_attempts, Some(until));
    }
    // 上一轮锁定**已到期** → 失败计数从头开始（不接着旧的累计）。
    let attempts = if user.locked_until.is_some() {
        1
    } else {
        user.failed_attempts + 1
    };
    let locked_until = if attempts >= limit {
        Some(now + lockout)
    } else {
        None
    };
    (attempts, locked_until)
}

/// IP → 摘要（全局限流表里**不存明文 IP**）。
fn ip_hash(ip: &str) -> String {
    crypto::digest_base64url(ip.as_bytes())
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
        auth_provider: session.auth_provider.clone(),
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
