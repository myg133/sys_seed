import { describe, expect, it } from 'vitest';

import { describeField, findField } from '../src/index';

describe('@base/schema', () => {
  it('必填字段描述带标记', () => {
    expect(describeField({ key: 'name', label: '名称', type: 'text', required: true })).toBe(
      '名称（必填）',
    );
  });

  it('选填字段描述只含标签', () => {
    expect(describeField({ key: 'name', label: '名称', type: 'text' })).toBe('名称');
  });

  it('按字段名查找元数据', () => {
    const fields = [
      { key: 'name', label: '名称', type: 'text' },
      { key: 'amount', label: '金额', type: 'number' },
    ] as const;
    expect(findField(fields, 'amount')?.label).toBe('金额');
    expect(findField(fields, 'missing')).toBeUndefined();
  });
});
