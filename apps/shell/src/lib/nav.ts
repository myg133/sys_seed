/**
 * 壳层菜单（`nav.*` 文案由 i18n 渲染 —— 组件与模块里不出现菜单文案字面量）。
 *
 * `labelKey` 是翻译键而不是文案：语言切换后菜单要跟着变，
 * 把文案在模块加载时翻一次会留在旧语言里。
 *
 * # `NAV_ITEMS` 现在是**兜底**（REQ-016 / AC-S-162）
 *
 * 导航的正源是后端的 `GET /api/apps/navigation`（manifest 的 `menus[]` +
 * `platform_app` 合并下发）。`NAV_ITEMS` 保留下来是为了**接口不可用时仍有个入口**
 * —— 删掉它会让既有用例（`nav.test.ts`）与「后端没上线」的形态都变成空白菜单。
 *
 * # `routesFromNavigation`：只把 `internal` 项变成 SPA 路由（D278 / AC-S-163）
 *
 * 第三方应用是**独立站点**（`launch_url`），**没有本 SPA 的组件**
 * → 天生不该进 SPA 路由表。它们由工作台渲染成**整页跳转**的链接。
 */

import type { NavigationItem } from './app-api';
import type { RouteDefinition } from './router';

/** 菜单项（`labelKey` 由消费方用 `t()` 渲染）。 */
export interface NavItem {
  /** 菜单键。 */
  readonly key: string;
  /** 翻译键（`nav.*`）。 */
  readonly labelKey: string;
  /** 关联路径。 */
  readonly path: string;
}

/** 带已渲染标签的菜单项。 */
export interface LocalizedNavItem extends NavItem {
  /** 当前语言的展示标签。 */
  readonly label: string;
}

/** 默认菜单（本期为占位数据，后续由应用注册表下发）。 */
export const NAV_ITEMS: readonly NavItem[] = [
  { key: 'workbench', labelKey: 'nav.workbench', path: '/workbench' },
  { key: 'iam', labelKey: 'nav.iam', path: '/api/iam' },
  { key: 'rbac', labelKey: 'nav.rbac', path: '/api/rbac' },
  { key: 'tenant', labelKey: 'nav.tenant', path: '/api/tenant' },
];

/** 用翻译函数把菜单项渲染成当前语言的标签。 */
export function localizeNav(
  items: readonly NavItem[],
  t: (key: string) => string,
): LocalizedNavItem[] {
  return items.map((item) => ({ ...item, label: t(item.labelKey) }));
}

/**
 * 从下发的导航项里挑出**站内**项，转成 SPA 路由定义（AC-S-163 步骤 1 / 2）。
 *
 * # 三条判据（D278）
 *
 * 1. `kind === 'internal'` 的项 → `{ path: target, title: label }`；
 * 2. `kind === 'external'` 的项 → **0 条**（进 SPA 路由表会让 `resolve()`
 *    拿外部 URL 去 `match()` → **静默失败**）；
 * 3. 纯函数、无副作用 → 可单测。
 *
 * ⚠️ `title` 用 **`label` 的文案**，不是翻译键（D284 / S-09：`MenuSpec` 里
 * 没有 `labelKey`）。代价是模块菜单不随语言切换（**TD-049**）。
 *
 * ⚠️ **不产出 `load` / `permission`**：导航项只有 `target` + `label` + `requiredPermission`，
 * 而 `RouteDefinition` 的 `permission` 决定守卫 —— 但 `App.svelte` 的页面渲染是
 * 逐路径硬编码（10 个 `if/else`），这些路由**没有对应的页面组件**。
 * 所以本函数的产物是「路由登记」的**可见形态**，点击落页面**明确不做**
 * （AC-S-163.b / TD-048）。真正可点的是工作台里的 `external` 入口。
 */
export function routesFromNavigation(
  items: readonly NavigationItem[],
): readonly RouteDefinition[] {
  return items
    .filter((item) => item.kind === 'internal')
    .map((item) => ({ path: item.target, title: item.label }));
}

/** 按关键字过滤菜单；关键字为空时返回全部。 */
export function filterNav(items: readonly LocalizedNavItem[], keyword: string): LocalizedNavItem[] {
  const query = keyword.trim().toLowerCase();
  if (!query) {
    return [...items];
  }
  return items.filter(
    (item) => item.label.toLowerCase().includes(query) || item.key.toLowerCase().includes(query),
  );
}
