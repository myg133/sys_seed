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
    /// 不就绪时的说明。
    ///
    /// 契约 C6（`api-conventions v1`）：`detail` 是**面向开发者的英文描述** ——
    /// 面向用户的中文文案由前端按 `code` 做 i18n 渲染，后端**不得**在这里拼中文句子。
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
            detail: Some(detail_database_unavailable(&err.to_string())),
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
                detail: Some(detail_migration_state_unreadable(&err.to_string())),
            };
        }
    };

    let pending: Vec<i64> = expected_versions(migrator)
        .into_iter()
        .filter(|version| !applied.contains(version))
        .collect();

    let (error_code, detail) = migration_error(&pending, dirty);

    Readiness {
        database_ok: true,
        applied,
        pending,
        dirty,
        error_code,
        detail,
    }
}

/// 迁移状态判定（纯函数，便于单测）。
///
/// 返回 `(错误码, detail)`；一切正常时错误码为 `None`。
fn migration_error(pending: &[i64], dirty: Option<i64>) -> (Option<&'static str>, Option<String>) {
    if let Some(version) = dirty {
        return (
            Some(CODE_MIGRATION_DIRTY),
            Some(detail_migration_dirty(version)),
        );
    }
    if !pending.is_empty() {
        return (
            Some(CODE_MIGRATIONS_PENDING),
            Some(detail_migrations_pending(pending)),
        );
    }
    (None, None)
}

// ---------------------------------------------------------------------------
// detail 文案（契约 C6：必须是面向开发者的**英文**描述）
// ---------------------------------------------------------------------------

/// `DATABASE_UNAVAILABLE` 的 detail：数据库连不上。
fn detail_database_unavailable(err: &str) -> String {
    dev_detail(&format!("database is unreachable: {err}"))
}

/// `MIGRATIONS_PENDING` 的 detail：迁移表读不出来（多半是没跑过迁移）。
fn detail_migration_state_unreadable(err: &str) -> String {
    dev_detail(&format!(
        "migration state is unreadable, migrations may not have run: {err}"
    ))
}

/// `MIGRATIONS_PENDING` 的 detail：还有待应用的迁移。
fn detail_migrations_pending(pending: &[i64]) -> String {
    dev_detail(&format!("pending migrations: {pending:?}"))
}

/// `MIGRATION_DIRTY` 的 detail：迁移执行到一半失败，需人工处理。
fn detail_migration_dirty(version: i64) -> String {
    dev_detail(&format!(
        "migration {version} is in a dirty state and needs manual recovery"
    ))
}

/// 收敛 detail 文案：剔除非 ASCII 字符。
///
/// 契约 C6 要求 `detail` 是英文的开发者描述。数据库服务端消息会随
/// `lc_messages` 本地化，直接拼进 `detail` 就可能漏出中文 ——
/// 这里统一只保留可打印 ASCII，保证 `detail` 里**不可能**出现中文句子。
/// 面向用户的中文文案由前端按 `code` 做 i18n 渲染。
fn dev_detail(raw: &str) -> String {
    let ascii: String = raw
        .chars()
        .filter(|c| c.is_ascii_graphic() || *c == ' ')
        .collect();
    ascii.trim().to_string()
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

    /// 是否含中文/日文/韩文字符（CJK 统一表意文字 + 扩展 A + CJK 标点 + 全角）。
    fn contains_cjk(text: &str) -> bool {
        text.chars().any(|c| {
            matches!(
                c as u32,
                0x4E00..=0x9FFF | 0x3400..=0x4DBF | 0x3000..=0x303F | 0xFF00..=0xFFEF
            )
        })
    }

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
            detail: Some("database is unreachable".to_string()),
        };
        assert!(!readiness.is_ready());
    }

    #[test]
    fn error_codes_are_stable() {
        assert_eq!(CODE_DB_UNAVAILABLE, "DATABASE_UNAVAILABLE");
        assert_eq!(CODE_MIGRATIONS_PENDING, "MIGRATIONS_PENDING");
        assert_eq!(CODE_MIGRATION_DIRTY, "MIGRATION_DIRTY");
    }

    /// 契约 C6：`detail` 是面向开发者的**英文**描述，后端不得拼中文句子
    /// （面向用户的中文文案由前端按 `code` 做 i18n 渲染）。
    ///
    /// 三个 readiness 错误路径（`DATABASE_UNAVAILABLE` / `MIGRATIONS_PENDING` /
    /// `MIGRATION_DIRTY`）的 `detail` 必须全部不含中文字符。
    #[test]
    fn readiness_detail_is_english_on_every_error_path() {
        let details = [
            detail_database_unavailable("pool timed out while waiting for an open connection"),
            detail_migration_state_unreadable("relation \"_sqlx_migrations\" does not exist"),
            detail_migrations_pending(&[20261118000100]),
            detail_migration_dirty(20261118000100),
        ];

        for detail in &details {
            assert!(detail.is_ascii(), "detail 必须是 ASCII：{detail}");
            assert!(!contains_cjk(detail), "detail 不得含中文字符：{detail}");
        }
    }

    #[test]
    fn dirty_migration_error_carries_english_detail() {
        let (code, detail) = migration_error(&[], Some(20261118000100));
        assert_eq!(code, Some(CODE_MIGRATION_DIRTY));
        let detail = detail.expect("dirty 必须带 detail");
        assert!(
            detail.contains("20261118000100"),
            "detail 要点名版本：{detail}"
        );
        assert!(detail.contains("dirty"), "detail 要说明 dirty：{detail}");
        assert!(!contains_cjk(&detail));
    }

    #[test]
    fn pending_migrations_error_carries_english_detail() {
        let (code, detail) = migration_error(&[20261118000100], None);
        assert_eq!(code, Some(CODE_MIGRATIONS_PENDING));
        let detail = detail.expect("有待应用迁移时必须带 detail");
        assert!(
            detail.contains("20261118000100"),
            "detail 要列出待应用版本：{detail}"
        );
        assert!(!contains_cjk(&detail));
    }

    #[test]
    fn healthy_database_has_no_error_and_no_detail() {
        let (code, detail) = migration_error(&[], None);
        assert_eq!(code, None);
        assert_eq!(detail, None);
    }

    /// 数据库服务端消息可能按 `lc_messages` 本地化 —— 即便如此，
    /// 中文也不许漏进 `detail`。
    #[test]
    fn dev_detail_strips_non_ascii_from_database_errors() {
        let detail = dev_detail("database is unreachable: 数据库不可达（连接超时）");
        assert!(detail.is_ascii(), "detail 必须是 ASCII：{detail}");
        assert!(!contains_cjk(&detail), "detail 不得含中文字符：{detail}");
        assert!(
            detail.starts_with("database is unreachable:"),
            "英文描述必须保留：{detail}"
        );
    }
}
