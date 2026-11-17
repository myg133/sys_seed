//! C-1 ~ C-4 四项契约检查。
//!
//! | 检查 | 判定 |
//! |------|------|
//! | C-1 | `manifest.json` 与实现一致（id / version / enabled / dependencies） |
//! | C-2 | 代码里用到的权限点都在 manifest 里声明；manifest 多余的只警告 |
//! | C-3 | 租户隔离生效：列不出别租户的数据，查不到别租户的记录 |
//! | C-4 | 事务边界：单次写全有或全无；跨表写必须显式开平台事务 |

use std::collections::BTreeSet;

use anyhow::{Result, anyhow};

use platform_core::context::RequestContext;
use platform_core::extension::repository::{ListQuery, Repository};

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
