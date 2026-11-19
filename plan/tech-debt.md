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

---

## TD-010: 80 条 `#[ignore]` 集成用例依赖 CI 上一个**从未真跑过**的新 job

- **发现于**: REQ-011（合并后配 CI 时发现）
- **证据**: `cargo test --workspace` **不跑 `#[ignore]` 用例**。全仓 80 条：
  `platform-rbac/tests/rbac_real_db.rs` 22、`platform-iam/tests/iam_real_db.rs` 15、
  `platform-org/tests/org_real_db.rs` 14、`platform-rbac/tests/me_permissions.rs` 10
  （**REQ-011 的 AC-S-064/065 验收就这 10 条**）、其余 19
- **现状**: REQ-011 已新增 `integration` job 跑 `./scripts/test-integration.sh`
  来覆盖它们 —— 但该 job **从未真跑**（CI 无 remote，TD-006）
- **额外风险**: 该 job 依赖脚本的「主机直连 psql」分支，而开发机**没有 psql**，
  本地一直走 podman 分支 → **这个分支从未被执行过**
- **为什么本期不修**: 需要能真跑 CI 才能验证（依赖 TD-006 的 remote）
- **什么时候修**: 配好 remote 后**第一次跑 CI 时优先看这个 job**
- **若它红了**：先看是不是 psql 分支的问题（脚本里 `HOST_HAS_PSQL` 的两条路径），
  再核 `cannot_reset` 是否按预期在 CI 下报错

---

## TD-011: 前端单测的部分用例依赖 `dist` 是否存在（`skipIf(!built)`）

- **发现于**: REQ-011（合并后 post-merge 回归时发现）
- **证据**: `packages/sdk/test/packaging.test.ts:74` 的
  `const built = PACKAGES.every((n) => existsSync(.../dist))`，
  配合 `:118` 的 `describe.skipIf(!built)`
- **现状**: 已在 CI 的 `test` job 里加了 `pnpm -r build`（`pnpm -r test` 之前）
- **残留风险**: **本地**开发者若直接 `pnpm -r test` 而没先 build，
  那 2 条会**静默跳过**（不报错、不提示）→ 本地看起来 149 passed，
  实际 151 里少跑 2 条
- **为什么本期不修**: 已用 CI 侧修法覆盖；本地行为是「有 dist 就跑、没有就跳过」，
  属于有意的降级设计
- **什么时候修**: 下次碰前端测试基建时
- **建议修法**: 把 `skipIf` 改成「缺失时**显式告警**」而不是静默跳过
  （例如在 `beforeAll` 里 `console.warn` 一句，或让 vitest 输出一个 skipped 的原因），
  这样「少跑了」是**可见的**

---

## TD-012: 只读用户仍见「新建」按钮（点开提交 403）

- **发现于**: REQ-012（复核 L-3）
- **证据**: `apps/shell/src/components/SchemaTable.svelte` 的「新建」按钮按
  `api.create` 是否存在渲染，**不按写权限**判断
- **影响**: 只有读权限的用户能看到「新建」，点开后提交才 403。
  不算越权（服务端拦住了），但**体验差且暴露了操作存在性**
- **为什么本期不修**: 属 REQ-011 的 `<Can>`（**按钮级权限**）范畴，
  不是本需求的渲染器问题。元数据里没有「这个按钮需要什么权限点」的声明
- **什么时候修**: 需要给 schema 的 `api` 子段加 `permission` 字段（如
  `api.create.permission`），是**元数据形状的扩展**，应单独立项
- **建议修法**: `api.create` / `update` / `delete` 各自可声明 `permission`；
  渲染器用它 + `can()` 决定是否渲染按钮。**服务端已有 403 兜底，所以这是体验问题不是安全问题**

---

## TD-013: 数据列表头缺 `scope="col"`（无障碍）

- **发现于**: REQ-012（复核，已修数据列；此处记录**遗留的通用性**）
- **证据**: `SchemaTable.svelte` 的数据列 `<th>` 现在**已**有 `scope="col"`
  （复核后发现并修复）；但 `@base/ui` 的 `Table.svelte` 表头**没有**
- **影响**: 屏幕阅读器无法把数据单元格关联到表头
- **为什么本期不修**: 属 REQ-011 的 `@base/ui` 产物
- **什么时候修**: 下次碰 `@base/ui` 时
- **建议修法**: `Table.svelte` 的表头统一加 `scope="col"`

---

## TD-014: `AC-S-086` 步骤 8（请求路径白名单）只做了人工核对，未入 E2E

- **发现于**: REQ-012（复核）
- **证据**: 复核用真实浏览器**人工确认**了请求都落在
  `/api/(schema|sandbox|config|iam|rbac)`，但这条**没有写成自动化断言**
- **影响**: 下次有人往页面里加一个未声明的请求，**不会被自动抓到**
- **为什么本期不修**: 白名单断言需要「基线请求剔除」（branding/health/assets），
  实现成本不低；已人工验过一次
- **什么时候修**: 下次扩展渲染器时

---

## TD-015: `packages/schema` 的 10 处中文文案未走 i18n

- **发现于**: REQ-021（前端 Dev 报告，BA 核实）
- **证据**: `packages/schema/src/index.ts` 有 **10 处中文代码字面量**（排除注释后逐行核实）：
  - `describeField` → `${label}（必填）`
  - `validateForm` → 8 条校验文案（`不能为空` / `必须是数字` / `不能小于` / `不能大于` /
    `不能超过…个字符` / `不能少于…个字符` / `格式不正确`）
  - `formatCell` → boolean 的 `是` / `否`
  - `unknownTypeWarning` → 降级告警
- **影响**: 切到 `en-US` 后，**表单校验错误与 boolean 显示仍是中文** ——
  用户可见文案没国际化
- **为什么本期不修**（D117）: `packages/schema` 是 **REQ-012 的交付物**，且是
  **`deps: {}` 的零依赖纯逻辑包**。迁它要：① 给零依赖包引入 i18n 依赖；
  ② 改 REQ-012 的 API 签名（`validateForm` 返回 `string` → 改成返回错误码）。
  两者都是**跨需求的结构性改动**，超出 REQ-021 的范围
- **已经做的准备**: `@base/i18n` 预留了 **`schema.*` 键域**，键名空间已备好
- **什么时候修**: 下次动 `@base/schema` 的 API 签名时（与 REQ-012 侧的渲染器重构一起做）

---

## TD-016: 金额的 JSON 传输精度上限是 2^53，不是 `i64` 的上限

- **发现于**: REQ-021（前端 Dev 报告，BA 核实）
- **证据**: `Money.minor_units` 是 `i64`（上限 ~9.22×10^18 最小单位 ≈ 9.2×10^16 元），
  但 **JSON `number` 只能精确表示到 2^53 ≈ 9.007×10^15**
- **影响**: 金额超过 **9.0×10^13 元** 时，**JSON 解析阶段就丢精度** ——
  还没到 `Money` 的算术就已经错了。前端 `formatMoney` 对非安全整数抛 `RangeError`
  （**抛错而非静默截断**，行为正确）
- **为什么本期不修**（D118）: 改契约要动后端序列化 + 前端类型 + 全部调用点，
  是**跨需求的结构性改动**。本轮的真实上限是 **JSON 的 2^53**，不是 `i64` 的
- **什么时候修**: 需要支持超 2^53 金额时 —— 契约改为「金额用字符串传输」
  （后端 `minorUnits` 输出十进制字符串，前端用 `BigInt` 解析）

---

## TD-017: `cargo fmt --all --check` 在 `develop` 基线上就是红的（8 个存量文件）

- **发现于**: REQ-021（修复轮报告，BA 在基线 `develop` @ `88b7ca3` 上独立复现）
- **证据**: 在**未改动的** `develop` worktree 上跑 `cargo fmt --all --check` → **11 个文件**不合规。
  本轮改动后降到 **8 个**（修复轮顺手修好了本轮碰过的 2 个）：
  ```
  crates/platform-contracts/src/checks.rs
  crates/platform-contracts/src/schema_scan.rs
  crates/platform-contracts/tests/c6_schema_projection.rs
  crates/platform-schema/src/registry.rs
  crates/platform-schema/tests/metadata_api.rs
  modules/sandbox/backend/src/demo_item_http.rs
  modules/sandbox/backend/src/demo_item.rs
  modules/sandbox/tests/demo_items.rs
  ```
  **本轮新增的文件全部合规**（0 处 i18n 相关 diff）
- **影响**: CI 的 lint job 会红 —— 但**本仓 CI 从未真跑过**（无 remote），
  所以这个红一直没人发现
- **为什么本期不修**: 要改 8 个**与本需求无关**的文件（REQ-002/010/012 的产物），
  会把本需求的 diff 污染成「一大坨无关的格式化改动」，评审时难以分辨真改动
- **什么时候修**: 单独一个「格式化专案」：一次 `cargo fmt --all` + 单独提交 + 单独验证。
  **不要**混在功能需求里 —— 那正是本条要避免的

---

## TD-018: `scripts/e2e-server.sh` 不注入认证密钥 → 干净环境里 E2E 根本起不来服务

- **发现于**: REQ-021（BA 亲自跑 E2E 时撞上）
- **证据**: `scripts/e2e-server.sh`（在 `develop` 基线 `88b7ca3` 上就是如此，**不是本轮引入**）
  **完全没有**注入 `BASE_JWT_PRIVATE_KEY` / `BASE_JWT_KID` / `BASE_JWT_ISSUER` /
  `BASE_JWT_AUDIENCE` / `BASE_IAM_SECRET_KEY`。
  → 在**干净环境**里 `base serve` 在启动自检处失败：
  ```
  ✗ 认证与账号体系初始化失败：[INTERNAL_ERROR] ... the JWT signing key is not a
    valid PKCS#8 EC key: InvalidKeyFormat
  ```
  → Playwright 的 webServer 拿不到服务 → 页面收到**空响应** →
  SDK 报 `client.INTERNAL_ERROR`（"响应无法解析，请稍后重试"）→
  **13 条 E2E 一起红**，看起来像「前端坏了」，其实是**服务没起来**
- **为什么之前没发现**: REQ-011/012 的 E2E 之所以能过，是因为 BA **手工**起过带密钥的服务，
  Playwright 的 `reuseExistingServer: true` **复用了它** —— 掩盖了这个缺口。
  **CI 从未真跑过**（本仓无 remote）→ 没人发现
- **本轮已修**: `scripts/e2e-server.sh` 现在会**现生成**临时密钥
  （JWT = PKCS#8 EC；IAM = base64url），不落盘、不是生产密钥。
  修后**干净环境**实测：`npx playwright test e2e/` → **20 passed / 0 failed / 0 skipped**
- **仍然存在的风险**: 密钥是**每次运行现生成**的 —— 意味着
  **每次 E2E 都会让已有的 token 失效**。若将来有「跨运行的会话持久化」类用例，
  需要改成固定密钥或把密钥落进 `target/`（不入库）
