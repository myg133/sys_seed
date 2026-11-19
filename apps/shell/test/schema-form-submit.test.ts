import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type { EntityMeta } from '@base/schema';

import { gateSubmit } from '../src/lib/schema-submit';

/**
 * 「校验失败**不发请求**」的组件级护栏（AC-S-083 步骤 3 / 步骤 5，L-5）。
 *
 * # 为什么需要这一条
 *
 * QA 的 L-5 证据：把 `validateForm` 变异成「恒返回空映射」后，`apps/shell` 的
 * 10 条用例**全绿** —— 说明组件测试没有一条断言「校验失败时一个请求都没发」。
 *
 * 组件本身（`SchemaForm.svelte`）的 `handleSubmit` 现在调用
 * `src/lib/schema-submit.ts` 的 `gateSubmit()`；这里直接验那层闸门，
 * 用**假 fetch 计数**断言网络请求数 —— 变异 `validateForm` 会让计数变成 1，用例即红。
 *
 * SSR 单测没有 DOM，点不到「创建」按钮，所以「提交」这个动作由这里代替；
 * 真浏览器的同一条判据在 `e2e/schema-driven.spec.ts` 里复验。
 */

/** 与 design §2.1 一致的最小元数据（`id` 只读、`title` 必填、`amount` 有下界）。 */
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
  ],
};

/** 被截获的写请求次数（断言「一个都没发」就看它）。 */
let fetchCalls = 0;

/** 被抓到的提交体（断言 body 的形状）。 */
let sentBodies: Record<string, unknown>[] = [];

/** 模拟 `SchemaPage` 的写请求：真发一次 `fetch`（已被 stub，只计数）。 */
function sendThroughNetwork(body: Record<string, unknown>): void {
  sentBodies.push(body);
  void fetch(META.api.create ?? '/', {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify(body),
  });
}

beforeEach(() => {
  fetchCalls = 0;
  sentBodies = [];
  vi.stubGlobal(
    'fetch',
    vi.fn(async () => {
      fetchCalls += 1;
      return new Response(null, { status: 201 });
    }),
  );
});

afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe('提交闸门 —— 校验失败不发请求（AC-S-083 步骤 3 / 5）', () => {
  it('必填字段留空 → 不发出任何请求，错误定位在该字段', () => {
    const result = gateSubmit(META, { title: '' }, sendThroughNetwork);

    expect(result.sent).toBe(false);
    expect(fetchCalls, '必填留空却发出了写请求 —— 前端没有拦住').toBe(0);
    expect(sentBodies).toEqual([]);
    expect(result.errors['title']).toBeDefined();
    // 错误只落在该字段（不是页面顶部的一条汇总）。
    expect(Object.keys(result.errors)).toEqual(['title']);
  });

  it('number 超出 min/max → 不发出任何请求，错误定位在 number 字段', () => {
    // rev 3 修订（L-6）：真浏览器里 `input[type=number]` 不接受 `abc`
    // （value 变成空串），所以「填非数字」物理上不可达 —— 用越界值取证。
    const result = gateSubmit(META, { title: '合法标题', amount: '-1' }, sendThroughNetwork);

    expect(result.sent).toBe(false);
    expect(fetchCalls, '越界值却发出了写请求 —— 前端没有拦住').toBe(0);
    expect(result.errors['amount']).toContain('不能小于');
  });
});

describe('提交闸门 —— 校验通过才发一个请求（AC-S-083 步骤 6）', () => {
  it('合法值 → 恰好一个请求，body 只含元数据声明的可写字段', () => {
    const result = gateSubmit(
      META,
      {
        id: 'server-generated', // readonly → 不提交
        title: '合法标题',
        amount: '12.5', // number 控件给的是字符串 → 转成数字
        paid: true,
        $internalState: '不该出现', // 渲染器内部状态 / 未声明键 → 不提交
      },
      sendThroughNetwork,
    );

    expect(result.sent).toBe(true);
    expect(result.errors).toEqual({});
    expect(fetchCalls).toBe(1);
    expect(sentBodies).toHaveLength(1);
    expect(sentBodies[0]).toEqual({ title: '合法标题', amount: 12.5, paid: true });
    expect(sentBodies[0]).not.toHaveProperty('id');
    expect(sentBodies[0]).not.toHaveProperty('$internalState');
  });
});
