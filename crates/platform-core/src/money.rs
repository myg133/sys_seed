//! 金额与币种：**整数最小单位**、零浮点、银行家舍入的 FX 换算（REQ-021 §5）。
//!
//! # 为什么不用 `f64` / `rust_decimal`
//!
//! - `f64` 表示不了 `0.1` / `0.2`：`0.1 + 0.2 != 0.3`，金额里出现这种误差是**账错**；
//! - `rust_decimal` 要新增第三方依赖（违反本仓「零新依赖」，见 design §9 R6）。
//!
//! **整数就是最精确的小数**：`i64` 的最小单位（人民币的「分」、日元的「円」）
//! 能精确表达任何货币金额。上限约 `9.22×10^18` 最小单位（≈ `9.2×10^16` 元），
//! 超限**返回错误**（[`MoneyError::Overflow`] → HTTP 400 `MONEY_OVERFLOW`），
//! 不 panic、不静默回绕（AC-S-096 / 裁定 #11）。
//!
//! # 小数位数由**货币**决定，不是 locale（AC-S-098 / 裁定 #13）
//!
//! `ja-JP` 显示 **CNY** 仍是 2 位小数（`元 1,234.56`）；显示 **JPY** 才是 0 位
//! （`￥123,456`）。所以 [`CurrencyCode::exponent`] 的表是**钉死**的（裁定 #20）：
//!
//! | 货币 | exponent |
//! |------|----------|
//! | CNY / USD / EUR | 2 |
//! | JPY / KRW | 0 |
//! | 其余 ISO 4217 | 2（默认） |
//!
//! # 本模块**不碰数据库**
//!
//! 纯逻辑：`platform-core` 没有 sqlx。汇率快照的存取在
//! `platform-db` 的 `fx` 模块；HTTP 接口在 `platform-server`。

use std::fmt;

/// 时间点类型（`FxRate::as_of`）。
///
/// **重新导出**是为了让调用方不必自己依赖 `time` —— 快照的字段类型是公开 API 的一部分。
pub use time::OffsetDateTime;

/// 金额溢出（`i64` 最小单位放不下 / 中间量超出 `i64`）。
pub const CODE_MONEY_OVERFLOW: &str = "MONEY_OVERFLOW";
/// 汇率快照不存在（**HTTP 404**，裁定 #21 —— 不是 0、不是原值）。
pub const CODE_FX_RATE_NOT_FOUND: &str = "FX_RATE_NOT_FOUND";
/// 参数不合法（金额文本 / 币种代码 / 汇率值）。
pub const CODE_INVALID_ARGUMENT: &str = "INVALID_ARGUMENT";

/// ISO 4217 的**现行**货币代码（关税/基金/贵金属的 `X*` 记账单位不在内）。
///
/// # 为什么是白名单而不是正则
///
/// `^[A-Z]{3}$` 会放行 `XXX`（「无货币」）、`XTS`（测试用）与任意大写三字母 ——
/// **语义校验必须是收敛的**（D111 / 裁定 #8）。所以这里枚举真实代码。
///
/// # 局限（如实记录）
///
/// - 只收**现行**代码：已废止的（`BYR` / `ZWL`（2024 被 ZWG 取代）之外的旧码等）会被拒；
/// - 不含贵金属与基金记账单位（`XAU` / `XAG` / `XDR` / `XSU` / `XUA` / `XBA`…）：
///   它们不是可用于支付的货币，作为「币种」放行会让 exponent 与舍入规则失去意义；
/// - 新货币（例如某国换币）需要改这张表 —— 这是**刻意的**：错放行比错拒绝更难发现。
pub const ISO_4217_CURRENCIES: &[&str] = &[
    "AED", "AFN", "ALL", "AMD", "ANG", "AOA", "ARS", "AUD", "AWG", "AZN", "BAM", "BBD", "BDT",
    "BGN", "BHD", "BIF", "BMD", "BND", "BOB", "BOV", "BRL", "BSD", "BTN", "BWP", "BYN", "BZD",
    "CAD", "CDF", "CHE", "CHF", "CHW", "CLF", "CLP", "CNY", "COP", "COU", "CRC", "CUP", "CVE",
    "CZK", "DJF", "DKK", "DOP", "DZD", "EGP", "ERN", "ETB", "EUR", "FJD", "FKP", "GBP", "GEL",
    "GHS", "GIP", "GMD", "GNF", "GTQ", "GYD", "HKD", "HNL", "HTG", "HUF", "IDR", "ILS", "INR",
    "IQD", "IRR", "ISK", "JMD", "JOD", "JPY", "KES", "KGS", "KHR", "KMF", "KPW", "KRW", "KWD",
    "KYD", "KZT", "LAK", "LBP", "LKR", "LRD", "LSL", "LYD", "MAD", "MDL", "MGA", "MKD", "MMK",
    "MNT", "MOP", "MRU", "MUR", "MVR", "MWK", "MXN", "MXV", "MYR", "MZN", "NAD", "NGN", "NIO",
    "NOK", "NPR", "NZD", "OMR", "PAB", "PEN", "PGK", "PHP", "PKR", "PLN", "PYG", "QAR", "RON",
    "RSD", "RUB", "RWF", "SAR", "SBD", "SCR", "SDG", "SEK", "SGD", "SHP", "SLE", "SOS", "SRD",
    "SSP", "STN", "SVC", "SYP", "SZL", "THB", "TJS", "TMT", "TND", "TOP", "TRY", "TTD", "TWD",
    "TZS", "UAH", "UGX", "USD", "USN", "UYI", "UYU", "UYW", "UZS", "VED", "VES", "VND", "VUV",
    "WST", "XAF", "XCD", "XCG", "XOF", "XPF", "YER", "ZAR", "ZMW", "ZWG",
];

/// 该字符串是否是**真实**的 ISO 4217 货币代码（`XXXX` / `XXX` 必须被拒）。
pub fn is_iso4217(raw: &str) -> bool {
    ISO_4217_CURRENCIES.contains(&raw)
}

/// 金额相关的错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MoneyError {
    /// `i64` 最小单位放不下（含中间量溢出）。
    Overflow,
    /// 金额文本不是合法的十进制数（或小数位多于货币 exponent）。
    InvalidAmount {
        /// 原始文本（截断后回显，避免把超长输入塞进响应体）。
        input: String,
        /// 面向开发者的英文原因。
        reason: String,
    },
    /// 币种代码不是 ISO 4217 现行货币。
    UnknownCurrency {
        /// 原始代码。
        code: String,
    },
    /// 两个金额的币种不同（**不允许隐式混算**）。
    CurrencyMismatch {
        /// 左侧币种。
        left: String,
        /// 右侧币种。
        right: String,
    },
    /// 汇率值非法（必须为正数；`rate_micros` 是「汇率 × 10^6」）。
    InvalidRate {
        /// 原始汇率值。
        rate_micros: i64,
    },
}

impl MoneyError {
    /// 稳定的机器可读错误码（已登记进 `TITLES`）。
    pub fn code(&self) -> &'static str {
        match self {
            Self::Overflow => CODE_MONEY_OVERFLOW,
            Self::InvalidAmount { .. }
            | Self::UnknownCurrency { .. }
            | Self::InvalidRate { .. } => CODE_INVALID_ARGUMENT,
            Self::CurrencyMismatch { .. } => CODE_INVALID_ARGUMENT,
        }
    }

    /// HTTP 状态码。
    ///
    /// `MONEY_OVERFLOW` 是 **400**（裁定 #11：调用方给了放不下的金额，是请求问题，
    /// 不是服务端错）。`FX_RATE_NOT_FOUND` 是 404，但那是个**独立的**码，
    /// 由存储层在查不到快照时产生，不在本枚举里。
    pub fn status(&self) -> u16 {
        match self {
            Self::Overflow => 400,
            Self::InvalidAmount { .. }
            | Self::UnknownCurrency { .. }
            | Self::CurrencyMismatch { .. }
            | Self::InvalidRate { .. } => 400,
        }
    }

    /// 面向开发者的英文 ASCII 描述。
    pub fn detail(&self) -> String {
        match self {
            Self::Overflow => "the amount does not fit into i64 minor units".to_string(),
            Self::InvalidAmount { input, reason } => {
                format!("`{input}` is not a valid decimal amount: {reason}")
            }
            Self::UnknownCurrency { code } => {
                format!("`{code}` is not a current ISO 4217 currency code")
            }
            Self::CurrencyMismatch { left, right } => {
                format!("cannot combine `{left}` with `{right}` without an explicit conversion")
            }
            Self::InvalidRate { rate_micros } => {
                format!("the exchange rate `{rate_micros}` must be a positive scaled integer")
            }
        }
    }
}

impl fmt::Display for MoneyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}", self.code(), self.detail())
    }
}

impl std::error::Error for MoneyError {}

/// ISO 4217 三位货币代码。
///
/// 构造时校验：必须是 [`ISO_4217_CURRENCIES`] 里的真实代码 —— 于是
/// 「`Money` 的币种一定是真货币」是**类型不变量**，不是调用方的纪律。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CurrencyCode([u8; 3]);

impl CurrencyCode {
    /// 解析并校验一个 ISO 4217 代码（大写三字母 + 在现行清单内）。
    pub fn parse(raw: &str) -> Result<Self, MoneyError> {
        if !is_iso4217(raw) {
            return Err(MoneyError::UnknownCurrency {
                code: raw.to_string(),
            });
        }
        let bytes = raw.as_bytes();
        Ok(Self([bytes[0], bytes[1], bytes[2]]))
    }

    /// 三位代码（构造时已校验 ASCII，`from_utf8` 不会失败）。
    pub fn as_str(&self) -> &str {
        std::str::from_utf8(&self.0).unwrap_or("XXX")
    }

    /// 小数位数（裁定 #20 钉死的表；其余默认 2）。
    pub fn exponent(&self) -> u32 {
        match self.as_str() {
            "JPY" | "KRW" => 0,
            _ => 2,
        }
    }

    /// `10^exponent`（`exponent` 只会是 0 或 2，不会溢出）。
    pub fn minor_units_per_major(&self) -> i64 {
        10_i64.pow(self.exponent())
    }
}

impl fmt::Display for CurrencyCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// 金额：**整数最小单位** + 币种。
///
/// 字段私有 —— 只能经 [`Money::from_minor`] / [`Money::from_major`] 构造，
/// 于是「币种没校验」的状态不存在。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Money {
    minor_units: i64,
    currency: CurrencyCode,
}

impl Money {
    /// 从最小单位构造（`from_minor(123456)` + CNY = `1234.56`）。
    pub fn from_minor(minor_units: i64, currency: CurrencyCode) -> Self {
        Self {
            minor_units,
            currency,
        }
    }

    /// 从**十进制文本**构造（`"12.34"`）。
    ///
    /// # 为什么是字符串解析而不是 `parse::<f64>()`
    ///
    /// `"0.1".parse::<f64>()` 得到的不是 0.1 的精确值；再乘 `100` 取整就会
    /// 出现 `10.000000000000002 → 10` 或 `9.999999999999998 → 9` 这种**差一分**。
    /// 这里按字符逐位累加到 `i128`，再按货币 exponent 缩放，**全程整数**。
    ///
    /// 允许：可选正负号、整数部分至少一位、可选小数部分（位数 ≤ exponent）。
    /// 拒绝：空串、多小数点、非数字、小数位超出 exponent（`JPY` 的 `"1.5"`）。
    pub fn from_major(text: &str, currency: CurrencyCode) -> Result<Self, MoneyError> {
        let invalid = |reason: &str| MoneyError::InvalidAmount {
            input: truncate(text),
            reason: reason.to_string(),
        };

        let (negative, digits) = match text.strip_prefix('-') {
            Some(rest) => (true, rest),
            None => (false, text.strip_prefix('+').unwrap_or(text)),
        };
        if digits.is_empty() {
            return Err(invalid("the amount is empty"));
        }

        let mut parts = digits.split('.');
        let integer = parts.next().unwrap_or_default();
        let fraction = parts.next();
        if parts.next().is_some() {
            return Err(invalid("the amount contains more than one decimal point"));
        }
        if integer.is_empty() || !integer.bytes().all(|b| b.is_ascii_digit()) {
            return Err(invalid(
                "the integer part is empty or not a decimal digit sequence",
            ));
        }
        // 20 位十进制已接近 i64 上限；再多就一定溢出，先挡掉避免无谓的长循环。
        if integer.len() > 20 {
            return Err(MoneyError::Overflow);
        }

        let exponent = currency.exponent() as usize;
        if fraction.is_some_and(str::is_empty) {
            return Err(invalid("the amount ends with a decimal point"));
        }
        let fraction = fraction.unwrap_or("");
        if !fraction.bytes().all(|b| b.is_ascii_digit()) {
            return Err(invalid("the fraction part is not a decimal digit sequence"));
        }
        if fraction.len() > exponent {
            return Err(invalid(
                "the amount has more fraction digits than the currency exponent allows",
            ));
        }

        // 整数部分：逐位累加到 i128（20 位一定放得下）。
        let mut magnitude: i128 = 0;
        for byte in integer.bytes() {
            magnitude = magnitude * 10 + i128::from(byte - b'0');
        }
        // 缩放：× 10^exponent，再加上补齐后的小数部分。
        magnitude *= i128::from(currency.minor_units_per_major());
        let mut fraction_value: i128 = 0;
        for byte in fraction.bytes() {
            fraction_value = fraction_value * 10 + i128::from(byte - b'0');
        }
        for _ in fraction.len()..exponent {
            fraction_value *= 10;
        }
        magnitude += fraction_value;

        let total = if negative { -magnitude } else { magnitude };
        let minor_units = i64::try_from(total).map_err(|_| MoneyError::Overflow)?;
        Ok(Self {
            minor_units,
            currency,
        })
    }

    /// 最小单位。
    pub fn minor_units(&self) -> i64 {
        self.minor_units
    }

    /// 币种。
    pub fn currency(&self) -> CurrencyCode {
        self.currency
    }

    /// 加法（同币种；溢出 → [`MoneyError::Overflow`]）。
    pub fn checked_add(&self, other: &Self) -> Result<Self, MoneyError> {
        self.ensure_same_currency(other)?;
        let minor_units = self
            .minor_units
            .checked_add(other.minor_units)
            .ok_or(MoneyError::Overflow)?;
        Ok(Self {
            minor_units,
            currency: self.currency,
        })
    }

    /// 减法（同币种；溢出 → [`MoneyError::Overflow`]）。
    pub fn checked_sub(&self, other: &Self) -> Result<Self, MoneyError> {
        self.ensure_same_currency(other)?;
        let minor_units = self
            .minor_units
            .checked_sub(other.minor_units)
            .ok_or(MoneyError::Overflow)?;
        Ok(Self {
            minor_units,
            currency: self.currency,
        })
    }

    /// 乘以整数（溢出 → [`MoneyError::Overflow`]）。
    pub fn checked_mul_int(&self, factor: i64) -> Result<Self, MoneyError> {
        let minor_units = self
            .minor_units
            .checked_mul(factor)
            .ok_or(MoneyError::Overflow)?;
        Ok(Self {
            minor_units,
            currency: self.currency,
        })
    }

    /// 两个金额的币种必须一致。
    ///
    /// 注意：本类型**故意不提供**「换币种但不换算」的方法 —— 直接把 `CNY` 改成
    /// `USD` 而数额不变等于凭空改账，换算只能走 [`FxRate::convert`]。
    fn ensure_same_currency(&self, other: &Self) -> Result<(), MoneyError> {
        if self.currency != other.currency {
            return Err(MoneyError::CurrencyMismatch {
                left: self.currency.as_str().to_string(),
                right: other.currency.as_str().to_string(),
            });
        }
        Ok(())
    }
}

/// 换算用的缩放因子：汇率以「× 10^6」的整数存储（`rate_micros`）。
pub const RATE_SCALE: i128 = 1_000_000;

/// 把 `amount_minor` 按 `rate_micros` 换算成目标币种的最小单位，**银行家舍入**。
///
/// # 算法（写死，裁定 #12）
///
/// ```text
/// numerator = a_minor × rate_micros            // ← 必须 i128
/// quotient  = numerator / 1_000_000
/// remainder = numerator % 1_000_000
/// remainder*2 >  1_000_000 → quotient + 1
/// remainder*2 == 1_000_000 → 偶数化（quotient 为奇数才 +1）
/// try_into::<i64>() 失败 → MONEY_OVERFLOW
/// ```
///
/// # ⚠️ 负数必须按符号归一化
///
/// Rust 的 `/` 与 `%` **向零截断**，负数余数为负 →
/// `remainder * 2 > 1_000_000` 对负数**永不成立**，偶数化分支也走不到 →
/// **负数会系统性地偏向零侧**（`-2.5 → -2`，而银行家舍入应当是 `-2`；
/// `-3.5 → -3`，应当是 `-4`）。所以先取绝对值算完，再回填符号：
/// 银行家舍入满足 `RNE(-x) = -RNE(x)`，逐位一致。
///
/// # 为什么是 `i128`
///
/// 两个 `i64` 相乘可达 `≈8.5×10^37`，而 `i64` 上限只有 `9.2×10^18` ——
/// 用 `i64` 做中间量会静默错值（release）或 panic（debug）。
pub fn convert_minor_units(amount_minor: i64, rate_micros: i64) -> Result<i64, MoneyError> {
    if rate_micros <= 0 {
        return Err(MoneyError::InvalidRate { rate_micros });
    }
    let numerator = i128::from(amount_minor)
        .checked_mul(i128::from(rate_micros))
        .ok_or(MoneyError::Overflow)?;

    // 符号归一化（见上文）：只在**绝对值**上做舍入判定。
    let negative = numerator < 0;
    let magnitude = numerator.unsigned_abs();
    let quotient = magnitude / RATE_SCALE as u128;
    let remainder = magnitude % RATE_SCALE as u128;

    let mut rounded = quotient;
    let twice = remainder * 2;
    if twice > RATE_SCALE as u128 {
        rounded += 1;
    } else if twice == RATE_SCALE as u128 && rounded % 2 != 0 {
        // round half to even：奇数才进位。
        rounded += 1;
    }

    // `magnitude ≤ 8.5×10^37 < i128::MAX`，这里仍显式判一次以免将来放宽入参。
    if rounded > i128::MAX as u128 {
        return Err(MoneyError::Overflow);
    }
    let signed = if negative {
        -(rounded as i128)
    } else {
        rounded as i128
    };
    i64::try_from(signed).map_err(|_| MoneyError::Overflow)
}

/// 一条汇率快照（`platform_fx_rate` 的一行）。
///
/// # 为什么要带 `as_of` + `source`
///
/// AC-S-097 要求换算结果**可追溯**：同一笔 100 CNY 在不同日期的快照下换算结果不同，
/// 调用方必须能反查到「用的是哪一条汇率」。`as_of` 是快照标识，`source` 是来源
/// （`ecb` / `manual` …）。
///
/// # 为什么换算要**显式**传 `as_of`
///
/// 隐式取「最新」在并发下不自觉（R4）：两次请求之间插了一条新快照，
/// 同一个查询就会得到两个结果。快照由调用方显式选择 → 结果可复现。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FxRate {
    /// 快照行 ID（可追溯标识之一）。
    pub id: i64,
    /// 源币种。
    pub from: CurrencyCode,
    /// 目标币种。
    pub to: CurrencyCode,
    /// 汇率 × 10^6，整数存储。
    pub rate_micros: i64,
    /// 快照生效时刻（UTC）。
    pub as_of: OffsetDateTime,
    /// 来源（如 `manual` / `ecb`）。
    pub source: String,
}

impl FxRate {
    /// 快照时刻的 RFC 3339 UTC 文本（可追溯信息的一部分）。
    pub fn as_of_rfc3339(&self) -> String {
        format_rfc3339_utc(self.as_of)
    }

    /// 按本条快照换算金额。
    ///
    /// 入参金额的币种必须等于 `from`（防止「拿 USD 的金额按 CNY→USD 换算」）。
    pub fn convert(&self, amount: &Money) -> Result<Money, MoneyError> {
        if amount.currency() != self.from {
            return Err(MoneyError::CurrencyMismatch {
                left: amount.currency().as_str().to_string(),
                right: self.from.as_str().to_string(),
            });
        }
        let minor_units = convert_minor_units(amount.minor_units(), self.rate_micros)?;
        Ok(Money::from_minor(minor_units, self.to))
    }
}

/// 时间点 → RFC 3339 字符串（UTC，契约 C13）。
///
/// 放在本模块而不是 HTTP 层，是因为 `platform-server` **没有** `time` 依赖
/// （它只经 `platform-db` / `platform-core` 间接用到时间类型）；
/// 快照自己知道怎么把 `as_of` 渲染成传输格式，是更合适的归属。
pub fn format_rfc3339_utc(moment: OffsetDateTime) -> String {
    let utc = moment.to_offset(time::UtcOffset::UTC);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        utc.year(),
        u8::from(utc.month()),
        utc.day(),
        utc.hour(),
        utc.minute(),
        utc.second()
    )
}

/// 回显用截断（超长输入不进响应体，避免把用户数据无界放大）。
fn truncate(value: &str) -> String {
    const LIMIT: usize = 64;
    if value.chars().count() <= LIMIT {
        return value.to_string();
    }
    let mut out: String = value.chars().take(LIMIT).collect();
    out.push('…');
    out
}

/// 本模块登记进 `TITLES` 的错误码（完备性测试用）。
pub const ALL_CODES: &[&str] = &[
    CODE_MONEY_OVERFLOW,
    CODE_FX_RATE_NOT_FOUND,
    CODE_INVALID_ARGUMENT,
];

#[cfg(test)]
mod tests {
    use super::*;

    fn cny() -> CurrencyCode {
        CurrencyCode::parse("CNY").expect("CNY 是现行货币")
    }

    fn jpy() -> CurrencyCode {
        CurrencyCode::parse("JPY").expect("JPY 是现行货币")
    }

    fn usd() -> CurrencyCode {
        CurrencyCode::parse("USD").expect("USD 是现行货币")
    }

    // -- 构造与指数 --------------------------------------------------------

    /// AC-S-096 步骤 1：字段是 `i64` + 币种，不是浮点。
    #[test]
    fn money_is_integer_minor_units_plus_a_currency() {
        let money = Money::from_minor(123_456, cny());
        assert_eq!(money.minor_units(), 123_456);
        assert_eq!(money.currency(), cny());
        // 类型层面的判据：`i64` 能表示的最大值就是上限。
        assert_eq!(Money::from_minor(i64::MAX, cny()).minor_units(), i64::MAX);
    }

    /// exponent 表钉死（裁定 #20）。
    #[test]
    fn currency_exponents_are_pinned() {
        assert_eq!(cny().exponent(), 2);
        assert_eq!(usd().exponent(), 2);
        assert_eq!(CurrencyCode::parse("EUR").expect("EUR").exponent(), 2);
        assert_eq!(jpy().exponent(), 0);
        assert_eq!(CurrencyCode::parse("KRW").expect("KRW").exponent(), 0);
        // 其余默认 2（如 GBP）。
        assert_eq!(CurrencyCode::parse("GBP").expect("GBP").exponent(), 2);
    }

    /// 非真实货币代码必须被拒（`XXXX` 是「无货币」占位）。
    #[test]
    fn iso4217_validation_rejects_non_currencies() {
        for bad in ["XXXX", "XXX", "XTS", "XAU", "cny", "CN", "CNYY", ""] {
            assert!(!is_iso4217(bad), "`{bad}` 不是可支付的现行货币");
            assert_eq!(
                CurrencyCode::parse(bad).expect_err("应当被拒").code(),
                CODE_INVALID_ARGUMENT
            );
        }
        for good in ["CNY", "USD", "JPY", "EUR", "KRW", "BRL", "XAF"] {
            assert!(is_iso4217(good), "`{good}` 是现行货币");
        }
    }

    // -- from_major（字符串解析，零浮点）-----------------------------------

    /// `from_major` 按 exponent 缩放：CNY 两位、JPY 零位。
    #[test]
    fn from_major_scales_by_the_currency_exponent() {
        assert_eq!(
            Money::from_major("12.34", cny())
                .expect("合法")
                .minor_units(),
            1234
        );
        assert_eq!(
            Money::from_major("0.1", cny()).expect("合法").minor_units(),
            10
        );
        assert_eq!(
            Money::from_major("7", cny()).expect("合法").minor_units(),
            700
        );
        assert_eq!(
            Money::from_major("-1.5", cny())
                .expect("合法")
                .minor_units(),
            -150
        );
        assert_eq!(
            Money::from_major("123456", jpy())
                .expect("JPY 没有小数位")
                .minor_units(),
            123_456
        );
    }

    /// 非法文本必须报 `INVALID_ARGUMENT`，不是静默 0。
    #[test]
    fn from_major_rejects_malformed_text() {
        for bad in [
            "", "-", "+", ".5", "1.", "1.2.3", "1,5", " 1", "1 ", "abc", "--1",
        ] {
            let error = Money::from_major(bad, cny()).expect_err("应当被拒");
            assert_eq!(error.code(), CODE_INVALID_ARGUMENT, "输入 {bad:?}");
        }
        // JPY 的 exponent 是 0 → 带小数位必须被拒（否则会静默截断）。
        let error = Money::from_major("1.5", jpy()).expect_err("JPY 不允许小数位");
        assert_eq!(error.code(), CODE_INVALID_ARGUMENT);
    }

    /// 超长整数部分 → `MONEY_OVERFLOW`，不是 wrap。
    #[test]
    fn from_major_reports_overflow_instead_of_wrapping() {
        let error = Money::from_major("99999999999999999999", cny()).expect_err("应溢出");
        assert_eq!(error.code(), CODE_MONEY_OVERFLOW);
        // 超过 i64 最小单位上限（i64::MAX 分 = 92233720368547758.07 元）。
        let error = Money::from_major("92233720368547758.08", cny()).expect_err("应溢出");
        assert_eq!(error.code(), CODE_MONEY_OVERFLOW);
        // 边界值本身是可以表示的。
        assert_eq!(
            Money::from_major("92233720368547758.07", cny())
                .expect("边界值可表示")
                .minor_units(),
            i64::MAX
        );
    }

    // -- 算术：整数精确相等 ------------------------------------------------

    /// AC-S-096 步骤 2：`0.1 + 0.2 == 0.3`（**整数相等**，不是「差值 < eps」）。
    #[test]
    fn decimal_addition_is_exact() {
        let sum = Money::from_minor(10, cny())
            .checked_add(&Money::from_minor(20, cny()))
            .expect("不应溢出");
        assert_eq!(sum, Money::from_minor(30, cny()));
        assert_eq!(sum, Money::from_major("0.3", cny()).expect("合法"));

        let tenth = Money::from_major("0.1", cny()).expect("合法");
        let fifth = Money::from_major("0.2", cny()).expect("合法");
        assert_eq!(
            tenth.checked_add(&fifth).expect("不应溢出"),
            Money::from_major("0.3", cny()).expect("合法")
        );
    }

    /// AC-S-096 步骤 3：大额累加 1000 次后**精确**等于期望值（无累积误差）。
    #[test]
    fn large_accumulation_has_no_drift() {
        let unit = Money::from_major("9999999.99", cny()).expect("合法");
        let mut total = Money::from_minor(0, cny());
        for _ in 0..1000 {
            total = total.checked_add(&unit).expect("1000 次累加不应溢出");
        }
        assert_eq!(total.minor_units(), 999_999_999_000);
        assert_eq!(
            total,
            Money::from_major("9999999990.00", cny()).expect("合法")
        );
    }

    /// 溢出 → `MONEY_OVERFLOW`（400），不 panic、不回绕。
    #[test]
    fn addition_overflow_is_reported_not_wrapped() {
        let max = Money::from_minor(i64::MAX, cny());
        let error = max
            .checked_add(&Money::from_minor(1, cny()))
            .expect_err("应当溢出");
        assert_eq!(error.code(), CODE_MONEY_OVERFLOW);
        assert_eq!(error.status(), 400);
        assert_eq!(
            error.detail(),
            "the amount does not fit into i64 minor units"
        );

        let min = Money::from_minor(i64::MIN, cny());
        assert_eq!(
            min.checked_sub(&Money::from_minor(1, cny()))
                .expect_err("应当下溢")
                .code(),
            CODE_MONEY_OVERFLOW
        );
        assert_eq!(
            Money::from_minor(i64::MAX, cny())
                .checked_mul_int(2)
                .expect_err("应当溢出")
                .code(),
            CODE_MONEY_OVERFLOW
        );
    }

    /// 不同币种不能隐式相加。
    #[test]
    fn different_currencies_cannot_be_combined() {
        let error = Money::from_minor(100, cny())
            .checked_add(&Money::from_minor(100, usd()))
            .expect_err("不同币种应被拒");
        assert_eq!(error.code(), CODE_INVALID_ARGUMENT);
        assert!(error.detail().contains("CNY") && error.detail().contains("USD"));
    }

    // -- 银行家舍入 --------------------------------------------------------

    /// 恰好 `.5` 的边界：**round half to even**（`2.5→2`、`3.5→4`、`0.5→0`、`1.5→2`）。
    ///
    /// 用 `rate_micros = 1_000_000`（即 1:1）不产生余数，所以这里构造
    /// 「金额 × 0.5」`rate_micros = 500_000`：`5 × 0.5 = 2.5`。
    #[test]
    fn half_cases_round_to_even() {
        // 5 × 0.5 = 2.5 → 2（偶数）
        assert_eq!(convert_minor_units(5, 500_000).expect("合法"), 2);
        // 7 × 0.5 = 3.5 → 4（偶数）
        assert_eq!(convert_minor_units(7, 500_000).expect("合法"), 4);
        // 1 × 0.5 = 0.5 → 0（偶数）
        assert_eq!(convert_minor_units(1, 500_000).expect("合法"), 0);
        // 3 × 0.5 = 1.5 → 2（偶数）
        assert_eq!(convert_minor_units(3, 500_000).expect("合法"), 2);
        // 9 × 0.5 = 4.5 → 4（偶数）
        assert_eq!(convert_minor_units(9, 500_000).expect("合法"), 4);
        // 11 × 0.5 = 5.5 → 6（偶数）
        assert_eq!(convert_minor_units(11, 500_000).expect("合法"), 6);
    }

    /// ★ 负数舍入：**必须**与正数对称（`RNE(-x) = -RNE(x)`）。
    ///
    /// 这是「按原算法直接跑」会漏掉的一条：`%` 向零截断使负余数为负，
    /// `remainder * 2 > 1_000_000` 永不成立 → 负数会系统性偏向零。
    #[test]
    fn negative_amounts_round_symmetrically() {
        // -2.5 → -2（偶数；若按「向零截断」也会得到 -2，这条不足以区分）
        assert_eq!(convert_minor_units(-5, 500_000).expect("合法"), -2);
        // -3.5 → -4（偶数）。**向零截断会得到 -3** —— 这条是判据。
        assert_eq!(convert_minor_units(-7, 500_000).expect("合法"), -4);
        // -0.5 → 0（-0 归一为 0）
        assert_eq!(convert_minor_units(-1, 500_000).expect("合法"), 0);
        // -1.5 → -2（偶数）。向零截断会得到 -1。
        assert_eq!(convert_minor_units(-3, 500_000).expect("合法"), -2);
        // -4.5 → -4（偶数）。向零截断会得到 -4，相等，构造更强的例子：
        // -5.5 → -6（偶数）。向零截断会得到 -5。
        assert_eq!(convert_minor_units(-11, 500_000).expect("合法"), -6);
        // 非 .5 的负数：-2.6 → -3（四舍）；向零截断会得到 -2。
        assert_eq!(convert_minor_units(-13, 200_000).expect("合法"), -3);
        // -2.4 → -2。
        assert_eq!(convert_minor_units(-12, 200_000).expect("合法"), -2);
    }

    /// 每个正数结果与对应负数结果严格互为相反数（对称性的一般判据）。
    #[test]
    fn rounding_is_symmetric_for_a_range_of_rates() {
        for rate in [123_456_i64, 333_333, 500_000, 999_999, 1_000_000, 7] {
            for amount in [1_i64, 2, 3, 5, 7, 11, 999, 123_456] {
                let positive = convert_minor_units(amount, rate).expect("合法");
                let negative = convert_minor_units(-amount, rate).expect("合法");
                assert_eq!(
                    negative, -positive,
                    "rate={rate} amount={amount}：负数必须与正数对称"
                );
            }
        }
    }

    /// 非 `.5` 的普通舍入：`0.4 → 0`、`0.6 → 1`。
    #[test]
    fn ordinary_fractions_round_to_nearest() {
        assert_eq!(convert_minor_units(2, 200_000).expect("合法"), 0); // 0.4
        assert_eq!(convert_minor_units(3, 200_000).expect("合法"), 1); // 0.6
        assert_eq!(convert_minor_units(100, 1_000_000).expect("合法"), 100);
        assert_eq!(convert_minor_units(0, 1_234_567).expect("合法"), 0);
    }

    /// 中间量必须是 `i128`：`i64::MAX × 1_000_000` 在 `i64` 里必然溢出。
    #[test]
    fn the_intermediate_product_uses_i128() {
        // i64::MAX × 1e6 的精确值远远超出 i64 —— 用 i64 会算错或 panic。
        let result = convert_minor_units(i64::MAX, 1_000_000);
        assert_eq!(result.expect("i128 中间量应能算出来"), i64::MAX);
        // 大额放大 → 结果超出 i64 → MONEY_OVERFLOW（不 panic）。
        assert_eq!(
            convert_minor_units(i64::MAX, 2_000_000)
                .expect_err("应当溢出")
                .code(),
            CODE_MONEY_OVERFLOW
        );
        assert_eq!(
            convert_minor_units(i64::MIN, 2_000_000)
                .expect_err("应当溢出")
                .code(),
            CODE_MONEY_OVERFLOW
        );
    }

    /// 汇率必须为正（0 / 负数都不是合法汇率）。
    #[test]
    fn non_positive_rates_are_rejected() {
        for rate in [0_i64, -1, -1_000_000] {
            assert_eq!(
                convert_minor_units(100, rate).expect_err("应被拒").code(),
                CODE_INVALID_ARGUMENT
            );
        }
    }

    /// `as_of` 的传输格式是 RFC 3339 UTC（契约 C13）。
    #[test]
    fn snapshot_timestamps_render_as_utc_rfc3339() {
        let rate = FxRate {
            id: 1,
            from: cny(),
            to: usd(),
            rate_micros: 139_500,
            as_of: OffsetDateTime::from_unix_timestamp(1_767_225_600).expect("合法时间"),
            source: "manual".to_string(),
        };
        assert_eq!(rate.as_of_rfc3339(), "2026-01-01T00:00:00Z");
    }

    /// `FxRate::convert` 的结果带目标币种，且拒绝源币种不符的金额。
    #[test]
    fn fx_rate_converts_and_checks_the_source_currency() {
        let rate = FxRate {
            id: 7,
            from: cny(),
            to: usd(),
            rate_micros: 139_500, // 1 CNY = 0.1395 USD
            as_of: OffsetDateTime::from_unix_timestamp(1_767_225_600).expect("合法时间"),
            source: "manual".to_string(),
        };
        // 100.00 CNY = 10000 分 → 1395 分 = 13.95 USD
        let converted = rate
            .convert(&Money::from_minor(10_000, cny()))
            .expect("应能换算");
        assert_eq!(converted, Money::from_minor(1_395, usd()));
        assert_eq!(converted.currency(), usd());

        let error = rate
            .convert(&Money::from_minor(10_000, usd()))
            .expect_err("源币种不符应被拒");
        assert_eq!(error.code(), CODE_INVALID_ARGUMENT);
    }

    /// 不同快照 → 不同结果（AC-S-097 步骤 2/3 的纯函数侧判据）。
    #[test]
    fn different_snapshots_produce_different_results() {
        let base = FxRate {
            id: 1,
            from: cny(),
            to: usd(),
            rate_micros: 135_000,
            as_of: OffsetDateTime::from_unix_timestamp(1_767_225_600).expect("合法时间"),
            source: "manual".to_string(),
        };
        let later = FxRate {
            id: 2,
            rate_micros: 142_000,
            as_of: OffsetDateTime::from_unix_timestamp(1_780_099_200).expect("合法时间"),
            ..base.clone()
        };
        let amount = Money::from_minor(10_000, cny());
        let first = base.convert(&amount).expect("第一份快照");
        let second = later.convert(&amount).expect("第二份快照");
        assert_ne!(first, second);
        assert_eq!(first.minor_units(), 1_350);
        assert_eq!(second.minor_units(), 1_420);
        // 可追溯信息不同（`id` / `asOf`）—— 结果能反查到用的是哪条。
        assert_ne!(base.id, later.id);
        assert_ne!(base.as_of, later.as_of);
    }
}
