//! 命令行接口定义。

use clap::{Args, Parser, Subcommand};

/// 底座工程任务运行器。
#[derive(Debug, Parser)]
#[command(name = "xtask", about = "底座工程任务运行器", version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

/// 可用子命令。
#[derive(Debug, Subcommand)]
pub enum Command {
    /// 校验 `modules/<name>/` 目录规范（AC-S-001）。
    CheckModules(CheckModulesArgs),
}

/// `check-modules` 参数。
#[derive(Debug, Args)]
pub struct CheckModulesArgs {
    /// 仓库根目录，默认为本工作区根。
    #[arg(long, default_value_t = default_root())]
    pub root: String,
}

/// 默认仓库根目录（字符串形式，便于 clap 生成帮助信息）。
fn default_root() -> String {
    crate::workspace_root().to_string_lossy().into_owned()
}
