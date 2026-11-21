# REQ-016 E2E 补齐报告（`e2e/apps.spec.ts`）

- **交付物**：`e2e/apps.spec.ts`（**新建，573 行**，6 条用例）+ 本报告
- **代码改动文件集**：**仅** `e2e/apps.spec.ts`（`git status --short e2e/` 只有 `?? e2e/apps.spec.ts`）
- **结果**：`apps.spec.ts` **6 passed / 0 failed / 0 skipped**；全量 `e2e/` **50 passed / 0 failed / 0 skipped**（既有 44 条未破）
- **日期**：2026-11-21

---

## 1. 六条用例逐条

| # | 测试名 | `e2e/apps.spec.ts:行` | 断言什么（可观测后果） | AC |
|---|---|---|---|---|
| 1 | 工作台：finance 在 general 前、组内按 (sortOrder, displayName)，hidden 应用不出现 | `:266` | ① 分类组**按字典序升序**（读 `app-workspace-category` 全部文本后与自身排序比较）；② `finance` 的下标 **<** `general` 的下标；③ finance 组内**夹具卡片**顺序 `[E2E 费用申请, E2E 差旅报销]`（sortOrder 10 < 20）；④ general 组内夹具顺序 `[E2E C 应用, E2E D 应用]`（sortOrder 并列 → displayName 升序）；⑤ hidden 的 `E2E E 应用` 在页面上**出现 0 次**；⑥ 4 个 active 夹具**各恰好出现 1 次** | AC-S-160 步骤 3 |
| 2 | 工作台：launch_url 为空的应应用仍显示卡片，但没有可点入口（D279） | `:329` | 用 `app:registry:write` 账号 `POST /api/apps`（**不给 `launchUrl`**）现造样本 → ① 该卡片**存在**（D279：工作台不按 launch_url 过滤）；② 卡片内 `app-workspace-open` **0 个**、`app-workspace-no-entry` 可见、`a` 元素 **0 个**；③ 点卡片名后 URL **仍是** `/workbench` | D279（AC-S-160 末段） |
| 3 | 导航：external 应用项点击 → 整页跳到 launch_url，且未触发 SPA 内部导航 | `:372` | 现造一条 `launchUrl = https://expense.example/app` 的应用，`page.route` 就地满足成桩页 → ① `page.url()` **逐字** `=== https://expense.example/app`；② 跳转后看到桩页内容 `stub-ok`（真顶层文档导航）；③ **`history.pushState` 计数增量为 0** | AC-S-163 步骤 3 / TC-016-052 |
| 4 | 导航：audit 项随 audit:events:read 出现/消失，应用项两边都在 | `:436` | ① `u-perm-audit`（持 `audit:events:read`）页面上「审计日志」项 **1 个**；② 同页 `E2E 差旅报销` 应用项 **1 个**；③ `u-perm-read`（不持）审计项 **0 个**；④ 应用项**仍 1 个**（对照）；⑤ `perm` 缺省的「工作台」项在无权限账号下**仍 1 个**（D269） | AC-S-162 步骤 2 |
| 5 | 应用注册表：注册 → 改名 → 清空入口（专用端点）→ 删除，每步在页面上可观察 | `:472` | ① 注册后表格出现该行，入口列 = `https://probe.example/app`；② 点「编辑」后 `客户端标识` 输入框**被禁用**（编辑态），改名保存后该行第 2 个 `<td>` **逐字**等于新名；③ 点「清空入口」→ 捕获到的响应是 **`DELETE …/launch-url` + 204**，入口列变 `-`；④ 点「删除」后该行 **0 个**；⑤ 收尾用 API 复查库里确实没有该名（防「只是没刷新」的假绿） | AC-S-156 主路径 + AC-S-283（D283） |
| 6 | 只读账号进 /admin/apps → 落 403 页，且守卫不放行（GET /api/apps 请求数为 0） | `:544` | ① 落在 `data-testid="forbidden"` 的 403 页（pathname `/403`）；② `page.on('request')` 对 **精确**路径 `/api/apps` 计数 = **0**；③ `app-registry` 容器 **0 个**（不是「渲染了但空」） | AC-S-156 守卫面 |

### 「pushState 计数」为什么要跨文档（第 3 条）

计数器若只活在当前文档的变量里，整页跳转后读到的是**新文档的 0** —— 那证明不了任何事（整页跳转天然归零）。所以计数经 **`window.name`** 传递：

- `window.name` 是唯一**跨文档保留**的字符串槽（同源 / 跨源都保留，浏览器规范保证）；
- 本产品**没有**用过它（`grep -rn "window.name" apps/shell/src packages` → 0 命中）；
- 用 `page.addInitScript` 安装 → 每个新文档自动重新包装 `pushState` 并读回累计值。

---

## 2. 跑过的命令与结果

```bash
# 环境
podman ps -a
DATABASE_URL=...base_test cargo run -q -p platform-server --bin base -- migrate up
DATABASE_URL=...base_test cargo xtask tenant-policies
./scripts/seed-e2e.sh

# 单文件
npx playwright test e2e/apps.spec.ts --reporter=list
#   → 6 passed (13.9s)

# 全量
npx playwright test e2e/ --reporter=list
#   → 50 passed (1.0m)      ← 既有 44 条 + 新增 6 条
```

| 命令 | passed | failed | skipped |
|---|---|---|---|
| `npx playwright test e2e/apps.spec.ts` | **6** | 0 | **0** |
| `npx playwright test e2e/` | **50** | 0 | **0** |

**0 skipped**：全文件**没有** `test.skip` / `test.fixme` / `test.todo` 调用
（第 37 行出现的 `test.skip` 是注释里的说明文字）。

### 变异验证（证明断言不是「永远通过」）

写绿后做了 3 次**故意破坏**的对照，确认用例会红，随后还原：

| 变异 | 结果 | 说明 |
|---|---|---|
| `Workbench.svelte` 的 external 点击从 `env.assign` 改成 `history.pushState` | **1 failed**（`waitForURL` 超时 —— `pushState` 不改文档） | 第 3 条有区分力；还原后 `md5sum` 与备份一致、`git diff` 为空 |
| `e2e-app-e.status` → `active` | **1 failed**（general 组多出一项） | hidden 过滤判据有区分力；已还原 |
| `e2e-app-a.sort_order` `20` → `5` | **1 failed**（finance 组顺序反转） | sortOrder 排序判据有区分力；已还原 |

---

## 3. seed 里应用的确切值（核实自 `scripts/seed-e2e.sh:492-515` + 实测库）

| id | client_id | displayName（**逐字**） | category | launch_url | sort_order | status |
|---|---|---|---|---|---|---|
| `e2e-app-a` | `client-app-a` | **`E2E 差旅报销`** | finance | `https://a.example/` | 20 | active |
| `e2e-app-b` | `client-app-b` | **`E2E 费用申请`** | finance | `https://b.example/` | 10 | active |
| `e2e-app-z` | `client-app-c` | **`E2E C 应用`** | general | `https://c.example/` | 0 | active |
| `e2e-app-d0` | `client-app-d` | **`E2E D 应用`** | general | `https://d.example/` | 0 | active |
| `e2e-app-e` | `client-app-e` | **`E2E E 应用`** | general | `https://e.example/` | 0 | **hidden** |
| `e2e-app-other` | `client-app-other` | `B 租户的应用`（tenant-b） | general | `https://other.example/` | 0 | active |

> ⚠️ displayName **带 `E2E ` 前缀**，已逐字核实（不是 `费用申请` / `差旅报销`）。
> ⚠️ 播种里 6 个应用**全部有 `launch_url`**，且**没有一个**指向 `https://expense.example/app`
> → 第 2、3 条**必须自造样本**（见 §4）。

---

## 4. 自造的数据 / 做不到的地方

### 4.1 第 2 条：没有「`launch_url` 为空」的播种应用 → **自造**

用 `u-perm-app@example.com`（持 `app:registry:write`）`POST /api/apps` 时**不给 `launchUrl`**，
`displayName = "E2E 临时无入口应用"`。用例结束在 `finally` 里 `DELETE /api/apps/{id}`，
开头还按 displayName 清残留。

### 4.2 第 3 条：没有指向 `https://expense.example/app` 的应用 → **自造**

AC-S-163 步骤 3 / TC-016-052 逐字要求该目标；最接近的播种值是 `https://a.example/`
→ 现造一条 `displayName = "E2E 临时外部应用"`、`launchUrl = https://expense.example/app`，跑完即删。

### 4.3 造数时撞到的真实约束（已修进 spec）

- **`(tenant_id, client_id)` 唯一索引** → 一个 client 只能接一个应用。第一版硬编码
  `client-A`，在「先跑过集成测试」的环境里直接吃 `409 APP_DUPLICATED`。
  → 新增 `pickFreeClient()`：先 `GET /api/apps` 取已引用的 client 集合，再从候选里挑第一个空闲的。
- **改名断言第一版零区分力**：新名以旧名为前缀，子串匹配永远命中。
  → 改成断言该行第 2 个 `<td>` **逐字**等于新名。

### 4.4 第 1 条为什么不再断言「组恰好 2 个」（**实测发现的真问题**）

`base_test` 是**共享**库。第一次跑通后，我把它改成「组恰好 2 个 + general 组恰好 C→D」，
结果被 `crates/platform-app/tests` 播种的 `created_by='fixture'` 应用（`A 的接入`，
`category=general`）打红 —— 这类断言**依赖别人的数据**，是脆弱的。

→ 改成断**实现性质**（组按字典序升序、finance 在 general 前、**夹具子集**内部顺序、
hidden 不在页面上、4 个 active 夹具各恰好 1 次），并保留 3 次变异验证证明仍有区分力。
这是**数据污染防护**，不是削弱断言：被验的那几条性质（分组有序、sortOrder 优先、
并列按 displayName、hidden 过滤）一条没少。

### 4.5 **我的 spec 曾经把既有 `audit.spec.ts` 打红**（已修）

第一版 `login()` 没带 `x-forwarded-for` → 登录写入的 `iam.login.succeeded` 审计事件
`ip` 为空 → 它是最新记录、占据审计页首行 → 既有 TC-014-112「首行五格都非空」变红
（实测 49 passed / 1 failed）。
→ 现在 `login()` 统一 `page.setExtraHTTPHeaders({ 'x-forwarded-for': '203.0.113.9' })`
（与 `audit.spec.ts` 同款），修完全量 50 passed。

### 4.6 环境坑（不在交付物里，但复跑的人会撞上）

| 现象 | 原因 | 处置 |
|---|---|---|
| 库里有 `app-a`/`费用申请`/重复授权 | 上一轮集成测试残留 → `seed-e2e.sh` 自检失败 → 服务起不来 | 按 `scripts/test-integration.sh` 方式 `DROP SCHEMA public CASCADE; CREATE SCHEMA public;` + `deploy/seed/00-init.sql` |
| `cargo xtask tenant-policies` 报 `sandbox_tenant_item does not exist` | `base migrate up` **只跑根 `migrations/`**，不跑 `modules/*/migrations/` | 手工 `psql < modules/sandbox/migrations/*.up.sql`（4 个）后再生成策略 |
| `base serve` 启动即退出 `permission denied for table platform_permission` | 重建 schema 后 `base_app` 授权丢失 | 重跑 `deploy/seed/02-app-role.sql` |
| **跑 E2E 期间 `base_test` 被清空 / 夹具变 `created_by='fixture'`** | **另一个进程在跑 `cargo test`（实测撞到 `platform-oauth`、`platform-audit`、`platform-app` 三次）** —— 它们重置共享库并播自己的夹具 | 等它结束后重建环境再跑；这是并行开发下的固有争用 |

`platform_app%` 的 RLS 策略实测恰好 2 条：
```
platform_app        | platform_app_tenant_isolation
platform_app_access | platform_app_access_tenant_isolation
```

---

## 5. 有没有改动 `e2e/apps.spec.ts` 之外的文件

**最终代码状态：没有。** `git status --short e2e/` 只有 `?? e2e/apps.spec.ts`；
新增的文档是本报告 `feature-meta/req-016-e2e.md`（派单明确要求）。

过程中为 §2 的**变异验证**临时改过 `apps/shell/src/pages/Workbench.svelte`（把 external 点击改成
`pushState`）并重建 `dist`，随后**用备份还原**：

```
md5sum apps/shell/src/pages/Workbench.svelte /tmp/Workbench.bak
  323d20f6d5dbc8d8c8ec45d61235e5b5  apps/shell/src/pages/Workbench.svelte
  323d20f6d5dbc8d8c8ec45d61235e5b5  /tmp/Workbench.bak     ← 一致
git diff --stat apps/shell/src/pages/Workbench.svelte      ← 空
```

`apps/shell/dist/` 是构建产物（不入库），现在是**未变异**的构建结果。

数据库侧：为了跑通曾多次 `DROP SCHEMA public CASCADE` + 重播夹具 —— 这只影响**运行时数据**
（`base_test`），不改任何文件。

> ⚠️ **不是我改的文件**：`crates/platform-audit/tests/audit_real_db.rs` 在 `git status` 里显示为 `M`
> （内容是「扫描器跳过 `tests/` 目录」的改动，mtime `21:02:50`），**不是本会话产生的**。

---

## 6. 六条完成情况（如实）

| # | AC | 状态 |
|---|---|---|
| 1 | AC-S-160（分组 / 排序 / hidden） | ✅ 完成（夹具子集断言 + 3 次变异验证） |
| 2 | D279（工作台不按 launch_url 过滤） | ✅ 完成（自造样本） |
| 3 | AC-S-163 步骤 3（external 整页跳） | ✅ 完成（自造样本 + 跨文档 pushState 计数） |
| 4 | AC-S-162 步骤 2（权限点过滤） | ✅ 完成 |
| 5 | AC-S-156 / AC-S-283（CRUD + 清空 launch_url） | ✅ 完成 |
| 6 | AC-S-156 守卫面（403 + 零请求） | ✅ 完成 |

**没有跳过、没有为了数字好看弱化断言。** 唯一相对派单原文的调整是 §4.4：
第 1 条从「组恰好 2 个」放宽为「有序 + 夹具子集顺序」，原因是共享库里别人的夹具会让
「恰好」类断言假红 —— 且已用变异验证证明放宽后**仍有区分力**。
