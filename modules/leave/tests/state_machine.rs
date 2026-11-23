//! 状态机：**4 条合法流转 + 5 条非法流转**（AC-S-189 / design-summary §1.2）。
//!
//! 非法流转的每一条都断言**具体错误码** `LEAVE_INVALID_TRANSITION` ——
//! 只断言「返回了错误」会把「换个错误码」也算通过，那是假绿。
//!
//! 跑法：
//!
//! ```bash
//! cargo test -p leave --test state_machine -- --test-threads=1
//! ```

use leave::entity::Status;
use leave::service::{CODE_INVALID_TRANSITION, Decision, LeaveError, decide_from, submit_from};
use time::OffsetDateTime;

/// 合法流转的判定：必须是**目标状态**，不是「没报错」。
fn assert_moves(result: Result<Status, LeaveError>, expected: Status) {
    assert_eq!(result.expect("合法流转必须成功"), expected);
}

/// 非法流转的判定：409 + 具体错误码 + 说清「从哪个状态、做什么动作」。
fn assert_rejected(result: Result<Status, LeaveError>, from: Status, action: &'static str) {
    let error = result.expect_err("非法流转必须被拒绝");
    assert_eq!(
        error.code(),
        CODE_INVALID_TRANSITION,
        "错误码必须写死这个值"
    );
    assert_eq!(error.status(), 409, "非法流转必须是 409（不是 422）");
    match error {
        LeaveError::InvalidTransition {
            from: actual,
            action: actual_action,
        } => {
            assert_eq!(actual, from);
            assert_eq!(actual_action, action);
        }
        other => panic!("expected an invalid transition, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// 4 条合法流转
// ---------------------------------------------------------------------------

#[test]
fn a_new_request_starts_in_draft() {
    let day = time::Date::from_calendar_date(2026, time::Month::December, 1).expect("合法日期");
    let request = leave::entity::LeaveRequest::new(
        "tenant-a",
        "user_a@example.com",
        day,
        day,
        "family matters",
        OffsetDateTime::UNIX_EPOCH,
    );
    assert_eq!(request.status, Status::Draft);
    assert!(
        request.decided_by.is_none() && request.decided_at.is_none(),
        "建单时审批人与审批时间都必须为空"
    );
}

#[test]
fn submit_moves_draft_to_pending() {
    assert_moves(submit_from(Status::Draft), Status::Pending);
}

#[test]
fn approve_moves_pending_to_approved() {
    assert_moves(
        decide_from(Status::Pending, Decision::Approve),
        Status::Approved,
    );
}

#[test]
fn reject_moves_pending_to_rejected() {
    assert_moves(
        decide_from(Status::Pending, Decision::Reject),
        Status::Rejected,
    );
}

// ---------------------------------------------------------------------------
// 5 条非法流转（design-summary §1.2 的完整清单）
// ---------------------------------------------------------------------------

#[test]
fn submitting_a_pending_request_is_rejected() {
    // 重复提交：目的是防止「提交两次 = 两条待办」。
    assert_rejected(submit_from(Status::Pending), Status::Pending, "submit");
}

#[test]
fn submitting_an_approved_request_is_rejected() {
    assert_rejected(submit_from(Status::Approved), Status::Approved, "submit");
}

#[test]
fn deciding_an_approved_request_is_rejected() {
    assert_rejected(
        decide_from(Status::Approved, Decision::Approve),
        Status::Approved,
        "approve",
    );
}

#[test]
fn submitting_a_rejected_request_is_rejected() {
    assert_rejected(submit_from(Status::Rejected), Status::Rejected, "submit");
}

#[test]
fn deciding_a_rejected_request_is_rejected() {
    assert_rejected(
        decide_from(Status::Rejected, Decision::Reject),
        Status::Rejected,
        "reject",
    );
}

// ---------------------------------------------------------------------------
// 状态的闭集与终态
// ---------------------------------------------------------------------------

#[test]
fn the_status_set_is_closed_over_exactly_four_values() {
    assert_eq!(Status::ALL.len(), 4);
    for status in Status::ALL {
        assert_eq!(
            Status::parse(status.as_str()),
            Some(status),
            "闭集里的每个值都必须能落库再读回"
        );
    }
    for outside in ["cancelled", "withdrawn", "DRAFT", ""] {
        assert_eq!(
            Status::parse(outside),
            None,
            "闭集外的 {outside} 必须还原失败"
        );
    }
}

#[test]
fn approved_and_rejected_are_the_only_terminal_states() {
    for status in [Status::Approved, Status::Rejected] {
        assert!(status.is_terminal(), "{} 必须是终态", status.as_str());
        assert!(
            submit_from(status).is_err() && decide_from(status, Decision::Approve).is_err(),
            "终态必须拒绝所有动作"
        );
    }
    assert!(!Status::Draft.is_terminal());
    assert!(!Status::Pending.is_terminal());
}
