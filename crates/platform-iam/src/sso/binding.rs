//! 账号绑定与 JIT 建号（REQ-013 AC-S-103 / AC-S-105）。
//!
//! # 绑定**绝不可改指**（P7 / S19）
//!
//! `platform-org` 的 `upsert_external_identity` 是
//! `ON CONFLICT (tenant_id, provider, external_id) DO UPDATE SET user_id = EXCLUDED.user_id`。
//! 那在组织同步里合理（管理员改了映射），**在认证路径上是账号接管**：
//! 同一个外部身份再写一次，绑定就被静默改到另一个本地用户。
//!
//! 所以认证路径一律 **`ON CONFLICT DO NOTHING` + 读回比对**：
//! 读回的 `user_id` 与期望不一致 → `409 SSO_IDENTITY_LINK_CONFLICT`，**不改**。
//!
//! # 匹配顺序（P6）
//!
//! 1. 先查 `(provider, external_id)` 绑定；命中即用
//! 2. 未命中再按 `mail` 匹配本地账号
//! 3. 匹配到的本地账号**在该 provider 下已绑了别的 `external_id`** → `409`（不静默接管）
//! 4. 都没有：`jitProvisioning = true` 建号 + 绑定；否则 `403 SSO_ACCOUNT_NOT_LINKED`
//!
//! # JIT 建号规则（P8）
//!
//! - `username` = email 的 `@` 前部分；**本租户已存在 → `409`**（不接管既有账号）
//! - `password_hash` = **永不匹配**的固定 PHC 串（列是 NOT NULL）
//! - **并发幂等**：`INSERT … ON CONFLICT DO NOTHING` → 重查 → 绑定 `DO NOTHING` + 读回比对。
//!   并发下允许返回 `409`，但 **`5xx` 必须为 0**

use platform_core::RequestContext;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::error::IamError;
use crate::store::{self, IamStore, UserRecord};

use super::config::SsoConfig;

/// LDAP 的 provider 名（P13：单一 LDAP 源）。
pub const LDAP_PROVIDER: &str = "ldap";

/// **永不匹配**的占位密码 PHC 串。
///
/// `password_hash` 是 `NOT NULL`；SSO 账号的本地密码必须**永远校验失败**，
/// 否则「SSO 建出来的账号能用某个口令本地登录」就成了后门。
///
/// 形态是合法的 Argon2id PHC 串（`verify_password` 能解析），
/// 摘要部分是 32 字节全零 —— Argon2 的输出撞上它的概率是 2^-256。
pub const UNMATCHABLE_PASSWORD_HASH: &str = concat!(
    "$argon2id$v=19$m=19456,t=2,p=1$",
    "c3NvLXBsYWNlaG9sZGVy",
    "$",
    "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
);

/// 一次绑定解析的结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BindingOutcome {
    /// 命中的本地账号。
    pub user: UserRecord,
    /// 本次是否**新建**了账号。
    pub created_user: bool,
    /// 本次是否**新建**了绑定。
    pub created_link: bool,
}

/// 解析（必要时建立）外部身份 → 本地账号的绑定。
///
/// `provider` 用 [`OIDC_PROVIDER`] / [`LDAP_PROVIDER`]；
/// `external_id` 是 OIDC 的 `sub` / LDAP 的 **DN**。
pub async fn resolve(
    store: &IamStore,
    ctx: &RequestContext,
    config: &SsoConfig,
    provider: &str,
    external_id: &str,
    email: Option<&str>,
    display_name: Option<&str>,
) -> Result<BindingOutcome, IamError> {
    let email = email
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);

    // 1) 先查绑定（P6 第一步）。
    if let Some(binding) = store
        .find_external_identity(ctx, provider, external_id)
        .await
        .map_err(internal)?
    {
        let user = store
            .find_user_by_id(ctx, &binding.user_id)
            .await
            .map_err(internal)?
            // 绑定指向一个不存在的账号 = 数据损坏。不回退到「重新建号」
            // （那会让一次脏数据把账号换掉）。
            .ok_or_else(|| IamError::Internal {
                reason: "the external identity points to a missing user".to_string(),
            })?;

        // ⚠️ **绑定一致性校验**：目录说这个外部身份的邮箱属于 A，库里却绑给了 B ——
        // 这只能是「绑定被改指过」（组织同步的 DO UPDATE 语义，或人工改库）。
        // 认证路径**绝不**默默采信被改指的绑定（P7 / AC-S-103 步骤 7）：
        // 返回 409，且**不改动**那一行。
        //
        // 正常场景不受影响：目录里改了 mail（AC-S-105 步骤 7）时，
        // 新邮箱匹配不到任何本地账号 → 不触发；邮箱仍属于同一个账号 → 也不触发。
        if let Some(email) = email.as_deref() {
            if let Some(owner) = store
                .find_user_by_email(ctx, email)
                .await
                .map_err(internal)?
            {
                if owner.id != user.id {
                    tracing::warn!(
                        provider,
                        bound_user = user.id,
                        email_owner = owner.id,
                        "the external identity is bound to a different account than the directory email says; refusing it"
                    );
                    return Err(IamError::SsoIdentityLinkConflict);
                }
            }
        }

        return Ok(BindingOutcome {
            user,
            created_user: false,
            created_link: false,
        });
    }

    // 2) 未命中 → 按 mail 匹配本地账号。
    if let Some(email) = email.as_deref() {
        if let Some(user) = store
            .find_user_by_email(ctx, email)
            .await
            .map_err(internal)?
        {
            return link_existing(store, ctx, provider, external_id, user).await;
        }
    }

    // 3) 仍未命中 → JIT 或拒绝。
    if !config.jit_provisioning {
        return Err(IamError::SsoAccountNotLinked);
    }
    let Some(email) = email else {
        // 没有可用的邮箱 → 无法派生 username，也无法建号。
        return Err(IamError::SsoAccountNotLinked);
    };
    provision(store, ctx, provider, external_id, &email, display_name).await
}

/// 把外部身份绑到**已存在**的本地账号（先判「不可改指」）。
async fn link_existing(
    store: &IamStore,
    ctx: &RequestContext,
    provider: &str,
    external_id: &str,
    user: UserRecord,
) -> Result<BindingOutcome, IamError> {
    let existing = store
        .find_binding_for_user(ctx, &user.id, provider)
        .await
        .map_err(internal)?;
    if existing
        .iter()
        .any(|binding| binding.external_id != external_id)
    {
        // 该本地账号在这个 provider 下已经绑给别的外部身份了。
        // 静默加一条新绑定 = 让两个外部身份共用一个本地账号（AC-S-105 步骤 6）。
        tracing::warn!(
            user_id = user.id,
            provider,
            "the local account is already bound to a different external identity; refusing to add a second one"
        );
        return Err(IamError::SsoIdentityLinkConflict);
    }

    let created = create_binding(
        store,
        ctx,
        provider,
        external_id,
        &user.id,
        Some(&user),
    )
    .await?;
    Ok(BindingOutcome {
        user,
        created_user: false,
        created_link: created,
    })
}

/// JIT 建号 + 绑定（并发幂等）。
async fn provision(
    store: &IamStore,
    ctx: &RequestContext,
    provider: &str,
    external_id: &str,
    email: &str,
    display_name: Option<&str>,
) -> Result<BindingOutcome, IamError> {
    let username = username_from_email(email).ok_or(IamError::SsoIdentityLinkConflict)?;
    let candidate = UserRecord {
        id: Uuid::new_v4().to_string(),
        tenant_id: ctx
            .tenant_id()
            .ok_or(IamError::TenantContextMissing)?
            .to_string(),
        username,
        email: Some(email.to_string()),
        // **永不匹配**的占位串（SSO 用户的密码永远在 IdP 侧）。
        password_hash: UNMATCHABLE_PASSWORD_HASH.to_string(),
        status: store::STATUS_ACTIVE.to_string(),
        mfa_enabled: false,
        must_change_password: false,
        failed_attempts: 0,
        locked_until: None,
        password_changed_at: OffsetDateTime::now_utc(),
        display_name: display_name.map(str::to_string),
        manager_id: None,
        departed_at: None,
        created_at: OffsetDateTime::now_utc(),
        updated_at: OffsetDateTime::now_utc(),
    };

    let inserted = store
        .insert_user_on_conflict_do_nothing(ctx, &candidate)
        .await
        .map_err(internal)?;

    // 重查：拿到「自己刚建的」或「并发对手刚建的」那一行。
    let user = store
        .find_user_by_email(ctx, email)
        .await
        .map_err(internal)?
        // 插入 0 行且按 email 查不到 → 冲突发生在 **username** 唯一键上
        // （本租户已有同名账号）→ 409，不接管既有账号（P8）。
        .ok_or(IamError::SsoIdentityLinkConflict)?;

    let created_link = create_binding(store, ctx, provider, external_id, &user.id, Some(&user)).await?;
    Ok(BindingOutcome {
        user,
        created_user: inserted > 0,
        created_link,
    })
}

/// 写一条绑定（`DO NOTHING` + 读回比对）。返回是否新建。
async fn create_binding(
    store: &IamStore,
    ctx: &RequestContext,
    provider: &str,
    external_id: &str,
    user_id: &str,
    known_user: Option<&UserRecord>,
) -> Result<bool, IamError> {
    let inserted = store
        .insert_external_identity_do_nothing(
            ctx,
            &Uuid::new_v4().to_string(),
            provider,
            external_id,
            user_id,
        )
        .await
        .map_err(internal)?;

    // ⚠️ **读回比对**：无论插入成功与否，都以库里的那一行为准。
    let binding = store
        .find_external_identity(ctx, provider, external_id)
        .await
        .map_err(internal)?
        .ok_or_else(|| IamError::Internal {
            reason: "the external identity could not be read back".to_string(),
        })?;
    if binding.user_id != user_id {
        // 绑定已存在且指向别的账号 —— **绝不改指**（P7）。
        tracing::warn!(
            provider,
            "the external identity is already bound to a different local account; refusing to repoint it"
        );
        let _ = known_user;
        return Err(IamError::SsoIdentityLinkConflict);
    }
    Ok(inserted > 0)
}

/// email → username（`@` 前部分）。
///
/// 只保留 `[A-Za-z0-9._-]`；空结果 → `None`（调用方转 409）。
/// 不做大小写归一：`username` 的唯一约束是**区分大小写**的列，
/// 归一化会让「Alice」与「alice」互相撞车而改变既有语义。
fn username_from_email(email: &str) -> Option<String> {
    let local = email.split('@').next().unwrap_or_default().trim();
    let sanitized: String = local
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-'))
        .take(64)
        .collect();
    (!sanitized.is_empty()).then_some(sanitized)
}

/// 数据库错误 → 内部错误。
fn internal(err: platform_db::DbError) -> IamError {
    tracing::error!(error = %err, "the sso binding data access failed");
    IamError::Internal {
        reason: "the data access layer failed".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::password::verify_password;

    /// 占位密码**必须永远校验失败**（否则 SSO 账号能用固定口令本地登录）。
    #[test]
    fn the_placeholder_hash_never_matches() {
        for attempt in ["", "password", UNMATCHABLE_PASSWORD_HASH, "correct-horse"] {
            assert!(
                !verify_password(attempt, UNMATCHABLE_PASSWORD_HASH),
                "占位哈希不得通过校验：{attempt}"
            );
        }
    }

    #[test]
    fn username_is_the_local_part_of_the_email() {
        assert_eq!(username_from_email("carol@example.com").as_deref(), Some("carol"));
        assert_eq!(
            username_from_email("first.last@example.com").as_deref(),
            Some("first.last")
        );
        assert_eq!(username_from_email("@example.com"), None);
        assert_eq!(username_from_email("   "), None);
        // 非 ASCII 字符被剔掉 —— 不允许把奇怪字符带进 username。
        assert_eq!(username_from_email("张三@example.com"), None);
    }

    #[test]
    fn provider_names_are_stable() {
        assert_eq!(super::super::oidc::PROVIDER, "oidc");
        assert_eq!(LDAP_PROVIDER, "ldap");
    }
}
