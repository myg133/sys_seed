//! 子命令分派。

use std::process::ExitCode;

use anyhow::Result;
use clap::Parser;
use xtask::cli::{Cli, Command};
use xtask::modules::check_modules;

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Command::CheckModules(args) => match run_check_modules(std::path::Path::new(&args.root)) {
            Ok(code) => code,
            Err(err) => {
                eprintln!("错误：{err:#}");
                ExitCode::FAILURE
            }
        },
    }
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
