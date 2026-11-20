import { render } from 'svelte/server';
import { describe, expect, it } from 'vitest';

import type { AppAuth } from '../src/lib/api';
import type { ApiClient } from '@base/sdk';

import Login from '../src/pages/Login.svelte';
import { resolveLoginRedirect } from '../src/lib/oauth-api';

/** 假认证入口（登录页只在 props 里取它，本文件不触发登录）。 */
const AUTH = {
  state: { status: 'anonymous', authenticated: false, permissions: [] },
  login: async (): Promise<void> => undefined,
  loginWithLdap: async (): Promise<void> => undefined,
  loginWithTicket: async (): Promise<void> => undefined,
  logout: async (): Promise<void> => undefined,
} as unknown as AppAuth;

/** 假客户端（登录页首帧不发请求）。 */
const CLIENT = {} as unknown as ApiClient;

/** 渲染登录页。 */
function renderLogin(search: string): string {
  const { body } = render(Login, {
    props: { auth: AUTH, client: CLIENT, onLoggedIn: (): void => undefined, search },
  });
  return body;
}

describe('登录页：有 `?return_to=` 时提示「登录后返回原页面」（D221）', () => {
  it('合格的 `return_to` → 渲染提示', () => {
    const body = renderLogin('?return_to=%2Foauth%2Fauthorize%3Fclient_id%3Dclient-A');
    expect(body).toContain('登录后将返回原页面');
  });

  it('**没有** `return_to` → 不渲染提示（保持现行为）', () => {
    const body = renderLogin('');
    expect(body).not.toContain('登录后将返回原页面');
  });

  it('`return_to` 不合格（开放重定向尝试）→ 不渲染提示，也不接受它', () => {
    const body = renderLogin('?return_to=%2F%2Fevil.com');
    expect(body).not.toContain('登录后将返回原页面');
    expect(resolveLoginRedirect('?return_to=%2F%2Fevil.com', '/')).toEqual({
      kind: 'navigate',
      path: '/',
    });
  });
});

describe('登录成功后的落点（`App.svelte` 用的纯判定）', () => {
  it('有合格的 `return_to` → 整页 `replace`（query 一个不丢）', () => {
    expect(
      resolveLoginRedirect('?return_to=%2Foauth%2Fauthorize%3Fclient_id%3Dclient-A', '/'),
    ).toEqual({ kind: 'replace', url: '/oauth/authorize?client_id=client-A' });
  });

  it('没有 `return_to` → **保持现行为**：跳 `intendedPath`', () => {
    expect(resolveLoginRedirect('', '/admin/audit')).toEqual({
      kind: 'navigate',
      path: '/admin/audit',
    });
  });

  it('没有 `return_to` 也没有 `intendedPath` → 跳首页', () => {
    expect(resolveLoginRedirect('', undefined)).toEqual({ kind: 'navigate', path: '/' });
  });

  it('`return_to` 不合格 → **保持现行为**（绝不跳外站）', () => {
    expect(resolveLoginRedirect('?return_to=https%3A%2F%2Fevil.com', '/admin/audit')).toEqual({
      kind: 'navigate',
      path: '/admin/audit',
    });
  });
});
