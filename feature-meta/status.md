# REQ-003 状态

- **需求**: REQ-003 扩展点框架与契约测试
- **分支**: feature/REQ-003
- **基线**: develop @ `eacff21`（REQ-002 已合并）
- **状态**: **待验证**（Dev 自验全部通过，等待 QA Pre-merge）
- **开发者**: myg133-dev-1
- **完成时间**: 2026-11-18
- **产物**: `feature-meta/traceability.md`、`feature-meta/verification-report.md`

## 本轮交付

设计摘要 §7 的 7 项交付清单全部实现：

| 交付项 | 位置 |
|--------|------|
| 5 个扩展点 trait + 配套类型 | `crates/platform-core/src/extension/` |
| `inventory` 编译期注册 + `discover()` | `crates/platform-core/src/extension/registry.rs` |
| 契约套件 C-1 ~ C-4 | `crates/platform-contracts/src/checks.rs` |
| 简化版 `#[platform_module_contract]` 宏 | `crates/platform-contracts-macros/src/lib.rs` |
| manifest JSON Schema + 解析器 | `crates/platform-core/src/extension/manifest{,_schema}.rs`、`capabilities/schema/manifest.schema.json` |
| `xtask check-modules` 完整规则 | `crates/xtask/src/modules.rs`（+ `crates/xtask/src/deps.rs` 依赖方向检查） |
| 示例模块 `modules/sandbox/` | `modules/sandbox/**` |

## 自验结论

- `cargo check --workspace --all-targets` ✅
- `cargo test --workspace -- --test-threads=2` ✅ **64/64**（其中 REQ-002 原有 24 个全绿）
- `cargo clippy --workspace --all-targets -- -D warnings` ✅ 无告警
- `cargo fmt --all --check` ✅
- `cargo xtask check-modules` ✅ 找到并校验 `modules/sandbox`（1 个模块）；故意破坏后 exit 1 并列出缺失项
- 契约测试破坏性验证 ✅ C-1 / C-2 / C-3 / C-4 各自能拦住违规实现（验证后逐字节还原）
- `pnpm -r test` ✅ 23/23
- `bun run test:e2e` ✅ 1/1
- `pnpm exec eslint .` ✅ / `pnpm exec prettier --check .` ✅
- 依赖方向 ✅ `platform-core` / `platform-contracts` 的依赖图里没有业务模块路径

## 新增依赖

- 第三方：**仅 `inventory 0.3.25`**（约 3KB）
- workspace crate：`platform-contracts-macros`（过程宏，只依赖标准库 `proc_macro`，零第三方依赖）
- 未引入 `async-trait` / `sqlx` / `reqwest` / `aws-sdk` / `opendal` / `jsonwebtoken`

## 偏离（详见验证报告 §7）

共 11 项，核心 4 项：

1. trait 侧用 `-> impl Future<Output = ..> + Send` 展开写法（`async fn in trait`），
   调用点与实现方写法不变；
2. 新增零第三方依赖的过程宏 crate（`macro_rules!` 属性宏在 Rust 1.98 上仍是 unstable）；
3. 注册表元素用包装类型 `ModuleRegistration`（`inventory` 0.3 的 `Collect: Sized` 不允许
   `collect!(dyn Trait)`），对外 `discover()` 签名不变；
4. 新增 `TransactionalWrite` 作为 C-4 事务边界的**声明通道**（不是第 6 个扩展点）。

## 待 QA 补验

- CI 六个 job 实跑（本仓库无 remote，GitHub Actions 跑不起来）
- 冷编译峰值内存与耗时复测（本轮未清 `target/`、未做 RSS 采样）
- 覆盖率数字（本机无覆盖率工具，按低配机约束不 `cargo install`）
- 扩展点 trait 的 dyn-compatible 形态（本期不做，留给 REQ-015 / REQ-017 决策）
