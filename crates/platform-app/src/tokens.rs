//! 登录令牌的**校验接缝**（REQ-016）。
//!
//! # 为什么用 trait 而不是直接依赖 `platform-iam`
//!
//! 依赖方向是单向的：`platform-app → {platform-core, platform-db, platform-oauth}`。
//! 直接依赖 `platform-iam` 会把「装配层注入实现」变成「模块自己选实现」——
//! 与 `platform-config` / `platform-schema` 的 `TokenVerifier` 同款口径
//! （它们的实现在 `platform-server` 里，用 `IamService` 兜底）。
//!
//! ⚠️ 失败**一律**按「未认证」处理（**不区分**原因，避免探测）。

use platform_core::RequestContext;

/// 已认证的主体（**只含**本模块要用的两项）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedToken {
    /// 用户 ID。
    pub user_id: String,
    /// 租户 ID（**来自令牌**）。
    pub tenant_id: String,
}

/// 令牌校验失败。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuthError;

/// 登录令牌校验（由装配层实现）。
pub trait TokenVerifier: Send + Sync + 'static {
    /// 验签并取出主体；失败一律 [`AuthError`]。
    fn verify(&self, token: &str) -> Result<VerifiedToken, AuthError>;
}

/// **一律拒绝**的校验器（未装配 IAM 时的 fail closed 实现）。
#[derive(Debug, Clone, Copy, Default)]
pub struct RejectAllTokens;

impl TokenVerifier for RejectAllTokens {
    fn verify(&self, _token: &str) -> Result<VerifiedToken, AuthError> {
        Err(AuthError)
    }
}

/// 内存里的固定校验器（单测 / 不需要真签名的场景）。
///
/// ⚠️ **不是**生产实现 —— 它把令牌当用户 ID（`token` → 该用户）：
/// 仅供「只想验路由形状」的测试用；真实用例走真 IAM 实现。
#[derive(Debug, Clone, Default)]
pub struct StaticTokenVerifier;

impl TokenVerifier for StaticTokenVerifier {
    fn verify(&self, token: &str) -> Result<VerifiedToken, AuthError> {
        let trimmed = token.trim();
        if trimmed.is_empty() {
            return Err(AuthError);
        }
        Ok(VerifiedToken {
            user_id: trimmed.to_string(),
            tenant_id: "tenant-a".to_string(),
        })
    }
}

/// 为某个上下文绑定校验结果（测试辅助：把令牌映射成固定的主体）。
pub fn verified(ctx: &RequestContext) -> Option<VerifiedToken> {
    Some(VerifiedToken {
        user_id: ctx.user_id()?.to_string(),
        tenant_id: ctx.tenant_id()?.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_reject_all_verifier_rejects_everything() {
        assert!(RejectAllTokens.verify("anything").is_err());
    }

    #[test]
    fn the_static_verifier_maps_a_token_to_a_subject() {
        let verified = StaticTokenVerifier.verify(" u-1 ").expect("应当通过");
        assert_eq!(verified.user_id, "u-1");
        assert!(StaticTokenVerifier.verify("   ").is_err());
    }
}
