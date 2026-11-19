import { render } from 'svelte/server';
import { afterEach, describe, expect, it, vi } from 'vitest';

import type { EntityMeta } from '@base/schema';

import SchemaForm from '../src/components/SchemaForm.svelte';

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
    { key: 'secret', label: '内部备注', type: 'text' },
  ],
};

/** 组件 props（测试侧镜像）。 */
interface FormProps {
  meta: EntityMeta;
  record?: Record<string, unknown>;
  mode?: 'create' | 'edit';
  submitting?: boolean;
  submitLabel?: string;
  onSubmit?: (body: Record<string, unknown>) => void;
  onCancel?: () => void;
}

/** 渲染组件（SSR）。 */
function renderForm(overrides: Partial<FormProps> = {}): string {
  const props: FormProps = { meta: META, ...overrides };
  return render(SchemaForm, { props }).body;
}

/** 取某个字段的输入控件标签（无控件时返回 undefined）。 */
function inputTag(html: string, key: string): string | undefined {
  return new RegExp(`<input[^>]*id="schema-field-${key}"[^>]*>`).exec(html)?.[0];
}

afterEach(() => {
  vi.restoreAllMocks();
});

describe('SchemaForm —— 5 种字段类型的控件（AC-S-083 步骤 2 / TC-012-041）', () => {
  it('text → 文本框；number → 数字输入；date → 日期；boolean → 复选框', () => {
    const html = renderForm();
    expect(inputTag(html, 'title')).toContain('type="text"');
    expect(inputTag(html, 'amount')).toContain('type="number"');
    expect(inputTag(html, 'dueAt')).toContain('type="date"');
    expect(inputTag(html, 'paid')).toContain('type="checkbox"');
  });

  it('reference → 只读文本（id 原值），不渲染输入控件（D105）', () => {
    const html = renderForm({ record: { id: 'item-1', ownerId: 'u-1001' } });
    expect(inputTag(html, 'ownerId')).toBeUndefined();
    expect(html).toContain('data-field="ownerId"');
    expect(/data-field="ownerId">([\s\S]*?)<\/span>/.exec(html)?.[1]?.trim()).toBe('u-1001');
  });

  it('readonly 字段不渲染输入控件，展示纯文本（TC-012-045）', () => {
    const html = renderForm({ record: { id: 'item-1' } });
    expect(inputTag(html, 'id')).toBeUndefined();
    expect(/data-field="id">([\s\S]*?)<\/span>/.exec(html)?.[1]?.trim()).toBe('item-1');
  });

  it('每个可编辑控件都有 label（可被 getByLabel 定位）', () => {
    const html = renderForm();
    expect(html).toMatch(/<label[^>]*for="schema-field-title"/);
    expect(html).toMatch(/<label[^>]*for="schema-field-amount"/);
    expect(html).toMatch(/<label[^>]*for="schema-field-paid"/);
  });

  it('必填字段有可读的「必填」标记', () => {
    const html = renderForm();
    expect(html).toContain('base-schema-required');
    expect(html).toContain('必填');
  });

  it('关闭浏览器原生校验（novalidate），数字控件允许小数（step=any）', () => {
    // 真机回归：`type=number` 默认 step=1 会让 12.5 变成 :invalid，
    // 浏览器**静默拦截**提交（没有任何字段级错误）—— SSR 单测看不出来。
    const html = renderForm();
    expect(html).toContain('novalidate');
    expect(inputTag(html, 'amount')).toContain('step="any"');
  });
});

describe('SchemaForm —— 校验错误的 DOM 契约（AC-S-083 步骤 4）', () => {
  it('初始渲染：没有错误节点，控件不带 aria-invalid', () => {
    const html = renderForm();
    expect(html).not.toContain('aria-invalid');
    expect(html).not.toContain('base-field-error');
    expect(html).not.toContain('不能为空');
  });
});

describe('SchemaForm —— 未知类型降级（AC-S-087）', () => {
  it('type=currency → 文本框 + 恰好一条告警', () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    const meta: EntityMeta = {
      ...META,
      fields: META.fields.map((field) =>
        field.key === 'amount' ? { ...field, type: 'currency' } : field,
      ),
    };
    const html = renderForm({ meta });

    expect(inputTag(html, 'amount')).toContain('type="text"');
    expect(warn).toHaveBeenCalledTimes(1);
    const message = String(warn.mock.calls[0]?.[0] ?? '');
    expect(message).toContain('currency');
    expect(message).toContain('amount');
  });
});

describe('SchemaForm —— 提交按钮文案与状态', () => {
  it('创建模式：按钮文案为「创建」；编辑模式为「保存」', () => {
    expect(renderForm({ mode: 'create' })).toContain('>创建<');
    expect(renderForm({ mode: 'edit' })).toContain('>保存<');
  });

  it('submitting → 提交按钮禁用（避免重复写）', () => {
    const html = renderForm({ submitting: true });
    expect(html).toMatch(/<button[^>]*disabled[^>]*>[^<]*创建[^<]*<\/button>/);
  });
});
