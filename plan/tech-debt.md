# 技术债清单

- **维护者**: BA（myg133-ba-1）
- **创建**: 2026-11-19（REQ-011）
- **用途**: 记录**已发现、已核实、但本期明确不修**的问题。

## 为什么要有这份清单

Sprint 3 复盘的第 2 条教训：**修漏洞会制造新漏洞** —— 但另一半是
**发现的问题若不落到清单上，下一轮就会重新「发现」一遍，然后重新讨论一遍**。

本清单的每一条都带：证据位置、为什么本期不修、什么时候该修。

## 登记标准

一条技术债必须同时满足：
1. **已核实**（有文件:行号证据，不是推测）
2. **本期明确不修**（有理由：超范围 / 成本过高 / 有更大替代方案）
3. **有影响面判断**（说不清影响面的不算债，是待办）

---

## TD-001: `platform-org` 不读 `x-tenant-id` 头，跨租户头被静默忽略

- **发现于**: REQ-011（后端 Dev 在修 `/api/rbac` 同类问题时发现）
- **证据**: `crates/platform-org/src/http.rs:288` ——
  `RequestContext::new().with_tenant(auth.tenant_id.clone())`，
  ctx **只从令牌取租户**，从不读请求头
- **与 `/api/rbac` 的对照**: REQ-011 之前 `/api/rbac` 也是同样行为
  （`crates/platform-rbac/src/http.rs` 修复前不读头）；
  REQ-011 已按契约 C3 修复（头租户 ≠ 令牌租户 → **404**，不再静默忽略）
- **影响**: 客户端传了 `x-tenant-id: 别的租户` 时，
  `/api/org/*` **静默忽略**该头、按令牌租户执行 ——
  与 `/api/rbac/*` 修复后的行为**不一致**
- **为什么本期不修**: 属 `platform-org`（REQ-008 已合并），不在 REQ-011 范围；
  且修它会**改变既有接口的响应码**（可能从 200 变 404），需要单独评估
- **什么时候修**: 与 TD-002 一起，在「租户上下文一致性」专项里做
- **建议修法**: 与 `/api/rbac` 同构 —— 在 `authenticate` 中间件里读头并比对，
  不一致 → 404 `NOT_FOUND`（不泄露资源存在性）

---

## TD-002: 租户头校验只覆盖 `/api/rbac`，其他平台路由未统一

- **发现于**: REQ-011（TD-001 的推广）
- **证据**: `crates/platform-config/src/http.rs`（配置接口）、
  `crates/platform-iam/src/http.rs`（认证接口）——
  各自的租户上下文来源与 `/api/rbac` 不同
- **影响**: **同一个 `x-tenant-id` 头，在不同接口上语义不同** ——
  有的接口忽略它，有的接口按它 404。前端 SDK 只能「全都带上」，
  但**无法预期各接口的行为**（见 REQ-011 的 D93 同类问题：
  前端不能依赖未文档化的行为）
- **为什么本期不修**: 跨 4 个 crate 的行为统一，属架构级改动，
  需要逐接口核对契约 C3 的适用范围
- **什么时候修**: 「租户上下文一致性」专项，或 REQ-020（底座可用性验证）之前
- **建议修法**: 在 `platform-core` 提供**统一的租户上下文中间件**，
  所有平台路由用同一个；然后逐接口补测试

---

## TD-003: `platform-org` 的 9 个权限点无 `require_permission` 调用点（C-5 缺口）

- **发现于**: REQ-009（已登记，见 D58）
- **证据**: `crates/platform-org/src/http.rs` 的 9 个已登记权限点，
  `require_permission` 调用点为 **0**；`rbac:permission:read` 同样为 **0**
- **现状**: C-5 扫描**只扫业务模块**（`modules/`），不扫平台 crate ——
  所以这个缺口**不会被 CI 发现**
- **为什么本期不修**: REQ-009 已把「基线 = 0」如实登记；
  扩围到平台 crate 需要先给 C-5 造一个**不依赖 `ContractFixture`** 的入口
  （D58 已为 REQ-010 造过类似的扫描入口，可复用思路）
- **什么时候修**: 与 TD-001/TD-002 的「平台 crate 一致性」专项一起
- **风险**: 平台 org 接口**实际无鉴权**（只验令牌，不验权限点）——
  任何已认证用户都能调 org 写接口。**这是安全问题，不只是可维护性问题**

---

## TD-004: `Cargo test --workspace` 在本仓必然失败（既有陷阱）

- **发现于**: REQ-011（后端 Dev 踩到）
- **证据**: `platform-config` 的 24 条用例缺 `DATABASE_URL_TEST` 环境变量即 **panic**
- **影响**: 新人 / AI 助手按文档跑 `cargo test --workspace` 会看到一片红，
  误判为「代码坏了」或「环境没起好」
- **为什么本期不修**: 是**测试基建问题**，不是本需求问题
- **什么时候修**: 下一个碰测试基建的需求
- **建议修法**: 要么给这些用例加 `#[ignore]` + 显式说明，
  要么在 `cargo test` 的默认路径里跳过需外部环境的用例；
  同时把 `AGENTS.md` 的命令段改成 `./scripts/test-integration.sh`

---

## TD-008: `cargo test -p <crate>` 不加 `--all-targets` 会漏掉库里的单元测试

- **发现于**: REQ-011（BA 亲自踩到）
- **证据**: `cargo test -p xtask` → **0 tests**；
  `cargo test -p xtask --all-targets` → **49 tests**（含
  `catalog::tests::agents_doc_matches_catalog` ——
  守住「AGENTS.md 与能力目录逐行一致」的那条）
- **影响**: `xtask` 同时有 `lib.rs` 与 `main.rs` 两个 target，
  不加 `--all-targets` 时**只跑 bin target** → `lib.rs` 里的测试**一个都不跑**，
  而且**退出码仍是 0**（假绿）
  → 「AGENTS.md 与能力目录不一致」这条护栏**看起来在，实际不跑**
- **为什么本期不修**: 是**测试基建问题**；本期只改了 `catalog.rs` 与 `AGENTS.md`，
  已用 `--all-targets` 验证通过
- **什么时候修**: 下一个碰测试基建的需求（可与 TD-004 一起做）
- **建议修法**: ① 文档里统一写 `cargo test --workspace --all-targets`；
  ② 或给 `xtask` 加一个 `[[test]]` 目标把 lib 测试拉出来；
  ③ 长期看：这类「0 passed 却 exit 0」的陷阱应收进 `AGENTS.md` 的
  「已知假绿清单」

---

## TD-005: `platform-server` 的装配路径无集成测试覆盖

- **发现于**: REQ-011（后端 Dev 自行发现）
- **证据**: `crates/platform-server/src/main.rs` 的装配（IAM ↔ RBAC 的 provider 注入）
  只在 `main()` 里，集成测试**手动注入** → 装配代码本身**没被测过**
- **影响**: 装配写错了（如忘了注入 provider），集成测试**照样全绿** ——
  因为测试绕过了装配路径。REQ-011 的后端 Dev 因此补了**真机冒烟**
  （真实 `base serve` 进程）来补这个洞
- **为什么本期不修**: 真机冒烟已在本轮补上，但它是**手工的**，
  不在 CI 里
- **什么时候修**: REQ-005（交付基线）后续，或 REQ-020
- **建议修法**: 把「起真实服务 + 打几个关键接口」的冒烟做成 CI 里的一个 job
  （REQ-010 的 E2E 已是这个形态，可以扩展）

---

## TD-006: CI 从未真跑过（无 remote）

- **发现于**: Sprint 1（持续未解）
- **证据**: 本仓**无 git remote** → GitHub Actions 起不来；
  6 个 job（lint / test / build / dependency-audit / contract-check / e2e /
  migration-test / build-image）**全部未验证**
- **影响**: 所有 CI 里的门禁（镜像体积 < 100MB、Trivy 扫描、
  契约检查、迁移 up/down）**都只是「写在文件里」**，不是「跑过」
- **为什么本期不修**: 需要用户提供 GitHub / Gitea 地址
- **什么时候修**: 用户配好 remote 后立刻做
- **⚠️ 优先级**: **高** —— 这是整个交付基线的**唯一真实验证缺口**

---

## TD-007: 分支保护需人工配置

- **发现于**: Sprint 1
- **证据**: 代码里做不到 —— required check + Code Owners 必须在远端 UI 配置
- **影响**: 合并保护依赖**人的自觉**，而不是机制
- **为什么本期不修**: 需用户在远端配置
- **什么时候修**: 同 TD-006

---

## TD-009: `iam_real_db.rs` 的 Argon2 时序用例对 CPU 负载敏感

- **发现于**: REQ-011（QA Pre-merge 验证第 2 轮）
- **证据**: `crates/platform-iam/tests/iam_real_db.rs:706` 附近的用例断言两个
  Argon2 哈希的耗时比（实测 ratio **1.74** 时判红）
- **影响**: QA 在**自己起的 `base serve` 同时占 CPU** 的情况下跑回归 → 该用例失败；
  停掉服务后重跑**全绿**（550 passed）。
  **这不是产品缺陷，是测试本身对环境敏感** ——
  但在 CI 上多 job 并行时会**偶发红**，且红的时候看起来像「认证坏了」，
  会诱导人去查错误的方向
- **为什么本期不修**: 时序断言属测试基建；本期 QA 用「停掉并发负载后重跑」得到干净结论
- **什么时候修**: 下一个碰测试基建的需求（可与 TD-004 / TD-008 一起做）
- **建议修法**: 别断**绝对耗时比**。改断「**两次哈希的成本量级相同**」
  （如 ratio 落在 `[0.25, 4]` 这种宽松区间），或在 CI 里给这条用例打 `#[ignore]`
  单独在无负载的 job 里跑
