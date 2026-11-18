//! 健康检查：liveness（进程活着）与 readiness（数据库连通 + 迁移已应用）。
//!
//! readiness **必须**查数据库 —— 否则滚动更新时流量会打到还没迁移好的实例，
//! 表现为「偶发 500」，且极难定位（REQ-005 AC-S-006）。

use sqlx::PgPool;
use sqlx::migrate::Migrator;

/// 数据库不可达时的错误码。
pub const CODE_DB_UNAVAILABLE: &str = "DATABASE_UNAVAILABLE";
/// 迁移未全部应用时的错误码。
pub const CODE_MIGRATIONS_PENDING: &str = "MIGRATIONS_PENDING";
/// 迁移处于 dirty 状态（应用失败残留）时的错误码。
pub const CODE_MIGRATION_DIRTY: &str = "MIGRATION_DIRTY";

/// 就绪检查的结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Readiness {
    /// 数据库是否连通。
    pub database_ok: bool,
    /// 已应用的迁移版本。
    pub applied: Vec<i64>,
    /// 尚未应用的迁移版本。
    pub pending: Vec<i64>,
    /// 处于 dirty 状态的迁移版本（应用失败残留）。
    pub dirty: Option<i64>,
    /// 不就绪时的错误码；就绪时为 `None`。
    pub error_code: Option<&'static str>,
    /// 不就绪时的说明（面向运维）。
    pub detail: Option<String>,
}

impl Readiness {
    /// 是否就绪。
    pub fn is_ready(&self) -> bool {
        self.error_code.is_none()
    }
}

/// 检查就绪状态。
///
/// - 数据库连通性：`SELECT 1`
/// - 迁移状态：已应用版本是否覆盖当前镜像携带的全部迁移
pub async fn check_readiness(pool: &PgPool, migrator: &Migrator) -> Readiness {
    if let Err(err) = sqlx::query("SELECT 1").execute(pool).await {
        return Readiness {
            database_ok: false,
            applied: Vec::new(),
            pending: expected_versions(migrator),
            dirty: None,
            error_code: Some(CODE_DB_UNAVAILABLE),
            detail: Some(format!("database is unreachable: {err}")),
        };
    }

    let (applied, dirty) = match applied_versions(pool).await {
        Ok(state) => state,
        Err(err) => {
            return Readiness {
                database_ok: true,
                applied: Vec::new(),
                pending: expected_versions(migrator),
                dirty: None,
                error_code: Some(CODE_MIGRATIONS_PENDING),
                detail: Some(format!(
                    "migration state is unreadable, migrations may not have run: {err}"
                )),
            };
        }
    };

    let pending: Vec<i64> = expected_versions(migrator)
        .into_iter()
        .filter(|version| !applied.contains(version))
        .collect();

    let (error_code, detail) = if let Some(version) = dirty {
        (
            Some(CODE_MIGRATION_DIRTY),
            Some(format!("迁移 {version} 处于 dirty 状态，需人工处理")),
        )
    } else if !pending.is_empty() {
        (
            Some(CODE_MIGRATIONS_PENDING),
            Some(format!("待应用迁移：{pending:?}")),
        )
    } else {
        (None, None)
    };

    Readiness {
        database_ok: true,
        applied,
        pending,
        dirty,
        error_code,
        detail,
    }
}

/// 当前镜像携带的迁移版本（只取 up 方向，down 文件与 up 同版本号）。
fn expected_versions(migrator: &Migrator) -> Vec<i64> {
    let mut versions: Vec<i64> = migrator
        .iter()
        .filter(|migration| migration.migration_type.is_up_migration())
        .map(|migration| migration.version)
        .collect();
    versions.sort_unstable();
    versions
}

/// 读取已应用迁移：`(成功应用的版本, dirty 版本)`。
///
/// 迁移表不存在（还没跑过迁移）时返回错误，由调用方判定为「待迁移」。
async fn applied_versions(pool: &PgPool) -> Result<(Vec<i64>, Option<i64>), sqlx::Error> {
    let rows: Vec<(i64, bool)> =
        sqlx::query_as("SELECT version, success FROM _sqlx_migrations ORDER BY version")
            .fetch_all(pool)
            .await?;
    let applied = rows
        .iter()
        .filter(|(_, success)| *success)
        .map(|(version, _)| *version)
        .collect();
    let dirty = rows
        .iter()
        .find(|(_, success)| !*success)
        .map(|(version, _)| *version);
    Ok((applied, dirty))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readiness_without_error_code_is_ready() {
        let readiness = Readiness {
            database_ok: true,
            applied: vec![1],
            pending: vec![],
            dirty: None,
            error_code: None,
            detail: None,
        };
        assert!(readiness.is_ready());
    }

    #[test]
    fn readiness_with_error_code_is_not_ready() {
        let readiness = Readiness {
            database_ok: false,
            applied: vec![],
            pending: vec![1],
            dirty: None,
            error_code: Some(CODE_DB_UNAVAILABLE),
            detail: Some("数据库不可达".to_string()),
        };
        assert!(!readiness.is_ready());
    }

    #[test]
    fn error_codes_are_stable() {
        assert_eq!(CODE_DB_UNAVAILABLE, "DATABASE_UNAVAILABLE");
        assert_eq!(CODE_MIGRATIONS_PENDING, "MIGRATIONS_PENDING");
        assert_eq!(CODE_MIGRATION_DIRTY, "MIGRATION_DIRTY");
    }
}
