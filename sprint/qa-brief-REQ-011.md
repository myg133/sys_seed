# QA 任务书：REQ-011 前端 SDK 骨架

- **需求**: REQ-011「前端 SDK 骨架」
- **迭代**: Sprint 4（第一棒）
- **基线**: `develop` @ `a3fae66`（REQ-010 合并点）
- **Dev 分支**: `feature/REQ-011`
- **性质**: 底座里**第一个「让业务方少写代码」的前端需求** —— 前端包全是占位，`can()` 根本没有数据源

---

## 背景

REQ-009 落地了服务端鉴权，REQ-010 落地了配置与品牌。但**前端七个包至今全是占位**：
`packages/auth/src/index.ts` 只有 `AuthState` 形状和 `isAuthenticated`，**没有 `can()`**；
`packages/theme` 只有 3 个 token 默认值，**没有 CSS 变量输出、没有暗色模式**；
`packages/sdk` 只有 `createApiClient` 的 URL 拼接，**不发请求**。

本需求把它们变成能真实跑通的实现，**并补上后端缺失的数据出口**。

### ★ 本需求与前两棒最大的不同：它要动后端

`can()` 需要「当前用户拥有哪些权限点」。核过后端代码，**这个数据没有任何出口**：

| 候选 | 现状（证据） |
|---|---|
| 登录响应的 `permissions` | `crates/platform-iam/src/service.rs:539`、`:665` 写死 `Vec::new()` |
| `GET /api/rbac/permissions` | 返回**注册表**（全体已登记权限点），不是「我拥有的」 |
| `RbacService::effective_permissions` | `crates/platform-rbac/src/service.rs:780` **有实现**，但**无 HTTP 路由暴露** |

**不做这个接口，AC-S-066 / 067 / 068 三条全部落空。**

---

## 角色

你是 **QA**。规则：

- **你不写实现代码、不改实现**（临时变异除外，且必须逐字节还原）
- **你不 commit**。报告写到 `feature-meta/qa-report.md`，由 BA 落到 `demand` 分支
- **每项自己复现**，不采信 Dev 自验报告的数字

---

## 需求文档（绝对路径）

| 文件 | 作用 |
|---|---|
| `BA/demands/REQ-011-frontend-sdk/acceptance.md` | **12 条 AC（AC-S-064~075）**，唯一验收基线 |
| `BA/demands/REQ-011-frontend-sdk/design-summary.md` | 设计决策（§2 后端 / §3 五个包 / §4 测试策略 / §5 风险 R1~R10） |
| `BA/demands/REQ-011-frontend-sdk/test-cases/` | **你写的用例集**（按它跑） |
| `BA/contracts/api-conventions/v2.md` | 契约（错误体 C6、配置 C9/C10） |
| `BA/contracts/extension-points/v2.md` | C-5（鉴权入口唯一性） |
| `BA/demands/REQ-001-platform-foundation/decisions.md` | **D1~D91** 全部历史决策 |

---

## A. ★ 最高优先级：权限闭环（「前端能自说自话」）

### A-1 `/api/rbac/me/permissions` 真的给出「我拥有的」（AC-S-064）

必须验：
1. 返回**展开通配**后的集合（不是原始 grant）
2. 未授予的权限点**不在**集合里
3. **未认证 → 401** `code=UNAUTHENTICATED`（不是 403、不是 404）
4. **该接口豁免权限点要求** —— 普通用户（无任何管理权限点）也能读自己的

> ⚠️ **第 4 点最容易翻车**：若接口自己也要权限点，就会出现
> 「要先有权限才能问有没有权限」的循环 —— 用户首次进页面拿不到权限点。
> （与 REQ-009 的「注册表查询豁免」同一个道理。）

### A-2 两个入口必须给**同一个答案**（AC-S-065 步骤 2）

登录响应的 `permissions` 与 `/api/rbac/me/permissions` **逐项相等**。
> ⚠️ 若两边用了不同的展开逻辑 → 前端 `can()` 说有、后端 403。
> 变异验证：临时改一边的展开逻辑，确认另一边的测试**会红**。

### A-3 无角色用户 → `[]`（AC-S-065 步骤 3）

不是 `null`、不是**省略字段**、不是 `["*"]`。
> 省略字段最阴险 —— 前端 `resp.permissions ?? []` 看着能跑，
> 但 TypeScript 类型会说是 `string[] | undefined`，严格模式下直接编译失败。

### A-4 `can()` 与后端 `require_permission` 判定**一致**（AC-S-066 步骤 5）

同一组权限点，两边判定结果必须相同。
> **变异验证**：把前端的判定改成"永远 true"或"永远 false"，
> 确认测试**会红**。断言"不等于默认值"抓不到这类假绿（D83）。

### A-5 `<Can>` 无权限时**不渲染**（AC-S-067）

**不是 `display:none`** —— 内容仍在 DOM 里，用户从 DevTools 能看到，
自动化测试用 `toBeHidden()` 也可能判过。必须验 **DOM 里查无此内容**。

### A-6 路由守卫拦得住**直接敲 URL**（AC-S-068 步骤 2）

这是**最容易假绿**的一条：很多实现只在"程序化跳转"时做守卫，
直接敲 URL 就漏了。必须**绕过前端导航**直接访问路径。

---

## B. ★ 高危区：SDK 的三个「静默失败」陷阱

### B-1 401 刷新**并发只刷一次**（AC-S-070 步骤 2）

**后端 refresh 是轮换 + 重放检测**（`crates/platform-iam/src/service.rs:576`）。
5 个并发 401 → 5 次 refresh → **整个会话被踢下线**。

> **测试构造要点**：必须**同时**发出 5 个请求（不是顺序 await）。
> 顺序 await 每次都拿到的是新 token，**根本触发不了这个 bug** ——
> 这是「测试写得对但测不到东西」的典型。

### B-2 请求体**必须可重放**（AC-S-070 步骤 4）

若用 `ReadableStream` 作为 body，重放时已失效 → 第二次发空 body。
表现为「第一次失败、第二次成功」的诡异 bug。必须断言**重放请求的 body 逐字节相同**。

### B-3 幂等键**每次新生成**（AC-S-071 步骤 3）

> **为什么这是静默丢数据**：后端幂等中间件
> （`crates/platform-iam/src/http.rs:295`）按 `(tenant_id, idempotency_key)` 去重，
> 同一个键的第二次请求**直接返回首次结果，新数据不落库** ——
> 表现为「点了保存没反应」。

**验收**：两次**相同逻辑**的写请求 → 两个**不同**的键。

### B-4 幂等只覆盖 `/api/iam` 路由（AC-S-071 步骤 4 的边界）

后端幂等中间件**只在 IAM 路由上**。`/api/rbac/*` 等**没有**。
验收时**不要**假设全站都有幂等支持 —— 这条 AC 的端到端部分要么限定在
`/api/iam` 路由上，要么明确标注「本期只有 IAM 路由有幂等支持」。

### B-5 错误码**优先于**英文原文（AC-S-069）

后端错误体是 RFC 9457（`crates/platform-core/src/problem.rs`），
有 `title`（中文）、`detail`（英文原文）、`code`（稳定码）。
前端**只认 `code`**。断言 `code` **精确等于** `INSUFFICIENT_PERMISSION`，
不是 `title` 的「未认证」。

### B-6 非 JSON 响应体与网络失败（AC-S-069 步骤 3）

不能抛裸 `TypeError`，也不能把原始响应体塞进错误信息。
必须断 `code = 'INTERNAL_ERROR'` / `'NETWORK_ERROR'`。

---

## C. 主题令牌与包依赖

### C-1 暗色模式断「**等于**期望值」（AC-S-073 步骤 2）

**不是**「不等于亮色值」（D83 的假绿经典）。
且必须可逆：切回亮色 → **等于**亮色期望值。

### C-2 品牌主色只接受合法十六进制（AC-S-074 步骤 2）

非法值（`"red"` / `"#zzz"`）→ 保持**中性兜底**，不是原样写入。
> ⚠️ 若不校验就写进 CSS 变量 → **CSS 注入**（`red; background: url(...)` 之类）。

### C-3 无硬编码颜色（AC-S-073 步骤 4 / AC-S-074 步骤 3）

机械扫描 `packages/theme/src` 与 `packages/ui/src` 的颜色字面量。
> ⚠️ **注意**：`packages/theme/src/index.ts` 现在就有 `#1677ff`（占位默认值）。
> 它算不算「硬编码」需要**明确裁定** —— 若算，本期必须清掉；
> 若不算（中性兜底 / 默认值可接受），规格要写清「哪些位置允许字面量」。

### C-4 包依赖单向（AC-S-075）

`@base/sdk` **不依赖** `@base/ui` / `@base/theme` / `@base/hooks`；
`@base/auth` **不依赖** `@base/ui`；`@base/ui` → `@base/theme` 单向。

> ⚠️ **现实冲突**：表格组件通常要用分页 hooks。
> 若实现时发现必须打破这条，**停下来问 BA**，不要自己破例。

---

## D. 回归与环境

| 项 | 要求 |
|---|---|
| Rust | `cargo test --workspace -- --test-threads=2`，**N passed > 0**（注意只有一个 `--`） |
| 前端 | `pnpm -r test` 全绿 |
| E2E | `bun run test:e2e` 全绿 |
| 依赖预算 | `Cargo.lock` 包数 **= 295**，**不得增长**（不引 utoipa / MSW / Storybook） |
| 磁盘 | 预算 20G，已用 ~4G；**禁止 `cargo install`** |
| 编译 | `export CARGO_TARGET_DIR=/home/myg133/Projects/system_seed/code/target`（复用，节省 10~20 分钟） |
| 数据库 | 应用连接**必须** `postgres://base_app:base@127.0.0.1:5432/base_test`；**超管会被 `require_isolation` 拒绝启动（退出码 3）** |
| psql | 本机**没有** psql 客户端 → `podman exec base-postgres psql -U base -d base_test -tAc "..."` |
| 容器 | rootless podman；`export XDG_RUNTIME_DIR=/run/user/$(id -u)` |

---

## E. ★ 继承 Sprint 3 的三条教训

1. **「我修好了」必须由没参与修复的人验证** —— 修复轮后必须有独立复核
2. **注释与代码不一致比没有注释更坏** —— 注释声称覆盖 X 必须有变异验证支撑
3. **每个「应当被拦住」的机制，必须自己造违规复现** —— 不采信 Dev 的数字

以及本仓的**假绿三兄弟**：

| 陷阱 | 正确写法 |
|---|---|
| 否定式断言 | 断「**等于**期望值」 |
| 快照被污染 | 取点必须在**任何被测写之前**，且含 `updated_at` |
| 环境变量劫持 | 断言「默认行为」要走**不受环境变量影响**的入口 |
