//! 启动自检：应用连接的角色**必须**不绕过 RLS（REQ-006）。
//!
//! # 为什么这是「拒绝启动」而不是「打个警告」
//!
//! PostgreSQL 的 RLS 对**超级用户**与带 `BYPASSRLS` 属性的角色**不生效**。
//! 应用用这种角色连接时，数据库层的兜底**完全失效** —— 租户隔离从
//! 「fail closed」变成「fail open」。
//!
//! 部署侧如果不小心把超级用户写进 `DATABASE_URL`，没有这道自检的话
//! **没有任何人会发现**：服务照跑、健康检查照过、数据照查，只是隔离没了。
//! 所以这里默认**拒绝启动**。
//!
//! # 表级前提为什么不在这里查
//!
//! 启动时业务表可能**还没建**（迁移与策略都是部署期的独立步骤：
//! `migrate` → `tenant-policies` → 应用）。所以表级前提由部署期的
//! `base tenant-policies --check` 负责（它失败就让部署失败），
//! 启动自检只管**角色**这一件 —— 它与表在不在无关，而且恰恰是最容易被配错的那件。
//!
//! # 逃生开关
//!
//! `BASE_ALLOW_SUPERUSER=true` 只放行「角色绕过 RLS」这一条，**仅供本地开发**。
//! 默认（false）必须拒绝 —— 见 `deploy/RUNBOOK.md`「应用角色与 RLS」。

use std::fmt;

use platform_db::{DbError, DbOptions, IsolationReport, TenantDb, require_isolation};

/// 启动自检的结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IsolationOutcome {
    /// 自检报告（角色 + 表状态）。
    pub report: IsolationReport,
    /// 是否**放行**了绕过 RLS 的角色（只有 `BASE_ALLOW_SUPERUSER=true` 才可能为 true）。
    pub superuser_allowed: bool,
}

/// 启动自检失败。进程据此 fail-fast 退出。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IsolationError {
    /// 面向运维的说明（中文）。
    pub message: String,
}

impl fmt::Display for IsolationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for IsolationError {}

/// 启动判定（纯函数，便于单测）。
///
/// - 角色**不**绕过 RLS → `Ok(false)`：正常放行。
/// - 角色绕过 RLS 且允许超级用户 → `Ok(true)`：放行，但记警告。
/// - 角色绕过 RLS 且**不允许** → `Err(message)`：拒绝启动（消息面向运维）。
pub fn startup_verdict(report: &IsolationReport, allow_superuser: bool) -> Result<bool, String> {
    if !report.role_bypasses_rls {
        return Ok(false);
    }
    if allow_superuser {
        return Ok(true);
    }
    Err(format!(
        "连接角色 `{}` 绕过 RLS（超级用户或 BYPASSRLS）—— 租户隔离会 fail open，数据库层兜底完全失效，拒绝启动。\n\
         应用连接必须用**普通角色**（base_platform_app 组的成员）；见 deploy/RUNBOOK.md「应用角色与 RLS」。\n\
         仅本地开发可用 BASE_ALLOW_SUPERUSER=true 临时放行（生产严禁）。",
        report.role
    ))
}

/// 启动自检：连接角色不得绕过 RLS。
///
/// 判据用平台的 [`require_isolation`] —— 它是「隔离前提」的唯一口径。
pub async fn verify_startup_isolation(
    database_url: &str,
    allow_superuser: bool,
) -> Result<IsolationOutcome, IsolationError> {
    let db = TenantDb::connect(database_url, DbOptions::default())
        .await
        .map_err(|err| IsolationError {
            message: format!("隔离自检失败：无法建立数据库连接：{err}"),
        })?;

    let report = match require_isolation(&db, &[]).await {
        Ok(report) => report,
        Err(DbError::RlsBypassed { role }) => IsolationReport {
            role,
            role_bypasses_rls: true,
            tables: Vec::new(),
        },
        Err(err) => {
            return Err(IsolationError {
                message: format!("隔离自检失败：{err}"),
            });
        }
    };

    match startup_verdict(&report, allow_superuser) {
        Ok(superuser_allowed) => Ok(IsolationOutcome {
            report,
            superuser_allowed,
        }),
        Err(message) => Err(IsolationError { message }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use platform_db::TableIsolation;

    /// 构造一份自检报告。
    fn report(role: &str, bypasses: bool) -> IsolationReport {
        IsolationReport {
            role: role.to_string(),
            role_bypasses_rls: bypasses,
            tables: Vec::<TableIsolation>::new(),
        }
    }

    #[test]
    fn a_normal_role_passes_the_startup_check() {
        let verdict = startup_verdict(&report("base_app", false), false).expect("普通角色应当放行");
        assert!(!verdict, "普通角色不是「放行的超级用户」");
    }

    #[test]
    fn a_superuser_is_rejected_by_default() {
        let err = startup_verdict(&report("base", true), false).expect_err("超级用户必须拒绝启动");
        assert!(err.contains("base"), "错误信息要点名角色：{err}");
        assert!(
            err.contains("BASE_ALLOW_SUPERUSER"),
            "错误信息要给逃生开关：{err}"
        );
    }

    #[test]
    fn the_escape_hatch_only_relaxes_the_role_check() {
        let verdict = startup_verdict(&report("base", true), true).expect("开关打开时应当放行");
        assert!(verdict, "放行时必须标记为「超级用户被放行」");
    }
}
