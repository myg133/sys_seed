# REQ-003 验证报告

- **需求**: REQ-003 扩展点框架与契约测试
- **分支**: `feature/REQ-003`
- **基线**: `develop` @ `eacff21`
- **设计依据**: `BA/demands/REQ-003-extension-framework/design-summary.md`
- **契约**: api-conventions v1（ratified）
- **开发者**: myg133-dev-1
- **日期**: 2026-11-18

---

## 1. 结论

**自验通过。** 交付清单（设计摘要 §7）7 项全部实现并有自动化证据；
REQ-002 的 24 个原有测试全部仍在且通过；前端与 E2E 无回归。

| 自验项 | 结果 |
|--------|------|
| `cargo check --workspace --all-targets` | ✅ 通过 |
| `cargo test --workspace -- --test-threads=2` | ✅ 64/64（含 REQ-002 原有 24 个，全绿） |
| `cargo clippy --workspace --all-targets -- -D warnings` | ✅ 无告警 |
| `cargo fmt --all --check` | ✅ 通过 |
| `cargo xtask check-modules` | ✅ 找到并校验 `modules/sandbox`（1 个模块，exit 0）；故意破坏后报错列出缺失项，exit 1 |
| 契约测试能拦住违规实现 | ✅ C-1 / C-2 / C-3 / C-4 逐一破坏性验证通过（见 §4） |
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
| `xtask`（lib） | 13 | 7 |
| `sandbox`（`tests/contracts.rs`） | 4 | 0（新增：C-1 ~ C-4） |
| `sandbox`（lib）、`platform-core`（bin）、`platform-contracts-macros` | 0 | 0 |
| **合计** | **64** | **24（全绿）** |

新增测试覆盖：
- `platform-core`：`extension::*` 共 25 个（5 个 trait 的行为、manifest 解析与校验 6 个、JSON Schema 3 个、注册表 2 个、事务边界 2 个等）；
- `platform-contracts`：夹具 / 运行时 / 源码扫描 5 个 + `suite` 告警通道 1 个；
- `xtask`：目录规范新增 3 个（Schema 违规、id 不一致、空目录）+ 依赖方向 3 个。

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
| 依赖方向 | `cargo tree` 确认：`platform-core` ← `platform-contracts` ← `sandbox`；`platform-core` / `platform-contracts` 均**不依赖** `modules/*` |

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

> REQ-002 时该命令输出「无业务模块」；加了 `modules/sandbox/` 后**能找到并校验它** ✓。
> 上述破坏场景均已还原（`diff` 确认）。

## 6. 依赖方向检查

`crates/xtask/src/deps.rs` 扫描 `crates/platform-core/Cargo.toml` 与
`crates/platform-contracts/Cargo.toml`，确认依赖图里**没有任何业务模块路径**。
`deps::tests::the_real_workspace_has_no_reverse_dependency` 对真实工作区跑这条检查 → 通过。

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

## 9. 未完成 / 未验证

| 项 | 状态 |
|----|------|
| CI 六个 job 实跑 | **未验证**（本仓库无 remote，`push` 前无法跑 GitHub Actions） |
| 冷编译峰值内存 / 耗时复测 | **未验证**（本轮未清空 `target/`，未做 RSS 采样；REQ-002 已实测 1071MB / 17s，本期新增依赖只有 `inventory`，量级可忽略） |
| 覆盖率数字 | **未验证**（本机无覆盖率工具，且按低配机约束不 `cargo install`；角色规范里的「覆盖率 ≥ 80%」本轮以「每条 AC 都有对应自动化测试」替代） |
| 圈复杂度 / 认知复杂度的工具化度量 | **未验证**（本机无 `lizard` / `complexipy` 之类工具，不安装；已按函数拆分人工核查） |
| 扩展点 trait 的 dyn-compatible 形态 | **未实现**（见风险 #1）—— 本期不做，按设计摘要 §7「不做」清单处理 |
| 通知通道 / 工作流的实际消费、微前端容器、`xtask create-module` | **不做**（设计摘要 §7 明确留给 REQ-015 / REQ-017 / ADR A4 / REQ-004） |
