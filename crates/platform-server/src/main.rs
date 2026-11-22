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

use axum::extract::{Request, State};
use axum::http::{HeaderMap, StatusCode, Uri, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use clap::{Parser, Subcommand};
use platform_core::RequestContext;
use platform_core::extension::expr::CompiledCondition;
use platform_core::extension::permission::{
    DataScope, PermissionChecker, PermissionProvider, PermissionRefresher, ScopeFilter,
};
use platform_core::log_filter;
use platform_iam::config::IamConfig;
use platform_iam::http::IamState;
use platform_iam::service::IamService;
use platform_iam::store::IamStore;
use platform_schema::{SchemaRegistry, SchemaState};
use platform_server::config::{ConfigError, DeployConfig, Environment};
use platform_server::sso_source::ConfigServiceSsoSource;
use platform_server::{db, health, isolation, migrate, policies, probe, static_files};
use serde::Serialize;
use sqlx::PgPool;
use sqlx::migrate::Migrator;

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

/// 日志过滤器：**复用 `platform-core` 的那一份**（`platform_core::log_filter`）。
///
/// # 为什么不再在这里定义
///
/// 曾经它是本地实现、只给 `["base", "platform_server"]` 生成 directive ——
/// 于是 `platform_config` 的 WARN 在默认部署下被**静默丢弃**
/// （REQ-010 QA 判 HIGH）。更根本的问题是：**测试够不着这个函数**，
/// 只能用自定义 `EnvFilter` 近似 → 测过的东西与线上跑的不是同一份代码 → 假绿。
///
/// 抽到 `platform-core` 之后，测试与生产调同一个函数，「测试放行但生产吞掉」结构上不可能。
/// 形态是「全局默认级别 + 噪音库降级」，**新增 crate 不需要改清单**。
/// 实现与守护测试见 `crates/platform-core/src/log.rs`。
///
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

    // 实体元数据注册表（REQ-012）。**必须在装配业务模块之前**建成：
    //
    // 1. 它是**阶段 2**（跨文件）校验的落点 —— 字段权限点是否已登记、
    //    `reference` 指向的实体是否存在，**只有**在所有 manifest 都加载完后才能判；
    // 2. 失败 → 进程**起不来**（退出码 1），逐条点名「谁的哪个引用指向了不存在的东西」。
    //
    // 顺序放在最前：`platform-config` / `platform-rbac` 也会各自 parse 一遍 manifest，
    // 它们的外层标题是「配置键注册表」「权限点对账」—— 元数据的错不该披着那两张皮
    // （QA #30）。
    let schema_registry = load_schema_registry(&config.manifest_root)?;

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
    // 元数据接口的令牌校验：与配置接口同一个 IAM 实现（没配 IAM → 一律拒绝）。
    let mut schema_tokens: Arc<dyn platform_schema::TokenVerifier> =
        Arc::new(RejectAllSchemaTokens);
    // 业务模块路由的认证层需要 IAM（未配置 → 模块路由一律 401，fail closed）。
    let mut module_iam: Option<Arc<platform_iam::service::IamService>> = None;
    // SSO 入站（REQ-013）的配置源要用 `ConfigService` 实现 —— 而配置服务在本函数
    // 较后才构造（它依赖注册表）。所以先留一个句柄，等配置服务就绪再注入。
    let mut sso_iam: Option<Arc<platform_iam::service::IamService>> = None;
    let mut config_checker: Arc<dyn PermissionChecker> = Arc::new(DenyAllPermissions);
    let mut config_permissions: Arc<dyn PermissionRefresher> = Arc::new(DenyAllPermissions);
    // 出站 IdP（REQ-022）的装配依赖（IAM / RBAC 就绪后才拿得到）。
    //
    // ⚠️ **没有 IAM 就不装配** —— OAuth 令牌与登录令牌共用密钥集，
    //    没有 IAM 就没有可签发的密钥 → 不提供 `/api/oauth/*`。
    let mut oauth_iam: Option<(
        Arc<platform_iam::service::IamService>,
        Arc<platform_rbac::RbacChecker>,
        Arc<platform_rbac::RbacService>,
    )> = None;

    // 应用接入（REQ-016）的装配依赖（IAM / RBAC 就绪后才拿得到）。
    //
    // ⚠️ **没有 IAM 就不装配** —— 应用注册表按「登录用户」隔离，
    //    没有 IAM 就没有可验的登录会话 → 不提供 `/api/apps/*`。
    let mut app_iam: Option<(
        Arc<platform_iam::service::IamService>,
        Arc<platform_rbac::RbacChecker>,
    )> = None;

    // 审计 sink（REQ-014 D150）：**平台构造** sink → 以 `Arc<dyn AuditSink>`
    // **向下注入**各业务模块。
    //
    // ⚠️ 数据访问走 `platform-db` 的租户作用域入口（**不得**直连池）。
    let audit_db =
        platform_db::TenantDb::connect(&config.database_url, platform_db::DbOptions::default())
            .await
            .map_err(|err| Failure::Runtime(format!("数据库连接串非法：{err}")))?;
    // 🔑 各业务模块**只认 core 的 trait**（`platform-core/extension/audit.rs`）→
    // 它们**不依赖** `platform-audit` → 不成环（D150）。
    let audit_sink: Arc<dyn platform_core::extension::audit::AuditSink> =
        Arc::new(platform_audit::PgAuditSink::new(audit_db.clone()));

    // 认证与账号体系（REQ-007）：`/api/iam/*` + JWKS。
    //
    // **配了就必须配对**：环境变量里只要出现过一个 IAM 相关变量，就要求整套配置
    // 完整有效 —— 半配置上线（密钥缺失 / 是占位符）一律 fail-fast。
    // 一个都没配 → 平台**不提供**认证接口（服务照常起）。
    if IamConfig::is_configured(|key| std::env::var(key).ok()) {
        let iam_state = build_iam_state(&config).await?;
        app = app
            .nest("/api/iam", platform_iam::http::router(iam_state.clone()))
            .merge(platform_iam::http::jwks_router(iam_state.clone()))
            // SSO 入站（REQ-013）：`/api/iam/sso/*`（与 `/api/iam` 同一层 nest）。
            .nest(
                "/api/iam",
                platform_iam::http::sso_router(iam_state.clone()),
            );
        sso_iam = Some(iam_state.service.clone());

        // 🔑 审计 sink 注入 IAM（REQ-014 D150）：登录埋点靠它。
        if !iam_state.service.with_audit_sink(audit_sink.clone()) {
            tracing::warn!("审计 sink 已经注入过：保留先装好的实现");
        }

        // 用户与组织架构（REQ-008）：`/api/org/*`。
        //
        // **依赖 IAM**：组织数据是租户数据，接口必须验令牌（否则改个请求头就能读别人的组织树）。
        // 所以 IAM 没配 → 不提供 /api/org/*。
        let org_state =
            build_org_state(&config, iam_state.service.clone(), audit_sink.clone()).await?;
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

        // 🔑 审计 sink 注入 RBAC（REQ-014 D150）：角色 / 授权变更靠它。
        if !rbac_state.service.with_audit_sink(audit_sink.clone()) {
            tracing::warn!("审计 sink 已经注入过：保留先装好的实现");
        }

        // 出站 IdP（REQ-022）：`/api/oauth/*` + `/.well-known/oauth-authorization-server`。
        //
        // **依赖 IAM**：OAuth 令牌与登录令牌**共用同一套 ES256 密钥集**（D198）→
        // 所以 IAM 没配 → 不提供 `/api/oauth/*`（与 `/api/org/*` 同款口径）。
        //
        // ⚠️ 装配**放在这里**（IAM 分支内），但**路由挂载**要等配置服务就绪
        //    （OAuth 的运行期配置读 `platform_config`）→ 见下面 `build_oauth_state`。
        //
        // 依赖的三件套：
        //
        // | 依赖 | 实现 | 用途 |
        // |------|------|------|
        // | `IamService` | 平台既有 | 验登录令牌 + 签发 OAuth 令牌（同一套密钥） |
        // | `PermissionProvider` | `RbacService` | scope 三重交集里的「用户有效权限点」 |
        // | `PermissionChecker` | `RbacChecker` | 鉴权层判定 6 个 `oauth:*` 权限点 |
        oauth_iam = Some((
            iam_state.service.clone(),
            rbac_state.checker.clone(),
            rbac_state.service.clone(),
        ));

        // 应用接入（REQ-016）：`/api/apps/*` + 工作台 / 导航。
        //
        // 依赖的两件套：
        //
        // | 依赖 | 实现 | 用途 |
        // |------|------|------|
        // | `TokenVerifier` | `IamService` | 验登录令牌 → 租户 + 用户 |
        // | `PermissionChecker` / `PermissionRefresher` | `RbacChecker` | 判 3 个 `app:*` 权限点 + 导航过滤 |
        app_iam = Some((iam_state.service.clone(), rbac_state.checker.clone()));

        // 审计查询（REQ-014）：`/api/audit/*`。
        //
        // **依赖 IAM / RBAC**：审计记录是租户数据（要验令牌 + 判权限点）。
        // 所以 IAM 没配 → 不提供 /api/audit/*。
        let audit_state = Arc::new(platform_server::audit::AuditState {
            query: Arc::new(platform_audit::AuditQueryService::new(audit_db.clone())),
            verifier: Arc::new(platform_audit::ChainVerifier::new(audit_db.clone())),
            archive: Arc::new(platform_audit::ArchiveService::new(audit_db.clone())),
            tokens: Arc::new(platform_server::audit::IamTokenVerifier::new(
                iam_state.service.clone(),
            )),
            checker: rbac_state.checker.clone(),
            permissions: rbac_state.checker.clone(),
        });
        app = app.nest("/api/audit", platform_server::audit::router(audit_state));
        tracing::info!("审计日志已启用：/api/audit/*");

        // 权限点提供者（REQ-011）：登录响应要带上**真实**的权限点集合。
        //
        // `platform-iam` **不能**依赖 `platform-rbac`（依赖方向是 rbac → iam，
        // 反过来成环）—— 所以 IAM 只认 `platform-core` 的 `PermissionProvider` trait，
        // 由**装配层**在 RBAC 就绪之后注入。
        //
        // 注入的是 `RbacService` 本身 → 登录响应与 `GET /api/rbac/me/permissions`
        // 走**同一套**展开逻辑（两套逻辑分叉 = 前端说有、后端 403）。
        //
        // 未注入时登录**仍然成功**（`permissions` 是空数组）——
        // fail closed 的是「判定」，不是「登录」。
        let permission_provider: Arc<dyn PermissionProvider> = rbac_state.service.clone();
        if !iam_state
            .service
            .set_permission_provider(permission_provider)
        {
            tracing::warn!("权限点提供者已经注入过：保留先装好的实现");
        }

        // 配置接口的令牌校验与权限判定复用同一套 IAM / RBAC 实现：
        // 令牌主体来自 IAM，权限点判定与缓存刷新来自 `RbacChecker`
        // （同一个 `Arc` 同时实现 `PermissionChecker` 与 `PermissionRefresher`）。
        config_tokens = Arc::new(ConfigIamTokenVerifier::new(iam_state.service.clone()));
        schema_tokens = Arc::new(SchemaIamTokenVerifier::new(iam_state.service.clone()));
        module_iam = Some(iam_state.service.clone());
        config_checker = rbac_state.checker.clone();
        config_permissions = rbac_state.checker.clone();
    } else {
        tracing::warn!(
            "未检测到 IAM 环境变量（BASE_JWT_* / BASE_IAM_*）：平台不提供 /api/iam/* 与 /api/org/* 接口"
        );
    }

    // 元数据读取（REQ-012）：`/api/schema/<module>/<entity>`。
    //
    // **不新增权限点**（design §3.1）：元数据是界面的描述，不是数据；要求额外权限点
    // 会让前端「拿到元数据之前先知道该要什么权限」。无权字段在元数据里就被剔掉。
    let schema_state = Arc::new(SchemaState {
        registry: Arc::new(schema_registry),
        tokens: schema_tokens,
        checker: config_checker.clone(),
        permissions: config_permissions.clone(),
    });
    let mut app = app.nest("/api/schema", platform_schema::http::router(schema_state));
    tracing::info!("元数据读取已启用：/api/schema/<module>/<entity>");

    // 业务模块装配（REQ-012 D103）：**编译期注册**的模块由 `discover_enabled()` 决定挂哪些，
    // 路由由装配层接到 `/api/<module-id>` 下。
    //
    // ⚠️ `PlatformModule` trait **没有** router 方法（它的形状是「模块注册」，
    // 不持有数据访问层），所以这里按模块 ID 接线 —— 未接线的已启用模块会**出声**，
    // 不静默跳过（静默跳过 = 部署了却 404，最难查的一类问题）。
    let enabled_modules: Vec<&'static str> = platform_core::extension::registry::discover_enabled()
        .into_iter()
        .map(|module| module.id())
        .collect();
    for module_id in &enabled_modules {
        match *module_id {
            "sandbox" => {
                let router: Router<Arc<AppState>> =
                    sandbox::mount(&config.database_url, Path::new(&config.manifest_root))
                        .await
                        .map_err(|error| {
                            Failure::Runtime(format!("沙箱模块装配失败：{error:#}"))
                        })?;
                // 认证层由**装配层**加（模块不依赖 IAM / RBAC）：
                // 验令牌 → 建上下文 → 注入判定器，之后模块的 `require_permission` 才拿得到用户。
                let router = router.layer(axum::middleware::from_fn_with_state(
                    ModuleAuthState {
                        iam: module_iam.clone(),
                        checker: config_checker.clone(),
                        permissions: config_permissions.clone(),
                    },
                    authenticate_module,
                ));
                app = app.nest("/api/sandbox", router);
                tracing::info!("业务模块已挂载：/api/sandbox/*（sandbox）");
                if module_iam.is_none() {
                    tracing::warn!("未配置 IAM：/api/sandbox/* 的请求一律 401（fail closed）");
                }
            }
            // `org` 是**平台子系统**：它的路由由 `build_org_state` + `/api/org` 单独装配，
            // 不经过「业务模块装配点」。这里静默跳过是**明确列出**的，
            // 不是「反正不认识就不管」。
            "org" => {}
            other => {
                tracing::warn!(
                    module = other,
                    "模块已启用但没有 HTTP 装配点：its routes are not served on this service"
                );
            }
        }
    }

    let app = app.fallback(static_fallback).with_state(state);

    // 配置、品牌与 Feature Flag（REQ-010）：`/api/config/*` + `/api/feature-flags/*`。
    //
    // 品牌接口公开（登录页在认证前就要显示 logo）；其余接口需要令牌 + 权限点。
    let config_service = build_config_service(&config).await?;
    // 🔑 审计 sink 注入配置服务（REQ-014 D150）：配置写入靠它。
    if !config_service.with_audit_sink(audit_sink.clone()) {
        tracing::warn!("审计 sink 已经注入过：保留先装好的实现");
    }
    let config_state = Arc::new(platform_config::http::ConfigState {
        service: config_service.clone(),
        tokens: config_tokens.clone(),
        checker: config_checker,
        permissions: config_permissions,
    });
    let app = app.merge(platform_config::http::router(config_state));
    tracing::info!("配置与品牌已启用：/api/config/*、/api/feature-flags/*");

    // 出站 IdP（REQ-022）：`/api/oauth/*` + `/.well-known/oauth-authorization-server`。
    //
    // ⚠️ **装配顺序**：OAuth 的运行期配置读 `platform_config` → 必须等配置服务就绪
    //    （上面刚建好）才能装。
    //
    // ⚠️ **IAM 没配 → 不提供 `/api/oauth/*`**（与 `/api/org/*` 同款口径）：
    //    OAuth 令牌与登录令牌**共用同一套 ES256 密钥集**（D198）→ 没有 IAM
    //    就没有可签发的密钥，也没有可验的登录会话。
    let app = if let Some((iam, checker, permissions)) = oauth_iam {
        let oauth_state = build_oauth_state(
            &config,
            iam,
            config_service.clone(),
            audit_sink.clone(),
            checker,
            permissions,
        )
        .await?;
        let app = app
            .nest(
                "/api/oauth",
                platform_oauth::http::router(oauth_state.clone()),
            )
            .merge(platform_oauth::http::metadata_router(oauth_state.clone()));
        tracing::info!("出站 IdP 已启用：/api/oauth/*、/.well-known/oauth-authorization-server");
        app
    } else {
        tracing::warn!("未配置 IAM：平台不提供 /api/oauth/*（OAuth 令牌与登录令牌共用密钥集）");
        app
    };

    // 应用接入与应用注册表（REQ-016）：`/api/apps/*`。
    //
    // ⚠️ **装配顺序**：应用的运行期配置读 `platform_config`（每请求读）→
    //    必须等配置服务就绪（上面刚建好）才能装。
    //
    // ⚠️ **IAM 没配 → 不提供 `/api/apps/*`**（与 `/api/oauth/*` 同款口径）：
    //    应用注册表按「登录用户 / 租户」隔离，没有 IAM 就没有可验的登录会话。
    let app = if let Some((iam, checker)) = app_iam.clone() {
        let app_state = build_app_state(
            &config,
            iam,
            config_service.clone(),
            audit_sink.clone(),
            checker,
        )
        .await?;
        let app = app.nest("/api/apps", platform_app::http::router(app_state));
        tracing::info!("应用接入已启用：/api/apps/*（注册表 / 工作台 / 最近 / 导航）");
        app
    } else {
        tracing::warn!("未配置 IAM：平台不提供 /api/apps/*（应用注册表按登录用户隔离）");
        app
    };

    // 通知系统（REQ-015）：`/api/notify/*`。
    //
    // ⚠️ **与 `/api/apps/*` 同款口径**：IAM 没配 → 不提供（通知模板按租户隔离，
    //    没有 IAM 就没有可验的登录会话）。
    //
    // ⚠️ **SMTP 凭据走环境变量**（D306）：没配齐 → smtp 通道注册为「未配置」
    //    （投递返回 `NOTIFY_CHANNEL_NOT_CONFIGURED`，**不是静默成功**）。
    let app = if let Some((iam, checker)) = app_iam.clone() {
        let notify_state =
            build_notify_state(&config, iam, config_service.clone(), checker).await?;
        let app = app.nest("/api/notify", platform_notify::http::router(notify_state));
        tracing::info!("通知系统已启用：/api/notify/*（模板 / 投递记录 / 订阅偏好）");
        app
    } else {
        tracing::warn!("未配置 IAM：平台不提供 /api/notify/*（通知模板按租户隔离）");
        app
    };

    // 文件存储与导入导出（REQ-018）：`/api/storage/*`。
    //
    // ⚠️ **与 `/api/notify/*` 同款口径**：IAM 没配 → 不提供（对象按租户隔离，
    //    没有 IAM 就没有可验的登录会话）。
    //
    // ⚠️ **存储后端走环境变量**（D319）：`BASE_STORAGE_ROOT` /
    //    `BASE_STORAGE_PRESIGN_SECRET` / `BASE_STORAGE_PRESIGN_KEY_ID` 没配齐
    //    → `build_storage_state` 返回 Err，这里 warn 后**只跳过这组路由**（服务照常起）。
    let app = if let Some((iam, checker)) = app_iam.clone() {
        match build_storage_state(&config, iam, config_service.clone(), checker).await {
            Ok(storage_state) => {
                let app = app.nest(
                    "/api/storage",
                    platform_storage::http::router(storage_state),
                );
                tracing::info!(
                    "文件存储已启用：/api/storage/*（对象 / 上传会话 / 导入导出 / 预签名下载）"
                );
                app
            }
            Err(Failure::Runtime(reason)) => {
                tracing::warn!("未配置存储后端，平台不提供 /api/storage/*：{reason}");
                app
            }
            Err(_) => {
                tracing::warn!("存储系统装配失败，平台不提供 /api/storage/*");
                app
            }
        }
    } else {
        tracing::warn!("未配置 IAM：平台不提供 /api/storage/*（对象按租户隔离）");
        app
    };

    // SSO 入站（REQ-013）：把「配置源」等后设槽接到 IAM 服务上。
    //
    // 配置源用 `ConfigService::read_entries` 实现 —— 与 `GET /api/config/entries`
    // **同一套解析**（三层覆盖 + manifest 默认值），不另开读路径。
    if let Some(service) = sso_iam {
        let source = Arc::new(ConfigServiceSsoSource::new(config_service.clone()));
        if !service.set_sso_config_source(source) {
            tracing::warn!("SSO 配置源已经注入过：保留先装好的实现");
        }
        // 🔑 审计：把既有的 SSO 审计事件**桥接**到平台 sink（REQ-014 D150）。
        //
        // ⚠️ 桥接器**只**搬 `iam.sso.link.created`；
        //    两个**登录**事件码由 IAM 服务自己写（await 平台 sink）——
        //    否则一次登录会产生**两条**记录（AC-S-127 的「产生 1 条」会失败）。
        //
        // ⚠️ 既有 `SsoAuditRecorder` trait 的**签名不变**（D135）→ 这里只是
        //    换一个实现装进既有的后设槽。
        if !service.with_sso_audit_recorder(Arc::new(
            platform_iam::sso::bridge::PlatformSinkSsoRecorder::new(audit_sink.clone()),
        )) {
            tracing::warn!("SSO 审计 recorder 已经注入过：保留先装好的实现");
        }
        if let Ok(base) = std::env::var("BASE_SSO_FRONTEND_BASE_URL") {
            service.set_sso_frontend_base_url(base);
        }
        // `client_secret` **只从环境变量来**，绝不进 config / 响应 / 日志（AC-S-106）。
        if let Ok(secret) = std::env::var(platform_iam::sso::config::OIDC_CLIENT_SECRET_ENV) {
            service.set_sso_oidc_client_secret(secret);
        }
        // LDAP 服务账号（可选；不配则匿名 bind）。
        if let (Ok(dn), Ok(password)) = (
            std::env::var(platform_iam::sso::config::LDAP_SERVICE_BIND_DN_ENV),
            std::env::var(platform_iam::sso::config::LDAP_SERVICE_BIND_PASSWORD_ENV),
        ) {
            service.set_sso_ldap_service_bind(dn, password);
        }
        tracing::info!("SSO 入站已启用：/api/iam/sso/*");
    }

    // 国际化与本地化（REQ-021）：`/api/i18n/context` + `/api/i18n/fx/convert`。
    //
    // **不新增权限点**（design §5.3.1 / D115）→ 不碰权限点三件套，没有
    // 「manifest 少一项 → 进程起不来」的风险。两个接口都只要**已认证**。
    //
    // 「已注册语言」的唯一来源是 `packages/i18n/src/locales/*.json` 的文件名
    // （裁定 #16），随配置注册表一起扫出来；目录缺失时回退 `zh-CN` 并告警。
    let i18n_locales = config_service.registry().locales().clone();
    if i18n_locales.len() == 1 && i18n_locales.contains("zh-CN") {
        tracing::warn!(
            "已注册语言只有 zh-CN：协商结果只会是基准语言（前端翻译包目录 packages/i18n/src/locales 可能还没生成）"
        );
    }
    let fx_db =
        platform_db::TenantDb::connect(&config.database_url, platform_db::DbOptions::default())
            .await
            .map_err(|err| Failure::Runtime(format!("数据库连接串非法：{err}")))?;
    let i18n_state = Arc::new(platform_server::i18n::I18nState {
        settings: config_service,
        rates: Arc::new(platform_db::fx::FxRateStore::new(fx_db)),
        tokens: config_tokens,
        locales: Arc::new(i18n_locales),
    });
    let app = app.merge(platform_server::i18n::router(i18n_state));
    tracing::info!("国际化已启用：/api/i18n/context、/api/i18n/fx/convert");

    // 🔑 **全局**中间件：把来源 IP 与设备填进 `RequestContext`（REQ-014 D149）。
    //
    // 为什么必须在**顶层**：各子系统**各自**建上下文（`platform-config`、
    // `platform-rbac`、`platform-i18n`、业务模块的 `authenticate_module`）
    // → 「对所有路由生效」只能在顶层加一次。
    // 各子系统的中间件随后 `extensions().get::<RequestContext>().cloned()`
    // **继承**它。
    //
    // ⚠️ `ip` 是**客户端可控的头**（`x-forwarded-for` 的第一跳）——
    // 用途是**关联与排查**，**不是取证**（D149）。
    let app = app.layer(axum::middleware::from_fn(
        platform_server::client_info::client_info,
    ));

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
///
/// 🔑 审计落库用**平台 sink**（REQ-014 D150）：既有的 `AuditRecorder` trait
/// **签名不变**（D135）→ 这里装的是**桥接实现**（`platform-org` 自带）。
async fn build_org_state(
    config: &DeployConfig,
    iam: Arc<platform_iam::service::IamService>,
    audit_sink: Arc<dyn platform_core::extension::audit::AuditSink>,
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
        // 审计落库（REQ-014）：**桥接**到平台 sink（既有 trait 签名不变）。
        std::sync::Arc::new(platform_org::audit::PlatformSinkAuditRecorder::new(
            audit_sink,
        )),
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

/// 构建出站 IdP 的状态（数据访问 + 服务 + 配置源）。
///
/// # 依赖注入的六件套
///
/// | 依赖 | 实现 | 用途 |
/// |------|------|------|
/// | `OAuthStore` | `platform-db` 的租户作用域入口 | 5 张表的数据访问 |
/// | `IamService` | 平台既有 | 验登录令牌 + **签发** OAuth 令牌（同一套 ES256 密钥集，D198） |
/// | `OAuthConfigSource` | [`PlatformOAuthConfigSource`] | 运行期配置（**每请求读**） |
/// | `PermissionProvider` | `RbacService` | scope 三重交集里的「用户有效权限点」 |
/// | `AuditSink` | 平台 sink | 8 个 `oauth.*` 审计事件码 |
/// | `OutboundClient` | `platform-oauth` 自带 | back-channel 登出投递（**独立实现**，D220） |
async fn build_oauth_state(
    config: &DeployConfig,
    iam: Arc<platform_iam::service::IamService>,
    config_service: Arc<platform_config::ConfigService>,
    audit_sink: Arc<dyn platform_core::extension::audit::AuditSink>,
    checker: Arc<platform_rbac::RbacChecker>,
    permissions: Arc<platform_rbac::RbacService>,
) -> Result<Arc<platform_oauth::http::OAuthState>, Failure> {
    // 数据访问走 `platform-db` 的租户作用域入口（**不得**直连池）。
    let tenant_db =
        platform_db::TenantDb::connect(&config.database_url, platform_db::DbOptions::default())
            .await
            .map_err(|err| Failure::Runtime(format!("数据库连接串非法：{err}")))?;

    let service = Arc::new(platform_oauth::service::OAuthService::new(
        Arc::new(platform_oauth::store::OAuthStore::new(tenant_db)),
        iam,
        Arc::new(PlatformOAuthConfigSource::new(config_service)),
        permissions,
        audit_sink,
        // 出站 client 构造失败时**降级成永远不可达**（登出不得因此起不来，D211）。
        Arc::new(platform_oauth::outbound::OutboundClient::default()),
    ));

    // ⚠️ `refresher` 与 `checker` 是**同一个** `RbacChecker`（它同时实现两个 trait）——
    // 少了 `refresher`，用户第一次打受保护的端点必然 403（见 `OAuthState::refresher` 的注释）。
    let refresher: Arc<dyn platform_core::extension::permission::PermissionRefresher> =
        checker.clone();

    Ok(Arc::new(platform_oauth::http::OAuthState {
        service,
        checker: checker as Arc<dyn PermissionChecker>,
        refresher,
    }))
}

/// 出站 IdP 的**配置源**（每请求读 `platform_config`）。
///
/// # 为什么是「每请求读」而不是「装配时读一次」
///
/// 装配时读一次 = **配置改了永远不生效**（除非重启）→ 用户改了
/// `oauth.accessTokenTtlSeconds` 却看不到变化 → 这类「改了没反应」最难查。
/// → 每请求读；`ConfigService` 自带 epoch 失效的缓存 → 读的开销可控。
///
/// # 读不到怎么办
///
/// **返回 `Err`**（fail closed），**不得**降级成默认值 ——
/// 「读不出来就用默认」会让「配置改了不生效」变成一件看不见的事。
struct PlatformOAuthConfigSource {
    /// 平台配置服务。
    config: Arc<platform_config::ConfigService>,
}

impl PlatformOAuthConfigSource {
    /// 包一个平台配置服务。
    fn new(config: Arc<platform_config::ConfigService>) -> Self {
        Self { config }
    }
}

impl platform_oauth::config::OAuthConfigSource for PlatformOAuthConfigSource {
    fn read<'a>(
        &'a self,
        ctx: &'a RequestContext,
    ) -> platform_oauth::config::OAuthBoxFuture<
        'a,
        Result<platform_oauth::config::OAuthSettings, platform_oauth::config::OAuthConfigError>,
    > {
        Box::pin(async move {
            let keys: Vec<String> = platform_oauth::config::CONFIG_KEYS
                .iter()
                .map(|key| key.to_string())
                .collect();
            let entries = self
                .config
                .read_platform_entries(ctx, &keys)
                .await
                .map_err(|_| platform_oauth::config::OAuthConfigError)?;
            let mut settings = platform_oauth::config::OAuthSettings::default();
            for entry in entries {
                match entry.key.as_str() {
                    platform_oauth::config::CONFIG_KEY_FRONTEND_BASE_URL => {
                        if let Some(value) = entry.value.as_str() {
                            settings.frontend_base_url = value.to_string();
                        }
                    }
                    platform_oauth::config::CONFIG_KEY_ACCESS_TOKEN_TTL => {
                        if let Some(value) = entry.value.as_i64() {
                            settings.access_token_ttl_seconds = value;
                        }
                    }
                    platform_oauth::config::CONFIG_KEY_AUTHORIZATION_CODE_TTL => {
                        if let Some(value) = entry.value.as_i64() {
                            settings.authorization_code_ttl_seconds = value;
                        }
                    }
                    platform_oauth::config::CONFIG_KEY_DEVICE_CODE_TTL => {
                        if let Some(value) = entry.value.as_i64() {
                            settings.device_code_ttl_seconds = value;
                        }
                    }
                    platform_oauth::config::CONFIG_KEY_DEVICE_CODE_POLL_INTERVAL => {
                        if let Some(value) = entry.value.as_i64() {
                            settings.device_code_poll_interval_seconds = value;
                        }
                    }
                    _ => {}
                }
            }
            Ok(settings)
        })
    }
}

/// 构建应用接入的状态（数据访问 + 服务 + 配置源 + 令牌校验 + 模块 manifest）。
///
/// # 依赖注入的五件套
///
/// | 依赖 | 实现 | 用途 |
/// |------|------|------|
/// | `AppStore` | `platform-db` 的租户作用域入口 | 2 张表 + **只读** client 表 |
/// | `AppConfigSource` | [`PlatformAppConfigSource`] | 运行期配置（**每请求读**） |
/// | `AuditSink` | 平台 sink | 3 个 `app.*` 审计事件码 |
/// | `TokenVerifier` | [`AppIamTokenVerifier`] | 验登录令牌 → 租户 + 用户 |
/// | `PermissionChecker` / `PermissionRefresher` | `RbacChecker` | 3 个 `app:*` 权限点 + 导航过滤 |
/// 装配通知系统（REQ-015）。
async fn build_notify_state(
    config: &DeployConfig,
    iam: Arc<platform_iam::service::IamService>,
    config_service: Arc<platform_config::ConfigService>,
    checker: Arc<platform_rbac::RbacChecker>,
) -> Result<Arc<platform_notify::http::NotifyState>, Failure> {
    // 数据访问走 `platform-db` 的租户作用域入口（**不得**直连池）。
    let tenant_db =
        platform_db::TenantDb::connect(&config.database_url, platform_db::DbOptions::default())
            .await
            .map_err(|err| Failure::Runtime(format!("数据库连接串非法：{err}")))?;

    let service = Arc::new(platform_notify::service::NotifyService::new(
        platform_notify::store::NotifyStore::new(tenant_db),
        platform_notify::config::NotifyConfig::new(Arc::new(PlatformNotifyConfigSource::new(
            config_service,
        ))),
    ));

    // ⚠️ `refresher` 与 `checker` 是**同一个** `RbacChecker`（它同时实现两个 trait）——
    // 少了 `refresher`，用户第一次打受保护的端点必然 403（TD-047）。
    let refresher: Arc<dyn PermissionRefresher> = checker.clone();

    Ok(Arc::new(platform_notify::http::NotifyState {
        service,
        tokens: Arc::new(NotifyIamTokenVerifier::new(iam)),
        checker: checker as Arc<dyn PermissionChecker>,
        refresher,
    }))
}

/// 通知系统的令牌校验（复用 IAM 的 access token 验签）。
struct NotifyIamTokenVerifier {
    /// IAM 服务。
    inner: Arc<platform_iam::service::IamService>,
}

impl NotifyIamTokenVerifier {
    fn new(inner: Arc<platform_iam::service::IamService>) -> Self {
        Self { inner }
    }
}

impl platform_notify::http::TokenVerifier for NotifyIamTokenVerifier {
    fn verify(&self, token: &str) -> Option<platform_notify::http::VerifiedUser> {
        let claims = self.inner.verify_access_token(token).ok()?;
        Some(platform_notify::http::VerifiedUser {
            user_id: claims.subject,
            tenant_id: claims.tenant_id,
        })
    }
}

/// 通知系统的**配置源**（每请求读 `platform_config`，与 REQ-016 的 `PlatformAppConfigSource` 同款）。
struct PlatformNotifyConfigSource {
    /// 平台配置服务。
    config: Arc<platform_config::ConfigService>,
}

impl PlatformNotifyConfigSource {
    fn new(config: Arc<platform_config::ConfigService>) -> Self {
        Self { config }
    }

    /// 读一个平台级配置项为 `i64`（`number` 型）。
    async fn read_number(&self, ctx: &RequestContext, key: &str) -> Option<i64> {
        let entries = self
            .config
            .read_platform_entries(ctx, &[key.to_string()])
            .await
            .ok()?;
        entries
            .into_iter()
            .find(|entry| entry.key == key)
            .and_then(|entry| entry.value.as_i64())
    }
}

impl platform_notify::config::NotifyConfigSource for PlatformNotifyConfigSource {
    fn read_platform<'a>(
        &'a self,
        ctx: &'a RequestContext,
        key: &'a str,
    ) -> platform_notify::config::NotifyBoxFuture<'a, Option<i64>> {
        Box::pin(async move { self.read_number(ctx, key).await })
    }

    fn tenant_default_locale(
        &self,
        _ctx: &RequestContext,
    ) -> platform_notify::config::NotifyBoxFuture<'_, String> {
        // 与 `platform-config` 的 `FALLBACK_LOCALES` 一致（`registry.rs:44`）。
        // TODO(TD-057)：租户级 locale 偏好还没有存储位；本轮固定基准语言。
        Box::pin(async { "zh-CN".to_string() })
    }
}

/// 装配文件存储系统（REQ-018）。
///
/// # 依赖注入的五件套
///
/// | 依赖 | 实现 | 用途 |
/// |------|------|------|
/// | `StorageStore` | `platform-db` 的租户作用域入口 | 对象 / 上传会话 / 导入导出任务 |
/// | `StorageConfigSource` | [`PlatformStorageConfigSource`] | 运行期配置（**每请求读**） |
/// | `ObjectStore` | `LocalFsStore` | 本地 FS 真实现（D319） |
/// | `VirusScanner` | `NoopScanner` | 本轮**不做**病毒扫描（显式 `Skipped`） |
/// | `TokenVerifier` | [`StorageIamTokenVerifier`] | 验登录令牌 → 租户 + 用户 |
///
/// # ⚠️ 存储后端的三个环境变量**必须齐**
///
/// 缺一个 → 返回 `Err`，调用方 warn 后**不挂** `/api/storage/*`
/// （不是静默用默认根目录 —— 那会把对象写到一个运维不知道的地方）。
async fn build_storage_state(
    config: &DeployConfig,
    iam: Arc<platform_iam::service::IamService>,
    config_service: Arc<platform_config::ConfigService>,
    checker: Arc<platform_rbac::RbacChecker>,
) -> Result<Arc<platform_storage::http::StorageState>, Failure> {
    let env = platform_storage::config::LocalStorageEnv::from_env().ok_or_else(|| {
        Failure::Runtime(
            "缺少 BASE_STORAGE_ROOT / BASE_STORAGE_PRESIGN_SECRET / BASE_STORAGE_PRESIGN_KEY_ID"
                .to_string(),
        )
    })?;

    // 数据访问走 `platform-db` 的租户作用域入口（**不得**直连池）。
    let tenant_db =
        platform_db::TenantDb::connect(&config.database_url, platform_db::DbOptions::default())
            .await
            .map_err(|err| Failure::Runtime(format!("数据库连接串非法：{err}")))?;

    let service = Arc::new(platform_storage::service::StorageService::new(
        platform_storage::store::StorageStore::new(tenant_db),
        platform_storage::config::StorageConfig::new(Arc::new(PlatformStorageConfigSource::new(
            config_service,
        ))),
        Arc::new(platform_storage::object_store::local_fs::LocalFsStore::new(
            env.root,
        )),
        Arc::new(platform_storage::scanner::NoopScanner),
        env.presign_secret.into_bytes(),
        env.presign_key_id,
    ));

    // ⚠️ `refresher` 与 `checker` 是**同一个** `RbacChecker`（它同时实现两个 trait）——
    // 少了 `refresher`，用户第一次打受保护的端点必然 403（TD-047）。
    let refresher: Arc<dyn PermissionRefresher> = checker.clone();

    Ok(Arc::new(platform_storage::http::StorageState {
        service,
        tokens: Arc::new(StorageIamTokenVerifier::new(iam)),
        checker: checker as Arc<dyn PermissionChecker>,
        refresher,
    }))
}

/// 文件存储的令牌校验（复用 IAM 的 access token 验签）。
struct StorageIamTokenVerifier {
    /// IAM 服务。
    inner: Arc<platform_iam::service::IamService>,
}

impl StorageIamTokenVerifier {
    fn new(inner: Arc<platform_iam::service::IamService>) -> Self {
        Self { inner }
    }
}

impl platform_storage::http::TokenVerifier for StorageIamTokenVerifier {
    fn verify(&self, token: &str) -> Option<platform_storage::http::VerifiedUser> {
        let claims = self.inner.verify_access_token(token).ok()?;
        Some(platform_storage::http::VerifiedUser {
            user_id: claims.subject,
            tenant_id: claims.tenant_id,
        })
    }
}

/// 文件存储的**配置源**（每请求读 `platform_config`，与 `PlatformNotifyConfigSource` 同款）。
struct PlatformStorageConfigSource {
    /// 平台配置服务。
    config: Arc<platform_config::ConfigService>,
}

impl PlatformStorageConfigSource {
    fn new(config: Arc<platform_config::ConfigService>) -> Self {
        Self { config }
    }

    /// 读一个平台级配置项为 `i64`（`number` 型）。
    async fn read_number(&self, ctx: &RequestContext, key: &str) -> Option<i64> {
        let entries = self
            .config
            .read_platform_entries(ctx, &[key.to_string()])
            .await
            .ok()?;
        entries
            .into_iter()
            .find(|entry| entry.key == key)
            .and_then(|entry| entry.value.as_i64())
    }
}

impl platform_storage::config::StorageConfigSource for PlatformStorageConfigSource {
    fn read_platform<'a>(
        &'a self,
        ctx: &'a RequestContext,
        key: &'a str,
    ) -> platform_storage::config::StorageBoxFuture<'a, Option<i64>> {
        Box::pin(async move { self.read_number(ctx, key).await })
    }
}

async fn build_app_state(
    config: &DeployConfig,
    iam: Arc<platform_iam::service::IamService>,
    config_service: Arc<platform_config::ConfigService>,
    audit_sink: Arc<dyn platform_core::extension::audit::AuditSink>,
    checker: Arc<platform_rbac::RbacChecker>,
) -> Result<Arc<platform_app::http::AppState>, Failure> {
    // 数据访问走 `platform-db` 的租户作用域入口（**不得**直连池）。
    let tenant_db =
        platform_db::TenantDb::connect(&config.database_url, platform_db::DbOptions::default())
            .await
            .map_err(|err| Failure::Runtime(format!("数据库连接串非法：{err}")))?;

    // 导航的 `source="module"` 那一半：**全部已启用模块的菜单**。
    //
    // 扫 `modules/` + `crates/` 两处（与 `platform-rbac` / `platform-db` collect_scopes 同口径）。
    let manifests = load_module_manifests(&config.manifest_root)?;

    let service = Arc::new(platform_app::service::AppService::new(
        Arc::new(platform_app::store::AppStore::new(tenant_db)),
        Arc::new(PlatformAppConfigSource::new(config_service)),
        audit_sink,
        Arc::new(manifests),
        checker.clone() as Arc<dyn PermissionChecker>,
        Arc::new(AppIamTokenVerifier::new(iam)),
    ));

    // ⚠️ `refresher` 与 `checker` 是**同一个** `RbacChecker`（它同时实现两个 trait）——
    // 少了 `refresher`，用户第一次打受保护的端点必然 403（TD-047）。
    let refresher: Arc<dyn PermissionRefresher> = checker.clone();

    Ok(Arc::new(platform_app::http::AppState {
        service,
        checker: checker as Arc<dyn PermissionChecker>,
        refresher,
    }))
}

/// 加载**全部已启用模块**的 manifest（导航的 `source="module"` 那一半）。
///
/// 扫两处（与 `platform-rbac::collect_manifest_permissions` 的口径一致）：
///
/// | 目录 | 谁 |
/// |------|-----|
/// | `modules/<模块>/manifest.json` | 业务模块 |
/// | `crates/<crate>/manifest.json` | 平台 crate |
///
/// ⚠️ **单个 manifest 解析失败 → 启动失败**（fail-fast）：导航少一项是**静默**的
/// 功能缺失，比起不来更难查。
fn load_module_manifests(
    manifest_root: &str,
) -> Result<Vec<platform_core::extension::manifest::Manifest>, Failure> {
    let root = Path::new(manifest_root);
    let mut manifests = Vec::new();
    for directory in ["modules", "crates"] {
        let Ok(entries) = std::fs::read_dir(root.join(directory)) else {
            continue;
        };
        let mut names: Vec<PathBuf> = entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.is_dir())
            .collect();
        names.sort();
        for module_dir in names {
            let manifest_path = module_dir.join("manifest.json");
            if !manifest_path.is_file() {
                continue;
            }
            let raw = std::fs::read_to_string(&manifest_path).map_err(|error| {
                Failure::Runtime(format!("读不到 {}：{error}", manifest_path.display()))
            })?;
            let manifest = platform_core::extension::manifest::Manifest::parse_checked(&raw)
                .map_err(|issues| {
                    let detail = issues
                        .iter()
                        .map(|issue| format!("{}: {}", issue.field(), issue.reason()))
                        .collect::<Vec<_>>()
                        .join("; ");
                    Failure::Runtime(format!(
                        "{} 不符合 Schema（导航下发要用它的 menus[]）：{detail}",
                        manifest_path.display()
                    ))
                })?;
            if manifest.enabled {
                manifests.push(manifest);
            }
        }
    }
    Ok(manifests)
}

/// 应用接入的**令牌校验**（用 IAM 的验签实现）。
struct AppIamTokenVerifier {
    inner: Arc<platform_iam::service::IamService>,
}

impl AppIamTokenVerifier {
    fn new(inner: Arc<platform_iam::service::IamService>) -> Self {
        Self { inner }
    }
}

impl platform_app::tokens::TokenVerifier for AppIamTokenVerifier {
    fn verify(
        &self,
        token: &str,
    ) -> Result<platform_app::tokens::VerifiedToken, platform_app::tokens::AuthError> {
        let claims = self
            .inner
            .verify_access_token(token)
            .map_err(|_| platform_app::tokens::AuthError)?;
        Ok(platform_app::tokens::VerifiedToken {
            user_id: claims.subject,
            tenant_id: claims.tenant_id,
        })
    }
}

/// 应用接入的**配置源**（每请求读 `platform_config`）。
///
/// # 为什么是「每请求读」而不是「装配时读一次」
///
/// 装配时读一次 = **配置改了永远不生效**（除非重启）→ 用户改了
/// `app.workspace.maxApps` 却看不到变化 → 这类「改了没反应」最难查。
/// → 每请求读；`ConfigService` 自带 epoch 失效的缓存 → 读的开销可控。
///
/// # 读不到怎么办
///
/// **返回 `Err`**（fail closed），**不得**降级成默认值 ——
/// 「读不出来就用默认」会让「配置改了不生效」变成一件看不见的事。
struct PlatformAppConfigSource {
    /// 平台配置服务。
    config: Arc<platform_config::ConfigService>,
}

impl PlatformAppConfigSource {
    /// 包一个平台配置服务。
    fn new(config: Arc<platform_config::ConfigService>) -> Self {
        Self { config }
    }
}

impl platform_app::config::AppConfigSource for PlatformAppConfigSource {
    fn read<'a>(
        &'a self,
        ctx: &'a RequestContext,
    ) -> platform_app::config::AppBoxFuture<
        'a,
        Result<platform_app::config::AppSettings, platform_app::config::AppConfigError>,
    > {
        Box::pin(async move {
            let keys: Vec<String> = platform_app::config::CONFIG_KEYS
                .iter()
                .map(|key| key.to_string())
                .collect();
            let entries = self
                .config
                .read_platform_entries(ctx, &keys)
                .await
                .map_err(|_| platform_app::config::AppConfigError)?;
            let mut settings = platform_app::config::AppSettings::default();
            for entry in entries {
                match entry.key.as_str() {
                    platform_app::config::CONFIG_KEY_RECENT_COUNT => {
                        if let Some(value) = entry.value.as_i64() {
                            settings.recent_count = value;
                        }
                    }
                    platform_app::config::CONFIG_KEY_WORKSPACE_MAX_APPS => {
                        if let Some(value) = entry.value.as_i64() {
                            settings.workspace_max_apps = value;
                        }
                    }
                    platform_app::config::CONFIG_KEY_ALLOW_HTTP => {
                        if let Some(value) = entry.value.as_bool() {
                            settings.allow_http = value;
                        }
                    }
                    _ => {}
                }
            }
            Ok(settings)
        })
    }
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

/// 业务模块路由的**认证层**（装配层的职责）。
///
/// # 为什么必须在装配层
///
/// 业务模块**不依赖** `platform-iam` / `platform-rbac`（AGENTS.md 硬规则 2）——
/// 它只认 `platform-core` 的 `PermissionChecker`，判定实现由**平台通过请求扩展注入**。
/// 所以「验令牌 → 建上下文 → 注入判定器」只能在这里做。
///
/// # 这不是可选的
///
/// 漏了它，`require_permission` 拿不到用户 → **401**（fail closed），
/// 而单测里是手动注入上下文的，**测不出来**（REQ-011 的教训：单测绿 ≠ 能用）。
#[derive(Clone)]
struct ModuleAuthState {
    /// 令牌校验（未配置 IAM 时为 `None` → 一律 401）。
    iam: Option<Arc<platform_iam::service::IamService>>,
    /// 权限判定（字段级权限也用它）。
    checker: Arc<dyn PermissionChecker>,
    /// 判定缓存刷新（授权在下一个请求即生效）。
    permissions: Arc<dyn PermissionRefresher>,
}

/// 业务模块路由的认证中间件：验令牌 → 建立租户/用户上下文 → 注入判定器。
async fn authenticate_module(
    State(auth): State<ModuleAuthState>,
    mut request: Request,
    next: Next,
) -> Response {
    let instance = request
        .extensions()
        .get::<axum::extract::OriginalUri>()
        .map(|uri| uri.0.path().to_string())
        .unwrap_or_else(|| request.uri().path().to_string());
    let mut ctx = request
        .extensions()
        .get::<RequestContext>()
        .cloned()
        .unwrap_or_else(RequestContext::new);

    let Some(iam) = &auth.iam else {
        return platform_core::problem::problem_response(
            &platform_core::problem::ProblemDetails::unauthenticated(&ctx, &instance),
        );
    };
    let Some(token) = bearer_token(request.headers()) else {
        return platform_core::problem::problem_response(
            &platform_core::problem::ProblemDetails::unauthenticated(&ctx, &instance),
        );
    };
    let Ok(claims) = iam.verify_access_token(&token) else {
        return platform_core::problem::problem_response(
            &platform_core::problem::ProblemDetails::unauthenticated(&ctx, &instance),
        );
    };

    // 请求头租户优先；与令牌租户不一致 → 404（不泄露资源存在性，契约 C3）。
    if let Some(tenant) = header_value(request.headers(), "x-tenant-id") {
        ctx = ctx.with_tenant(tenant);
    }
    match ctx.tenant_id() {
        Some(tenant) if tenant != claims.tenant_id => {
            return platform_core::problem::problem_response(
                &platform_core::problem::ProblemDetails::not_found(&ctx, &instance, "resource"),
            );
        }
        Some(_) => {}
        None => ctx = ctx.with_tenant(claims.tenant_id.clone()),
    }
    ctx = ctx.with_user(claims.subject.clone());

    if auth.permissions.ensure_fresh(&ctx).await.is_err() {
        return platform_core::problem::problem_response(
            &platform_core::problem::ProblemDetails::from_context(
                platform_core::problem::CODE_INTERNAL_ERROR,
                platform_core::titles::title_of_code(platform_core::problem::CODE_INTERNAL_ERROR),
                StatusCode::INTERNAL_SERVER_ERROR.as_u16(),
                "the permission cache could not be refreshed",
                &ctx,
                &instance,
                None,
            ),
        );
    }

    request.extensions_mut().insert(ctx);
    request.extensions_mut().insert(auth.checker.clone());
    next.run(request).await
}

/// `Authorization: Bearer <token>`。
fn bearer_token(headers: &HeaderMap) -> Option<String> {
    let raw = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let trimmed = raw.trim();
    let token = trimmed
        .strip_prefix("Bearer ")
        .or_else(|| trimmed.strip_prefix("bearer "))?
        .trim();
    (!token.is_empty()).then(|| token.to_string())
}

/// 读一个请求头（去空白；空值视为没有）。
fn header_value(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

/// 读入 **阶段 1 + 阶段 2** 都通过的实体元数据注册表。
///
/// 失败即 fail-fast：元数据错了前端会渲染出错的表单，而这在启动时就能知道
/// （与 REQ-010 的 config 段同源 —— **告警就是静默放行的一种**）。
fn load_schema_registry(manifest_root: &str) -> Result<SchemaRegistry, Failure> {
    SchemaRegistry::from_manifest_root(Path::new(manifest_root)).map_err(|error| {
        Failure::Runtime(format!(
            "实体元数据注册表不成立，进程退出（fail-fast）：\n{error}\n  \
             提示：阶段 1 的问题在 manifest 的 schema 段本身（类型 / 键名 / 重复 / api 路径）；\
             阶段 2 的问题需要全局上下文（权限点是否已登记、reference 指向的实体是否存在）。"
        ))
    })
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

/// 没配 IAM 时元数据接口的令牌校验：**一律拒绝**（fail closed）。
struct RejectAllSchemaTokens;

impl platform_schema::TokenVerifier for RejectAllSchemaTokens {
    fn verify(
        &self,
        _token: &str,
    ) -> Result<platform_schema::VerifiedToken, platform_schema::AuthError> {
        Err(platform_schema::AuthError)
    }
}

/// IAM 的令牌校验适配器（元数据接口用；只转发验签，不重写语义）。
struct SchemaIamTokenVerifier {
    inner: Arc<platform_iam::service::IamService>,
}

impl SchemaIamTokenVerifier {
    fn new(inner: Arc<platform_iam::service::IamService>) -> Self {
        Self { inner }
    }
}

impl platform_schema::TokenVerifier for SchemaIamTokenVerifier {
    fn verify(
        &self,
        token: &str,
    ) -> Result<platform_schema::VerifiedToken, platform_schema::AuthError> {
        let claims = self
            .inner
            .verify_access_token(token)
            .map_err(|_| platform_schema::AuthError)?;
        Ok(platform_schema::VerifiedToken {
            user_id: claims.subject,
            tenant_id: claims.tenant_id,
        })
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
        // `title` 的唯一来源是 `TITLES`（D113）—— 响应体不得出现中文。
        title: platform_core::titles::title_of_code(code).to_string(),
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
    // 测试断言的是「没有 RUST_LOG 时的默认形态」，所以用不受环境影响的入口。
    use platform_core::log_filter_ignoring_env;
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
    fn capture_logs(filter: tracing_subscriber::EnvFilter, emit: impl FnOnce()) -> String {
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

    /// **守护测试**：`platform_config` 的 WARN 经过**生产的**过滤器后仍会输出。
    ///
    /// REQ-010 QA 抓到的 HIGH：默认部署（`BASE_LOG_LEVEL=info`、无 `RUST_LOG`）下
    /// `platform_config::*` 的 WARN 被静默丢弃 —— 旧过滤器只给
    /// `["base", "platform_server"]` 生成 directive。
    ///
    /// ⚠️ 这里用的是 `log_filter_ignoring_env` —— 它构造的 directive 集合与
    /// **没有 `RUST_LOG` 时** `init_tracing` 走的 `log_filter` **完全相同**
    /// （生产入口 `log_filter` 会优先读 `RUST_LOG`；本测试断言的是「默认部署形态」，
    /// 所以必须用不受环境影响的入口，见 [D91]）。
    ///
    /// 关键点是**不再是自定义 `EnvFilter`** —— 本 bug 正是靠「测试自己放行」藏住的。
    #[test]
    fn log_filter_captures_platform_config_warn() {
        let out = capture_logs(log_filter_ignoring_env("info"), || {
            // 与 `platform-config/src/resolver.rs:280` 同一条事件（同 target、同文案）。
            tracing::warn!(
                target: "platform_config::resolver",
                flag = "newinvoice",
                "the feature flag is not registered; returning disabled (fail closed)"
            );
        });
        assert!(
            out.contains("the feature flag is not registered"),
            "生产过滤器吞掉了 platform_config 的 WARN（默认部署会静默）：{out}"
        );
    }

    /// 根因判据：**将来新增的 crate 不需要改任何清单**也能出日志。
    ///
    /// 旧实现（逐个列 target）在这条上必然失败 —— 这正是本 bug 的形态。
    #[test]
    fn log_filter_covers_a_crate_that_nobody_listed() {
        let out = capture_logs(log_filter_ignoring_env("info"), || {
            tracing::warn!(target: "some_future_crate::module", "未来某个 crate 的日志");
        });
        assert!(
            out.contains("未来某个 crate 的日志"),
            "全局默认级别没覆盖未列出的 crate：{out}"
        );
    }

    /// `BASE_LOG_LEVEL=error` 时 info 要被挡掉，error 要留下（级别开关仍然生效）。
    #[test]
    fn log_filter_respects_the_configured_level() {
        let out = capture_logs(log_filter_ignoring_env("error"), || {
            tracing::info!(target: "platform_config::resolver", "不该出现的 info 日志");
            tracing::error!(target: "platform_config::resolver", "该出现的 error 日志");
        });
        assert!(!out.contains("不该出现的 info 日志"), "级别没生效：{out}");
        assert!(
            out.contains("该出现的 error 日志"),
            "error 日志被丢了：{out}"
        );
    }
}
