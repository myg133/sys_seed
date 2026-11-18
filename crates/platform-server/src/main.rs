//! 底座服务二进制：`base`。
//!
//! ```text
//! base serve                 # 启动服务（默认子命令）
//! base migrate up            # 应用待执行迁移
//! base migrate down --target 0   # 回滚到指定版本
//! base migrate status        # 查看迁移状态
//! base tenant-policies       # 按 manifest 生成租户作用域表的 RLS 策略
//! ```
//!
//! 配置校验失败时**立即退出**（退出码 2），不进入监听状态。
//! 租户隔离前提不成立时（连接角色绕过 RLS）同样**立即退出**（退出码 3）。

use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::process::ExitCode;
use std::sync::Arc;

use axum::extract::State;
use axum::http::{StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use clap::{Parser, Subcommand};
use platform_core::RequestContext;
use platform_core::extension::expr::CompiledCondition;
use platform_core::extension::permission::{
    DataScope, PermissionChecker, PermissionRefresher, ScopeFilter,
};
use platform_iam::config::IamConfig;
use platform_iam::http::IamState;
use platform_iam::service::IamService;
use platform_iam::store::IamStore;
use platform_server::config::{ConfigError, DeployConfig, Environment};
use platform_server::{db, health, isolation, migrate, policies, probe, static_files};
use serde::Serialize;
use sqlx::PgPool;
use sqlx::migrate::Migrator;
use tracing_subscriber::EnvFilter;

/// 配置校验失败的退出码（与运行期错误区分开）。
const EXIT_CONFIG: u8 = 2;
/// 运行期失败的退出码。
const EXIT_RUNTIME: u8 = 1;
/// 租户隔离前提不成立的退出码（连接角色绕过 RLS）。
const EXIT_ISOLATION: u8 = 3;

/// CLI。
#[derive(Parser, Debug)]
#[command(
    name = "base",
    version,
    about = "底座服务：配置校验 / 健康检查 / 数据库迁移 / 前端托管"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

/// 子命令。
#[derive(Subcommand, Debug)]
enum Command {
    /// 启动服务（默认）。
    Serve,
    /// 数据库迁移（独立步骤，不在服务启动流程里）。
    Migrate {
        #[command(subcommand)]
        action: MigrateAction,
    },
    /// 探针：检查本机健康端点（镜像 HEALTHCHECK 用）。
    Healthcheck {
        /// 要检查的地址。
        #[arg(long, default_value = "http://127.0.0.1:8080/health/ready")]
        url: String,
    },
    /// 租户作用域策略：按业务模块 manifest 统一生成 / 校验 RLS 策略。
    ///
    /// 部署期用（镜像里没有 cargo，但策略必须能落地）。
    /// **必须先跑迁移再跑它** —— 策略是对已建好的表做的。
    TenantPolicies {
        /// 只校验，不改数据库；前提不成立时退出码非 0（让部署失败）。
        #[arg(long)]
        check: bool,
        /// manifest 根目录（其下是 `modules/<模块>/manifest.json`）。
        ///
        /// 缺省用 `BASE_MANIFEST_ROOT`（再缺省是当前目录）。
        #[arg(long)]
        manifest_root: Option<String>,
    },
}

/// 迁移动作。
#[derive(Subcommand, Debug)]
enum MigrateAction {
    /// 应用待执行迁移。
    Up,
    /// 回滚到目标版本（该版本及之前的迁移保留）。
    Down {
        /// 目标版本号。
        #[arg(long)]
        target: i64,
    },
    /// 查看迁移状态。
    Status,
}

/// 服务共享状态。
struct AppState {
    pool: PgPool,
    migrator: Arc<Migrator>,
    environment: Environment,
    static_root: Option<PathBuf>,
}

/// 统一错误响应体（RFC 9457，见 `api-conventions v1` C6）。
#[derive(Serialize)]
struct ProblemDetails {
    #[serde(rename = "type")]
    kind: String,
    title: String,
    status: u16,
    detail: String,
    instance: String,
    code: String,
    trace_id: String,
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(Failure::Config(err)) => {
            eprintln!("✗ 配置校验失败，进程退出（fail-fast）：");
            eprintln!("{err}");
            eprintln!(
                "提示：必填项见 deploy/RUNBOOK.md 与 .env.example；生产用 K8s Secret / compose .env 注入。"
            );
            ExitCode::from(EXIT_CONFIG)
        }
        Err(Failure::Runtime(err)) => {
            eprintln!("✗ {err}");
            ExitCode::from(EXIT_RUNTIME)
        }
        Err(Failure::Isolation(err)) => {
            eprintln!("✗ 租户隔离前提不成立，进程退出（fail-fast）：");
            eprintln!("{err}");
            ExitCode::from(EXIT_ISOLATION)
        }
    }
}

/// 失败类型。
enum Failure {
    /// 配置校验失败。
    Config(ConfigError),
    /// 运行期失败。
    Runtime(String),
    /// 租户隔离前提不成立。
    Isolation(isolation::IsolationError),
}

impl From<ConfigError> for Failure {
    fn from(err: ConfigError) -> Self {
        Self::Config(err)
    }
}

impl From<anyhow::Error> for Failure {
    fn from(err: anyhow::Error) -> Self {
        Self::Runtime(format!("{err:#}"))
    }
}

impl From<migrate::MigrationError> for Failure {
    fn from(err: migrate::MigrationError) -> Self {
        Self::Runtime(err.to_string())
    }
}

/// 按子命令分发。
async fn run(cli: Cli) -> Result<(), Failure> {
    match cli.command.unwrap_or(Command::Serve) {
        Command::Serve => serve().await,
        Command::Migrate { action } => run_migrate(action).await,
        Command::Healthcheck { url } => run_healthcheck(&url).await,
        Command::TenantPolicies {
            check,
            manifest_root,
        } => run_tenant_policies(check, manifest_root).await,
    }
}

/// 执行健康探针：状态码 200 算通过。
async fn run_healthcheck(url: &str) -> Result<(), Failure> {
    match probe::get_status(url).await {
        Ok(200) => {
            println!("✓ {url} → 200");
            Ok(())
        }
        Ok(status) => {
            println!("✗ {url} → {status}");
            Err(Failure::Runtime(format!("探针 {url} 返回 {status}")))
        }
        Err(err) => Err(Failure::Runtime(format!("探针 {url} 失败：{err}"))),
    }
}

/// 读取配置并校验；失败即 fail-fast。
fn load_config() -> Result<DeployConfig, Failure> {
    DeployConfig::from_env().map_err(Failure::from)
}

/// 日志事件的 target：**bin crate `base`** 与 lib crate `platform_server`。
///
/// 本服务的日志事件都写在 `main.rs` 里 → tracing 的 target 是 **bin crate 名 `base`**，
/// 不是 lib crate 名。过滤器只写 `platform_server=…` 时**一条都匹配不上**，
/// 日志会被静默丢弃（历史 bug：`BASE_LOG_LEVEL` 形同虚设，默认配置下一行日志都不输出）。
/// 两个 target 都要写进过滤器。
const LOG_TARGETS: [&str; 2] = ["base", "platform_server"];

/// 生成日志过滤器：`RUST_LOG` 优先，否则按 `BASE_LOG_LEVEL` 生成。
///
/// `BASE_LOG_LEVEL` 的值会套到**每一个** target 上（见 [`LOG_TARGETS`]），
/// 保证「级别开关」与「事件所在 target」两件事都覆盖到。
fn log_filter(level: &str) -> EnvFilter {
    EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        let mut directives = LOG_TARGETS
            .iter()
            .map(|target| format!("{target}={level}"))
            .collect::<Vec<_>>();
        directives.push("tower_http=info".to_string());
        EnvFilter::new(directives.join(","))
    })
}

/// 初始化日志。
fn init_tracing(level: &str) {
    tracing_subscriber::fmt()
        .with_env_filter(log_filter(level))
        .init();
}

/// 加载迁移目录；目录不存在时 fail-fast（readiness 依赖它）。
async fn load_migrator(dir: &str) -> Result<Migrator, Failure> {
    Migrator::new(PathBuf::from(dir))
        .await
        .map_err(|err| Failure::Runtime(format!("加载迁移目录 `{dir}` 失败：{err}")))
}

/// 启动服务。
async fn serve() -> Result<(), Failure> {
    let config = load_config()?;
    init_tracing(&config.log_level);

    let migrator = Arc::new(load_migrator(&config.migrations_dir).await?);
    let pool = db::pool(&config.database_url, db::DEFAULT_MAX_CONNECTIONS)
        .await
        .map_err(|err| Failure::Runtime(format!("数据库连接串非法：{err}")))?;

    // 租户隔离前提自检：连接角色**不得**绕过 RLS（超级用户 / BYPASSRLS）。
    // 命中即拒绝启动 —— 否则 RLS 在部署里等于没开（租户隔离 fail open）。
    // 见 deploy/RUNBOOK.md「应用角色与 RLS」。
    let isolation =
        isolation::verify_startup_isolation(&config.database_url, config.allow_superuser)
            .await
            .map_err(Failure::Isolation)?;
    if isolation.superuser_allowed {
        tracing::warn!(
            role = %isolation.report.role,
            "BASE_ALLOW_SUPERUSER=true：放行了绕过 RLS 的连接角色 —— 租户隔离会 fail open（仅限本地开发）"
        );
    } else {
        tracing::info!(role = %isolation.report.role, "租户隔离前提自检通过：连接角色不绕过 RLS");
    }

    let state = Arc::new(AppState {
        pool,
        migrator,
        environment: config.environment,
        static_root: config.static_dir.as_ref().map(PathBuf::from),
    });

    let mut app = Router::new()
        .route("/health/live", get(live))
        .route("/health/ready", get(ready));

    // 配置与开关（REQ-010）的认证 / 鉴权部件。默认 **fail closed**：
    // 没配 IAM 时没有可铸造的令牌、权限判定一律拒绝 —— 但 `/api/config/branding`
    // 是公开接口，仍要能渲染登录页（它不经过认证中间件）。
    let mut config_tokens: Arc<dyn platform_config::http::TokenVerifier> =
        Arc::new(RejectAllTokens);
    let mut config_checker: Arc<dyn PermissionChecker> = Arc::new(DenyAllPermissions);
    let mut config_permissions: Arc<dyn PermissionRefresher> = Arc::new(DenyAllPermissions);

    // 认证与账号体系（REQ-007）：`/api/iam/*` + JWKS。
    //
    // **配了就必须配对**：环境变量里只要出现过一个 IAM 相关变量，就要求整套配置
    // 完整有效 —— 半配置上线（密钥缺失 / 是占位符）一律 fail-fast。
    // 一个都没配 → 平台**不提供**认证接口（服务照常起）。
    if IamConfig::is_configured(|key| std::env::var(key).ok()) {
        let iam_state = build_iam_state(&config).await?;
        app = app
            .nest("/api/iam", platform_iam::http::router(iam_state.clone()))
            .merge(platform_iam::http::jwks_router(iam_state.clone()));

        // 用户与组织架构（REQ-008）：`/api/org/*`。
        //
        // **依赖 IAM**：组织数据是租户数据，接口必须验令牌（否则改个请求头就能读别人的组织树）。
        // 所以 IAM 没配 → 不提供 /api/org/*。
        let org_state = build_org_state(&config, iam_state.service.clone()).await?;
        app = app.nest("/api/org", platform_org::http::router(org_state));
        tracing::info!("用户与组织架构已启用：/api/org/*");

        // 角色与权限（REQ-009）：`/api/rbac/*`。
        //
        // **启动时先对账**：manifest 与库里的权限点不一致 → **进程起不来**
        // （退出码非 0，逐条列出差异）。不一致不是告警 —— 告警就是静默放行的一种。
        let rbac_state = build_rbac_state(&config, iam_state.service.clone()).await?;
        reconcile_permissions(&config, &rbac_state).await?;
        app = app.nest("/api/rbac", platform_rbac::http::router(rbac_state.clone()));
        tracing::info!("角色与权限已启用：/api/rbac/*");

        // 配置接口的令牌校验与权限判定复用同一套 IAM / RBAC 实现：
        // 令牌主体来自 IAM，权限点判定与缓存刷新来自 `RbacChecker`
        // （同一个 `Arc` 同时实现 `PermissionChecker` 与 `PermissionRefresher`）。
        config_tokens = Arc::new(ConfigIamTokenVerifier::new(iam_state.service.clone()));
        config_checker = rbac_state.checker.clone();
        config_permissions = rbac_state.checker.clone();
    } else {
        tracing::warn!(
            "未检测到 IAM 环境变量（BASE_JWT_* / BASE_IAM_*）：平台不提供 /api/iam/* 与 /api/org/* 接口"
        );
    }

    let app = app.fallback(static_fallback).with_state(state);

    // 配置、品牌与 Feature Flag（REQ-010）：`/api/config/*` + `/api/feature-flags/*`。
    //
    // 品牌接口公开（登录页在认证前就要显示 logo）；其余接口需要令牌 + 权限点。
    let config_state = Arc::new(platform_config::http::ConfigState {
        service: build_config_service(&config).await?,
        tokens: config_tokens,
        checker: config_checker,
        permissions: config_permissions,
    });
    let app = app.merge(platform_config::http::router(config_state));
    tracing::info!("配置与品牌已启用：/api/config/*、/api/feature-flags/*");

    let addr = config.socket_addr();
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|err| Failure::Runtime(format!("监听 {addr} 失败：{err}")))?;
    tracing::info!(
        %addr,
        environment = config.environment.as_str(),
        tenant_mode = config.tenant_mode.as_str(),
        auth_mode = config.auth_mode.as_str(),
        "底座服务已启动"
    );
    axum::serve(listener, app)
        .await
        .map_err(|err| Failure::Runtime(format!("HTTP 服务退出：{err}")))?;
    Ok(())
}

/// 构建认证与账号体系的状态（配置 + 数据访问 + 服务）。
///
/// 失败即 fail-fast（退出码 2 / 1）：**密钥配错了绝不能带着半套配置上线**。
async fn build_iam_state(config: &DeployConfig) -> Result<Arc<IamState>, Failure> {
    let iam_config = IamConfig::from_env().map_err(|err| {
        Failure::Config(ConfigError {
            problems: err
                .problems
                .into_iter()
                .map(|problem| platform_server::config::ConfigProblem {
                    code: problem.code,
                    key: problem.key,
                    detail: problem.detail,
                })
                .collect(),
        })
    })?;

    // 数据访问走 `platform-db` 的租户作用域入口（**不得**直连池）。
    let tenant_db =
        platform_db::TenantDb::connect(&config.database_url, platform_db::DbOptions::default())
            .await
            .map_err(|err| Failure::Runtime(format!("数据库连接串非法：{err}")))?;

    let service = Arc::new(
        IamService::new(IamStore::new(tenant_db), iam_config)
            .map_err(|err| Failure::Runtime(format!("认证与账号体系初始化失败：{err}")))?,
    );
    Ok(Arc::new(IamState { service }))
}

/// 构建用户与组织架构的状态（数据访问 + 服务 + 令牌校验）。
///
/// 组织接口**必须**验令牌（组织数据是租户数据）—— 所以它依赖 IAM 的认证服务。
async fn build_org_state(
    config: &DeployConfig,
    iam: Arc<platform_iam::service::IamService>,
) -> Result<Arc<platform_org::http::OrgState>, Failure> {
    // 数据访问走 `platform-db` 的租户作用域入口（**不得**直连池）。
    let tenant_db =
        platform_db::TenantDb::connect(&config.database_url, platform_db::DbOptions::default())
            .await
            .map_err(|err| Failure::Runtime(format!("数据库连接串非法：{err}")))?;

    let service = platform_org::OrgService::new(
        platform_org::OrgStore::new(tenant_db.clone()),
        platform_iam::store::IamStore::new(tenant_db),
        // 通知通道（REQ-015）还没落地：事件照常产生，先写日志。
        std::sync::Arc::new(platform_org::notify::LoggingNotifier),
        // 审计落库（REQ-014）还没落地：事件照常产生，先用空实现。
        std::sync::Arc::new(platform_org::audit::NoopAuditRecorder),
    );

    Ok(Arc::new(platform_org::http::OrgState {
        service,
        tokens: std::sync::Arc::new(platform_org::http::IamTokenVerifier::new(iam)),
    }))
}

/// 构建角色与权限的状态（数据访问 + 服务 + 判定器 + 令牌校验）。
async fn build_rbac_state(
    config: &DeployConfig,
    iam: Arc<platform_iam::service::IamService>,
) -> Result<Arc<platform_rbac::http::RbacState>, Failure> {
    // 数据访问走 `platform-db` 的租户作用域入口（**不得**直连池）。
    let tenant_db =
        platform_db::TenantDb::connect(&config.database_url, platform_db::DbOptions::default())
            .await
            .map_err(|err| Failure::Runtime(format!("数据库连接串非法：{err}")))?;

    let service = Arc::new(platform_rbac::RbacService::new(
        platform_rbac::RbacStore::new(tenant_db),
        platform_rbac::ScopePolicy::default(),
    ));
    let checker = Arc::new(platform_rbac::RbacChecker::new(service.clone()));

    Ok(Arc::new(platform_rbac::http::RbacState {
        service,
        checker,
        tokens: Arc::new(platform_rbac::http::IamTokenVerifier::new(iam)),
    }))
}

/// 构造配置服务（注册表来自 manifest 根目录，数据访问只走 `platform-db`）。
async fn build_config_service(
    config: &DeployConfig,
) -> Result<Arc<platform_config::ConfigService>, Failure> {
    // 数据访问走 `platform-db` 的租户作用域入口（**不得**直连池）。
    let tenant_db =
        platform_db::TenantDb::connect(&config.database_url, platform_db::DbOptions::default())
            .await
            .map_err(|err| Failure::Runtime(format!("数据库连接串非法：{err}")))?;

    let registry =
        platform_config::ConfigRegistry::from_manifest_root(Path::new(&config.manifest_root))
            .map_err(|err| Failure::Runtime(format!("配置键注册表加载失败：{err}")))?;
    if registry.is_empty() {
        return Err(Failure::Runtime(
            "配置键注册表为空：manifest 根目录下没有找到任何 config 段（检查 BASE_MANIFEST_ROOT）"
                .to_string(),
        ));
    }

    Ok(Arc::new(platform_config::ConfigService::new(
        tenant_db,
        Arc::new(registry),
    )))
}

/// 没配 IAM 时的令牌校验：**一律拒绝**（fail closed）。
struct RejectAllTokens;

impl platform_config::http::TokenVerifier for RejectAllTokens {
    fn verify(
        &self,
        _token: &str,
    ) -> Result<platform_config::http::VerifiedToken, platform_config::ConfigError> {
        Err(platform_config::ConfigError::Unauthenticated)
    }
}

/// 没配 IAM 时的权限判定：**一律拒绝**（fail closed）。
struct DenyAllPermissions;

impl PermissionChecker for DenyAllPermissions {
    fn has(&self, _ctx: &RequestContext, _permission: &str) -> bool {
        false
    }

    fn scope(&self, _ctx: &RequestContext, _scope: DataScope) -> ScopeFilter {
        ScopeFilter::All
    }

    fn effective_scope(&self, _ctx: &RequestContext, _permission: &str) -> ScopeFilter {
        ScopeFilter::All
    }

    fn condition(&self, _ctx: &RequestContext, _permission: &str) -> Option<CompiledCondition> {
        None
    }
}

impl PermissionRefresher for DenyAllPermissions {
    fn ensure_fresh<'a>(
        &'a self,
        _ctx: &'a RequestContext,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>> {
        Box::pin(async { Ok(()) })
    }
}

/// IAM 的令牌校验适配器（只转发验签，不重写语义）。
struct ConfigIamTokenVerifier {
    inner: Arc<platform_iam::service::IamService>,
}

impl ConfigIamTokenVerifier {
    fn new(inner: Arc<platform_iam::service::IamService>) -> Self {
        Self { inner }
    }
}

impl platform_config::http::TokenVerifier for ConfigIamTokenVerifier {
    fn verify(
        &self,
        token: &str,
    ) -> Result<platform_config::http::VerifiedToken, platform_config::ConfigError> {
        let claims = self
            .inner
            .verify_access_token(token)
            .map_err(|_| platform_config::ConfigError::Unauthenticated)?;
        Ok(platform_config::http::VerifiedToken {
            user_id: claims.subject,
            tenant_id: claims.tenant_id,
        })
    }
}

/// **权限点注册表的双向对账**：manifest ↔ 库。
///
/// 不一致 → **启动失败**（退出码非 0），逐条列出差异。
///
/// 为什么不是告警：权限点缺失时角色授了也判不出来，表现为「配了权限却不生效」——
/// 排查成本极高，且容易被误判成业务 bug。告警 = 静默放行的一种。
async fn reconcile_permissions(
    config: &DeployConfig,
    state: &Arc<platform_rbac::http::RbacState>,
) -> Result<(), Failure> {
    let manifest_permissions =
        platform_rbac::collect_manifest_permissions(std::path::Path::new(&config.manifest_root))
            .map_err(|err| Failure::Runtime(format!("权限点注册表对账失败：{err}")))?;

    platform_rbac::registry::require_consistent(state.service.store(), &manifest_permissions)
        .await
        .map_err(|err| {
            Failure::Runtime(format!(
                "✗ 权限点注册表与数据库不一致，进程退出（fail-fast）：\n{err}\n  \
                 提示：权限点由**迁移**写入（见 migrations/ 下的权限点种子迁移），\
                 manifest 变更要同时改 manifest 与迁移。"
            ))
        })?;
    Ok(())
}

/// 执行租户作用域策略的生成 / 校验。
///
/// 部署期用（镜像里没有 cargo，但策略必须能落地）。
/// 连接串用**迁移（管理员）连接** —— 建策略需要表的属主权限。
async fn run_tenant_policies(check: bool, manifest_root: Option<String>) -> Result<(), Failure> {
    let config = load_config()?;
    init_tracing(&config.log_level);

    let root = manifest_root.unwrap_or_else(|| config.manifest_root.clone());
    policies::run(&root, &config.database_url, check)
        .await
        .map_err(|err| Failure::Runtime(format!("{err:#}")))
}

/// 执行迁移子命令。
async fn run_migrate(action: MigrateAction) -> Result<(), Failure> {
    let config = load_config()?;
    init_tracing(&config.log_level);

    let migrator = load_migrator(&config.migrations_dir).await?;
    let pool = db::migration_pool(&config.database_url)
        .await
        .map_err(|err| Failure::Runtime(format!("数据库连接串非法：{err}")))?;

    match action {
        MigrateAction::Up => {
            let applied = migrate::migrate_up(&pool, &migrator).await?;
            if applied.is_empty() {
                println!("迁移：没有待应用的迁移");
            } else {
                println!("迁移：已应用版本 {applied:?}");
            }
        }
        MigrateAction::Down { target } => {
            let reverted = migrate::migrate_down(&pool, &migrator, target).await?;
            if reverted.is_empty() {
                println!("迁移：目标版本 {target} 之上没有可回滚的迁移");
            } else {
                println!("迁移：已回滚版本 {reverted:?} → 目标版本 {target}");
            }
        }
        MigrateAction::Status => {
            let status = migrate::migration_status(&pool, &migrator).await?;
            println!("已应用：{:?}", status.applied);
            println!("待应用：{:?}", status.pending);
            println!("不可逆（无配套 down）：{:?}", status.irreversible);
        }
    }
    Ok(())
}

/// liveness：进程活着就返回 200，不查数据库。
async fn live() -> Json<serde_json::Value> {
    let ctx = RequestContext::new();
    Json(serde_json::json!({
        "status": "alive",
        "version": platform_core::VERSION,
        "traceId": ctx.trace_id(),
    }))
}

/// readiness：数据库连通 **且** 迁移已应用才返回 200。
async fn ready(State(state): State<Arc<AppState>>) -> Response {
    let ctx = RequestContext::new();
    let readiness = health::check_readiness(&state.pool, &state.migrator).await;

    if readiness.is_ready() {
        return Json(serde_json::json!({
            "status": "ready",
            "environment": state.environment.as_str(),
            "version": platform_core::VERSION,
            "appliedMigrations": readiness.applied,
            "traceId": ctx.trace_id(),
        }))
        .into_response();
    }

    let code = readiness.error_code.unwrap_or(health::CODE_DB_UNAVAILABLE);
    let detail = readiness.detail.clone().unwrap_or_default();
    // `detail` 是面向开发者的英文描述（契约 C6），面向用户的中文文案由前端按 `code` 渲染；
    // 运维视角的中文说明放在**日志**里。
    tracing::warn!(
        code = code,
        detail = %detail,
        pending = ?readiness.pending,
        dirty = ?readiness.dirty,
        "readiness 未通过：数据库不可达或迁移未应用（面向用户的中文文案由前端按 code 渲染）"
    );
    let problem = ProblemDetails {
        kind: "https://errors.base.example/service-not-ready".to_string(),
        title: "服务未就绪".to_string(),
        status: StatusCode::SERVICE_UNAVAILABLE.as_u16(),
        detail,
        instance: "/health/ready".to_string(),
        code: code.to_string(),
        trace_id: ctx.trace_id().to_string(),
    };
    (StatusCode::SERVICE_UNAVAILABLE, Json(problem)).into_response()
}

/// 静态资源回落：配置了静态目录就托管前端，否则 404。
async fn static_fallback(State(state): State<Arc<AppState>>, uri: Uri) -> Response {
    match &state.static_root {
        Some(root) => static_files::serve(root, &uri).await,
        None => (
            StatusCode::NOT_FOUND,
            [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
            "404 Not Found",
        )
            .into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::sync::{Arc, Mutex};

    /// 把格式化后的日志写进内存缓冲（用来断言过滤器有没有命中）。
    #[derive(Clone, Default)]
    struct BufWriter(Arc<Mutex<Vec<u8>>>);

    impl Write for BufWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().expect("日志缓冲锁 poisoned").write(buf)
        }

        fn flush(&mut self) -> std::io::Result<()> {
            self.0.lock().expect("日志缓冲锁 poisoned").flush()
        }
    }

    impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for BufWriter {
        type Writer = BufWriter;

        fn make_writer(&'a self) -> Self::Writer {
            self.clone()
        }
    }

    /// 用给定过滤器捕获 `emit` 产生的日志，返回格式化后的文本。
    fn capture_logs(filter: EnvFilter, emit: impl FnOnce()) -> String {
        let buf = BufWriter::default();
        let subscriber = tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_ansi(false)
            .with_writer(buf.clone())
            .finish();
        tracing::subscriber::with_default(subscriber, emit);
        let bytes = buf.0.lock().expect("日志缓冲锁 poisoned").clone();
        String::from_utf8(bytes).expect("日志是 UTF-8")
    }

    #[test]
    fn cli_defaults_to_serve() {
        let cli = Cli::parse_from(["base"]);
        assert!(cli.command.is_none());
    }

    #[test]
    fn migrate_up_parses() {
        let cli = Cli::parse_from(["base", "migrate", "up"]);
        assert!(matches!(
            cli.command,
            Some(Command::Migrate {
                action: MigrateAction::Up
            })
        ));
    }

    #[test]
    fn migrate_down_requires_target() {
        let cli = Cli::parse_from(["base", "migrate", "down", "--target", "0"]);
        let Some(Command::Migrate {
            action: MigrateAction::Down { target },
        }) = cli.command
        else {
            panic!("应当解析为 migrate down");
        };
        assert_eq!(target, 0);
    }

    #[test]
    fn healthcheck_url_can_be_overridden() {
        let cli = Cli::parse_from([
            "base",
            "healthcheck",
            "--url",
            "http://127.0.0.1:9/health/live",
        ]);
        assert!(matches!(cli.command, Some(Command::Healthcheck { .. })));
    }

    #[test]
    fn migrate_down_without_target_is_rejected() {
        assert!(Cli::try_parse_from(["base", "migrate", "down"]).is_err());
    }

    #[test]
    fn log_filter_captures_events_from_the_bin_target() {
        // 本服务的日志事件写在 main.rs 里 → target 是 bin crate 名 `base`。
        // 过滤器只写 `platform_server=…` 时这条会被丢掉（`BASE_LOG_LEVEL` 形同虚设）。
        let out = capture_logs(log_filter("info"), || {
            tracing::warn!(target: "base", code = "MIGRATIONS_PENDING", "运维冒烟：readiness 未通过");
        });
        assert!(
            out.contains("运维冒烟：readiness 未通过"),
            "过滤器没命中 bin target 的日志：{out}"
        );
    }

    #[test]
    fn log_filter_captures_events_without_an_explicit_target() {
        // 不写 `target:` 时 tracing 用模块路径（`base::…`）当 target —— 也要能命中。
        let out = capture_logs(log_filter("info"), || {
            tracing::info!("底座服务已启动");
        });
        assert!(
            out.contains("底座服务已启动"),
            "过滤器没命中默认 target 的日志：{out}"
        );
    }

    #[test]
    fn log_filter_captures_events_from_the_lib_target() {
        // lib crate `platform_server` 侧的日志同样要能被命中。
        let out = capture_logs(log_filter("info"), || {
            tracing::warn!(target: "platform_server", "lib 侧日志冒烟");
        });
        assert!(
            out.contains("lib 侧日志冒烟"),
            "过滤器没命中 lib target 的日志：{out}"
        );
    }

    #[test]
    fn log_filter_respects_the_configured_level() {
        // `BASE_LOG_LEVEL=error` 时，info 事件要被挡掉，error 事件要留下。
        let out = capture_logs(log_filter("error"), || {
            tracing::info!(target: "base", "不该出现的 info 日志");
            tracing::error!(target: "base", "该出现的 error 日志");
        });
        assert!(!out.contains("不该出现的 info 日志"), "级别没生效：{out}");
        assert!(
            out.contains("该出现的 error 日志"),
            "error 日志被丢了：{out}"
        );
    }
}
