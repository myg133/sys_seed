//! 语言 / 时区 / 币种的**协商与观测**（REQ-021 §3 / D110 / D116）。
//!
//! # 两个接口（本需求的全部 HTTP 面）
//!
//! | 方法 | 路径 | 鉴权 |
//! |------|------|------|
//! | `GET` | `/api/i18n/context` | **需认证**（无新增权限点） |
//! | `GET` | `/api/i18n/fx/convert` | **需认证**（无新增权限点） |
//!
//! 两个都**不新增权限点** —— 不碰权限点三件套（manifest `permissions[]` +
//! 种子迁移 + 启动对账，D27）。碰错的表现是「进程起不来」，代价远大于收益。
//!
//! # 优先级（写死）
//!
//! ```text
//! 用户显式偏好  >  Accept-Language  >  租户配置  >  平台配置  >  manifest 默认
//! ```
//!
//! **用户在我们系统里显式设的偏好最权威** —— 他设了 English 就该看英文，
//! 不管浏览器语言是什么。`Accept-Language` 只是「用户没设时的最佳猜测」。
//! （起草时曾写反，见 design §3.1。）
//!
//! # 「平台配置」这一级（D119）
//!
//! 协商链**从四级改为五级**：平台层的值是**组织级默认值** —— 依据 D53，
//! `scope` 是「允许覆盖到的最深层」，`scope: tenant` 意味着**平台层可以写**。
//! 平台管理员改了全组织默认语言却毫无效果 = **静默丢弃运维配置**。
//!
//! - 平台层只在**租户没配**时生效（取值优先级仍是「租户 > 平台」）；
//! - `source` 如实反映**哪一层生效**：租户自己配的报 `tenant`，
//!   **只有平台配的**报 `platform`，都没配（manifest 默认）报 `default`；
//! - `source ∈ {user, accept-language, tenant, platform, default}`（**5 值**）。
//!
//! # 「用户偏好」这一级怎么承载（D116）
//!
//! 用户表**没有** locale / timezone 列（D112），所以后端不持久化任何用户偏好。
//! 这一级只通过**显式查询参数**承载：
//!
//! ```text
//! GET /api/i18n/context?locale=en-US&timezone=Europe/London
//! ```
//!
//! - **不持久化、不跨请求记忆**（下次不带参数就回到下一级）；
//! - **仅当值合法时生效**：`locale` 必须是**已注册**语言，`timezone` 必须是
//!   合法的 IANA 名 —— 否则视为「没设」，落到 `Accept-Language`。
//!
//! 不做这一级的话，`source === "user"` 永远不会出现，AC-S-090 步骤 8
//! **物理不可达**（那是本仓反复踩过的错）。
//!
//! # 时区**不看** `Accept-Language`（§3.4）
//!
//! `Accept-Language` 是**语言**头，不含时区。所以时区的 `source`
//! **只会是** `user` / `tenant` / `platform` / `default`，**永不出现** `accept-language`。
//!
//! # 已注册语言与平台默认值
//!
//! - 已注册语言 = `packages/i18n/src/locales/*.json` 的**文件名**（裁定 #16），
//!   由 `platform_config::registry::registered_locales` 扫描并随注册表一起传入；
//! - 平台默认值 = manifest 的 `default`（裁定 #17），由
//!   [`Setting::manifest_default`] 带上来 —— **不是**代码常量。
//!
//! # 汇率换算
//!
//! - 换算走 `platform_core::money` 的纯函数（`i128` 中间量 + 银行家舍入）；
//! - 汇率快照**显式按 `asOf` 取**（不隐式取最新，R4）；
//! - 结果带 `asOf` + `source` + `rateId`（可追溯，AC-S-097 步骤 4）；
//! - 不存在的快照 → **404 `FX_RATE_NOT_FOUND`**（裁定 #21）。
//!
//! # 本轮**没有**汇率写入接口（D115）
//!
//! 没有 `POST /api/i18n/fx/rates` —— 管理端需求未开工。写入只走
//! `platform_db::fx::FxRateStore::insert_rate`（测试自己播种）。

use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use axum::extract::{FromRequestParts, Query, Request, State};
use axum::http::{HeaderMap, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use platform_config::error::ConfigError;
use platform_config::http::{TokenVerifier, VerifiedToken};
use platform_core::RequestContext;
use platform_core::extension::config::ConfigSource;
use platform_core::money::{CurrencyCode, FxRate, Money, MoneyError};
use platform_core::problem::{CODE_INTERNAL_ERROR, ProblemDetails, problem_response};
use platform_core::titles::title_of_code;
use platform_db::fx::FxRateStore;
use serde::{Deserialize, Serialize};

/// 协商结果观测路径（D110）。
pub const CONTEXT_PATH: &str = "/api/i18n/context";
/// 汇率换算路径（只读，D115）。
pub const CONVERT_PATH: &str = "/api/i18n/fx/convert";

/// 租户上下文请求头（契约 C3）。
const TENANT_HEADER: &str = "x-tenant-id";
/// 空间上下文请求头（契约 C3）。
const SPACE_HEADER: &str = "x-space-id";

// ---------------------------------------------------------------------------
// 值 / 来源
// ---------------------------------------------------------------------------

/// 某一层配置解析出来的值。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Setting {
    /// 解析后的值（租户 / 空间 / 平台显式配置，或 manifest 默认值）。
    pub value: String,
    /// 命中层级（四值，来自配置解析器）。
    pub source: ConfigSource,
    /// manifest 的 `default`（**平台默认值的唯一来源**，裁定 #17）。
    pub manifest_default: String,
}

impl Setting {
    /// 造一个「只有默认值」的设置（测试与降级路径用）。
    pub fn default_only(value: &str) -> Self {
        Self {
            value: value.to_string(),
            source: ConfigSource::Default,
            manifest_default: value.to_string(),
        }
    }

    /// 这一层配置对应的**协商来源**（D119）。
    ///
    /// 判定**只按 `self.source` 分派**（不另造一套判定逻辑）：
    ///
    /// - 租户 / 空间层 → [`LocaleSource::Tenant`]（`LocaleSource` 没有 `space` 取值 ——
    ///   空间是租户之下的更深层，对外一律报 `tenant`）；
    /// - 平台层 → [`LocaleSource::Platform`]；
    /// - 默认层 → `None`（调用方回落到 manifest 默认值）。
    ///
    /// 三个协商函数（语言 / 时区 / 币种）**共用**这一个判定 —— 少改一处就会漏一层。
    pub fn layer_source(&self) -> Option<LocaleSource> {
        match self.source {
            ConfigSource::Tenant | ConfigSource::Space => Some(LocaleSource::Tenant),
            ConfigSource::Platform => Some(LocaleSource::Platform),
            ConfigSource::Default => None,
        }
    }

    /// 是否来自**显式**配置（租户 / 空间 / 平台层；**不是** manifest 默认值）。
    ///
    /// D119：平台层也算**显式配置** —— 平台管理员写的值就是组织级默认值，
    /// 不能被当成「没配」。
    pub fn is_explicit(&self) -> bool {
        self.layer_source().is_some()
    }
}

/// 三个 i18n 配置项的解析结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct I18nSettings {
    /// 语言。
    pub locale: Setting,
    /// 时区。
    pub timezone: Setting,
    /// 币种。
    pub currency: Setting,
}

/// 协商结果的来源（`source` 字段的**5 值**，D110 / D119）。
///
/// `user` / `accept-language` / `tenant` / `platform` / `default`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum LocaleSource {
    /// 用户显式偏好（`?locale=`，D116，不持久化）。
    User,
    /// `Accept-Language` 里第一个已注册的语言。
    AcceptLanguage,
    /// 租户（或空间）配置。
    Tenant,
    /// 平台配置（组织级默认值，D119）。
    Platform,
    /// 平台默认（manifest 的 `default`）。
    Default,
}

impl LocaleSource {
    /// 响应里的字面值（`accept-language` 用连字符，与契约一致）。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::AcceptLanguage => "accept-language",
            Self::Tenant => "tenant",
            Self::Platform => "platform",
            Self::Default => "default",
        }
    }
}

// ---------------------------------------------------------------------------
// 接缝（可对象化 —— 测试不必连库）
// ---------------------------------------------------------------------------

/// 装箱的 future（与 `platform-org::notify` 同一写法：接缝要保持可对象化）。
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// 读租户级的 i18n 配置（生产实现是 `platform_config::ConfigService`）。
///
/// 抽成 trait 的理由与 `TokenVerifier` 相同：**测试不连库**也能驱动 HTTP 层。
/// 但协商算法本身（[`negotiate_locale`] / [`negotiate_timezone`]）是**同一个函数**
/// —— 测试与生产不走两段逻辑（D85）。
pub trait I18nSettingsSource: Send + Sync {
    /// 解析语言的三个配置项。
    fn settings<'a>(
        &'a self,
        ctx: &'a RequestContext,
    ) -> BoxFuture<'a, Result<I18nSettings, ConfigError>>;
}

impl I18nSettingsSource for platform_config::ConfigService {
    fn settings<'a>(
        &'a self,
        ctx: &'a RequestContext,
    ) -> BoxFuture<'a, Result<I18nSettings, ConfigError>> {
        Box::pin(async move {
            let keys: Vec<String> = ["locale", "timezone", "currency"]
                .iter()
                .map(|key| (*key).to_string())
                .collect();
            let resolved = self.read_entries(ctx, &keys).await?;
            let mut by_key: BTreeMap<String, (String, ConfigSource)> = BTreeMap::new();
            for entry in resolved {
                let value = entry.value.as_str().unwrap_or_default().to_string();
                by_key.insert(entry.key, (value, entry.source));
            }
            let mut pick = |key: &str| -> Result<Setting, ConfigError> {
                // manifest 的 default 是平台默认值的唯一来源（裁定 #17）。
                let manifest_default = self
                    .registry()
                    .get(key)
                    .and_then(|spec| spec.default.as_str())
                    .unwrap_or_default()
                    .to_string();
                let (value, source) =
                    by_key
                        .remove(key)
                        .ok_or_else(|| ConfigError::KeyNotRegistered {
                            key: key.to_string(),
                        })?;
                // 显式配置为空串等价于「没配」——否则空串会成为一个渲染不出来的值。
                if value.trim().is_empty() {
                    return Ok(Setting {
                        value: manifest_default.clone(),
                        source: ConfigSource::Default,
                        manifest_default,
                    });
                }
                Ok(Setting {
                    value,
                    source,
                    manifest_default,
                })
            };
            Ok(I18nSettings {
                locale: pick("locale")?,
                timezone: pick("timezone")?,
                currency: pick("currency")?,
            })
        })
    }
}

/// 汇率快照的只读接缝。
///
/// 生产实现是 `platform_db::fx::FxRateStore`；测试实现是内存表。
pub trait FxRates: Send + Sync {
    /// 按 `(from, to, as_of)` **精确**取一条快照；没有 → `Ok(None)`。
    fn resolve<'a>(
        &'a self,
        from: CurrencyCode,
        to: CurrencyCode,
        as_of: &'a str,
    ) -> BoxFuture<'a, Result<Option<FxRate>, String>>;
}

impl FxRates for FxRateStore {
    fn resolve<'a>(
        &'a self,
        from: CurrencyCode,
        to: CurrencyCode,
        as_of: &'a str,
    ) -> BoxFuture<'a, Result<Option<FxRate>, String>> {
        Box::pin(async move {
            FxRateStore::resolve_rate(self, from, to, as_of)
                .await
                .map_err(|error| error.to_string())
        })
    }
}

/// HTTP 层共享状态。
#[derive(Clone)]
pub struct I18nState {
    /// 租户级配置的读取（生产：`ConfigService`）。
    pub settings: Arc<dyn I18nSettingsSource>,
    /// 汇率快照（生产：`FxRateStore`）。
    pub rates: Arc<dyn FxRates>,
    /// 令牌校验（复用 `platform-config` 的接缝 —— 装配层注入同一份实现）。
    pub tokens: Arc<dyn TokenVerifier>,
    /// 已注册语言（翻译包目录的文件名，裁定 #16）。
    pub locales: Arc<BTreeSet<String>>,
}

impl std::fmt::Debug for I18nState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("I18nState").finish_non_exhaustive()
    }
}

// ---------------------------------------------------------------------------
// 错误
// ---------------------------------------------------------------------------

/// i18n 接口的错误。
#[derive(Debug, Clone, PartialEq)]
pub enum I18nError {
    /// 配置读取失败（沿用配置模块的码与状态，不在 i18n 里另立一套）。
    Config(ConfigError),
    /// 参数不合法（`from` / `to` / `minor` / `asOf`）。
    InvalidArgument {
        /// 参数名。
        param: String,
        /// 面向开发者的英文原因。
        reason: String,
    },
    /// 金额/汇率错误（溢出、非法汇率）。
    Money(MoneyError),
    /// 快照不存在（**404**，裁定 #21）。
    RateNotFound {
        /// 源币种。
        from: String,
        /// 目标币种。
        to: String,
        /// 快照时刻。
        as_of: String,
    },
    /// 数据访问层失败（500）。
    Internal {
        /// 面向开发者的英文原因。
        reason: String,
    },
}

impl I18nError {
    /// 稳定的机器可读错误码。
    pub fn code(&self) -> &'static str {
        match self {
            Self::Config(error) => error.code(),
            Self::InvalidArgument { .. } => platform_core::money::CODE_INVALID_ARGUMENT,
            Self::Money(error) => error.code(),
            Self::RateNotFound { .. } => platform_core::money::CODE_FX_RATE_NOT_FOUND,
            Self::Internal { .. } => CODE_INTERNAL_ERROR,
        }
    }

    /// HTTP 状态码。
    pub fn status(&self) -> u16 {
        match self {
            Self::Config(error) => error.status(),
            Self::InvalidArgument { .. } | Self::Money(_) => 400,
            Self::RateNotFound { .. } => 404,
            Self::Internal { .. } => 500,
        }
    }

    /// 面向开发者的英文 ASCII 描述（契约 C6：不得拼中文句子）。
    pub fn detail(&self) -> String {
        match self {
            Self::Config(error) => error.detail(),
            Self::InvalidArgument { param, reason } => {
                format!("the query parameter `{param}` is invalid: {reason}")
            }
            Self::Money(error) => error.detail(),
            Self::RateNotFound { from, to, as_of } => {
                format!("no FX rate snapshot exists for {from}->{to} at {as_of}")
            }
            Self::Internal { reason } => {
                format!("the FX rate lookup failed: {}", ascii_safe(reason))
            }
        }
    }

    /// 补充参数（前端渲染用）。
    pub fn params(&self) -> Option<BTreeMap<String, serde_json::Value>> {
        let params = match self {
            Self::Config(error) => error.params(),
            Self::InvalidArgument { param, reason } => {
                let mut map = BTreeMap::new();
                map.insert("param".to_string(), serde_json::Value::from(param.clone()));
                map.insert(
                    "reason".to_string(),
                    serde_json::Value::from(ascii_safe(reason)),
                );
                map
            }
            Self::Money(_) => BTreeMap::new(),
            Self::RateNotFound { from, to, as_of } => {
                let mut map = BTreeMap::new();
                map.insert("from".to_string(), serde_json::Value::from(from.clone()));
                map.insert("to".to_string(), serde_json::Value::from(to.clone()));
                map.insert("asOf".to_string(), serde_json::Value::from(as_of.clone()));
                map
            }
            Self::Internal { .. } => BTreeMap::new(),
        };
        if params.is_empty() {
            None
        } else {
            Some(params)
        }
    }
}

impl From<ConfigError> for I18nError {
    fn from(error: ConfigError) -> Self {
        Self::Config(error)
    }
}

impl From<MoneyError> for I18nError {
    fn from(error: MoneyError) -> Self {
        Self::Money(error)
    }
}

/// 把错误变成 RFC 9457 响应（`title` 从 `TITLES` 取 —— 唯一来源，D113）。
fn error_response(error: &I18nError, ctx: &RequestContext, instance: &str) -> Response {
    let details = ProblemDetails::from_context(
        error.code(),
        title_of_code(error.code()),
        error.status(),
        error.detail(),
        ctx,
        instance,
        error.params(),
    );
    problem_response(&details)
}

/// 把非 ASCII / 控制字符替换成 `?`（`detail` 必须是 ASCII，契约 C6）。
fn ascii_safe(value: &str) -> String {
    value
        .chars()
        .take(200)
        .map(|ch| {
            if ch.is_ascii() && !ch.is_control() {
                ch
            } else {
                '?'
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// 协商（纯函数）
// ---------------------------------------------------------------------------

/// 一条 `Accept-Language` 语言范围。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LanguageRange {
    /// 语言标签（`zh-CN`）；`*` 时为 `"*"`。
    pub tag: String,
    /// 权重 × 1000（`q=0.9` → 900；缺省 1000）。
    pub quality: u16,
}

impl LanguageRange {
    /// 是否通配（`*`）。
    pub fn is_wildcard(&self) -> bool {
        self.tag == "*"
    }
}

/// 解析 `Accept-Language`（按 `q` **降序**排列；同 `q` 保持原始顺序）。
///
/// # 规则（裁定 #23 / #24）
///
/// - `q` 缺省 = 1.0（1000）；最多三位小数；
/// - `q=0` 表示**明确拒绝**该语言 —— 调用方跳过它（不选、也不回退到它）；
/// - `*` 留给调用方按「平台默认」处理（**不做任意匹配**）；
/// - 语法坏掉的范围整条丢弃（不因为一个坏项让整个头失效）。
pub fn parse_accept_language(raw: &str) -> Vec<LanguageRange> {
    let mut ranges: Vec<LanguageRange> = raw
        .split(',')
        .filter_map(|item| {
            let mut parts = item.split(';');
            let tag = parts.next()?.trim();
            if tag.is_empty() {
                return None;
            }
            let tag = if tag == "*" {
                "*".to_string()
            } else if is_language_tag(tag) {
                tag.to_string()
            } else {
                return None;
            };
            let mut quality = 1_000_u16;
            for parameter in parts {
                let parameter = parameter.trim();
                let Some(raw_q) = parameter.strip_prefix("q=") else {
                    // 其它参数（如 `level=1`）忽略。
                    continue;
                };
                match parse_quality(raw_q) {
                    Some(parsed) => quality = parsed,
                    // `q` 写坏了 → 整条范围丢弃（宁可不用，也不要猜）。
                    None => return None,
                }
            }
            Some(LanguageRange { tag, quality })
        })
        .collect();
    // 稳定排序：同权重保持客户端给的顺序（`zh-CN, en-US` 里 zh-CN 优先）。
    ranges.sort_by_key(|range| std::cmp::Reverse(range.quality));
    ranges
}

/// `q` 值（0~1，最多三位小数）→ 千分比；非法返回 `None`。
///
/// `q=0.9` → 900；`q=0` → 0（明确拒绝）；`q=1` → 1000。
/// 越界（`1.5`）、多余小数位、非数字一律判非法 —— **不猜**。
fn parse_quality(raw: &str) -> Option<u16> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    let (integer, fraction) = match raw.split_once('.') {
        Some((integer, fraction)) => (integer, fraction),
        None => (raw, ""),
    };
    if !integer.bytes().all(|b| b.is_ascii_digit()) || !fraction.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    if integer.len() > 1 || fraction.len() > 3 {
        return None;
    }
    let whole: u16 = if integer.is_empty() {
        0
    } else {
        integer.parse().ok()?
    };
    let mut padded = fraction.to_string();
    while padded.len() < 3 {
        padded.push('0');
    }
    let thousandths: u16 = if padded.is_empty() {
        0
    } else {
        padded.parse().ok()?
    };
    let total = whole.checked_mul(1_000)?.checked_add(thousandths)?;
    if total > 1_000 {
        return None;
    }
    Some(total)
}

/// 语言标签形状（与 `platform_config::registry::is_locale_tag` 同一口径）。
fn is_language_tag(value: &str) -> bool {
    platform_config::registry::is_locale_tag(value)
}

/// 一次语言协商的输入。
#[derive(Debug, Clone, Copy)]
pub struct LocaleLayers<'a> {
    /// 用户显式偏好（`?locale=`，已注册才生效；D116）。
    pub user: Option<&'a str>,
    /// `Accept-Language` 原始头。
    pub accept_language: Option<&'a str>,
    /// 租户层 / 空间层 / 平台层 / 默认层**已解析好**的配置结果（D119）。
    ///
    /// 命中的层级由 [`Setting::source`] 携带 —— 协商时**如实反映**是哪一层生效。
    pub tenant: &'a Setting,
    /// 已注册语言。
    pub registered: &'a BTreeSet<String>,
}

/// 语言协商：**用户显式偏好 > Accept-Language > 租户配置 > 平台配置 > 平台默认**。
///
/// 每一级都要求「值已注册」——**不允许**返回未注册的 locale（前端加载不到翻译包，
/// 全是回退）。
///
/// `*` 按裁定 #23 直接落到**平台默认**（不做任意匹配）；`q=0` 的语言被跳过。
///
/// D119：租户层与平台层由 [`Setting::layer_source`] 统一判定 ——
/// 租户自己配的报 `tenant`，**只有平台配的**报 `platform`。
pub fn negotiate_locale(layers: LocaleLayers<'_>) -> (String, LocaleSource) {
    // 1) 用户显式偏好：只有已注册才生效，否则视为「没设」。
    if let Some(user) = layers.user
        && layers.registered.contains(user)
    {
        return (user.to_string(), LocaleSource::User);
    }

    // 2) Accept-Language：按 q 降序取第一个已注册的。
    if let Some(raw) = layers.accept_language {
        for range in parse_accept_language(raw) {
            if range.quality == 0 {
                // 明确拒绝：跳过（不选它，也不把它当兜底）。
                continue;
            }
            if range.is_wildcard() {
                // 裁定 #23：`*` → 平台默认（不做任意匹配）。
                return (
                    layers.tenant.manifest_default.clone(),
                    LocaleSource::Default,
                );
            }
            if layers.registered.contains(&range.tag) {
                return (range.tag, LocaleSource::AcceptLanguage);
            }
        }
    }

    // 3) 租户 / 空间层配置，4) 平台层配置（D119）。
    //    值已注册才算数 —— 配置可能写在翻译包之前。
    if let Some(layer) = layers.tenant.layer_source()
        && layers.registered.contains(&layers.tenant.value)
    {
        return (layers.tenant.value.clone(), layer);
    }

    // 5) 平台默认（manifest 的 `default`，裁定 #17）。
    (
        layers.tenant.manifest_default.clone(),
        LocaleSource::Default,
    )
}

/// 时区协商：**用户显式偏好 > 租户配置 > 平台配置 > 平台默认**。
///
/// **不看** `Accept-Language`（§3.4）→ `source` **永不出现** `accept-language`。
///
/// D119：平台层与语言层同一套判定（[`Setting::layer_source`]）。
pub fn negotiate_timezone(user: Option<&str>, tenant: &Setting) -> (String, LocaleSource) {
    if let Some(user) = user
        && platform_config::registry::is_iana_timezone(user)
    {
        return (user.to_string(), LocaleSource::User);
    }
    // 租户 / 空间层 → `tenant`；平台层 → `platform`（D119）。
    if let Some(layer) = tenant.layer_source() {
        return (tenant.value.clone(), layer);
    }
    (tenant.manifest_default.clone(), LocaleSource::Default)
}

/// 币种：本轮**没有**用户级覆盖（D112 只给了语言/时区的前端存储），
/// 所以只有「租户配置 > 平台配置 > 平台默认」三级。
///
/// D119：平台层与语言/时区同一套判定（[`Setting::layer_source`]）。
pub fn resolve_currency(tenant: &Setting) -> (String, LocaleSource) {
    // 租户 / 空间层 → `tenant`；平台层 → `platform`（D119）。
    if let Some(layer) = tenant.layer_source() {
        return (tenant.value.clone(), layer);
    }
    (tenant.manifest_default.clone(), LocaleSource::Default)
}

// ---------------------------------------------------------------------------
// 时间文本（RFC 3339）
// ---------------------------------------------------------------------------

/// `asOf` 的形状校验：`YYYY-MM-DDTHH:MM:SS[.fff](Z|±HH:MM)`。
///
/// 只判**形状**：真正的时间解析交给 PostgreSQL（`?::timestamptz`）。
/// 这样既有明确的 400（不把 500 漏给调用方），又不需要 `time` 的 `parsing` feature。
pub fn is_rfc3339(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() < 20 {
        return false;
    }
    let digit = |index: usize| bytes[index].is_ascii_digit();
    if !(0..4).all(digit) || bytes[4] != b'-' || !(5..7).all(digit) || bytes[7] != b'-' {
        return false;
    }
    if !(8..10).all(digit) {
        return false;
    }
    if bytes[10] != b'T' && bytes[10] != b't' {
        return false;
    }
    if !(11..13).all(digit) || bytes[13] != b':' || !(14..16).all(digit) || bytes[16] != b':' {
        return false;
    }
    if !(17..19).all(digit) {
        return false;
    }
    // 数值范围：月 01~12、日 01~31、时 00~23、分/秒 00~59。
    // （不校验「2 月 31 日」这类日历规则 —— 那是 PostgreSQL 的 `timestamptz` 解析的事。）
    let two_digits = |start: usize| -> u32 {
        u32::from(bytes[start] - b'0') * 10 + u32::from(bytes[start + 1] - b'0')
    };
    if !(1..=12).contains(&two_digits(5))
        || !(1..=31).contains(&two_digits(8))
        || two_digits(11) > 23
        || two_digits(14) > 59
        || two_digits(17) > 59
    {
        return false;
    }
    let mut index = 19;
    if bytes.get(index) == Some(&b'.') {
        index += 1;
        let start = index;
        while bytes.get(index).is_some_and(u8::is_ascii_digit) {
            index += 1;
        }
        if index == start {
            return false;
        }
    }
    match bytes.get(index) {
        Some(b'Z') | Some(b'z') => index + 1 == bytes.len(),
        Some(b'+') | Some(b'-') => {
            let rest = &bytes[index + 1..];
            rest.len() == 5
                && rest[0].is_ascii_digit()
                && rest[1].is_ascii_digit()
                && rest[2] == b':'
                && rest[3].is_ascii_digit()
                && rest[4].is_ascii_digit()
        }
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// HTTP
// ---------------------------------------------------------------------------

/// 构造 i18n 路由（两个接口都需要认证，**不新增权限点**）。
pub fn router(state: Arc<I18nState>) -> Router {
    Router::new()
        .route(CONTEXT_PATH, get(context))
        .route(CONVERT_PATH, get(convert))
        // 认证中间件在**外层**：先验令牌，再进处理器。
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            authenticate,
        ))
        .with_state(state)
}

/// 已认证的请求上下文提取器。
#[derive(Debug, Clone)]
struct ReqCtx(RequestContext);

impl<S: Send + Sync> FromRequestParts<S> for ReqCtx {
    type Rejection = Response;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        _state: &S,
    ) -> Result<Self, Self::Rejection> {
        let ctx = parts
            .extensions
            .get::<RequestContext>()
            .cloned()
            .unwrap_or_else(RequestContext::new);
        Ok(ReqCtx(ctx))
    }
}

/// 认证中间件：验令牌 → 解析租户/空间头 → 注入上下文。
///
/// 与 `/api/config/*` 的中间件**同一套语义**（契约 C3）：
/// 请求头租户与令牌租户不一致 → **404**（不泄露资源存在性）。
async fn authenticate(
    State(state): State<Arc<I18nState>>,
    mut request: Request,
    next: Next,
) -> Response {
    let instance = request.uri().path().to_string();
    let mut ctx = request
        .extensions()
        .get::<RequestContext>()
        .cloned()
        .unwrap_or_else(RequestContext::new);

    if let Some(tenant) = header_value(request.headers(), TENANT_HEADER) {
        ctx = ctx.with_tenant(tenant);
    }
    if let Some(space) = header_value(request.headers(), SPACE_HEADER) {
        ctx = ctx.with_space(space);
    }

    let Some(token) = bearer_token(request.headers()) else {
        return problem_response(&ProblemDetails::unauthenticated(&ctx, &instance));
    };
    let verified: VerifiedToken = match state.tokens.verify(&token) {
        Ok(verified) => verified,
        Err(_) => return problem_response(&ProblemDetails::unauthenticated(&ctx, &instance)),
    };

    match ctx.tenant_id() {
        Some(tenant) if tenant != verified.tenant_id => {
            return problem_response(&ProblemDetails::not_found(&ctx, &instance, "resource"));
        }
        Some(_) => {}
        None => ctx = ctx.with_tenant(verified.tenant_id.clone()),
    }
    ctx = ctx.with_user(verified.user_id.clone());
    request.extensions_mut().insert(ctx);
    next.run(request).await
}

/// `/api/i18n/context` 的查询参数（「用户显式偏好」这一级，D116）。
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextQuery {
    /// 用户偏好语言；仅当**已注册**时生效。
    #[serde(default)]
    pub locale: Option<String>,
    /// 用户偏好时区；仅当格式合法时生效。
    #[serde(default)]
    pub timezone: Option<String>,
}

/// 单个字段的观测结果（用于逐字段判定「哪一级生效」）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldSources {
    /// 语言来源（`user` / `accept-language` / `tenant` / `platform` / `default`）。
    pub locale: &'static str,
    /// 时区来源（**永不为** `accept-language`）。
    pub timezone: &'static str,
    /// 币种来源。
    pub currency: &'static str,
}

/// `/api/i18n/context` 的响应（D110）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextResponse {
    /// 协商出的语言（**一定是已注册语言之一**）。
    pub locale: String,
    /// 协商出的时区。
    pub timezone: String,
    /// 协商出的币种。
    pub currency: String,
    /// **语言的**来源（AC-S-090 步骤 8 的判据）。
    ///
    /// 取值集合是 **5 个**：`user` / `accept-language` / `tenant` / `platform` / `default`。
    pub source: &'static str,
    /// 逐字段来源（时区 / 币种各有自己的判定，不能共用一个 `source`）。
    pub sources: FieldSources,
}

/// 协商结果的观测接口。
async fn context(
    State(state): State<Arc<I18nState>>,
    ReqCtx(ctx): ReqCtx,
    Query(query): Query<ContextQuery>,
    request: Request,
) -> Response {
    let instance = request.uri().path().to_string();
    let settings = match state.settings.settings(&ctx).await {
        Ok(settings) => settings,
        Err(error) => {
            return error_response(&I18nError::Config(error), &ctx, &instance);
        }
    };

    let accept_language = header_value(request.headers(), "accept-language");
    let (locale, locale_source) = negotiate_locale(LocaleLayers {
        user: query.locale.as_deref(),
        accept_language: accept_language.as_deref(),
        tenant: &settings.locale,
        registered: &state.locales,
    });
    let (timezone, timezone_source) =
        negotiate_timezone(query.timezone.as_deref(), &settings.timezone);
    let (currency, currency_source) = resolve_currency(&settings.currency);

    Json(ContextResponse {
        locale,
        timezone,
        currency,
        source: locale_source.as_str(),
        sources: FieldSources {
            locale: locale_source.as_str(),
            timezone: timezone_source.as_str(),
            currency: currency_source.as_str(),
        },
    })
    .into_response()
}

/// `/api/i18n/fx/convert` 的查询参数。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConvertQuery {
    /// 源币种（ISO 4217）。
    pub from: String,
    /// 目标币种（ISO 4217）。
    pub to: String,
    /// 源金额（**最小单位**，整数）。
    pub minor: i64,
    /// 快照时刻（RFC 3339）。**必填** —— 不隐式取最新（R4）。
    pub as_of: String,
}

/// `/api/i18n/fx/convert` 的响应（带可追溯信息，AC-S-097 步骤 4）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConvertResponse {
    /// 源币种。
    pub from: String,
    /// 目标币种。
    pub to: String,
    /// 源金额（最小单位）。
    pub minor: i64,
    /// 换算后金额（目标币种的最小单位，银行家舍入）。
    pub converted_minor: i64,
    /// 使用的汇率 × 10^6。
    pub rate_micros: i64,
    /// 快照行 ID（可追溯标识）。
    pub rate_id: i64,
    /// 快照时刻（RFC 3339 UTC）。
    pub as_of: String,
    /// 汇率来源。
    pub source: String,
}

/// 汇率换算。
async fn convert(
    State(state): State<Arc<I18nState>>,
    ReqCtx(ctx): ReqCtx,
    Query(query): Query<ConvertQuery>,
    request: Request,
) -> Response {
    let instance = request.uri().path().to_string();

    let from = match CurrencyCode::parse(&query.from) {
        Ok(code) => code,
        Err(error) => {
            return error_response(
                &I18nError::InvalidArgument {
                    param: "from".to_string(),
                    reason: error.detail(),
                },
                &ctx,
                &instance,
            );
        }
    };
    let to = match CurrencyCode::parse(&query.to) {
        Ok(code) => code,
        Err(error) => {
            return error_response(
                &I18nError::InvalidArgument {
                    param: "to".to_string(),
                    reason: error.detail(),
                },
                &ctx,
                &instance,
            );
        }
    };
    if !is_rfc3339(&query.as_of) {
        return error_response(
            &I18nError::InvalidArgument {
                param: "asOf".to_string(),
                reason: "expected an RFC 3339 timestamp such as `2026-01-01T00:00:00Z`".to_string(),
            },
            &ctx,
            &instance,
        );
    }

    let snapshot = match state.rates.resolve(from, to, &query.as_of).await {
        Ok(Some(snapshot)) => snapshot,
        Ok(None) => {
            return error_response(
                &I18nError::RateNotFound {
                    from: from.as_str().to_string(),
                    to: to.as_str().to_string(),
                    as_of: query.as_of.clone(),
                },
                &ctx,
                &instance,
            );
        }
        Err(reason) => {
            return error_response(&I18nError::Internal { reason }, &ctx, &instance);
        }
    };

    let amount = Money::from_minor(query.minor, from);
    let converted = match snapshot.convert(&amount) {
        Ok(converted) => converted,
        Err(error) => return error_response(&I18nError::Money(error), &ctx, &instance),
    };

    Json(ConvertResponse {
        from: from.as_str().to_string(),
        to: to.as_str().to_string(),
        minor: query.minor,
        converted_minor: converted.minor_units(),
        rate_micros: snapshot.rate_micros,
        rate_id: snapshot.id,
        as_of: snapshot.as_of_rfc3339(),
        source: snapshot.source,
    })
    .into_response()
}

/// 读一个请求头（去空白；空值视为没有）。
fn header_value(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

/// 从 `Authorization: Bearer <token>` 里取令牌。
fn bearer_token(headers: &HeaderMap) -> Option<String> {
    let raw = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let trimmed = raw.trim();
    let token = trimmed
        .strip_prefix("Bearer ")
        .or_else(|| trimmed.strip_prefix("bearer "))?;
    let token = token.trim();
    if token.is_empty() {
        None
    } else {
        Some(token.to_string())
    }
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn registered(codes: &[&str]) -> BTreeSet<String> {
        codes.iter().map(|code| (*code).to_string()).collect()
    }

    /// 造一个设置（**来源由调用方给**：`Tenant` / `Space` / `Platform` / `Default`）。
    ///
    /// D119：同一个构造函数既能造租户层的值，也能造平台层的值 ——
    /// 两者的差别只在 `source`，不在构造方式。
    fn setting_with_source(value: &str, source: ConfigSource, default: &str) -> Setting {
        Setting {
            value: value.to_string(),
            source,
            manifest_default: default.to_string(),
        }
    }

    // -- Accept-Language 解析 ---------------------------------------------

    #[test]
    fn accept_language_is_sorted_by_quality() {
        let ranges = parse_accept_language("zh-CN;q=0.9, en-US;q=0.8");
        assert_eq!(ranges[0].tag, "zh-CN");
        assert_eq!(ranges[0].quality, 900);
        assert_eq!(ranges[1].tag, "en-US");
        assert_eq!(ranges[1].quality, 800);
    }

    #[test]
    fn accept_language_defaults_quality_to_one_and_keeps_input_order_on_ties() {
        let ranges = parse_accept_language("zh-CN, en-US");
        assert_eq!(ranges[0].quality, 1_000);
        assert_eq!(ranges[0].tag, "zh-CN", "同权重保持客户端顺序");
        assert_eq!(ranges[1].tag, "en-US");
    }

    #[test]
    fn accept_language_keeps_zero_quality_and_wildcards_for_the_caller() {
        let ranges = parse_accept_language("ja-JP;q=0, *;q=0.5");
        assert!(
            ranges
                .iter()
                .any(|range| range.tag == "ja-JP" && range.quality == 0)
        );
        assert!(ranges.iter().any(LanguageRange::is_wildcard));
    }

    #[test]
    fn malformed_accept_language_items_are_dropped() {
        let ranges = parse_accept_language("zh-CN;q=abc, en-US;q=1.5, !!, fr-FR");
        assert_eq!(ranges.len(), 1, "坏项整条丢弃，不拖垮整个头：{ranges:?}");
        assert_eq!(ranges[0].tag, "fr-FR");
    }

    // -- AC-S-090 的 8 步（纯函数侧）--------------------------------------

    /// 步骤 1：只带 `Accept-Language: zh-CN`，无用户偏好、无租户配置。
    #[test]
    fn ac090_step1_accept_language_wins_over_the_platform_default() {
        let reg = registered(&["zh-CN", "en-US"]);
        let tenant = Setting::default_only("zh-CN");
        let (locale, source) = negotiate_locale(LocaleLayers {
            user: None,
            accept_language: Some("zh-CN"),
            tenant: &tenant,
            registered: &reg,
        });
        assert_eq!(locale, "zh-CN");
        println!("步骤1: locale={locale} source={source:?}");
        assert_eq!(source, LocaleSource::AcceptLanguage);
    }

    /// 步骤 2：租户配 `en-US`，请求头 `zh-CN` → **请求头赢**（`zh-CN`）。
    #[test]
    fn ac090_step2_accept_language_wins_over_the_tenant_config() {
        let reg = registered(&["zh-CN", "en-US"]);
        let tenant = setting_with_source("en-US", ConfigSource::Tenant, "zh-CN");
        let (locale, source) = negotiate_locale(LocaleLayers {
            user: None,
            accept_language: Some("zh-CN"),
            tenant: &tenant,
            registered: &reg,
        });
        assert_eq!(locale, "zh-CN");
        println!("步骤2: locale={locale} source={source:?}");
        assert_eq!(source, LocaleSource::AcceptLanguage);
    }

    /// 步骤 3：用户偏好 `ja-JP`（**未注册**）→ 视为没设，落到请求头 `zh-CN`。
    #[test]
    fn ac090_step3_an_unregistered_user_preference_is_ignored() {
        let reg = registered(&["zh-CN", "en-US"]);
        let tenant = setting_with_source("en-US", ConfigSource::Tenant, "zh-CN");
        let (locale, source) = negotiate_locale(LocaleLayers {
            user: Some("ja-JP"),
            accept_language: Some("zh-CN"),
            tenant: &tenant,
            registered: &reg,
        });
        assert_eq!(locale, "zh-CN", "不是 ja-JP、不是租户的 en-US");
        println!("步骤3: locale={locale} source={source:?}");
        assert_eq!(source, LocaleSource::AcceptLanguage);
    }

    /// 步骤 4：用户偏好 `en-US`，请求头**不带** `Accept-Language` → `en-US`（user）。
    #[test]
    fn ac090_step4_the_user_preference_applies_without_any_header() {
        let reg = registered(&["zh-CN", "en-US"]);
        let tenant = Setting::default_only("zh-CN");
        let (locale, source) = negotiate_locale(LocaleLayers {
            user: Some("en-US"),
            accept_language: None,
            tenant: &tenant,
            registered: &reg,
        });
        assert_eq!(locale, "en-US");
        println!("步骤4: locale={locale} source={source:?}");
        assert_eq!(source, LocaleSource::User);
    }

    /// 步骤 5：请求头 `fr-FR`（未注册）+ 租户 `en-US` → 回退到租户。
    #[test]
    fn ac090_step5_an_unregistered_header_falls_back_to_the_tenant() {
        let reg = registered(&["zh-CN", "en-US"]);
        let tenant = setting_with_source("en-US", ConfigSource::Tenant, "zh-CN");
        let (locale, source) = negotiate_locale(LocaleLayers {
            user: None,
            accept_language: Some("fr-FR"),
            tenant: &tenant,
            registered: &reg,
        });
        assert_eq!(locale, "en-US");
        println!("步骤5: locale={locale} source={source:?}");
        assert_eq!(source, LocaleSource::Tenant);
        assert_ne!(locale, "fr-FR");
    }

    /// 步骤 6：`zh-CN;q=0.9, en-US;q=0.8` → 按 q 最高的已注册语言。
    #[test]
    fn ac090_step6_quality_values_decide() {
        let reg = registered(&["zh-CN", "en-US"]);
        let tenant = Setting::default_only("zh-CN");
        let (locale, source) = negotiate_locale(LocaleLayers {
            user: None,
            accept_language: Some("zh-CN;q=0.9, en-US;q=0.8"),
            tenant: &tenant,
            registered: &reg,
        });
        assert_eq!(locale, "zh-CN");
        println!("步骤6: locale={locale} source={source:?}");
        assert_eq!(source, LocaleSource::AcceptLanguage);
    }

    /// 步骤 7：`*` → **平台默认**（不做任意匹配）。
    #[test]
    fn ac090_step7_wildcard_means_the_platform_default() {
        let reg = registered(&["zh-CN", "en-US"]);
        // 租户显式配了 en-US —— 若实现成「任意匹配」或「落到租户」，这条就会红。
        let tenant = setting_with_source("en-US", ConfigSource::Tenant, "zh-CN");
        let (locale, source) = negotiate_locale(LocaleLayers {
            user: None,
            accept_language: Some("*"),
            tenant: &tenant,
            registered: &reg,
        });
        assert_eq!(locale, "zh-CN");
        println!("步骤7: locale={locale} source={source:?}");
        assert_eq!(source, LocaleSource::Default);
    }

    /// 步骤 8 ★：用户偏好 `en-US`（已注册）+ 请求头 `zh-CN`（相反）
    /// → `locale === "en-US"` 且 `source === "user"`。
    ///
    /// 这是**优先级的唯一有效判据** —— 两者相同时无法区分谁生效。
    #[test]
    fn ac090_step8_the_user_preference_beats_a_contradicting_header() {
        let reg = registered(&["zh-CN", "en-US"]);
        let tenant = setting_with_source("zh-CN", ConfigSource::Tenant, "zh-CN");
        let (locale, source) = negotiate_locale(LocaleLayers {
            user: Some("en-US"),
            accept_language: Some("zh-CN"),
            tenant: &tenant,
            registered: &reg,
        });
        assert_eq!(locale, "en-US");
        println!("步骤8: locale={locale} source={source:?}");
        assert_eq!(source, LocaleSource::User);
    }

    /// `q=0` 是「明确拒绝」：跳过后选下一个已注册的。
    #[test]
    fn a_zero_quality_language_is_skipped() {
        let reg = registered(&["zh-CN", "en-US"]);
        let tenant = Setting::default_only("zh-CN");
        let (locale, source) = negotiate_locale(LocaleLayers {
            user: None,
            accept_language: Some("zh-CN;q=0, en-US;q=0.5"),
            tenant: &tenant,
            registered: &reg,
        });
        assert_eq!(locale, "en-US");
        assert_eq!(source, LocaleSource::AcceptLanguage);
    }

    /// **任何情况下**结果都是已注册语言（否则前端加载不到翻译包）。
    #[test]
    fn the_result_is_always_a_registered_locale() {
        let reg = registered(&["zh-CN", "en-US"]);
        let cases = [
            (None, None),
            (None, Some("fr-FR")),
            (None, Some("*")),
            (Some("ja-JP"), Some("de-DE")),
            (Some("en-US"), Some("fr-FR")),
        ];
        for (user, accept) in cases {
            let tenant = setting_with_source("fr-FR", ConfigSource::Tenant, "zh-CN");
            let (locale, _) = negotiate_locale(LocaleLayers {
                user,
                accept_language: accept,
                tenant: &tenant,
                registered: &reg,
            });
            assert!(
                reg.contains(&locale),
                "协商结果必须是已注册语言：{locale}（user={user:?} accept={accept:?}）"
            );
        }
    }

    /// 租户值未注册（配置写在翻译包之前）→ 不返回它，落到平台默认。
    #[test]
    fn an_unregistered_tenant_locale_falls_back_to_the_platform_default() {
        let reg = registered(&["zh-CN"]);
        let tenant = setting_with_source("ja-JP", ConfigSource::Tenant, "zh-CN");
        let (locale, source) = negotiate_locale(LocaleLayers {
            user: None,
            accept_language: None,
            tenant: &tenant,
            registered: &reg,
        });
        assert_eq!(locale, "zh-CN");
        assert_eq!(source, LocaleSource::Default);
    }

    // -- 时区（AC-S-095 / §3.4）-------------------------------------------

    /// 时区**不看** `Accept-Language`；来源只有 user / tenant / default。
    #[test]
    fn timezone_negotiation_never_uses_accept_language() {
        let tenant = setting_with_source("America/New_York", ConfigSource::Tenant, "Asia/Shanghai");
        let (value, source) = negotiate_timezone(None, &tenant);
        assert_eq!(value, "America/New_York");
        assert_eq!(source, LocaleSource::Tenant);

        let (value, source) = negotiate_timezone(Some("Europe/London"), &tenant);
        assert_eq!(value, "Europe/London");
        assert_eq!(source, LocaleSource::User);

        let default_only = Setting::default_only("Asia/Shanghai");
        let (value, source) = negotiate_timezone(None, &default_only);
        assert_eq!(value, "Asia/Shanghai");
        assert_eq!(source, LocaleSource::Default);

        for (_, source) in [
            negotiate_timezone(None, &tenant),
            negotiate_timezone(Some("Europe/London"), &tenant),
            negotiate_timezone(None, &default_only),
        ] {
            assert_ne!(
                source,
                LocaleSource::AcceptLanguage,
                "时区来源永不出现 accept-language"
            );
        }
    }

    /// 非法的用户时区被忽略（D116：仅当格式合法时生效）。
    #[test]
    fn an_invalid_user_timezone_is_ignored() {
        let tenant = Setting::default_only("Asia/Shanghai");
        let (value, source) = negotiate_timezone(Some("Not/AZone"), &tenant);
        assert_eq!(value, "Asia/Shanghai");
        assert_eq!(source, LocaleSource::Default);
    }

    // -- 币种 --------------------------------------------------------------

    /// 币种只有「租户 > 平台 > 默认」三级（**没有**用户级覆盖，D112）。
    #[test]
    fn currency_has_tenant_platform_and_default_tiers() {
        let tenant = setting_with_source("USD", ConfigSource::Tenant, "CNY");
        assert_eq!(
            resolve_currency(&tenant),
            ("USD".to_string(), LocaleSource::Tenant)
        );
        // D119：只有平台层配了 → 值生效，来源如实报 `platform`（不是 `tenant`）。
        let platform = setting_with_source("USD", ConfigSource::Platform, "CNY");
        assert_eq!(
            resolve_currency(&platform),
            ("USD".to_string(), LocaleSource::Platform)
        );
        let default_only = Setting::default_only("CNY");
        assert_eq!(
            resolve_currency(&default_only),
            ("CNY".to_string(), LocaleSource::Default)
        );
    }

    // -- D119：平台配置层加入协商 -------------------------------------------

    /// D119 的层级判定：每个配置层映射到**自己的**来源值。
    ///
    /// 平台层必须报 `platform` —— 报成 `tenant` 等于把 M1 换了个形式，没真修。
    #[test]
    fn d119_each_config_layer_maps_to_its_own_source() {
        assert_eq!(
            setting_with_source("x", ConfigSource::Platform, "d").layer_source(),
            Some(LocaleSource::Platform)
        );
        assert_eq!(
            setting_with_source("x", ConfigSource::Tenant, "d").layer_source(),
            Some(LocaleSource::Tenant)
        );
        // 空间层是租户之下的更深层，对外一律报 `tenant`（`LocaleSource` 没有 space 取值）。
        assert_eq!(
            setting_with_source("x", ConfigSource::Space, "d").layer_source(),
            Some(LocaleSource::Tenant)
        );
        assert_eq!(
            setting_with_source("x", ConfigSource::Default, "d").layer_source(),
            None,
            "默认层没有来源 → 回落 manifest 默认值"
        );

        // `source` 的取值集合是 **5 个**（序列化后小写）。
        let names: Vec<&str> = [
            LocaleSource::User,
            LocaleSource::AcceptLanguage,
            LocaleSource::Tenant,
            LocaleSource::Platform,
            LocaleSource::Default,
        ]
        .iter()
        .map(|source| source.as_str())
        .collect();
        assert_eq!(
            names,
            vec!["user", "accept-language", "tenant", "platform", "default"],
            "source 的取值集合必须是这 5 个"
        );
        assert_eq!(
            serde_json::to_value(LocaleSource::Platform).expect("应能序列化"),
            serde_json::json!("platform"),
            "平台层序列化后是小写 `platform`（与 ConfigSource::as_str 一致）"
        );
    }

    /// **TC-021-096**：平台级 `locale=en-US`、租户未配 → `locale == "en-US"`，
    /// 且 `source == "platform"`（**不是** `default`，也不是 `tenant`）。
    ///
    /// 语言 / 时区 / 币种**三个字段一起验**（D119 的三个协商函数）。
    #[test]
    fn tc021_096_the_platform_layer_is_used_when_the_tenant_has_not_configured_it() {
        let reg = registered(&["zh-CN", "en-US"]);

        // 语言：平台层的值生效，来源报 `platform`。
        let platform_locale = setting_with_source("en-US", ConfigSource::Platform, "zh-CN");
        let (locale, locale_source) = negotiate_locale(LocaleLayers {
            user: None,
            accept_language: None,
            tenant: &platform_locale,
            registered: &reg,
        });
        assert_eq!(locale, "en-US", "平台级的语言必须生效");
        assert_eq!(locale_source, LocaleSource::Platform, "如实报出是平台层");
        assert_ne!(locale, "zh-CN", "不能退回 manifest 默认值");
        assert_ne!(locale_source, LocaleSource::Tenant, "平台层不是租户层");
        assert_ne!(locale_source, LocaleSource::Default, "平台层不是默认层");

        // 时区：平台层的值生效，来源报 `platform`。
        let platform_timezone =
            setting_with_source("America/New_York", ConfigSource::Platform, "Asia/Shanghai");
        let (timezone, timezone_source) = negotiate_timezone(None, &platform_timezone);
        assert_eq!(timezone, "America/New_York", "平台级的时区必须生效");
        assert_eq!(timezone_source, LocaleSource::Platform, "如实报出是平台层");
        assert_ne!(timezone, "Asia/Shanghai", "不能退回 manifest 默认值");

        // 币种：平台层的值生效，来源报 `platform`。
        let platform_currency = setting_with_source("USD", ConfigSource::Platform, "CNY");
        let (currency, currency_source) = resolve_currency(&platform_currency);
        assert_eq!(currency, "USD", "平台级的币种必须生效");
        assert_eq!(currency_source, LocaleSource::Platform, "如实报出是平台层");
        assert_ne!(currency, "CNY", "不能退回 manifest 默认值");
    }

    /// **TC-021-097**：平台 `en-US`、租户 `ja-JP` → `locale == "ja-JP"`，
    /// 且 `source == "tenant"`（租户层压过平台层）。
    ///
    /// 协商拿到的是**配置层已解析好的那一层**（租户 > 平台的优先级由解析器保证），
    /// 所以这里断言的是：命中租户层时，返回的是**租户的值**、来源报 `tenant`。
    /// 「租户压过平台」的跨层判据在真实库用例里验（`i18n_settings_real_db.rs`）。
    #[test]
    fn tc021_097_the_tenant_layer_wins_over_the_platform_layer() {
        let reg = registered(&["zh-CN", "en-US", "ja-JP"]);

        // 语言：租户层的值（ja-JP）生效，来源报 `tenant`。
        let tenant_locale = setting_with_source("ja-JP", ConfigSource::Tenant, "zh-CN");
        let (locale, locale_source) = negotiate_locale(LocaleLayers {
            user: None,
            accept_language: None,
            tenant: &tenant_locale,
            registered: &reg,
        });
        assert_eq!(locale, "ja-JP", "租户层的语言必须压过平台层");
        assert_eq!(locale_source, LocaleSource::Tenant, "如实报出是租户层");
        assert_ne!(locale, "en-US", "不能取平台层的值");
        assert_ne!(locale_source, LocaleSource::Platform, "租户层不是平台层");

        // 时区：租户层的值生效，来源报 `tenant`。
        let tenant_timezone =
            setting_with_source("Europe/London", ConfigSource::Tenant, "Asia/Shanghai");
        let (timezone, timezone_source) = negotiate_timezone(None, &tenant_timezone);
        assert_eq!(timezone, "Europe/London", "租户层的时区必须压过平台层");
        assert_eq!(timezone_source, LocaleSource::Tenant, "如实报出是租户层");
        assert_ne!(timezone, "America/New_York", "不能取平台层的值");

        // 币种：租户层的值生效，来源报 `tenant`。
        let tenant_currency = setting_with_source("JPY", ConfigSource::Tenant, "CNY");
        let (currency, currency_source) = resolve_currency(&tenant_currency);
        assert_eq!(currency, "JPY", "租户层的币种必须压过平台层");
        assert_eq!(currency_source, LocaleSource::Tenant, "如实报出是租户层");
        assert_ne!(currency, "USD", "不能取平台层的值");
    }

    // -- RFC 3339 ----------------------------------------------------------

    #[test]
    fn rfc3339_shape_check_accepts_the_contract_shapes() {
        for good in [
            "2026-01-01T00:00:00Z",
            "2026-01-01T00:00:00.123Z",
            "2026-01-01T08:00:00+08:00",
            "2026-06-01t00:00:00z",
        ] {
            assert!(is_rfc3339(good), "`{good}` 应被放行");
        }
        for bad in [
            "",
            "2026-01-01",
            "2026-01-01T00:00:00",
            "2026-13-40T00:00:00Z",
            "2026-01-01 00:00:00",
            "2026-01-01T00:00:00.Z",
            "2026-01-01T00:00:00+0800",
            "not a timestamp",
        ] {
            assert!(!is_rfc3339(bad), "`{bad}` 应被拒");
        }
    }

    // -- 错误体的 title 来自 TITLES ----------------------------------------

    #[test]
    fn error_codes_have_registered_ascii_titles() {
        let errors = [
            I18nError::InvalidArgument {
                param: "from".to_string(),
                reason: "bad".to_string(),
            },
            I18nError::Money(MoneyError::Overflow),
            I18nError::RateNotFound {
                from: "CNY".to_string(),
                to: "USD".to_string(),
                as_of: "2026-01-01T00:00:00Z".to_string(),
            },
        ];
        for error in errors {
            let title = title_of_code(error.code());
            assert!(title.is_ascii(), "title 必须是 ASCII：{title}");
            assert_ne!(title, error.code(), "title 不得是 code 本身");
            assert!(error.detail().is_ascii(), "detail 必须是 ASCII");
        }
        assert_eq!(I18nError::Money(MoneyError::Overflow).status(), 400);
        assert_eq!(
            I18nError::RateNotFound {
                from: "CNY".to_string(),
                to: "USD".to_string(),
                as_of: "2026-01-01T00:00:00Z".to_string(),
            }
            .status(),
            404
        );
    }
}
