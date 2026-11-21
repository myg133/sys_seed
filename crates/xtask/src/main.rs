//! 子命令分派。

use std::process::ExitCode;

use anyhow::Result;
use clap::Parser;
use xtask::capabilities;
use xtask::cli::{Cli, Command};
use xtask::deps::dependency_direction_violations;
use xtask::modules::check_modules;
use xtask::scaffold::create_module;
use xtask::tenant_policies;

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Command::CheckModules(args) => match run_check_modules(std::path::Path::new(&args.root)) {
            Ok(code) => code,
            Err(err) => fail(err),
        },
        Command::CheckDeps(args) => match run_check_deps(std::path::Path::new(&args.root)) {
            Ok(code) => code,
            Err(err) => fail(err),
        },
        Command::Capabilities(args) => match run_capabilities(&args) {
            Ok(code) => code,
            Err(err) => fail(err),
        },
        Command::CreateModule(args) => match run_create_module(&args) {
            Ok(code) => code,
            Err(err) => fail(err),
        },
        Command::TenantPolicies(args) => match run_tenant_policies(&args) {
            Ok(code) => code,
            Err(err) => fail(err),
        },
        Command::Audit(args) => match run_audit(&args) {
            Ok(code) => code,
            Err(err) => fail(err),
        },
        Command::Notify(args) => match run_notify(&args) {
            Ok(code) => code,
            Err(err) => fail(err),
        },
    }
}

/// 执行通知投递批处理（REQ-015 / D296）。
fn run_notify(args: &xtask::cli::NotifyArgs) -> Result<ExitCode> {
    let runtime = tokio::runtime::Runtime::new()?;
    match &args.action {
        xtask::cli::NotifyAction::Worker(worker) => {
            let database_url = database_url_of(worker.database_url.clone())?;
            let (service, registry) = runtime.block_on(xtask::notify::build_service(
                &database_url,
                &worker.tenant,
            ))?;
            let ctx = platform_core::RequestContext::new().with_tenant(worker.tenant.clone());
            let outcomes = runtime.block_on(xtask::notify::run_rounds(
                &service,
                &ctx,
                &registry,
                worker.batch,
                worker.rounds,
            ))?;
            for (index, outcome) in outcomes.iter().enumerate() {
                println!("  第 {} 轮：{}", index + 1, outcome.summary());
            }
            let total_claimed: usize = outcomes.iter().map(|o| o.claimed).sum();
            println!(
                "共 {} 轮，认领 {} 条{}",
                outcomes.len(),
                total_claimed,
                if total_claimed == 0 {
                    "（队列为空）"
                } else {
                    ""
                }
            );
            Ok(ExitCode::SUCCESS)
        }
    }
}

/// 统一的错误出口：打印错误链并返回失败退出码。
fn fail(err: anyhow::Error) -> ExitCode {
    eprintln!("错误：{err:#}");
    ExitCode::FAILURE
}

/// 执行目录规范校验。
///
/// 没有业务模块是合法状态（退出码 0）；有模块不合规则列出缺失项并失败。
fn run_check_modules(root: &std::path::Path) -> Result<ExitCode> {
    let report = check_modules(root);

    if report.is_empty() {
        println!(
            "无业务模块：{} 下没有模块目录",
            root.join("modules").display()
        );
        return Ok(ExitCode::SUCCESS);
    }

    if report.is_valid() {
        println!("模块目录校验通过：{} 个模块", report.modules.len());
        for module in &report.modules {
            println!("  ✓ {}", module.name);
        }
        return Ok(ExitCode::SUCCESS);
    }

    eprintln!("模块目录校验失败：错误码 E_MODULE_LAYOUT");
    for module in report.invalid_modules() {
        eprintln!("  ✗ {}", module.name);
        for problem in module.problems() {
            eprintln!("      - {problem}");
        }
    }
    Ok(ExitCode::FAILURE)
}

/// 执行依赖方向校验：平台 crate 不得反向依赖业务模块。
fn run_check_deps(root: &std::path::Path) -> Result<ExitCode> {
    let violations = dependency_direction_violations(root)?;
    if violations.is_empty() {
        println!("依赖方向校验通过：平台 crate 未反向依赖业务模块");
        return Ok(ExitCode::SUCCESS);
    }

    eprintln!("依赖方向校验失败：错误码 E_DEPENDENCY_DIRECTION");
    for violation in &violations {
        eprintln!("  ✗ {violation}");
    }
    Ok(ExitCode::FAILURE)
}

/// 执行能力索引的生成或校验。
fn run_capabilities(args: &xtask::cli::CapabilitiesArgs) -> Result<ExitCode> {
    let root = std::path::Path::new(&args.root);
    let generated = capabilities::generate(root)?;

    if args.check {
        let report = capabilities::check(root, &generated)?;
        if report.is_consistent() {
            println!("能力索引校验通过：{} 个文件与源码一致", generated.len());
            return Ok(ExitCode::SUCCESS);
        }
        eprintln!("能力索引校验失败：错误码 E_CAPABILITIES_OUTDATED");
        for name in &report.missing {
            eprintln!("  ✗ 缺失：capabilities/{name}");
        }
        for name in &report.changed {
            eprintln!("  ✗ 内容不一致：capabilities/{name}");
        }
        for name in &report.extra {
            eprintln!("  ✗ 多余文件（源码里已无对应产物）：capabilities/{name}");
        }
        eprintln!("  修复方式：运行 `cargo xtask capabilities` 重新生成");
        eprintln!("    缺失 / 内容不一致的文件会被重新写出，多余文件会被自动删除");
        return Ok(ExitCode::FAILURE);
    }

    let pruned = capabilities::write_files(root, &generated)?;
    println!("能力索引已生成：{} 个文件", generated.len());
    for name in generated.keys() {
        println!("  ✓ capabilities/{name}");
    }
    for name in &pruned {
        println!("  ✓ 已清理多余文件：capabilities/{name}");
    }
    Ok(ExitCode::SUCCESS)
}

/// 执行模块骨架生成。
fn run_create_module(args: &xtask::cli::CreateModuleArgs) -> Result<ExitCode> {
    let root = std::path::Path::new(&args.root);
    let report = create_module(root, &args.name, args.perm_prefix.as_deref())?;
    println!("模块骨架已生成：{}", report.module_dir.display());
    for path in &report.files {
        println!("  ✓ {}", path.display());
    }
    for note in &report.notes {
        println!("  ! {note}");
    }
    println!("下一步：");
    println!("  1. cargo xtask check-modules");
    println!(
        "  2. cargo test -p {} -- --test-threads=2",
        report.crate_name
    );
    println!("  3. cargo xtask capabilities");
    Ok(ExitCode::SUCCESS)
}

/// 执行租户作用域策略的统一生成 / 校验。
///
/// 业务模块在 manifest 的 `tenantScoped` 段声明表，**平台统一生成** RLS 策略 ——
/// 模块不手写（漏建一张表 = 该表完全无隔离）。
fn run_tenant_policies(args: &xtask::cli::TenantPoliciesArgs) -> Result<ExitCode> {
    let root = std::path::Path::new(&args.root);
    let scopes = tenant_policies::collect_scopes(root)?;
    if scopes.is_empty() {
        println!("没有模块声明 tenantScoped：无需生成策略");
        return Ok(ExitCode::SUCCESS);
    }

    let database_url = args
        .database_url
        .clone()
        .or_else(|| std::env::var("DATABASE_URL").ok())
        .ok_or_else(|| {
            anyhow::anyhow!("缺少数据库连接串：传 --database-url 或设置环境变量 DATABASE_URL")
        })?;

    let runtime = tokio::runtime::Runtime::new()?;

    if args.check {
        runtime.block_on(tenant_policies::check(&scopes, &database_url))?;
        println!(
            "租户隔离前提校验通过：{} 个模块声明的表都开了 RLS 且有策略",
            scopes.len()
        );
        return Ok(ExitCode::SUCCESS);
    }

    runtime.block_on(tenant_policies::apply(&scopes, &database_url))?;
    println!("已为 {} 个模块生成 RLS 策略：", scopes.len());
    for scope in &scopes {
        println!("  ✓ {}：{}", scope.module, scope.spec.tables.join(", "));
    }
    Ok(ExitCode::SUCCESS)
}

/// 执行审计链校验 / 归档（REQ-014）。
fn run_audit(args: &xtask::cli::AuditArgs) -> Result<ExitCode> {
    let runtime = tokio::runtime::Runtime::new()?;

    match &args.action {
        xtask::cli::AuditAction::Verify(verify) => {
            let database_url = database_url_of(verify.database_url.clone())?;
            let tenants = match &verify.tenant {
                Some(tenant) => vec![xtask::audit::require_tenant(Some(tenant))?],
                None => {
                    let tenants = runtime.block_on(xtask::audit::list_tenants(&database_url))?;
                    if tenants.is_empty() {
                        println!(
                            "没有找到任何有审计链状态的租户。\n  \
                             ⚠️ 用**应用角色**连接时这是**预期的**（RLS 只让本租户可见）——\n  \
                             「校验全部租户」需要**管理员**连接。"
                        );
                        return Ok(ExitCode::SUCCESS);
                    }
                    tenants
                }
            };

            let mut outcomes = Vec::with_capacity(tenants.len());
            for tenant in &tenants {
                let outcome =
                    runtime.block_on(xtask::audit::verify_tenant(&database_url, tenant))?;
                println!(
                    "  {}：{}（检查 {} 条，断裂 {} 条）",
                    outcome.tenant_id,
                    if outcome.ok { "通过" } else { "失败" },
                    outcome.checked,
                    outcome.broken
                );
                outcomes.push(outcome);
            }
            println!("{}", xtask::audit::summarize(&outcomes));
            if xtask::audit::has_failures(&outcomes) {
                return Ok(ExitCode::FAILURE);
            }
            Ok(ExitCode::SUCCESS)
        }
        xtask::cli::AuditAction::Archive(archive) => {
            let database_url = database_url_of(archive.database_url.clone())?;
            let tenant = xtask::audit::require_tenant(Some(&archive.tenant))?;
            let days = xtask::audit::require_positive_days(archive.days)?;
            let moved =
                runtime.block_on(xtask::audit::archive_tenant(&database_url, &tenant, days))?;
            println!("已归档租户 {tenant} 的超期记录：{moved} 条（在线保留 {days} 天）");
            Ok(ExitCode::SUCCESS)
        }
    }
}

/// 取数据库连接串：显式参数优先，其次环境变量 `DATABASE_URL`。
fn database_url_of(explicit: Option<String>) -> Result<String> {
    explicit
        .or_else(|| std::env::var("DATABASE_URL").ok())
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            anyhow::anyhow!("缺少数据库连接串：传 --database-url 或设置环境变量 DATABASE_URL")
        })
}
