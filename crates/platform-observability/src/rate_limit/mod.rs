//! 三级限流：**PG 滑动窗口**（REQ-019，D337）。
//!
//! # 为什么是滑动窗口
//!
//! 固定窗口（`window_start` 对齐 + 计数列）在**窗口边界**会出现**双倍突发**：
//! 窗口 A 的最后 1 毫秒打满配额、窗口 B 的第 1 毫秒又打满 →
//! 实际瞬时速率是配额的 **2 倍**。滑动窗口没有这个问题，
//! 代价是**每请求一行**（需要定期清理，见 [`prune`]）。
//!
//! # ⚠️ 三个桶的作用域（D337 —— 这是 AC-S-031 判据设计的基础）
//!
//! 表是 **RLS 租户表**，`tenant_id` 是列，查询恒带 `WHERE tenant_id = {tenant}`
//! → **三个维度的桶都是「租户内」的**：
//!
//! | 维度 | 谁共享这个桶 |
//! |---|---|
//! | `tenant` | 该租户的**所有**请求 |
//! | `user` | 该租户内**某个用户** |
//! | `endpoint` | 该租户内**某个端点的所有用户**（⚠️ 键里**没有** user 段） |
//!
//! ⚠️ 因此「同租户内 user-b 不受 user-a 的 endpoint 限流影响」是**错的** ——
//! 跨用户隔离要靠**跨租户**（`tenant-b` 的桶完全独立）。
//!
//! # ⚠️ 判定与记账的顺序（不可换）
//!
//! **先判定、后记账**。若先写行再判定，「超限的那次请求」也会被计入 →
//! 配额实际少 1 次。判据：上限 N 时，**第 N 次**放行、**第 N+1 次**才拒。

pub mod middleware;


use platform_core::RequestContext;
use platform_db::TenantDb;

use crate::error::{ObservabilityError, dimension};

/// 限流计数表（**租户级**，RLS）。
pub const RATE_LIMIT_TABLE: &str = "platform_rate_limit";

/// 一个限流维度在某次请求下的一次判定。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowLimit {
    /// 窗口内允许的请求数（**0 = 禁用该维度**，不是「允许 0 次」）。
    pub requests: i64,
    /// 窗口长度（秒）。
    pub window_seconds: i64,
}

impl WindowLimit {
    /// 该维度是否**启用**（`requests > 0`）。
    ///
    /// ⚠️ `requests = 0` 表示**禁用**，不是「允许 0 次」（D344）。
    /// 两者语义相反：前者不限流，后者拒绝一切请求。
    pub fn enabled(&self) -> bool {
        self.requests > 0
    }
}

/// 三个维度的限流配置。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RateLimitSettings {
    /// 总开关（**默认 false**，D344 —— 打开会改变所有现有端点的行为）。
    pub enabled: bool,
    /// 租户维度。
    pub tenant: WindowLimit,
    /// 用户维度。
    pub user: WindowLimit,
    /// 接口维度。
    pub endpoint: WindowLimit,
}

impl RateLimitSettings {
    /// 全关（`enabled = false`，三个维度都禁用）。
    ///
    /// ⚠️ 这是**默认值**：现有 1496 个测试的数据库里没有 `rateLimit.*` 这些键
    /// → 读到 `None` → 降级到这个默认值 → **中间件直接放行** → 测试不受影响（D344）。
    pub const fn disabled() -> Self {
        Self {
            enabled: false,
            tenant: WindowLimit {
                requests: 0,
                window_seconds: 60,
            },
            user: WindowLimit {
                requests: 0,
                window_seconds: 60,
            },
            endpoint: WindowLimit {
                requests: 0,
                window_seconds: 60,
            },
        }
    }

    /// 按维度取配置。
    pub fn limit_of(&self, dimension: &str) -> WindowLimit {
        match dimension {
            dimension::TENANT => self.tenant,
            dimension::USER => self.user,
            dimension::ENDPOINT => self.endpoint,
            // 未知维度 → 禁用（fail open，不 panic）。
            _ => WindowLimit {
                requests: 0,
                window_seconds: 60,
            },
        }
    }
}

/// 生成某个维度的桶键（D337）。
///
/// # ⚠️ 三个维度必须用独立前缀
///
/// 否则会串桶（同一个 key 同时被租户维度与用户维度消费）。
///
/// # ⚠️ `endpoint` 用**路径模板**，不是完整 URI
///
/// `GET /api/rbac/roles?x=1` 与 `?x=2` 必须**共用**一个桶 ——
/// 否则加个随机 query 就能无限打（AC-S-031 步骤 5 专门测这个）。
/// 路径模板由 axum 的 `MatchedPath` 提供（已实测：`?x=1` → `/api/rbac/roles`）。
///
/// # ⚠️ `endpoint` 维度的键**必须含 HTTP 方法**（D337 / AC-S-033）
///
/// D337 规定的形态是 `rl:endpoint:<METHOD>:<路径模板>`，
/// 例 `rl:endpoint:GET:/api/rbac/roles`。
///
/// **为什么方法必须进键**：同一路径常常挂多个方法
/// （`GET /api/rbac/roles` 列表、`POST /api/rbac/roles` 新建）。
/// 只按路径做键 → 写请求先到先得会把读请求的配额挤掉，
/// 与「按接口限流」的语义不符。
///
/// # 参数
///
/// - `method`：HTTP 方法（大写，如 `GET`）。**只有 `endpoint` 维度用得上**，
///   另两个维度忽略它（它们的桶与具体接口无关）。
pub fn bucket_key(
    dimension: &str,
    tenant_id: &str,
    user_id: Option<&str>,
    method: &str,
    endpoint: &str,
) -> String {
    match dimension {
        dimension::TENANT => format!("rl:tenant:{tenant_id}"),
        dimension::USER => format!("rl:user:{tenant_id}:{}", user_id.unwrap_or("(anonymous)")),
        // ⚠️ **不含 user 段** —— endpoint 桶是租户内跨用户共享的（D337）。
        // ⚠️ **含 METHOD** —— 同一路径的不同方法必须是不同的桶（F13）。
        dimension::ENDPOINT => format!("rl:endpoint:{method}:{endpoint}"),
        other => format!("rl:{other}:{tenant_id}:{endpoint}"),
    }
}

/// PG 滑动窗口限流器。
///
/// ⚠️ 每个实例持有一个 `TenantDb`（内含连接池）。
/// 多个实例共享**同一个数据库** → 计数天然一致（AC-S-033 验这个）。
pub struct PgRateLimiter {
    db: TenantDb,
}

impl PgRateLimiter {
    /// 用租户作用域入口构造。
    pub fn new(db: TenantDb) -> Self {
        Self { db }
    }

    /// 判定一个维度是否放行；放行则**记账**。
    ///
    /// # 返回
    ///
    /// - `Ok(())`：放行（已写入一行）
    /// - `Err(RateLimited)`：拒绝（**不写行**）
    /// - `Err(RateLimitUnavailable)`：存储不可用（调用方应 **fail open**，不拦业务）
    ///
    /// # ⚠️ 顺序：先判定、后记账
    ///
    /// 见模块文档。
    pub async fn check(
        &self,
        ctx: &RequestContext,
        dimension_name: &str,
        key: &str,
        limit: WindowLimit,
    ) -> Result<(), ObservabilityError> {
        if !limit.enabled() {
            // 该维度禁用 → 不查库、不记账、放行。
            return Ok(());
        }

        // ① 判定：数窗口内的行，并算出「最早一条过期还要多久」。
        //
        // 一次查询同时拿 count 与 retry_after —— 避免两次往返。
        // ⚠️ `{tenant}` 恰好出现一次（`TenantQuery` 的硬约束）。
        let query = platform_db::TenantQuery::new(format!(
            "SELECT count(*) AS n, \
                    COALESCE(EXTRACT(EPOCH FROM (MIN(occurred_at) + (? * interval '1 second') - now())), 0)::bigint AS retry_after \
               FROM {table} \
              WHERE tenant_id = {{tenant}} AND dimension = ? AND key = ? \
                AND occurred_at > now() - (? * interval '1 second')",
            table = self.db.table(RATE_LIMIT_TABLE)
        ))
        .bind(limit.window_seconds)
        .bind(dimension_name.to_string())
        .bind(key.to_string())
        .bind(limit.window_seconds);

        #[derive(Debug)]
        struct WindowRow {
            n: i64,
            retry_after: i64,
        }
        platform_db::tenant_row!(WindowRow {
            n: "n",
            retry_after: "retry_after",
        });

        let row: WindowRow = match self.db.fetch_one(ctx, &query).await {
            Ok(row) => row,
            Err(error) => {
                // ⚠️ 存储不可用 → 返回可辨识的错误，**由调用方决定 fail open**。
                // 中间件会放行并打 WARN（不拦业务）；就绪探针会返回 503（AC-S-038）。
                return Err(ObservabilityError::RateLimitUnavailable {
                    reason: format!("the rate limit window query failed: {error}"),
                });
            }
        };

        if row.n >= limit.requests {
            // ② 超限 → **不记账**（否则配额会越用越少）。
            //
            // ⚠️ `retry_after` 必须是**正整数**（AC-S-032 步骤 1）：
            // 最旧一条过期时就会释放 1 个名额，所以等待时间 = 它剩余的寿命。
            // 向上取整 + 至少 1 秒（不能是 0 或负数）。
            let retry_after = row.retry_after.max(1);
            return Err(ObservabilityError::RateLimited {
                dimension: dimension_name.to_string(),
                retry_after,
            });
        }

        // ③ 放行 → 记账。
        let insert = platform_db::TenantQuery::new(format!(
            "INSERT INTO {table} (tenant_id, dimension, key, occurred_at) \
             VALUES ({{tenant}}, ?, ?, now())",
            table = self.db.table(RATE_LIMIT_TABLE)
        ))
        .bind(dimension_name.to_string())
        .bind(key.to_string());

        if let Err(error) = self.db.execute(ctx, &insert).await {
            // 记账失败 → 同样按「存储不可用」处理（不拦业务）。
            return Err(ObservabilityError::RateLimitUnavailable {
                reason: format!("the rate limit insert failed: {error}"),
            });
        }

        Ok(())
    }

    /// 清理过期行（**必做，D337**）。
    ///
    /// # ⚠️ 不实现清理就是埋雷
    ///
    /// 每请求一行 → 一天 100 万请求就是 100 万行。没有清理，
    /// 表会在几周内变成性能瓶颈（索引再准也没用）。
    ///
    /// 保留期 = `max_window_seconds × 2`（留一倍余量，
    /// 避免边界上把还在窗口内的行删掉）。
    ///
    /// # 返回
    ///
    /// 删除的行数。
    pub async fn prune(
        &self,
        ctx: &RequestContext,
        max_window_seconds: i64,
    ) -> Result<u64, ObservabilityError> {
        // ⚠️ 至少保留 2 倍窗口。
        //
        // ⚠️ **QA 复查修正（注释与代码不一致）**：原注释写
        // 「若传入 0 或负数，用 60 兜底（防误删全表）」——
        // 而代码是 `.max(1) * 2`，传 0 得到的是**保留 2 秒**，不是 60 秒。
        // QA 实跑 CLI `--max-window-seconds 0` 输出「保留期 0 秒」，与注释矛盾。
        //
        // → 现在**代码按注释实现**：0 或负数一律用 60 秒兜底。
        // 保留期 = 兜底值 × 2 = 120 秒（与「窗口 60 × 2」一致）。
        //
        // # 为什么兜底是 60 而不是 1
        //
        // 传 0 通常意味着「调用方算错了」，不该静默变成「几乎不保留」——
        // 那会把**仍在窗口内**的行删掉 → 限流计数凭空变少
        // → 实际放行量超过配额（比不清理更危险：它是**静默失效**）。
        const FALLBACK_MAX_WINDOW_SECONDS: i64 = 60;
        let window = if max_window_seconds <= 0 {
            FALLBACK_MAX_WINDOW_SECONDS
        } else {
            max_window_seconds
        };
        let keep = window * 2;
        let query = platform_db::TenantQuery::new(format!(
            "DELETE FROM {table} \
              WHERE tenant_id = {{tenant}} \
                AND occurred_at < now() - (? * interval '1 second')",
            table = self.db.table(RATE_LIMIT_TABLE)
        ))
        .bind(keep);

        self.db
            .execute(ctx, &query)
            .await
            .map_err(|error| ObservabilityError::RateLimitUnavailable {
                reason: format!("the rate limit prune failed: {error}"),
            })
    }

    /// 数某维度某键在窗口内的行数（**测试与就绪探针用**）。
    ///
    /// ⚠️ AC-S-033 步骤 3 的核心判据靠它：DB 里**真有行**才能区分
    /// 「PG 共享」与「进程内全局 static」（后者在 DB 里零行）。
    pub async fn count_in_window(
        &self,
        ctx: &RequestContext,
        dimension_name: &str,
        key: &str,
        window_seconds: i64,
    ) -> Result<i64, ObservabilityError> {
        let query = platform_db::TenantQuery::new(format!(
            "SELECT count(*) AS n FROM {table} \
              WHERE tenant_id = {{tenant}} AND dimension = ? AND key = ? \
                AND occurred_at > now() - (? * interval '1 second')",
            table = self.db.table(RATE_LIMIT_TABLE)
        ))
        .bind(dimension_name.to_string())
        .bind(key.to_string())
        .bind(window_seconds);

        #[derive(Debug)]
        struct CountRow {
            n: i64,
        }
        platform_db::tenant_row!(CountRow { n: "n" });

        self.db
            .fetch_one(ctx, &query)
            .await
            .map(|row: CountRow| row.n)
            .map_err(|error| ObservabilityError::RateLimitUnavailable {
                reason: format!("the rate limit count failed: {error}"),
            })
    }

    /// 就绪检查：能不能读限流表（AC-S-038）。
    ///
    /// ⚠️ **不写行**（就绪探针不能污染限流计数）。
    pub async fn is_reachable(&self, ctx: &RequestContext) -> bool {
        let query = platform_db::TenantQuery::new(format!(
            "SELECT count(*) AS n FROM {table} WHERE tenant_id = {{tenant}}",
            table = self.db.table(RATE_LIMIT_TABLE)
        ));
        #[derive(Debug)]
        struct CountRow {
            n: i64,
        }
        platform_db::tenant_row!(CountRow { n: "n" });
        // ⚠️ 显式标注类型 —— `fetch_one` 是泛型，不标注无法推断 `from_row` 的实现者。
        //
        // ⚠️ 只关心「查得通」，`n` 的值本身无所谓 —— 但**不能**让字段变成
        // `dead_code`（那会产生构建警告，`lib.rs` 声明了 `warn(clippy::pedantic)`）。
        // → 显式读一次，语义上说明「查到了就是可达」。
        self.db
            .fetch_one::<CountRow>(ctx, &query)
            .await
            .map(|row| row.n)
            .is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `requests = 0` 是**禁用**，不是「允许 0 次」（D344）。
    #[test]
    fn zero_requests_means_disabled_not_deny_all() {
        let limit = WindowLimit {
            requests: 0,
            window_seconds: 60,
        };
        assert!(!limit.enabled(), "requests=0 必须是「禁用」而不是「拒绝一切」");
    }

    /// 正数才是启用。
    #[test]
    fn a_positive_request_count_is_enabled() {
        assert!(
            WindowLimit {
                requests: 1,
                window_seconds: 60
            }
            .enabled()
        );
    }

    /// 默认配置是全关（保护现有 1496 个测试）。
    #[test]
    fn the_default_settings_are_fully_disabled() {
        let s = RateLimitSettings::disabled();
        assert!(!s.enabled);
        assert!(!s.tenant.enabled());
        assert!(!s.user.enabled());
        assert!(!s.endpoint.enabled());
    }

    /// 三个维度的键前缀**互不相同**（防串桶）。
    #[test]
    fn the_three_dimensions_use_distinct_prefixes() {
        let t = bucket_key(dimension::TENANT, "tenant-a", Some("u1"), "GET", "/x");
        let u = bucket_key(dimension::USER, "tenant-a", Some("u1"), "GET", "/x");
        let e = bucket_key(dimension::ENDPOINT, "tenant-a", Some("u1"), "GET", "/x");
        assert!(t.starts_with("rl:tenant:"));
        assert!(u.starts_with("rl:user:"));
        assert!(e.starts_with("rl:endpoint:"));
        assert_ne!(t, u);
        assert_ne!(u, e);
    }

    /// **endpoint 键不含 user 段**（D337 —— AC-S-031 步骤 4 的基础）。
    ///
    /// ⚠️ 这条测试记录了「endpoint 桶是租户内跨用户共享」这个语义。
    /// 如果实现把 user_id 拼进去，这条会红。
    #[test]
    fn the_endpoint_key_does_not_contain_the_user() {
        let a = bucket_key(dimension::ENDPOINT, "tenant-a", Some("user-a"), "GET", "/x");
        let b = bucket_key(dimension::ENDPOINT, "tenant-a", Some("user-b"), "GET", "/x");
        assert_eq!(
            a, b,
            "同一个租户的同一端点必须是**同一个桶**（不分用户）；若不同说明键设计错了"
        );
        assert!(
            !a.contains("user-a"),
            "endpoint 键里不得出现 user_id：{a}"
        );
    }

    /// **endpoint 键只用路径**，query 不进键（AC-S-031 步骤 5）。
    #[test]
    fn the_endpoint_key_uses_the_path_template_only() {
        // `MatchedPath` 给的就是不带 query 的模板。
        let with = bucket_key(
            dimension::ENDPOINT,
            "tenant-a",
            None,
            "GET",
            "/api/rbac/roles",
        );
        // ⚠️ 形态必须与 D337 逐字一致：`rl:endpoint:<METHOD>:<路径模板>`。
        assert_eq!(with, "rl:endpoint:GET:/api/rbac/roles");
        assert!(!with.contains('?'), "键里不得含 query string");
    }

    /// **endpoint 键必须含 HTTP 方法**（D337 / AC-S-033 / F13）。
    ///
    /// ⚠️ 这条是 F13 的判据：同一路径挂 `GET` 与 `POST` 时，
    /// 它们的配额**必须分开** —— 否则写请求会挤掉读请求的配额。
    /// 若实现退回 `rl:endpoint:<路径>`（不含方法），这条会红。
    #[test]
    fn the_endpoint_key_differs_per_http_method() {
        let get = bucket_key(dimension::ENDPOINT, "tenant-a", None, "GET", "/api/apps");
        let post = bucket_key(dimension::ENDPOINT, "tenant-a", None, "POST", "/api/apps");
        let delete = bucket_key(dimension::ENDPOINT, "tenant-a", None, "DELETE", "/api/apps");
        assert_ne!(get, post, "GET 与 POST 必须是不同的桶（不能共享配额）");
        assert_ne!(get, delete, "GET 与 DELETE 必须是不同的桶");
        assert_ne!(post, delete);
        // 键里逐字含方法名（AC-S-033 步骤 3 要求 `key` 含 `GET`）。
        assert!(get.contains("GET"), "endpoint 键必须含方法名：{get}");
        assert_eq!(get, "rl:endpoint:GET:/api/apps");
    }

    /// 另两个维度的键**不受** method 影响（它们的桶与具体接口无关）。
    #[test]
    fn the_tenant_and_user_keys_ignore_the_method() {
        let t_get = bucket_key(dimension::TENANT, "tenant-a", None, "GET", "/x");
        let t_post = bucket_key(dimension::TENANT, "tenant-a", None, "POST", "/x");
        assert_eq!(t_get, t_post, "租户维度的桶与接口无关");
        let u_get = bucket_key(dimension::USER, "tenant-a", Some("user-a"), "GET", "/x");
        let u_post = bucket_key(dimension::USER, "tenant-a", Some("user-a"), "POST", "/x");
        assert_eq!(u_get, u_post, "用户维度的桶与接口无关");
    }

    /// user 键**必须**含 user_id（不同用户不同桶）。
    #[test]
    fn the_user_key_differs_per_user() {
        let a = bucket_key(dimension::USER, "tenant-a", Some("user-a"), "GET", "/x");
        let b = bucket_key(dimension::USER, "tenant-a", Some("user-b"), "GET", "/x");
        assert_ne!(a, b, "不同用户必须是不同桶");
        assert!(a.contains("user-a"));
    }

    /// **跨租户**的桶必须不同（AC-S-031 步骤 4 的基础）。
    #[test]
    fn the_same_endpoint_in_another_tenant_is_another_bucket() {
        let a = bucket_key(dimension::ENDPOINT, "tenant-a", None, "GET", "/x");
        let b = bucket_key(dimension::ENDPOINT, "tenant-b", None, "GET", "/x");
        // ⚠️ endpoint 键**不含** tenant_id，但表有 tenant_id 列 →
        // 实际隔离靠 RLS 的 WHERE。这里断言的是「键本身相同」，
        // 隔离由 DB 层保证（见 rate_limit_real_db 的跨租户用例）。
        assert_eq!(
            a, b,
            "endpoint 键不含 tenant_id（隔离靠 RLS 的 tenant_id 列）"
        );
    }

    /// 未知维度 → 取到的是禁用配置（fail open，不 panic）。
    #[test]
    fn an_unknown_dimension_falls_back_to_disabled() {
        let s = RateLimitSettings::disabled();
        assert!(!s.limit_of("nonsense").enabled());
    }
}
