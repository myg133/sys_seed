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
    /// 校验依赖方向：平台 crate 不得反向依赖业务模块。
    CheckDeps(CheckDepsArgs),
    /// 生成 / 校验能力索引（`capabilities/`）。
    Capabilities(CapabilitiesArgs),
    /// 生成业务模块骨架。
    CreateModule(CreateModuleArgs),
}

/// `check-modules` 参数。
#[derive(Debug, Args)]
pub struct CheckModulesArgs {
    /// 仓库根目录，默认为本工作区根。
    #[arg(long, default_value_t = default_root())]
    pub root: String,
}

/// `check-deps` 参数。
#[derive(Debug, Args)]
pub struct CheckDepsArgs {
    /// 仓库根目录，默认为本工作区根。
    #[arg(long, default_value_t = default_root())]
    pub root: String,
}

/// `capabilities` 参数。
#[derive(Debug, Args)]
pub struct CapabilitiesArgs {
    /// 仓库根目录，默认为本工作区根。
    #[arg(long, default_value_t = default_root())]
    pub root: String,
    /// 只校验不写入：索引与源码不一致时以非零码退出（CI 用）。
    #[arg(long)]
    pub check: bool,
}

/// `create-module` 参数。
#[derive(Debug, Args)]
pub struct CreateModuleArgs {
    /// 模块名（小写 kebab-case，同时是目录名与 manifest 的 id）。
    pub name: String,
    /// 权限点前缀（权限点编码的第一段），默认为模块名。
    #[arg(long)]
    pub perm_prefix: Option<String>,
    /// 仓库根目录，默认为本工作区根。
    #[arg(long, default_value_t = default_root())]
    pub root: String,
}

/// 默认仓库根目录（字符串形式，便于 clap 生成帮助信息）。
fn default_root() -> String {
    crate::workspace_root().to_string_lossy().into_owned()
}
