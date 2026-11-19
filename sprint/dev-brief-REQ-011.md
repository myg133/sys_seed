# Dev 任务书：REQ-011 前端 SDK 骨架

- **需求**: REQ-011「前端 SDK 骨架」
- **迭代**: Sprint 4（第一棒）
- **基线**: `develop` @ `a3fae66`（REQ-010 合并点）
- **分支**: `feature/REQ-011`
- **Worktree**: `/home/myg133/Projects/system_seed/feature-REQ-011`

---

## 0. 开工前必读（已调研过，不要重复摸索）

| 事实 | 位置 |
|---|---|
| 规格 | `BA/demands/REQ-011-frontend-sdk/acceptance.md`（**AC-S-064~075，12 条**） |
| 设计 | `BA/demands/REQ-011-frontend-sdk/design-summary.md`（**§2 后端 / §3 五个包 / §4 测试 / §5 风险**） |
| 用例 | `BA/demands/REQ-011-frontend-sdk/test-cases/`（QA 写的，逐条实现） |
| 契约 | `BA/contracts/api-conventions/v2.md`、`BA/contracts/extension-points/v2.md` |
| 决策 | `BA/demands/REQ-001-platform-foundation/decisions.md`（D1~D91） |

### 0.1 已核实的关键事实（**直接用，不要重新推导**）

**后端**
- 登录响应的 `permissions` 写死空数组 → `crates/platform-iam/src/service.rs:539`、`:665`
- `RbacService::effective_permissions(ctx, user_id)` **已实现**（展开通配）
  → `crates/platform-rbac/src/service.rs:780`
- **它没有任何 HTTP 路由暴露** → 要新加 `GET /api/rbac/me/permissions`
- 权限点注册表路由 `/permissions`（豁免权限点要求）→ `crates/platform-rbac/src/http.rs:420`
- `RbacState` = `{ service, checker, tokens }` → `crates/platform-rbac/src/http.rs:105`
- **依赖方向**：`platform-rbac → platform-iam`（**iam 绝不能依赖 rbac**，会成环）
- `PermissionChecker` trait 在 `crates/platform-core/src/extension/permission.rs:164`
  （方法全同步、dyn 兼容；读库在 `PermissionRefresher::ensure_fresh` 里）
- 幂等中间件**只在 `/api/iam` 路由上** → `crates/platform-iam/src/http.rs:295`
- 幂等头名 `idempotency-key` → `crates/platform-iam/src/http.rs:54`
- RFC 9457 错误体与 `code` 常量 → `crates/platform-core/src/problem.rs`
- 租户头 `x-tenant-id`、空间头 `x-space-id`（契约 C3）
- SPA fallback：路径无扩展名 → 回落 `index.html`
  → `crates/platform-server/src/static_files.rs:18`
- 装配顺序：`build_iam_state` → `build_org_state` → `build_rbac_state` → 配置模块
  → `crates/platform-server/src/main.rs:255~340`

**前端**
- Svelte **5.57.3**、vite-plugin-svelte **5.1.1**、vitest 3、Playwright 1.58
- 七个包全是占位，每个只有 `src/index.ts` + `test/<name>.test.ts`
- `packages/*/package.json` 的 build = `tsc -p tsconfig.json`，
  `tsconfig.json` 的 `include` = `["src/**/*.ts"]` → **`.svelte` 文件装不进包**
- `apps/shell` 用 `vite.config.ts` 的 `resolve.alias` **直指包源码**
  （`packages/<name>/src/index.ts`）→ 开发期无需先构建
- **前端没有任何路由库**（无 `svelte-routing` / `@sveltejs/kit`）
- `tsconfig.base.json`：`strict`、`noUncheckedIndexedAccess`、
  `verbatimModuleSyntax`（**类型导入必须 `import type`**）

### 0.2 环境（低配机）

```bash
export CARGO_TARGET_DIR=/home/myg133/Projects/system_seed/code/target   # 复用，节省 10~20 分钟
export XDG_RUNTIME_DIR=/run/user/$(id -u)
```

- 8 线程 / 7.7G 内存 / 磁盘预算 20G（已用 ~4G）
- **禁止 `cargo install`**；`.cargo/config.toml` 已固定 `jobs = 3`
- `cargo test` **只能有一个 `--`**：`cargo test --workspace -- --test-threads=2`
  （双 `--` → libtest 把 flag 当过滤串 → 0 passed 却 exit 0 → **假绿**）
- 数据库：**只用 `base_test`**
  - 应用连接（serve 用）：`postgres://base_app:base@127.0.0.1:5432/base_test`
    （**超管会被 `require_isolation` 拒绝启动，退出码 3**）
  - 迁移/夹具（脚本用）：`postgres://base:base@127.0.0.1:5432/base_test`
- **本机没有 psql 客户端** →
  `podman exec base-postgres psql -U base -d base_test -tAc "..."`
- 注册密码须过策略：`Tr0ub4dor&3xKq`
- 容器已起（`base-postgres` / `base-redis` healthy）

### 0.3 硬约束（违反即返工）

| # | 约束 |
|---|---|
| 1 | **`Cargo.lock` 包数 = 295，不得增长** —— 不引 `utoipa` / 新 Rust 依赖 |
| 2 | **不引 MSW / Storybook** —— 前端单测用**可注入的 `fetch` 假件** |
| 3 | **不引 npm 路由库** —— 路由用 History API 极简实现 |
| 4 | `Cargo.lock` **第三方 registry 284 不变** |
| 5 | 前端断言**必须断「等于期望值」**，不能只断「不等于默认值」（D83） |
| 6 | 注释里凡写「也覆盖 X」，必须有**变异验证**支撑（Sprint 3 教训） |
| 7 | 仓里有**两个锁文件**：`pnpm-lock.yaml`（CI 权威）+ `bun.lock`（需同步） |
| 8 | `pnpm-workspace.yaml` 的 `allowBuilds` 只放行 `esbuild` —— 新依赖若需构建脚本会被拦 |

---

## 1. 后端：补齐权限点数据出口

### 1.1 `platform-core`：新增 provider trait

**不要**改 `PermissionChecker`（那是「判一个权限点」，语义不符）。
**新增**一个独立的 trait：

```rust
// crates/platform-core/src/extension/permission.rs
pub trait PermissionProvider: Send + Sync + 'static {
    /// 当前用户的有效权限点集合（**展开通配**）。
    fn effective_permissions<'a>(
        &'a self,
        ctx: &'a RequestContext,
        user_id: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<String>, String>> + Send + 'a>>;
}
```

**为什么放 `platform-core`**：`platform-iam` 与 `platform-rbac` **都要用它**，
而两者互不依赖（iam 不能依赖 rbac）。放 core 才能让装配在
`platform-server` 完成（依赖单向 `平台 → trait ← 业务`）。

**为什么用 `Pin<Box<dyn Future>>`**：与同文件里已有的 `PermissionRefresher`
完全同构（`PermissionChecker` 全同步是因为它读的是**已填好的缓存**；
而这里是**首次拉取**，必须真的读库）。

### 1.2 `platform-rbac`：实现 + 新端点

- `RbacService` 实现 `PermissionProvider`（直接转调已有的 `effective_permissions`）
  —— **不得另写一套展开逻辑**（两套逻辑分叉 → 前端说有、后端 403）
- 新增路由 `GET /me/permissions`：
  - 挂在 `platform_rbac::http::router`（**注册表那条路由旁边**）
  - **豁免权限点要求**（任何已认证用户可读自己的），
    但**要求已认证**（未认证 → 401 `UNAUTHENTICATED`）
  - 响应体：`{ "permissions": ["rbac:role:read", ...] }`（**对象**，不是裸数组）
  - 错误体走 `rbac_problem_response`（与同文件其他 handler 一致）

### 1.3 `platform-iam`：登录响应填权限点

- `IamService` 持有一个**可选**的 `Arc<dyn PermissionProvider>`
  （用 `Option` + 可后设的装配接口，或在构造时传入）
- `login()` 成功时取该用户的有效权限点集合填入 `LoginResponse.permissions`
- **未注入 provider 时** → 空数组 `[]`（不是 `null`、不是省略字段），
  登录**仍然成功** —— 不能因为 RBAC 没起就登不了录
  （fail closed 的是「判定」，不是「登录」）

### 1.4 `platform-server`：装配

在 `main.rs` 里，rbac 装配完成后把 provider **注入** iam 服务。
注意**装配顺序**：iam 在前、rbac 在后 → 需要一个「后设」接口
（如 `set_permission_provider`），或调整装配顺序。

---

## 2. 前端：五个包

### 2.1 `@base/sdk` —— API client

**接口**（详见 design-summary §3.2）：

```ts
createApiClient({
  baseUrl, fetch?, getAccessToken?, refreshToken?, tenantId?, spaceId?, onUnauthorized?
})
```

**必须实现的三件事**（AC-S-069/070/071/072）：

1. **错误模型**：解析 RFC 9457 → `{ code, status, title, detail }`。
   `code` **优先**；非 JSON → `INTERNAL_ERROR`；网络失败 → `NETWORK_ERROR`。
   **不把原始响应体塞进错误信息**。
2. **token 刷新**：401 → refresh → 重放原请求。
   - **并发去重**：多个请求同时 401 时**只发一次** refresh
     （后端 refresh 是**轮换 + 重放检测**，多次 refresh 会**踢掉整个会话**）
   - **请求体必须可重放**：先缓冲成字符串再发（`ReadableStream` 重放时已失效）
   - 刷新失败 → 抛**后端返回的**码（`TOKEN_EXPIRED` / `TOKEN_REVOKED` /
     `TOKEN_REUSE_DETECTED` 之一），**不无限重试**（D94：**透传**不合成）
   - ★ **重放请求不带 `idempotency-key`**（D92）—— 401 意味着「请求没执行」，
     沿用旧键会被后端判为「还在处理」→ **409 CONFLICT**
3. **幂等键**：`POST`/`PUT`/`DELETE` 自动带 `idempotency-key`（UUID v4）；
   `GET`/`HEAD` **不带**；**每次请求新键**（复用会被后端当重放 → 静默丢数据）；
   键**不得由路径派生**、**不得跨路径复用**（后端去重键不含 method/path）
4. **上下文头**：`x-tenant-id` / `x-space-id`；**未配置时不带头**（空串 ≠ 不带）；
   注入于**所有**请求（**含 login / refresh** —— 否则没配租户时根本登不进去）
5. **合成码**：`client.NETWORK_ERROR` / `client.INTERNAL_ERROR`（带前缀，D94）

**公开 API 已钉死** —— 见 design-summary **§3.2.1**（`ApiError` / `RequestOptions` /
`ApiClient` 的完整签名）。**照抄，不要另设计一套**。

### 2.2 `@base/auth` —— 登录态与权限

- `can(permission)`：权限点集合**为空** → 一律 `false`（fail closed）；
  集合里没有 → `false`；**未登记**权限点 → `false`
- **`guard(permission)`**（框架无关的纯判定，签名见 design §3.3.1）：
  - 未登录 → `{ allowed: false, redirectTo: '/login' }`（**不是** 403）
  - 已登录无权限 → `{ allowed: false, redirectTo: '/403' }`
  - 有权限 → `{ allowed: true }`
- **不依赖** `@base/ui`（AC-S-075）
- ⚠️ **`<Can>` 组件不在本包** —— 落在 `apps/shell`（rev 2 裁定 #7，见 §2.4）

### 2.3 `@base/theme` —— 设计令牌

- 令牌以 **CSS 变量**写到根元素（inline style，切换即时生效）
- **令牌契约表**见 design **§3.4.1**（精确的变量名清单 + 亮/暗取值，**照抄**）
- 暗色模式：切 `data-theme="dark"` + `color-scheme: dark` + 换一组颜色变量；**可逆**
- 主色来自 `GET /api/config/branding`；**只接受合法十六进制**，
  非法 → **自己**把 `--base-primary` 写成 `currentColor`
  （**不依赖宿主样式表** —— 否则读到的是空串，预期不成立）
- **源码里不许有品牌颜色字面量**
- ⚠️ **唯一允许颜色字面量的文件**：`packages/theme/src/tokens.ts`（D95）

### 2.4 `@base/ui` —— 基础组件

Button / Input / Card / Table / Dialog + token 消费。

**★ 落地形态（rev 2 裁定 #7 —— 与 rev 1 不同）**：

| 位置 | 内容 |
|---|---|
| `packages/ui/src/**` | **逻辑与样式契约**：变体名、类名、props 类型、行为函数 |
| `apps/shell/src/components/*.svelte` | **Svelte 组件**（薄封装，调用包的逻辑） |
| `apps/shell/src/components/Can.svelte` | `<Can>` 组件 |

**为什么**：包的 build 是 `tsc -p tsconfig.json`（`include` 只有 `src/**/*.ts`）→
`.svelte` 文件**进不了产物**；引 `svelte-package` = **新增依赖**（违反本期零新依赖约束）。
而 `apps/shell` **已支持** `.svelte`（tsconfig 含 `src/**/*.svelte`，vite 插件已装）。
**组件库发逻辑，宿主应用发视图** —— Svelte 生态的常规做法。

**硬规则**：组件**只消费 CSS 变量**，不硬编码颜色字面量（AC-S-073 步骤 4 / D95）。

### 2.5 `@base/hooks` —— 通用 hooks

分页 / 表格状态 / SSE-WebSocket / 剪贴板。
- 分页、表格状态：**纯状态容器**（不绑框架）
- SSE/WebSocket：**重连退避** + **关闭时不重连**（组件卸载后重连会泄漏）
- 剪贴板：不可用时**返回失败**（不静默吞掉）

### 2.6 ★ `apps/shell` 的最小壳（AC-S-067 / AC-S-068 的应用侧）

本期要做（**只做这些**，不做完整布局框架）：

| 件 | 要求 |
|---|---|
| 登录页 | 输入用户名密码 → 调 SDK 登录 → 拿权限点 |
| 极简路由 | **History API**，无新依赖；深链可达（服务端已有 SPA fallback） |
| 受保护页 | 路由元数据接 `guard(permission)`；**守卫放行后才调用**数据加载函数 |
| 403 页 | 守卫拒绝时的落点 |
| `<Can>` 组件 | 见 §2.4 |
| 品牌委托 | `apps/shell/src/lib/branding.ts`（REQ-010 已实现）**改为委托 `@base/theme`** —— 消除两份实现（否则 REQ-010 的既有 E2E 会因漂移而红） |

**关键**：「403 页**不发起**目标接口的请求」→ 受保护页的数据加载函数
**在守卫放行后才被调用** → 该接口请求计数为 **0**。

---

## 3. 依赖方向（单向，AC-S-075）

```
@base/theme   （无内部依赖）
@base/ui   →  @base/theme
@base/hooks   （无内部依赖）
@base/sdk     （无内部依赖）
@base/auth →  @base/sdk
```

- `@base/sdk` **不依赖** `@base/ui`（否则任何用 SDK 的产物都拖进组件代码）
- `@base/auth` **不依赖** `@base/ui`
- `@base/ui` **不依赖** `@base/hooks`（表格要用分页 hooks 时，
  **停下来问 BA**，不要自己破例）

---

## 4. 验收与自验

**逐条**对照 `test-cases/` 里的用例实现。实现完自验：

```bash
cd /home/myg133/Projects/system_seed/feature-REQ-011
export CARGO_TARGET_DIR=/home/myg133/Projects/system_seed/code/target
export XDG_RUNTIME_DIR=/run/user/$(id -u)

cargo test --workspace -- --test-threads=2     # 注意：只有一个 --
cargo clippy --workspace --all-targets -- -D warnings
pnpm -r check && pnpm -r test && pnpm -r build
cargo xtask capabilities --check               # 能力索引一致性（见下）
```

### 4.1 ★ 能力索引必须重生成

`capabilities/sdk/*.json` 由 **`cargo xtask capabilities`** 从包源码**自动生成**
（`generated_by: "cargo xtask capabilities"`）。本需求改动了 5 个包 → 索引会失配。

```bash
cargo xtask capabilities            # 重新生成
cargo xtask capabilities --check     # 校验一致（CI 会跑）
```

**不重生成 → CI 的能力索引检查会红**。生成后**把变更一起提交**。

### 4.2 自验报告

写 `feature-meta/dev-report.md`，必须含：
- 每条 AC 的**实现位置**（文件:行号）
- **实际跑出的数字**（不是「应该通过」）
- **自验发现的问题**（不要只报好消息）
- **没做的部分 + 原因**
- **依赖预算核对**：`Cargo.lock` 包数是否仍为 **295**（后端任务）

> ⚠️ **「我修好了」不由你自证** —— Sprint 3 教训：修复轮后必须有
> **没参与修复的人**独立复核。你自验通过 ≠ 验收通过。

---

## 5. 提交

- 分支 `feature/REQ-011`，**不要**合到 `develop`
- 提交信息格式：`[Dev] <做了什么> (关联: REQ-011)`
- 一次提交一件事（后端 / SDK / auth / theme / ui / hooks 可分开）
- **不要**改 `Cargo.lock`（除非确有必要 —— 必要的话先问 BA）
