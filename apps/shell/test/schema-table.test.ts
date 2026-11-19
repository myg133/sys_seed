import { render } from 'svelte/server';
import { afterEach, describe, expect, it, vi } from 'vitest';

import type { EntityMeta } from '@base/schema';

import SchemaTable from '../src/components/SchemaTable.svelte';

/** 与 design §2.1 一致的 7 字段元数据。 */
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
    { key: 'ownerId', label: '负责人', type: 'reference' },
    { key: 'secret', label: '内部备注', type: 'text', permission: 'sandbox:demo:secret' },
  ],
};

/** 一条完整数据。 */
const ROW: Record<string, unknown> = {
  id: 'item-1',
  title: '示例',
  amount: 12.5,
  paid: true,
  dueAt: '2026-11-19',
  ownerId: 'u-1001',
  secret: '内部-A',
};

/** 表头文本（按出现顺序）。 */
function headers(html: string): string[] {
  return [...html.matchAll(/<th>([\s\S]*?)<\/th>/g)].map((match) => (match[1] ?? '').trim());
}

/** 数据单元格文本（不含行操作列 —— 那是带 class 的 td）。 */
function cells(html: string): string[] {
  return [...html.matchAll(/<td>([\s\S]*?)<\/td>/g)].map((match) => (match[1] ?? '').trim());
}

/** 组件 props（测试侧镜像：只用于构造渲染参数）。 */
interface TableProps {
  meta: EntityMeta;
  items: readonly Record<string, unknown>[];
  nextCursor?: string | null;
  hasPrev?: boolean;
  loading?: boolean;
  error?: string;
  emptyText?: string;
  onCreate?: () => void;
  onEdit?: (record: Record<string, unknown>) => void;
  onDelete?: (record: Record<string, unknown>) => void;
  onPrevPage?: () => void;
  onNextPage?: () => void;
}

/** 渲染组件（SSR：不需要 jsdom —— 本仓前端单测没有 DOM 环境）。 */
function renderTable(overrides: Partial<TableProps> = {}): { body: string } {
  const props: TableProps = { meta: META, items: [ROW], nextCursor: null, ...overrides };
  const result = render(SchemaTable, { props });
  return { body: result.body };
}

afterEach(() => {
  vi.restoreAllMocks();
});

describe('SchemaTable —— 表头与单元格（AC-S-082）', () => {
  it('表头文本与顺序精确等于元数据 fields 的 label 顺序', () => {
    const { body } = renderTable({});
    expect(headers(body)).toEqual(['ID', '标题', '金额', '已付款', '到期日', '负责人', '内部备注']);
  });

  it('单元格按类型格式化：boolean → 是、date → YYYY-MM-DD、number → 十进制', () => {
    const { body } = renderTable({});
    expect(cells(body)).toEqual(['item-1', '示例', '12.5', '是', '2026-11-19', 'u-1001', '内部-A']);
  });

  it('boolean false → 否（不是空、不是原值）', () => {
    const { body } = renderTable({ items: [{ ...ROW, paid: false }] });
    expect(cells(body)[3]).toBe('否');
  });

  it('数据缺非必填字段 → 空串（不出现 undefined / null / NaN）', () => {
    const { body } = renderTable({ items: [{ id: 'item-9', title: '只有标题' }] });
    expect(cells(body)).toEqual(['item-9', '只有标题', '', '', '', '', '']);
    expect(body).not.toContain('undefined');
    expect(body).not.toContain('NaN');
  });
});

describe('SchemaTable —— 空态（AC-S-082 步骤 5 / TC-012-040）', () => {
  it('空数据：不渲染表头空壳，只渲染非空文案', () => {
    const { body } = renderTable({ items: [] });
    expect(body).not.toContain('<thead');
    expect(body).not.toContain('<table');
    const match = /<p class="base-schema-empty">([\s\S]*?)<\/p>/.exec(body);
    expect(match?.[1]).toBe('暂无数据');
    expect(body).not.toContain('undefined');
    expect(body).not.toContain('null');
  });
});

describe('SchemaTable —— 游标分页（AC-S-082 步骤 4）', () => {
  it('nextCursor === null → 「下一页」禁用', () => {
    const { body } = renderTable({ nextCursor: null });
    expect(body).toMatch(/<button[^>]*disabled[^>]*>下一页<\/button>/);
  });

  it('nextCursor 非空 → 「下一页」可用', () => {
    const { body } = renderTable({ nextCursor: 'c2VxOjIw' });
    expect(body).not.toMatch(/<button[^>]*disabled[^>]*>下一页<\/button>/);
  });

  it('hasPrev 决定「上一页」是否禁用', () => {
    expect(renderTable({ hasPrev: false }).body).toMatch(
      /<button[^>]*disabled[^>]*>上一页<\/button>/,
    );
    expect(renderTable({ hasPrev: true }).body).not.toMatch(
      /<button[^>]*disabled[^>]*>上一页<\/button>/,
    );
  });
});

describe('SchemaTable —— 按钮由元数据声明决定（D102）', () => {
  it('声明了 update / delete → 渲染编辑与删除', () => {
    const { body } = renderTable({ onEdit: () => undefined, onDelete: () => undefined });
    expect(body).toContain('编辑');
    expect(body).toContain('删除');
  });

  it('未声明 update / delete → 不渲染对应按钮', () => {
    const meta: EntityMeta = {
      ...META,
      api: { list: META.api.list, create: META.api.create },
    };
    const body = renderTable({ meta, onEdit: () => undefined, onDelete: () => undefined }).body;
    expect(body).not.toContain('编辑');
    expect(body).not.toContain('删除');
  });

  it('没有 onCreate → 不渲染新建按钮', () => {
    const { body } = renderTable({});
    expect(body).not.toContain('新建');
  });
});

describe('SchemaTable —— 元数据演进容错（AC-S-087）', () => {
  it('新增字段：多一列，数据缺该键显示空串', () => {
    const meta: EntityMeta = {
      ...META,
      fields: [...META.fields, { key: 'memo', label: '备注', type: 'text' }],
    };
    const { body } = renderTable({ meta });
    expect(headers(body)).toHaveLength(8);
    expect(headers(body)[7]).toBe('备注');
    expect(cells(body)[7]).toBe('');
  });

  it('删除字段：该列不渲染，数据里多出来的键被忽略', () => {
    const meta: EntityMeta = {
      ...META,
      fields: META.fields.filter((field) => field.key !== 'secret'),
    };
    const { body } = renderTable({ meta });
    expect(headers(body)).toEqual(['ID', '标题', '金额', '已付款', '到期日', '负责人']);
    expect(body).not.toContain('内部备注');
    expect(body).not.toContain('内部-A');
  });

  it('未知类型：降级为文本 + 恰好一条告警，其它列不受影响', () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    const meta: EntityMeta = {
      ...META,
      fields: META.fields.map((field) =>
        field.key === 'amount' ? { ...field, type: 'currency' } : field,
      ),
    };
    const { body } = renderTable({ meta });

    expect(headers(body)).toHaveLength(7);
    expect(cells(body)[2]).toBe('12.5');
    expect(cells(body)).toEqual(['item-1', '示例', '12.5', '是', '2026-11-19', 'u-1001', '内部-A']);
    expect(warn).toHaveBeenCalledTimes(1);
    const message = String(warn.mock.calls[0]?.[0] ?? '');
    expect(message).toContain('currency');
    expect(message).toContain('amount');
  });
});
