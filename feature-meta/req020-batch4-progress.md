# REQ-020 第四批 进度

状态：**完成**（4 项均落地；2 个判据如实判不成立 / 无法判定，见末尾）

## ① 前端页面 —— 完成

`modules/leave/frontend/`：

| 文件 | 行数 | 作用 |
|---|---:|---|
| `lib/leave-api.ts` | 219 | 4 端点封装 + 解析 + 建单本地校验 |
| `pages/LeavePage.svelte` | 350 | 列表 + 新建表单 + 审批（通过/驳回） |
| `routes.ts` | 35 | SPA 路径 + 权限点（shell import 它） |

- 审批**只**走 `POST .../decide`：接口层没有改 `status` 的函数，页面表单没有 status 字段；
  单测直接断言「请求路径 = `/decide`、body = `{decision}`」。
- 复用：`@base/ui`（buttonClass / inputClass / tableClass）、`@base/auth` 的 `can()`。
- **没复用** shell 的 Button/Input/Table 组件：业务模块 import 宿主内部组件会让依赖反向。
- **没复用** `@base/schema`：它吃 manifest 的 `EntityMeta`（api 只有 4 键），
  为请假单伪造一份只是把「没有审批动作」藏起来。
- 文案硬编码中文：`packages/i18n` 的 domain 是闭集，**无模块级命名空间**（记缺口 6）。

## ② shell 路由 —— 完成（**方案 B**）

- `apps/shell/src/lib/routes.ts`：import 模块的 `LEAVE_ROUTE_PATH` / `LEAVE_ROUTE_PERMISSION`，
  加一条 `permission: 'leave:request:read'` 的路由（不挂 `load`，列表由页面挂载时取）。
- `apps/shell/src/App.svelte`：`import LeavePage from '../../../modules/leave/frontend/pages/LeavePage.svelte'`
  + 一个 `resolution.allowed` 渲染分支。
- `apps/shell/tsconfig.json`：`include` 加 `../../modules/leave/frontend/**`（svelte-check 只查 include）。

**为什么选 B**：页面若放 `apps/shell/src/pages/`，业务 UI 就逃出了 AC-S-187 的前端代码量口径
（`modules/leave/frontend`），AC-029「业务代码集中在独立目录」也名不副实。
**代价**：宿主 tsconfig 多 2 行（记缺口 7）。方案 B 实测可行：`svelte-check` 0 error、`vite build` 通过、
E2E 真跑通 —— sandbox 的孤儿页面只是「没人装配」，不是「跨目录引用不可行」。

## ③ E2E —— 完成

`e2e/leave.spec.ts`（1 个用例，4 个 `test.step`）：

```console
$ npx playwright test e2e/leave.spec.ts --reporter=json
{"expected":1,"skipped":0,"unexpected":0,"flaky":0}
$ npx playwright test e2e/leave.spec.ts --reporter=list
  ✓ 请假：列表 → 建单 → 审批通过 → 刷新保持（AC-S-192） (3.8s)
  1 passed (12.4s)
```

- **文件内 skipped = 0**；没有任何 `test.skip` / 条件跳过（D108：403/500 让它红）。
- 步骤 3 是**切账号**：`sessionStorage.clear()` + 整页重载 + 用审批人登录。
- 步骤 4 用 `page.reload()` 后再断言（不是前端内存态）。

### 夹具（`scripts/seed-e2e.sh`）

新增两个账号（权限点**分开**，审批人不能建单、申请人不能审批）：

| 账号 | 权限点 |
|---|---|
| `u-leave-applicant@example.com` | `leave:request:read` + `leave:request:write` |
| `u-leave-approver@example.com` | `leave:request:read` + `leave:request:approve` |

强断言同步更新：用户数 `9 1×10`、授权数 `4 5 0 1 3 5 1 3 2 2`。

## ④ 度量报告 —— 完成

`BA/demands/REQ-020-platform-consumption/metrics-report.md`（每个数字都有命令 + 实跑输出）。

核心数字：后端 1374 + 前端 604 = **1978 行**（>500，**超 1478**）；
平台改动 30 个文件（`git diff` 23 + 未跟踪 7），A 类 **0**；依赖面 3 个 `platform-*`；
`check-modules` / `check-deps` / `capabilities --check` 退出码 **0/0/0**；
已改 6 个 crate 测试 **684 passed / 0 failed**；shell 单测 **422 passed**；E2E skipped **0**。

## 与预想不符（逐条）

1. **AC-S-187 结构性不成立**（与 D357 预判一致）：剔注释 / 空行 / 内联测试后仍 1241 行。
2. **AC-S-190 的 `BASE_UPGRADE` 不存在**：批次 1~4 的改动全部未提交 → 升级零改动**当前无法判定**。
   报告里已写清；需要先提交消费方（形成 BASE_UPGRADE）再提交升级改动。
3. **AC-S-191 给的 skipped `jq` 命令是语法错误**（jq 1.8.2：`?//` 被当成解构备选运算符）。
   等价命令 `jq '[.. | .skipped? // empty | select(. != 0)] | length'` 实测 `0`。
4. **`scripts/seed-e2e.sh` 有两处既存 bug，阻塞 E2E**（已修，报告「顺带修复」一节）：
   - REQ-018 存储 job 的 UUID 最后一段只有 10 位 → PostgreSQL 拒绝 → 脚本 exit 1 → webServer 起不来；
   - `platform_storage_upload` 无冲突键 → 每次重跑累积 → 「恰好 10 行」断言第二次必红。
   修法：补足 12 位 + 夹具块开头按 `e2e/` 键清理。改后连跑两次 exit 0。
5. **design-summary §3 的登记清单滞后于实际**：实际多出 7 类文件（见报告「差异」表）——
   需 BA 补登记（批次 3 已提示过 `platform-rbac/src/service.rs`）。
6. `pnpm install --frozen-lockfile` **不需要新依赖**（前端只用现有 workspace 包）。
7. `apps/shell/src/App.svelte` 有一处**既存** prettier 问题（`navigation={navigation}` →
   `{navigation}`，第 323 行，非本次改动），未顺手动它。

## 判据结论

| AC | 结论 |
|---|---|
| AC-S-186（A 类 = 0，逐项登记） | 成立（A=0；登记表需 BA 更新） |
| AC-S-187（< 500 行） | **不成立**（1978 行，未改口径） |
| AC-S-190（升级零改动） | **无法判定**（BASE_UPGRADE 未形成） |
| AC-S-191（可复算） | 成立（附 AC 命令瑕疵说明） |
| AC-S-192（E2E，skipped=0） | 成立 |
| AC-S-193（依赖面） | 成立 |
