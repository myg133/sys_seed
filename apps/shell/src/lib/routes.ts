/**
 * 路由表（REQ-011 的最小壳 + REQ-012 的**一条通配路由**）。
 *
 * # 受保护页的权限点
 *
 * `/admin/roles` 声明 `rbac:role:write` ——
 * 守卫不放行时，**它的 `load` 不会被调用** → `/api/rbac/roles` 的请求计数为 **0**。
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

import type { RouteDefinition } from './router';

/** 数据页面的通配路由（D104）。 */
export const SCHEMA_ROUTE_PATH = '/schema/:moduleId/:entityKey';

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
): readonly RouteDefinition[] {
  return [
    { path: '/', title: '工作台' },
    { path: '/login', title: '登录' },
    { path: '/403', title: '无权访问' },
    {
      // 一条通配路由服务**所有**实体（D104）：新增实体不改这里。
      path: SCHEMA_ROUTE_PATH,
      title: '数据页面',
      requiresAuth: true,
    },
    {
      path: '/admin/roles',
      title: '角色管理',
      permission: 'rbac:role:write',
      load: async () => {
        const roles = await client.get(PROTECTED_ROLES_PATH);
        onRoles?.(roles);
      },
    },
  ];
}
