//! `cargo xtask audit` —— 审计链校验与归档（REQ-014 AC-S-022 / AC-S-131）。
//!
//! ```bash
//! # 校验某个租户的审计链
//! DATABASE_URL=postgres://base_app:base@host/base cargo xtask audit verify --tenant tenant-a
//!
//! # 校验**全部**租户（需要能看见所有租户的行 —— 管理员连接）
//! DATABASE_URL=postgres://base:base@host/base cargo xtask audit verify
//!
//! # 归档某个租户的超期记录
//! DATABASE_URL=postgres://base_app:base@host/base cargo xtask audit archive --tenant tenant-a
//! ```
//!
//! # 为什么归档**必须**用应用角色（D158）
//!
//! 审计表是 **append-only**（应用角色**没有** DELETE 权限）→ 归档只能走
//! 受限的 `SECURITY DEFINER` 函数，而那个函数**绕过 RLS** → 必须由**调用方**
//! 把作用域限定住（函数内有**显式租户守卫**，D174）。
//!
//! → 用**应用角色**连接（受 RLS 约束）→ 逐租户调用（D158）。
//! 跨租户批量归档**不在本轮**（要跨租户特权 + 独立权限点）。
//!
//! # 为什么「定期」靠**外部调度器**（D164）
//!
//! 本仓**没有**调度器基础设施（不引入常驻进程）→ 「定期校验」由外部定时任务
//! 调用这两个入口实现。

use anyhow::{Context, Result, bail};
use platform_audit::{ArchiveService, ChainVerifier};
use platform_core::RequestContext;
use platform_db::{DbOptions, TenantDb};

/// 一次 `verify` 的结果（供命令行打印）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifyOutcome {
    /// 租户。
    pub tenant_id: String,
    /// 链是否完好。
    pub ok: bool,
    /// 检查了多少条。
    pub checked: usize,
    /// 断裂条数。
    pub broken: usize,
}

/// 校验**一个**租户的审计链。
///
/// 连接必须是**该租户**可见的连接（应用角色 + RLS → 只能看见本租户的行）。
pub async fn verify_tenant(database_url: &str, tenant_id: &str) -> Result<VerifyOutcome> {
    let db = TenantDb::connect(database_url, DbOptions::default())
        .await
        .context("cannot connect to the database")?;
    let ctx = RequestContext::new().with_tenant(tenant_id);
    let report = ChainVerifier::new(db)
        .verify(&ctx)
        .await
        .context("the audit chain could not be verified")?;
    Ok(VerifyOutcome {
        tenant_id: tenant_id.to_string(),
        ok: report.ok,
        checked: report.checked,
        broken: report.broken.len(),
    })
}

/// 列出**有审计链状态**的租户。
///
/// ⚠️ 这条查询**不带**租户上下文 → RLS 会让它返回 **0 行**
/// （除非连接角色绕过 RLS —— 管理员连接）。
/// → 「校验全部租户」需要**管理员连接**；用应用角色跑只会看到 0 个租户
/// （**不是**「没有审计数据」）→ 这一点必须让操作者知道。
pub async fn list_tenants(database_url: &str) -> Result<Vec<String>> {
    // ⚠️ 直接建连接池（**不**用 `TenantDb`）：`TenantDb` 的读接口**强制**带租户
    // 作用域 → 这条查询必须是「不带租户上下文」的（否则 RLS 恒返回 0 行）。
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(database_url)
        .await
        .context("cannot connect to the database")?;
    let rows: Vec<String> =
        sqlx::query_scalar("SELECT tenant_id FROM platform_audit_chain_state ORDER BY tenant_id")
            .fetch_all(&pool)
            .await
            .context("cannot read the audit chain state table")?;
    Ok(rows)
}

/// 归档**一个**租户的超期记录。
///
/// ⚠️ **单租户**（D158）：作用域由调用方的租户上下文 + 函数内的守卫限定。
pub async fn archive_tenant(database_url: &str, tenant_id: &str, online_days: i64) -> Result<i64> {
    let db = TenantDb::connect(database_url, DbOptions::default())
        .await
        .context("cannot connect to the database")?;
    let ctx = RequestContext::new().with_tenant(tenant_id);
    let result = ArchiveService::new(db)
        .archive_older_than(&ctx, online_days)
        .await
        .context("the audit archive could not be executed")?;
    Ok(result.moved)
}

/// 校验结果的**机器可读**汇总（打印用）。
pub fn summarize(outcomes: &[VerifyOutcome]) -> String {
    let broken: Vec<&VerifyOutcome> = outcomes.iter().filter(|item| !item.ok).collect();
    let checked: usize = outcomes.iter().map(|item| item.checked).sum();
    if broken.is_empty() {
        format!(
            "审计链校验通过：{} 个租户，共 {} 条记录",
            outcomes.len(),
            checked
        )
    } else {
        let detail = broken
            .iter()
            .map(|item| format!("{}（{} 条断裂）", item.tenant_id, item.broken))
            .collect::<Vec<_>>()
            .join("、");
        format!("审计链校验失败：{detail}")
    }
}

/// 校验结果里**有断裂**时 → 失败退出码。
pub fn has_failures(outcomes: &[VerifyOutcome]) -> bool {
    outcomes.iter().any(|item| !item.ok)
}

/// 参数校验：`--tenant` 不能是空串。
pub fn require_tenant(tenant: Option<&str>) -> Result<String> {
    let tenant = tenant
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow::anyhow!("缺少 --tenant：审计操作是**单租户**的（D158）"))?;
    Ok(tenant.to_string())
}

/// 归档天数必须是正数。
pub fn require_positive_days(days: i64) -> Result<i64> {
    if days <= 0 {
        bail!("--days 必须是正整数（当前：{days}）");
    }
    Ok(days)
}
