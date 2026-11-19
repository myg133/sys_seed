//! C-1 ~ C-6 六项契约检查。
//!
//! | 检查 | 判定 |
//! |------|------|
//! | C-1 | `manifest.json` 与实现一致（id / version / enabled / dependencies） |
//! | C-2 | 代码里用到的权限点都在 manifest 里声明；manifest 多余的只警告 |
//! | C-3 | 租户隔离生效：列不出别租户的数据，查不到别租户的记录 |
//! | C-4 | 事务边界：单次写全有或全无；跨表写必须显式开平台事务 |
//! | C-5 | 已登记权限点必须有**鉴权调用点**，且鉴权入口**全仓库唯一** |
//! | C-6 | 声明了**字段级权限**的模块，源码里必须有 `SchemaProjector::project` 调用点 |

use std::collections::BTreeSet;

use anyhow::{Context, Result, anyhow};

use platform_core::context::RequestContext;
use platform_core::extension::repository::{ListQuery, Repository};

use crate::authz_scan::{self, PLATFORM_ENTRY_FILE};
use crate::schema_scan;
use crate::fixture::{ContractFixture, ContractSubject};
use crate::manifest_scan::scan_permission_codes;
use crate::suite::{ContractViolation, ContractWarning};

/// C-1 检查 ID。
pub const C1: &str = "C-1";
/// C-2 检查 ID。
pub const C2: &str = "C-2";
/// C-3 检查 ID。
pub const C3: &str = "C-3";
/// C-4 检查 ID。
pub const C4: &str = "C-4";
/// C-5 检查 ID。
pub const C5: &str = "C-5";
/// C-6 检查 ID。
pub const C6: &str = "C-6";

/// 一项检查的结果：违例（失败）与告警（不阻断）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CheckOutcome {
    /// 收集到的违例；非空即失败。
    pub violations: Vec<ContractViolation>,
    /// 收集到的告警。
    pub warnings: Vec<ContractWarning>,
}

impl CheckOutcome {
    /// 是否完全合规。
    pub fn is_compliant(&self) -> bool {
        self.violations.is_empty()
    }

    fn violation(&mut self, check_id: &str, message: impl Into<String>) {
        self.violations
            .push(ContractViolation::new(check_id, message));
    }

    fn warning(&mut self, check_id: &str, message: impl Into<String>) {
        self.warnings.push(ContractWarning::new(check_id, message));
    }
}

/// 断言检查通过；有违例就 panic 并列出全部违例。
///
/// 告警会打印到 stderr —— 测试默认捕获输出，需要看到时加 `-- --nocapture`。
pub fn assert_compliant(check_id: &str, outcome: &CheckOutcome) {
    for warning in &outcome.warnings {
        eprintln!("warning: [{}] {}", warning.check_id, warning.message);
    }
    assert!(
        outcome.is_compliant(),
        "[{check_id}] contract violation:\n{}",
        format_violations(&outcome.violations)
    );
}

fn format_violations(violations: &[ContractViolation]) -> String {
    violations
        .iter()
        .map(|violation| format!("  - {}", violation.message))
        .collect::<Vec<_>>()
        .join("\n")
}

/// 声明为**字段级**的权限点编码（`type: "field"`）。
///
/// # 为什么 C-2 / C-5 要排除它们
///
/// 字段级权限点的**消费方不是路由**：它由 manifest 的 `schema.<entity>.fields[].permission`
/// 引用，在源码里是**动态**字符串（`checker.has(ctx, code)`），
/// 所以既不会有 `require_permission("字面量")` 调用点，也不会以字面量出现在代码里。
///
/// 它自己的契约是 **C-6**：声明了字段级权限的模块必须有 `SchemaProjector::project` 调用点。
/// 不排除的话，C-5 会对每个合法的字段级权限点报「没有鉴权调用点」——
/// 那是**误报**，而误报会让检查被关掉（关掉之后比没有更坏）。
fn field_permission_codes(
    manifest: &platform_core::extension::manifest::Manifest,
) -> BTreeSet<&str> {
    manifest
        .permissions
        .iter()
        .filter(|permission| {
            permission.kind == platform_core::extension::manifest::PermissionType::Field
        })
        .map(|permission| permission.code.as_str())
        .collect()
}

/// C-1：`manifest.json` 与实现一致。
pub fn check_c1<F: ContractFixture>(subject: &ContractSubject<F>) -> Result<CheckOutcome> {
    let mut outcome = CheckOutcome::default();
    let manifest = subject.manifest()?;
    let module = subject.module.as_ref();

    compare_text(&mut outcome, "id", &manifest.id, module.id());
    compare_text(&mut outcome, "version", &manifest.version, module.version());
    if manifest.enabled != module.enabled() {
        outcome.violation(
            C1,
            format!(
                "manifest.enabled is {} but module.enabled() is {}",
                manifest.enabled,
                module.enabled()
            ),
        );
    }

    let declared: Vec<&str> = manifest.dependencies.iter().map(String::as_str).collect();
    let implemented: Vec<&str> = module.dependencies().to_vec();
    if declared != implemented {
        outcome.violation(
            C1,
            format!(
                "manifest.dependencies is {declared:?} but module.dependencies() is {implemented:?}"
            ),
        );
    }

    Ok(outcome)
}

/// C-2：权限点声明完整。
pub fn check_c2<F: ContractFixture>(subject: &ContractSubject<F>) -> Result<CheckOutcome> {
    let mut outcome = CheckOutcome::default();
    let manifest = subject.manifest()?;

    let used = scan_permission_codes(&subject.fixture.backend_dir(), &manifest.id)?;
    let declared: BTreeSet<&str> = manifest.permission_codes().into_iter().collect();
    let field_level = field_permission_codes(&manifest);

    for code in used.iter().filter(|code| !declared.contains(code.as_str())) {
        outcome.violation(
            C2,
            format!(
                "permission point `{code}` is used in the code but not declared in manifest.json"
            ),
        );
    }
    for code in declared
        .iter()
        .filter(|code| !field_level.contains(*code))
        .filter(|code| !used.iter().any(|used| used == *code))
    {
        outcome.warning(
            C2,
            format!(
                "manifest.json declares permission point `{code}` but the code does not use it"
            ),
        );
    }

    Ok(outcome)
}

/// C-3：租户隔离生效。
pub async fn check_c3<F: ContractFixture>(subject: &ContractSubject<F>) -> Result<CheckOutcome> {
    let mut outcome = CheckOutcome::default();
    let seeds = subject.fixture.seed()?;

    let tenants: BTreeSet<String> = seeds.iter().map(|seed| seed.tenant_id.clone()).collect();
    if tenants.len() < 2 {
        outcome.violation(
            C3,
            "the fixture must provide same-named data for at least two tenants",
        );
        return Ok(outcome);
    }
    let mut tenants = tenants.into_iter();
    let tenant_a = tenants.next().expect("at least two tenants");
    let tenant_b = tenants.next().expect("at least two tenants");

    let expected_a = seeds
        .iter()
        .filter(|seed| seed.tenant_id == tenant_a)
        .count();
    let expected_b = seeds
        .iter()
        .filter(|seed| seed.tenant_id == tenant_b)
        .count();
    let id_a = primary_key::<F>(&seeds, &tenant_a);
    let id_b = primary_key::<F>(&seeds, &tenant_b);

    let repo = subject.fixture.repository(seeds);
    let ctx_a = RequestContext::new().with_tenant(&tenant_a);
    let ctx_b = RequestContext::new().with_tenant(&tenant_b);

    check_list(
        &mut outcome,
        &subject.fixture,
        &repo,
        &ctx_a,
        &tenant_a,
        expected_a,
    )
    .await?;
    check_list(
        &mut outcome,
        &subject.fixture,
        &repo,
        &ctx_b,
        &tenant_b,
        expected_b,
    )
    .await?;

    let leaked_a = repo
        .find(&ctx_a, &id_b)
        .await
        .map_err(|err| anyhow!("find() under tenant {tenant_a} failed: {err}"))?;
    if leaked_a.is_some() {
        outcome.violation(
            C3,
            format!(
                "find() under tenant {tenant_a} returned tenant {tenant_b}'s record, expected None"
            ),
        );
    }

    let leaked_b = repo
        .find(&ctx_b, &id_a)
        .await
        .map_err(|err| anyhow!("find() under tenant {tenant_b} failed: {err}"))?;
    if leaked_b.is_some() {
        outcome.violation(
            C3,
            format!(
                "find() under tenant {tenant_b} returned tenant {tenant_a}'s record, expected None"
            ),
        );
    }

    Ok(outcome)
}

/// C-4：事务边界。
pub async fn check_c4<F: ContractFixture>(subject: &ContractSubject<F>) -> Result<CheckOutcome> {
    let mut outcome = CheckOutcome::default();
    check_write_boundary(&mut outcome, &subject.fixture);

    let seeds = subject.fixture.seed()?;
    let Some(tenant) = seeds.first().map(|seed| seed.tenant_id.clone()) else {
        outcome.violation(C4, "the fixture must provide data for the write checks");
        return Ok(outcome);
    };

    let repo = subject.fixture.repository(seeds);
    let ctx = RequestContext::new().with_tenant(&tenant);
    let (id, original) = subject.fixture.fresh_item(&tenant, 0)?;
    let (_, updated) = subject.fixture.fresh_item(&tenant, 1)?;

    check_insert(&mut outcome, &subject.fixture, &repo, &ctx, &id, &original).await?;
    check_duplicate_insert(&mut outcome, &subject.fixture, &repo, &ctx, &id, &original).await?;
    check_update(&mut outcome, &subject.fixture, &repo, &ctx, &id, &updated).await?;
    check_delete::<F>(&mut outcome, &repo, &ctx, &id).await?;

    Ok(outcome)
}

fn compare_text(outcome: &mut CheckOutcome, field: &str, declared: &str, implemented: &str) {
    if declared != implemented {
        outcome.violation(
            C1,
            format!("manifest.{field} is `{declared}` but module.{field}() is `{implemented}`"),
        );
    }
}

/// 取某租户下第一条数据的主键。
fn primary_key<F: ContractFixture>(
    seeds: &[crate::fixture::Seeded<F::Item, F::Id>],
    tenant: &str,
) -> F::Id {
    seeds
        .iter()
        .find(|seed| seed.tenant_id == tenant)
        .expect("tenant must have seeded data")
        .id
        .clone()
}

/// 列表必须只含本租户的数据，且条数正确。
async fn check_list<F: ContractFixture>(
    outcome: &mut CheckOutcome,
    fixture: &F,
    repo: &F::Repository,
    ctx: &RequestContext,
    tenant: &str,
    expected: usize,
) -> Result<()> {
    let list = repo
        .list(ctx, &ListQuery::default())
        .await
        .map_err(|err| anyhow!("list() under tenant {tenant} failed: {err}"))?;

    for item in &list.items {
        let owner = fixture.tenant_of(item);
        if owner != tenant {
            outcome.violation(
                C3,
                format!("list() under tenant {tenant} returned a record owned by tenant {owner}"),
            );
        }
    }
    if list.items.len() != expected {
        outcome.violation(
            C3,
            format!(
                "list() under tenant {tenant} returned {} rows, expected {expected}",
                list.items.len()
            ),
        );
    }
    Ok(())
}

/// 跨表写必须显式开平台事务；单表写声明事务只是多余。
fn check_write_boundary<F: ContractFixture>(outcome: &mut CheckOutcome, fixture: &F) {
    let boundary = fixture.write_boundary();
    if boundary.is_cross_table() && !boundary.uses_platform_transaction {
        outcome.violation(
            C4,
            format!(
                "cross-table write (tables: {}) does not go through the platform transaction helper",
                boundary.tables.join(", ")
            ),
        );
    }
    if !boundary.is_cross_table() && boundary.uses_platform_transaction {
        outcome.warning(
            C4,
            "single-table write declares the platform transaction helper, which is unnecessary",
        );
    }
}

/// 插入必须全有：成功后立刻能读到，且内容一致。
async fn check_insert<F: ContractFixture>(
    outcome: &mut CheckOutcome,
    fixture: &F,
    repo: &F::Repository,
    ctx: &RequestContext,
    id: &F::Id,
    original: &F::Item,
) -> Result<()> {
    if let Err(err) = repo.insert(ctx, original).await {
        outcome.violation(
            C4,
            format!("insert() returned an error for a brand-new record: {err}"),
        );
        return Ok(());
    }

    match repo
        .find(ctx, id)
        .await
        .map_err(|err| anyhow!("find() after insert() failed: {err}"))?
    {
        None => outcome.violation(C4, "the record is not readable after insert() succeeded"),
        Some(item) => {
            if fixture.fingerprint(&item) != fixture.fingerprint(original) {
                outcome.violation(
                    C4,
                    "the record read back after insert() does not match the one written",
                );
            }
        }
    }
    Ok(())
}

/// 主键重复的插入必须被拒绝，且不得覆盖既有记录（全无）。
async fn check_duplicate_insert<F: ContractFixture>(
    outcome: &mut CheckOutcome,
    fixture: &F,
    repo: &F::Repository,
    ctx: &RequestContext,
    id: &F::Id,
    original: &F::Item,
) -> Result<()> {
    if repo.insert(ctx, original).await.is_ok() {
        outcome.violation(
            C4,
            "insert() with a duplicate primary key succeeded; a single write must be all-or-nothing",
        );
    }

    if let Some(item) = repo
        .find(ctx, id)
        .await
        .map_err(|err| anyhow!("find() after the rejected insert() failed: {err}"))?
    {
        if fixture.fingerprint(&item) != fixture.fingerprint(original) {
            outcome.violation(C4, "the rejected insert() overwrote the existing record");
        }
    }
    Ok(())
}

/// 更新必须全有：成功后读到的是新内容。
async fn check_update<F: ContractFixture>(
    outcome: &mut CheckOutcome,
    fixture: &F,
    repo: &F::Repository,
    ctx: &RequestContext,
    id: &F::Id,
    updated: &F::Item,
) -> Result<()> {
    if let Err(err) = repo.update(ctx, updated).await {
        outcome.violation(
            C4,
            format!("update() returned an error for an existing record: {err}"),
        );
        return Ok(());
    }

    match repo
        .find(ctx, id)
        .await
        .map_err(|err| anyhow!("find() after update() failed: {err}"))?
    {
        None => outcome.violation(C4, "the record is not readable after update() succeeded"),
        Some(item) => {
            if fixture.fingerprint(&item) != fixture.fingerprint(updated) {
                outcome.violation(
                    C4,
                    "the record read back after update() does not match the written value",
                );
            }
        }
    }
    Ok(())
}

/// 删除必须全有：成功后读不到。
async fn check_delete<F: ContractFixture>(
    outcome: &mut CheckOutcome,
    repo: &F::Repository,
    ctx: &RequestContext,
    id: &F::Id,
) -> Result<()> {
    if let Err(err) = repo.delete(ctx, id).await {
        outcome.violation(
            C4,
            format!("delete() returned an error for an existing record: {err}"),
        );
        return Ok(());
    }

    let still_there = repo
        .find(ctx, id)
        .await
        .map_err(|err| anyhow!("find() after delete() failed: {err}"))?;
    if still_there.is_some() {
        outcome.violation(C4, "the record is still readable after delete() succeeded");
    }
    Ok(())
}

/// C-5 的**不依赖 `ContractFixture`** 入口（REQ-010 D58）。
///
/// # 为什么需要它
///
/// [`check_c5`] 的形状是 `check_c5<F: ContractFixture>(subject: &ContractSubject<F>)`，
/// 而 `ContractFixture` 要求 `Repository` + `PlatformModule` —— **平台 crate 没有也不该有**。
/// 于是「跑平台的契约测试」这个验收手段根本不存在（D58）。
///
/// 这个入口只做 C-5 的**调用点**部分：
///
/// | 检查 | 说明 |
/// |------|------|
/// | 已登记权限点必须有 `require_permission("字面量")` 调用点 | 缺 → **违例**（不是告警） |
/// | 调用了但未登记 | 违例 |
/// | 参数不是字面量 | 违例（无法静态确认 → fail closed） |
/// | 模块自建鉴权入口 | 违例（扫描会被绕过） |
///
/// **不做**「鉴权入口定义全仓库唯一」那条 —— 它是**全仓库**性质，
/// 而本入口的典型用法是把 `crates/<平台 crate>/{manifest.json,src/}` 复制到临时目录做负向用例
/// （AC-S-061 步骤 3），临时目录里找不到仓库根。全仓库唯一性仍由 [`check_c5`] 覆盖。
///
/// # 参数
///
/// - `manifest_path`：被测 crate 的 `manifest.json`；
/// - `source_dir`：被测 crate 的 `src/`（平台 crate 没有 `backend/`）。
pub fn check_c5_for(
    manifest_path: &std::path::Path,
    source_dir: &std::path::Path,
) -> Result<Vec<ContractViolation>> {
    let raw = std::fs::read_to_string(manifest_path)
        .with_context(|| format!("cannot read {}", manifest_path.display()))?;
    let manifest =
        platform_core::extension::manifest::Manifest::parse_checked(&raw).map_err(|issues| {
            let detail = issues
                .iter()
                .map(|issue| format!("{}: {}", issue.field(), issue.reason()))
                .collect::<Vec<_>>()
                .join("; ");
            anyhow::anyhow!("{} is invalid: {detail}", manifest_path.display())
        })?;

    let declared: BTreeSet<&str> = manifest.permission_codes().into_iter().collect();
    let field_level = field_permission_codes(&manifest);
    let scan = authz_scan::scan_auth_calls(source_dir)?;
    let called: BTreeSet<&str> = scan.codes.iter().map(String::as_str).collect();

    let mut violations = Vec::new();
    for code in declared.iter().filter(|code| !field_level.contains(*code)) {
        if !called.contains(code) {
            violations.push(ContractViolation::new(
                C5,
                format!(
                    "the permission point `{code}` is declared in manifest.json but has no authorization call site"
                ),
            ));
        }
    }
    for code in &scan.codes {
        if !declared.contains(code.as_str()) {
            violations.push(ContractViolation::new(
                C5,
                format!(
                    "the authorization call site uses the permission point `{code}`, which is not declared in manifest.json"
                ),
            ));
        }
    }
    for call in &scan.non_literal_calls {
        violations.push(ContractViolation::new(
            C5,
            format!(
                "the authorization call site at {call} does not use a string literal; it cannot be verified statically"
            ),
        ));
    }
    for entry in &scan.custom_entries {
        violations.push(ContractViolation::new(
            C5,
            format!(
                "the custom authorization entry point at {entry} bypasses the platform entry point `{}`",
                authz_scan::AUTH_ENTRY_POINT
            ),
        ));
    }
    Ok(violations)
}

/// C-5：已登记权限点必须有**鉴权调用点**，且鉴权入口**全仓库唯一**。
///
/// | 情况 | 判定 |
/// |------|------|
/// | 已登记，且有 `require_permission("字面量")` 调用点 | ✅ |
/// | 已登记，但**没有任何**鉴权调用点 | ❌ **失败**（登记了却不执行 = 假权限） |
/// | 调用点的参数**不是字面量** | ❌ **失败**（无法静态确认 → fail closed） |
/// | 出现**第二个**鉴权入口（模块自己写一个） | ❌ **失败**（扫描会被绕过） |
///
/// ⚠️ **与 C-2 的分工**：C-2 扫「源码里出现的权限点**字面量**」，
/// 分不出「常量」与「调用点」；C-5 扫的是**调用**
/// （函数名 + 括号 + 字面量参数）—— 沿用 C-2 的逻辑会**假绿**。
pub fn check_c5<F: ContractFixture>(subject: &ContractSubject<F>) -> Result<CheckOutcome> {
    let mut outcome = CheckOutcome::default();
    let manifest = subject.manifest()?;
    let declared: BTreeSet<&str> = manifest.permission_codes().into_iter().collect();
    let field_level = field_permission_codes(&manifest);
    let backend_dir = subject.fixture.backend_dir();

    // 1) 鉴权入口的**定义处**必须全仓库唯一，且必须是平台那一个。
    match authz_scan::find_repo_root(&backend_dir) {
        Some(repo_root) => {
            let definitions = authz_scan::scan_entry_definitions(&repo_root)?;
            if definitions.len() != 1 {
                outcome.violation(
                    C5,
                    format!(
                        "the authorization entry point must be defined exactly once in the whole repository, found {}: {:?}",
                        definitions.len(),
                        definitions
                    ),
                );
            } else if !definitions[0].ends_with(PLATFORM_ENTRY_FILE) {
                outcome.violation(
                    C5,
                    format!(
                        "the authorization entry point must be defined in {PLATFORM_ENTRY_FILE}, found {}",
                        definitions[0].display()
                    ),
                );
            }
        }
        None => {
            outcome.violation(
                C5,
                format!(
                    "the platform authorization entry point ({PLATFORM_ENTRY_FILE}) could not be located from {}",
                    backend_dir.display()
                ),
            );
        }
    }

    // 2) 扫描模块自己的鉴权调用点。
    let scan = authz_scan::scan_auth_calls(&backend_dir)?;
    let called: BTreeSet<&str> = scan.codes.iter().map(String::as_str).collect();

    // 3) 已登记的权限点**必须**有调用点（登记了却不执行 = 假权限）。
    //
    //    字段级权限点例外：它们的调用点是 `SchemaProjector`（C-6 扫）。
    for code in declared.iter().filter(|code| !field_level.contains(*code)) {
        if !called.contains(code) {
            outcome.violation(
                C5,
                format!(
                    "the permission point `{code}` is declared in manifest.json but has no authorization call site"
                ),
            );
        }
    }

    // 4) 调用了但没登记 → 失败（C-2 也覆盖；这里给出更直接的提示）。
    for code in &scan.codes {
        if !declared.contains(code.as_str()) {
            outcome.violation(
                C5,
                format!(
                    "the authorization call site uses the permission point `{code}`, which is not declared in manifest.json"
                ),
            );
        }
    }

    // 5) 参数不是字面量 → **失败**（无法静态确认 = 不放行）。
    for call in &scan.non_literal_calls {
        outcome.violation(
            C5,
            format!(
                "the authorization call site at {call} does not use a string literal; it cannot be verified statically"
            ),
        );
    }

    // 6) 自定义鉴权入口 → **失败**（扫描会被绕过）。
    for entry in &scan.custom_entries {
        outcome.violation(
            C5,
            format!(
                "the custom authorization entry point at {entry} bypasses the platform entry point `{}`",
                authz_scan::AUTH_ENTRY_POINT
            ),
        );
    }

    Ok(outcome)
}

/// 从 manifest 收集「声明了字段级权限」的字段：`(实体, 字段下标, 字段键, 权限点)`。
///
/// 实体按 manifest 里的键序（`BTreeMap`）遍历 —— 错误输出因此是确定的。
fn declared_permission_fields(
    manifest: &platform_core::extension::manifest::Manifest,
) -> Vec<(String, usize, String, String)> {
    let Some(schemas) = &manifest.schema else {
        return Vec::new();
    };
    schemas
        .iter()
        .flat_map(|(entity, spec)| {
            spec.fields
                .iter()
                .enumerate()
                .filter_map(|(index, field)| {
                    field.permission().map(|code| {
                        (entity.clone(), index, field.key.clone(), code.to_string())
                    })
                })
        })
        .collect()
}

/// C-6 的**不依赖 `ContractFixture`** 入口（照 C-5 的 `check_c5_for` 先例，D58）。
///
/// # 判据（design §4.4 已选定）
///
/// 若 `manifest` 声明了任何带 `permission` 的字段，则 `source_dir` 里**必须出现**
/// `SchemaProjector::project` 的调用。
///
/// 这是「**至少一处**」判据：
///
/// | 情形 | 判定 |
/// |------|------|
/// | 没有声明字段级权限 | ✅ 通过（无声明即无约束） |
/// | 声明了，且源码里有调用点 | ✅ 通过 |
/// | 声明了，源码里**没有**调用点（含只有注释 / 字符串里出现） | ❌ 失败 |
///
/// ⚠️ **局限**：「3 个 handler 只有 1 个调用」**能过**。静态判定「哪个 handler 返回哪个实体」
/// 需要类型推导，在源码扫描里不可靠 —— 会误报，进而被人关掉，
/// **关掉之后比没有更坏**。主路径由 AC-S-080 的列表接口用例兜住。
pub fn check_c6_for(
    manifest_path: &std::path::Path,
    source_dir: &std::path::Path,
) -> Result<Vec<ContractViolation>> {
    let raw = std::fs::read_to_string(manifest_path)
        .with_context(|| format!("cannot read {}", manifest_path.display()))?;
    let manifest =
        platform_core::extension::manifest::Manifest::parse_checked(&raw).map_err(|issues| {
            let detail = issues
                .iter()
                .map(|issue| format!("{}: {}", issue.field(), issue.reason()))
                .collect::<Vec<_>>()
                .join("; ");
            anyhow::anyhow!("{} is invalid: {detail}", manifest_path.display())
        })?;

    let declared = declared_permission_fields(&manifest);
    if declared.is_empty() {
        return Ok(Vec::new());
    }

    let scan = schema_scan::scan_project_calls(source_dir)?;
    if scan.has_call() {
        return Ok(Vec::new());
    }

    Ok(declared
        .into_iter()
        .map(|(entity, index, field, code)| {
            ContractViolation::new(
                C6,
                format!(
                    "the module `{module}` declares the field-level permission `{code}` on entity `{entity}` field `{field}` (schema.{entity}.fields[{index}].permission), but there is no project call site (`SchemaProjector::project`) in the module source",
                    module = manifest.id
                ),
            )
        })
        .collect())
}

/// C-6：声明了字段级权限就必须有**投影调用点**（判据 = 「至少一处」，见 [`check_c6_for`]）。
pub fn check_c6<F: ContractFixture>(subject: &ContractSubject<F>) -> Result<CheckOutcome> {
    let mut outcome = CheckOutcome::default();
    // 先解析一次：manifest 坏了要让检查**失败**，不是静默通过。
    subject.manifest()?;
    let violations =
        check_c6_for(&subject.fixture.manifest_path(), &subject.fixture.backend_dir())?;
    for violation in violations {
        outcome.violation(C6, violation.message);
    }
    Ok(outcome)
}
