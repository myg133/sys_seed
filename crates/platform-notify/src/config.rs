//! 通知系统的**运行期配置**（REQ-015，design §6 / D304 / D306）。
//!
//! # 两类配置，走**两条不同的路**（这一点必须分清）
//!
//! | 类 | 5 个退避/阈值参数 | SMTP 连接信息 |
//! |---|---|---|
//! | 存哪 | `platform_config` 表（manifest `config` 段登记） | **环境变量** |
//! | 为什么 | 非敏感，运维可在界面上调 | **密码不能落库** |
//! | 依据 | D304 | **D306** |
//!
//! ## 为什么 SMTP 凭据走环境变量，不进配置表
//!
//! `ConfigSpec` 的 `ConfigType`（`crates/platform-core/src/extension/manifest.rs:189-207`）
//! 只有 `String` / `Number` / `Boolean` / `Array` / `Object` —— **没有 `Secret`**；
//! `platform-config` 里也**没有任何加密/脱敏机制**。
//! → 密码若进配置表就是**明文落库** + 通过配置读接口回传给任何持 `config:read` 的人。
//!
//! 与既有 SSO / SCIM 同款：`crates/platform-org/src/config.rs:37` 的 `SCIM_BASE_URL_ENV`、
//! `crates/platform-iam/src/sso/config.rs` 的 `LDAP_SERVICE_BIND_PASSWORD_ENV`。
//!
//! # ⚠️ 5 个配置键必须是 `scope: "platform"` 且 **lowerCamelCase**（契约 C9）
//!
//! `ConfigSpec.scope` 的 serde 默认值是 **`Tenant`** → 漏写 `"scope": "platform"`
//! 就落到租户作用域 → 平台级读写端点直接拒 → 配置改了**永不生效**（REQ-016 的 S-01）。
//!
//! `ConfigKey::parse` 要求**每一段**「小写字母开头 + 只含 ASCII 字母数字」
//! （`crates/platform-core/src/config.rs:213` 的 `is_lower_camel_case`）
//! → **段内带 `_` 会让服务起不来**（REQ-016 的 S-02）。
//!
//! # ⚠️ 配置**每请求读**（不是装配时读一次）
//!
//! 装配时读一次 = 配置改了**永远不生效**（除非重启）。
//! 本 crate 用 trait 接缝 [`NotifyConfigSource`]，实现由装配层注入
//! （与 `platform-oauth` 的 `OAuthConfigSource` 同款）。

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use platform_core::RequestContext;

/// 装箱的 future（让 trait 保持**可对象化** —— `async fn` 在 trait 上不可 `dyn`）。
pub type NotifyBoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// 最多尝试几次（含首次），默认 `5`。
pub const CONFIG_KEY_MAX_ATTEMPTS: &str = "notify.maxAttempts";
/// 第 1 次重试的延迟（秒），默认 `30`。
pub const CONFIG_KEY_RETRY_BASE_SECONDS: &str = "notify.retryBaseSeconds";
/// 每次翻倍，默认 `2`。
pub const CONFIG_KEY_RETRY_MULTIPLIER: &str = "notify.retryMultiplier";
/// 单次延迟上限（秒），默认 `3600`（防指数爆炸）。
pub const CONFIG_KEY_RETRY_MAX_SECONDS: &str = "notify.retryMaxSeconds";
/// `delivering` 残留状态的回收阈值（秒），默认 `300`（D313）。
pub const CONFIG_KEY_DELIVERING_TIMEOUT_SECONDS: &str = "notify.deliveringTimeoutSeconds";

/// 5 个配置键（AC-S-174 步骤 1a 断言 manifest 里恰好这 5 个）。
pub const CONFIG_KEYS: &[&str] = &[
    CONFIG_KEY_MAX_ATTEMPTS,
    CONFIG_KEY_RETRY_BASE_SECONDS,
    CONFIG_KEY_RETRY_MULTIPLIER,
    CONFIG_KEY_RETRY_MAX_SECONDS,
    CONFIG_KEY_DELIVERING_TIMEOUT_SECONDS,
];

/// 默认值（D297 / D313）。
pub mod defaults {
    /// 最多尝试次数。
    pub const MAX_ATTEMPTS: i32 = 5;
    /// 首次重试延迟。
    pub const RETRY_BASE_SECONDS: i64 = 30;
    /// 退避倍数。
    pub const RETRY_MULTIPLIER: i64 = 2;
    /// 单次延迟上限。
    pub const RETRY_MAX_SECONDS: i64 = 3600;
    /// `delivering` 回收阈值。
    pub const DELIVERING_TIMEOUT_SECONDS: i64 = 300;
}

/// 退避与重试参数（D297）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryPolicy {
    /// 最多尝试几次（**含首次**）。
    pub max_attempts: i32,
    /// 第 1 次重试的延迟（秒）。
    pub base_seconds: i64,
    /// 每次翻倍。
    pub multiplier: i64,
    /// 单次延迟上限（秒）。
    pub max_seconds: i64,
    /// `delivering` 残留状态的回收阈值（秒）。
    pub delivering_timeout_seconds: i64,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: defaults::MAX_ATTEMPTS,
            base_seconds: defaults::RETRY_BASE_SECONDS,
            multiplier: defaults::RETRY_MULTIPLIER,
            max_seconds: defaults::RETRY_MAX_SECONDS,
            delivering_timeout_seconds: defaults::DELIVERING_TIMEOUT_SECONDS,
        }
    }
}

impl RetryPolicy {
    /// 第 `attempt` 次失败后的退避延迟（秒）。
    ///
    /// `attempt` 从 **1** 开始（第 1 次失败 → 第 1 个退避值）。
    ///
    /// 默认参数下：`attempt = 1,2,3,4` → `[30, 60, 120, 240]`
    /// （⚠️ **不是 5 个值**：第 5 次失败直接进死信，不再算延迟）。
    ///
    /// # ⚠️ 用整数运算，不用浮点（D297）
    ///
    /// 低配机 + 确定性断言：`f64` 的 `powi` 在不同平台可能差 1 个 ULP，
    /// 而这条断言是**逐值精确比较**的。
    ///
    /// # ⚠️ 计算过程**必须防溢出**
    ///
    /// `max_seconds` 只是**结果**的上限 —— 它**不防**计算过程中的溢出。
    /// `i64` 上乘到 `2^63` 会 panic（debug）或回绕（release）→
    /// 所以用 `checked_mul`，一旦溢出直接取 `max_seconds`。
    pub fn backoff_seconds(&self, attempt: i32) -> i64 {
        if attempt <= 0 {
            return 0;
        }
        // base * multiplier^(attempt-1)，逐步乘并检查溢出。
        let mut value = self.base_seconds.max(0);
        let mut remaining = attempt - 1;
        while remaining > 0 {
            // 提前判断：已经到上限就不必再乘了。
            if value >= self.max_seconds {
                return self.max_seconds;
            }
            match value.checked_mul(self.multiplier.max(1)) {
                Some(next) => value = next,
                // 溢出 → 直接取上限（不 panic、不回绕）。
                None => return self.max_seconds,
            }
            remaining -= 1;
        }
        value.min(self.max_seconds)
    }

    /// 这一轮失败后是否**还有重试机会**（还有 → `pending`，没有 → `dead_letter`）。
    ///
    /// ⚠️ `attempt_count` 是**含首次**的计数：`max_attempts = 5` 时
    /// 第 5 次失败就**没有**下一次了（AC-S-171 步骤 1）。
    pub fn has_attempts_left(&self, attempt_count: i32) -> bool {
        attempt_count < self.max_attempts
    }
}

/// 配置读取的接缝（由装配层注入实现）。
///
/// ⚠️ 本 crate **不依赖** `platform-config` 的实现细节 —— 用 trait 接缝。
pub trait NotifyConfigSource: Send + Sync {
    /// 读一个平台级配置项的值（`None` = 未配置 → 用默认值）。
    fn read_platform<'a>(
        &'a self,
        ctx: &'a RequestContext,
        key: &'a str,
    ) -> NotifyBoxFuture<'a, Option<i64>>;

    /// 租户默认 locale（三级回退的第 2 级，D294）。
    fn tenant_default_locale(&self, ctx: &RequestContext) -> NotifyBoxFuture<'_, String>;
}

/// 装配后的配置（每个请求读一次，D274 同款语义）。
#[derive(Clone)]
pub struct NotifyConfig {
    source: Arc<dyn NotifyConfigSource>,
}

impl NotifyConfig {
    /// 用配置源构造。
    pub fn new(source: Arc<dyn NotifyConfigSource>) -> Self {
        Self { source }
    }

    /// 读退避与重试参数（5 个键，缺省用 [`RetryPolicy::default`]）。
    pub async fn retry_policy(&self, ctx: &RequestContext) -> RetryPolicy {
        let d = RetryPolicy::default();
        RetryPolicy {
            max_attempts: self
                .read_i64(ctx, CONFIG_KEY_MAX_ATTEMPTS)
                .await
                .map(|v| v as i32)
                .unwrap_or(d.max_attempts),
            base_seconds: self
                .read_i64(ctx, CONFIG_KEY_RETRY_BASE_SECONDS)
                .await
                .unwrap_or(d.base_seconds),
            multiplier: self
                .read_i64(ctx, CONFIG_KEY_RETRY_MULTIPLIER)
                .await
                .unwrap_or(d.multiplier),
            max_seconds: self
                .read_i64(ctx, CONFIG_KEY_RETRY_MAX_SECONDS)
                .await
                .unwrap_or(d.max_seconds),
            delivering_timeout_seconds: self
                .read_i64(ctx, CONFIG_KEY_DELIVERING_TIMEOUT_SECONDS)
                .await
                .unwrap_or(d.delivering_timeout_seconds),
        }
    }

    /// 租户默认 locale（三级回退的第 2 级）。
    pub async fn tenant_default_locale(&self, ctx: &RequestContext) -> String {
        self.source.tenant_default_locale(ctx).await
    }

    async fn read_i64(&self, ctx: &RequestContext, key: &str) -> Option<i64> {
        self.source.read_platform(ctx, key).await
    }
}

// ---------------------------------------------------------------------------
// SMTP 配置（环境变量，D306）
// ---------------------------------------------------------------------------

/// SMTP 主机名。
pub const SMTP_HOST_ENV: &str = "BASE_SMTP_HOST";
/// SMTP 端口（通常 587）。
pub const SMTP_PORT_ENV: &str = "BASE_SMTP_PORT";
/// SMTP 用户名。
pub const SMTP_USERNAME_ENV: &str = "BASE_SMTP_USERNAME";
/// SMTP 密码（**敏感值，永不进日志**）。
pub const SMTP_PASSWORD_ENV: &str = "BASE_SMTP_PASSWORD";
/// 发件人地址。
pub const SMTP_FROM_ENV: &str = "BASE_SMTP_FROM";

/// 全部 5 个环境变量（`is_configured` 要求**全有**）。
pub const SMTP_ENV_KEYS: &[&str] = &[
    SMTP_HOST_ENV,
    SMTP_PORT_ENV,
    SMTP_USERNAME_ENV,
    SMTP_PASSWORD_ENV,
    SMTP_FROM_ENV,
];

/// 邮件通道的配置（D306）。
///
/// ⚠️ 手写 `Debug`，密码固定输出 `"***"`（AC-S-166 步骤 8 的判据）。
#[derive(Clone, PartialEq, Eq)]
pub struct SmtpConfig {
    /// 主机名（**不接受 IP** —— rustls 拒绝 IP 型 `ServerName`，D305 第 3 条）。
    pub host: String,
    /// 端口（通常 `587`，走 STARTTLS）。
    pub port: u16,
    /// 用户名。
    pub username: String,
    /// 密码（**永不进日志**）。
    pub password: String,
    /// 发件人地址。
    pub from: String,
}

impl std::fmt::Debug for SmtpConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // ⚠️ 密码固定打码：`Debug` 会被 `tracing` / `assert_eq!` 的失败信息带出去。
        f.debug_struct("SmtpConfig")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("username", &self.username)
            .field("password", &"***")
            .field("from", &self.from)
            .finish()
    }
}

impl SmtpConfig {
    /// 从「查环境变量」的函数构造。
    ///
    /// 返回 `None` 表示**未配置完整** → 该通道注册为「未配置」
    /// （投递返回 `NOTIFY_CHANNEL_NOT_CONFIGURED`，**不是静默成功**，D292）。
    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Option<Self> {
        let host = non_empty(lookup(SMTP_HOST_ENV))?;
        // ⚠️ 主机名不得是 IP（rustls 的 ServerName 拒绝 IP 做 SNI）。
        if host.parse::<std::net::IpAddr>().is_ok() {
            return None;
        }
        let port: u16 = non_empty(lookup(SMTP_PORT_ENV))?.parse().ok()?;
        let username = non_empty(lookup(SMTP_USERNAME_ENV))?;
        let password = non_empty(lookup(SMTP_PASSWORD_ENV))?;
        let from = non_empty(lookup(SMTP_FROM_ENV))?;
        Some(Self {
            host,
            port,
            username,
            password,
            from,
        })
    }

    /// 从**真实环境变量**构造。
    pub fn from_env() -> Option<Self> {
        Self::from_lookup(|key| std::env::var(key).ok())
    }

    /// 是否配置完整（5 个环境变量**全有**，D306）。
    pub fn is_configured(lookup: impl Fn(&str) -> Option<String>) -> bool {
        Self::from_lookup(lookup).is_some()
    }
}

fn non_empty(value: Option<String>) -> Option<String> {
    value.filter(|v| !v.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    /// 5 个配置键**恰好 5 个**且无重复（D304）。
    #[test]
    fn there_are_exactly_five_config_keys() {
        assert_eq!(CONFIG_KEYS.len(), 5, "D304 rev 2 把基线钉死为 5 个");
        let unique: std::collections::BTreeSet<&str> = CONFIG_KEYS.iter().copied().collect();
        assert_eq!(unique.len(), 5);
    }

    /// 每个键的**每一段**都是 lowerCamelCase（契约 C9）——
    /// 段内带 `_` 会让 `ConfigKey::parse` 失败 → **服务起不来**。
    #[test]
    fn every_config_key_segment_is_lower_camel_case() {
        for key in CONFIG_KEYS {
            for segment in key.split('.') {
                assert!(!segment.is_empty(), "键 {key} 有空段");
                let first = segment.chars().next().expect("非空");
                assert!(
                    first.is_ascii_lowercase(),
                    "键 {key} 的段 {segment} 必须以小写字母开头"
                );
                assert!(
                    segment.chars().all(|c| c.is_ascii_alphanumeric()),
                    "键 {key} 的段 {segment} 只能含 ASCII 字母数字（带 _ 会让服务起不来）"
                );
            }
        }
    }

    /// 全部键都在 `notify.` 命名空间下。
    #[test]
    fn every_config_key_is_in_the_notify_namespace() {
        for key in CONFIG_KEYS {
            assert!(key.starts_with("notify."), "键必须在 notify. 下：{key}");
        }
    }

    /// 退避序列**逐值**：默认参数下 attempt 1~4 → `[30, 60, 120, 240]`（AC-S-171 步骤 2）。
    #[test]
    fn the_backoff_sequence_is_thirty_sixty_one_twenty_two_forty() {
        let policy = RetryPolicy::default();
        assert_eq!(policy.backoff_seconds(1), 30);
        assert_eq!(policy.backoff_seconds(2), 60);
        assert_eq!(policy.backoff_seconds(3), 120);
        assert_eq!(policy.backoff_seconds(4), 240);
    }

    /// 上限截断**真的生效**：`2000 * 2 = 4000 > 3600` → 取 `3600`（AC-S-171 步骤 3）。
    #[test]
    fn the_backoff_is_clamped_to_the_maximum() {
        let policy = RetryPolicy {
            base_seconds: 2000,
            multiplier: 2,
            max_seconds: 3600,
            ..RetryPolicy::default()
        };
        // attempt=1 → 2000（未到上限）
        assert_eq!(policy.backoff_seconds(1), 2000);
        // attempt=2 → 4000 → 被截断到 3600
        assert_eq!(policy.backoff_seconds(2), 3600);
        // 更大的 attempt 也还是 3600
        assert_eq!(policy.backoff_seconds(9), 3600);
    }

    /// **溢出防护**：`attempt` 很大时不得 panic / 回绕，必须返回上限。
    #[test]
    fn the_backoff_does_not_overflow_on_large_attempts() {
        let policy = RetryPolicy {
            base_seconds: 30,
            multiplier: 2,
            max_seconds: i64::MAX,
            ..RetryPolicy::default()
        };
        // 2^62 级别 —— 若不防溢出，i64 会回绕成负数或 panic。
        let value = policy.backoff_seconds(63);
        // ⚠️ 判据必须是「**具体值**」而不是 `value > 0`
        // （后者对「回绕成一个小的正数」也会绿）。
        assert_eq!(
            value,
            i64::MAX,
            "溢出时必须取上限（checked_mul 返回 None → max_seconds），得到 {value}"
        );
    }

    /// **上限受 `max_seconds` 约束**（不是 `i64::MAX` 时才区分得出）。
    #[test]
    fn the_overflow_falls_back_to_the_configured_max() {
        let policy = RetryPolicy {
            base_seconds: 30,
            multiplier: 2,
            max_seconds: 7200,
            ..RetryPolicy::default()
        };
        // 30 * 2^62 必然溢出 → 取 max_seconds。
        assert_eq!(policy.backoff_seconds(63), 7200);
        // 阳性对照：未溢出时算的是真值（30 * 2^1 = 60）。
        assert_eq!(policy.backoff_seconds(2), 60);
    }

    /// `max_attempts` 决定何时进死信：5 次时 `attempt_count = 4` 还有机会、
    /// `5` 就没有了（AC-S-171 步骤 1）。
    #[test]
    fn the_attempt_budget_is_inclusive_of_the_first_try() {
        let policy = RetryPolicy::default();
        assert!(policy.has_attempts_left(0));
        assert!(policy.has_attempts_left(1));
        assert!(policy.has_attempts_left(4));
        assert!(
            !policy.has_attempts_left(5),
            "第 5 次失败后不得再有下一次（maxAttempts 含首次）"
        );
    }

    /// 配置 `maxAttempts = 2` 时**第 2 次**就进死信（证明读的是配置不是写死的 5）。
    #[test]
    fn a_lower_max_attempts_shrinks_the_budget() {
        let policy = RetryPolicy {
            max_attempts: 2,
            ..RetryPolicy::default()
        };
        assert!(policy.has_attempts_left(1));
        assert!(
            !policy.has_attempts_left(2),
            "maxAttempts=2 时第 2 次失败即进死信"
        );
    }

    // -- SMTP 配置 ----------------------------------------------------------

    fn smtp_env() -> BTreeMap<&'static str, String> {
        BTreeMap::from([
            (SMTP_HOST_ENV, "smtp.example.com".to_string()),
            (SMTP_PORT_ENV, "587".to_string()),
            (SMTP_USERNAME_ENV, "notify@example.com".to_string()),
            (SMTP_PASSWORD_ENV, "s3cr3t-value".to_string()),
            (SMTP_FROM_ENV, "noreply@example.com".to_string()),
        ])
    }

    /// 5 个环境变量全有 → 能构造。
    #[test]
    fn the_smtp_config_builds_when_all_five_are_present() {
        let env = smtp_env();
        let cfg = SmtpConfig::from_lookup(|k| env.get(k).cloned()).expect("应能构造");
        assert_eq!(cfg.host, "smtp.example.com");
        assert_eq!(cfg.port, 587);
        assert_eq!(cfg.from, "noreply@example.com");
    }

    /// **缺任一**都视为未配置（D306：5 个全有才算配置好）。
    #[test]
    fn the_smtp_config_is_none_when_any_variable_is_missing() {
        for missing in SMTP_ENV_KEYS {
            let mut env = smtp_env();
            env.remove(missing);
            assert!(
                SmtpConfig::from_lookup(|k| env.get(k).cloned()).is_none(),
                "缺 {missing} 时必须是 None（未配置）"
            );
        }
    }

    /// 空字符串也算未配置（防止 `BASE_SMTP_PASSWORD=""` 被当作有效密码）。
    #[test]
    fn the_smtp_config_is_none_when_a_variable_is_blank() {
        let mut env = smtp_env();
        env.insert(SMTP_PASSWORD_ENV, "   ".to_string());
        assert!(SmtpConfig::from_lookup(|k| env.get(k).cloned()).is_none());
    }

    /// **主机名不得是 IP**（rustls 的 `ServerName` 拒绝 IP 做 SNI，D305 第 3 条）。
    #[test]
    fn the_smtp_config_rejects_ip_hosts() {
        for ip in ["127.0.0.1", "192.168.1.10", "::1"] {
            let mut env = smtp_env();
            env.insert(SMTP_HOST_ENV, ip.to_string());
            assert!(
                SmtpConfig::from_lookup(|k| env.get(k).cloned()).is_none(),
                "主机名是 IP（{ip}）时必须拒绝 —— rustls 的 ServerName 不接受 IP"
            );
        }
    }

    /// **`Debug` 必须打码密码**（AC-S-166 步骤 8）。
    #[test]
    fn the_debug_output_masks_the_password() {
        let env = smtp_env();
        let cfg = SmtpConfig::from_lookup(|k| env.get(k).cloned()).expect("应能构造");
        let debug = format!("{cfg:?}");
        assert!(
            !debug.contains("s3cr3t-value"),
            "Debug 不得含密码明文：{debug}"
        );
        assert!(debug.contains("***"), "Debug 必须含打码标记：{debug}");
        // 阳性对照：证明 Debug 不是全空（否则上一条恒真）。
        assert!(
            debug.contains("smtp.example.com"),
            "Debug 必须含主机名（阳性对照）：{debug}"
        );
    }

    /// `is_configured` 与 `from_lookup` 判断一致。
    #[test]
    fn is_configured_agrees_with_from_lookup() {
        let env = smtp_env();
        assert!(SmtpConfig::is_configured(|k| env.get(k).cloned()));
        let empty: BTreeMap<&str, String> = BTreeMap::new();
        assert!(!SmtpConfig::is_configured(|k| empty.get(k).cloned()));
    }

    /// 端口不是数字 → 未配置（不 panic）。
    #[test]
    fn a_non_numeric_port_yields_none() {
        let mut env = smtp_env();
        env.insert(SMTP_PORT_ENV, "not-a-port".to_string());
        assert!(SmtpConfig::from_lookup(|k| env.get(k).cloned()).is_none());
    }
}
