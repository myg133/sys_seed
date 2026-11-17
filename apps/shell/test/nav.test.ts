import { describe, expect, it } from 'vitest';

import { NAV_ITEMS, filterNav } from '../src/lib/nav';

describe('shell 菜单', () => {
  it('关键字为空时返回全部菜单', () => {
    expect(filterNav(NAV_ITEMS, '   ')).toHaveLength(NAV_ITEMS.length);
  });

  it('按标签过滤', () => {
    const result = filterNav(NAV_ITEMS, '租户');
    expect(result.map((item) => item.key)).toEqual(['tenant']);
  });

  it('按菜单键过滤且忽略大小写', () => {
    const result = filterNav(NAV_ITEMS, 'RBAC');
    expect(result.map((item) => item.key)).toEqual(['rbac']);
  });

  it('无匹配时返回空数组', () => {
    expect(filterNav(NAV_ITEMS, '不存在的菜单')).toEqual([]);
  });
});
