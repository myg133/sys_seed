/**
 * 沙箱模块前端路由表。
 *
 * 路由路径遵守 `api-conventions v1` C2：全小写连字符分隔，动作放路径末段。
 * 每个路由绑定一个权限点（`api-conventions v1` C4），前端 `can()` 与后端鉴权读同一份注册表。
 */

/** 一条模块路由。 */
export interface SandboxRoute {
  /** 路由路径。 */
  path: string;
  /** 菜单标题。 */
  label: string;
  /** 访问该路由需要的权限点。 */
  permission: string;
}

/** 沙箱模块的全部路由。 */
export const sandboxRoutes: SandboxRoute[] = [
  { path: '/sandbox', label: '沙箱', permission: 'sandbox:demo:read' },
];

/** 按路径查找路由。 */
export function findRoute(path: string): SandboxRoute | undefined {
  return sandboxRoutes.find((route) => route.path === path);
}
