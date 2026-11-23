//! 请假单实体与状态闭集。
//!
//! # 状态机（D347）
//!
//! ```text
//! draft ──submit──► pending ──approve──► approved（终态）
//!                     └──reject──► rejected（终态）
//! ```
//!
//! `status` 的取值是**闭集 4 个**（AC-S-189 的 N1 修正）——数据库侧有 `CHECK`，
//! Rust 侧是 [`Status`] 枚举：两层都关死，就不存在「第五个值」被写进去的路径。

use platform_db::{FromRow, PgRow, Row, SqlxError};
use serde::{Deserialize, Serialize};
use time::{Date, Month, OffsetDateTime};

/// 状态闭集（**恰好 4 个值**）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    /// 草稿：建单后的初始状态。
    Draft,
    /// 待审批：已提交，等审批人处理。
    Pending,
    /// 已通过：终态。
    Approved,
    /// 已驳回：终态。
    Rejected,
}

impl Status {
    /// 闭集里的全部取值（供测试断言「没有第五个」）。
    pub const ALL: [Status; 4] = [
        Status::Draft,
        Status::Pending,
        Status::Approved,
        Status::Rejected,
    ];

    /// 落库 / 出参用的文本值（与数据库 `CHECK` 的取值逐字一致）。
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Draft => "draft",
            Status::Pending => "pending",
            Status::Approved => "approved",
            Status::Rejected => "rejected",
        }
    }

    /// 从库里的文本值还原；**不在闭集里就是 `None`**（调用方必须当错误处理）。
    pub fn parse(raw: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|status| status.as_str() == raw)
    }

    /// 是否终态：终态不再接受任何流转。
    pub fn is_terminal(self) -> bool {
        matches!(self, Status::Approved | Status::Rejected)
    }
}

/// 请假单。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeaveRequest {
    /// 主键（租户内唯一）。
    pub id: String,
    /// 归属租户（RLS 与 `{tenant}` 占位符共同强制）。
    pub tenant_id: String,
    /// 申请人（来自 `ctx.user_id()`，不由请求体决定）。
    pub applicant_id: String,
    /// 起始日期。
    pub starts_on: Date,
    /// 结束日期。
    pub ends_on: Date,
    /// 请假理由。
    pub reason: String,
    /// 状态（闭集 4 个）。
    pub status: Status,
    /// 审批人；未审批为 `None`。
    pub decided_by: Option<String>,
    /// 审批时间；未审批为 `None`。
    pub decided_at: Option<OffsetDateTime>,
    /// 创建时间。
    pub created_at: OffsetDateTime,
    /// 最后更新时间。
    pub updated_at: OffsetDateTime,
}

impl LeaveRequest {
    /// 新建一张**草稿**单（建单只做状态初始化，不产生流转）。
    pub fn new(
        tenant_id: impl Into<String>,
        applicant_id: impl Into<String>,
        starts_on: Date,
        ends_on: Date,
        reason: impl Into<String>,
        now: OffsetDateTime,
    ) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            tenant_id: tenant_id.into(),
            applicant_id: applicant_id.into(),
            starts_on,
            ends_on,
            reason: reason.into(),
            status: Status::Draft,
            decided_by: None,
            decided_at: None,
            created_at: now,
            updated_at: now,
        }
    }
}

/// 手写 `FromRow`：`status` 是闭集枚举，**解码时必须校验** ——
/// 库里出现闭集外的值（不该发生，`CHECK` 挡了）要报错而不是静默映射。
impl<'r> FromRow<'r, PgRow> for LeaveRequest {
    fn from_row(row: &'r PgRow) -> Result<Self, SqlxError> {
        let raw_status: String = Row::try_get(row, "status")?;
        let status = Status::parse(&raw_status).ok_or_else(|| SqlxError::ColumnDecode {
            index: "status".to_string(),
            source: Box::new(std::io::Error::other(format!(
                "leave request status is outside the closed set: {raw_status}"
            ))),
        })?;
        Ok(Self {
            id: Row::try_get(row, "id")?,
            tenant_id: Row::try_get(row, "tenant_id")?,
            applicant_id: Row::try_get(row, "applicant_id")?,
            starts_on: Row::try_get(row, "starts_on")?,
            ends_on: Row::try_get(row, "ends_on")?,
            reason: Row::try_get(row, "reason")?,
            status,
            decided_by: Row::try_get(row, "decided_by")?,
            decided_at: Row::try_get(row, "decided_at")?,
            created_at: Row::try_get(row, "created_at")?,
            updated_at: Row::try_get(row, "updated_at")?,
        })
    }
}

/// 解析 `YYYY-MM-DD`。
///
/// ⚠️ 手写而不是用 `Date::parse`：`time` 在本工作区只开了 `formatting`
/// feature（`parsing` 没开），为了一个日期格式去开 feature 不划算。
pub fn parse_date(raw: &str) -> Option<Date> {
    let year: i32 = raw.get(0..4)?.parse().ok()?;
    let month: u8 = raw.get(5..7)?.parse().ok()?;
    let day: u8 = raw.get(8..10)?.parse().ok()?;
    // 严格长度 + 分隔符：`2026-1-01` 之类必须拒绝，不能靠 `parse` 兜着。
    if raw.len() != 10 || raw.as_bytes()[4] != b'-' || raw.as_bytes()[7] != b'-' {
        return None;
    }
    Date::from_calendar_date(year, Month::try_from(month).ok()?, day).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_closed_set_has_exactly_four_values() {
        assert_eq!(Status::ALL.len(), 4);
        let round_trip: Vec<&str> = Status::ALL
            .into_iter()
            .map(|status| {
                Status::parse(status.as_str())
                    .expect("闭集内的值必须能还原")
                    .as_str()
            })
            .collect();
        assert_eq!(round_trip, vec!["draft", "pending", "approved", "rejected"]);
    }

    #[test]
    fn a_status_outside_the_closed_set_is_rejected() {
        assert_eq!(Status::parse("cancelled"), None);
        assert_eq!(Status::parse(""), None);
        assert_eq!(Status::parse("Draft"), None);
    }

    #[test]
    fn only_approved_and_rejected_are_terminal() {
        assert!(!Status::Draft.is_terminal());
        assert!(!Status::Pending.is_terminal());
        assert!(Status::Approved.is_terminal());
        assert!(Status::Rejected.is_terminal());
    }

    #[test]
    fn parse_date_accepts_iso_dates_and_rejects_loose_ones() {
        assert_eq!(
            parse_date("2026-12-01"),
            Some(Date::from_calendar_date(2026, Month::December, 1).unwrap())
        );
        assert_eq!(parse_date("2026-1-01"), None);
        assert_eq!(parse_date("2026-13-01"), None);
        assert_eq!(parse_date("2026-02-30"), None);
        assert_eq!(parse_date("not-a-date"), None);
    }
}
