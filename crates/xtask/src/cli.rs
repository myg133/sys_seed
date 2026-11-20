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
    /// 按 manifest 的 `tenantScoped` 段统一生成 / 校验 RLS 策略。
    TenantPolicies(TenantPoliciesArgs),
    /// 审计链校验与归档（REQ-014）。
    Audit(AuditArgs),
}

/// `audit` 参数。
#[derive(Debug, Args)]
pub struct AuditArgs {
    #[command(subcommand)]
    pub action: AuditAction,
}

/// `audit` 的子动作。
#[derive(Debug, Subcommand)]
pub enum AuditAction {
    /// 校验审计链的完整性（逐条独立重算 + 接续校验 + 末尾截断检测）。
    Verify(AuditVerifyArgs),
    /// 把超期记录搬进归档表（**单租户**）。
    Archive(AuditArchiveArgs),
}

/// `audit verify` 参数。
#[derive(Debug, Args)]
pub struct AuditVerifyArgs {
    /// 租户 ID；**省略** = 校验全部租户（需要能看见所有租户的行 —— 管理员连接）。
    #[arg(long)]
    pub tenant: Option<String>,
    /// 数据库连接串；默认取环境变量 `DATABASE_URL`。
    #[arg(long)]
    pub database_url: Option<String>,
}

/// `audit archive` 参数。
#[derive(Debug, Args)]
pub struct AuditArchiveArgs {
    /// 租户 ID（**必填** —— 归档是**单租户**的，D158）。
    #[arg(long)]
    pub tenant: String,
    /// 在线保留天数；默认 90（与登记的 `audit.retention.onlineDays` 一致）。
    #[arg(long, default_value_t = 90)]
    pub days: i64,
    /// 数据库连接串；默认取环境变量 `DATABASE_URL`。
    ///
    /// ⚠️ 应当是**应用角色**的连接（`base_app`）—— 受 RLS 约束，
    /// 作用域由调用方的租户上下文 + 函数内的守卫限定（D158 / D174）。
    #[arg(long)]
    pub database_url: Option<String>,
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

/// `tenant-policies` 参数。
#[derive(Debug, Args)]
pub struct TenantPoliciesArgs {
    /// 仓库根目录，默认为本工作区根。
    #[arg(long, default_value_t = default_root())]
    pub root: String,
    /// 只校验不写入：有表漏建 policy 时以非零码退出（CI 用）。
    #[arg(long)]
    pub check: bool,
    /// 数据库连接串；默认取环境变量 `DATABASE_URL`。
    ///
    /// 生成策略需要**表的属主权限** → 用管理员角色；
    /// 校验隔离前提可用应用角色（超级用户会给出「绕过 RLS」告警）。
    #[arg(long)]
    pub database_url: Option<String>,
}

/// 默认仓库根目录（字符串形式，便于 clap 生成帮助信息）。
fn default_root() -> String {
    crate::workspace_root().to_string_lossy().into_owned()
}
