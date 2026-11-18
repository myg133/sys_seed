//! 数据库连接池构造。
//!
//! 连接串非法时**立刻**报错（fail-fast），不让进程带着坏配置跑起来。

use std::str::FromStr;
use std::time::Duration;

use sqlx::PgPool;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};

/// 连接池默认上限。低配机与私有化单机场景够用。
pub const DEFAULT_MAX_CONNECTIONS: u32 = 5;

/// 连接池**获取连接**的超时。
///
/// **与探针超时的关系（改一个必须同时改另一个，否则探针拿不到 503）**：
///
/// | 谁 | 值 | 位置 |
/// |---|---|---|
/// | 连接池 acquire_timeout | **2s** | 本常量 |
/// | K8s readinessProbe.timeoutSeconds | **5s** | `deploy/helm/base-platform/templates/deployment.yaml` |
/// | 镜像 HEALTHCHECK --timeout | **5s** | `Dockerfile` |
///
/// 数据库不可达时，`/health/ready` 要等满 `acquire_timeout` 才返回 **503**
/// （实测约 2s）。探针超时必须**大于**它，否则 kubelet 会先掐断请求，
/// 那个语义清晰的 `503 / DATABASE_UNAVAILABLE` 就永远进不了 K8s 事件。
pub const ACQUIRE_TIMEOUT: Duration = Duration::from_secs(2);

/// 按连接串建连接池。
///
/// 用 `connect_lazy_with`：进程启动不依赖数据库是否已就绪 ——
/// 数据库是否可用由 readiness 探针判定（滚动更新期间流量不会打到未就绪实例）。
///
/// 必须是 `async`：连接池的维护任务需要在 Tokio 运行时里创建。
pub async fn pool(database_url: &str, max_connections: u32) -> Result<PgPool, sqlx::Error> {
    let options = PgConnectOptions::from_str(database_url)?;
    Ok(PgPoolOptions::new()
        .max_connections(max_connections)
        .acquire_timeout(ACQUIRE_TIMEOUT)
        .connect_lazy_with(options))
}

/// 迁移专用连接池：单连接即可（迁移之间用 advisory lock 串行化）。
pub async fn migration_pool(database_url: &str) -> Result<PgPool, sqlx::Error> {
    pool(database_url, 1).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn invalid_url_is_rejected_instead_of_panicking() {
        let err = pool("not-a-postgres-url", 1)
            .await
            .expect_err("非法连接串必须报错");
        assert!(!err.to_string().is_empty());
    }

    #[tokio::test]
    async fn valid_url_builds_a_lazy_pool() {
        let pool = pool("postgres://base:base@localhost:5432/base_dev", 1)
            .await
            .expect("合法连接串应可构造连接池");
        assert_eq!(pool.options().get_max_connections(), 1);
    }

    /// 探针超时（5s）必须大于 acquire_timeout（2s），否则探针先超时，
    /// 拿不到 `503 / DATABASE_UNAVAILABLE`。见 `ACQUIRE_TIMEOUT` 的说明。
    #[test]
    fn acquire_timeout_is_shorter_than_the_probe_timeout() {
        assert!(
            ACQUIRE_TIMEOUT < Duration::from_secs(5),
            "连接池 acquire_timeout 必须小于探针超时（5s）"
        );
    }
}
