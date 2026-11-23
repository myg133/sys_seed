/**
 * 请假模块的前端路由表（REQ-020 / AC-S-192 · D356）。
 *
 * # 为什么模块自己声明路径
 * 业务页面在 `modules/leave/frontend/`，路由路径与权限点也归模块所有；
 * `apps/shell/src/lib/routes.ts` 只做**装配**（import 这个常量 + 指向页面组件），
 * 不复制一份路径字面量 —— 否则改路径要改两处，且容易漂移。
 *
 * # 与 `/schema/:moduleId/:entityKey` 的关系
 * 通配页没有「审批」这类行内动作（`platform-core/src/extension/schema.rs` 的
 * `api` 只有 4 个键）→ 带审批流的业务必须有自己的路由（TD-087）。
 */

import { LEAVE_PERMISSION_READ } from './lib/leave-api';

/** 请假页的 SPA 路径。 */
export const LEAVE_ROUTE_PATH = '/leave';

/** 访问请假页需要的权限点（路由守卫读它；与后端同一份注册表）。 */
export const LEAVE_ROUTE_PERMISSION = LEAVE_PERMISSION_READ;

/** 一条模块前端路由。 */
export interface LeaveRoute {
  /** SPA 路径。 */
  readonly path: string;
  /** 菜单 / 页面标题（中文文案 —— 模块没有 i18n 命名空间，见页面头的说明）。 */
  readonly label: string;
  /** 访问该路径需要的权限点（与后端 `require_permission` 同一份注册表）。 */
  readonly permission: string;
}

/** 模块的全部前端路由（当前只有请假列表 / 审批页）。 */
export const leaveRoutes: readonly LeaveRoute[] = [
  { path: LEAVE_ROUTE_PATH, label: '请假', permission: LEAVE_ROUTE_PERMISSION },
];
