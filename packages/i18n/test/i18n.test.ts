import { describe, expect, it } from 'vitest';

import { hasMessage, translate } from '../src/index';

const DICT = {
  INSUFFICIENT_PERMISSION: '当前角色缺少所需权限',
  INVALID_CONFIG_KEY: '配置键不合法',
};

describe('@base/i18n', () => {
  it('命中时返回中文文案', () => {
    expect(translate(DICT, 'INSUFFICIENT_PERMISSION')).toBe('当前角色缺少所需权限');
  });

  it('未命中时回落到默认文案', () => {
    expect(translate(DICT, 'UNKNOWN_CODE')).toBe('UNKNOWN_CODE');
    expect(translate(DICT, 'UNKNOWN_CODE', '未知错误')).toBe('未知错误');
  });

  it('判断文案是否存在', () => {
    expect(hasMessage(DICT, 'INVALID_CONFIG_KEY')).toBe(true);
    expect(hasMessage(DICT, 'UNKNOWN_CODE')).toBe(false);
  });
});
