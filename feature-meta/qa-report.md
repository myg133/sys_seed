# 验证报告 —— Pre-merge

- 需求: REQ-003 扩展点框架与契约测试
- 分支: feature/REQ-003 @ `04b57d39dc92ebf336584a91d65e7ed658a7fb58`
- 基线: develop @ `eacff21`
- 验证者: QA Agent（独立复现，未采信 Dev 自验报告的数字与结论）
- 工具链: rustc 1.98.1 / cargo 1.98.1 / pnpm / bun
- 日期: 2026-11-18
- 结论: **PASS-WITH-RISK**

> AC-S-003 与设计摘要 §7 的 7 项交付全部落地，且**契约测试确实会咬人** —— C-1~C-4 每项都由本轮独立造违规复现。
> 扣分项集中在两个「门禁」本身有盲区（见问题 #1、#2），以及扩展点冻结签名的契约后果未登记（#3）。

---

## 追溯

| AC | 实现 | 测试 | 结论 |
|----|------|------|------|
| AC-S-003 扩展点契约随平台分发（业务模块只依赖 platform-contracts 即可获得契约测试） | `crates/platform-contracts/`（套件 C-1~C-4）、`crates/platform-contracts-macros/`（`#[platform_module_contract]`）、`modules/sandbox/Cargo.toml`（`platform-contracts` 只在 `[dev-dependencies]`） | `modules/sandbox/tests/contracts.rs`（宏生成 4 个契约测试）+ 4 项独立破坏性验证 | **PASS** |
| 父 AC-031 契约随平台分发 | 同上 | 同上 | **PASS** |
| 父 AC-032 租户上下文随契约强制传递 | `crates/platform-core/src/extension/repository.rs:26-61`（每个方法强制 `ctx: &RequestContext`） | `checks.rs:142-219`（C-3 行为验证）+ 破坏性验证 | **PASS** |
| 父 AC-033 权限点登记与一致性 | `crates/platform-contracts/src/manifest_scan.rs` + `crates/platform-core/src/extension/manifest.rs` | `checks.rs:111-139`（C-2）+ 双向破坏性验证 | **PASS**（有保真度限制，见 #4） |
| 父 AC-034 事务边界约定 | `crates/platform-core/src/extension/transaction.rs`、`crates/platform-contracts/src/checks.rs:222-243` | `checks.rs:303-320`（跨表写）+ 破坏性验证 | **PASS** |
| 设计摘要 §2 五个扩展点 trait 签名冻结 | `crates/platform-core/src/extension/{platform_module,repository,auditable,notification,workflow}.rs` | `cargo test -p platform-core --lib`（39） | **PASS-WITH-RISK**（方法/参数/类型逐项一致；仅 async 写法偏离，契约后果见 #3） |
| 设计摘要 §3 inventory 编译期注册 | `crates/platform-core/src/extension/registry.rs` | `registry::tests::*`（2） | **PASS** |
| 设计摘要 §5 manifest JSON Schema + 解析器 | `crates/platform-core/src/extension/manifest_schema.rs`、`manifest.rs`；导出 `capabilities/schema/manifest.schema.json` | `manifest_schema::tests::*`（3）、`manifest::tests::*`（6） | **PASS**（schema 与设计摘要 §5 逐字段一致；解析器与 schema 有两处判定差异，见 #5） |
| 设计摘要 §6 `xtask check-modules` 五项规则 | `crates/xtask/src/modules.rs` | `modules::tests::*`（10）+ 5 项手工破坏验证 | **PASS-WITH-RISK**（五项规则逐项验证通过；结构级 manifest 违规漏报，见 #1） |
| 设计摘要 §1 / ADR D7 依赖方向单向 | `crates/xtask/src/deps.rs` | `deps::tests::*`（3）+ 独立绕过实验 | **PASS-WITH-RISK**（可复现绕过，见 #2） |
| 设计摘要 §7 示例模块 `modules/sandbox/` | `modules/sandbox/**`（13 个文件） | `cargo test -p sandbox`（4/4）+ `cargo xtask check-modules` | **PASS**（见 #6） |
| REQ-002 的 24 个原有测试不被打破 | — | 逐个测试名比对：24/24 仍在且全绿 | **PASS** |

---

## 破坏性验证结果

全部由 QA 自行造违规 → 跑测试 → 逐字节还原（`git checkout --` / 备份回写 + sha256 校验），
**未采信 Dev 报告中的结论**。

| 检查 | 制造了什么违规 | 期望 | 实际 | 还原确认 |
|------|----------------|------|------|----------|
| **C-1** manifest 与实现一致 | `modules/sandbox/manifest.json` 的 `id`：`sandbox` → `wrong-name` | 测试失败并指出两者各是什么 | ✅ `contract_c1_manifest_matches_implementation` FAILED，报 ``manifest.id is `wrong-name` but module.id() is `sandbox` ``（`checks.rs:62`） | ✅ sha256 一致，`git status` 为空 |
| **C-2** 权限点声明完整（正向） | 从 manifest 删掉 `sandbox:demo:write`（代码里仍在用） | 测试失败（未声明） | ✅ FAILED，报 ``permission point `sandbox:demo:write` is used in the code but not declared in manifest.json`` | ✅ sha256 一致，`git status` 为空 |
| **C-2** 权限点声明完整（**反向**） | manifest 加一个代码里没用的 `sandbox:unused:read` | **只警告，不失败** | ✅ **exit 0，4/4 全绿**，stderr 仅打印 `warning: [C-2] manifest.json declares permission point `sandbox:unused:read` but the code does not use it` | ✅ sha256 一致，`git status` 为空 |
| **C-3** 租户隔离 | 让 `InMemoryRepository` 的 `find`/`list` 忽略 `ctx.tenant_id()` | 测试失败并报出跨租户的具体记录 | ✅ FAILED，报 6 条越权细节：`list() under tenant tenant-a returned a record owned by tenant tenant-b` ×2、`returned 2 rows, expected 1` ×2、`find() under tenant tenant-a returned tenant tenant-b's record, expected None` ×2 | ✅ sha256 一致，`git status` 为空 |
| **C-4** 事务边界 | 夹具 `WriteBoundary::new(["sandbox_demo_item"], false)` → `(["sandbox_demo_item","sandbox_demo_log"], false)` | 测试失败并指出 cross-table write | ✅ FAILED（隔离复跑，C-1/C-2/C-3 仍绿），报 ``cross-table write (tables: sandbox_demo_item, sandbox_demo_log) does not go through the platform transaction helper`` | ✅ sha256 一致，`git status` 为空 |
| **C-3 守卫** | 夹具只给 1 个租户 | 应直接判失败（防止 C-3 失去意义） | ✅ FAILED，报 `the fixture must provide same-named data for at least two tenants` | ✅ sha256 一致，`git status` 为空 |
| **check-modules 规则 1** | 探针模块删 `AGENTS.md` | 拦住并列缺失项 | ✅ exit 1，`缺少文件 AGENTS.md` | ✅ 探针目录已删，`git status` 为空 |
| **check-modules 规则 2** | 探针 `manifest.json` 的 `version` → `0.1` | 给出字段路径与原因 | ✅ exit 1，`manifest.json 不符合 Schema：version：must match ^\d+\.\d+\.\d+$` | ✅ |
| **check-modules 规则 3** | 探针 `manifest.json` 的 `id` → `wrong-name` | 说明应以哪个为准 | ✅ exit 1，``manifest.id 为 `wrong-name`，与目录名 `qa-probe` 不一致（应以目录名为准）`` | ✅ |
| **check-modules 规则 4** | 清空探针的 `backend/` `frontend/` `migrations/` | 指出空目录 | ✅ exit 1，`目录 backend/ 为空` / `frontend/` / `migrations/` | ✅ |
| **check-modules 规则 5** | 删探针 `README.md` | 列出缺失 | ✅ exit 1，`缺少文件 README.md` | ✅ |
| **依赖方向** | `crates/platform-core/Cargo.toml` 加 `sandbox = { path = "../../modules/sandbox" }` | 检查失败 | ✅ 检查报 violations=1；**且 cargo 自身先报 `cyclic package dependency`** | ✅ `git status` 为空 |
| **依赖方向** | `crates/platform-contracts/Cargo.toml` 加 `sandbox = { path = "../../modules/sandbox" }`（无循环，cargo 接受） | 检查失败 | ✅ 检查报 violations=1；`cargo tree -p platform-contracts` 确实出现 `sandbox v0.1.0 (modules/sandbox)` | ✅ `git status` 为空 |
| **依赖方向** | **绕过实验**：`platform-contracts` 写 `sandbox.workspace = true`，路径放到根 `Cargo.toml` 的 `[workspace.dependencies]` | 应失败 | ❌ **检查报 violations=0，但 `cargo tree -p platform-contracts` 确实出现 `sandbox`** → 见问题 #2 | ✅ `git status` 为空 |
| **依赖方向** | `crates/platform-contracts-macros/Cargo.toml` 加 `sandbox = { path = "../../modules/sandbox" }` | 应失败 | ❌ **检查报 violations=0**，`cargo tree -p platform-contracts-macros` 出现 `sandbox` → 见问题 #2 | ✅ `git status` 为空 |
| **manifest 结构** | 探针 `manifest.json` = `{}` | 应判不符合 Schema | ❌ **exit 0，「模块目录校验通过」** → 见问题 #1 | ✅ |
| **manifest 结构** | 探针 `manifest.json` = `{"name":"Probe","enabled":true}` / 含未知字段 `"bogus":1` | 应判不符合 Schema | ❌ 两种均 exit 0 → 见问题 #1 | ✅ |
| **C-2 保真度** | 加常量 `PERMISSION_DEMO_DELETE="sandbox:demo:delete"` + manifest 声明，但 `service.rs`/`repository.rs` 0 处引用 | — | ⚠️ **C-2 全绿、无告警** → 见问题 #4 | ✅ `git status` 为空 |

---

## 测试

- **Rust 64/64**（`cargo test --workspace -- --test-threads=2`，exit 0，0 failed）
  - `platform-core` lib 39 ｜ `platform-contracts` lib 8 ｜ `xtask` lib 13 ｜ `sandbox` `tests/contracts.rs` 4 ｜ 其余 0 ｜ doctest 4 ignored
  - **REQ-002 的 24 个原有测试逐个比对测试名，全部仍在且全绿**（`platform-core` 14 + `platform-contracts` 3 + `xtask` 7）
  - Dev 改动的 2 个 xtask 夹具（`make_module` / `missing_entries_are_listed`）：**断言一行未改**，
    `missing_entries_are_listed` 仍断言 `status.problems().len() == 5` 与 `missing_files == ["AGENTS.md","README.md"]`、
    `missing_dirs == ["backend","frontend","migrations"]`，且测试通过 → **断言强度未被削弱**
- **前端 23/23**（`pnpm -r test`，exit 0；auth/hooks/i18n/schema 各 3、sdk 2、theme 2、ui 3、apps/shell 4）
- **E2E 1/1**（`bun run test:e2e`，chromium，exit 0）
- `cargo clippy --workspace --all-targets -- -D warnings` → **0 告警**（exit 0）
- `cargo fmt --all --check` → **通过**（exit 0）
- `cargo xtask check-modules` → 正常态 `模块目录校验通过：1 个模块 / ✓ sandbox`，exit 0

### 依赖核对

| 项 | 实测 |
|----|------|
| host 解析树唯一 crate 名 | **90** = 5 个 workspace 成员 + **85 个第三方** |
| 本期新增第三方依赖 | **仅 `inventory 0.3.25`**（传递依赖只有 `rustversion 1.0.23`），`Cargo.lock` diff 证实 |
| `platform-contracts-macros` 第三方依赖 | **0**（`cargo tree -p platform-contracts-macros` 输出只有一个节点），源码只用 `proc_macro` 标准库 |
| 禁止清单 | `sqlx` / `reqwest` / `aws-sdk` / `opendal` / `jsonwebtoken` / `diesel` / `async-trait` 在 `Cargo.lock` 中 **均 0 命中** |
| `cargo tree -p platform-core` / `-p platform-contracts` | 依赖图中**无任何 `modules/*` 路径**（当前干净状态） |

### 独立验证的两个工具链说法

| 说法 | 独立验证结果 |
|------|--------------|
| Rust 1.98 上 `macro_rules!` 属性宏仍 unstable | ✅ **证实**。最小样例编译失败：`error: cannot find attribute `my_attr` in this scope` + `` `my_attr` exists, but has no `attr` rules ``。稳定通道拿不到被标注的 item → 过程宏是必需方案 |
| `async fn` 写在公开 trait 上触发 `async_fn_in_trait` lint | ✅ **证实**。Rust 1.98.1 报 `warning: use of 'async fn' in public traits is discouraged…`；加 `-D warnings` 后变 error。rustc 的 help 建议正是 `fn work(&self) -> impl std::future::Future<Output = u32> + Send;` |

### trait 展开写法的判定

| 维度 | 结论 |
|------|------|
| 与设计摘要 §2 的方法/参数/类型一致性 | ✅ **逐项一致**，无擅自加方法、改参数、换类型。5 个 trait 全部核对 |
| 调用语义 | ✅ **保持**。实测：`async fn` 实现被 trait 接受；调用方写法不变（`repo.find(&ctx, &id).await`）；返回 future 经 `assert_send` 断言确为 `Send`（可安全交给 tokio 任务） |
| `Send` 约束是否正确 | ✅ **正确**。`+ Send` 与 `async_trait` 默认行为一致，是这个展开写法的关键收益 |
| 偏离是否合理 | ✅ **合理且必要**：`async-trait` 会新增第三方依赖，违反「本期只新增 `inventory`」；`async fn in trait` 在 CI 的 `-D warnings` 下失败。两难下 rustc 自己推荐的展开写法是唯一解 |
| 但有一处契约后果 | ⚠️ 展开写法**不是 dyn-compatible**，而设计摘要 §2 的 `#[async_trait]` 是。实测 `Vec<Box<dyn NotificationSender>>` 编译失败：`error[E0038]: the trait NotificationSender is not dyn compatible … method 'send' references an 'impl Trait' type in its return type`。→ 见问题 #3 |

---

## 问题（按严重度）

| # | 级别 | 描述 | 证据 | 建议 |
|---|------|------|------|------|
| **1** | **中** | **`xtask check-modules` 的「manifest 符合 JSON Schema」会静默吞掉结构级违规。**`check_manifest_schema` 在 `Manifest::from_path` 解析失败时返回空列表，而 serde 层面的失败（缺必备字段、未知字段 —— 后者对应 schema 的 `additionalProperties: false`）因此完全不被报告。设计摘要 §6 要求「manifest.json 符合 §5 的 JSON Schema → 给出字段路径与原因」，实际只覆盖了字段级规则（id / version / permission code / route），覆盖不了结构级规则。 | `crates/xtask/src/modules.rs:192-208`（`let Ok(manifest) = Manifest::from_path(path) else { return Vec::new(); }`）。实测探针：`manifest.json` = `{}` → `模块目录校验通过：2 个模块`，exit 0；`{"name":"Probe","enabled":true}` → exit 0；含未知字段 `"bogus":1` → exit 0。**缓解**：契约测试是第二道防线 —— 把 `modules/sandbox/manifest.json` 改成 `{}` 后 C-1/C-2 均 panic 报 `manifest.json is not valid JSON: missing field 'id' at line 1 column 2`；但只有实现了 `#[platform_module_contract]` 的模块才有这道防线。 | `check_manifest_schema` 在 `Manifest::from_path` 失败时，把 serde 错误作为一条 `manifest_issues` 报出（字段路径可取 serde 的 span）。补一条测试：`manifest.json` = `{}` 必须让 `check_modules` 判失败 |
| **2** | **中** | **依赖方向检查（ADR D7 硬约束）不是依赖图扫描，是两个文件的字符串扫描，且可被复现绕过。**`dependency_direction_violations` 逐行读 `crates/platform-core/Cargo.toml` 与 `crates/platform-contracts/Cargo.toml`，只找含 `modules/` 的行。两个盲区：① workspace 继承写法下路径不在被扫文件里 → 完全漏检；② `PLATFORM_CRATES` 不含本期新增的 `platform-contracts-macros` → 该 crate 不受任何约束。 | `crates/xtask/src/deps.rs:19`（`PLATFORM_CRATES = ["platform-core", "platform-contracts"]`）、`:25-47`（按行 `contains("modules/")`）。实测：`platform-contracts` 写 `sandbox.workspace = true` + 根 `Cargo.toml` 的 `[workspace.dependencies]` 声明路径 → `cargo tree -p platform-contracts` 显示 `sandbox v0.1.0 (modules/sandbox)`，但检查报 `violations=0`；`platform-contracts-macros` 直接加 `sandbox = { path = "../../modules/sandbox" }` → `cargo tree` 显示 `sandbox`，检查同样报 `violations=0`。**缓解**：直接写 `path = ".../modules/..."` 能被抓到（实测 violations=1）；`platform-core → modules/sandbox` 因成环被 cargo 自己拒绝（`cyclic package dependency`）。真正的盲区 = 非循环 + workspace 继承 / 未列入清单的 crate。CI 侧由 `cargo test --workspace` 里的 `deps::tests::the_real_workspace_has_no_reverse_dependency` 承担，因此上述两种情况**在 CI 里也不会红** | 改为读 `cargo metadata` 的 resolve 图做拓扑判定（而非文本匹配），并把 `platform-contracts-macros` 纳入平台 crate 清单；补两条测试：workspace 继承写法、未列入清单的 crate |
| **3** | **低** | **扩展点冻结签名的 dyn 兼容性变更未登记。**设计摘要 §2 冻结的是 `#[async_trait]` 形态（dyn-compatible），实现改为 RPITIT 展开写法后**不是** dyn-compatible。这个契约后果只写在 Dev 自验报告的风险 #1 里，没有进入 design-summary §2 的冻结签名或 `api-conventions/v1.md`。§2 明确写「冻结的意思：Sprint 2 及以后的业务模块按这份签名实现」，Sprint 2 的开发者若按 `#[async_trait]` 的心智模型写代码会踩空。 | `crates/platform-core/src/extension/notification.rs:86-90`（`-> impl Future<Output = Result<DeliveryResult>> + Send;`）。实测 `Box<dyn NotificationSender>` → `error[E0038]: the trait NotificationSender is not dyn compatible`。Dev 报告风险 #1 已自陈该点 | BA 在 design-summary §2 补一条扩展点版本注记：本期起 5 个 trait 为 RPITIT 形态、不可用于 trait object；异构汇聚延后到 REQ-015/017 再决策。**不是要求 Dev 改代码** |
| **4** | **低** | **C-2 无法区分「登记了权限点」与「代码里真的做了鉴权」。**扫描取 `backend/**/*.rs` 的字符串字面量，而权限点常量的**定义处**本身就带字面量，因此被算作「已使用」。结果：一个只在 `permission.rs` 里声明、manifest 里也登记、但服务端从不做鉴权判断的权限点，C-2 全绿且无告警。 | `crates/platform-contracts/src/manifest_scan.rs:14-27`。实测：加 `PERMISSION_DEMO_DELETE = "sandbox:demo:delete"` + manifest 声明，`service.rs`/`repository.rs` 中 0 处引用 → `4 passed; 0 failed`，无 warning。Dev 风险 #2 只提了「非字面量形式会漏检」，未覆盖这一类 | 扫描时排除纯常量定义文件，或改为统计「引用常量名」而非字面量。与 #3 一样属契约口径问题，建议 BA 定口径后再改 |
| **5** | **低** | **平台 `id` 校验比已发布的 JSON Schema 更严，两者判定不一致。**`is_module_id` 要求每个连字符分段首字符为小写字母，而 schema 的 `^[a-z][a-z0-9]*(-[a-z0-9]+)*$` 允许分段以数字开头。另有两处 schema/实现不匹配：`dependencies[]` 在 schema 里**根本没有 pattern**，平台却强制按 id 规则校验；`permissions[].name` schema 无 `minLength`，平台要求非空。 | `crates/platform-core/src/extension/manifest.rs:282-288` vs `manifest_schema.rs:17` / `:48`。实测 `sandbox-1demo`、`a-1`、`sandbox-1`：schema 正则接受=true，平台解析器接受=false | 方向是「平台更严」，无安全洞。但 `capabilities/schema/manifest.schema.json` 是给 REQ-004 能力索引与第三方用的，第三方面向 schema 校验会与平台判定不一致。建议把 `is_module_id` 改成与 schema 正则等价，或把 schema 的 `id` 正则收紧到与实现一致（并给 `dependencies[]` 补 pattern） |
| **6** | **低** | **sandbox 的排序白名单外字段被静默忽略，与 api-conventions v1 C7「未声明字段返回 400」不一致。**sandbox 定位是「业务方照着抄」的标准范例，静默忽略会被复制。 | `modules/sandbox/backend/src/repository.rs:147-149`：`if !SORTABLE_FIELDS.contains(&spec.field.as_str()) { continue; }`（不是报错）；`api-conventions/v1.md` C7「排序 … 未声明字段返回 400」。本期无 HTTP 层，但 `Repository` 接口有能力返回 `Result` | 改为返回错误（如 `anyhow!("sort field {field} is not sortable")`），并在 sandbox README 里写明 |
| **7** | 提示 | **`feature-meta/manifest.json` 在本分支上的整体改写不是 Dev 做的。**除 Dev 补的末尾换行外，`req`/`branch`/`created_at`/`demand_dir` 的变更以及 `contracts`/`adrs`/`delivered_by` 三个键的**删除**，都来自 BA 的提交 `c8e2d5d`。Dev 报告 §7 第 10 条的表述（「补了一个末尾换行」）对 Dev 自己的改动是准确的，但未注明其余改动的归属，复核者容易误判。 | `git show c8e2d5d -- feature-meta/manifest.json`（BA 的完整改写 + 去掉末尾换行）vs `git diff 04b57d3^ 04b57d3 -- feature-meta/manifest.json`（Dev 的改动 = 仅末尾换行 1 行） | 在 Dev 报告里注明归属；另请 BA 确认 `contracts`/`adrs`/`delivered_by` 三个键被删是否符合预期（该文件由 BA 维护） |
| **8** | 提示 | **`migrations/` 目录只有一份 8 行 README 占位。**`check-modules` 的「非空」规则被 README 满足；但一份照抄这个范例的新模块，会得到一个「通过非空校验却没有任何迁移」的目录。 | `modules/sandbox/migrations/README.md`（8 行，说明本期无真实表）；`crates/xtask/src/modules.rs:146-151`（`is_empty_dir` 只判目录项数） | 本期（无 DB 层）可接受。REQ-006/007 落地 DB 层时，建议把「migrations/ 非空」升级为「至少一个 `*.sql` 迁移文件」，并在 README 里点明 |

**问题统计：中 2 条、低 4 条、提示 2 条，共 8 条。**

---

## 未覆盖 / 无法验证

| 项 | 状态 |
|----|------|
| **CI 六个 job 实跑** | **未验证**。本仓库无 remote，GitHub Actions 跑不起来。CI 的改动（新增 `cargo test -p sandbox` 一步）只做了静态核对：命令、路径、crate 名均存在且本地等价命令已实跑通过 |
| **覆盖率数字** | **未验证**。本机无覆盖率工具，且低配机约束下不 `cargo install`。角色规范的「覆盖率 ≥ 80%」未量化，以「每条 AC 均有自动化测试 + 破坏性验证」替代 |
| **圈复杂度 / 认知复杂度** | **未验证**。本机无 `lizard` / `complexipy`，不安装。仅人工核查 `checks.rs` / `repository.rs` 的函数拆分 |
| **冷编译峰值内存 / 耗时** | **未验证**。按要求未 `cargo clean`、未做 RSS 采样。本期唯一新增依赖 `inventory`（约 3KB，传递依赖 `rustversion`），量级可忽略，但无本轮实测数据 |
| **C-3 的真实多租户数据库行为** | **不适用**（BA 已澄清本期不连库）。C-3 验的是契约语义：编译期保证（每个 `Repository` 方法强制带 `ctx`）+ 行为验证（双租户同名夹具）。真实库口径待 REQ-006/007 落地后叠加 |
| **C-4 的真实事务回滚** | **不适用**（本期无 DB 访问层）。C-4 现口径为「单次写全有或全无」的可观测行为 + 跨表写声明检查，无真实回滚语义 |
| **REQ-002 的数据库集成脚本 `scripts/test-integration.sh`** | **未运行**（需 podman 起容器 + `psql` 客户端，本机无 psql）。与本需求无耦合；Rust 单测/契约测试不依赖数据库 |
| **`#[platform_module_contract]` 的错误路径** | **未验证**。宏对「传参」「非 mod item」「缺 `contract_subject()`」的处理代码存在（`compile_error!` 分支），但无对应测试；本轮未额外造违规（不在验收要求内） |
| **`inventory` 跨平台行为** | **未验证**。依赖 ELF `.init_array` 段，本仓库只在 Linux 跑 |

---

## 还原与工作树状态

- 本轮全部破坏性验证涉及 **14 次对已跟踪文件的变异**（C-1×1、C-2×2、C-3×1、C-4×2、C-3 守卫×1、manifest `{}`×1、C-2 保真度×1、依赖方向×4）+ **1 个临时探针目录**（5 个子场景），每次均经备份回写 + sha256 校验 + `git status --porcelain` 复核。
- 临时探针目录 `modules/qa-probe/` 已删除；临时工程置于 `.local/qa/`（已被 `.gitignore` 覆盖）并已清理。
- **被审实现代码与配置零改动**：`git diff --stat` 只列出 `feature-meta/qa-report.md` 一个文件，
  其余 47 个被审文件全部逐字节等于 HEAD。
- `git status --porcelain` 仅一条：` M feature-meta/qa-report.md`。
- **关于这一条 `M`**：`feature-meta/qa-report.md` 是**仓库内已跟踪的固定槽位**，内容为 REQ-002
  的 QA 报告（提交 `cbb877c`，blob `40d5dd6`）。本轮按任务指定路径写入 REQ-003 的报告，
  属**槽位覆写**，与本仓库既有约定一致 —— Dev 对 `verification-report.md`（462 行改写）、
  `traceability.md`（144 行改写）做的是同一件事。REQ-002 的旧报告未丢失：
  完整保留在 `develop:feature-meta/qa-report.md` 与提交 `cbb877c` 中，可随时取回；
  REQ-002 的复核轮报告 `feature-meta/qa-report-recheck.md` 本轮未触碰。
- 未改动任何被审实现代码，未 commit / merge / push。

---

## 给 BA 的处置建议

1. **可以合** —— AC-S-003 与 §7 交付清单已达成，契约测试经独立破坏性验证确认有效，无回归。
2. **合前或紧随其后处理 2 条中等问题**（#1 check-modules 结构级漏报、#2 依赖方向检查可绕过）：两条都在「门禁」本身，不影响本期功能，但 D7 是硬约束，建议 Dev 补修 + 补测试后再并。
3. **BA 侧登记 2 条契约后果**（#3 dyn 兼容性变更、#4 C-2 的「登记 ≠ 鉴权」口径），并同步进 design-summary §2 与 `api-conventions/v1.md`，否则 Sprint 2 的模块会按旧心智模型写代码。
4. 低/提示级 4 条可排入 REQ-004 或 DB 层落地批次。
