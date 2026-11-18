//! 数据库迁移：独立步骤，不在应用启动流程里跑。
//!
//! 迁移由部署流程显式触发（compose 的 `migrate` profile / Helm 的 pre-install hook），
//! 避免多副本并发迁移互相冲突（REQ-005 AC-S-006）。
//!
//! 迁移文件采用 sqlx 的可逆格式：`<版本>_<描述>.up.sql` + `<版本>_<描述>.down.sql`。
//! 没有 `.down.sql` 的迁移视为**不可逆**，`down` 命令会拒绝回滚越过它。

use sqlx::PgPool;
use sqlx::migrate::{MigrateError, Migrator};

/// 迁移命令的错误。
#[derive(Debug, thiserror::Error)]
pub enum MigrationError {
    /// 数据库访问失败。
    #[error("数据库访问失败：{0}")]
    Database(#[from] sqlx::Error),
    /// 迁移执行失败。
    #[error("迁移执行失败：{0}")]
    Migrate(#[from] MigrateError),
    /// 目标版本之上存在不可逆迁移，拒绝回滚。
    #[error("目标版本 {target} 之上存在不可逆迁移 {versions:?}（没有配套 down），拒绝回滚")]
    Irreversible {
        /// 目标版本。
        target: i64,
        /// 不可逆的版本列表。
        versions: Vec<i64>,
    },
}

/// 迁移状态。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationStatus {
    /// 已成功应用的版本。
    pub applied: Vec<i64>,
    /// 待应用的版本。
    pub pending: Vec<i64>,
    /// 不可逆（没有配套 down）的已应用版本。
    pub irreversible: Vec<i64>,
}

/// 执行待应用的迁移（`up`），返回本次新应用的版本。
pub async fn migrate_up(pool: &PgPool, migrator: &Migrator) -> Result<Vec<i64>, MigrationError> {
    let before = applied_versions(pool).await?;
    migrator.run(pool).await?;
    let after = applied_versions(pool).await?;
    Ok(after
        .into_iter()
        .filter(|version| !before.contains(version))
        .collect())
}

/// 回滚到目标版本（`down`）：撤销版本号大于 `target` 的迁移，返回被撤销的版本。
///
/// 目标版本之上存在**不可逆**迁移时拒绝回滚 —— 宁可报错也不做半截数据破坏。
pub async fn migrate_down(
    pool: &PgPool,
    migrator: &Migrator,
    target: i64,
) -> Result<Vec<i64>, MigrationError> {
    let applied = applied_versions(pool).await?;
    let irreversible: Vec<i64> = applied
        .iter()
        .copied()
        .filter(|version| *version > target)
        .filter(|version| !has_down_migration(migrator, *version))
        .collect();

    if !irreversible.is_empty() {
        return Err(MigrationError::Irreversible {
            target,
            versions: irreversible,
        });
    }

    migrator.undo(pool, target).await?;
    let after = applied_versions(pool).await?;
    Ok(applied
        .into_iter()
        .filter(|version| !after.contains(version))
        .collect())
}

/// 迁移状态（`status`）。
pub async fn migration_status(
    pool: &PgPool,
    migrator: &Migrator,
) -> Result<MigrationStatus, MigrationError> {
    let applied = applied_versions(pool).await?;
    let expected: Vec<i64> = migrator
        .iter()
        .filter(|migration| migration.migration_type.is_up_migration())
        .map(|migration| migration.version)
        .collect();
    let pending: Vec<i64> = expected
        .into_iter()
        .filter(|version| !applied.contains(version))
        .collect();
    let irreversible: Vec<i64> = applied
        .iter()
        .copied()
        .filter(|version| !has_down_migration(migrator, *version))
        .collect();
    Ok(MigrationStatus {
        applied,
        pending,
        irreversible,
    })
}

/// 某版本是否有配套的 down 迁移。
fn has_down_migration(migrator: &Migrator, version: i64) -> bool {
    migrator.iter().any(|migration| {
        migration.version == version && migration.migration_type.is_down_migration()
    })
}

/// 读取已应用迁移版本；迁移表不存在时返回空列表。
async fn applied_versions(pool: &PgPool) -> Result<Vec<i64>, MigrationError> {
    let rows: Result<Vec<(i64, bool)>, sqlx::Error> =
        sqlx::query_as("SELECT version, success FROM _sqlx_migrations ORDER BY version")
            .fetch_all(pool)
            .await;
    match rows {
        Ok(rows) => Ok(rows
            .into_iter()
            .filter(|(_, success)| *success)
            .map(|(version, _)| version)
            .collect()),
        // 迁移表不存在 = 还没跑过任何迁移。
        Err(sqlx::Error::Database(err)) if err.code().as_deref() == Some("42P01") => Ok(Vec::new()),
        Err(err) => Err(MigrationError::Database(err)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migration_status_shape_is_stable() {
        let status = MigrationStatus {
            applied: vec![1, 2],
            pending: vec![3],
            irreversible: vec![],
        };
        assert_eq!(status.applied, vec![1, 2]);
        assert_eq!(status.pending, vec![3]);
        assert!(status.irreversible.is_empty());
    }

    #[test]
    fn irreversible_error_names_the_blocking_versions() {
        let err = MigrationError::Irreversible {
            target: 1,
            versions: vec![2, 3],
        };
        let text = err.to_string();
        assert!(text.contains("不可逆"), "text = {text}");
        assert!(text.contains("2"), "text = {text}");
    }
}
