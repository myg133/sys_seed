/**
 * 路由表（本期只做**最小壳**：登录页 + 极简路由 + 一个受保护页 + 403 页）。
 *
 * # 受保护页的权限点
 *
 * `/admin/roles` 声明 `rbac:role:write` ——
 * 守卫不放行时，**它的 `load` 不会被调用** → `/api/rbac/roles` 的请求计数为 **0**。
 */

import type { ApiClient } from '@base/sdk';

import type { RouteDefinition } from './router';

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
