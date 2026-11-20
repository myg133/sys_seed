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
