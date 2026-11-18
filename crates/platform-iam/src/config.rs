//! 认证与账号体系的部署期配置：环境变量 → 强类型配置，缺必填项 **fail-fast**。
//!
//! # 密钥只从环境注入（design-summary §2.2 / D3 / R3）
//!
//! | 环境变量 | 必填 | 说明 |
//! |---------|------|------|
//! | `BASE_JWT_PRIVATE_KEY` | ✅ | 当前签名密钥（PEM，PKCS#8） |
//! | `BASE_JWT_KID` | ✅ | 当前密钥的 `kid` |
//! | `BASE_JWT_PREVIOUS_PRIVATE_KEY` | — | 上一把密钥（PEM）—— **轮换宽限期**内与当前密钥同时有效 |
//! | `BASE_JWT_PREVIOUS_KID` | — | 上一把密钥的 `kid` |
//! | `BASE_IAM_SECRET_KEY` | ✅ | TOTP secret 静态加密的密钥（base64url，32 字节） |
//!
//! **密钥绝不入库、不入 Git**。启动时会校验「不是占位符密钥」——
//! 典型的 `changeme` / `REPLACE_ME` 一律拒绝启动（R3）。
//!
//! # 参数可配（design-summary §3.1）
//!
//! Argon2 参数、密码策略、限流阈值全部可配：低配开发机跑不动生产参数，
//! 生产值在部署配置里给足。

use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;

use crate::error::{IamError, PolicyViolation};

/// 缺必填环境变量时的错误码。
pub const CODE_MISSING: &str = "IAM_CONFIG_MISSING_REQUIRED";
/// 环境变量取值非法时的错误码。
pub const CODE_INVALID: &str = "IAM_CONFIG_INVALID_VALUE";
/// 密钥看起来是占位符时的错误码。
pub const CODE_PLACEHOLDER_SECRET: &str = "IAM_CONFIG_PLACEHOLDER_SECRET";

/// 占位符密钥的常见标记（大小写不敏感）。
///
/// 命中任何一个 → 拒绝启动：宁可起不来，也不能用一把众所周知的密钥签发令牌。
const PLACEHOLDER_MARKERS: [&str; 10] = [
    "changeme",
    "change_me",
    "replace_me",
    "replaceme",
    "your-",
    "your_",
    "placeholder",
    "example",
    "dummy",
    "testkey",
];

/// 一条配置问题。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigProblem {
    /// 稳定错误码。
    pub code: &'static str,
    /// 出问题的环境变量名。
    pub key: &'static str,
    /// 面向运维的说明（**不面向最终用户**；这是运维日志，不是 API 响应体）。
    pub detail: String,
}

impl std::fmt::Display for ConfigProblem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}：{}", self.code, self.key, self.detail)
    }
}

/// 配置校验失败。进程据此 fail-fast 退出。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigError {
    /// 全部问题（一次报全，避免运维来回试）。
    pub problems: Vec<ConfigProblem>,
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "IAM 配置校验失败（{} 项）", self.problems.len())?;
        for problem in &self.problems {
            write!(f, "\n  - {problem}")?;
        }
        Ok(())
    }
}

impl std::error::Error for ConfigError {}

/// Argon2id 参数（可配）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Argon2Settings {
    /// 内存开销（KiB）。
    pub memory_kib: u32,
    /// 迭代次数。
    pub iterations: u32,
    /// 并行度。
    pub parallelism: u32,
}

impl Default for Argon2Settings {
    /// OWASP 对 Argon2id 的推荐下限：19 MiB / t=2 / p=1。
    fn default() -> Self {
        Self {
            memory_kib: 19_456,
            iterations: 2,
            parallelism: 1,
        }
    }
}

/// 令牌有效期（可配）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TokenSettings {
    /// access token 的有效期（JWT，15 分钟）。
    pub access_ttl: Duration,
    /// refresh token 的有效期（30 天）。
    pub refresh_ttl: Duration,
    /// 密码重置令牌的有效期。
    pub reset_ttl: Duration,
}

impl Default for TokenSettings {
    fn default() -> Self {
        Self {
            access_ttl: Duration::from_secs(15 * 60),
            refresh_ttl: Duration::from_secs(30 * 24 * 60 * 60),
            reset_ttl: Duration::from_secs(60 * 60),
        }
    }
}

/// 密码策略（可配）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PasswordPolicySettings {
    /// 最小长度。
    pub min_length: usize,
    /// 有效期（天）；`None` = 不检查有效期。
    pub max_age_days: Option<u32>,
    /// 历史不重复：最近 N 条。
    pub history: usize,
}

impl Default for PasswordPolicySettings {
    fn default() -> Self {
        Self {
            min_length: 12,
            max_age_days: Some(180),
            history: 5,
        }
    }
}

/// 登录限流与账号锁定阈值（可配）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoginSettings {
    /// 同一 IP 在窗口内的最大登录尝试次数。
    pub ip_limit: u32,
    /// IP 限流的滑动窗口长度。
    pub ip_window: Duration,
    /// 同一账号连续失败多少次后锁定。
    pub account_failure_limit: u32,
    /// 锁定时长。
    pub lockout_duration: Duration,
}

impl Default for LoginSettings {
    fn default() -> Self {
        Self {
            ip_limit: 20,
            ip_window: Duration::from_secs(5 * 60),
            account_failure_limit: 5,
            lockout_duration: Duration::from_secs(15 * 60),
        }
    }
}

/// TOTP 参数（可配）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TotpSettings {
    /// 时间步长（秒）。
    pub interval: u64,
    /// 验证码位数。
    pub digits: u32,
    /// 容差步数（±1）。
    pub tolerance_steps: u64,
    /// 恢复码个数。
    pub recovery_code_count: usize,
}

impl Default for TotpSettings {
    fn default() -> Self {
        Self {
            interval: 30,
            digits: 6,
            tolerance_steps: 1,
            recovery_code_count: 10,
        }
    }
}

/// JWT 相关设置。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JwtSettings {
    /// 签发者（`iss`）。
    pub issuer: String,
    /// 受众（`aud`）。
    pub audience: String,
    /// 当前密钥的 PEM（PKCS#8）。
    pub private_key_pem: String,
    /// 当前密钥的 `kid`。
    pub kid: String,
    /// 上一把密钥的 PEM（轮换宽限期）。
    pub previous_private_key_pem: Option<String>,
    /// 上一把密钥的 `kid`。
    pub previous_kid: Option<String>,
}

/// 认证与账号体系的部署期配置。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IamConfig {
    /// JWT 设置。
    pub jwt: JwtSettings,
    /// 令牌有效期。
    pub tokens: TokenSettings,
    /// Argon2 参数。
    pub argon2: Argon2Settings,
    /// 密码策略。
    pub password_policy: PasswordPolicySettings,
    /// 登录限流与锁定阈值。
    pub login: LoginSettings,
    /// TOTP 参数。
    pub totp: TotpSettings,
    /// TOTP secret 静态加密的密钥（32 字节）。
    pub secret_key: [u8; 32],
}

impl IamConfig {
    /// IAM 相关环境变量的名字清单。
    ///
    /// 用来判断「IAM 到底配没配」：一个都没出现 = 平台**不提供**认证接口；
    /// 出现了一部分 = **配错了** → 启动时 fail-fast（绝不半配置上线）。
    pub const ENV_KEYS: [&'static str; 16] = [
        "BASE_JWT_PRIVATE_KEY",
        "BASE_JWT_KID",
        "BASE_JWT_PREVIOUS_PRIVATE_KEY",
        "BASE_JWT_PREVIOUS_KID",
        "BASE_JWT_ISSUER",
        "BASE_JWT_AUDIENCE",
        "BASE_IAM_SECRET_KEY",
        "BASE_IAM_ACCESS_TTL_SECONDS",
        "BASE_IAM_REFRESH_TTL_SECONDS",
        "BASE_IAM_RESET_TTL_SECONDS",
        "BASE_IAM_ARGON2_MEMORY_KIB",
        "BASE_IAM_ARGON2_ITERATIONS",
        "BASE_IAM_ARGON2_PARALLELISM",
        "BASE_IAM_PASSWORD_MIN_LENGTH",
        "BASE_IAM_PASSWORD_MAX_AGE_DAYS",
        "BASE_IAM_PASSWORD_HISTORY",
    ];

    /// IAM 是否被配置（环境变量里出现过任何一个 IAM 相关变量）。
    ///
    /// - `false` → 平台**不提供** `/api/iam/*`（服务照常起，认证能力缺席）；
    /// - `true`  → IAM 已启用，配置**必须**完整有效，否则启动 fail-fast。
    pub fn is_configured<F>(lookup: F) -> bool
    where
        F: Fn(&str) -> Option<String>,
    {
        Self::ENV_KEYS
            .iter()
            .any(|key| lookup(key).is_some_and(|value| !value.trim().is_empty()))
    }

    /// 从进程环境读取配置。
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_lookup(|key| std::env::var(key).ok())
    }

    /// 从任意查找函数读取配置 —— 测试用注入的键值表，不碰真实环境。
    pub fn from_lookup<F>(lookup: F) -> Result<Self, ConfigError>
    where
        F: Fn(&str) -> Option<String>,
    {
        let mut problems: Vec<ConfigProblem> = Vec::new();

        let jwt = jwt_settings(&lookup, &mut problems);
        let secret_key = secret_key(&lookup, &mut problems);
        let tokens = token_settings(&lookup, &mut problems);
        let argon2 = argon2_settings(&lookup, &mut problems);
        let password_policy = password_policy_settings(&lookup, &mut problems);
        let login = login_settings(&lookup, &mut problems);
        let totp = totp_settings(&lookup, &mut problems);

        if !problems.is_empty() {
            return Err(ConfigError { problems });
        }

        Ok(Self {
            jwt,
            tokens,
            argon2,
            password_policy,
            login,
            totp,
            secret_key,
        })
    }
}

/// 读取 JWT 设置。
fn jwt_settings<F>(lookup: &F, problems: &mut Vec<ConfigProblem>) -> JwtSettings
where
    F: Fn(&str) -> Option<String>,
{
    let private_key_pem = required(
        lookup,
        "BASE_JWT_PRIVATE_KEY",
        "JWT 签名密钥的 PEM（PKCS#8）；只从环境注入，不入库不入 Git",
        problems,
    )
    .unwrap_or_default();
    let kid = required(
        lookup,
        "BASE_JWT_KID",
        "JWT 签名密钥的 kid；轮换时换 kid，客户端按 kid 选密钥",
        problems,
    )
    .unwrap_or_default();

    let previous_private_key_pem = optional(lookup, "BASE_JWT_PREVIOUS_PRIVATE_KEY");
    let previous_kid = optional(lookup, "BASE_JWT_PREVIOUS_KID");

    // 上一把密钥与上一把 kid 必须成对出现：只有 PEM 没有 kid 就无法在宽限期内定位它。
    match (&previous_private_key_pem, &previous_kid) {
        (Some(_), None) => problems.push(invalid(
            "BASE_JWT_PREVIOUS_KID",
            "",
            "设置了 BASE_JWT_PREVIOUS_PRIVATE_KEY 就必须同时设置 BASE_JWT_PREVIOUS_KID",
        )),
        (None, Some(_)) => problems.push(invalid(
            "BASE_JWT_PREVIOUS_PRIVATE_KEY",
            "",
            "设置了 BASE_JWT_PREVIOUS_KID 就必须同时设置 BASE_JWT_PREVIOUS_PRIVATE_KEY",
        )),
        _ => {}
    }

    // 占位符密钥一律拒绝启动（R3）。
    if !private_key_pem.is_empty() && looks_like_placeholder(&private_key_pem) {
        problems.push(placeholder(
            "BASE_JWT_PRIVATE_KEY",
            "密钥看起来是占位符；生产密钥必须是真实生成的（P-256 / PKCS#8）",
        ));
    }
    if !kid.is_empty() && looks_like_placeholder(&kid) {
        problems.push(placeholder(
            "BASE_JWT_KID",
            "kid 看起来是占位符；请用真实且稳定的密钥标识",
        ));
    }

    JwtSettings {
        issuer: optional(lookup, "BASE_JWT_ISSUER").unwrap_or_else(|| "base".to_string()),
        audience: optional(lookup, "BASE_JWT_AUDIENCE").unwrap_or_else(|| "base-api".to_string()),
        private_key_pem,
        kid,
        previous_private_key_pem,
        previous_kid,
    }
}

/// 读取 TOTP secret 的静态加密密钥。
fn secret_key<F>(lookup: &F, problems: &mut Vec<ConfigProblem>) -> [u8; 32]
where
    F: Fn(&str) -> Option<String>,
{
    let raw = required(
        lookup,
        "BASE_IAM_SECRET_KEY",
        "TOTP secret 静态加密的密钥（base64url，32 字节）；只从环境注入",
        problems,
    );
    let Some(raw) = raw else {
        return [0u8; 32];
    };
    let decoded = URL_SAFE_NO_PAD.decode(raw.as_bytes());
    match decoded {
        Ok(bytes) if bytes.len() == 32 => {
            let mut key = [0u8; 32];
            key.copy_from_slice(&bytes);
            key
        }
        Ok(bytes) => {
            problems.push(invalid(
                "BASE_IAM_SECRET_KEY",
                "",
                &format!("解码后必须是 32 字节，当前是 {} 字节", bytes.len()),
            ));
            [0u8; 32]
        }
        Err(err) => {
            problems.push(invalid(
                "BASE_IAM_SECRET_KEY",
                "",
                &format!("必须是 base64url 编码：{err}"),
            ));
            [0u8; 32]
        }
    }
}

/// 读取令牌有效期。
fn token_settings<F>(lookup: &F, problems: &mut Vec<ConfigProblem>) -> TokenSettings
where
    F: Fn(&str) -> Option<String>,
{
    let defaults = TokenSettings::default();
    TokenSettings {
        access_ttl: seconds(
            lookup,
            "BASE_IAM_ACCESS_TTL_SECONDS",
            defaults.access_ttl,
            problems,
        ),
        refresh_ttl: seconds(
            lookup,
            "BASE_IAM_REFRESH_TTL_SECONDS",
            defaults.refresh_ttl,
            problems,
        ),
        reset_ttl: seconds(
            lookup,
            "BASE_IAM_RESET_TTL_SECONDS",
            defaults.reset_ttl,
            problems,
        ),
    }
}

/// 读取 Argon2 参数。
fn argon2_settings<F>(lookup: &F, problems: &mut Vec<ConfigProblem>) -> Argon2Settings
where
    F: Fn(&str) -> Option<String>,
{
    let defaults = Argon2Settings::default();
    let memory_kib = number(
        lookup,
        "BASE_IAM_ARGON2_MEMORY_KIB",
        defaults.memory_kib,
        problems,
    );
    let iterations = number(
        lookup,
        "BASE_IAM_ARGON2_ITERATIONS",
        defaults.iterations,
        problems,
    );
    let parallelism = number(
        lookup,
        "BASE_IAM_ARGON2_PARALLELISM",
        defaults.parallelism,
        problems,
    );
    if memory_kib < 8 {
        problems.push(invalid(
            "BASE_IAM_ARGON2_MEMORY_KIB",
            "",
            "内存开销至少 8 KiB；低于 OWASP 推荐值时仅限本地开发",
        ));
    }
    if iterations < 1 || parallelism < 1 {
        problems.push(invalid(
            "BASE_IAM_ARGON2_ITERATIONS",
            "",
            "迭代次数与并行度都必须 ≥ 1",
        ));
    }
    Argon2Settings {
        memory_kib,
        iterations,
        parallelism,
    }
}

/// 读取密码策略。
fn password_policy_settings<F>(
    lookup: &F,
    problems: &mut Vec<ConfigProblem>,
) -> PasswordPolicySettings
where
    F: Fn(&str) -> Option<String>,
{
    let defaults = PasswordPolicySettings::default();
    let min_length = number(
        lookup,
        "BASE_IAM_PASSWORD_MIN_LENGTH",
        defaults.min_length as u32,
        problems,
    ) as usize;
    if min_length < 8 {
        problems.push(invalid(
            "BASE_IAM_PASSWORD_MIN_LENGTH",
            "",
            "最小长度至少 8；低于 12 仅限本地开发",
        ));
    }
    let max_age_days = match optional(lookup, "BASE_IAM_PASSWORD_MAX_AGE_DAYS") {
        None => defaults.max_age_days,
        Some(raw) => match raw.trim().parse::<u32>() {
            Ok(0) => None,
            Ok(value) => Some(value),
            Err(_) => {
                problems.push(invalid(
                    "BASE_IAM_PASSWORD_MAX_AGE_DAYS",
                    &raw,
                    "非负整数；0 表示不检查有效期",
                ));
                defaults.max_age_days
            }
        },
    };
    let history = number(
        lookup,
        "BASE_IAM_PASSWORD_HISTORY",
        defaults.history as u32,
        problems,
    ) as usize;
    PasswordPolicySettings {
        min_length,
        max_age_days,
        history,
    }
}

/// 读取登录限流与锁定阈值。
fn login_settings<F>(lookup: &F, problems: &mut Vec<ConfigProblem>) -> LoginSettings
where
    F: Fn(&str) -> Option<String>,
{
    let defaults = LoginSettings::default();
    let ip_limit = number(
        lookup,
        "BASE_IAM_LOGIN_IP_LIMIT",
        defaults.ip_limit,
        problems,
    );
    let ip_window = seconds(
        lookup,
        "BASE_IAM_LOGIN_IP_WINDOW_SECONDS",
        defaults.ip_window,
        problems,
    );
    let account_failure_limit = number(
        lookup,
        "BASE_IAM_LOGIN_ACCOUNT_LIMIT",
        defaults.account_failure_limit,
        problems,
    );
    let lockout_duration = seconds(
        lookup,
        "BASE_IAM_ACCOUNT_LOCKOUT_MINUTES",
        defaults.lockout_duration,
        problems,
    );
    if ip_limit == 0 || account_failure_limit == 0 {
        problems.push(invalid(
            "BASE_IAM_LOGIN_IP_LIMIT",
            "",
            "限流阈值必须 ≥ 1（0 会让限流形同虚设）",
        ));
    }
    LoginSettings {
        ip_limit,
        ip_window,
        account_failure_limit,
        lockout_duration,
    }
}

/// 读取 TOTP 参数。
fn totp_settings<F>(lookup: &F, problems: &mut Vec<ConfigProblem>) -> TotpSettings
where
    F: Fn(&str) -> Option<String>,
{
    let defaults = TotpSettings::default();
    let interval = number(
        lookup,
        "BASE_IAM_TOTP_INTERVAL_SECONDS",
        defaults.interval as u32,
        problems,
    ) as u64;
    let digits = number(lookup, "BASE_IAM_TOTP_DIGITS", defaults.digits, problems);
    let tolerance_steps = number(
        lookup,
        "BASE_IAM_TOTP_TOLERANCE_STEPS",
        defaults.tolerance_steps as u32,
        problems,
    ) as u64;
    let recovery_code_count = number(
        lookup,
        "BASE_IAM_RECOVERY_CODE_COUNT",
        defaults.recovery_code_count as u32,
        problems,
    ) as usize;
    if !(6..=8).contains(&digits) {
        problems.push(invalid("BASE_IAM_TOTP_DIGITS", "", "6 到 8 位"));
    }
    if interval == 0 {
        problems.push(invalid("BASE_IAM_TOTP_INTERVAL_SECONDS", "", "必须 ≥ 1"));
    }
    if recovery_code_count == 0 {
        problems.push(invalid("BASE_IAM_RECOVERY_CODE_COUNT", "", "必须 ≥ 1"));
    }
    TotpSettings {
        interval,
        digits,
        tolerance_steps,
        recovery_code_count,
    }
}

/// 值是否像占位符。
pub fn looks_like_placeholder(value: &str) -> bool {
    let lowered = value.to_ascii_lowercase();
    PLACEHOLDER_MARKERS
        .iter()
        .any(|marker| lowered.contains(marker))
}

/// 取必填项；缺失或为空串记一条问题。
fn required<F>(
    lookup: &F,
    key: &'static str,
    hint: &str,
    problems: &mut Vec<ConfigProblem>,
) -> Option<String>
where
    F: Fn(&str) -> Option<String>,
{
    match lookup(key) {
        Some(value) if !value.trim().is_empty() => Some(value.trim().to_string()),
        _ => {
            problems.push(ConfigProblem {
                code: CODE_MISSING,
                key,
                detail: format!("必填环境变量缺失或为空；{hint}"),
            });
            None
        }
    }
}

/// 取可选项；缺失或为空串都算「没设」。
fn optional<F>(lookup: &F, key: &str) -> Option<String>
where
    F: Fn(&str) -> Option<String>,
{
    lookup(key)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

/// 读一个无符号整数。
fn number<F>(lookup: &F, key: &'static str, default: u32, problems: &mut Vec<ConfigProblem>) -> u32
where
    F: Fn(&str) -> Option<String>,
{
    match optional(lookup, key) {
        None => default,
        Some(raw) => match raw.trim().parse::<u32>() {
            Ok(value) => value,
            Err(_) => {
                problems.push(invalid(key, &raw, "非负整数"));
                default
            }
        },
    }
}

/// 读一个秒数并转成时长。
fn seconds<F>(
    lookup: &F,
    key: &'static str,
    default: Duration,
    problems: &mut Vec<ConfigProblem>,
) -> Duration
where
    F: Fn(&str) -> Option<String>,
{
    match optional(lookup, key) {
        None => default,
        Some(raw) => match raw.trim().parse::<u64>() {
            Ok(0) => {
                problems.push(invalid(key, &raw, "必须 ≥ 1 秒"));
                default
            }
            Ok(value) => Duration::from_secs(value),
            Err(_) => {
                problems.push(invalid(key, &raw, "非负整数（秒）"));
                default
            }
        },
    }
}

/// 记一条「取值非法」问题。
fn invalid(key: &'static str, raw: &str, expected: &str) -> ConfigProblem {
    ConfigProblem {
        code: CODE_INVALID,
        key,
        detail: format!("取值 `{raw}` 非法，期望 {expected}"),
    }
}

/// 记一条「看起来是占位符密钥」问题。
fn placeholder(key: &'static str, detail: &str) -> ConfigProblem {
    ConfigProblem {
        code: CODE_PLACEHOLDER_SECRET,
        key,
        detail: detail.to_string(),
    }
}

/// 密码策略的默认值（供测试与文档引用）。
pub fn default_password_policy() -> PasswordPolicySettings {
    PasswordPolicySettings::default()
}

/// 校验密码是否满足策略，返回**具体违反的规则**。
///
/// 契约 C6 + AC-S-009：拒绝时必须指出**违反哪条规则**（`params.violations`）。
pub fn check_password_policy(
    password: &str,
    policy: &PasswordPolicySettings,
) -> Result<(), IamError> {
    let mut violations = Vec::new();

    let length = password.chars().count();
    if length < policy.min_length {
        violations.push(PolicyViolation::new(
            "min_length",
            format!(
                "the password must be at least {} characters long",
                policy.min_length
            ),
        ));
    }

    if crate::password::is_common_password(password) {
        violations.push(PolicyViolation::new(
            "common_password",
            "the password appears in the local list of commonly used passwords",
        ));
    }

    if violations.is_empty() {
        Ok(())
    } else {
        Err(IamError::PasswordPolicyViolation { violations })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// 构造一个查找函数。
    fn lookup_from(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
            .collect();
        move |key: &str| map.get(key).cloned()
    }

    /// 一把可用的测试密钥（占位符检测必须放过它）。
    const TEST_PEM: &str = "-----BEGIN PRIVATE KEY-----\nMIGHAgEAMBMG\n-----END PRIVATE KEY-----\n";

    /// 一份「齐全」的环境变量。
    fn complete_env() -> Vec<(String, String)> {
        vec![
            ("BASE_JWT_PRIVATE_KEY".to_string(), TEST_PEM.to_string()),
            ("BASE_JWT_KID".to_string(), "kid-current".to_string()),
            (
                "BASE_IAM_SECRET_KEY".to_string(),
                URL_SAFE_NO_PAD.encode([7u8; 32]),
            ),
        ]
    }

    /// 把「齐全」的环境变量转成查找函数。
    fn lookup_complete() -> impl Fn(&str) -> Option<String> {
        let map: HashMap<String, String> = complete_env().into_iter().collect();
        move |key: &str| map.get(key).cloned()
    }

    #[test]
    fn missing_required_keys_are_reported_at_once() {
        let err = IamConfig::from_lookup(lookup_from(&[])).expect_err("缺必填项必须失败");
        let keys: Vec<&str> = err.problems.iter().map(|p| p.key).collect();
        assert!(keys.contains(&"BASE_JWT_PRIVATE_KEY"), "keys = {keys:?}");
        assert!(keys.contains(&"BASE_JWT_KID"), "keys = {keys:?}");
        assert!(keys.contains(&"BASE_IAM_SECRET_KEY"), "keys = {keys:?}");
        assert!(err.problems.iter().all(|p| p.code == CODE_MISSING));
    }

    #[test]
    fn a_complete_environment_produces_the_default_policy() {
        let config = IamConfig::from_lookup(lookup_complete()).expect("配置应当通过");
        assert_eq!(config.password_policy.min_length, 12);
        assert_eq!(config.password_policy.max_age_days, Some(180));
        assert_eq!(config.password_policy.history, 5);
        assert_eq!(config.tokens.access_ttl, Duration::from_secs(900));
        assert_eq!(
            config.tokens.refresh_ttl,
            Duration::from_secs(30 * 24 * 3600)
        );
        assert_eq!(config.argon2.memory_kib, 19_456);
        assert_eq!(config.argon2.iterations, 2);
        assert_eq!(config.jwt.kid, "kid-current");
        assert_eq!(config.jwt.issuer, "base");
        assert_eq!(config.jwt.audience, "base-api");
    }

    #[test]
    fn placeholder_keys_are_rejected() {
        let mut pairs = complete_env();
        pairs[0].1 = "-----BEGIN PRIVATE KEY-----\nchangeme\n-----END PRIVATE KEY-----".to_string();
        let map: HashMap<String, String> = pairs.into_iter().collect();
        let err = IamConfig::from_lookup(move |key: &str| map.get(key).cloned())
            .expect_err("占位符密钥必须被拒绝");
        assert!(
            err.problems
                .iter()
                .any(|p| p.code == CODE_PLACEHOLDER_SECRET),
            "problems = {err}"
        );
    }

    #[test]
    fn previous_key_requires_previous_kid() {
        let mut pairs = complete_env();
        pairs.push((
            "BASE_JWT_PREVIOUS_PRIVATE_KEY".to_string(),
            TEST_PEM.to_string(),
        ));
        let map: HashMap<String, String> = pairs.into_iter().collect();
        let err = IamConfig::from_lookup(move |key: &str| map.get(key).cloned())
            .expect_err("只给上一把 PEM 不行");
        assert!(
            err.problems
                .iter()
                .any(|p| p.key == "BASE_JWT_PREVIOUS_KID"),
            "problems = {err}"
        );
    }

    #[test]
    fn secret_key_must_decode_to_32_bytes() {
        let mut pairs = complete_env();
        pairs[2].1 = URL_SAFE_NO_PAD.encode([1u8; 8]);
        let map: HashMap<String, String> = pairs.into_iter().collect();
        let err = IamConfig::from_lookup(move |key: &str| map.get(key).cloned())
            .expect_err("长度不对的密钥必须被拒绝");
        assert!(
            err.problems.iter().any(|p| p.key == "BASE_IAM_SECRET_KEY"),
            "problems = {err}"
        );
    }

    #[test]
    fn parameters_are_configurable() {
        let mut pairs = complete_env();
        pairs.push(("BASE_IAM_PASSWORD_MIN_LENGTH".to_string(), "16".to_string()));
        pairs.push((
            "BASE_IAM_ARGON2_MEMORY_KIB".to_string(),
            "32768".to_string(),
        ));
        pairs.push(("BASE_IAM_LOGIN_IP_LIMIT".to_string(), "7".to_string()));
        let map: HashMap<String, String> = pairs.into_iter().collect();
        let config =
            IamConfig::from_lookup(move |key: &str| map.get(key).cloned()).expect("配置应当通过");
        assert_eq!(config.password_policy.min_length, 16);
        assert_eq!(config.argon2.memory_kib, 32_768);
        assert_eq!(config.login.ip_limit, 7);
    }

    #[test]
    fn password_policy_violations_name_the_rule() {
        let policy = default_password_policy();
        let err = check_password_policy("short", &policy).expect_err("太短必须被拒");
        match err {
            IamError::PasswordPolicyViolation { violations } => {
                assert_eq!(violations[0].rule, "min_length");
            }
            other => panic!("unexpected error: {other}"),
        }
        assert!(check_password_policy("a-very-long-passphrase", &policy).is_ok());
    }

    #[test]
    fn common_passwords_are_rejected_by_the_policy() {
        let policy = default_password_policy();
        let err = check_password_policy("password123456", &policy).expect_err("常见口令必须被拒");
        match err {
            IamError::PasswordPolicyViolation { violations } => {
                assert!(
                    violations.iter().any(|v| v.rule == "common_password"),
                    "violations = {violations:?}"
                );
            }
            other => panic!("unexpected error: {other}"),
        }
    }
}
