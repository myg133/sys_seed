//! 品牌**公开**接口的白名单（安全边界，D66 / design §6.1）。
//!
//! `GET /api/config/branding` **不需要令牌**（登录页在认证前就要显示 logo），
//! 所以它只能返回**展示类**键。白名单**枚举具体键**，不用 `theme.*` 前缀通配 ——
//! 前缀通配意味着将来任何人加一个 `theme.debugToken` / `theme.internalApiBase`，
//! 它会**自动**出现在无令牌的公开响应里，与「显式枚举、fail closed」的初衷相反。
//!
//! # 不变式（有测试守着）
//!
//! 白名单里的每个键都必须在某个 manifest 里登记为 `scope: space`。
//! 反之不必：登记为 `space` 但不在白名单的键 → 空间级写 **400**（AC-S-050 / D66）。

/// 品牌公开接口的白名单（**11 个具体键**，顺序固定）。
pub const BRANDING_WHITELIST: [&str; 11] = [
    "logo",
    "favicon",
    "title",
    "loginBackground",
    "footer",
    "copyright",
    "primaryColor",
    "theme.radius",
    "theme.mode",
    "theme.density",
    "theme.fontStack",
];

/// 该键是否允许出现在公开的品牌响应里。
pub fn is_branding_key(key: &str) -> bool {
    BRANDING_WHITELIST.contains(&key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_whitelist_is_exact_and_has_no_wildcards() {
        assert_eq!(BRANDING_WHITELIST.len(), 11);
        for key in BRANDING_WHITELIST {
            assert!(!key.contains('*'), "白名单不得有通配：{key}");
            assert!(is_branding_key(key));
        }
    }

    /// 前缀通配的实现会让任意 `theme.xxx` 通过 —— 这里显式断言**不**通过。
    #[test]
    fn a_future_theme_key_is_not_whitelisted() {
        for key in [
            "theme.debugToken",
            "theme.internalApiBase",
            "theme.tokens",
            "conflictStrategy",
            "scimBaseUrl",
            "ldapBaseDn",
            "cacheTtlSeconds",
            "loginBanner",
            "pageSize",
        ] {
            assert!(!is_branding_key(key), "不在白名单的键必须被拒：{key}");
        }
    }
}
