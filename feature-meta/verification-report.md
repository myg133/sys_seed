# REQ-003 验证报告

- **需求**: REQ-003 扩展点框架与契约测试
- **分支**: `feature/REQ-003`
- **基线**: `develop` @ `eacff21`
- **设计依据**: `BA/demands/REQ-003-extension-framework/design-summary.md`
- **契约**: api-conventions v1（ratified）
- **开发者**: myg133-dev-1
- **日期**: 2026-11-18
- **轮次**: **修复轮 2**（承接 QA Pre-merge 报告 `feature-meta/qa-report.md` 的 PASS-WITH-RISK 结论）

---

## 0. 修复轮 2 摘要

QA 的 Pre-merge 结论是 PASS-WITH-RISK：破坏性验证全通过，但两个「门禁」本身有盲区。
本轮按 QA 报告的问题清单修复 **中 1 / 中 2 / 低 5 / 低 6 / 提示 7** 五项，
**未扩大范围**（低 3 / 低 4 由 BA 处理，提示 8 本期不改）。

| 项 | 问题 | 本轮处理 | 结果 |
|----|------|----------|------|
| **中 1** | `check-modules` 静默吞掉 manifest 结构级违规 | `check_manifest_schema` 解析失败时把 serde 错误作为一条 `manifest_issues` 报出 | ✅ 已修 + 2 测试 |
| **中 2** | 依赖方向检查是字符串扫描，可被绕过 | 改读 `cargo metadata --no-deps` 的成员依赖图做可达性判定；`platform-contracts-macros` 纳入平台 crate | ✅ 已修 + 3 测试 |
| **低 5** | manifest Schema 与平台校验器判定不一致 | schema 收紧到与实现**完全一致**（`id` / `dependencies[]` / 名称） | ✅ 已修 + 2 测试 |
| **低 6** | sandbox 排序白名单外字段被静默忽略 | 改为返回错误，README / AGENTS.md 写明 | ✅ 已修 + 2 测试 |
| **提示 7** | `feature-meta/manifest.json` 的契约 pin 被冲掉 | 补回 `contracts` / `adrs` / `delivered_by` | ✅ 已修 |

本轮改动：**11 文件 / +865 -80 行**（6 个小步提交，基线 `04b57d3` → `2166eae`）。

**测试**：Rust **73/73**（原 64 个全绿，只增不减：+9）、前端 **23/23**、E2E **1/1**。

---

## 0.1 设计摘要 §5 需要 BA 同步的字段

代码已改完，但 **BA 维护的 `BA/demands/REQ-003-extension-framework/design-summary.md` §5
仍是旧 schema** —— 它是给第三方看的契约，必须与代码一致。本轮**没有改**该文件（不在 Dev 的写入范围）。

| §5 位置 | 现状（需改成） | 原因 |
|---------|--------------|------|
| `properties.id.pattern` | `^[a-z][a-z0-9]*(-[a-z0-9]+)*$` → **`^[a-z][a-z0-9]*(-[a-z][a-z0-9]*)*$`** | 旧 pattern 允许分段以数字开头（`sandbox-1demo` / `a-1` / `sandbox-1`），平台 `is_module_id` 不允许 |
| `properties.dependencies.items` | `{ "type": "string" }` → **`{ "type": "string", "pattern": "^[a-z][a-z0-9]*(-[a-z][a-z0-9]*)*$" }`** | 平台按 id 规则校验依赖 ID，schema 原本没有 pattern |
| `properties.name` | `{ "type": "string", "minLength": 1 }` → **追加 `"pattern": "\\S"`** | 平台判「去掉首尾空白后为空」即非法；只补 `minLength` 的话纯空白串仍会两边不一致 |
| `properties.permissions.items.properties.name` | `{ "type": "string" }` → **`{ "type": "string", "minLength": 1, "pattern": "\\S" }`** | 同上 |

> `\S` 的语义是「至少含一个非空白字符」，与平台的 `name.trim().is_empty()` 判定等价。
> 极端 Unicode 空白字符（NEL / U+180E 等）在 ECMA-262 与 Rust 的空白定义下有细微差异，
> 不影响业务判定。

代码侧的权威副本：`crates/platform-core/src/extension/manifest_schema.rs::MANIFEST_JSON_SCHEMA`
与导出件 `capabilities/schema/manifest.schema.json`（二者由
`manifest_schema::tests::exported_schema_file_matches_the_constant` 保证一致）。

## 0.2 C-2 的口径（BA 已定，写在这里备查）

**C-2 只验「权限点声明完整性」，不验「服务端是否真的执行鉴权」。**

- 验的：代码里用到的权限点常量是否都在 `manifest.json` 的 `permissions` 里登记
  （漏登记 → 失败）；manifest 里登记了但代码没用的 → 只告警不失败。
- 不验的：服务端在处理请求时是否真的拿权限点做了鉴权判断 ——
  那是 **REQ-009 RBAC** 的职责，本期不验、也不应被理解为已验。

因此 QA 低 4「C-2 无法区分『登记』与『真的鉴权』」**不是本轮的缺陷**，是口径问题；
本轮**未改动** `crates/platform-contracts/src/manifest_scan.rs` 的扫描逻辑。

---

## 1. 结论

**自验通过。** 交付清单（设计摘要 §7）7 项全部实现并有自动化证据；
REQ-002 的 24 个原有测试全部仍在且通过；前端与 E2E 无回归；
QA 指出的 4 个代码问题 + 1 个 manifest 字段已全部修复。

| 自验项 | 结果 |
|--------|------|
| `cargo check --workspace --all-targets` | ✅ 通过 |
| `cargo test --workspace -- --test-threads=2` | ✅ **73/73**（含 REQ-002 原有 24 个，全绿） |
| `cargo clippy --workspace --all-targets -- -D warnings` | ✅ 无告警 |
| `cargo fmt --all --check` | ✅ 通过 |
| `cargo xtask check-modules` | ✅ 找到并校验 `modules/sandbox`（1 个模块，exit 0）；manifest 结构级违规 → exit 1 并指出缺什么 |
| 契约测试能拦住违规实现 | ✅ C-1 / C-2 / C-3 / C-4 逐一破坏性验证通过（见 §4） |
| 依赖方向检查能拦住绕过 | ✅ workspace 继承写法 + 未列入清单的 crate，两种绕过都被报出（见 §6） |
| `pnpm -r test` | ✅ 23/23 |
| `bun run test:e2e` | ✅ 1/1（chromium） |
| `pnpm exec eslint .` | ✅ 无告警 |
| `pnpm exec prettier --check .` | ✅ 全部符合 |


## 2. 测试明细

### Rust（`cargo test --workspace -- --test-threads=2`）

| 包 | 通过 | 其中 REQ-002 原有 |
|----|------|------------------|
| `platform-core`（lib） | 39 | 14 |
| `platform-contracts`（lib） | 8 | 3 |
| `xtask`（lib） | 18 | 7 |
| `sandbox`（`tests/contracts.rs`） | 4 | 0（新增：C-1 ~ C-4） |
| `sandbox`（lib） | 2 | 0（修复轮 2 新增：排序白名单） |
| `sandbox`（lib）、`platform-core`（bin）、`platform-contracts-macros` | 0 | 0 |
| **合计** | **73** | **24（全绿）** |

> `platform-core`（lib）由 39 增至 41（修复轮 2 新增 2 个 Schema 一致性测试）。

新增测试覆盖：
- `platform-core`：`extension::*` 共 27 个（5 个 trait 的行为、manifest 解析与校验 6 个、JSON Schema 5 个、注册表 2 个、事务边界 2 个等）；
- `platform-contracts`：夹具 / 运行时 / 源码扫描 5 个 + `suite` 告警通道 1 个；
- `xtask`：目录规范 8 个 + 依赖方向 6 个（修复轮 2：依赖方向从 3 个增至 6 个）。

### 前端与 E2E

| 命令 | 结果 |
|------|------|
| `pnpm -r test` | 23/23（`apps/shell` 4 + `packages/{auth,hooks,i18n,schema}` 各 3 + `packages/sdk` 2 + `packages/theme` 2 + `packages/ui` 3） |
| `bun run test:e2e` | 1/1（chromium，`e2e/shell.spec.ts`） |

> 前端无回归：本期只新增 `modules/sandbox/frontend/`（不在 pnpm workspace 内，无 `package.json`），
> 但仍受根目录 eslint / prettier 约束，已实测通过。

## 3. 依赖

| 检查 | 结果 |
|------|------|
| 新增第三方依赖 | **仅 `inventory 0.3.25`**（约 3KB，无额外传递依赖；传递依赖只有 `rustversion`） |
| 禁止清单（sqlx / reqwest / aws-sdk / opendal / jsonwebtoken） | `Cargo.lock` + 各 `Cargo.toml` 扫描 **0 命中** |
| `async-trait` | **未引入**。trait 侧用 `async fn in trait` 的展开写法 `-> impl Future<Output = ..> + Send`（Rust 1.75+ 稳定），实现方仍可写 `async fn`，调用方写法不变 |
| 新增 workspace crate | `platform-contracts-macros`（过程宏，**只依赖标准库 `proc_macro`**，不引入 syn / quote / proc-macro2） |
| 依赖方向 | `cargo tree` 确认：`platform-core` ← `platform-contracts` ← `sandbox`；`platform-core` / `platform-contracts` / `platform-contracts-macros` 均**不依赖** `modules/*`。修复轮 2 起由 `cargo metadata` 的成员依赖图判定（见 §6） |

> **为什么不用 `async-trait`**：`async fn` 写在公开 trait 上会触发 `async_fn_in_trait` lint
> （warn 级，`clippy -D warnings` 下即失败），且无法约束返回 future 的 `Send`。
> 展开写法是编译器给出的等价建议：签名语义与设计摘要 §2 一致、调用点写法完全不变，
> 同时保证 future 是 `Send`（可安全交给 tokio 任务），且不新增任何依赖。

## 4. C-1 ~ C-4 的验证方式

| 检查 | 判定 | 正常态 | 破坏性验证（故意违规） | 结果 |
|------|------|--------|----------------------|------|
| **C-1** | `manifest.json` 的 `id` / `version` / `enabled` / `dependencies` 与 `PlatformModule` 实现逐项一致 | 通过 | `manifest.json` 的 `id` 改成 `wrong-name` | ❌ `contract_c1_manifest_matches_implementation` FAILED，报 ``manifest.id is `wrong-name` but module.id() is `sandbox` `` |
| **C-2** | 代码里用到的权限点必须都在 manifest 声明；manifest 多余的只警告 | 通过 | 删掉 manifest 里的 `sandbox:demo:write`；再加一个代码没用的 `sandbox:unused:read` | ❌ FAILED（`sandbox:demo:write` 未声明 → 失败）+ `sandbox:unused:read` 只打印 warning，**不失败** |
| **C-3** | tenant_a 的 ctx `list()` 只得到 tenant_a 的数据；tenant_a 的 ctx `find(tenant_b 的 id)` 返回 `None` | 通过 | 让 `InMemoryRepository` 的 `find` / `list` 忽略 `ctx.tenant_id()` | ❌ `contract_c3_tenant_isolation_holds` FAILED，报 6 条越权细节（list 串租户 ×4、find 命中他人记录 ×2） |
| **C-4** | 单次写全有或全无（重复主键的 `insert` 必须报错且不覆盖既有数据）；跨表写必须显式开平台事务 | 通过 | 夹具声明 2 张表但不声明平台事务 | ❌ `contract_c4_transaction_boundary_holds` FAILED，报 `cross-table write (tables: sandbox_demo_item, sandbox_demo_log) does not go through the platform transaction helper` |

> 4 次破坏性验证**全部已逐字节还原**（`diff` 确认），还原后 `cargo test -p sandbox --test contracts` 4/4 通过。

### C-3 不连数据库的实现方式（BA 已澄清）

平台本期没有 DB 访问层，C-3 因此由两条保证组成：

1. **编译期保证**：`Repository<T>` 的每个方法都强制带 `ctx: &RequestContext`，
   租户只存在于 `ctx` 里 —— 实现方**没有任何别的途径拿到租户**，漏传就编译不过；
2. **行为验证**：夹具造两个租户的同名数据（`tenant_a`/`demo-1`/`"同名记录"` 与
   `tenant_b`/`demo-2`/`"同名记录"`，唯一区别是租户），用 ctx 指定的租户调 `list()`，
   断言只返回该租户的数据；用该 ctx 调 `find()` 查另一个租户的 id，断言返回 `None`。

`modules/sandbox` 的 `InMemoryRepository` 按 `ctx.tenant_id()` 过滤，因此 C-3 能真实跑通。

## 5. `xtask check-modules` 的验证

| 场景 | 输出 | 退出码 |
|------|------|-------|
| 正常（`modules/sandbox` 齐全） | `模块目录校验通过：1 个模块` + `✓ sandbox` | 0 |
| 删掉 `AGENTS.md` | `缺少文件 AGENTS.md` | 1 |
| `manifest.json` 的 `version` 改成 `0.1` | `manifest.json 不符合 Schema：version：must match ^\d+\.\d+\.\d+$` | 1 |
| `manifest.json` 的 `id` 改成 `wrong-name` | ``manifest.id 为 `wrong-name`，与目录名 `sandbox` 不一致（应以目录名为准）`` | 1 |
| 清空 `migrations/` | `目录 migrations/ 为空` | 1 |
| **`manifest.json` = `{}`（修复轮 2 新增）** | **`manifest.json 不符合 Schema：missing field \`id\` at line 1 column 2`** | **1** |
| **`manifest.json` = `{"name":"Probe","enabled":true}`（修复轮 2 新增）** | **`manifest.json 不符合 Schema：missing field \`id\` at line 1 column 31`** | **1** |
| **`manifest.json` 含未知字段 `"bogus":1`（修复轮 2 新增）** | **`manifest.json 不符合 Schema：unknown field \`bogus\`, expected one of \`id\`, \`name\`, … at line 1 column 113`** | **1** |

> REQ-002 时该命令输出「无业务模块」；加了 `modules/sandbox/` 后**能找到并校验它** ✓。
> 上述破坏场景均已还原（`sha256` 校验确认，`git status` 为空）。

### 修复轮 2：结构级 manifest 违规不再被吞掉

**问题**（QA 中 1）：`check_manifest_schema` 在 `Manifest::from_path` 解析失败时 `return Vec::new()`，
serde 层面的失败（缺必备字段 / 未知字段）因此完全不被报告。

**改法**（`crates/xtask/src/modules.rs`）：先把文件读成文本，判 JSON 语法（语法错误仍由
`invalid_manifest` 记录，不重复报），再 `Manifest::parse_str`：

- 解析成功 → 走 `validate()`，报字段级问题（原有行为不变）；
- 解析失败 → 报**一条** `manifest_issues`，内容是 serde 的原始错误（自带字段路径与行列号）。

**回归测试**：`modules::tests::manifest_without_required_fields_is_reported`（`{}`）与
`modules::tests::manifest_with_unknown_field_is_reported`（未知字段）——
两者都断言 `check_modules` 判失败。

## 6. 依赖方向检查

### 修复轮 2 之前（QA 中 2）

`crates/xtask/src/deps.rs` **逐行读** `crates/platform-core/Cargo.toml` 与
`crates/platform-contracts/Cargo.toml`，只找含 `modules/` 的行。两个盲区：

1. workspace 继承写法（`sandbox.workspace = true` + 根 `[workspace.dependencies]` 声明路径）
   → 路径不在被扫文件里，**完全漏检**；
2. `PLATFORM_CRATES` 不含本期新增的 `platform-contracts-macros` → **该 crate 不受任何约束**。

### 修复轮 2 之后

`crates/xtask/src/deps.rs` 改为读 **`cargo metadata --format-version 1 --no-deps`** 的成员依赖图，
从每个平台 crate 出发做**可达性遍历**（不是文本匹配）：

- 成员依赖图 = `cargo metadata` 的 `packages[].dependencies`（`--no-deps` 时业务模块也是工作区成员，
  依赖边完整可见；本机实测 46ms、离线）；
- 平台 crate 集合 = 显式清单 `PLATFORM_CRATES`（已补上 `platform-contracts-macros`）
  **∪** `crates/` 下的工作区成员（新增平台 crate 不会因为忘了改清单而漏检）；
- 遍历到 `modules/` 下的包 → 报违规，并给出完整依赖链。

### 破坏性验证（两种绕过都被报出）

| 绕过 | 植入方式 | 检查输出 |
|------|----------|----------|
| workspace 继承写法 | 根 `Cargo.toml` 的 `[workspace.dependencies]` 加 `sandbox = { path = "modules/sandbox" }`；`crates/platform-contracts/Cargo.toml` 写 `sandbox.workspace = true` | **FAILED**：`平台 crate \`platform-contracts\` 反向依赖了业务模块 \`sandbox\`（路径：modules/sandbox，依赖链：platform-contracts → sandbox）` |
| 未列入清单的 crate | `crates/platform-contracts-macros/Cargo.toml` 加 `sandbox = { path = "../../modules/sandbox" }` | **FAILED**：`平台 crate \`platform-contracts-macros\` 反向依赖了业务模块 \`sandbox\`（路径：modules/sandbox，依赖链：platform-contracts-macros → sandbox）` |

> 两次绕过植入后均**逐字节还原**（`sha256` 一致，`git status` 为空）。
> 门禁仍是 `deps::tests::the_real_workspace_has_no_reverse_dependency`（CI 的 `cargo test --workspace` 承担）。

### 新增回归测试

| 测试 | 覆盖 |
|------|------|
| `deps::tests::workspace_inherited_dependency_is_reported` | workspace 继承写法 |
| `deps::tests::macros_crate_is_subject_to_the_direction_rule` | 本期新增的平台 crate（历史上不在清单里） |
| `deps::tests::platform_crate_outside_the_explicit_list_is_also_checked` | `crates/` 下、不在显式清单里的 crate |

（另 2 条原有测试 `business_module_dependencies_are_reported` /
`platform_only_dependencies_are_allowed` 改成用真实 cargo 工作区夹具，断言不变。）

## 6.1 manifest Schema 与平台实现的一致性（修复轮 2 · 低 5）

**问题**（QA 低 5）：平台 `is_module_id` 比已发布的 JSON Schema 更严，两边判定不一致。

**改法**：schema 收紧到与实现**完全一致**（schema 是对外发布的，更严更安全）：

| 字段 | 改前 | 改后 |
|------|------|------|
| `id` | `^[a-z][a-z0-9]*(-[a-z0-9]+)*$` | `^[a-z][a-z0-9]*(-[a-z][a-z0-9]*)*$` |
| `dependencies[]` | 无 pattern | 与 `id` 同 pattern |
| `name` | 只有 `minLength: 1` | 追加 `pattern: \S` |
| `permissions[].name` | 无 minLength | `minLength: 1` + `pattern: \S` |

**验证**：`manifest_schema::tests::schema_and_implementation_agree_on_module_id_samples` 用一个
**独立于平台实现**的极简正则求值器（仓库不引入 `regex` / `jsonschema` 依赖）读 schema 里的
pattern，与平台实现逐样本比对：

| id 样本 | 平台实现 | schema（改后） |
|---------|---------|--------------|
| `sandbox` | 接受 | 接受 |
| `demo` | 接受 | 接受 |
| `demo2` | 接受 | 接受 |
| `a-b-c` | 接受 | 接受 |
| `a-b1` | 接受 | 接受 |
| `a1-b` | 接受 | 接受 |
| `sandbox-1demo` | 拒绝 | 拒绝 |
| `a-1` | 拒绝 | 拒绝 |
| `sandbox-1` | 拒绝 | 拒绝 |
| `demo-2` | 拒绝 | 拒绝 |
| `Demo` | 拒绝 | 拒绝 |
| `demo_x` | 拒绝 | 拒绝 |
| ``（空串） | 拒绝 | 拒绝 |
| `demo-` | 拒绝 | 拒绝 |
| `a--b` | 拒绝 | 拒绝 |

（另有 `manifest_schema::tests::schema_and_implementation_agree_on_permission_name_samples`
覆盖名称样本：`Read` / `读取` / `  x  ` 接受，`` / ` ` / `\t` / `   ` 拒绝。）

## 6.2 sandbox 排序白名单外字段（修复轮 2 · 低 6）

**问题**（QA 低 6）：`sort_rows` 对白名单外的排序字段 `continue`（静默忽略），
与契约 C7「未声明字段返回 400」不一致；sandbox 是「业务方照着抄」的标准范例。

**改法**：`modules/sandbox/backend/src/repository.rs` 的 `sort_rows` 改为返回 `Result<()>`，
白名单外的字段直接报错，`list()` 用 `?` 传播：

```text
sort field unknown is not sortable
```

`modules/sandbox/README.md` 新增「排序与过滤字段白名单」小节，
`modules/sandbox/AGENTS.md` 的硬规则新增第 6 条 —— 两者都写明「白名单外 → 报错，不静默忽略」。

**回归测试**：`repository::tests::list_rejects_a_sort_field_outside_the_whitelist`（断言错误文案）
与 `repository::tests::list_sorts_by_a_declared_field`（白名单内排序仍正常）。

## 7. 偏离与必要改动（需求未提及但必要的）

| # | 偏离 / 必要改动 | 原因 |
|---|----------------|------|
| 1 | **trait 侧用 `-> impl Future<Output = ..> + Send` 展开写法**，而非字面的 `async fn` | `async fn` 写在公开 trait 上触发 `async_fn_in_trait` lint（`clippy -D warnings` 下失败）且无法保证 `Send`；调用点写法与实现方写法都不变（实现方仍写 `async fn`），符合「用 `async fn in trait`，不用 `async-trait`」的指示 |
| 2 | **新增 workspace crate `crates/platform-contracts-macros`（过程宏）** | Rust 1.98 上 `macro_rules!` 的属性宏形态仍是 unstable（rust-lang/rust#143547），拿不到被标注的 item，`#[platform_module_contract]` 只能用过程宏实现。该 crate **只依赖标准库 `proc_macro`**，不引入 syn / quote / proc-macro2，第三方依赖仍只有 `inventory` 一个 |
| 3 | **注册表元素是包装类型 `ModuleRegistration`，不是 `dyn PlatformModule`** | 设计摘要写的是 `inventory::collect!(PlatformModule)` + `iter::<dyn PlatformModule>`，但 `inventory` 0.3 的 `Collect: Sized` 使 `collect!(dyn Trait)` **无法编译**（inventory 自带 `tests/ui/collect-unsized.rs` 即该失败用例）。对外 `discover()` 仍返回 `Vec<&'static dyn PlatformModule>`，对调用方无差别 |
| 4 | **新增第 6 个 trait `TransactionalWrite`**（`crates/platform-core/src/extension/transaction.rs`） | 设计摘要 §4.4 要求「套件检查模块是否在写多张表时使用了平台的事务辅助」，但 5 个冻结扩展点里没有承载写边界声明的位置。它**不是**扩展点，而是平台为 C-4 提供的能力声明面（REQ-006/007 落地 DB 层后补真事务助手，签名不变） |
| 5 | **C-4 的「单次写是一个事务」落成可判定口径**：重复主键的 `insert` 必须返回错误且不得覆盖既有数据；单表写声明平台事务只告警 | §4.4 的原文没有给出可判定口径。契约测试必须能失败才有意义，因此把「一次写全有或全无」落成上面两条可观测行为 |
| 6 | **C-2 的权限点来源是源码扫描**（扫 `backend/**/*.rs` 的字符串字面量） | §4.3 说「遍历模块代码里用到的权限点常量」。走 AST 需要 syn（明显拖慢低配机冷编译），而权限点形态足够特殊（`<module-id>:<resource>:<action>` 三段小写编码），字面量扫描足够精确 |
| 7 | **`crates/platform-core/src/lib.rs` 新增 `pub mod extension;` + 1 行 re-export；`crates/platform-core/Cargo.toml` 新增 `anyhow` 与 `inventory`** | `extension` 模块必须挂到 crate root 才可达；`anyhow::Result` 是冻结签名的一部分，`inventory` 是编译期注册所需。**既有内部（`config.rs` / `context.rs` / `error.rs`）一行未改** |
| 8 | **`crates/xtask` 依赖 `platform-core`** | `check-modules` 的 Schema 校验要复用平台核心的 manifest 解析器，避免两套实现漂移 |
| 9 | **REQ-002 的 2 个 xtask 测试夹具更新**：`make_module` 与 `missing_entries_are_listed` 的占位 manifest（`{"name":"demo"}` / `{}`）换成符合 REQ-003 Schema 的合法 manifest，`make_module` 另往三个必备目录各写一个占位文件 | REQ-003 给了 `manifest.json` 真实的 Schema，占位 manifest 不再合法。**断言一行未改**（`missing_entries_are_listed` 仍断言 `problems().len() == 5`），7 个原有 xtask 测试全部仍通过 |
| 10 | **`feature-meta/manifest.json` 补了一个末尾换行** | 该文件由 BA 在 `c8e2d5d` 写入时缺末尾换行，导致 `pnpm exec prettier --check .` 失败（CI lint job 会红）。补换行是纯格式改动，无语义变化 |
| 11 | **CI `contract-check` job 新增一步 `cargo test -p sandbox`** | 契约测试的主体在业务模块 crate 里，不在 `platform-contracts` 里；只跑 `cargo test -p platform-contracts` 覆盖不到 C-1 ~ C-4 |
| 12 | **修复轮 2：依赖方向检查改读 `cargo metadata` 的成员依赖图**（`crates/xtask/src/deps.rs` 重写） | QA 中 2：逐行文本扫描可被 workspace 继承写法与未列入清单的 crate 绕过。改为读依赖图做可达性判定，**不再**做文本匹配。`dependency_direction_violations` 的返回类型由 `Vec<String>` 改为 `Result<Vec<String>>`（`cargo metadata` 失败必须报错，不能静默通过） |
| 13 | **修复轮 2：`crates/xtask` 新增 `serde` 依赖** | 解析 `cargo metadata` 的输出。`serde` 已在 workspace 与 `Cargo.lock` 里，**未引入任何新的第三方包**（`Cargo.lock` 只多一行 `xtask → serde`） |
| 14 | **修复轮 2：manifest Schema 收紧**（`id` / `dependencies[]` / 名称） | QA 低 5：已发布的 schema 与平台实现判定不一致。schema 收紧到与实现完全一致 → **设计摘要 §5 需要 BA 同步**（见 §0.1） |
| 15 | **修复轮 2：sandbox 排序白名单外字段改为报错**（行为变更） | QA 低 6：静默忽略与契约 C7 不一致，且 sandbox 是「照着抄」的标准范例。从「静默忽略」变成「返回错误」是**有意的行为变更** |
| 16 | **修复轮 2：`feature-meta/manifest.json` 补回 `contracts` / `adrs` / `delivered_by`** | QA 提示 7：`claim` 工具重建 REQ-003 的 manifest 时把这三个键删了。格式与 REQ-002 的版本一致，文件末尾保留换行（prettier 依赖它） |

## 8. 风险与已知边界

| # | 风险 | 说明 / 缓解 |
|---|------|------------|
| 1 | **扩展点 trait 不是 dyn-compatible** | `impl Future` 展开写法（以及 `async fn in trait` 本身）都不支持 `Box<dyn NotificationSender>` 这类异构集合。REQ-015 / REQ-017 若需要汇聚多个通道，需用泛型或届时再评估是否引入 `async-trait`（属扩展点版本变更）。本期 `PlatformModule` 仍是 dyn-compatible（无 async 方法），注册表照常工作 |
| 2 | **C-2 的源码扫描是启发式** | 只认「三段小写编码且首段等于模块 id」的字符串字面量。极端情况下（裸字符串、宏拼接的权限点）可能漏检；当前登记方式（`permission.rs` 里的常量）下可靠 |
| 3 | **C-3 / C-4 的行为口径依赖夹具质量** | 夹具由业务模块提供，若模块给的「两个租户」数据本来就是同一个租户，C-3 的断言会失去意义。套件已对「租户数 < 2」直接判失败 |
| 4 | **C-2 的告警默认被测试框架捕获** | 告警走 `eprintln!`，需要 `cargo test -- --nocapture` 才可见；已在 `modules/sandbox/README.md` 与 `AGENTS.md` 写明 |
| 5 | **`inventory` 的链接期构造器** | 依赖 ELF `.init_array` 段，跨平台行为由 `inventory` 自身保证；本仓库只跑 Linux |
| 6 | **本期无 DB 访问层** | C-3 / C-4 验的是契约语义而非真实 SQL。设计摘要 §4.2 已预告：「如果后续允许模块直接拿 pool，那这条测试必须同步升级为检查 SQL」 |
| 7 | **CI 未实跑** | 本仓库无 remote，GitHub Actions 跑不起来；CI 配置的改动（新增 `cargo test -p sandbox` 一步）只做了静态核对，**未实跑** |
| 8 | **依赖方向门禁仍在单元测试里，没有 CLI 子命令** | `xtask` 只有 `check-modules` 一个子命令；依赖方向靠 `cargo test --workspace` 里的 `deps::tests::the_real_workspace_has_no_reverse_dependency` 承担（CI 的 test job 会跑）。本期未新增子命令（不在修复轮范围） |
| 9 | **过滤字段白名单外仍被静默忽略** | 修复轮 2 只处理了 QA 列出的**排序**（低 6）。`matches_filter` 对白名单外的过滤字段仍按「不参与过滤」处理。QA 未列此项，本期**未改**；若要一并对齐 C7，需另开一项 |
| 10 | **schema 的 `\S` 与平台 trim 判定在极端 Unicode 空白字符上有细微差异** | ECMA-262 的空白定义与 Rust 的 `char::is_whitespace` 在个别码位（NEL / U+180E 等）上不同。业务字段名不受影响 |

## 9. 未完成 / 未验证

| 项 | 状态 |
|----|------|
| **设计摘要 §5 同步旧 schema** | **未完成（不在 Dev 范围）**。代码侧已改完并与实现一致，但 `BA/demands/REQ-003-extension-framework/design-summary.md` §5 仍是旧 schema，需 BA 同步（字段清单见 §0.1） |
| CI 六个 job 实跑 | **未验证**（本仓库无 remote，`push` 前无法跑 GitHub Actions） |
| 冷编译峰值内存 / 耗时复测 | **未验证**（本轮未清空 `target/`，未做 RSS 采样；REQ-002 已实测 1071MB / 17s，本期新增依赖只有 `inventory`，量级可忽略） |
| 覆盖率数字 | **未验证**（本机无覆盖率工具，且按低配机约束不 `cargo install`；角色规范里的「覆盖率 ≥ 80%」本轮以「每条 AC 都有对应自动化测试」替代） |
| 圈复杂度 / 认知复杂度的工具化度量 | **未验证**（本机无 `lizard` / `complexipy` 之类工具，不安装；已按函数拆分人工核查） |
| 扩展点 trait 的 dyn-compatible 形态 | **未实现**（见风险 #1）—— 本期不做，按设计摘要 §7「不做」清单处理 |
| 通知通道 / 工作流的实际消费、微前端容器、`xtask create-module` | **不做**（设计摘要 §7 明确留给 REQ-015 / REQ-017 / ADR A4 / REQ-004） |
