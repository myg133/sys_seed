/**
 * 壳层菜单（`nav.*` 文案由 i18n 渲染 —— 组件与模块里不出现菜单文案字面量）。
 *
 * `labelKey` 是翻译键而不是文案：语言切换后菜单要跟着变，
 * 把文案在模块加载时翻一次会留在旧语言里。
 */

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
