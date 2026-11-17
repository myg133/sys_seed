# REQ-003 状态

- **需求**: REQ-003 扩展点框架与契约测试
- **分支**: feature/REQ-003
- **基线**: develop @ `eacff21`（REQ-002 已合并）
- **状态**: **待验证**（修复轮 2 已完成并自验通过，等待 QA 复验）
- **开发者**: myg133-dev-1
- **完成时间**: 2026-11-18
- **产物**: `feature-meta/traceability.md`、`feature-meta/verification-report.md`、`feature-meta/qa-report.md`（QA 的）

## 本轮交付（修复轮 2）

承接 QA Pre-merge 报告（`feature-meta/qa-report.md`，结论 PASS-WITH-RISK）的修复轮，
范围严格限定在 QA 列出的 5 项，**未扩大范围**：

| 项 | QA 编号 | 处理 | 位置 |
|----|--------|------|------|
| manifest 结构级违规被静默吞掉 | 中 1 | 解析失败时把 serde 错误作为一条 `manifest_issues` 报出 + 2 测试 | `crates/xtask/src/modules.rs` |
| 依赖方向检查可被绕过 | 中 2 | 改读 `cargo metadata --no-deps` 的成员依赖图做可达性判定；`platform-contracts-macros` 纳入平台 crate + 3 测试 | `crates/xtask/src/deps.rs` |
| Schema 与平台校验器不一致 | 低 5 | schema 收紧到与实现完全一致（`id` / `dependencies[]` / 名称）+ 2 测试 | `crates/platform-core/src/extension/manifest{,_schema}.rs`、`capabilities/schema/manifest.schema.json` |
| sandbox 排序白名单外字段静默忽略 | 低 6 | 改为返回错误，README / AGENTS.md 写明 + 2 测试 | `modules/sandbox/backend/src/repository.rs` |
| manifest 的契约 pin 被冲掉 | 提示 7 | 补回 `contracts` / `adrs` / `delivered_by` | `feature-meta/manifest.json` |

> **需要 BA 同步**：设计摘要 §5 的 JSON Schema 仍是旧版，代码侧已收紧，
> 需 BA 按验证报告 §0.1 的字段清单同步（`id` / `dependencies[]` / `name` / `permissions[].name`）。

## 设计摘要 §7 的 7 项交付清单（首轮已完成，本轮未回退）

| 交付项 | 位置 |
|--------|------|
| 5 个扩展点 trait + 配套类型 | `crates/platform-core/src/extension/` |
| `inventory` 编译期注册 + `discover()` | `crates/platform-core/src/extension/registry.rs` |
| 契约套件 C-1 ~ C-4 | `crates/platform-contracts/src/checks.rs` |
| 简化版 `#[platform_module_contract]` 宏 | `crates/platform-contracts-macros/src/lib.rs` |
| manifest JSON Schema + 解析器 | `crates/platform-core/src/extension/manifest{,_schema}.rs`、`capabilities/schema/manifest.schema.json` |
| `xtask check-modules` 完整规则 | `crates/xtask/src/modules.rs`（+ `crates/xtask/src/deps.rs` 依赖方向检查） |
| 示例模块 `modules/sandbox/` | `modules/sandbox/**` |

## 自验结论（修复轮 2）

- `cargo check --workspace --all-targets` ✅
- `cargo test --workspace -- --test-threads=2` ✅ **73/73**（原 64 个全绿，只增不减：+9）
- `cargo clippy --workspace --all-targets -- -D warnings` ✅ 无告警
- `cargo fmt --all --check` ✅
- `cargo xtask check-modules` ✅ 找到并校验 `modules/sandbox`（1 个模块）；`manifest.json` = `{}` → exit 1 并指出 `missing field \`id\``
- 依赖方向 ✅ workspace 继承写法、`platform-contracts-macros` 两种绕过都被报出（验证后逐字节还原）
- `pnpm -r test` ✅ 23/23
- `bun run test:e2e` ✅ 1/1
- `pnpm exec eslint .` ✅ / `pnpm exec prettier --check .` ✅

## 新增依赖

- 第三方：**仅 `inventory 0.3.25`**（约 3KB）
- workspace crate：`platform-contracts-macros`（过程宏，只依赖标准库 `proc_macro`，零第三方依赖）
- 修复轮 2：`crates/xtask` 增加了 `serde`（解析 `cargo metadata` 的输出）——
  `serde` 早已在 workspace 与 `Cargo.lock` 里，**未引入任何新的第三方包**
- 未引入 `async-trait` / `sqlx` / `reqwest` / `aws-sdk` / `opendal` / `jsonwebtoken`

## 偏离（详见验证报告 §7）

首轮 11 项 + 修复轮 2 新增 5 项（§7 的第 12 ~ 16 条），核心 4 项：

1. trait 侧用 `-> impl Future<Output = ..> + Send` 展开写法（`async fn in trait`），
   调用点与实现方写法不变；
2. 新增零第三方依赖的过程宏 crate（`macro_rules!` 属性宏在 Rust 1.98 上仍是 unstable）；
3. 注册表元素用包装类型 `ModuleRegistration`（`inventory` 0.3 的 `Collect: Sized` 不允许
   `collect!(dyn Trait)`），对外 `discover()` 签名不变；
4. 新增 `TransactionalWrite` 作为 C-4 事务边界的**声明通道**（不是第 6 个扩展点）。

## 待 QA 补验

- 修复轮 2 的 5 项修复的复验（重点：两种依赖方向绕过 + manifest 结构级违规）
- CI 六个 job 实跑（本仓库无 remote，GitHub Actions 跑不起来）
- 冷编译峰值内存与耗时复测（本轮未清 `target/`、未做 RSS 采样）
- 覆盖率数字（本机无覆盖率工具，按低配机约束不 `cargo install`）
- 扩展点 trait 的 dyn-compatible 形态（本期不做，留给 REQ-015 / REQ-017 决策）
