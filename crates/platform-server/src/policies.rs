//! 部署期的租户作用域策略生成（`base tenant-policies`，REQ-006）。
//!
//! # 为什么要有这个命令（`cargo xtask tenant-policies` 已经有了）
//!
//! 交付环境**没有 cargo**：私有化部署只拿到镜像 + compose / Helm，
//! 客户机上跑不了 `cargo xtask`。但「租户作用域表必须有 RLS policy」
//! 这件事必须在部署期能落地 —— **漏建一张表 = 该表完全无隔离**（不是「少一个条件」，
//! 是「谁都能查」）。
//!
//! 所以平台把同一套逻辑做成两个入口，读的是**同一份** manifest：
//!
//! | 入口 | 场景 |
//! |------|------|
//! | `cargo xtask tenant-policies` | 开发期：本地生成 / 校验 |
//! | `base tenant-policies` | 部署期：镜像里没有 cargo，照样能生成 |
//!
//! 收集逻辑在 `platform-db`（`collect_scopes`），策略 SQL 在
//! `platform-db`（`apply_policies`）—— 两边**不会**漂移。
//!
//! # 顺序
//!
//! **必须先跑迁移（建表）再跑策略（建策略）**：策略是对**已存在的表**做的
//! `ALTER TABLE` + `CREATE POLICY`，表还不存在时必然失败。
//! 部署流程：迁移 → 策略 → 滚应用。

use std::path::Path;

use anyhow::{Context, Result, bail};
use platform_db::{DbOptions, TenantDb, apply_scopes, check_scopes, collect_scopes};

/// 执行策略生成 / 校验。
///
/// - `check_only = false`：生成 / 补齐策略（幂等，可重复执行）。
/// - `check_only = true`：只校验；前提不成立时**失败**（调用方据此让部署失败）。
///
/// ⚠️ 需要**表的属主权限**（`ALTER TABLE` / `CREATE POLICY`）——
/// 用管理员（迁移）角色执行；用应用角色跑会报「must be owner of table」。
pub async fn run(manifest_root: &str, database_url: &str, check_only: bool) -> Result<()> {
    let scopes = collect_scopes(Path::new(manifest_root))?;
    if scopes.is_empty() {
        println!("没有模块声明 tenantScoped：无需生成策略");
        return Ok(());
    }

    let db = TenantDb::connect(database_url, DbOptions::default())
        .await
        .context("cannot connect to the database")?;

    if check_only {
        return check(&db, &scopes).await;
    }

    apply_scopes(&db, &scopes)
        .await
        .context("cannot apply the tenant isolation policies")?;
    println!("已为 {} 个模块生成 RLS 策略：", scopes.len());
    for scope in &scopes {
        println!("  ✓ {}：{}", scope.module, scope.spec.tables.join(", "));
    }
    Ok(())
}

/// 校验：每张声明的表都必须**真的有** policy。
///
/// 连接角色绕过 RLS 时**只警告不失败**：运维通常用管理员角色跑这条检查，
/// 而「应用连接用的是什么角色」由启动自检（`require_isolation`）负责。
async fn check(db: &TenantDb, scopes: &[platform_db::TenantScope]) -> Result<()> {
    let report = check_scopes(db, scopes)
        .await
        .context("cannot verify the tenant isolation policies")?;

    let problems = report.table_problems();
    if !problems.is_empty() {
        bail!(
            "租户隔离前提不成立：\n{}",
            problems
                .iter()
                .map(|problem| format!("  - {problem}"))
                .collect::<Vec<_>>()
                .join("\n")
        );
    }

    if report.role_bypasses_rls {
        println!(
            "警告：当前连接角色 `{}` 绕过 RLS（超级用户或 BYPASSRLS）。\
             如果**应用**也用这个角色连接，租户隔离会 fail open —— \
             请用应用角色（组 base_platform_app 的成员）再自检一次。",
            report.role
        );
    }
    println!(
        "租户隔离前提校验通过：{} 个模块声明的表都开了 RLS 且有策略",
        scopes.len()
    );
    Ok(())
}
