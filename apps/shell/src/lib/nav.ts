/** 菜单项。 */
export interface NavItem {
  /** 菜单键。 */
  readonly key: string;
  /** 展示标签。 */
  readonly label: string;
  /** 关联路径。 */
  readonly path: string;
}

/** 默认菜单（本期为占位数据，后续由应用注册表下发）。 */
export const NAV_ITEMS: readonly NavItem[] = [
  { key: 'workbench', label: '工作台', path: '/workbench' },
  { key: 'iam', label: '账号与认证', path: '/api/iam' },
  { key: 'rbac', label: '角色与权限', path: '/api/rbac' },
  { key: 'tenant', label: '租户', path: '/api/tenant' },
];

/** 按关键字过滤菜单；关键字为空时返回全部。 */
export function filterNav(items: readonly NavItem[], keyword: string): NavItem[] {
  const query = keyword.trim().toLowerCase();
  if (!query) {
    return [...items];
  }
  return items.filter(
    (item) => item.label.toLowerCase().includes(query) || item.key.toLowerCase().includes(query),
  );
}
