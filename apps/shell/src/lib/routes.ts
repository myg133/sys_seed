/**
 * 路由表（REQ-011 的最小壳 + REQ-012 的**一条通配路由** + REQ-014 的审计页
 * + REQ-016 的工作台 / 应用注册表）。
 *
 * # 受保护页的权限点
 *
 * `/admin/roles` 声明 `rbac:role:write` ——
 * 守卫不放行时，**它的 `load` 不会被调用** → `/api/rbac/roles` 的请求计数为 **0**。
 *
 * `/admin/audit` 声明 `audit:events:read`（契约 C4；REQ-014 / AC-S-132）——
 * 同样的机制：守卫不放行 → 地址改写成 `/403` → **路由的 `load` 不会被调用**
 * → `GET /api/audit/events` 的请求计数为 **0**。
 *
 * `/admin/apps` 声明 `app:registry:read`（REQ-016 / AC-S-156）—— 同一套机制。
 *
 * # 工作台是**两条**路由，不是一个页面的两个地址（REQ-016 / D278）
 *
 * | 路径 | 页面 | 为什么两条 |
 * |---|---|---|
 * | `/` | 首页（REQ-010 的品牌壳 + 菜单过滤，既有 E2E 依赖） | 保持 REQ-010 的语义不变 |
 * | `/workbench` | 应用工作台（REQ-016） | 新增页面，不改既有首页 |
 *
 * ⚠️ `/workbench` 在 REQ-016 之前**只出现在** `nav.ts` 的占位菜单里、**没有路由**
 * （S-03 实测）→ 本需求补上。`/` 的 `title` 仍是 `nav.workbench`（语言切换要跟着变），
 * `/workbench` 也是 —— 两者共用同一个翻译键是**有意的**：都是「工作台」这个词。
 *
 * # 下发的导航项**不进**路由表（D278 / AC-S-163）
 *
 * `routesFromNavigation(items)`（`lib/nav.ts`）产出的 `{ path, title }` 是**登记形态**，
 * 它**不**被拼进这里的路由表：`RouteDefinition` 没有 `component` 槽、
 * `createRouter` 捕获数组引用、`Router` 没有 `addRoute`、而 `App.svelte` 的页面渲染
 * 是逐路径硬编码 → 拼进去也只有「落到 `<Home/>`」一种结果，且 `external` 项会让
 * `resolve()` 拿外部 URL 去 `match()` → **静默失败**。
 * 第三方应用走**整页跳转**（工作台里的 `<a href>`），`router.ts` **零改动**。
 *
 * REQ-022 的四条 OAuth 路由：
 *
 * - `/oauth/authorize`、`/oauth/device`、`/oauth/logout` 是**公开路由**
 *   （D221）：浏览器顶层导航**不会**带 `Authorization` 头 → 守卫若要求登录，
 *   未登录时会被改写到登录页，而守卫记的 `intendedPath` **只有 pathname**
 *   → **授权参数 / `user_code` 全丢** → 页面**自己**处理未登录；
 * - `/oauth/grants` 声明 `oauth:grant:read` → 与 `/admin/audit` 同一套机制
 *   （守卫不放行 → `load` 不被调用 → `GET /api/oauth/grants` 请求计数为 **0**）。
 *
 * # 零代码的关键是通配路由（D104 / AC-S-085）
 *
 * `/schema/:moduleId/:entityKey` 是**唯一**的数据页面路由：新增实体只改后端 manifest，
 * `apps/shell/src/**` 一行不改。若把它写成每个实体一条路由，「零前端代码」就是假的。
 *
 * 它**不声明静态权限点**：实体级权限不在元数据里（本轮不加），而按参数拼权限点
 * （`<moduleId>:...`）会让权限点变成运行时字符串 —— 后端启动对账覆盖不到。
 * 因此这里只要求「已登录」，字段级可见性由后端过滤元数据与数据负责。
 */

import type { ApiClient } from '@base/sdk';

import { APP_REGISTRY_READ_PERMISSION, listApps, type AppView } from './app-api';
import { AUDIT_EVENTS_PERMISSION, loadAuditFirstPage, type AuditLoadResult } from './audit-api';
import {
  loadOAuthGrants,
  OAUTH_AUTHORIZE_ROUTE_PATH,
  OAUTH_DEVICE_ROUTE_PATH,
  OAUTH_GRANTS_PERMISSION,
  OAUTH_GRANTS_ROUTE_PATH,
  OAUTH_LOGOUT_ROUTE_PATH,
  type OAuthGrantsLoadResult,
} from './oauth-api';
import type { RouteDefinition } from './router';
import { SSO_CALLBACK_ROUTE_PATH } from './sso-api';

/** 数据页面的通配路由（D104）。 */
export const SCHEMA_ROUTE_PATH = '/schema/:moduleId/:entityKey';

/** 审计查询页的路由（REQ-014 / AC-S-132）。 */
export const AUDIT_ROUTE_PATH = '/admin/audit';

/**
 * 应用工作台的路由（REQ-016 / AC-S-160 / D278）。
 *
 * ⚠️ **不是** `/`：既有首页（REQ-010 的品牌壳 + 菜单过滤）保持原语义，
 * 工作台是**新增页面**（S-03 实测 `/workbench` 此前没有路由）。
 */
export const WORKBENCH_ROUTE_PATH = '/workbench';

/** 应用注册表管理页的路由（REQ-016 / AC-S-156；权限点 `app:registry:read`）。 */
export const APPS_ADMIN_ROUTE_PATH = '/admin/apps';

/** 应用注册表首屏的加载结果（路由 `load` 的产物 + 失败原因）。 */
export interface AppsLoadResult {
  /** 首屏的应用列表。 */
  readonly apps: readonly AppView[];
  /** 失败原因（`undefined` = 没失败）。 */
  readonly failure?: unknown;
}

/**
 * 取应用注册表首屏（路由 `load` 用）。
 *
 * **不抛错**：失败也返回结果（原因放 `failure`）—— 路由的 `load` 是被 `void`
 * 调用的，抛错会变成**未处理的 Promise 拒绝**（页面上看不到任何提示）。
 */
export async function loadAppsFirstPage(client: ApiClient): Promise<AppsLoadResult> {
  try {
    return { apps: await listApps(client) };
  } catch (failure) {
    return { apps: [], failure };
  }
}

/** 受保护页的数据接口。 */
export const PROTECTED_ROLES_PATH = '/api/rbac/roles';

/** 角色行。 */
export interface Role {
  readonly id: string;
  readonly name: string;
}

/**
 * 从接口返回值里取角色行。
 *
 * 后端可能直接返回数组，也可能返回 `{ roles: [...] }` —— 两种都认；
 * 取不到就是空数组（**不抛错**：列表页不该因为形状不对而白屏）。
 */
export function toRoles(loaded: unknown): Role[] {
  const list = Array.isArray(loaded)
    ? loaded
    : ((loaded as { roles?: unknown } | undefined)?.roles ?? []);
  if (!Array.isArray(list)) {
    return [];
  }
  return list as Role[];
}

/** 构造路由表。 */
export function createRoutes(
  client: ApiClient,
  onRoles?: (roles: unknown) => void,
  onAudit?: (result: AuditLoadResult) => void,
  onOAuthGrants?: (result: OAuthGrantsLoadResult) => void,
  /**
   * 应用注册表首屏（REQ-016 / AC-S-156）。
   *
   * ⚠️ 第 5 个参数是**追加**的（D270 / 风险 R4）：前 4 个既有参数与返回类型
   * 一字不改 → REQ-011/013/014/022 的既有用例不受影响。
   * 失败也回调（`failure` 里带原因），**不抛错** —— 路由的 `load` 是被 `void`
   * 调用的（`router.ts:248`），抛错会变成未处理的 Promise 拒绝。
   */
  onApps?: (result: AppsLoadResult) => void,
): readonly RouteDefinition[] {
  // `title` 存**翻译键**而不是文案：路由表在模块加载时构造，
  // 存文案会把它钉死在加载时的语言上（页面标题由渲染层用 `t()` 取）。
  return [
    { path: '/', title: 'nav.workbench' },
    { path: '/login', title: 'common.login' },
    {
      // P4：OIDC 回调的浏览器交接页。**公开路由** ——
      // 用户此刻刚被 IdP 302 回来、还没有令牌；要求登录态会让守卫先把 ticket 丢掉。
      path: SSO_CALLBACK_ROUTE_PATH,
      title: 'common.ssoCallback',
    },
    {
      // REQ-022：授权页 / 同意页。**公开路由**（D221）——
      // 平台的会话是 Bearer + sessionStorage，浏览器顶层导航**不会**带
      // `Authorization` 头 → 守卫若要求登录，未登录时会被改写到登录页，
      // 而守卫记的 `intendedPath` **只有 pathname**（`router.ts:261`）
      // → **授权参数与 query 全丢**。
      // → 页面**自己**处理未登录（渲染「登录并继续」）。
      path: OAUTH_AUTHORIZE_ROUTE_PATH,
      title: 'oauth.nav.authorize',
    },
    {
      // REQ-022：设备码批准页。**公开路由**（D221）——
      // query 里带 `user_code`（`verification_uri_complete`）→
      // 任何「重定向丢 query」的路径都会让设备码流程不可用。
      path: OAUTH_DEVICE_ROUTE_PATH,
      title: 'oauth.nav.device',
    },
    {
      // REQ-022：登出页。**公开路由** —— 登出由前端 `fetch` 发起
      // （会话是 Bearer，导航带不上 `Authorization` 头，D225）。
      path: OAUTH_LOGOUT_ROUTE_PATH,
      title: 'oauth.nav.logout',
    },
    { path: '/403', title: 'nav.forbidden' },
    {
      // 应用工作台（REQ-016 / AC-S-160）：**登录即可**（不声明权限点）。
      //
      // 数据由页面自己取（工作台要四个端点：workspace / recent / frequent /
      // navigation）—— 首屏走 `load` 只会拿到其中一个，反而多一条「部分数据」
      // 的中间态。导航项由 `App.svelte` 在登录后取好、以 prop 传进来（一次请求）。
      path: WORKBENCH_ROUTE_PATH,
      title: 'nav.workbench',
      requiresAuth: true,
    },
    {
      // 应用注册表管理页（REQ-016 / AC-S-156）：权限点 `app:registry:read`。
      //
      // 与 `/admin/audit` 同一套机制：守卫不放行 → **`load` 不会被调用**
      // （`router.ts:244-250`）→ `GET /api/apps` 的请求计数为 **0**。
      path: APPS_ADMIN_ROUTE_PATH,
      title: 'nav.appRegistry',
      permission: APP_REGISTRY_READ_PERMISSION,
      load: async () => {
        // ⚠️ **先取数据、再回调**（`onApps?.(await …)` 会在没有回调时把整个
        // 表达式短路掉 → 请求根本不会发，既有坑见 `/admin/audit`）。
        const result = await loadAppsFirstPage(client);
        onApps?.(result);
      },
    },
    {
      // 一条通配路由服务**所有**实体（D104）：新增实体不改这里。
      path: SCHEMA_ROUTE_PATH,
      title: 'nav.schemaPage',
      requiresAuth: true,
    },
    {
      path: '/admin/roles',
      title: 'nav.roles',
      permission: 'rbac:role:write',
      load: async () => {
        const roles = await client.get(PROTECTED_ROLES_PATH);
        onRoles?.(roles);
      },
    },
    {
      // 审计查询页（REQ-014 / AC-S-132）：权限点 `audit:events:read`。
      //
      // 首屏数据在**路由的 `load`** 里取（与 `/admin/roles` 同一套机制）：
      // 守卫不放行 → **`load` 不会被调用**（`router.ts:244-250`）
      // → `GET /api/audit/events` 的请求计数为 **0**。
      //
      // 翻页 / 过滤由页面自己再发请求（服务端分页：每页一次新的请求）。
      path: AUDIT_ROUTE_PATH,
      title: 'nav.audit',
      permission: AUDIT_EVENTS_PERMISSION,
      load: async () => {
        // ⚠️ **先取数据、再回调**：`onAudit?.(await …)` 会在没有回调时
        // 把整个表达式短路掉 → **请求根本不会发**（数据永远不来）。
        const result = await loadAuditFirstPage(client);
        onAudit?.(result);
      },
    },
    {
      // REQ-022：「我的授权」页。权限点 `oauth:grant:read`（契约 C4）。
      //
      // 首屏数据在**路由的 `load`** 里取（与 `/admin/audit` 同一套机制）：
      // 守卫不放行 → **`load` 不会被调用**（`router.ts:244-250`）
      // → `GET /api/oauth/grants` 的请求计数为 **0**。
      //
      // 撤销由页面自己发请求（撤销后**刷新列表**）。
      path: OAUTH_GRANTS_ROUTE_PATH,
      title: 'oauth.nav.grants',
      permission: OAUTH_GRANTS_PERMISSION,
      load: async () => {
        // ⚠️ 同样**先取数据、再回调**（短路坑见上）。
        const result = await loadOAuthGrants(client);
        onOAuthGrants?.(result);
      },
    },
  ];
}
