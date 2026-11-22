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
    /// 通知投递批处理（REQ-015 / D296）。
    Notify(NotifyArgs),
    /// 导入导出批处理（REQ-018 / D324）。
    Storage(StorageArgs),
    /// 生成 / 校验 OpenAPI 文档（REQ-019 / D338）。
    Openapi(OpenapiArgs),
    /// 可观测性运维任务：清理限流行等（REQ-019 / D337）。
    Observability(ObservabilityArgs),
}

/// `observability` 参数。
///
/// # ⚠️ 为什么清理必须是**显式命令**（D337 / D296 / D324）
///
/// 限流表每请求一行 → 必须定期清理，否则表会无限增长。
/// 清理**不放在 web 进程内**的后台线程，理由与 `notify worker` / `storage worker` 一致：
///
/// 1. web 重启会把定时器弄丢；
/// 2. 多实例部署时每个实例都跑一遍（重复删除、互相争锁）；
/// 3. 运维需要看到「跑了几次、删了多少行」；
/// 4. 测试可以直接调一次，不用 `sleep`。
///
/// 部署侧用 cron / systemd timer 定时调（见 `design-summary.md` 的「定期任务」表）。
#[derive(Debug, Args)]
pub struct ObservabilityArgs {
    #[command(subcommand)]
    pub action: ObservabilityAction,
}

/// `observability` 的子动作。
#[derive(Debug, Subcommand)]
pub enum ObservabilityAction {
    /// 清理 `platform_rate_limit` 里超出保留期的行。
    PruneRateLimit(PruneRateLimitArgs),
}

/// `observability prune-rate-limit` 参数。
#[derive(Debug, Args)]
pub struct PruneRateLimitArgs {
    /// 三个维度里**最大**的窗口长度（秒）。
    ///
    /// ⚠️ 必须 ≥ 配置里 `rateLimit.*.windowSeconds` 的最大值。
    /// 传小了会把**仍在窗口内**的行删掉 → 限流计数变少 → 实际放行超过配额。
    ///
    /// 实际保留期 = 本值 **× 2**（由 `PgRateLimiter::prune` 内部计算）。
    #[arg(long, default_value_t = 60)]
    pub max_window_seconds: i64,
    /// 数据库连接串；默认取环境变量 `DATABASE_URL`。
    ///
    /// ⚠️ 列「哪些租户有限流行」需要**绕过 RLS** → 这里应当传**管理员**连接。
    /// 用应用角色也能跑，但只会看到 0 个租户（RLS 会过滤掉）→ 等于没清理。
    #[arg(long)]
    pub database_url: Option<String>,
}

/// `openapi` 参数。
#[derive(Debug, Args)]
pub struct OpenapiArgs {
    /// 只校验不写入：产物与源码不一致时非零退出（CI 用）。
    #[arg(long)]
    pub check: bool,
}

/// `storage` 参数。
///
/// 与 `notify` 同构：worker 是**显式命令**而不是 web 进程内的后台线程（D296 / D324）。
#[derive(Debug, Args)]
pub struct StorageArgs {
    #[command(subcommand)]
    pub action: StorageAction,
}

/// `storage` 的子动作。
#[derive(Debug, Subcommand)]
pub enum StorageAction {
    /// 跑一轮导入导出批处理。
    Worker(StorageWorkerArgs),
}

/// `storage worker` 参数。
#[derive(Debug, Args)]
pub struct StorageWorkerArgs {
    /// 租户 ID（**必填** —— 导入导出任务是租户级的，D324）。
    #[arg(long)]
    pub tenant: String,
    /// 这一批最多取多少个任务。
    #[arg(long, default_value_t = 4)]
    pub batch: i64,
    /// 连续跑多少轮；`1` = 只跑一次（默认）。`0` = 一直跑到没有待处理任务。
    #[arg(long, default_value_t = 1)]
    pub rounds: u32,
    /// 数据库连接串；默认取环境变量 `DATABASE_URL`。
    #[arg(long)]
    pub database_url: Option<String>,
}

/// `notify` 参数。
///
/// # ⚠️ 为什么 worker 是**显式命令**而不是 web 进程内的后台线程（D296）
///
/// 1. web 进程重启会丢正在投递的消息（进程内队列的本质缺陷）；
/// 2. web 进程多实例时各自持有队列 → 投递**重复或遗漏**；
/// 3. **测试可控**：E2E / 集成测试可以直接调一次 `tick()`，不用 `sleep` 等后台。
#[derive(Debug, Args)]
pub struct NotifyArgs {
    #[command(subcommand)]
    pub action: NotifyAction,
}

/// `notify` 的子动作。
#[derive(Debug, Subcommand)]
pub enum NotifyAction {
    /// 跑**一轮**投递批处理（认领一批 → 逐个投递 → 写回结果）。
    Worker(NotifyWorkerArgs),
}

/// `notify worker` 参数。
#[derive(Debug, Args)]
pub struct NotifyWorkerArgs {
    /// 租户 ID（**必填** —— 投递记录是租户级的，D299）。
    #[arg(long)]
    pub tenant: String,
    /// 这一批最多取多少条。
    #[arg(long, default_value_t = 32)]
    pub batch: i64,
    /// 连续跑多少轮；`1` = 只跑一次（默认）。`0` = 一直跑到没有待投递。
    #[arg(long, default_value_t = 1)]
    pub rounds: u32,
    /// 数据库连接串；默认取环境变量 `DATABASE_URL`。
    #[arg(long)]
    pub database_url: Option<String>,
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
