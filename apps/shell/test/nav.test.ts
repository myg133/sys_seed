import { describe, expect, it } from 'vitest';

import { createI18n } from '@base/i18n';

import { NAV_ITEMS, filterNav, localizeNav } from '../src/lib/nav';

/** 默认语言（zh-CN）下的本地化菜单。 */
const items = localizeNav(NAV_ITEMS, (key) => createI18n().t(key));

describe('shell 菜单', () => {
  it('菜单项只存翻译键，不存文案（语言切换后能跟着变）', () => {
    for (const item of NAV_ITEMS) {
      expect(item.labelKey).toMatch(/^nav\.[a-z][A-Za-z0-9]*$/);
      expect(item).not.toHaveProperty('label');
    }
  });

  it('关键字为空时返回全部菜单', () => {
    expect(filterNav(items, '   ')).toHaveLength(NAV_ITEMS.length);
  });

  it('按标签过滤', () => {
    const result = filterNav(items, '租户');
    expect(result.map((item) => item.key)).toEqual(['tenant']);
  });

  it('按菜单键过滤且忽略大小写', () => {
    const result = filterNav(items, 'RBAC');
    expect(result.map((item) => item.key)).toEqual(['rbac']);
  });

  it('无匹配时返回空数组', () => {
    expect(filterNav(items, '不存在的菜单')).toEqual([]);
  });

  it('切换到 en-US 后标签变英文（同一份 NAV_ITEMS，不重新声明）', () => {
    const en = createI18n({ locale: 'en-US' });
    const english = localizeNav(NAV_ITEMS, (key) => en.t(key));
    expect(english.find((item) => item.key === 'workbench')?.label).toBe('Workbench');
    expect(filterNav(english, 'tenant').map((item) => item.key)).toEqual(['tenant']);
  });
});
