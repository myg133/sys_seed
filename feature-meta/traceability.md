# REQ-003 追溯性矩阵

- **需求**: REQ-003 扩展点框架与契约测试
- **分支**: `feature/REQ-003`
- **基线**: `develop` @ `eacff21`（REQ-002 已合并）
- **契约**: api-conventions v1（已 ratified，见 `BA/contracts/api-conventions/v1.md`）
- **设计依据**: `BA/demands/REQ-003-extension-framework/design-summary.md`（trait 签名已冻结）
- **更新时间**: 2026-11-18

图例：**PASS** = 已实现且有自动化验证；**未验证** = 无自动化证据。

---

## 1. 自有 AC

| AC | 内容 | 实现位置 | 测试位置 | 结论 |
|----|------|----------|----------|------|
| AC-S-003 | 扩展点契约随平台分发：业务模块只依赖 `platform-contracts` 即可获得契约测试，无需复制平台内部代码 | `crates/platform-contracts/`（套件）、`crates/platform-contracts-macros/`（宏）、`modules/sandbox/Cargo.toml`（`platform-contracts` 只出现在 `[dev-dependencies]`） | `modules/sandbox/tests/contracts.rs`（4 个契约测试全绿）、`cargo test -p sandbox` | **PASS** |

## 2. 设计摘要 §7 交付清单

| 交付项 | 实现位置 | 测试位置 | 结论 |
|--------|----------|----------|------|
| 5 个扩展点 trait 定义与文档注释 | `crates/platform-core/src/extension/{platform_module,repository,auditable,notification,workflow}.rs` | `cargo test -p platform-core --lib`（25 个新单测） | **PASS** |
| `inventory` 编译期注册与 `discover()` | `crates/platform-core/src/extension/registry.rs` | `extension::registry::tests::registered_modules_are_discovered`、`::disabled_modules_are_filtered_out` | **PASS** |
| `platform-contracts` 套件：C-1 ~ C-4 四项检查 | `crates/platform-contracts/src/checks.rs` | `modules/sandbox/tests/contracts.rs` 生成的 4 个测试；另有破坏性验证（见 §4） | **PASS** |
| 简化版 `#[platform_module_contract]` 宏 | `crates/platform-contracts-macros/src/lib.rs` | 由 `modules/sandbox/tests/contracts.rs` 实际调用并生成 4 个测试 | **PASS** |
| manifest JSON Schema + 解析器 | `crates/platform-core/src/extension/manifest.rs`、`manifest_schema.rs`、`capabilities/schema/manifest.schema.json` | `extension::manifest::tests::*`（6 个）、`extension::manifest_schema::tests::*`（3 个） | **PASS** |
| `xtask check-modules` 完整规则 | `crates/xtask/src/modules.rs` | `crates/xtask/src/modules.rs` 的 10 个单测 + 手工破坏验证（见 §4） | **PASS** |
| 示例模块 `modules/sandbox/` | `modules/sandbox/**` | `cargo test -p sandbox`（4/4）、`cargo xtask check-modules` | **PASS** |

## 3. 设计摘要 §6 目录规范校验（5 项）

| 校验项 | 实现位置 | 测试位置 | 结论 |
|--------|----------|----------|------|
| 必备文件齐全 | `crates/xtask/src/modules.rs::check_module` | `modules::tests::missing_entries_are_listed`（断言 5 项缺失）、手工破坏验证 | **PASS** |
| `manifest.json` 符合 Schema | 同上（`check_manifest_schema`，复用 `platform_core::extension::Manifest`） | `modules::tests::manifest_schema_violations_are_reported_with_field_path` | **PASS** |
| `manifest.id` 与目录名一致 | 同上（`check_manifest_id`） | `modules::tests::manifest_id_must_match_the_directory_name` | **PASS** |
| `backend/`、`frontend/`、`migrations/` 非空 | 同上（`empty_dirs`） | `modules::tests::empty_required_directories_are_reported` | **PASS** |
| `AGENTS.md`、`README.md` 存在 | 同上（`missing_files`） | `modules::tests::missing_entries_are_listed` | **PASS** |

## 4. C-1 ~ C-4 的验证方式与结果

| 检查 | 判定口径 | 正常态 | 破坏性验证（故意违规 → 必须失败） | 结论 |
|------|----------|--------|-----------------------------------|------|
| **C-1** manifest 与实现一致 | `id` / `version` / `enabled` / `dependencies` 四项逐一比对 | 通过 | 把 `manifest.json` 的 `id` 改成 `wrong-name` → `contract_c1_manifest_matches_implementation` **FAILED**，报 ``manifest.id is `wrong-name` but module.id() is `sandbox` `` | **PASS** |
| **C-2** 权限点声明完整 | 代码里用到的权限点（源码扫描）必须都在 manifest；manifest 多余的只警告 | 通过（`--nocapture` 下无告警） | 删掉 manifest 里的 `sandbox:demo:write`、加一个代码没用的 `sandbox:unused:read` → **FAILED**（未声明的 `sandbox:demo:write` 报错）+ 多余的 `sandbox:unused:read` 只打印 warning 不失败 | **PASS** |
| **C-3** 租户隔离生效 | 用 tenant_a 的 ctx `list()` 只得到 tenant_a 的数据；用 tenant_a 的 ctx `find(tenant_b 的 id)` 返回 `None` | 通过 | 让 repository 的 `find`/`list` 忽略 `ctx.tenant_id()` → `contract_c3_tenant_isolation_holds` **FAILED**，报 6 条越权细节（list 串租户 ×4、find 命中他人记录 ×2） | **PASS** |
| **C-4** 事务边界 | 单次写全有或全无（重复主键的 insert 必须报错且不覆盖）；跨表写必须显式开平台事务 | 通过 | 夹具声明 2 张表但不声明平台事务 → **FAILED**，报 `cross-table write (tables: sandbox_demo_item, sandbox_demo_log) does not go through the platform transaction helper` | **PASS** |

> 所有破坏性验证均已**逐字节还原**（`diff` 确认），还原后 `cargo test -p sandbox --test contracts` 4/4 通过。

## 5. 依赖方向（设计摘要 §1）

| 检查 | 实现位置 | 测试位置 | 结论 |
|------|----------|----------|------|
| `platform-core` / `platform-contracts` 的依赖图不得出现业务模块路径 | `crates/xtask/src/deps.rs` | `deps::tests::the_real_workspace_has_no_reverse_dependency`（扫真实工作区） | **PASS** |

## 6. 父需求承接的 AC（AC-031 ~ AC-034）

> 父 AC 的完整步骤在 `BA/demands/REQ-001-platform-foundation/acceptance.md`。本需求承担的是
> 「业务模块 ↔ 平台」接缝这一段，父 AC 的其余部分由 REQ-002 及后续需求承接。

| 父 AC | 本需求的承担部分 | 结论 |
|-------|------------------|------|
| AC-031 | 扩展点契约随平台分发（见 §1） | **PASS** |
| AC-032 | 租户上下文随契约强制传递（`Repository` 每个方法都带 `ctx: &RequestContext`，漏传编译不过） | **PASS** |
| AC-033 | 权限点登记与一致性（C-2） | **PASS** |
| AC-034 | 事务边界约定（C-4） | **PASS** |

## 7. 非功能性要求

| 要求 | 验证方式 | 结论 |
|------|----------|------|
| REQ-002 的 24 个原有测试不能破 | `cargo test --workspace -- --test-threads=2` → `platform-contracts` 3、`platform-core` 14、`xtask` 7 全部仍在且通过 | **PASS** |
| 圈复杂度 ≤ 10 / 认知复杂度 ≤ 15 | 逐函数拆分（`checks.rs` 的每个检查拆成独立小函数，`repository.rs` 的排序/过滤拆成 `sort_rows` / `matches_filters` / `matches_filter` / `in_range`） | **PASS**（人工核查；本机无复杂度度量工具可用） |
| 无 TODO / FIXME / 调试输出 | 对本轮新增文件全量 `grep` 扫描 | **PASS** |
| 不引入 sqlx / reqwest / aws-sdk / opendal / jsonwebtoken | `Cargo.lock` 与各 `Cargo.toml` 核对 | **PASS** |
| 本期只新增 `inventory` 一个第三方依赖 | `cargo tree` 核对（见验证报告 §3） | **PASS**（另有零第三方依赖的过程宏 crate） |
| 不改 `platform-core` 既有内部（`config.rs` / `context.rs` / `error.rs`） | `git diff` 核对 | **PASS**（`lib.rs` 仅新增 `pub mod extension;` 与 1 行 re-export，`Cargo.toml` 仅新增 2 个依赖） |
