import { describe, expect, it } from 'vitest';

import {
  buildListQuery,
  describeField,
  findField,
  formatCell,
  formValues,
  isKnownFieldType,
  parseEntityMeta,
  parseListResponse,
  prepareSubmit,
  recordId,
  resolveApiPath,
  unknownTypeWarning,
  validateForm,
  visibleFields,
  type EntityMeta,
  type FieldMeta,
} from '../src/index';

/** 与 design §2.1 一致的 7 字段元数据（U2 全字段可见）。 */
const META: EntityMeta = {
  module: 'sandbox',
  entity: 'demoItem',
  label: '示例条目',
  api: {
    list: '/api/sandbox/demo-items',
    create: '/api/sandbox/demo-items',
    update: '/api/sandbox/demo-items/{id}',
    delete: '/api/sandbox/demo-items/{id}',
  },
  fields: [
    { key: 'id', label: 'ID', type: 'text', readonly: true },
    { key: 'title', label: '标题', type: 'text', required: true, maxLength: 200 },
    { key: 'amount', label: '金额', type: 'number', min: 0 },
    { key: 'paid', label: '已付款', type: 'boolean' },
    { key: 'dueAt', label: '到期日', type: 'date' },
    {
      key: 'ownerId',
      label: '负责人',
      type: 'reference',
      reference: { module: 'org', entity: 'user' },
    },
    { key: 'secret', label: '内部备注', type: 'text', permission: 'sandbox:demo:secret' },
  ],
};

/** 只保留 label 的便捷取值。 */
function labels(meta: EntityMeta): string[] {
  return visibleFields(meta).map((field) => field.label);
}

describe('@base/schema 的既有契约（REQ-011）', () => {
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

describe('visibleFields —— 忠实渲染后端给的字段（AC-S-082 / 084）', () => {
  it('字段顺序与 label 精确等于元数据声明顺序', () => {
    expect(labels(META)).toEqual(['ID', '标题', '金额', '已付款', '到期日', '负责人', '内部备注']);
  });

  it('后端已过滤掉的无权字段不会出现（渲染器不自造字段）', () => {
    const filtered: EntityMeta = {
      ...META,
      fields: META.fields.filter((field) => field.key !== 'secret'),
    };
    expect(labels(filtered)).toEqual(['ID', '标题', '金额', '已付款', '到期日', '负责人']);
  });

  it('缺 label 时回退为 key（结构容错，不丢整列）', () => {
    const broken = {
      ...META,
      fields: [{ key: 'title', type: 'text' } as unknown as FieldMeta],
    };
    expect(visibleFields(broken).map((field) => field.label)).toEqual(['title']);
  });
});

describe('formValues —— 只留元数据声明的字段', () => {
  it('缺值：boolean → false，其余 → 空串', () => {
    expect(formValues(META)).toEqual({
      id: '',
      title: '',
      amount: '',
      paid: false,
      dueAt: '',
      ownerId: '',
      secret: '',
    });
  });

  it('数据里多出来的键被忽略（AC-S-087 的元数据删字段场景）', () => {
    const values = formValues(META, {
      id: 'item-1',
      title: '示例',
      amount: 12.5,
      paid: true,
      dueAt: '2026-11-19',
      ownerId: 'u-1001',
      secret: '内部-A',
      legacy: '不该出现',
    });
    expect(Object.keys(values).sort()).toEqual(
      ['amount', 'dueAt', 'id', 'ownerId', 'paid', 'secret', 'title'].sort(),
    );
    expect(values['legacy']).toBeUndefined();
  });

  it('空值（null）也归一为空串 / false', () => {
    expect(formValues(META, { title: null, paid: null })['title']).toBe('');
    expect(formValues(META, { title: null, paid: null })['paid']).toBe(false);
  });
});

describe('validateForm —— 错误只落在该字段（AC-S-083 / TC-012-043）', () => {
  it('number 字段填非数字 → 只报该字段', () => {
    expect(validateForm(META, { title: 'x', amount: 'abc', dueAt: '2026-11-19' })).toEqual({
      amount: '金额必须是数字',
    });
  });

  it('合法值 → 空映射', () => {
    expect(validateForm(META, { title: 'x', amount: '12.5', dueAt: '2026-11-19' })).toEqual({});
  });

  it('必填留空 → 该字段的「不能为空」', () => {
    expect(validateForm(META, { title: '', amount: '1', dueAt: '2026-11-19' })).toEqual({
      title: '标题不能为空',
    });
  });

  it('min / max 按声明校验', () => {
    expect(validateForm(META, { title: 'x', amount: '-1' })).toEqual({ amount: '金额不能小于0' });
    const withMax: EntityMeta = {
      ...META,
      fields: META.fields.map((field) => (field.key === 'amount' ? { ...field, max: 10 } : field)),
    };
    expect(validateForm(withMax, { title: 'x', amount: '11' })).toEqual({
      amount: '金额不能大于10',
    });
  });

  it('minLength / pattern 按声明校验', () => {
    const constrained: EntityMeta = {
      ...META,
      fields: [{ key: 'code', label: '编码', type: 'text', minLength: 3, pattern: '^[A-Z]+$' }],
    };
    expect(validateForm(constrained, { code: 'ab' })).toEqual({ code: '编码不能少于3个字符' });
    expect(validateForm(constrained, { code: 'abc' })).toEqual({ code: '编码格式不正确' });
    expect(validateForm(constrained, { code: 'ABC' })).toEqual({});
  });

  it('readonly 字段不参与校验（id 留空也不报错）', () => {
    expect(validateForm(META, { title: 'x', amount: '1', id: '' })).toEqual({});
  });

  it('非法的 pattern 不拦用户（元数据问题不该让表单永远提交不了）', () => {
    const broken: EntityMeta = {
      ...META,
      fields: [{ key: 'code', label: '编码', type: 'text', pattern: '(' }],
    };
    expect(validateForm(broken, { code: 'anything' })).toEqual({});
  });
});

describe('prepareSubmit —— 校验失败不给 body（AC-S-083 步骤 3 / 6）', () => {
  it('校验失败：errors 非空且 body 为 undefined（调用方据此不发请求）', () => {
    const prepared = prepareSubmit(META, { title: '', amount: 'x' });
    expect(prepared.errors).toEqual({ title: '标题不能为空', amount: '金额必须是数字' });
    expect(prepared.body).toBeUndefined();
  });

  it('空的 date / reference 不下发空串（后端解析日期会 400）', () => {
    const prepared = prepareSubmit(META, {
      title: 'x',
      amount: '',
      paid: false,
      dueAt: '',
      ownerId: '',
      secret: '',
    });
    expect(prepared.errors).toEqual({});
    expect(prepared.body).toEqual({ title: 'x', paid: false, secret: '' });
  });

  it('合法提交：body 只含元数据声明的可写字段，不含 id / 内部状态', () => {
    const prepared = prepareSubmit(META, {
      id: 'item-1',
      title: '示例条目',
      amount: '12.5',
      paid: true,
      dueAt: '2026-11-19',
      ownerId: 'u-1001',
      secret: '内部-A',
      $internal: '不该出现',
    });
    expect(prepared.errors).toEqual({});
    expect(prepared.body).toEqual({
      title: '示例条目',
      amount: 12.5,
      paid: true,
      dueAt: '2026-11-19',
      ownerId: 'u-1001',
      secret: '内部-A',
    });
    expect(Object.keys(prepared.body ?? {}).sort()).toEqual(
      ['amount', 'dueAt', 'ownerId', 'paid', 'secret', 'title'].sort(),
    );
  });
});

describe('formatCell —— 按类型格式化（AC-S-082 步骤 3 / TC-012-037）', () => {
  const booleanField: FieldMeta = { key: 'paid', label: '已付款', type: 'boolean' };
  const dateField: FieldMeta = { key: 'dueAt', label: '到期日', type: 'date' };
  const numberField: FieldMeta = { key: 'amount', label: '金额', type: 'number' };
  const textField: FieldMeta = { key: 'title', label: '标题', type: 'text' };
  const referenceField: FieldMeta = { key: 'ownerId', label: '负责人', type: 'reference' };

  it('boolean → 是 / 否', () => {
    expect(formatCell(booleanField, true)).toEqual({ text: '是', unknownType: false });
    expect(formatCell(booleanField, false)).toEqual({ text: '否', unknownType: false });
  });

  it('date → YYYY-MM-DD，不做时区换算', () => {
    expect(formatCell(dateField, '2026-11-19')).toEqual({ text: '2026-11-19', unknownType: false });
    // 时间戳只取日期部分（若换算时区，23:00Z 会变成 2026-11-20）。
    expect(formatCell(dateField, '2026-11-19T23:00:00Z')).toEqual({
      text: '2026-11-19',
      unknownType: false,
    });
  });

  it('number → 十进制文本', () => {
    expect(formatCell(numberField, 12.5)).toEqual({ text: '12.5', unknownType: false });
    expect(formatCell(numberField, 0)).toEqual({ text: '0', unknownType: false });
  });

  it('text / reference → 原值文本（reference 是 id 原值）', () => {
    expect(formatCell(textField, '示例')).toEqual({ text: '示例', unknownType: false });
    expect(formatCell(referenceField, 'u-1001')).toEqual({ text: 'u-1001', unknownType: false });
  });

  it('空值 → 空串（不是 undefined / null 字样）', () => {
    expect(formatCell(textField, null)).toEqual({ text: '', unknownType: false });
    expect(formatCell(textField, undefined)).toEqual({ text: '', unknownType: false });
    expect(formatCell(booleanField, null)).toEqual({ text: '', unknownType: false });
  });

  it('未知类型 → 原值文本 + unknownType: true（不抛错）', () => {
    const currency: FieldMeta = { key: 'amount', label: '金额', type: 'currency' };
    expect(isKnownFieldType('currency')).toBe(false);
    expect(formatCell(currency, 12.5)).toEqual({ text: '12.5', unknownType: true });
    expect(formatCell(currency, null)).toEqual({ text: '', unknownType: true });
  });

  it('未知类型的告警文案点名字段与类型', () => {
    const warning = unknownTypeWarning({ key: 'amount', label: '金额', type: 'currency' });
    expect(warning).toContain('currency');
    expect(warning).toContain('amount');
  });
});

describe('buildListQuery —— 游标分页参数（TC-012-038）', () => {
  it('首屏不带 cursor 键，limit 缺省 20', () => {
    expect(buildListQuery(undefined, 20)).toEqual({ limit: '20' });
  });

  it('带游标：键顺序 cursor 在前、limit 在后', () => {
    const query = buildListQuery('c2VxOjIw', 20);
    expect(query).toEqual({ cursor: 'c2VxOjIw', limit: '20' });
    expect(Object.keys(query)).toEqual(['cursor', 'limit']);
  });

  it('limit 缺省仍下发 20', () => {
    expect(buildListQuery('c2VxOjIw', undefined)).toEqual({ cursor: 'c2VxOjIw', limit: '20' });
    expect(buildListQuery(null)).toEqual({ limit: '20' });
  });
});

describe('recordId / resolveApiPath', () => {
  it('取记录 id', () => {
    expect(recordId({ id: 'item-1' })).toBe('item-1');
    expect(recordId({ id: 42 })).toBe('42');
    expect(recordId({})).toBe('');
  });

  it('替换 {id} 占位符', () => {
    expect(resolveApiPath('/api/sandbox/demo-items/{id}', 'item-1')).toBe(
      '/api/sandbox/demo-items/item-1',
    );
    expect(resolveApiPath('/api/sandbox/demo-items', 'item-1')).toBe('/api/sandbox/demo-items');
  });
});

describe('parseEntityMeta —— 元数据响应容错（AC-S-087）', () => {
  it('合法响应保留字段顺序与可选键', () => {
    const parsed = parseEntityMeta({
      module: 'sandbox',
      entity: 'demoItem',
      label: '示例条目',
      api: { list: '/l', create: '/c', update: '/u/{id}', delete: '/d/{id}' },
      fields: [{ key: 'title', label: '标题', type: 'text' }],
    });
    expect(parsed?.fields).toEqual([{ key: 'title', label: '标题', type: 'text' }]);
    expect(parsed?.api).toEqual({ list: '/l', create: '/c', update: '/u/{id}', delete: '/d/{id}' });
  });

  it('未声明的可选接口键被省略（不是 undefined 值）', () => {
    const parsed = parseEntityMeta({
      module: 'm',
      entity: 'e',
      label: 'L',
      api: { list: '/l', create: '/c' },
      fields: [],
    });
    expect(parsed?.api).toEqual({ list: '/l', create: '/c' });
    expect(Object.keys(parsed?.api ?? {})).toEqual(['list', 'create']);
  });

  it('缺 api 声明 → undefined（渲染器不知道该向哪取数）', () => {
    expect(parseEntityMeta({ module: 'm', entity: 'e', label: 'L', fields: [] })).toBeUndefined();
    expect(parseEntityMeta({ api: { list: '/l' } })).toBeUndefined();
  });
});

describe('parseListResponse —— 游标分页响应（AC-S-082 步骤 4）', () => {
  it('标准形状：items + nextCursor', () => {
    expect(parseListResponse({ items: [{ id: 'item-1' }], nextCursor: 'c2VxOjIw' })).toEqual({
      items: [{ id: 'item-1' }],
      nextCursor: 'c2VxOjIw',
    });
  });

  it('nextCursor 为 null → null（没有下一页）', () => {
    expect(parseListResponse({ items: [], nextCursor: null })).toEqual({
      items: [],
      nextCursor: null,
    });
  });

  it('裸数组与 next_cursor 也能解析（容忍老形状）', () => {
    expect(parseListResponse([{ id: 'a' }])).toEqual({ items: [{ id: 'a' }], nextCursor: null });
    expect(parseListResponse({ items: [], next_cursor: 'c1' })).toEqual({
      items: [],
      nextCursor: 'c1',
    });
  });

  it('形状不对 → 空页（不抛错）', () => {
    expect(parseListResponse(undefined)).toEqual({ items: [], nextCursor: null });
    expect(parseListResponse({ items: 'nope' })).toEqual({ items: [], nextCursor: null });
  });
});
