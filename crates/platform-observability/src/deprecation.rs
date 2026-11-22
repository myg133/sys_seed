//! API 弃用策略：`Deprecation` 与 `Sunset` 头（REQ-019，AC-S-036）。
//!
//! # 规范
//!
//! | 头 | 取值 | 依据 |
//! |---|---|---|
//! | `Deprecation` | **`true`** | RFC 9745（2025）。该 RFC 允许 `true` 或日期；**本实现统一用 `true`** —— 弃用是布尔状态，「什么时候宣布弃用」对客户端没有操作价值 |
//! | `Sunset` | **IMF-fixdate** | RFC 8594。形如 `Wed, 30 Jun 2027 00:00:00 GMT` |
//!
//! # ⚠️ `Sunset` 必须严格 IMF-fixdate
//!
//! 用 ISO8601（`2027-06-30T00:00:00Z`）或 Unix 时间戳都是**不符合 RFC** 的 ——
//! 客户端按 RFC 解析会失败，等于没发。判据用 `time` 的 `Rfc7231` 格式化器**反解**验证
//! （ISO8601 与时间戳都解不开 → 红）。
//!
//! # ⚠️ 弃用 ≠ 下线
//!
//! 被弃用的端点**仍然返回 200**。立刻返回 410 是**行为破坏**，客户端来不及迁移。

use axum::http::{HeaderName, HeaderValue};
use axum::response::Response;
use time::OffsetDateTime;

/// `Deprecation` 头（RFC 9745）。本实现统一用 `true`。
pub const DEPRECATION_HEADER: &str = "deprecation";
/// `Sunset` 头（RFC 8594）。
pub const SUNSET_HEADER: &str = "sunset";

/// 一个端点的弃用标记。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Deprecation {
    /// 停止提供的时刻（`Sunset` 头）。
    pub sunset: OffsetDateTime,
}

impl Deprecation {
    /// 用「停止提供」的时刻构造。
    pub fn new(sunset: OffsetDateTime) -> Self {
        Self { sunset }
    }

    /// 把 `Sunset` 格式化成 **IMF-fixdate**（RFC 8594 / RFC 7231）。
    ///
    /// # ⚠️ 为什么手写而不是用 `time` 的 well_known
    ///
    /// **实测**：`time 0.3.55` 的 `format_description::well_known` 里
    /// **只有** `Iso8601` / `Rfc2822` / `Rfc3339`，**没有 `Rfc7231`**。
    ///
    /// 而 `Rfc2822` 的输出是 `Sun, 06 Nov 1994 08:49:37 +0000`（时区写 `+0000`），
    /// 与 IMF-fixdate 的 `... 08:49:37 GMT` **不同** —— 直接用会不符合 RFC 8594。
    ///
    /// → 手工拼（月份/星期缩写是固定表，比 format_description 的 DSL 更不容易写错）。
    pub fn sunset_value(&self) -> String {
        const WEEKDAYS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
        const MONTHS: [&str; 12] = [
            "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
        ];

        // ⚠️ IMF-fixdate **只接受 GMT**（不接受其它时区）→ 先转 UTC。
        let utc = self.sunset.to_offset(time::UtcOffset::UTC);

        // `time` 的 `weekday().number_days_from_monday()` 返回 0..=6（周一起）。
        let weekday = WEEKDAYS[utc.weekday().number_days_from_monday() as usize];
        let month = MONTHS[(utc.month() as u8 - 1) as usize];

        format!(
            "{weekday}, {:02} {month} {:04} {:02}:{:02}:{:02} GMT",
            utc.day(),
            utc.year(),
            utc.hour(),
            utc.minute(),
            utc.second(),
        )
    }

    /// 给响应加上两个头。
    ///
    /// ⚠️ 只在**标记了弃用的端点**上调用 —— 不能给所有响应都加
    /// （AC-S-036 步骤 2 的阴性对照专门验这个）。
    pub fn apply(&self, response: &mut Response) {
        response.headers_mut().insert(
            HeaderName::from_static(DEPRECATION_HEADER),
            // RFC 9745 允许 `true`；本实现统一用它（D342）。
            HeaderValue::from_static("true"),
        );
        if let Ok(value) = HeaderValue::from_str(&self.sunset_value()) {
            response
                .headers_mut()
                .insert(HeaderName::from_static(SUNSET_HEADER), value);
        }
    }
}

/// 中间件：给匹配到的路由加弃用头。
///
/// # 用法
///
/// ```ignore
/// // 只包住被弃用的那一组路由，不是整个 app。
/// let deprecated = Router::new()
///     .route("/events", get(list_events))
///     .layer(axum::middleware::from_fn_with_state(deprecation, apply_deprecation));
/// ```
///
/// ⚠️ **不要**挂到全局 router 上 —— 那样所有端点都会带弃用头
/// （AC-S-036 步骤 2 会红）。
pub async fn apply_deprecation(
    axum::extract::State(deprecation): axum::extract::State<Deprecation>,
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    let mut response = next.run(request).await;
    deprecation.apply(&mut response);
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::datetime;

    /// `Sunset` 是严格 IMF-fixdate，且能被 `Rfc7231` **反解**回来。
    ///
    /// ⚠️ 反解比正则更强：ISO8601 与 Unix 时间戳都解不开 → 红。
    #[test]
    fn the_sunset_value_is_strict_imf_fixdate() {
        let deprecation = Deprecation::new(datetime!(2027-06-30 00:00:00 UTC));
        let value = deprecation.sunset_value();

        // ① 形态检查（人眼可读的期望）
        assert_eq!(value, "Wed, 30 Jun 2027 00:00:00 GMT");

        // ② 严格反解（这才是判据：客户端按 RFC 解析必须成功）
        //
        // ⚠️ `time 0.3.55` 没有 `Rfc7231` → 用 format_description 手工描述
        // IMF-fixdate 的形状。这同时是「格式真的合规」的判据：
        // 形状写错（比如少了 `GMT` 或月份不是缩写）→ 解析失败 → 用例红。
        let format = time::format_description::parse(
            "[weekday repr:short], [day] [month repr:short] [year] [hour]:[minute]:[second] GMT",
        )
        .expect("IMF-fixdate 的描述串应合法");
        let parsed = time::PrimitiveDateTime::parse(&value, &format)
            .expect("必须是合法的 IMF-fixdate（RFC 8594 要求）");
        // `Parsed` 是 `PrimitiveDateTime`（无时区）→ 与同形的常量比较
        assert_eq!(
            parsed,
            time::PrimitiveDateTime::new(
                datetime!(2027-06-30 00:00:00 UTC).date(),
                datetime!(2027-06-30 00:00:00 UTC).time(),
            )
        );
    }

    /// ISO8601 形态**不是**合法 IMF-fixdate（拒绝理由的判据）。
    #[test]
    fn an_iso8601_sunset_would_not_parse_as_imf_fixdate() {
        let iso = "2027-06-30T00:00:00Z";
        let format = time::format_description::parse(
            "[weekday repr:short], [day] [month repr:short] [year] [hour]:[minute]:[second] GMT",
        )
        .expect("描述串合法");
        assert!(
            time::PrimitiveDateTime::parse(iso, &format).is_err(),
            "ISO8601 不该被 IMF-fixdate 的形状解析成功"
        );
    }

    /// Unix 时间戳同样不是合法 IMF-fixdate。
    #[test]
    fn a_unix_timestamp_sunset_would_not_parse_as_imf_fixdate() {
        let format = time::format_description::parse(
            "[weekday repr:short], [day] [month repr:short] [year] [hour]:[minute]:[second] GMT",
        )
        .expect("描述串合法");
        assert!(
            time::PrimitiveDateTime::parse("1814371200", &format).is_err(),
            "Unix 时间戳不该被 IMF-fixdate 的形状解析成功"
        );
    }

    /// **`Rfc2822` 的输出不是 IMF-fixdate**（`+0000` vs `GMT`）。
    ///
    /// ⚠️ 这条记录了「为什么不能直接用 `Rfc2822`」—— 否则后续维护者
    /// 会以为可以直接换掉手写实现。
    #[test]
    fn the_rfc2822_form_is_not_imf_fixdate() {
        let instant = datetime!(2027-06-30 00:00:00 UTC);
        let rfc2822 = instant
            .format(&time::format_description::well_known::Rfc2822)
            .expect("Rfc2822 应能格式化");
        assert!(
            !rfc2822.ends_with("GMT"),
            "Rfc2822 用 +0000 而不是 GMT（这正是不能直接用的原因）：{rfc2822}"
        );
        assert!(rfc2822.ends_with("+0000"), "预期形态：{rfc2822}");
    }

    /// `Deprecation` 头逐字是 `true`（D342 统一形态）。
    #[test]
    fn the_deprecation_header_is_the_literal_true() {
        let deprecation = Deprecation::new(datetime!(2027-06-30 00:00:00 UTC));
        let mut response = Response::new(axum::body::Body::empty());
        deprecation.apply(&mut response);

        assert_eq!(
            response
                .headers()
                .get(DEPRECATION_HEADER)
                .and_then(|v| v.to_str().ok()),
            Some("true"),
            "Deprecation 必须是字面量 true"
        );
    }

    /// 两个头都加上了。
    #[test]
    fn both_headers_are_attached() {
        let deprecation = Deprecation::new(datetime!(2027-06-30 00:00:00 UTC));
        let mut response = Response::new(axum::body::Body::empty());
        deprecation.apply(&mut response);

        assert!(response.headers().get(DEPRECATION_HEADER).is_some());
        assert!(response.headers().get(SUNSET_HEADER).is_some());
    }

    /// 没有调 `apply` 的响应**不带**这两个头（阴性对照的基础）。
    #[test]
    fn an_untouched_response_has_neither_header() {
        let response = Response::new(axum::body::Body::empty());
        assert!(response.headers().get(DEPRECATION_HEADER).is_none());
        assert!(response.headers().get(SUNSET_HEADER).is_none());
    }

    /// 头名是小写（`HeaderName::from_static` 只接受小写，否则 panic）。
    #[test]
    fn the_header_names_are_lowercase() {
        assert_eq!(DEPRECATION_HEADER, "deprecation");
        assert_eq!(SUNSET_HEADER, "sunset");
        assert!(DEPRECATION_HEADER.chars().all(|c| !c.is_ascii_uppercase()));
        assert!(SUNSET_HEADER.chars().all(|c| !c.is_ascii_uppercase()));
    }

    /// 格式化不会产出兜底值（对合法时刻）。
    #[test]
    fn the_formatter_never_falls_back_for_valid_instants() {
        for year in [2027, 2030, 2035] {
            let deprecation =
                Deprecation::new(datetime!(2027-06-30 00:00:00 UTC).replace_year(year).unwrap());
            let value = deprecation.sunset_value();
            assert_ne!(value, "INVALID-SUNSET", "year={year} 不该走到兜底");
            assert!(value.ends_with(" GMT"), "IMF-fixdate 必须以 GMT 结尾：{value}");
        }
    }
}
