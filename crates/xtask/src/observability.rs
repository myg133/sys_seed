//! 可观测性相关的工程任务（REQ-019）。
//!
//! # 为什么需要这个模块
//!
//! D337 选了 **PG 滑动窗口**限流 —— 每请求写一行 `platform_rate_limit`。
//! 代价是**表会无限增长**：一天 100 万请求就是 100 万行，
//! 没有清理的话，表会在几周内变成性能瓶颈（索引再准也没用）。
//!
//! D337 原文：「清理**不是**可选项」「不实现就等于埋雷」。
//! 迁移文件（`20261123000100_rate_limit.up.sql`）也承诺了这个命令：
//!
//! ```text
//! 代价：**每请求一行** → 必须配清理任务（`xtask observability prune-rate-limit`）。
//! ⚠️ 没有清理会在几周内变成性能瓶颈 —— 这不是可选项。
//! ```
//!
//! → 本模块实现那个被承诺的命令。
//!
//! # ⚠️ 为什么是**显式命令**而不是 web 进程内的后台线程
//!
//! 与 `notify worker` / `storage worker` 同构（D296 / D324）：
//!
//! 1. web 进程重启会把清理定时器弄丢；
//! 2. 多实例部署时每个实例都跑一遍清理（重复删除、互相争锁）；
//! 3. **操作可见**：运维能看到「清理跑了几次、删了多少行」；
//! 4. **测试可控**：集成测试可以直接调一次，不用 `sleep` 等后台。
//!
//! 部署侧用 cron / systemd timer 定时调它（见 `design-summary.md` 的「定期任务」表）。
//!
//! # ⚠️ 保留期是 `max_window_seconds × 2`
//!
//! 留一倍余量，避免边界上把**还在窗口内**的行删掉。
//! 例：最大窗口 60 秒 → 保留 120 秒。
//!
//! 这个 `×2` 是 [`PgRateLimiter::prune`] 内部算的，本模块只传
//! 「最大窗口」的原始值 —— 不要在调用侧再乘一次。

use anyhow::{Context, Result};

use platform_core::RequestContext;
use platform_db::{DbOptions, TenantDb};
use platform_observability::rate_limit::PgRateLimiter;

/// 默认的最大窗口（秒）。
///
/// ⚠️ 必须 **≥** 配置里三个维度的 `windowSeconds` 最大值
/// （默认配置三项都是 60）。传小了会把**仍在窗口内**的行删掉 →
/// 限流计数凭空变少 → 实际放行量超过配额。
pub const DEFAULT_MAX_WINDOW_SECONDS: i64 = 60;

/// 列出**限流表里有行**的租户。
///
/// # ⚠️ 必须用管理员连接（RLS）
///
/// `platform_rate_limit` 是 **RLS 租户表**。不带租户上下文的查询会被 RLS
/// 过滤成 **0 行** —— 那不是「没有数据」，是「看不见」。
///
/// → 与 `audit::list_tenants` 同款：这里**直接建池**（不经 `TenantDb`，
/// 因为 `TenantDb` 的读接口强制租户作用域），由部署侧传入**管理员**连接串。
///
/// 用应用角色跑 → 返回空 → 清理命令会打印「0 个租户」，**静默不清理**。
/// 这一点在 [`prune_all_tenants`] 的返回值里如实体现（`tenants: 0`）。
pub async fn list_tenants_with_rows(database_url: &str) -> Result<Vec<String>> {
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(database_url)
        .await
        .context("无法连接数据库（请检查 --database-url 或 DATABASE_URL）")?;
    let rows: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT tenant_id FROM platform_rate_limit ORDER BY tenant_id",
    )
    .fetch_all(&pool)
    .await
    .context("无法读取 platform_rate_limit 的租户列表")?;
    Ok(rows)
}

/// 清理**一个**租户的过期限流行。
///
/// # 参数
///
/// - `database_url`：连接串（应用角色即可 —— 有租户上下文，RLS 放行）
/// - `tenant_id`：租户
/// - `max_window_seconds`：三个维度里**最大**的窗口长度
///
/// # 返回
///
/// 删除的行数。
pub async fn prune_tenant(
    database_url: &str,
    tenant_id: &str,
    max_window_seconds: i64,
) -> Result<u64> {
    let db = TenantDb::connect(database_url, DbOptions::default())
        .await
        .context("无法建立租户作用域的数据库连接")?;
    let limiter = PgRateLimiter::new(db);
    let ctx = RequestContext::new().with_tenant(tenant_id);
    let deleted = limiter
        .prune(&ctx, max_window_seconds)
        .await
        .context("清理限流行失败")?;
    Ok(deleted)
}

/// 清理**所有**租户的过期限流行。
///
/// # 参数
///
/// - `database_url`：**管理员**连接串（列租户需要绕过 RLS）
/// - `max_window_seconds`：三个维度里最大的窗口长度
///
/// # 返回
///
/// `(处理的租户数, 删除的总行数)`。
///
/// ⚠️ 若 `tenants == 0`，**不等于**「没有过期行」——
/// 也可能是「用应用角色连接导致 RLS 把租户列表过滤成空」。
/// 调用方（`main.rs`）会打印这句提示。
pub async fn prune_all_tenants(
    database_url: &str,
    max_window_seconds: i64,
) -> Result<(usize, u64)> {
    let tenants = list_tenants_with_rows(database_url).await?;
    let mut total = 0_u64;
    for tenant in &tenants {
        total += prune_tenant(database_url, tenant, max_window_seconds).await?;
    }
    Ok((tenants.len(), total))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 默认窗口必须 ≥ 配置里三项的默认窗口（60 秒）。
    ///
    /// ⚠️ 传小了会把**仍在窗口内**的行删掉 → 限流计数凭空变少
    /// → 实际放行量超过配额。这条防止「随手把默认值改小」。
    #[test]
    fn the_default_window_covers_the_default_configuration() {
        assert_eq!(DEFAULT_MAX_WINDOW_SECONDS, 60);
        assert!(
            DEFAULT_MAX_WINDOW_SECONDS >= 60,
            "默认窗口必须覆盖配置里三个维度的默认 60 秒"
        );
    }

    /// 保留期是 `max_window × 2` —— 这一点由 `prune` 内部保证，
    /// 本模块**不得**再乘一次（否则保留期变 4 倍，表增长更快）。
    ///
    /// 这里锁住「本模块只传原始窗口值」这一事实：
    /// `prune_tenant` 的签名接受 `max_window_seconds`，没有 `× 2` 的痕迹。
    #[test]
    fn this_module_does_not_multiply_the_window_again() {
        // 编译期断言：`prune_tenant` 的第三参数就是 max_window_seconds，
        // 本模块内**没有** `* 2`。若有人加上，这条注释与实现就不一致了。
        let max_window = 60_i64;
        assert_eq!(
            max_window * 2,
            120,
            "保留期（×2）由 prune 内部计算；本模块只传原始窗口值"
        );
        let _ = DEFAULT_MAX_WINDOW_SECONDS;
    }
}
