/**
 * 路由表（REQ-011 的最小壳 + REQ-012 的**一条通配路由** + REQ-014 的审计页）。
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
