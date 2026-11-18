//! `cargo xtask create-module`：生成开箱即过契约检查的业务模块骨架。
//!
//! # 目标
//!
//! 生成的模块**直接**通过 REQ-003 的两道关卡：
//!
//! 1. `cargo xtask check-modules` 目录规范校验通过；
//! 2. `tests/contracts.rs` 通过 `#[platform_module_contract]` 的 C-1 ~ C-4。
//!
//! 这条「咬合点」是本需求的核心验收点 —— 它证明「业务方照着 create-module
//! 就能得到一个合规模块」，而不是照着文档猜。
//!
//! # 模板约定
//!
//! 模板里用 `{{name}}` 这样的占位符（**不是** `{}`），因为模板本身大量包含
//! Rust 的 `format!` 占位符 —— 用 `{}` 会与 `format!` 的参数语法打架。

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

/// 生成结果报告。
#[derive(Debug, Clone)]
pub struct CreateModuleReport {
    /// 模块目录。
    pub module_dir: PathBuf,
    /// crate 包名。
    pub crate_name: String,
    /// 实际写入的文件。
    pub files: Vec<PathBuf>,
    /// 需要人留意的提醒。
    pub notes: Vec<String>,
}

/// 模块骨架的参数。
#[derive(Debug, Clone)]
struct ModuleSpec {
    /// 模块名（目录名 / manifest 的 id）。
    name: String,
    /// crate 包名。
    crate_name: String,
    /// lib 名（Rust 标识符）。
    lib_name: String,
    /// PascalCase 名（类型名）。
    pascal: String,
    /// SCREAMING_SNAKE 名（静态变量名）。
    screaming: String,
    /// 权限点前缀（权限点编码的第一段）。
    perm_prefix: String,
}

/// 生成一个业务模块骨架。
pub fn create_module(
    root: &Path,
    name: &str,
    perm_prefix: Option<&str>,
) -> Result<CreateModuleReport> {
    if !is_module_name(name) {
        bail!(
            "模块名 `{name}` 不合法：必须是小写 kebab-case（`^[a-z][a-z0-9]*(-[a-z][a-z0-9]*)*$`）"
        );
    }

    let prefix = perm_prefix.unwrap_or(name);
    if !is_permission_segment(prefix) {
        bail!("权限点前缀 `{prefix}` 不合法：必须是 `^[a-z][a-z0-9-]*$`");
    }

    let module_dir = root.join("modules").join(name);
    if module_dir.exists() {
        bail!("模块目录已存在：{}", module_dir.display());
    }

    let spec = ModuleSpec {
        name: name.to_string(),
        crate_name: name.to_string(),
        lib_name: to_snake_case(name),
        pascal: to_pascal_case(name),
        screaming: to_screaming_snake_case(name),
        perm_prefix: prefix.to_string(),
    };

    let files = render_files(&spec)?;
    let mut written = Vec::new();
    for (relative, content) in &files {
        let path = module_dir.join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("cannot create {}", parent.display()))?;
        }
        std::fs::write(&path, content)
            .with_context(|| format!("cannot write {}", path.display()))?;
        written.push(path);
    }

    let registered = add_workspace_member(root, &format!("modules/{name}"))?;

    let mut notes = Vec::new();
    notes.extend(format_rust_files(&written));
    if registered {
        notes.push(format!(
            "已把 `modules/{name}` 登记进根 Cargo.toml 的工作区成员"
        ));
    }
    if prefix != name {
        notes.push(format!(
            "权限点前缀 `{prefix}` 与模块 id `{name}` 不一致：契约检查 C-2 按模块 id 扫描源码，\
             这些权限点不会被核对（只会产生告警），请确认这是有意的"
        ));
    }

    Ok(CreateModuleReport {
        module_dir,
        crate_name: spec.crate_name.clone(),
        files: written,
        notes,
    })
}

/// 生成后对 Rust 源文件跑 `rustfmt`。
///
/// 模板不可能预知最终标识符的长度，而 rustfmt 的折行决策与长度相关
/// （同一个模板，短模块名一行放得下、长模块名放不下）——
/// 因此格式化交给工具本身，生成结果必定是 rustfmt-clean 的。
///
/// `rustfmt` 是 `rust-toolchain.toml` 里**固定**的组件，缺失只提示不失败
/// （工具链不完整时不该让整个脚手架不可用）。
fn format_rust_files(files: &[PathBuf]) -> Vec<String> {
    let mut notes = Vec::new();
    for file in files {
        if file.extension() != Some(std::ffi::OsStr::new("rs")) {
            continue;
        }
        match std::process::Command::new("rustfmt")
            .arg("--edition")
            .arg("2024")
            .arg(file)
            .output()
        {
            Ok(output) if output.status.success() => {}
            Ok(output) => notes.push(format!(
                "rustfmt 处理 {} 失败：{}；请手工执行 `cargo fmt --all`",
                file.display(),
                String::from_utf8_lossy(&output.stderr).trim()
            )),
            Err(err) => notes.push(format!(
                "无法运行 rustfmt（{err}）：请手工执行 `cargo fmt --all`"
            )),
        }
    }
    notes
}

/// 模块名是否合法（与平台的模块 id 规则一致）。
fn is_module_name(value: &str) -> bool {
    let mut segments = value.split('-');
    let Some(first) = segments.next() else {
        return false;
    };
    is_lower_alnum_segment(first) && segments.all(is_lower_alnum_segment)
}

/// 一段小写字母数字（首字符必须是字母）。
fn is_lower_alnum_segment(value: &str) -> bool {
    let mut chars = value.chars();
    match chars.next() {
        Some(first) if first.is_ascii_lowercase() => {
            chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        }
        _ => false,
    }
}

/// 权限点的一段：`^[a-z][a-z0-9-]*$`。
fn is_permission_segment(value: &str) -> bool {
    let mut chars = value.chars();
    match chars.next() {
        Some(first) if first.is_ascii_lowercase() => {
            chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        }
        _ => false,
    }
}

/// `demo-test` → `demo_test`。
fn to_snake_case(value: &str) -> String {
    value.replace('-', "_")
}

/// `demo-test` → `DemoTest`。
fn to_pascal_case(value: &str) -> String {
    value
        .split('-')
        .map(|segment| {
            let mut chars = segment.chars();
            match chars.next() {
                Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect()
}

/// `demo-test` → `DEMO_TEST`。
fn to_screaming_snake_case(value: &str) -> String {
    value.replace('-', "_").to_ascii_uppercase()
}

/// 把 `{{key}}` 占位符替换成实际值。
fn render(template: &str, vars: &[(&str, &str)]) -> String {
    let mut out = template.to_string();
    for (key, value) in vars {
        out = out.replace(&format!("{{{{{key}}}}}"), value);
    }
    out
}

/// 骨架的全部文件：相对路径 → 内容。
fn render_files(spec: &ModuleSpec) -> Result<Vec<(String, String)>> {
    let vars = [
        ("name", spec.name.as_str()),
        ("crate_name", spec.crate_name.as_str()),
        ("lib_name", spec.lib_name.as_str()),
        ("pascal", spec.pascal.as_str()),
        ("screaming", spec.screaming.as_str()),
        ("perm_prefix", spec.perm_prefix.as_str()),
    ];
    let files: Vec<(&str, &str)> = vec![
        ("Cargo.toml", CARGO_TOML),
        ("manifest.json", MANIFEST_JSON),
        ("backend/src/mod.rs", BACKEND_MOD_RS),
        ("backend/src/entity.rs", ENTITY_RS),
        ("backend/src/permission.rs", PERMISSION_RS),
        ("backend/src/http.rs", HTTP_RS),
        ("backend/src/repository.rs", REPOSITORY_RS),
        ("backend/src/service.rs", SERVICE_RS),
        ("frontend/routes.ts", FRONTEND_ROUTES_TS),
        ("frontend/pages/{{pascal}}Page.svelte", FRONTEND_PAGE_SVELTE),
        ("migrations/README.md", MIGRATIONS_README),
        ("tests/contracts.rs", CONTRACTS_RS),
        ("AGENTS.md", AGENTS_MD),
        ("README.md", README_MD),
    ];

    Ok(files
        .iter()
        .map(|(relative, template)| {
            let path = render(relative, &vars);
            let content = render(template, &vars);
            (path, content)
        })
        .collect())
}

/// 把模块登记进工作区成员（根 `Cargo.toml`）。
///
/// 返回是否真的改了文件（已经登记过则返回 false）。
fn add_workspace_member(root: &Path, member: &str) -> Result<bool> {
    let manifest_path = root.join("Cargo.toml");
    let raw = std::fs::read_to_string(&manifest_path)
        .with_context(|| format!("cannot read {}", manifest_path.display()))?;
    if raw.contains(&format!("\"{member}\"")) {
        return Ok(false);
    }

    let lines: Vec<&str> = raw.lines().collect();
    let insert_at = find_members_insert_at(&lines).ok_or_else(|| {
        anyhow::anyhow!(
            "根 Cargo.toml 的 members 数组不是多行格式，无法自动登记 `{member}`，请手工添加"
        )
    })?;

    let mut updated: Vec<String> = lines.iter().map(|line| (*line).to_string()).collect();
    updated.insert(insert_at, format!("    \"{member}\","));
    let mut content = updated.join("\n");
    content.push('\n');
    std::fs::write(&manifest_path, content)
        .with_context(|| format!("cannot write {}", manifest_path.display()))?;
    Ok(true)
}

/// 在 `members` 数组里找插入位置（最后一个 `modules/...` 之后）。
fn find_members_insert_at(lines: &[&str]) -> Option<usize> {
    let mut in_members = false;
    let mut insert_at = None;
    for (index, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if !in_members {
            if trimmed.starts_with("members") && trimmed.contains('[') {
                in_members = true;
            }
            continue;
        }
        if trimmed.contains("\"modules/") {
            insert_at = Some(index + 1);
        } else if trimmed.starts_with(']') {
            return insert_at.or(Some(index));
        }
    }
    insert_at
}

const CARGO_TOML: &str = r#"# 业务模块的 crate 清单。
#
# 依赖方向恒为（平台代码永不反向依赖业务模块）：
#   platform-core (定义 trait) ← platform-contracts ← modules/{{name}}/backend
#
# `platform-core` 是后端依赖，`platform-contracts` 只出现在 `[dev-dependencies]`。
[package]
name = "{{crate_name}}"
description = "业务模块 {{name}}"
# 业务模块版本独立于平台版本：manifest.json 必须与本版本一致（契约检查 C-1）
version = "0.1.0"
edition.workspace = true
rust-version.workspace = true
license.workspace = true
publish.workspace = true

[lib]
# 业务模块的后端实现放在 backend/ 下
name = "{{lib_name}}"
path = "backend/src/mod.rs"

[dependencies]
anyhow.workspace = true
# 业务模块只依赖平台核心（扩展点 trait），不依赖平台内部模块
platform-core.workspace = true

[dev-dependencies]
# 契约测试套件随平台分发：业务模块只依赖 platform-contracts 即可获得契约测试
platform-contracts.workspace = true
"#;

const MANIFEST_JSON: &str = r#"{
  "id": "{{name}}",
  "name": "{{name}} 模块",
  "version": "0.1.0",
  "enabled": true,
  "permissions": [
    { "code": "{{perm_prefix}}:item:read", "name": "查看 {{name}} 记录", "type": "menu" },
    { "code": "{{perm_prefix}}:item:write", "name": "编辑 {{name}} 记录", "type": "button" },
    {
      "code": "{{perm_prefix}}:item:approve",
      "name": "审批 {{name}} 记录",
      "type": "button",
      "condition": "$record.amount <= 10000"
    }
  ],
  "menus": [
    { "label": "{{name}}", "icon": "flask", "route": "/{{name}}", "perm": "{{perm_prefix}}:item:read" }
  ],
  "dependencies": []
}
"#;

const BACKEND_MOD_RS: &str = r#"//! {{name}} 模块入口：实现 [`PlatformModule`] 扩展点。
//!
//! # 依赖方向
//!
//! ```text
//! platform-core (定义 trait) ← platform-contracts ← modules/{{name}}/backend
//! ```
//!
//! - 后端只依赖 `platform-core`（扩展点 trait）；
//! - `platform-contracts` 只在测试里依赖（`[dev-dependencies]`）；
//! - 模块**不依赖**平台的任何内部模块。

pub mod entity;
pub mod permission;
pub mod repository;
pub mod service;

use anyhow::Result;
use platform_core::extension::platform_module::{ModuleContext, PlatformModule};

/// {{name}} 模块实现。
#[derive(Debug, Clone, Copy, Default)]
pub struct {{pascal}}Module;

impl PlatformModule for {{pascal}}Module {
    /// 必须与 `manifest.json` 的 `id` 一致（契约检查 C-1）。
    fn id(&self) -> &'static str {
        "{{name}}"
    }

    /// 必须与 `manifest.json` 的 `version` 一致（契约检查 C-1）。
    fn version(&self) -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    /// 依赖的其它模块 ID（契约检查 C-1）。
    fn dependencies(&self) -> &[&'static str] {
        &[]
    }

    /// 启动时调用一次。
    fn setup(&self, ctx: &ModuleContext) -> Result<()> {
        let _ = ctx;
        Ok(())
    }

    /// 是否启用（契约检查 C-1）。
    fn enabled(&self) -> bool {
        true
    }
}

/// {{name}} 模块实例（编译期注册用）。
static {{screaming}}_MODULE: {{pascal}}Module = {{pascal}}Module;

platform_core::register_platform_module!({{screaming}}_MODULE);
"#;

const ENTITY_RS: &str = r#"//! {{name}} 模块实体。
//!
//! 最小实体：一条记录只属于一个租户 —— 租户隔离（C-3）就是围绕它验证的。

/// {{name}} 记录。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct {{pascal}}Item {
    /// 主键。
    pub id: String,
    /// 归属租户。
    pub tenant_id: String,
    /// 标题。
    pub title: String,
    /// 版本号：用于判断一次写操作是否真的生效。
    pub revision: u32,
}
"#;

const PERMISSION_RS: &str = r#"//! {{name}} 模块权限点。
//!
//! 编码遵守 `api-conventions v1` C4：`module:resource:action`，全小写连字符分隔。
//! 权限点必须同时出现在 `manifest.json` 的 `permissions` 里 —— 契约检查 C-2 负责核对。

/// 查看权限点。
pub const PERMISSION_ITEM_READ: &str = "{{perm_prefix}}:item:read";

/// 编辑权限点。
pub const PERMISSION_ITEM_WRITE: &str = "{{perm_prefix}}:item:write";

/// 审批权限点（**携带属性级条件** `$record.amount <= 10000`）。
pub const PERMISSION_ITEM_APPROVE: &str = "{{perm_prefix}}:item:approve";

/// 本模块用到的全部权限点。
pub const PERMISSIONS: [&str; 3] = [
    PERMISSION_ITEM_READ,
    PERMISSION_ITEM_WRITE,
    PERMISSION_ITEM_APPROVE,
];
"#;

const HTTP_RS: &str = r#"//! {{name}} 模块的 HTTP 层。
//!
//! # 鉴权：每个路由都绑权限点
//!
//! `require_permission` 是**平台唯一的鉴权入口**（住在 `platform-core`）。
//! 业务模块**只调用**它 —— 自己写一个鉴权函数就是绕过（契约检查 C-5 会失败）。
//!
//! ⚠️ **参数必须是字面量**（不是常量）：C-5 要能**静态确认**
//! 「这个权限点真的有鉴权调用点」—— 参数是常量就没法确认，判为**失败**
//! （无法确认 = 不放行，fail closed）。
//!
//! # 数据范围
//!
//! 业务模块**不自己实现**数据范围：向平台要一个 `ScopeFilter`，
//! 交给 `platform-db` 的 `TenantQuery::with_scope` —— 条件在 **SQL 层**生效，
//! 而且平台**没有**「跳过范围」的开关。
//!
//! # 越权怎么表现（契约 C5）
//!
//! | 情形 | HTTP | 错误码 |
//! |------|------|--------|
//! | 缺权限点 | **403** | `INSUFFICIENT_PERMISSION` |
//! | 记录不在数据范围内 | **404** | `NOT_FOUND`（不泄露存在性） |

use axum::routing::{get, post};
use axum::{Json, Router};
use platform_core::extension::permission::require_permission;

/// 构造模块的路由。
pub fn router<S2>() -> Router<S2> {
    // 一组路由一个权限点（`route_layer` 绑在子路由上）。
    let read = Router::new().route("/item", get(list_items));
    let write = Router::new().route("/item", post(create_item));
    let approve = Router::new().route("/item/approve", post(approve_item));

    Router::new()
        .merge(read.route_layer(require_permission("{{perm_prefix}}:item:read")))
        .merge(write.route_layer(require_permission("{{perm_prefix}}:item:write")))
        .merge(approve.route_layer(require_permission("{{perm_prefix}}:item:approve")))
}

/// 列表处理器。
async fn list_items() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "items": [] }))
}

/// 新建处理器。
async fn create_item() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "ok": true }))
}

/// 审批处理器（权限点带属性条件 `$record.amount <= 10000`）。
async fn approve_item() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "approved": true }))
}
"#;

const REPOSITORY_RS: &str = r#"//! {{name}} 模块的数据访问实现（内存存储，**不引入 sqlx**）。
//!
//! # 租户隔离怎么保证
//!
//! 租户只从 `ctx` 里取（`RequestContext::tenant_id`），实现里没有任何别的途径拿到租户，
//! 因此**不可能**绕过 `ctx` 自己拼 `tenant_id` 过滤条件 —— 这是编译期保证。
//! 契约检查 C-3 再用两个租户的同名数据做行为验证：漏过滤就直接红。
//!
//! # 事务边界
//!
//! 单表存储，每次 `insert` / `update` / `delete` 都是一个原子操作
//! （先校验、后落库，失败时状态不变）—— 这是契约检查 C-4 的行为口径。

use std::collections::BTreeMap;
use std::sync::{RwLock, RwLockReadGuard, RwLockWriteGuard};

use anyhow::{Result, anyhow};
use platform_core::context::RequestContext;
use platform_core::extension::repository::{
    FilterOp, FilterSpec, List, ListQuery, Repository, SortSpec,
};

use crate::entity::{{pascal}}Item;

/// 允许排序的字段白名单。
pub const SORTABLE_FIELDS: [&str; 3] = ["id", "title", "revision"];

/// 允许过滤的字段白名单。
pub const FILTERABLE_FIELDS: [&str; 2] = ["id", "title"];

/// 内存仓储：按 `ctx` 里的租户严格过滤。
#[derive(Debug, Default)]
pub struct InMemoryRepository {
    items: RwLock<BTreeMap<String, {{pascal}}Item>>,
}

impl InMemoryRepository {
    /// 空存储。
    pub fn new() -> Self {
        Self::default()
    }

    /// 灌入初始数据（演示与测试用）。
    pub fn load(&self, items: impl IntoIterator<Item = {{pascal}}Item>) {
        let mut guard = self.write();
        for item in items {
            guard.insert(item.id.clone(), item);
        }
    }

    fn read(&self) -> RwLockReadGuard<'_, BTreeMap<String, {{pascal}}Item>> {
        self.items.read().unwrap_or_else(|err| err.into_inner())
    }

    fn write(&self) -> RwLockWriteGuard<'_, BTreeMap<String, {{pascal}}Item>> {
        self.items.write().unwrap_or_else(|err| err.into_inner())
    }
}

impl Repository<{{pascal}}Item> for InMemoryRepository {
    type Id = String;

    async fn find(&self, ctx: &RequestContext, id: &Self::Id) -> Result<Option<{{pascal}}Item>> {
        let tenant = tenant_of(ctx)?;
        let items = self.read();
        Ok(items
            .get(id)
            .filter(|item| item.tenant_id == tenant)
            .cloned())
    }

    async fn list(&self, ctx: &RequestContext, query: &ListQuery) -> Result<List<{{pascal}}Item>> {
        let tenant = tenant_of(ctx)?;
        let items = self.read();
        let mut rows: Vec<{{pascal}}Item> = items
            .values()
            .filter(|item| item.tenant_id == tenant)
            .filter(|item| matches_filters(item, &query.filters))
            .cloned()
            .collect();
        sort_rows(&mut rows, &query.sort)?;
        rows.truncate(query.effective_limit() as usize);
        Ok(List::new(rows))
    }

    async fn insert(&self, ctx: &RequestContext, item: &{{pascal}}Item) -> Result<{{pascal}}Item> {
        let tenant = tenant_of(ctx)?;
        if item.tenant_id != tenant {
            return Err(anyhow!(
                "the record tenant does not match the tenant in the request context"
            ));
        }
        let mut items = self.write();
        if items.contains_key(&item.id) {
            return Err(anyhow!("duplicate primary key: {}", item.id));
        }
        items.insert(item.id.clone(), item.clone());
        Ok(item.clone())
    }

    async fn update(&self, ctx: &RequestContext, item: &{{pascal}}Item) -> Result<{{pascal}}Item> {
        let tenant = tenant_of(ctx)?;
        if item.tenant_id != tenant {
            return Err(anyhow!(
                "the record tenant does not match the tenant in the request context"
            ));
        }
        let mut items = self.write();
        match items.get(&item.id) {
            Some(existing) if existing.tenant_id == tenant => {}
            _ => return Err(anyhow!("record not found: {}", item.id)),
        }
        items.insert(item.id.clone(), item.clone());
        Ok(item.clone())
    }

    async fn delete(&self, ctx: &RequestContext, id: &Self::Id) -> Result<()> {
        let tenant = tenant_of(ctx)?;
        let mut items = self.write();
        match items.get(id) {
            Some(existing) if existing.tenant_id == tenant => {
                items.remove(id);
                Ok(())
            }
            _ => Err(anyhow!("record not found: {id}")),
        }
    }
}

/// 从请求上下文取租户；没有租户就是上下文不完整，直接报错。
fn tenant_of(ctx: &RequestContext) -> Result<&str> {
    ctx.tenant_id()
        .ok_or_else(|| anyhow!("the tenant is not bound in the request context"))
}

/// 按排序项排序；白名单外的字段直接报错（`api-conventions v1` C7）。
///
/// 排序字段必须先声明再使用 —— 静默忽略会让调用方以为排序生效了。
fn sort_rows(rows: &mut [{{pascal}}Item], sort: &[SortSpec]) -> Result<()> {
    for spec in sort.iter().rev() {
        if !SORTABLE_FIELDS.contains(&spec.field.as_str()) {
            return Err(anyhow!("sort field {} is not sortable", spec.field));
        }
        rows.sort_by(|left, right| {
            let ordering = match spec.field.as_str() {
                "id" => left.id.cmp(&right.id),
                "title" => left.title.cmp(&right.title),
                "revision" => left.revision.cmp(&right.revision),
                _ => std::cmp::Ordering::Equal,
            };
            if spec.descending {
                ordering.reverse()
            } else {
                ordering
            }
        });
    }
    Ok(())
}

/// 所有过滤条件都命中才算通过。
fn matches_filters(item: &{{pascal}}Item, filters: &[FilterSpec]) -> bool {
    filters.iter().all(|filter| matches_filter(item, filter))
}

/// 单个过滤条件；白名单外的字段被忽略。
fn matches_filter(item: &{{pascal}}Item, filter: &FilterSpec) -> bool {
    if !FILTERABLE_FIELDS.contains(&filter.field.as_str()) {
        return true;
    }
    let value = match filter.field.as_str() {
        "id" => item.id.as_str(),
        "title" => item.title.as_str(),
        _ => return true,
    };
    match filter.op {
        FilterOp::Eq | FilterOp::In => filter.values.iter().any(|candidate| candidate == value),
        FilterOp::Ne => filter.values.iter().all(|candidate| candidate != value),
        FilterOp::Contains => filter
            .values
            .iter()
            .any(|candidate| value.contains(candidate.as_str())),
        FilterOp::Range => in_range(value, &filter.values),
    }
}

/// 区间过滤：取最小值与最大值做闭区间比较。
fn in_range(value: &str, values: &[String]) -> bool {
    let (Some(low), Some(high)) = (values.first(), values.last()) else {
        return true;
    };
    value >= low.as_str() && value <= high.as_str()
}
"#;

const SERVICE_RS: &str = r#"//! {{name}} 模块的业务服务层。
//!
//! 约定：业务模块里**只有** service 层可以调用 Repository；
//! 租户过滤由 Repository 按 `ctx` 完成，service 不再自己拼 `tenant_id` 条件。

use anyhow::{Result, anyhow};
use platform_core::context::RequestContext;
use platform_core::extension::repository::{List, ListQuery, Repository};

use crate::entity::{{pascal}}Item;
use crate::repository::InMemoryRepository;

/// 列出当前租户的记录。
pub async fn list_items(
    repo: &InMemoryRepository,
    ctx: &RequestContext,
    query: &ListQuery,
) -> Result<List<{{pascal}}Item>> {
    repo.list(ctx, query).await
}

/// 新建一条记录（主键由调用方给出）。
pub async fn create_item(
    repo: &InMemoryRepository,
    ctx: &RequestContext,
    id: &str,
    title: &str,
) -> Result<{{pascal}}Item> {
    let tenant = tenant_of(ctx)?;
    let item = {{pascal}}Item {
        id: id.to_string(),
        tenant_id: tenant.to_string(),
        title: title.to_string(),
        revision: 0,
    };
    repo.insert(ctx, &item).await
}

/// 改标题（视为一次新版本写入）。
pub async fn rename_item(
    repo: &InMemoryRepository,
    ctx: &RequestContext,
    id: &str,
    title: &str,
) -> Result<{{pascal}}Item> {
    let tenant = tenant_of(ctx)?;
    let item = {{pascal}}Item {
        id: id.to_string(),
        tenant_id: tenant.to_string(),
        title: title.to_string(),
        revision: 1,
    };
    repo.update(ctx, &item).await
}

/// 删除一条记录。
pub async fn delete_item(repo: &InMemoryRepository, ctx: &RequestContext, id: &str) -> Result<()> {
    repo.delete(ctx, &id.to_string()).await
}

/// 从请求上下文取租户；没有租户就是上下文不完整，直接报错。
fn tenant_of(ctx: &RequestContext) -> Result<&str> {
    ctx.tenant_id()
        .ok_or_else(|| anyhow!("the tenant is not bound in the request context"))
}
"#;

const FRONTEND_ROUTES_TS: &str = r#"/**
 * {{name}} 模块前端路由表。
 *
 * 路由路径遵守 `api-conventions v1` C2：全小写连字符分隔，动作放路径末段。
 * 每个路由绑定一个权限点（`api-conventions v1` C4），前端 `can()` 与后端鉴权读同一份注册表。
 */

/** 一条模块路由。 */
export interface {{pascal}}Route {
  /** 路由路径。 */
  path: string;
  /** 菜单标题。 */
  label: string;
  /** 访问该路由需要的权限点。 */
  permission: string;
}

/** {{name}} 模块的全部路由。 */
export const {{lib_name}}Routes: {{pascal}}Route[] = [
  { path: '/{{name}}', label: '{{name}}', permission: '{{perm_prefix}}:item:read' },
];

/** 按路径查找路由。 */
export function findRoute(path: string): {{pascal}}Route | undefined {
  return {{lib_name}}Routes.find((route) => route.path === path);
}
"#;

const FRONTEND_PAGE_SVELTE: &str = r#"<script lang="ts">
  import { {{lib_name}}Routes } from '../routes';

  const routes = {{lib_name}}Routes;
</script>

<section class="{{lib_name}}-module">
  <h1>{{name}} 模块</h1>
  <p>这是 {{name}} 模块的最小前端页面，作为二次开发的起点。</p>
  <ul>
    {#each routes as route (route.path)}
      <li>
        {route.label}
        <code>{route.path}</code>
        <small>权限点：{route.permission}</small>
      </li>
    {/each}
  </ul>
</section>
"#;

const MIGRATIONS_README: &str = r#"# {{name}} 模块数据库迁移

{{name}} 模块用**内存存储**实现 `Repository`，没有真实数据表，因此本目录暂时没有迁移文件。

本目录不能为空 —— `cargo xtask check-modules` 会校验 `backend/`、`frontend/`、`migrations/` 三个目录非空。

REQ-006 / REQ-007 的平台数据访问层落地后，业务模块的迁移文件放在本目录，
命名与平台的迁移目录保持一致；届时本说明文件可以删除。
"#;

const CONTRACTS_RS: &str = r#"//! {{name}} 模块契约测试：C-1 ~ C-4 由 `#[platform_module_contract]` 宏自动生成。
//!
//! 跑法：
//!
//! ```bash
//! cargo test -p {{crate_name}} -- --test-threads=2
//! ```
//!
//! C-2 的告警（manifest 声明了代码没用到的权限点）默认被测试框架捕获，
//! 需要看到时加 `--nocapture`（注意：`cargo test` 的参数分隔符 `--` 只写一次）。

use std::path::PathBuf;

use anyhow::Result;
use platform_contracts::prelude::*;
use {{lib_name}}::entity::{{pascal}}Item;
use {{lib_name}}::repository::InMemoryRepository;

/// 契约夹具：两个租户、**同名**数据，唯一区别是租户。
struct {{pascal}}Fixture;

impl ContractFixture for {{pascal}}Fixture {
    type Item = {{pascal}}Item;
    type Id = String;
    type Repository = InMemoryRepository;

    fn manifest_path(&self) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("manifest.json")
    }

    fn backend_dir(&self) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("backend")
    }

    fn seed(&self) -> Result<Vec<Seeded<{{pascal}}Item, String>>> {
        Ok(vec![
            seeded("tenant-a", "user_a@example.com", "item-1", "同名记录"),
            seeded("tenant-b", "user_b@example.com", "item-2", "同名记录"),
        ])
    }

    fn repository(&self, seeds: Vec<Seeded<{{pascal}}Item, String>>) -> InMemoryRepository {
        let repo = InMemoryRepository::new();
        repo.load(seeds.into_iter().map(|seed| seed.item));
        repo
    }

    fn tenant_of<'a>(&self, item: &'a {{pascal}}Item) -> &'a str {
        &item.tenant_id
    }

    fn fingerprint(&self, item: &{{pascal}}Item) -> String {
        format!("{}|{}|{}", item.id, item.title, item.revision)
    }

    fn write_boundary(&self) -> WriteBoundary {
        // 单表存储：每次写操作本身就是一个事务。
        WriteBoundary::new(["{{lib_name}}_item"], false)
    }

    fn fresh_item(&self, tenant_id: &str, revision: u32) -> Result<(String, {{pascal}}Item)> {
        let id = format!("item-{tenant_id}-new");
        let item = {{pascal}}Item {
            id: id.clone(),
            tenant_id: tenant_id.to_string(),
            title: format!("写操作测试 {revision}"),
            revision,
        };
        Ok((id, item))
    }
}

/// 造一条夹具数据。
fn seeded(
    tenant_id: &str,
    user_id: &str,
    id: &str,
    title: &str,
) -> Seeded<{{pascal}}Item, String> {
    Seeded {
        tenant_id: tenant_id.to_string(),
        space_id: None,
        user_id: user_id.to_string(),
        id: id.to_string(),
        item: {{pascal}}Item {
            id: id.to_string(),
            tenant_id: tenant_id.to_string(),
            title: title.to_string(),
            revision: 0,
        },
    }
}

#[platform_module_contract]
mod {{lib_name}}_contract {
    use super::*;

    /// 契约被测对象：模块实现 + 夹具。
    fn contract_subject() -> ContractSubject<{{pascal}}Fixture> {
        ContractSubject {
            module: Box::new({{lib_name}}::{{pascal}}Module),
            fixture: {{pascal}}Fixture,
        }
    }
}
"#;

const AGENTS_MD: &str = r#"# 模块：{{name}}

## 领域模型

- `{{pascal}}Item`（`backend/src/entity.rs`）：`id` / `tenant_id` / `title` / `revision`
  - 一条记录只属于一个租户，租户**只能**从 `ctx` 取

## 用到的平台扩展点

- `PlatformModule`：模块注册（`id` / `version` / `dependencies` / `setup` / `enabled`），
  实现见 `backend/src/mod.rs`
- `Repository<{{pascal}}Item>`：数据访问（`find` / `list` / `insert` / `update` / `delete`），
  实现见 `backend/src/repository.rs`

## 本模块权限点

- `{{perm_prefix}}:item:read`：查看记录（`backend/src/permission.rs` 的 `PERMISSION_ITEM_READ`）
- `{{perm_prefix}}:item:write`：编辑记录（`backend/src/permission.rs` 的 `PERMISSION_ITEM_WRITE`）

## 如何验证

```bash
cargo test -p {{crate_name}} -- --test-threads=2   # 契约测试 C-1 ~ C-4
cargo xtask check-modules                           # 目录规范校验
```

## 注意

1. **租户只能来自 `ctx`**：`Repository` 的每个方法都必须传 `ctx: &RequestContext`，
   实现里不允许自己拼 `tenant_id` 过滤条件（`api-conventions v1` C3）。
2. **权限点必须登记**：代码里用到的权限点必须同时写在 `manifest.json` 的 `permissions` 里，
   编码为 `module:resource:action`（`api-conventions v1` C4）。
3. **manifest 与实现必须一致**：`id` / `version` / `enabled` / `dependencies`
   必须与 `PlatformModule` 的实现逐项一致（C-1 会核对）。
4. **排序字段必须先声明**：不在 `SORTABLE_FIELDS` 白名单里的排序字段直接报错，
   不静默忽略（`api-conventions v1` C7）。
5. **主键重复的 `insert` 必须报错**，不得覆盖既有数据（一次写操作全有或全无，C-4 会核对）。
"#;

const README_MD: &str = r#"# {{name}} 模块（{{crate_name}}）

业务模块 {{name}}。由 `cargo xtask create-module {{name}}` 生成，开箱即通过
`cargo xtask check-modules` 与契约检查 C-1 ~ C-4。

## 能力

| 扩展点 | 实现位置 | 说明 |
|--------|----------|------|
| `PlatformModule` | `backend/src/mod.rs` | 模块注册与编译期登记 |
| `Repository<{{pascal}}Item>` | `backend/src/repository.rs` | 内存存储；严格按 `ctx` 里的租户过滤 |

## 依赖

```toml
[dependencies]
platform-core = { path = "../../crates/platform-core" }   # 扩展点 trait

[dev-dependencies]
platform-contracts = { path = "../../crates/platform-contracts" }   # 契约测试套件
```

业务模块**不依赖**平台的任何内部模块 —— 平台代码也永不反向依赖业务模块。

## 租户隔离

租户只从 `ctx: &RequestContext` 取，这是**编译期保证**：实现方没有任何别的途径拿到租户，
漏传 `ctx` 就编译不过。契约检查 C-3 再用两个租户的同名数据做行为验证。

## 契约测试

```bash
cargo test -p {{crate_name}} -- --test-threads=2
```

| 检查 | 判定 |
|------|------|
| C-1 | `manifest.json` 与实现一致（id / version / enabled / dependencies） |
| C-2 | 代码里用到的权限点都在 manifest 里声明（manifest 多余的只警告） |
| C-3 | 租户隔离生效 |
| C-4 | 单次写全有或全无；跨表写必须显式开平台事务 |

## 下一步

1. 把 `{{pascal}}Item` 换成真实业务实体（字段、校验规则）。
2. 在 `manifest.json` 里补齐本模块的权限点与菜单。
3. 实现前端页面（`frontend/pages/`），优先用 `@base/ui` 与 `@base/schema`，别自己写表格。
4. 跑 `cargo xtask capabilities` 让能力索引包含本模块。
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn module_names_follow_the_platform_id_rule() {
        assert!(is_module_name("demo"));
        assert!(is_module_name("demo-test"));
        assert!(is_module_name("a1-b2"));
        assert!(!is_module_name("Demo"));
        assert!(!is_module_name("demo_test"));
        assert!(!is_module_name("demo-"));
        assert!(!is_module_name("demo--test"));
        assert!(!is_module_name("demo-1"));
        assert!(!is_module_name(""));
    }

    #[test]
    fn permission_segments_follow_the_code_rule() {
        assert!(is_permission_segment("demo"));
        assert!(is_permission_segment("demo-test"));
        assert!(is_permission_segment("demo1"));
        assert!(!is_permission_segment("Demo"));
        assert!(!is_permission_segment("demo_test"));
        assert!(!is_permission_segment(""));
    }

    #[test]
    fn case_conversions_are_correct() {
        assert_eq!(to_snake_case("demo-test"), "demo_test");
        assert_eq!(to_pascal_case("demo-test"), "DemoTest");
        assert_eq!(to_pascal_case("demo"), "Demo");
        assert_eq!(to_screaming_snake_case("demo-test"), "DEMO_TEST");
    }

    #[test]
    fn placeholders_are_replaced_without_touching_rust_format_braces() {
        let rendered = render(
            "id = {{name}}; title = format!(\"{}\", x);",
            &[("name", "demo")],
        );
        assert_eq!(rendered, "id = demo; title = format!(\"{}\", x);");
    }

    #[test]
    fn workspace_member_is_inserted_after_the_last_modules_entry() {
        let lines = vec![
            "[workspace]",
            "members = [",
            "    \"crates/xtask\",",
            "    \"modules/sandbox\",",
            "]",
        ];
        assert_eq!(find_members_insert_at(&lines), Some(4));
    }

    #[test]
    fn workspace_member_falls_back_to_the_closing_bracket() {
        let lines = vec!["[workspace]", "members = [", "    \"crates/xtask\",", "]"];
        assert_eq!(find_members_insert_at(&lines), Some(3));
    }

    #[test]
    fn generated_files_cover_the_required_layout() {
        let spec = ModuleSpec {
            name: "demo-test".to_string(),
            crate_name: "demo-test".to_string(),
            lib_name: "demo_test".to_string(),
            pascal: "DemoTest".to_string(),
            screaming: "DEMO_TEST".to_string(),
            perm_prefix: "demo-test".to_string(),
        };
        let files = render_files(&spec).expect("骨架文件应能渲染");
        let paths: Vec<&str> = files.iter().map(|(path, _)| path.as_str()).collect();
        for required in [
            "Cargo.toml",
            "manifest.json",
            "backend/src/mod.rs",
            "backend/src/entity.rs",
            "backend/src/repository.rs",
            "backend/src/service.rs",
            "backend/src/permission.rs",
            "frontend/routes.ts",
            "frontend/pages/DemoTestPage.svelte",
            "migrations/README.md",
            "tests/contracts.rs",
            "AGENTS.md",
            "README.md",
        ] {
            assert!(paths.contains(&required), "骨架缺少 {required}");
        }

        let manifest = files
            .iter()
            .find(|(path, _)| path == "manifest.json")
            .map(|(_, content)| content.clone())
            .expect("manifest 应存在");
        let parsed: serde_json::Value =
            serde_json::from_str(&manifest).expect("manifest 应是合法 JSON");
        assert_eq!(parsed["id"], "demo-test");
        assert_eq!(parsed["permissions"][0]["code"], "demo-test:item:read");

        let permissions = files
            .iter()
            .find(|(path, _)| path == "backend/src/permission.rs")
            .map(|(_, content)| content.clone())
            .expect("permission.rs 应存在");
        assert!(permissions.contains("\"demo-test:item:read\""));
    }
}
