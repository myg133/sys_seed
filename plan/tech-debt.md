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

## ~~TD-003~~: `platform-org` 的 9 个权限点无 `require_permission` 调用点（C-5 缺口）

- **发现于**: REQ-009（已登记，见 D58）
- **证据**: `crates/platform-org/src/http.rs` 的 9 个已登记权限点，
  `require_permission` 调用点为 **0**；`rbac:permission:read` 同样为 **0**
- **现状**: C-5 扫描**只扫业务模块**（`modules/`），不扫平台 crate ——
  所以这个缺口**不会被 CI 发现**
- **风险**: 平台 org 接口**实际无鉴权**（只验令牌，不验权限点）——
  任何已认证用户都能调 org 写接口。**这是安全问题，不只是可维护性问题**

### ✅ 已解决（2026-11-23，develop 全量独立 QA 后）

**触发**：独立 QA 的 A 片（安全核心）实测复现了这条债的**实际危害** ——
只持 `app:registry:read`、无任何 `org:*` 权限的只读用户，
`POST /api/org/org-node` 拿到 **201 Created**，组织节点真的被写入。

**修法**（两项一起）：
1. **org 的 9 个权限点全部落地**：按语义分 10 组 + `route_layer(require_permission(...))`
   （`crates/platform-org/src/http.rs`）。权限点与 manifest 的差集为空。
   注意 `require_permission` 必须**直接调用**（不能写全路径
   `platform_core::...::require_permission`）—— 扫描器会跳过前一个字符是 `:` 的调用。
2. **C-5 扩到 8 个有 manifest 的平台 crate**：各加 `tests/contract_c5.rs`，
   直接用 `check_c5_for(manifest, src)`（纯函数形态，不需要 `ContractFixture`）。
   → 正是这一步让「扩围到平台 crate」这件被推迟的事真正完成。

**额外补的判据**（比 C-5 更严）：`every_route_group_has_a_permission_layer` ——
按 `let x = Router::new()` 切组，断言每个挂了 `.route()` 的组都有权限层。
C-5 只验「权限点有没有调用点」，**抓不住「新端点了但忘了挂」**；
而 QA-A 指出 `/api/org` **零 HTTP 层测试**，正是这个盲区。
变异验证：去掉 org 的 scim-sync 鉴权层 → 立刻红并点名 `scim_sync` 组。

**⚠️ `rbac:permission:read` 的 0 调用点不是缺陷**（这条要单独记住）：
REQ-009 的 AC 明文豁免 `/api/rbac/permissions`（「任何已认证用户都应能读，
否则前端 `can()` 无从判断」）。我一度按 C-5 的报错给它加了鉴权，
被现有测试 `a_plain_user_cannot_reach_the_management_api` 打红后撤回 ——
**C-5 报错 ≠ 代码错，必须先查验收标准**。
现由 `crates/platform-rbac/tests/contract_c5.rs` 的 `EXEMPT` 清单承载，
每条豁免必须写清依据文件 + 行号，且有「依据不得少于 40 字符」的自检。

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

---

## TD-019: 已注册的 locale 在翻译包变更后可能变成「未注册」，而旧值仍留在库里

- **发现于**: REQ-021（独立复核的 F-3，BA 复核后**判定不可达**，但机制值得记一笔）
- **机制**: `locale` 的 `format` 校验（`crates/platform-config/src/registry.rs`
  的 `is_registered_locale`）在**写入时**要求值必须是**已注册**语言。
  已注册集合 = `packages/i18n/src/locales/*.json` 的**文件名**。
  → **写入未注册的语言会被 400 拒绝**（这是对的）。
  → 但若**后来**从 `locales/` 里删掉某个语言文件，**库里已有的旧值**不会自动清理，
  而协商层会把它当「未注册」**丢弃** → 那时 `/api/i18n/context` 与
  `/api/config/entries` 会给出**不同答案**（D119 修的正是这一类不一致）
- **为什么本期不修**: 本轮只有 `zh-CN` / `en-US` 两个语言，**没有删语言的场景**；
  补「删除语言时清理或迁移存量值」属于**语言包生命周期的配套能力**，本轮无对应需求
- **什么时候修**: 接入多语言包管理时（那时会有人真的增删语言）
- **补充**: 复核者把这条报成 F-3「未注册 locale 被丢弃导致两路由答案不同」——
  BA 复核后确认**该情形当前不可达**（写不进去），但机制本身成立，故登记。

---

## TD-020: 真实库夹具在租户级留 3 行残留

- **发现于**: REQ-021（独立复核的 F-6）
- **证据**: `crates/platform-server/tests/i18n_settings_real_db.rs` 的夹具在**平台层**做了清理，
  但**租户级**（`platform_tenant_config`）留了 3 行
- **影响**: **自愈**（下次跑会覆盖），且不影响其它用例 —— 但会让「库里恰好干净」这种
  **隐式依赖**继续存在。REQ-021 的修复轮2 已把「租户级也清理」补进新夹具，
  但**旧夹具**仍有残留
- **为什么本期不修**: 影响极小（自愈）；且旧夹具正在被新夹具取代
- **什么时候修**: 顺手改，或下次动该文件时

---

## TD-021: `router.ts` 的 `start()` 会抹掉查询串（**本轮已修**，但记录成因）

- **发现于**: REQ-013（前端 Dev 实现 SSO 回调页时踩到）
- **问题**: `apps/shell/src/lib/router.ts` 的 `start()` 无条件
  `history.replaceState(null, '', location.pathname)` —— **只写 pathname**，
  把 `?ticket=...` 抹掉。SSO 回调页因此读不到一次性凭据。
- **这是 **REQ-011 交付物**的既有缺陷，不是本轮引入的**：
  之前没有任何页面依赖查询串，所以从未暴露。REQ-013 的回调交接（P4）是第一个。
- **本轮已修**: 同路径写回时保留 `search` / `hash`；`apps/shell/test/router.test.ts` 加了回归用例。
- **为什么仍然记一笔**：
  1. 让复核者知道「为什么一个 SSO 需求动了 router」——**不是顺手改，是必要修复**
  2. 这个 bug 的性质值得记住：**`replaceState(pathname)` 是常见写法，但它丢查询串**。
     任何后续依赖 URL 参数的页面都会踩同一个坑
  3. 它同时说明一件事：**单测只传 `pathname` 的假件看不出这个问题**，
     前端 Dev 是在真浏览器里发现的。与 §9.7「AC 的前置必须在真界面上试一遍」同源。
- **同类风险（未验证）**: 直接改 `window.location.href` 的地方是否也会丢参数 —— 本轮未全仓排查。

---

## TD-022: `client_secret` 是**单环境变量、多租户共用** —— 多 IdP 场景下是真错

- **发现于**: REQ-013（**Dev 主动报告**，不在 AC 覆盖范围内）
- **问题**: OIDC 的 `client_secret` 只从环境变量 `BASE_SSO_OIDC_CLIENT_SECRET` 读，
  **所有租户共用同一个 secret**。但 SSO 是**租户级**能力（`scope: tenant`），
  每个租户应该配自己的 IdP 与自己的 client。
- **为什么本轮不修**: AC 没覆盖（mock IdP **不校验** secret），所以验收会过；
  改造成租户级加密列需要：新列 + 迁移 + 用 REQ-007 的 `crypto::encrypt`（已就绪）+ 写入接口
  （而写入接口要连带权限点三件套，D27）。**属于独立工作量。**
- **风险等级**: **高** —— 这是「验收能过、生产会坏」的那一类。
  生产前**必须**做租户级加密列。
- **现状的适用范围**: 只有在「**全平台只有一个 IdP**」的部署下是对的（比如整个底座对接一个 Azure AD）。

---

## TD-023: JWKS / 发现文档缓存**无淘汰**（`Mutex<HashMap>` 无界）

- **发现于**: REQ-013（Dev 主动报告）
- **问题**: 按 `issuer` / `kid` 做的 JWKS 与 `.well-known` 缓存是 `Mutex<HashMap>`，
  **只增不减**。租户数 × IdP 数增长时内存无界。
- **为什么本轮不修**: 单 IdP 单租户场景下条目数是个位数；
  加淘汰要引入 TTL / LRU（可能是新依赖，或手写一个够用的）。
- **什么时候修**: 多 IdP 或大量租户接入时。修之前先量一下实际条目数。

---

## TD-024: LDAP 无连接池；`ldaps://` 不支持（真实 AD 大多要 LDAPS）

- **发现于**: REQ-013（Dev 主动报告 + BA 在 design §7 主动排除）
- **两件事，但同源**（都是「LDAP 侧只做了够用的一半」）：
  1. **无连接池**：每次登录新建连接（`LdapConnAsync` + `drive!`），用完即弃。
     登录是低频操作，开销可接受；高频场景需要连接池。
  2. **不支持 `ldaps://`**：`ldap3` 本身有 `tls-rustls` feature，且 **D123 之后 TLS 栈已在树里** ——
     所以「树里没有 TLS」这个理由**已经不成立**。真实理由是：启用它并做客户端证书校验
     是**独立的 LDAP 侧工作量**，本轮不扩范围。
     ⚠️ **但真实 Active Directory 绝大多数要求 LDAPS** —— 所以这条会挡住真实 AD 接入。
- **什么时候修**: 接第一个真实 AD 客户之前。

---

## TD-025: `platform-iam` 有 2 条 clippy 警告（设计取舍，非缺陷）

- **发现于**: REQ-013 合并前补跑 clippy（修复轮自陈「未跑 clippy / fmt」）
- **两条**:
  1. `sso/config.rs:153` —— `large size difference between variants`（枚举变体大小差异大）
  2. `sso/oidc.rs:237` —— `very complex type used. Consider factoring parts into type definitions`
- **为什么本轮不修**: 两条都是**设计取舍**，不是本轮引入的样式疏漏。
  ① 变体大小差异要求把大变体装箱（`Box`），那会引入一次堆分配 —— 为一个 lint 付运行时成本不值得；
  ② 复杂类型是 `hyper` 的 connector builder 链式调用的返回类型，抽 `type` 别名会**降低**可读性。
- **本轮已修掉的是 6 条真正的样式问题**（`.is_some()` → `contains_key`、`>= y + 1`、无效果的 `path statement`），
  新增的 17 个文件已全部 `cargo fmt` 过。
- **`cargo fmt --all --check` 仍红 11 个文件** —— 全是 **TD-017 的存量债**（基线 @ `59486d0` 实测 11 个），
  本轮**没有增加**，且已把它们从格式化中排除（只格式化自己新增的文件）。

---

## TD-026: `Ldap3Conn::simple_bind` 的**网络层**仍无测试（客观边界）

- **发现于**: REQ-013（修复轮2 自陈）
- **已闭合的部分**: 决策点已从不可测的 async 包装层**下沉到 service 层**（D129），
  所以「把 `bind_result` 判定改坏」现在会被**集成测试**直接抓到 —— 我用变异验证亲自确认
  （`!bind_result(...)` → `.is_err()` 后 4 条集成用例红，而 lib 单测 76 条仍绿）。
- **仍未覆盖的**: `self.ldap.simple_bind(..).await` 这行**真的发出 LDAP 报文**的行为。
  `ldap3` **没有服务端实现**，要覆盖得起一个 mock LDAP server（属新依赖范畴）。
- **为什么可以接受**: ① `rc → bool` 的映射已被 `bind_succeeded` / `bind_result` 逐值单测覆盖；
  ② 「有没有调用它」已被 service 层集成测试覆盖；③ 剩下的只是「`ldap3` 库本身对不对」，
  那不是本仓的代码。
- **风险**: 若 `ldap3` 升级改了 `simple_bind` 的返回值语义，本仓不会红 —— 但 `bind_result` 的
  单测里有一条 `bind_succeeded_agrees_with_ldap3_success_semantics` 直接对照该库的 `success()`，
  能抓到语义漂移。

---

# REQ-014 引入的技术债（2026-11-20）

> 以下 9 条全部在 **REQ-014 规格阶段**就已识别并**明确接受**
> （裁决见 `BA/demands/REQ-014-audit/decisions.md` rev 2 的 D145~D168）。
> 它们不是「实现缺陷」，是**设计取舍的代价**。

## TD-027: 哈希链**按租户分链** → 无法证明「全平台审计未被整体替换」

- **发现于**: REQ-014 规格（D130）
- **证据**: 决策 D130 —— `prev_hash` 取同租户上一条；每租户独立 genesis。
- **为什么本轮不修**: 全局单链要求写入时能读**跨租户**的最后一条 → 破坏 RLS 隔离，
  且让租户 B 的写入依赖租户 A 的写入历史（跨租户耦合 + 性能不可用）。
- **风险**: 攻击者若能**整体替换某个租户**的审计段并重算该段的哈希链，
  **跨租户的链校验发现不了**。缓解：单租户内的链仍是完整的；追加型权限 + 备份 + 归档表留痕。

## TD-028: `before` / `after` 存 **TEXT** → 按内容检索需 `before::jsonb` 转换

- **发现于**: REQ-014 规格（D141）
- **证据**: 决策 D141 —— 列类型是 `TEXT`，存规范化 JSON 文本。
- **为什么本轮不修**: 哈希必须覆盖**存进列里的那段文本**，否则校验方要重新实现
  JSON 序列化规则 → 差一个字节就「链校验失败」与「实现有 bug」无法区分 → 假绿。
- **风险**: 按 `before`/`after` **内容**做检索的查询会**不能用索引**（需 `::jsonb` 表达式）。
  本轮审计查询的过滤维度是 actor/action/object/时间，**不受影响**。

## TD-029: 审计表的 RLS policy **手写**，不走 `cargo xtask tenant-policies` 生成器

- **发现于**: REQ-014 规格（D166）
- **证据**: 决策 D166 —— 生成器走 `tenantScoped.tables` 的**逻辑表名**
  （既有 `org_node` ↔ 物理 `platform_org_node`），与审计表的对应关系未验证；
  且生成器是**运行时**行为 → 测不到。
- **为什么本轮不修**: 形态完全可控 + 可测 > 与生成器机制一致。
- **风险**: 新增/迁移审计表时，**必须记得**手写 policy；
  忘了 → RLS 不生效 → 跨租户可见。**缓解**：AC-S-126 的 RLS 用例会在迁移时兜住。

## TD-030: 平台里**三个** recorder 概念并存

- **发现于**: REQ-014 规格（D135）
- **证据**: `platform_core` 的通用 `AuditSink` + `platform-org` 的 `AuditRecorder`
  （绑死 `OrgAuditEvent`）+ `platform-iam` 的 `SsoAuditRecorder`（绑死 `SsoAuditEvent`）。
- **为什么本轮不修**: 改后两者的 trait 签名会连带破坏 REQ-008 / REQ-013 的既有测试
  （`platform-iam` 129 条用例、`platform-org` 的 real-db 用例）。
- **风险**: 概念多 → 新人不知道该用哪个 → 可能又自建一个。
  **缓解**：`AGENTS.md` 能力目录 + `capabilities/extension-points.json` 明确指向通用入口。

## TD-031: `audit.retention.archive_days` **只登记不执行**（不物理删除）

- **发现于**: REQ-014 规格（D137 / §8）
- **为什么本轮不修**: 物理删除超出归档期的记录 = **删数据**，风险与收益不匹配。
- **风险**: 归档表会**无限增长**。**缓解**：本轮已有归档动作（把在线数据搬进归档表），
  归档表本身需要独立的保留策略 —— 待后续 REQ 处理。

## TD-032: 归档与链校验都**没有常驻调度器**，需外部定时任务调用

- **发现于**: REQ-014 规格（D137 / D164）
- **证据**: 本仓**没有**调度器基础设施；「定期校验」由外部调度器调
  `GET /api/audit/verify` 或 `cargo xtask audit verify` 实现。
- **为什么本轮不修**: 为归档/校验引入常驻进程 → 底座变重 + 低配机不接受 + **测不到**。
- **风险**: **没人调用就永远不归档 / 永不校验** → 链断裂可能长期不被发现。
  **缓解**：归档接口的响应里返回 `nextRecommendedAt`，让运维看得见。

## TD-033: 审计写入失败 = **全平台登录不可用**

- **发现于**: REQ-014 规格（D157）
- **证据**: 决策 D157 —— sink 写失败 → **拒绝登录**并返回 503 `AUDIT_SINK_UNAVAILABLE`。
- **为什么本轮不修**: 这是**刻意的可用性取舍** —— 放行 = 「有登录、无审计」，
  父 AC-022 的「完整且不可篡改」承诺被打破，而运维从日志里看不出「这次登录没被记录」。
- **风险**: 审计表写不动 → **所有人登不进去**。这是单点故障。
  **缓解**：审计写入失败率告警 + 一个降级开关（**本轮不做**）。

## TD-034: `clock_timestamp()` 依赖系统时钟，NTP 回拨会破坏链的时间单调性

- **发现于**: REQ-014 规格（D143 / D161）
- **证据**: 决策 D143 —— `created_at` 由 DB 的 `clock_timestamp()` 生成（不用 `now()`，
  因为 `now()` 是事务开始时间，同事务多条审计会撞时间戳）。
- **为什么本轮不修**: 改用单调序列（`BIGSERIAL`）要**新增一列**，
  而契约 C12 把审计表的字段**钉死为 13 个**，加列等于改契约（需升版本）。
- **风险**: NTP 回拨（罕见）会让 `created_at` 倒退 → 接续校验**告警**（**不是数据丢失**）。
  **缓解**：链的**正确性**不依赖时间（依赖 `prev_hash` 链），时间只用于**排序**。

## TD-035: 审计里的 `ip` 是**客户端可控头**，不是可信的物理来源

- **发现于**: REQ-014 规格（D149）
- **证据**: `ip` 取 `x-forwarded-for` 的第一跳（既有惯例
  `crates/platform-iam/src/http.rs:267-269`）；任何客户端都能伪造该头。
- **为什么本轮不修**: 本仓**没有**可信的反向代理来源保证（无 TrustedProxy 配置、
  无直连探测）；要可信得做代理信任链配置 + 直连 IP 判定 → 属部署域改造。
- **风险**: 审计里的 `ip` **不能用于取证**，只能用于**关联与排查**。
  规格与代码注释**必须如实写明**，不得写成「来源 IP 即真实客户端地址」。
  **缓解**：`platform-server` 部署在反向代理后时由部署方在代理层覆盖该头。


## TD-036: 配置键的「manifest ↔ DB」**没有启动自检**，登记值可能静默消失

- **发现于**: REQ-014 修复轮（D187 / 复验发现 M-01）
- **证据**: `crates/platform-config/tests/common/mod.rs` 的 `reset_fixture` 原本
  `DELETE FROM platform_config`（**整表**）→ 把 `audit.*` 的登记值一起删掉；
  而 `migrations/20261120000400_audit_config_seed.up.sql` 已进 `_sqlx_migrations`
  **不会重跑** → **没有任何东西会补回**，登记值在共享测试库里静默消失。
  仓内**没有**任何断言检查「manifest `config` 段的键在 `platform_config` 里都有行」。
- **为什么本轮不修**: 加「启动自检」是**平台级**改动（对所有 crate 的配置键生效），
  超出 REQ-014 的范围；且在当前测试库上会立刻让一批测试变红 → 需要单独一轮。
- **风险**: 任何新增配置键若忘了迁移 INSERT，或被某个测试夹具误删 →
  **配置面板显示的值与运行时行为不一致**（登记在骗人），且**没有任何测试会变红**。
  与 D27（权限点漏登记 → 启动失败）是同一类问题，但配置侧目前**没有**这条保护。
- **缓解**: 本轮已把 `platform-config` 的夹具复位收窄到「只删自己管的键」（D187），
  根除触发源；自检本身待专门一轮。
- **建议修法**: 在 `platform-config` 的启动装配里加一条自检 ——
  manifest `config` 段的每个键在 `platform_config` 里都有行 → 不一致就 fail-fast
  （对齐 D27 的思路）。

## TD-037: 审计查询把**所有** DB 错误都映射成 503，无法区分「SQL 拼错」与「DB 不可达」

- **发现于**: REQ-014 修复轮（D185 / 复验发现 B-02）
- **证据**: `crates/platform-audit/src/query.rs` 的查询路径用 `unavailable()` 把
  任何 DB 错误映射成 `AUDIT_SINK_UNAVAILABLE` → HTTP 503。
  B-02 的过滤子句拼错（`tenant_id = $1actor_id = $2`）因此表现为
  **「数据库不可用」**而不是「请求非法」→ 排查方向被带偏。
- **为什么本轮不修**: 拼进 SQL 的列名与 op 都有**白名单**（`column_of` / `FilterOp::parse`），
  用户输入**拼不出**非法 SQL → 拼错只可能是代码缺陷，**不是用户错误** →
  把「SQL 语法错」映射成 400 会把**代码 bug** 伪装成**用户错误**，反而更难查。
- **风险**: 未来若白名单放宽（例如支持用户自定义列），用户输入就能触发 SQL 错误 →
  会以 503 呈现，**客户端无法区分**「重试有用」与「请求本身非法」。
- **缓解**: 现有白名单 + 本轮已把渲染逻辑修对；F2 补的**真库断言**会在
  「拼错」发生时**立刻变红**（而不是等到线上 503）。

## TD-038: 6 个错误码在 i18n 词库里，**但不在 `TITLES`** —— 一旦被接到 HTTP 响应上，`title` 会兜底成 `"Error"`

- **发现于**: REQ-014 修复轮（D194 / F10 判据订正）
- **证据**: 两侧的集合差（BA 已逐条读源码核实）：
  - `TITLES` = **78 个错误码** + 6 个 `org.*` **组织事件码** = 84；
  - i18n `error.*` = 那 78 个 + **6 个当前不进 HTTP 响应体**的码 = 84
    （两个文件 `packages/i18n/src/locales/{en-US,zh-CN}.json` 键集一致）。
- **这 6 个码**：
  | 码 | 出处 | 为什么当前不进响应体 |
  |---|---|---|
  | `CONFIG_MISSING_REQUIRED` / `CONFIG_INVALID_VALUE` / `CONFIG_FORBIDDEN_COMBINATION` | `crates/platform-server/src/config.rs:16-20` | **启动期**环境变量诊断（`main.rs` 的 `Config(ConfigError)`）—— 启动就失败了，没有 HTTP |
  | `INVALID_CONFIG_KEY` / `INVALID_IDENTIFIER` / `IO_ERROR` | `crates/platform-core/src/error.rs:31-33`（`CoreError::code()`） | 全仓**无任何地方**把它转成 `ProblemDetails` |
- **为什么本轮不修**: 它们当前**不出现**在响应体里 → 在 `TITLES` 里缺席是**正确的**；
  补进去会让「有标题」变成谎称。
- **风险**: 将来若把其中任一码接到 HTTP 响应上，`title_of_code` 会走
  `FALLBACK_TITLE` = **`"Error"`** → 响应体的 `title` 变成泛泛的 `"Error"`，
  而前端**有**本地化译文（i18n 词库里已备好）→ 两侧表现不一致
  （前端显示中文具体文案，API 的 `title` 是 `"Error"`）。
- **缓解**: 本轮已把这条**变成机械可判的**：新增断言「`TITLES` 的错误码都有 i18n 条目」
  → 一旦有人给这 6 个码补了 `TITLES` 行而没同步 i18n，**测试会红**。
- **建议修法**: 若将来要把这 6 个码接到 HTTP 响应上，
  **必须同时**补 `crates/platform-core/src/titles.rs` 的 6 行（ASCII 标题）
  + 更新 `crates/platform-server/tests/i18n_titles.rs` 的行数钉死值。

## TD-039: E2E **不可从零复现** —— `scripts/e2e-server.sh` 建不出一个能跑的库

- **发现于**: REQ-014 修复轮 2（第 2 轮 Dev 报 U-1，BA 已独立复核）
- **证据（BA 已核实）**：
  | 事实 | 位置 |
  |---|---|
  | `migrations/` 里 **`base_app` 零命中** | `grep -rn "base_app" migrations/` → **0** |
  | 应用角色（`base_app` / `base_platform_app`）与全部表授权**只**在这个部署脚本里 | `deploy/seed/02-app-role.sql:20/34/40/49-51` |
  | sandbox 的表**不在**平台 `migrations/` 下 | `modules/sandbox/migrations/`（7 个迁移文件） |
  | E2E 启动脚本**只**做三件事：迁移 → 播种 → 起服务 | `scripts/e2e-server.sh`（`base migrate up` → `scripts/seed-e2e.sh` → `base serve`） |
  | **实测**：无历史残留时 `base serve` 以 `permission denied for table platform_permission` 退出 | 第 2 轮 Dev 实测，BA 复核脚本内容一致 |
- **为什么本轮不修**: 改 `e2e-server.sh` 要动**部署域**的脚本与角色初始化顺序，
  超出 REQ-014（审计日志）的范围；且它影响**所有**需求的 E2E → 属平台基建的独立一轮。
- **风险（这轮已经咬过我们一次）**：E2E 只能在「被手工预热过的库」上跑通 →
  ① 结果**不可复现**；② **脏库会掩盖缺陷** ——
  第 2 轮 Dev 报「35 passed / 0 failed」其实是**脏库**（tenant-a 累积到 42 条）下的结果，
  独立复核实测**干净库**上「回退」用例**必然失败**（H-01）。
  → 这正是「计数/状态都绿，但不是从零绿的」的经典陷阱。
- **缓解**: 本轮已在 `e2e/audit.spec.ts` 里**消除该用例对库状态的隐含依赖**
  （显式 `selectOption('10')`），并补了**负向对照**（撤掉修复 → 干净库上复现失败）。
  但**其他** E2E 用例是否还有同类隐含依赖，**尚未系统排查**。
- **建议修法**:
  1. `scripts/e2e-server.sh` 补齐：跑 `deploy/seed/02-app-role.sql` → 跑
     `modules/sandbox/migrations/` → 跑 `xtask tenant-policies`；
  2. 配套一个「**从零建库 → 跑全量 E2E**」的脚本，作为 CI 的入口（本仓暂无 remote，
     但脚本先备好）；
  3. 排查其余 E2E 用例是否隐含依赖「库里有 N 条以上数据」。

## TD-040: SSO 登录审计只到 **mock sink** 层，真库落表无判据

- **发现于**: REQ-014 修复轮 2（F14；第 2 轮 Dev 报 U-2）
- **证据**: 新增的 `crates/platform-iam/tests/sso_login_audit.rs` 用 **mock sink**
  断言事件码 / `object_type` / `tenant_id` / `ip` / `user_agent`；
  **没有**一条断言事件真的进了 `platform_audit_log`。
  （对照：`platform-iam` 的本地登录用例 `audit_events.rs:229-262` **也是** mock sink
  → 两条路径**同档**，不算新的缺口。）
- **⚠️ 纠正一个错误认知（避免它扩散）**：第 2 轮 Dev 报「加 `platform-audit` dev-dep 会**成环**」
  → **BA 已核实：不成环**。
  - `platform-iam` 的依赖 = `platform-core` + `platform-db`；
  - `platform-audit` 的依赖 = `platform-core` + `platform-db` + `platform-contracts`；
  - `platform-contracts` 的依赖 = `platform-core` + `platform-contracts-macros`。
  → **两条线没有交点** → `platform-iam` 加 `platform-audit` 的 **dev-dependency** 完全可行。
  （Cargo 本身也允许经 dev-dependency 构成的环。）
- **为什么本轮不修**: F14 已经把 SSO 登录审计**提到与本地登录审计同档**（mock sink 的形状断言），
  而「事件真进表」这一层 iam 侧**已由独立复核用 HTTP 黑盒验过**
  （登录 → 真实表里 1 条 `iam.login.succeeded`）。
  剩下的只是**把同一条保证也套到 SSO 分支上** → 收益小、单独一轮不划算。
- **风险**: 若 `IamService` 里 SSO 分支的落库调用被误删/改道，
  现有用例**不会红**（mock sink 拦在 sink 之前）。
- **建议修法**: 在 `platform-iam` 加 `platform-audit` 的 **dev-dependency**，
  把 SSO 那条用例改成「真库 + 查 `platform_audit_log`」——
  与 `crates/platform-server/tests/audit_chain_real_db.rs` 里 org / config 的做法一致
  （把断言放在**同时能看到两个 crate** 的装配层也行）。

## TD-041: OAuth 令牌**不能**访问底座业务 API（默认拒绝，opt-in 机制未实现）

- **发现于**: REQ-022 规格阶段（D209）
- **证据**: `crates/platform-core/src/extension/permission.rs:178` `effective_scope` ——
  RBAC 的数据范围（`ScopeFilter`）是**按用户**算的；
  `crates/platform-core/src/extension/permission.rs:366` `authorize()` ——
  鉴权判定只问「有没有这个权限点」，**不问**「这个身份是谁」。
- **问题**: 若让 OAuth 令牌访问底座业务端点，scope checker 只能回答
  「令牌里有没有 `rbac:role:read`」→ **数据范围无处可依**
  （返回 `ScopeFilter::All` = 持令牌者可读**全租户**数据 → 越权）。
  要做对，必须让 scope checker **继承授权用户的数据范围**，
  再叠加**显式 opt-in**（哪些端点接受 OAuth 令牌）。
- **本轮的决定**: 底座业务端点**一律不接受**第三方 OAuth 令牌（默认拒绝，fail closed）。
  第三方应用验证令牌走两条路：**验签 JWKS**（离线，撤销最长延迟一个 TTL）
  或**调内省端点**（实时）。
- **为什么本轮不修**: 要做对需要三件事 —— ① scope checker ② 数据范围按授权用户继承
  ③ 显式 opt-in 登记机制（且要动契约或加注册表）。
  这三项**互相耦合**，单独做一半 = 越权。属于**新需求**规模，不是本轮能塞进去的补丁。
- **影响面**: 第三方应用**不能**用一枚令牌访问底座业务 API；
  需要「一个令牌走天下」的场景要等后续需求。
- **建议修法**: 另起需求，落三件事：
  1. `platform-core` 加「令牌主体」概念（不只是 `user_id` + `tenant_id`，
     还有「**以谁的身份**」）；
  2. scope checker 的 `effective_scope` **代理到授权用户的 RBAC 判定**；
  3. 显式 opt-in 登记（建议放 `platform-oauth` 的 manifest 或独立注册表，
     **不要**放租户级配置 —— 租户管理员不该能决定「哪些端点接受外部令牌」）。

## TD-042: 出站 HTTP client 有**两份**（`platform-iam` 的 OIDC one + `platform-oauth` 的）

- **发现于**: REQ-022 规格阶段（D220）
- **证据**:
  - `crates/platform-iam/src/sso/oidc.rs:88` `OidcHttpClient` ——
    带 **discovery + JWKS 缓存**，是**入站**专用（拉 IdP 的 metadata / JWKS）；
  - REQ-022 需要的是「**POST 一个 form 到任意 URL**」（back-channel logout），
    两者语义不同。
- **本轮的决定**: `platform-oauth` **内部独立实现**一个最小出站 client
  （`hyper` + `hyper-util` + `hyper-rustls`，**零新增依赖**），
  **不改** `platform-iam` 的 OIDC client —— 不碰 REQ-013 已交付的代码。
- **为什么本轮不修**: 抽公共 client 要改 `platform-iam` 的**已交付**实现
  （REQ-013 回归风险），而收益只是省掉几十行重复代码；
  低配机上「少编译一个 crate」反而更重要。
- **影响面**: TLS 配置、超时策略、连接池参数在两处**各自**演进，
  将来可能出现「入站能连、出站连不上」的不一致。
- **建议修法**: 抽到 `platform-core`（或独立的 `platform-http`），
  统一暴露「`get_json` / `post_form` / 超时 / TLS 根证书」，
  两个调用方共用；改动时**必须**同时跑 REQ-013 与 REQ-022 的回归。

## TD-043: 登出**只**作废 refresh token —— 已签发的登录 access token 仍有效到过期

- **发现于**: REQ-022 规格阶段（QA 挑刺 S-25，**安全缺口**）
- **证据**:
  - `crates/platform-iam/src/store.rs:754` `revoke_family` 与 `:765` `revoke_all_refresh_tokens`
    —— 两者**只** UPDATE `platform_iam_refresh_token`；
  - `crates/platform-iam/src/jwt.rs:143-166` `JwtKeySet::verify` —— 验签 + `exp`，
    **不查库**（access token 是自包含 JWT）。
- **问题**：用户点「退出登录」后，别人拿他的 access token **在 `exp` 之前仍能调所有业务端点**。
  REQ-022 的 D211 原本写「作废**全部会话**」，实现上只作废了 refresh token ——
  **措辞与实现不符**，且**没有任何 AC 覆盖**「登出后旧 access token 立即失效」。
- **本轮的决定**（D229）：**显式写明现状**，不假装实现了「立即失效」。
  - OAuth 侧的撤销是**另一套**：由 `platform_oauth_token` 表的状态决定 → **立即**生效；
  - 登录侧的撤销：只作废 refresh token → **最长延迟一个 `iam.accessTokenTtl` 秒**。
  → 这条差别必须写进 REQ-022 的 AC-S-149（否则两边的「撤销」会被混为一谈）。
- **为什么本轮不修**：让 access token 立即失效 = 在**验签后查一次会话状态**
  （或引入 `sid` 黑名单）→ 给**所有**业务端点的每次请求加一次查库 →
  **性能与架构改动**，属于 REQ-007 的范畴，不是本需求能顺带塞进去的补丁。
- **影响面**: 私有化部署里「退出登录」不是强制的；令牌泄露后的暴露窗口 = access token TTL。
- **建议修法**:
  1. 引入 `sid` 黑名单（登出时把 `sid` 写进一张短 TTL 的表），认证中间件在验签后查一次；
  2. 或把 access token TTL 压到足够短（如 5~10 分钟）+ 依赖 refresh token 续期；
  3. 两者都**要重新评估高频接口的延迟预算**（低配机）。

## TD-044: `cargo xtask capabilities --check` **基线就是红的** —— `catalog.rs` 的文案没跟上 REQ-014

- **发现于**: REQ-022 规格阶段（QA 挑刺 S-13，**实测**）
- **证据**:
  ```
  $ ./target/debug/xtask capabilities --check --root .../code
  能力索引校验失败：错误码 E_CAPABILITIES_OUTDATED
    ✗ 内容不一致：capabilities/index.json
    修复方式：运行 `cargo xtask capabilities` 重新生成
  EXIT=1
  ```
  - 根因：`crates/xtask/src/catalog.rs:147-149` 的 `audit_sink` 条目仍写着
    「审计只到事件声明面，落库与查询**待 REQ-014**」—— **REQ-014 已合并**
    （`develop` @ `310e4d3`）→ `capabilities/index.json` 因此过期。
- **影响面**: 任何把 `cargo xtask capabilities --check` 当判据的 AC **一上来就是红的**
  → 极易被当成「环境问题」放过（QA 明确提醒了这一点）。
  另注：该命令**不做 C-5**（`crates/xtask/src/capabilities.rs` 只比对索引产物与磁盘）
  → **它根本不是 C-5 的判据**。
- **本轮的决定**（D240）: REQ-022 **顺带还**这笔债（改 `catalog.rs` 的文案 + 重新生成索引），
  并把 AC-S-153 的判据**拆成两条**：
  - 1a = `cargo xtask capabilities` **重新生成** + 断言 `capabilities/` 仍 **14 个文件**
    + `permissions.json` 的 `total == 32`；
  - 1b = **真 C-5 命令** `cargo test -p platform-oauth --test contract_c5`。
- **建议修法**: 本轮还完后，把「`capabilities/` 与源码一致」纳入**每次改 manifest 后的必跑项**；
  长期可考虑让 `xtask capabilities --check` 在 CI 里作为**硬闸门**。

---

## TD-045: 「我的授权」两个端点**要权限点**，普通用户也得被发这两个权限点

- **发现于**: REQ-022 实现复核（BA 在棒 1 的 `http.rs` 上做跨栈对账时，D259）
- **证据**:
  - `BA/demands/REQ-022-outbound-idp/design-summary.md` §3.2：
    `GET /api/oauth/grants` 要 `oauth:grant:read`、
    `POST /api/oauth/grants/revoke` 要 `oauth:grant:revoke`；
  - 用例 TC-022-084 ~ TC-022-088 用的却是**只持 `rbac:role:read`** 的
    `u-perm-read@example.com` 与 `u-perm-other@example.com`
    （`BA/demands/REQ-022-outbound-idp/test-cases/consent-grant.md:171-233`）。
- **影响面**: 要么这两个端点对普通用户**全关**（「我的授权」页没法用），
  要么**给所有用户都发** `oauth:grant:read` + `oauth:grant:revoke`
  → 权限点从「能力门禁」退化成「人人都有」，
  **真正起作用的其实是数据范围**（只限当前用户）。
- **本轮的决定**（D259）: 走后者 —— 夹具给普通用户也发这两个权限点，
  并**硬性要求数据范围永远限定在当前用户**
  （即使持有 `oauth:grant:revoke` 也不能撤别人的）。
- **为什么现在不改成「自助端点不要权限点」**:
  那样 `oauth:grant:read` / `oauth:grant:revoke` 就**没有字面量调用点**
  → **C-5 报「已登记但无鉴权调用点」→ AC-S-153 红**；
  要走这条路必须新增「管理他人授权」的端点 → **扩大本需求范围**。
- **建议修法**（后续 REQ）: 引入**管理视角**的授权端点
  （如 `GET /api/oauth/clients/{client_id}/grants`，要 `oauth:grant:read`），
  把 `oauth:grant:read` / `oauth:grant:revoke` 挂到那里；
  「我的授权」两个端点则去掉权限点、只靠数据范围（当前用户）自证。

---

## TD-046: 私钥泄露时，自签 consent ticket 能**写出 grant 行**（但拿不到令牌）

- **发现于**: REQ-022 棒 1 交付复核（报告 §6.2 / **D260**）
- **证据**: `crates/platform-oauth/src/service.rs:1864` ——
  `confirm` **只信 ticket**、**不**校验 scope（D204 字面）→
  自签一个 `scope` 更大的票据可以通过 `confirm`，从而写入一行 `platform_oauth_grant`。
- **影响面**: 威胁模型是「**E2E 私钥泄露**」（私钥为了 AC-S-146 的判断力落盘在 `target/`）。
  泄露后攻击者能：① 写出用户没同意过的 grant 行（用户会在「我的授权」列表里看到）；
  ② **拿不到**越权令牌（令牌端点的三重交集仍拒 → `invalid_scope`）。
  → 是「**可见性污染**」，不是「越权」。
- **为什么本期不修**（D260）: 要修就得让 `confirm` 能判断「票据 scope 比 preview 返回的更大」，
  而 preview 的结果**没有持久化**（D204 定了「零新表」）→ 必须引入新表 / 新列。
  代价是**废掉 AC-S-140 的 4 条 P0 用例**（`invalid_scope` 永不出现）→ 权衡后选了保 AC-S-140。
- **建议修法**（后续 REQ）: preview 时把 `(jti, 交集 scope 摘要, 过期时间)` 持久化
  （可以复用 `platform_oauth_code` 表加一列，或新建轻量表），
  `confirm` 校验「票据 scope == 该 jti 记录的交集 scope」→ 自签票据必被拒（真正的 `OAUTH_TICKET_INVALID`）。
  届时 TD-046 与 D260 的替代判据一并回滚。

---

## TD-047: **每个自带认证中间件的模块都必须自己调 `ensure_fresh`** —— 漏了就「第一次必 403」

- **发现于**: REQ-022 终版 E2E（P3 第 1 轮：41 passed / **2 failed**）
- **证据**（修前）:
  - `crates/platform-oauth/src/http.rs` 的 `authenticate` 中间件**只**注入 `PermissionChecker`，
    **没有**调 `PermissionRefresher::ensure_fresh`；
  - `crates/platform-rbac/src/http.rs:235` 与 `platform-config` 的中间件**都调了**；
  - 后果：`RbacChecker::has` 是**纯缓存查询**（`crates/platform-rbac/src/checker.rs:336-348`，
    缓存里没有该用户 → `fail closed` 直接 `false`）→
    用户**第一次**打 `POST /api/oauth/clients` 必然 **403 `INSUFFICIENT_PERMISSION`**，
    **第二次就好**（缓存被别的请求填上了）。
  - 实测：同一 token 连打 3 次 → `403 / 403 / 403`（服务刚起、缓存全空）；
    一旦缓存被填过 → `201 / 201 / 201`。**「第一次失败、重试就好」是最难查的一类缺陷。**
- **影响面**: 任何**新增的、自带认证中间件的**平台模块都会踩同一个坑。
  `PermissionRefresher` 的 trait 注释（`crates/platform-core/src/extension/permission.rs:203-206`）
  **已经明写了这件事**（「任何自带认证中间件的平台模块都要在 `require_permission` 之前刷新判定缓存」）
  —— 但它是**散文**，没有任何机制强制。
- **本轮已修**: `OAuthState` 加 `refresher` 字段 + `authenticate` 里 `ensure_fresh` +
  装配层传同一个 `RbacChecker`（它同时实现 `PermissionChecker` 与 `PermissionRefresher`）。
- **建议修法**（后续 REQ）: 把这件事从「散文」变成**可判定**：
  ① 在 `platform-core` 提供一个 `authenticate_layer(state)` 的组合中间件
  （验令牌 + 建 ctx + `ensure_fresh` + 注入 checker），新模块**直接用**，不自己写；
  ② 或加一条契约检查：扫 `src/**/*.rs`，凡出现 `insert(checker)` 的文件，
  同一函数体内必须出现 `ensure_fresh`（**带阳性对照**：临时删掉 → 必须报错）。

---

## TD-048: 底座内的**页面不能由后端下发**（`RouteDefinition` 无组件槽 + `App.svelte` 硬编码 10 处 if/else）

- **发现于**: REQ-016 QA 挑刺（S-03）；BA 已逐条核实全部成立
- **证据**（`develop` @ `aa5b305`）:
  - `apps/shell/src/lib/router.ts:41-62` —— `RouteDefinition` 只有
    `path` / `title` / `permission?` / `requiresAuth?` / `load?`，**没有组件字段**；
  - `router.ts:156` —— `createRouter` 执行 `const { routes } = options` → **闭包捕获数组引用**；
  - `router.ts:113` —— `export interface Router` **没有** `addRoute` / `setRoutes`；
  - `apps/shell/src/App.svelte:221-266` —— 页面渲染是**逐路径硬编码**，
    **10 个** `{:else if resolution.path === ...}`，兜底 `{:else}<Home/>`。
- **影响面**: 「应用清单在运行时可扩展（新增应用 = 加一条注册记录 + 一个路由，**不改底座代码**）」
  （ADR-0001 D4）—— **只做到了「菜单级」，没做到「路由级」**。
  第三方应用能出现在导航里并**整页跳出**（本需求 AC-S-163 已交付），
  但**不能**作为「底座内的一个页面」被下发。
- **本轮的处置**（D278）: AC-S-163 **降级且重写**为三条可判的判据
  （纯函数单测 / `external` 整页跳 / `router.ts` 零改动），
  并把「不做」显式写进规格（`AC-S-163.b`）—— **不是漏了，是有意不做**。
- **要做需三件事同时**（后续 REQ）:
  1. `RouteDefinition` 加 `component`（或 `wrapper`）字段；
  2. `App.svelte` 的 10 处硬编码 if/else 改成**按 `route.component` 查表渲染**；
  3. `Router` 加 `addRoute`，或把 `routes` 改成 getter。
  ⚠️ 这三件都要动 **REQ-011 已交付的路由核心** → 必须显式声明（D109）+ 全量回归。

---

## TD-049: `menus[].label` 是**写死的中文文案** → 模块菜单**不随语言切换**

- **发现于**: REQ-016 QA 挑刺（S-09）；BA 核实 `MenuSpec` 只有 4 个字段
- **证据**:
  - `crates/platform-core/src/extension/manifest.rs:120-133` —— `MenuSpec { label / icon? / route / perm? }`
    → **没有 `labelKey`**；
  - 前端既有范式用的是**翻译键**：`apps/shell/src/lib/nav.ts` 的 `NavItem.labelKey`
    （注释明写「`labelKey` 是翻译键而不是文案：语言切换后菜单要跟着变」）。
- **影响面**: 本需求 AC-S-162 的 `source="module"` 导航项只能下发 `menus[].label`（中文文案），
  前端**直接渲染**、不过 `t()` → **中文环境正常，切到英文仍是中文**。
  （`platform-oauth` 的 `menus[]` 也受同样限制，只是它只有一条「授权管理」。）
- **本轮为何不修**: 要给 `MenuSpec` 加 `labelKey` 字段 = **改平台 manifest schema**
  → 跨需求（D109 要显式声明）+ 要改所有既有 manifest + 要改 `capabilities` 生成
  → **超出 REQ-016 范围**。
- **建议修法**: 给 `MenuSpec` 加 `labelKey: Option<String>`（可选，缺省回退到 `label`），
  然后逐步把各 manifest 的 `menus[]` 迁到 `labelKey`。

---

## TD-050: 应用注册**没有数量配额**（一个租户可注册任意多个）

- **发现于**: REQ-016 规格阶段（QA 提的 §4 缺漏之一）
- **现状**: `platform_app` 无「每租户应用数上限」约束；
  `app.workspace.maxApps`（默认 60）只控制**工作台展示**多少，**不限制注册**。
- **影响面**: 恶意/误操作可注册海量应用 → 工作台查询变慢、`platform_app` 表膨胀。
  比 REQ-006 的租户配额（那里有 `platform_db::quota`）弱。
- **建议修法**: 复用 `platform-db` 的配额机制（`crates/platform-db/src/quota.rs`），
  加一个 `app.maxAppsPerTenant` 配置 + 注册时校验。

---

## TD-051: `PATCH /api/apps/{id}` **无并发控制**（两人同时改会丢更新）

- **发现于**: REQ-016 规格阶段（QA 提的 §4 缺漏之一）
- **现状**: `PATCH` 是「读 → 改 → 写」，没有 `If-Match` / `version` 列 → **last-write-wins**。
- **影响面**: 两个管理员同时改同一个应用的 `displayName` / `sortOrder`，后提交的会**静默覆盖**前者。
  与平台其它写端点（`platform-config` 有 epoch、`platform-rbac` 有权限点 epoch）**不一致**。
- **本轮为何不修**: 加 `version` 列要改表结构 + 所有写路径 + 前端要处理 409 →
  超出「应用注册表 CRUD」的最小范围。
- **建议修法**: `platform_app` 加 `version int8 NOT NULL DEFAULT 1`；
  `PATCH` 要求 `If-Match: <version>` → 冲突返回 **409 `APP_VERSION_CONFLICT`**；
  更新时 `SET version = version + 1 ... WHERE version = $expected`。

---

## TD-052: `Home.svelte` 的硬编码占位菜单与工作台的真实导航**并存**

- **发现于**: REQ-016 棒 2 交付（它主动提的第 2 个问题）
- **证据**:
  - `apps/shell/src/lib/nav.ts` 的 `NAV_ITEMS` 是**硬编码占位**（REQ-010 交付，
    它自己的注释写着「本期为占位数据，后续由应用注册表下发」）；
  - 而 REQ-016 的工作台用**真实导航**（`GET /api/apps/navigation`）。
  - → `Home.svelte` 与 `Workbench.svelte` **两套菜单并存**，语义重叠但内容不同。
- **影响面**: 用户看到两个入口区域；`Home` 的占位菜单里有些路径（`/api/iam` 等）
  **不是页面路由**，点进去会落到 `<Home/>` 兜底 —— 看起来像「点了没反应」。
- **本轮为何不统一**: 统一会动 **REQ-010 的 E2E**（`e2e/shell.spec.ts:48`
  「shell 渲染并可过滤菜单」依赖 `NAV_ITEMS`）→ 超出 REQ-016 范围，
  且属「改已交付行为」（D109 要显式声明）。
- **建议修法**: 后续 REQ 里把 `NAV_ITEMS` 彻底换成「导航 API 的下发结果」，
  `Home.svelte` 与 `Workbench.svelte` 合并成一个入口页；同步改 REQ-010 的 E2E。


---

## TD-053: REQ-022 修复轮引入的**守卫误报** —— `no_rust_source_writes_the_audit_table_directly` 在 `develop` 上一直是红的

- **发现于**: REQ-016 终版全量（`cargo test -p platform-audit` 第一个 red）
- **证据**:
  ```
  $ cargo test -p platform-audit --test audit_real_db no_rust_source_writes_the_audit_table_directly
  crates/**/*.rs 里不得出现裸 INSERT：
    [".../crates/platform-server/tests/oauth_real_stack.rs:761"]
  test result: FAILED
  ```
  → 在 `develop` @ `aa5b305`（**已合并 REQ-022**）上跑**同样红** → **不是 REQ-016 引入的**。
- **根因**（两个都要说清）:
  1. **REQ-014 的守卫扫描口径太宽**：`scan_dir(&root.join("crates"), "rs", needle)`
     无条件递归，**把 `tests/` 也算进来**；
  2. **REQ-022 的阳性对照必须直插**：`oauth_real_stack.rs:757-770` 为验 AC-S-154 步骤 4
     （「管理员直连插一条含假 secret 的审计行 → 扫描器必须命中」）
     **必须**有 `INSERT INTO platform_audit_log` —— 那是 dev-guide §9.2 要求的**阳性对照**。
  → **守卫**（生产代码不得绕过 sink）与**阳性对照**（测试必须直插）**直接冲突**。
- **为什么 REQ-014 时没暴露**: 那时**没有任何测试**需要直插审计表 → 冲突不存在。
- **BA 的失误（如实记）**: REQ-022 合并时我只抽验了 3 个 crate
  （`platform-oauth` / `platform-server` / `xtask`），**漏了 `platform-audit`**
  → 这个红灯**带着它合并进了 `develop`**，直到 REQ-016 的全量才发现。
  ⚠️ **教训**：合并后验证**不能只抽验** —— 至少跑一遍受影响 crate 的**直接依赖方**。
- **本轮已修**（D290）: `scan_dir` 加跳过 `tests/` / `benches/` / `examples/` / `target`，
  并在函数文档里写清「为什么必须跳过」。
- ⚠️ **残余风险（独立复核 FINDING-4，BA 接受）**: 修后**不再扫 `tests/`**
  → 若有人把「生产代码」写进 `tests/` 目录（或反过来把测试放在 `src/` 里），守卫**会漏**。
  当前仓库结构下**不会发生**（`tests/` 只放测试），但这是一条**约定**而不是**机制**。
  → 更彻底的修法（后续 REQ）：扫描时**按模块归属**判断（`src/` 一律扫、`tests/` 里只允许
  出现带「阳性对照」注释的直插），或反过来给直插加一个**显式豁免标记**（如
  `// audit-guard:allow positive-control`）—— 后者更明确，但要改所有既有阳性对照。

- **修复的敏感度验证**（BA 亲跑）:
  - 变异：往**生产代码** `crates/platform-audit/src/sink.rs` 塞一个裸 INSERT
    → **仍然红**（报 `sink.rs:154`）→ 守卫**没有被削弱**；
  - 还原：`sha256sum -c` 成功 / `grep -c MUTATION` = 0。

---

## TD-054: 五类通知通道**只做了 3 类** —— 短信与 IM（钉钉 / 企微 / 飞书）未实现

- **发现于**: REQ-015 规格阶段（`develop` @ `a2b7e52`）
- **现状**: `demand.md` 的完成定义写「五类通道可投递」，REQ-015 交付 **3/5**：
  - ✅ 站内信（落库，无外部依赖）
  - ✅ 邮件（**自己写的 SMTP 客户端**，D305 —— 零新增依赖）
  - ✅ Webhook（`hyper` + `hyper-rustls`，与 `platform-oauth` 的 `OutboundClient` 同款栈）
  - ❌ **短信**
  - ❌ **IM**：钉钉 / 企微 / 飞书
- **为什么不做**（**不是「做不出来」，是「做了也验不了」**）:
  | 通道 | 技术难度 | 真正的拦路条件 |
  |---|---|---|
  | 短信 | 低（HTTPS POST） | **无厂商凭据** + 短信模板要**实名报备**才能发 |
  | 钉钉 | 低 | 要 `timestamp + HMAC-SHA256(secret)` 签名，**无真实 secret 无法验签** |
  | 企微 | 低 | 要先用 `corp_id + corp_secret` 换 `access_token` |
  | 飞书 | 低 | 要先用 `app_id + app_secret` 换 `tenant_access_token` |
  → 没有凭据时写出的实现**只能断言「我发出了一个请求」**，
  这与 REQ-022 修复轮抓到的「判据恒真」是同一类问题（D292）。
- **当前行为（**不是静默成功**）**: 4 个通道在 `ChannelRegistry` 里注册，
  但 `send()` 一律返回 `Err(NOTIFY_CHANNEL_NOT_CONFIGURED)`，
  入队投递则**立即进死信**并标该码 —— **不会**假绿（AC-S-166 步骤 3/4 判据）。
- **要做需要什么**:
  1. **真实凭据**（每类至少一个可用的测试账号）；
  2. **能回放的录制夹具**（把一次真实请求/响应录下来，测试里重放并断言签名）——
     这样即使凭据过期，测试判据仍然有区分力；
  3. 各平台的**错误码映射表**（把 `40058` 这类平台码翻译成 `NOTIFY_*`）。
- **优先级**: P2（当前 3 类已覆盖「用户能收到通知」的主路径）

---

## TD-055: SMTP 只做 **STARTTLS（587）**，未做隐式 TLS（465）

- **发现于**: REQ-015 规格阶段（D305 的范围裁剪）
- **现状**: `channels/smtp.rs` 只实现 STARTTLS 流程
  （`TCP → EHLO → STARTTLS → TLS 握手 → EHLO → AUTH LOGIN → …`）。
  **端口 465（SMTPS，连上即 TLS 握手，无 EHLO 前置）不支持**。
- **为什么裁剪**: 465 与 587 的**协议顺序不同**（465 没有明文阶段），
  要单独一条代码路径；而**没有真实 465 服务器**时这条路径无法验证
  —— 写出来会变成「看起来支持」。
- **影响**: 只支持 465 的邮件服务商无法使用（多数现代 ESP 同时支持 587，
  但部分老企业内部 SMTP 只开 465）。
- **要做需要什么**: 一个真实/可复现的 465 端点 + 一条对应用例
  （假服务器需在 `accept` 后**立即**握手，不回 `220`）。
- **优先级**: P3

---

## TD-056: REQ-015 的**前端两个页面未做**（模板管理 / 投递记录）

- **发现于**: REQ-015 终版（前端封装已完成，页面未做）
- **现状**: `notify-api.ts`（含 27 条单测）已就绪，10 个端点都有封装；
  但 **`NotifyTemplates.svelte` 与 `NotifyDeliveries.svelte` 两个页面没做**。
- **为什么没做**（**不是时间不够，是壳层的结构限制**）：
  - `RouteDefinition` **无 component 字段**（`apps/shell/src/lib/router.ts:41-62`）；
  - `createRouter` **捕获数组引用**（`router.ts:156` `const { routes } = options`）；
  - `Router` **无 `addRoute` / `setRoutes`**（`router.ts:113`）；
  - 页面渲染是**逐路径硬编码**（`App.svelte:221-266`，**10 个** `{:else if}`）。
  → 加两个页面必须改壳层（新路由 + 新分支），这是**跨需求改动**（D109）。
- **影响**（明码）:
  - AC-S-167~173 的**判据**全部是 API 层，已在 19 条真库集成测试里验过 → **判据达成**；
  - 但「管理员能在界面上维护模板 / 重投死信」这个**用户可用性**目标**未达成**。
- **要做需要什么**: 与 **TD-048** 一并解决 —— 即 `RouteDefinition` 加 `component`、
  `App.svelte` 改按 `route.component` 查表渲染。改完之后两个页面只是两个 `.svelte` 文件。
- **优先级**: P1（功能可达但不可用；比 TD-048 更具体，可作为 TD-048 的第一个落地案例）

---

## TD-057: 租户级 **locale 偏好没有存储位**

- **发现于**: REQ-015 实现（`PlatformNotifyConfigSource::tenant_default_locale`）
- **现状**: `notification` 的 locale 三级回退是「用户 locale → **租户默认** → 失败」（D294），
  但「租户默认 locale」**没有存储位置** ——
  `platform-config` 的 `FALLBACK_LOCALES`（`crates/platform-config/src/registry.rs:44`）
  只是**常量** `&["zh-CN"]`，没有租户级覆盖。
- **当前实现**: `platform-server/src/main.rs` 的 `PlatformNotifyConfigSource::tenant_default_locale`
  **固定返回 `zh-CN`**（已在该函数上标 `TODO(TD-057)`）。
- **影响**: 非中文租户的通知**一律回退到中文** ——
  除非他们为 `zh-CN` 也维护一份模板（那就等于「模板里写两遍」）。
- **要做需要什么**:
  1. 一个租户级配置键（如 `platform.defaultLocale`，`scope: tenant`）；
  2. `platform-config` 支持读**租户级**条目（现在 `read_platform_entries` 只读平台级）；
  3. 通知的 `tenant_default_locale` 改读它。
- **优先级**: P2（中文租户不受影响；多语言租户的主路径会退化）

---

## TD-058: 子 Agent **读一堆文件后不落盘**（本轮出现 4 次，每次烧掉 20~30 分钟）

- **发现于**: REQ-015 全程（2026-11-21）
- **现象**（4 次的共同模式）:
  | # | 角色 | 任务 | 结果 |
  |---|---|---|---|
  | 1 | `worker`（QA 挑刺+用例集） | 一次读 5 份需求文档 + 93 条用例 | 30+ 分钟零输出 |
  | 2 | `worker`（QA 挑刺，缩小范围后） | 只读 3 份 + 只写 1 份报告 | 仍是 5 行骨架 |
  | 3 | `worker`（Dev 后端全量） | 1 个 crate 17 个文件 | 20+ 分钟零文件 |
  | 4 | `verifier`（独立复核） | 核 72 处 `file:line` | 20+ 分钟零报告 |
- **有效的做法**（第 5、6 次派单成功）:
  - 把范围缩到 **1~3 个文件**；
  - 派单里**明确写**「第一步就用 `write` 建骨架，之后每 3~5 条 `edit` 一次」；
  - 把该读的内容**直接摘录进派单**（不让它自己去翻文档）；
  - 要求它**如实报告**「我的文件还没被 `mod.rs` 声明，那 87 条测试里没有我的一条」——
    这种自我披露很有价值，说明它真的在核，不是在编。
- **推测的根因**（未验证）:
  - 单次读太多 → 上下文被大文件挤占 → 到该写的时候已经没空间；
  - 或：模型倾向于「先把所有材料读完再动手」，而材料是无界的。
- **建议的机制化改法**（未实施，需要平台侧支持）:
  1. 派单模板里强制要求「先落骨架」这一步（已在后续派单里手工做到，但没有机制保证）；
  2. 给子 Agent 一个**硬预算**（如「最多读 5 个文件」），超了就报错而不是静默继续；
  3. 或在子 Agent 启动时注入「你的第一个工具调用必须是 `write`」。
- **本轮的实际代价**: 4 次空转 ≈ 1.5~2 小时。BA 最终改为**自己实现**（偏离了
  「BA 不自己实现代码」的纪律），并在报告里如实标注独立性不足。
- **优先级**: P1（影响所有后续需求）

---

## TD-059: S3 存储**只做了协议，没真连过**

- **发现于**: REQ-018 规格阶段（用户 2026-11-22 裁定）
- **现状**: 父 AC-025 要求「对象存储为 S3 兼容」。本轮实现：
  - **SigV4 预签名**（用树里已有的 `hmac` + `sha2`，**不新增依赖**）
  - **分片上传 REST 协议**（`CreateMultipartUpload` / `UploadPart` / `Complete` / `Abort`）
  - **但完全没有真实 S3 端点可测**：`podman ps` 里只有 `base-postgres` + `base-redis`
- **能证明什么 / 不能证明什么**（**这条必须说清**）:
  | 能证明 | 不能证明 |
  |---|---|
  | SigV4 的规范化请求 + HMAC 链按 **AWS 官方向量**正确 | 我们的 REST 调用格式被 S3 接受 |
  | 预签名 URL 的**结构**与签名可在本地验证 | 分片合并的 ETag 处理正确 |
  | 错误路径（签名不符 / 过期）能正确拒 | 真实网络下的行为（重试 / 超时） |
- **当前行为**: `LocalFsStore` 是**真实现**且真跑过（分片 / 断点续传 / 路径穿越防护）；
  `S3Store` 的代码路径**未被任何真实端点验证过**。
- **要做需要什么**: 一个 S3 兼容端点（MinIO 容器即可，约 200 MB 内存）+
  一组「上传 → 分片 → 合并 → 预签名下载 → 过期被拒」的集成测试。
- **优先级**: P1（生产若用 S3，这是**未验证**代码）

---

## TD-060: **Excel（.xlsx / .xls）完全不做** —— 上传返回明确错误码

- **发现于**: REQ-018 规格阶段（用户 2026-11-22 裁定）
- **现状**: `demand.md` 要求「Excel / CSV 模板下载」。本轮**只做 CSV**：
  - `.xlsx` / `.xls` 上传 → **415 `STORAGE_UNSUPPORTED_FORMAT`**（不是静默 500）；
  - 模板下载也只返回 CSV。
- **为什么不做**: `.xlsx` 的本质是 **ZIP 容器 + 若干 XML 部件**；
  实测 `zip` / `flate2` / `miniz_oxide` / `quick-xml` / `xml-rs` **全部 0 个**在 `Cargo.lock`。
  → 零依赖下做不出真 xlsx。
- **为什么不做「假 xlsx」**: 把 CSV 改名成 `.xlsx` 是**最坏的假绿**——
  Excel 打不开、用户以为文件坏了，而我们所有测试都会绿（只测字节）。
- **当前行为**: 明确拒绝，`detail` 里写 `only csv is supported in this release`。
- **要做需要什么**: 加 `zip`（或 `flate2`）+ `quick-xml` 两个依赖（约 6~8 个传递包），
  然后实现 SpreadsheetML 的最小读写（`[Content_Types].xml` / `workbook.xml` /
  `sharedStrings.xml` / `sheet1.xml`）。
- **优先级**: P2（CSV 已覆盖「大批量导入导出」的真实场景；Excel 主要是给非技术用户用）

---

## TD-061: **图片处理（缩略图 / 裁剪）完全不做**

- **发现于**: REQ-018 规格阶段（`demand.md` 列了「图片处理」）
- **为什么不做**:
  1. 图像解码需要 `image` crate（或 `libvips` 绑定）—— **不在树里**；
  2. **手写 JPEG/PNG 解码器是数万行**，且是**安全高风险面**
     （历史上 libjpeg / libpng 的 CVE 数量在所有库里排前列）；
  3. 低配机上给图片解码分配内存 / CPU 会挤占主服务。
- **影响**: 上传的图片**原样存储**，不生成缩略图。前端要缩略图只能自己缩放
  （或后续接 CDN 的图片处理）。
- **要做需要什么**: `image` crate（约 15 个传递包）+ 一个异步的缩略图 worker（复用 D324 的形态）。
- **优先级**: P3（核心的文件存取不受影响）

---

## TD-062: 预签名的**时序侧信道**无法用功能测试覆盖

- **发现于**: REQ-018 规格阶段（AC-S-176 的判据推到第 ③ 条时发现）
- **现状**: 预签名校验用 `subtle::ConstantTimeEq` 做常数时间比较（D322），
  但**「用了常数时间比较」这件事无法用功能测试断言**：
  - 用 `==` 比签名字符串，功能上**完全等价**（结果一样）；
  - 差别只在**耗时**，而耗时测量在测试环境里被编译优化 / CPU 频率 / 调度噪声淹没。
- **影响**: AC-S-176 的步骤 3（改签名一个字符）**能验证「篡改被拒」**，
  但**不能验证「拒的方式是常数时间的」**。
- **怎么缓解**（已做）:
  1. 依赖 `subtle`（一个专门的常数时间比较库）而不是自己写；
  2. 在代码注释里写明「此处必须是常数时间」，并链接本 TD；
  3. **靠代码评审**：谁改这段代码，必须看注释。
- **要做需要什么**: 一个**统计意义上的**时序测试（数千次采样 + t 检验）——
  在 CI 里不稳定（假阳性高），不推荐。
- **优先级**: P3（这条债的性质是「知道它测不了」，不是「知道它有问题」）

---

## TD-063: REQ-018 的**前端页面未做**（与 TD-056 同源）

- **发现于**: REQ-018 规格阶段
- **现状**: 只做 `storage-api.ts` 封装 + 单测；**页面不做**。
- **原因**: 与 **TD-056**（REQ-015 的通知页面）**完全同源** ——
  **TD-048** 的壳层限制：`RouteDefinition` 无 `component` 槽 +
  `App.svelte` 硬编码 10 处 `if/else` → 加页面必须改壳层（跨需求改动）。
- **影响**: AC-S-175~183 的**判据**全是 API 层（集成测试覆盖），
  但「管理员能在界面上传文件 / 看任务进度 / 下载错误回执」**未达成**。
- **要做需要什么**: 与 TD-048 / TD-056 **一并解决**。
  → 这三条债其实是一件事：**壳层缺组件槽**。
  → 建议下一个需求就做它（一次修好，三处受益）。
- **优先级**: **P1**（TD-048 / TD-056 / TD-063 三条债合并看，是当前最大的可用性缺口）

---

## TD-064: REQ-018 的导入只支持**列固定的探针实体** `probe_row`

- **发现于**: REQ-018 实现复核（F-01 / F-02）后的修复（D333）
- **现状**: 导入/导出是**通道能力**（CSV → 解析 → 校验 → 落库），
  但本轮只提供**一个列固定**的目标实体 `platform_storage_probe_row`
  （`email` / `amount` / `note`）。**不支持导到任意业务表**。
- **原因**:
  1. 「任意业务表」需要一张**实体注册表**（entity → 表名 + 列映射 + 类型转换 + 权限），
     这是一个独立的子系统，规模超过本需求本身；
  2. 动态选表意味着**动态拼 SQL** → 注入面从「CSV 内容」扩大到「entity 参数」，
     必须配白名单 + 列级校验，草率做比不做更危险；
  3. 规格里的两条 AC（180 / 181）只需要**一个可断言的落点**，探针表已够。
- **影响**: 真实业务（如「批量导入用户」「批量导入组织节点」）**用不了**这个通道，
  得等实体注册表做完。AC-S-180 / AC-S-181 的判据**不受影响**（它们只要求一个目标表）。
- **要做需要什么**:
  1. `platform_storage_entity` 注册表（entity → 表名 + 列定义 + 必填 + 类型）；
  2. `entity` 参数**必须**白名单校验（不在注册表里 → 400，不是 500）；
  3. 动态 INSERT 必须用**参数化 + 列名白名单**（列名不能来自 CSV 表头，
     只能来自注册表 —— 否则 CSV 表头就是注入点）；
  4. 每张目标表都要进 `tenantScoped.tables`（否则导入的数据无租户隔离）。
- **优先级**: **P2**（本需求的通道能力已可用；等有第二个真实导入需求时再做，
  避免提前抽象）

---

## TD-065: REQ-018 的 `size` 字段与下载 `Content-Type` 未落实（复核 F-06 / F-07）

- **发现于**: REQ-018 实现复核
- **现状**: `JobView` **没有 `size` 字段**（AC-S-182 步骤 3 要求 `downloadUrl` + `size`）；
  预签名下载端点的 `Content-Type` **硬编码** `application/octet-stream`，
  不读对象的 `content_type`（AC-S-182 步骤 4 要求 `text/csv`）。
- **原因**: 未做，不是不能做 —— 属于实现遗漏，未在本轮修复窗口内处理。
- **影响**: 导出下载时前端拿不到文件大小（无法显示进度 / 判断是否完整）；
  浏览器不会把导出结果当 CSV 处理（`text/csv` 才会触发下载预览或表格打开）。
- **要做需要什么**: `JobView` 加 `size: Option<i64>`（导出完成时从对象元数据取）；
  下载端点先查 `platform_storage_object.content_type`，取不到才回落
  `application/octet-stream`。两处都约 10 行。
- **优先级**: **P2**（功能可用，但体验与规格不符）

---

## TD-066: REQ-018 的**流式上传未做**（`to_bytes` 全量读入内存）

- **发现于**: REQ-018 实现复核（F-05）
- **现状**: 上传与 multipart 路径都用
  `axum::body::to_bytes(request.into_body(), 256MiB)` 把**整个 body 读进内存**，
  然后才交给 `put_object` 做大小校验。
- **原因**: 复用现成的 `to_bytes` 最省事；真正流式要手写
  `Body::into_data_stream()` + 分块写盘 + **边读边判超限**，
  且和 `put_object` 的「先校验再写盘」顺序要重新设计。
- **影响**: 内存上限是 `256 MiB × 并发数`。默认 `maxUploadBytes = 100 MiB` 时，
  10 个并发上传就能吃掉 1 GiB —— **本机 7.7 G 内存会紧张**。
  AC-S-178 已经**主动删掉了「内存峰值」这条判据**（当初理由：测不准），
  所以功能测试**不会变红** —— 这是明码记账的代价。
- **要做需要什么**: `put_object_streaming(key, stream, content_type)`：
  边读边累加 `actual`，**一旦超过上限立即中断并删除已写的 tmp**；
  落盘走 tmp+rename（已有 `write_atomically` 可改造成流式版）。
- **优先级**: **P1**（低配机上是真实的内存风险，不是洁癖）

---

## TD-067: REQ-018 的**分片上传绕过病毒扫描**（复核 F-17，安全）

- **发现于**: REQ-018 实现复核
- **现状**: `complete_upload` 直接 `upsert_object`，`scan_verdict` **硬写 `Skipped`**；
  `scanner.scan()` 只在直传（`put_object`）与导入（`run_import`）里调用。
  分片是**大文件的唯一通道** → 配了真扫描器也能把恶意文件经 `/uploads/*` 落库。
- **原因**: 分片边传边落盘，`complete` 时文件已在磁盘上；当初的取舍是
  「不为了扫描把整个文件再读回内存」（那与 TD-066 的内存问题冲突）。
- **影响**: **AC-S-178 步骤 4 的安全意图被绕开**。当前默认扫描器是 `NoopScanner`
  （一律 `Skipped`），所以**现在没有实际暴露面**；但一旦接上真扫描器，
  这个绕过就是真漏洞。
- **要做需要什么**: `complete_upload` 里对合并后的文件做**流式扫描**
  （读缓冲区扫，不整个读进内存）；感染则删除对象 + `STORAGE_FILE_INFECTED`。
  依赖 TD-066 的流式基础。
- **优先级**: **P1**（现在无暴露面，但接真扫描器前**必须先修** ——
  否则安全配置是假的）

---

## ~~TD-068~~: REQ-018 的 AC-S-179 步骤 4/5 与「异步解析」的取舍 —— **已关闭**（D336）

- **发现于**: REQ-018 的二轮复核后重判（AC 判定 rev 4）
- **当时的现状**: AC-S-179 步骤 4/5 要求「引号未闭合 / 单行超限 → **400** + 行号」，
  实现里这两类错误在**异步 worker** 的 `csv::parse` 里产生 → HTTP 层只有 202，
  用户拿到的是「任务 failed + error 含行号」→ 判据与规格字面不符。
- **关闭方式**: 用户 2026-11-22 裁定**改规格**（**D336**）——
  AC-S-179 步骤 4/5 改为「202 → 任务 `failed` + `error` 含行号」。
  理由：同步 400 必须全量解析（请求超时 + 与 D325 的流式要求互斥），
  而只预检头部抓不到「引号未闭合」（恰恰常在文件末尾）→ 会给出假安全。
- **为什么不降低信息量**: 用户能拿到的**可定位信息完全相同**（哪一行、为什么），
  只是**暴露时机**从同步变异步；判据仍是「`status == "failed"` **且** `error` 含 `line N`」，
  可断言、不放水。
- **实测证据**（`develop` @ `0b9697b`）：
  引号未闭合 → `error = "…a quoted field starting at line 3 is never closed"`；
  单行超限 → `error = "…the line 3 exceeds the maximum row size of 40 bytes"`。
- **状态**: ✅ 关闭（裁决 + 规格 + 决策记录已落盘；无需改代码）

---

## TD-069: REQ-019 的 OTel **真导出无法端到端验证**

- **发现于**: REQ-019 规格阶段
- **现状**: 接入了 OTLP（HTTP/protobuf）导出，但本机**没有 OTel collector**
  （`podman ps` 只有 postgres / redis）→ 只能验：
  ① 配置生效；② span 结构（`trace_id` / `span_id` / 父子层级）；
  ③ **导出失败不影响业务**；④ `OTEL_SDK_DISABLED=true` 时不初始化。
  **无法验**「span 真的到了 Jaeger/Tempo 并且能串成完整链路」。
- **原因**: 无 collector 环境。起一个 collector 需要额外镜像 + 配置文件，
  且即便起来了，也只是验「本地 collector 收到了」，
  与真生产的采样率 / 批量 / 重试行为仍有差距。
- **影响**: AC-S-037 的四条判据都是「可验证的部分」，**不声称**验过真导出。
  若线上 collector 配置错误（endpoint 写错、协议不匹配），**测试不会红**。
- **要做需要什么**: 在 CI 或部署环境起一个 `otel/opentelemetry-collector` 容器，
  配 `debug` exporter，断言它收到了指定 `trace_id` 的 span。
- **优先级**: **P2**（当前的判据已覆盖「不影响业务」这个最大风险；
  真导出属于部署验证）

---

## TD-070: REQ-019 的分页契约只做**样板**，其余 crate 未迁移

- **发现于**: REQ-019 规格阶段（D341）
- **现状**: 统一契约（`items` / `nextCursor` / `total` + `limit` / `cursor`）只在
  `platform-audit` 对齐 + 新增端点使用。其余 crate 的列表端点仍是各自的老形态：
  `platform-app`（12 处）、`platform-iam`（23 处）、`platform-notify`（6 处）、
  `platform-storage`（4 处）；**`platform-rbac` 与 `platform-org` 完全没有分页**。
- **原因**: 改所有列表端点属**跨需求改动**（D109）—— 会影响已验收的 19 个子需求的
  响应契约与前端代码。一次改完风险太大，且与本需求「API 治理」的目标不成比例。
- **影响**: 前端要按端点分别处理分页；`rbac` / `org` 的列表在数据量大时会**全量返回**
  （性能问题，不是正确性问题）。
- **要做需要什么**: 逐 crate 迁移，每个 crate 一批（改响应结构 + 改前端 + 改测试）。
  建议按实际数据量排优先级（`iam` 的用户列表最可能先撑不住）。
- **优先级**: **P1**（`rbac` / `org` 的「完全没有分页」在大数据量下会直接出问题）

---

## TD-071: 审计表**不带 trace_id**（加它要断哈希链）

- **发现于**: REQ-019 规格阶段的独立挑刺（B2）
- **现状**: `platform_audit_log` 是**契约 C12 的 13 个冻结字段**
  （`docs/contracts/api-conventions/v1.md:165-171`，迁移
  `migrations/20261120000200_audit.up.sql:45-59`），**没有 `trace_id` 列**；
  `AuditEvent`（`platform-core/src/extension/audit.rs:101-120`）也没有 trace 字段，
  全仓 **17 处**构造点。
- **为什么本轮不做**:
  1. 加列要同时改 **在线表 + 归档表 + `platform_audit_append` 的 9 参数签名 + 哈希公式**；
  2. **改哈希公式 → 历史行的链校验必然断裂** → 直接破坏 **REQ-014 已验收的链校验 AC**；
  3. 父需求 **AC-028** 的原文是「日志、链路、**异步消息**携带同一 trace_id」——
     **不含审计表**。把审计表写进判据是 BA 的过度设计。
- **影响**: 线上排障时**不能**用 trace_id 直接从审计表反查「这次请求改了哪些数据」。
  替代办法：审计行有 `created_at` + `tenant_id` + `actor_id`，可缩小范围但仍需人工对时。
- **要做需要什么**: 一次**契约升版**——
  ① C12 升到 14 字段（加 `trace_id`）；
  ② 哈希公式**版本化**（`hash_v2 = sha256(prev_hash + 规范化事件体含trace_id)`），
     旧行保留 v1 公式并有版本列；
  ③ `ChainVerifier` 按版本分流；
  ④ 归档表同步。
- **优先级**: **P2**（现在有替代排障路径；改它风险不小，值得单独立项）

---

## TD-072: **未认证请求**（登录/注册）不受 REQ-019 的三级限流保护

- **发现于**: REQ-019 规格阶段的独立挑刺（M7）
- **现状**: `platform_rate_limit` 是 **RLS 租户表**，而 `TenantDb::begin` 在**无租户**时
  直接 `DbError::MissingTenant`（`crates/platform-db/src/tenant.rs:180`）。
  → 登录 / 注册 / `/health` 等**认证之前**的请求拿不到租户上下文 → **写不进限流表**。
- **为什么本轮不做**:
  1. 要做对，需要「**按 IP 的独立维度**」（新表，IP 不是租户数据）或
     「**sentinel 租户 + 非 RLS 路径**」（在 RLS 上开后门，风险大）；
  2. 这是**独立设计**，规模不小于本需求的一个维度；
  3. **当前不是裸露的**：IAM 登录已有自己的 IP 限流
     （`platform-iam/src/service.rs:837` `check_ip_rate_limit`，
     表 `platform_iam_login_rate`）。
- **影响**: 登录暴力破解**恰好落在通用限流覆盖不到的地方** ——
  目前靠 IAM 的专用 IP 限流兜底，但那是**单点**、**无租户维度**、
  且不覆盖注册/找回密码等其它未认证端点。
- **要做需要什么**:
  ① 新表 `platform_rate_limit_ip`（**非 RLS**，因为 IP 不属于租户）；
  ② 中间件在「无租户上下文」时走这个桶；
  ③ 需要想清「共享 IP（NAT/代理）会不会误伤」——
    `X-Forwarded-For` 的可信边界是另一个问题（不能盲信）。
- **优先级**: **P1**（未认证端点是攻击面最集中的地方；
  虽然 IAM 有兜底，但覆盖不全）

---

## TD-073: REQ-019 的 OpenAPI **只覆盖部分端点**

- **发现于**: REQ-019 实现阶段
- **现状**: `cargo xtask openapi` 能生成合法的 OpenAPI 3.1.0 产物，
  `--check` 逐字节比对（阳性对照已验证：篡改产物 → 退出码 1）。
  **但产物里只有 1 个端点**：`/api/observability/rate-limit/status`。
- **原因**: `#[utoipa::path]` 必须挂在**被标注的 handler 所在 crate** 上，
  且该 crate 要能被 `xtask` 依赖。当前架构约束（D103「平台 crate 不依赖业务模块」）
  让 `xtask` **不能**依赖 `platform-server`（那会间接依赖 `sandbox`）。
  → 只有放在 `platform-observability` 里的端点能被标注。
- **影响**: 产物是**部分**文档，不覆盖 `platform-app` / `rbac` / `audit` / `iam` 等
  100+ 个端点。
  ⚠️ **rev 3 修正（D345）**：AC-S-035 已改为「机制成立 + 覆盖范围明确」，
  **不再**要求覆盖具体端点 → 该条 AC 现在**达成**。
  覆盖面仍是**真实的缺口**（运维拿到的文档不全），只是不再当作 AC 判据。
- **要做需要什么**（两条路）:
  1. **给各 crate 加 `utoipa` 依赖 + 标注**，然后让 xtask 通过
     「各 crate 导出的 `openapi_fragment()`」汇总 —— 不引入 server 依赖，
     但要在 8+ 个 crate 里逐个标注（工作量大，且改已验收 crate 源码）；
  2. **把 OpenAPI 生成移到一个独立的 bin**（`platform-server/src/bin/openapi.rs`），
     xtask 用 `cargo run --bin` 调它 —— 绕开依赖约束，但引入进程间调用。
- **优先级**: **P2**（机制已成立且可回归；覆盖面是渐进的事）

---

## ~~TD-074~~: REQ-019 的 OTel span layer —— **已解决**（2026-11-22）

- **发现于**: REQ-019 实现阶段
- **现状**: `init_otel` 能初始化 `SdkTracerProvider`（`is_enabled()` 可测、
  `OTEL_SDK_DISABLED=true` 时正确关闭）；`remote_span_context` 能把入站
  `X-Request-Id` 转成 OTel 的远端父上下文（含两个实测踩坑的修正）。
  **但 `tracing_opentelemetry` 的 layer 没有挂进 subscriber** ——
  因为 `platform-core::log` **不能依赖 `opentelemetry`**
  （否则每个 crate 都编译 OTel，D343 明确避免）。
- **尝试过什么**: 加过 `build_subscriber_with_layer<W, L>(level, writer, Option<L>)`，
  但泛型约束无法同时满足「`Option<L>` 两个分支返回同一类型」与
  「`L: Layer<Layered<FmtLayer<W>, Registry>>`」→ 编译不过，已回退。
- **影响**: **AC-S-037 步骤 1（span 的 trace_id 等于响应头 X-Request-Id）未达成**
  —— 没有 layer 就不会有 span 被导出。步骤 2/3（失败不崩、可关闭）已达成。
- **要做需要什么**: 在 `platform-server` 里直接构造 subscriber
  （它同时依赖 core 与 observability），
  用 `tracing_subscriber::registry().with(fmt_layer).with(otel_layer).with(env_filter)`，
  并把 trace 中间件里的 `tracing::info_span!` 换成
  `tracer.build_with_context(..., &remote_parent)` 的形式。
- **✅ 已解决方式**: 在 `platform-server::init_tracing` 里用**两个分支各自 `.init()`**
  （`.init()` 消费 self 返回 `()`，所以两条路径不必类型相同 —— 这正是泛型方案的死结）。
  实测验证：`otel_in_memory::the_exported_span_carries_the_incoming_trace_id` 通过；
  **变异**：去掉 `set_parent` → 2 条红。
  另加了 `set_parent(remote_span_context)` 到 trace 中间件（没有它 OTel 会自己生成 trace_id）。
- **优先级**: ~~P1~~ → **已关闭**


---

## ~~TD-075~~: REQ-019 的中间件装配 —— **已解决**（2026-11-23）

- **发现于**: REQ-019 实现阶段（挑刺 M5 的遗留）
- **现状**: 9 条 AC 的判据里，HTTP 级的那些跑的是
  `platform_observability` **自己的** router（`trace_logging` / `otel_in_memory` /
  `http_contract`），**不是** `main.rs` 的完整装配。
- **原因**: `main.rs` 的 router 装配是**「边构造 state 边挂路由」的 await 链**
  （约 500 行、9+ 个 state、含 `build_config_service` / `build_storage_state` 等异步构造）。
  完整抽取成 `build_router(deps)` 需要重排整个启动流程 —— 风险与本需求价值不成比例。
- **影响**: **「中间件在 main 里挂对了」这件事没有判据**。
  具体风险：若 `main.rs` 里 `apply_observability` 的位置放错（比如放在 `client_info` 之前），
  或某个 `nest` 漏了 —— 9 条 AC 的用例**全部仍然绿**。
  这是**真正的假绿风险**，不是洁癖。
- **已经缓解的部分**: 中间件装配逻辑**只有一份**
  （`platform_observability::apply_observability`），`main` 与测试都调它 ——
  所以「中间件内部顺序」是验过的；**没验的是「main 调它的位置与传参」**。
- **要做需要什么**:
  1. 把 `main.rs` 的 state 构造与 router 装配拆开（state 构造留在 main）；
  2. 抽 `pub fn build_router(deps: AppDeps) -> Router`，`AppDeps` 装 9 个 state；
  3. `main` 与所有 HTTP 级测试都调它；
  4. 补一条「装配契约」测试：断言 router 里含预期数量的路由
     （`router.into_iter()` 数 route id），防漏挂。
- **✅ 已解决方式**（比原计划更精准 —— 只抽中间件段，不抽整个 router）:
  1. `apply_all_middlewares_with(router, rate_limit, client_info)` 抽到
     `platform-observability::http`，`client_info` 用**函数注入**
     （避免 `server → observability` 成环）；
  2. `main.rs` 的 245 行内联中间件装配 → 10 行调用；
  3. 新增 `tests/middleware_assembly.rs`（5 条判据）：
     trace 挂上了 / limit 挂上了 / 弃用头挂上了 / **client_info 在最外层**。

  **⚠️ 这个测试立刻抓到 2 个真实的顺序 bug**（比 TD 描述的「漏挂」更实在）：
  - `apply_observability` 里先挂 trace 后挂 limit → axum 的 layer 是「后挂在外层」
    → **limit 先执行** → 读不到租户 → 按 TD-072 放行 → **限流永久不生效**；
  - `apply_all_middlewares_with` 里先挂 client_info → 它变内层
    → trace_id 先跑，读不到它填的租户/IP。

  → 两个都是「代码看起来对、功能静默失效」。变异验证：把 client_info 挂回内层 → 2 条红。

  **⚠️ 仍未做**：完整 `build_router` 抽取（把 **router 组装**也抽出来，
  让测试能断言「所有 20+ 个 nest 都挂上了」）。中间件层已有判据，
  **路由挂载**那一层仍然只靠人工核对 → 见 **TD-076**。
- **优先级**: ~~P1~~ → **中间件部分已关闭**；路由层遗留 → TD-076


---

## TD-076: REQ-019 / 全部需求的 **router 组装**仍无判据（`build_router` 未抽）

- **发现于**: REQ-019 实现阶段（TD-075 拆分出的遗留）
- **现状**: 中间件装配已有判据（`middleware_assembly` 5 条），
  但 `main.rs` 里 **20+ 个 `.nest()` / `.merge()` 调用**（`/api/iam`、`/api/org`、
  `/api/rbac`、`/api/audit`、`/api/schema`、`/api/config`、`/api/apps`、
  `/api/notify`、`/api/storage`、`/api/oauth`、`/api/i18n`、`/api/observability` …）
  仍然**内联**在 506 行的装配段里，测试够不着。
- **影响**: 若某个 `nest` 被误删或路径拼错（比如 `/api/notfy`），
  该组路由**整体 404**，而**没有任何测试会红** —— 各 crate 自己的路由测试
  测的是 `router()` 函数，不是「它被挂到了 server 的哪个路径上」。
- **注意**: 这不是 REQ-019 引入的债 —— 它从 REQ-002 起就存在，
  只是 REQ-019 在抽中间件时顺带发现了它。
- **要做需要什么**:
  1. 把 20+ 个 state 的构造与 `.nest()` 分离（state 构造留在 `main`）；
  2. 抽 `pub fn build_router(deps: AppDeps) -> Router`，`AppDeps` 装全部 state；
  3. 补「装配契约」测试：遍历 `router` 里的 route，断言**预期路径清单**全部存在
     （`axum::Router` 没有公开的 route 枚举 API → 可能需要用
     `tower::ServiceExt::oneshot` 对每个预期路径打一次，断言**不是 404**）。
- **优先级**: **P1**（现在已知的做法是「对每个路径打一次断言非 404」，
  成本不高但能抓住整组路由失踪）

---

## TD-077: REQ-019 三级限流在**生产装配**下恒不生效（已实证）

- **发现于**: REQ-019 实现复核（`qa-req019-impl-review.md` F15）
- **严重性**: **P0 —— 这是本需求的核心功能在生产路径上完全空转**
- **现状**:
  装配顺序是 `client_info`（最外）→ `deprecation` → `trace_id` → `rate_limit`
  → `nest("/api/rbac", ...)` → 子系统内层的 `authenticate`（**这里才 `with_tenant`**）。
  而 `client_info` **只填 IP / UA**（`platform-server/src/client_info.rs:53`，
  全文 `with_tenant` 出现 **0 次**）。
  → `rate_limit` 执行时 `ctx.tenant_id()` **恒为 `None`**
  → 走 `middleware.rs:110-115` 的 TD-072「无租户 → 放行」分支
  → **limiter 的 `check()` 一次都不被调用**。
- **实证**（复核做的，带对照组）:
  - 实验组（`client_info` 只填 IP/UA，**生产等价**）：5 次请求全 200，
    **limiter 被调用 0 次**；
  - 对照组（额外注入租户，**测试夹具做法**）：limiter 被调用 3 次。
  - 唯一差异是「是否注入租户」。
- **为什么测试没抓到**: `tests/middleware_assembly.rs` 的 `fake_client_info`
  **主动注入租户**（`:58-62`，注释自承「不注入租户的话，限流判据恒 200」）——
  夹具做了生产 `client_info` 不做的事，掩盖了缺陷。
- **影响面**: AC-S-031 / 032 / 033 在**生产路径**上**全部空转**。
  真库测试全绿是真的（它们直接调 `limiter.check()`），但它们验的不是生产链路。
- **三个候选修法**（**需用户裁定**，各自代价差别很大）:
  1. **顶层验令牌**：装配函数加一个「租户解析器」注入点，`main` 传 `IamService`
     验令牌拿租户/用户。改动只碰 `main.rs` + 装配函数；安全（令牌已验证）。
     代价：每请求多一次令牌验证（HMAC，微秒级）。
  2. **下沉到各子系统**：在每个 crate 的 `authenticate` 之后各自挂一层限流。
     语义最准、无重复验证。代价：要改 rbac/org/app/config/iam/notify/storage/audit
     等 **8+ 个已验收 crate**，属 D109 跨需求改动。
  3. **本轮不改**：明码记 TD，AC-S-031/032/033 从「达成」改为「不成立」。
- **优先级**: ~~P0~~ → ✅ **已解决（2026-11-23 用户裁定方案 ①，见 D346）**
- **修法**: 新增 `TokenTenantResolver` trait + `RateLimitState.token_resolver`；
  中间件在 `ctx` 无租户时从 `Authorization` 头验令牌拿身份；
  `main.rs` 注入 `TokenTenantResolverImpl`（走 `IamService::verify_access_token` **真验签**）。
- **判据**: `middleware_assembly` 新增 3 条，**带对照组**：
  组 A（有令牌+有解析器 → **429** + limiter 被调用）／
  组 B（无令牌 → 200 + limiter 不调用）／
  组 C（有令牌但无解析器 = 修复前 → 200 + 不调用，**回归护栏**）。
  变异验证：解析器强行置 `None` → **组 A 红**。
- **遗留**: `TokenTenantResolverImpl` 在 bin crate，测试引用不到 →
  「main 真的注入了它」这一层无判据（同 TD-076 的路由层遗留）。

---

## TD-078: 「导出超时会不会拖慢请求」在本技术栈下**无法端到端验证**

- **发现于**: REQ-019 实现复核（F17），在修正该判据的过程中发现
- **现状**: `BatchSpanProcessor` 用 `std::thread::Builder::spawn`
  （**普通 OS 线程**，不是 tokio task），而 `opentelemetry-otlp` 的
  `reqwest-client` 在该线程上首次导出时会 panic
  （`there is no reactor running`；见
  `opentelemetry-otlp-0.31.1/src/exporter/http/mod.rs:169` 的
  `reqwest::Client::builder().build()`）。
- **后果**: 「collector 不可达 → 导出超时 → 请求变慢」这个**具体场景**
  在本机无法复现 —— panic 取代了「等超时」。
- **当前判据能证明什么**（如实标注，已写入测试注释）:
  - ✅ 20 次请求在「配置了不可达 endpoint」时**总耗时仍 < 2 秒**
    → 证明**请求路径**没有被导出配置拖住（必要条件）；
  - ✅ 请求照常成功（导出旁路不影响业务）；
  - ❌ **不**证明「导出超时被正确限制在 `timeout_seconds` 内」。
- **要做需要什么**:
  1. 一个**可达但响应慢**的假 collector（本机可起一个只 accept 不回包的 TCP 服务）；
  2. 或用 `reqwest-blocking-client` 特性（但要评估包数净增与阻塞风险）；
  3. 或改用 `hyper-client`（opentelemetry-otlp 支持，可能不需要 tokio reactor）。
- **优先级**: P2（当前判据已覆盖「请求不被拖住」这个业务面；
  「超时值生效」是 OTel SDK 的行为，风险较低）

---

## TD-079: AC-S-034 的「跨 crate 分页契约统一」只验到**字段名**，没验**行为**

- **发现于**: REQ-019 实现复核（F19 + AC-S-034 的 `AuditPageView` 论证）
- **现状**（两件事）:
  1. 新端点 `GET /api/observability/rate-limit/status` 的 `items` **恒为 3 条**
     （三个限流维度），`nextCursor` **恒为 `null`** → **翻不动页**。
     AC-S-034 步骤 1 的「恰好 10 条 / total == 25」在本端点**不可能成立**；
  2. 「跨 crate 字段一致」的判据原先打在 `platform_audit::AuditPage` 上，
     而**真正上线序列化的是** `platform_server::audit::AuditPageView`。
     复核把 `AuditPageView.next_cursor` 改名后，那条判据**仍然全绿**（已实测）。
     `platform-server` 是 **bin crate**，集成测试**引用不到** `AuditPageView`。
- **已做的补救**:
  - 新增 `the_audit_http_response_uses_the_shared_field_names`：解析 audit 路由的
    **真实 HTTP 响应 JSON**，与本端点的键集合对照 —— 不依赖类型能被引用。
    ⚠️ 但它用的是「同形复刻类型」的路由（真实 handler 需要 PG），
    所以若 `AuditPageView` 与复刻类型不一致，仍发现不了；
  - 新增 `this_endpoint_has_at_most_three_rows_so_it_cannot_page`：
    **锁住能力边界**（若端点多出数据源 → 红，提醒更新覆盖声明）。
- **仍未做**: 「两个端点都能真的翻页且零重叠」的**跨 crate 行为**判据。
  目前翻页行为只在 `platform-audit` 的真库用例里验（25 行、两页零重叠）。
- **要做需要什么**: 把 audit 的 handler 与 `AuditPageView` 移到**库 crate**
  （`platform-audit` 或新 crate），让集成测试能直接驱动它。
- **优先级**: P2

---

## TD-080: 规格文档自称「9 条 AC」，实际 **10 条**

- **发现于**: REQ-019 实现复核（F22）
- **现状**: `grep -c "^### AC-S-0" acceptance.md` → **10**
  （AC-S-029 ~ AC-S-038 是闭区间 **10 条**）。而：
  - `acceptance.md:42`：「本需求自有 AC（**9 条**：AC-S-029 ~ AC-S-038）」
  - `acceptance.md:392`：「**7 条功能要求 → 9 条 AC**」
  - `acceptance.md:427`：「本需求的 **9 条** AC 全部只覆盖已认证请求」
  - `status.md:14`：「**9 条 AC**：AC-S-029 ~ AC-S-037」（**把 038 漏了**）
- **为什么记**: BA 在挑刺 B9 里专门批评过 rev 1「数错了」，
  现在 rev 2 的修正版**自身又数错**（补了 AC-S-038 却没把「9」改成「10」），
  且 `status.md:14` 与 `acceptance.md:42` **互相矛盾**（一个到 037，一个到 038）。
- **影响**: 「达成 N / 10」的**分母本身是错的**，凡引用「9 条」的下游文档都要修正。
- **本需求已直接改正文**（不留给以后）—— 此条目记录**为什么**发生过这次错。
- **优先级**: P3（文档一致性；已在 rev 4 修正）

---

## TD-081: 审计「归档 + 写入」的并发竞态**没有自动化判据**

- **发现于**: develop 全量独立 QA（B 片 F4）+ 我修复时实测
- **现状**: `platform_audit_archive_move` 原先不持 advisory lock，而
  `platform_audit_append` 持锁 → 并发时 `tip_hash` 会被陈旧值覆盖 →
  `ChainVerifier` 误报「链被截断」。
  **锁已加上**（迁移 `20261124000100_audit_hardening`，键与 `append` 完全一致）。
- **⚠️ 缺口**: 「锁生效」这件事**没有自动化判据**。
  我写了一版 `tokio::join!` 的并发用例（`concurrent_archive_and_append_keep_the_chain_tip_fresh`），
  但**实测把锁去掉后连跑 3 次仍然全绿** —— 两个操作的 SQL 往返在微秒级，
  `join!` 的调度粒度抓不住那个窗口。
  → 那条测试已如实改标注为「**回归护栏**，不是竞态判据」。
- **要做需要什么**: 用**两个真实会话按脚本逐句推进**（psql 或裸连接），
  在「archive 已写 tip、append 尚未写回」的时刻精确插入另一步。
  集成测试的 `oneshot`/`join!` 建模做不到。
- **优先级**: P2（锁本身已加，缺的是判据；风险是「有人删掉锁而无人发现」）

---

## TD-082: 租户隔离的 `SET LOCAL` 语义**没有判据**（变异后 38 测试全绿）

- **发现于**: develop 全量独立 QA（B 片 F7，做了变异实证）
- **证据**: 把 `crates/platform-db/src/tenant.rs` 的
  `set_config('app.tenant_id', $1, true)` 改成 `false`（`false` = 会话级，
  提交后**残留** → 下一个请求借到这条连接时看到**上一个租户**的数据）
  → `platform-db` 的 **38 个测试全绿**。
- **为什么严重**: `SET LOCAL`（事务级自动清除）是这个平台
  **租户隔离的第一道防线**（`tenant.rs` 开头的文档专门讲了
  「`SET` 与 `SET LOCAL` 的区别是串租户的根源」）。
  它被改坏 → RLS 的 `current_setting('app.tenant_id')` 会读到上一个租户的值 →
  **RLS 这道最后防线也失效**。而**没有任何测试会红**。
- **要做需要什么**:
  1. 一个「同一连接先后服务两个租户」的用例：
     租户 A 的请求结束后，用**同一条物理连接**跑租户 B 的查询，
     断言 B **看不到** A 的行（`false` 变异下会看到）；
  2. 需要能**固定连接**（`PgPool::acquire` 拿同一条，而不是每次新取）——
     `TenantDb` 目前不透出连接句柄，可能需要一个测试专用的旁路入口。
- **优先级**: **P1**（这是隔离机制的地基，且已有变异实证说明零覆盖）

---

## TD-083: 两张「每操作一行」的表**没有清理路径**

- **发现于**: develop 全量独立 QA（B 片 F6/F9）
- **现状**: 同项目的 `platform_rate_limit` 有 `xtask observability prune-rate-limit`，
  但这两张表只增不删：
  - `platform_notify_delivery`（每次入队一行）
  - `platform_storage_job` + `platform_storage_job_error`（每次导入导出任务一行）
- **影响**: 长期运行的实例会无限增长。REQ-019 刚为限流表补过同类问题（F2/TD-077 的兄弟问题），
  说明这是**系统性遗漏**而不是个别疏忽。
- **要做需要什么**: 按 `observability prune` 的范式各加一个
  `xtask notify prune` / `xtask storage prune`（含 `--older-than` 与保留期配置项），
  并在 `design-summary.md` 的「定期任务」表登记。
- **优先级**: P2（容量问题，不是正确性问题；但会随时间恶化）

---

## TD-084: `audit verify` 把整条在线链读进内存

- **发现于**: develop 全量独立 QA（B 片 F5）
- **现状**: `crates/platform-audit/src/verify.rs:234-242` 一次 `fetch_all` 取全部在线记录，
  **无 LIMIT**。QA 实测：5 万条约 **29MB** 载荷。
  而这是一条**用户可反复调用**的 GET 接口（`GET /api/audit/verify`），
  无并发上限 → 几个并发请求就能把内存打满。
- **要做需要什么**: 改成**流式/分批**校验（按 `created_at, id` 键集分页，每批 N 条，
  在批次间保持 `expected_prev_hash` 连续），或加并发闸门 + 上限。
- **优先级**: P2（需真实大数据量才暴露；单租户审计量小时无感）
