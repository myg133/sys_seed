import { createRawSnippet } from 'svelte';
import { render } from 'svelte/server';
import { describe, expect, it } from 'vitest';

import type { AuthState } from '@base/auth';

import Can from '../src/components/Can.svelte';

/** 已登录、只有 `rbac:role:read` 的用户。 */
const READ_ONLY: AuthState = {
  status: 'authenticated',
  authenticated: true,
  userId: 'u-1',
  tenantId: 'tenant-a',
  permissions: ['rbac:role:read'],
};

/** 子内容片段（等价于 `<Can>可见</Can>`）。 */
function snippet(text: string) {
  return createRawSnippet(() => ({ render: () => text }));
}

/**
 * 去掉 Svelte 的块锚点标记。
 *
 * Svelte 5 的 `{#if}` 块在 DOM / SSR 输出里会留下锚点（`<!--[-->` / `<!--]-->`）——
 * 它们**不是内容**，但会让 `innerHTML` 不等于空串。
 * 真正要守的不变量是：**子内容不在 DOM 里**（不是 `display:none`）。
 */
function contentOf(html: string): string {
  return html
    .replace(/<!--\[-?\d*!?-->/g, '')
    .replace(/<!--\]-->/g, '')
    .replace(/<!---->/g, '')
    .trim();
}

describe('<Can> 的条件渲染（AC-S-067）', () => {
  it('TC-011-016：有权限时渲染子内容', () => {
    const html = render(Can, {
      props: { permission: 'rbac:role:read', state: READ_ONLY, children: snippet('可见') },
    });
    expect(contentOf(html.body)).toBe('可见');
    expect(html.body).not.toContain('display:none');
    expect(html.body).not.toContain('visibility:hidden');
  });

  it('TC-011-017：无权限时子内容不在 DOM 里（精确等于空串）', () => {
    const html = render(Can, {
      props: { permission: 'rbac:role:write', state: READ_ONLY, children: snippet('不可见') },
    });
    expect(contentOf(html.body)).toBe('');
    expect(html.body).not.toContain('不可见');
  });

  it('TC-011-018：无权限且给了 fallback 时渲染 fallback', () => {
    const html = render(Can, {
      props: {
        permission: 'rbac:role:write',
        state: READ_ONLY,
        fallback: '无权限',
        children: snippet('兜底'),
      },
    });
    expect(contentOf(html.body)).toBe('<span>无权限</span>');
    expect(html.body).not.toContain('兜底');
  });

  it('权限点集合为空时一律不渲染（fail closed）', () => {
    const empty: AuthState = {
      status: 'authenticated',
      authenticated: true,
      permissions: [],
    };
    const html = render(Can, {
      props: { permission: 'rbac:role:read', state: empty, children: snippet('可见') },
    });
    expect(contentOf(html.body)).toBe('');
  });

  it('未登录时也不渲染（fail closed）', () => {
    const anonymous: AuthState = {
      status: 'anonymous',
      authenticated: false,
      permissions: [],
    };
    const html = render(Can, {
      props: { permission: 'rbac:role:read', state: anonymous, children: snippet('可见') },
    });
    expect(contentOf(html.body)).toBe('');
  });
});
