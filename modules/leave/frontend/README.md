# 请假模块前端（REQ-020 / AC-S-192）

| 文件 | 作用 |
|---|---|
| `lib/leave-api.ts` | 4 个端点的封装 + 响应解析 + 建单本地校验 |
| `pages/LeavePage.svelte` | 列表 + 新建表单 + 审批（通过 / 驳回） |
| `routes.ts` | SPA 路径与权限点（`apps/shell` 装配时 import 它） |

## 为什么页面在这里，而不是 `apps/shell/src/pages/`

方案 A（页面放 `apps/shell/src/pages/Leave.svelte`）import 更简单，但会让**业务 UI
逃出 AC-S-187 的前端代码量口径**（口径是 `modules/leave/frontend`），也会让
「业务代码集中在独立目录」（AC-029）名不副实。

本模块选**方案 B**：页面与接口封装留在模块目录，`apps/shell` 只加**一条**硬编码路由
（`apps/shell/src/lib/routes.ts`）与**一个**渲染分支（`App.svelte`）——
登记为 C 类改动（D353 / D356）。

⚠️ 代价：`apps/shell/tsconfig.json` 的 `include` 要显式加上本目录
（svelte-check 只检查 `include` 里的文件）。这是方案 B 唯一需要改的宿主配置。

## 红线

审批**只**走 `POST /api/leave/requests/{id}/decide`。页面与接口层里**没有**
任何写 `status` 字段的路径 —— 那会绕过状态机与 409 `LEAVE_INVALID_TRANSITION`
校验（D356）。

## 已知限制（底座缺口）

- 模块页文案**硬编码中文**：`packages/i18n` 的 domain 是闭集，没有模块级命名空间；
- 列表只带 `limit`（后端没有统一的游标编解码器，见批次一进度第 5 条）。
