import { render } from 'svelte/server';
import { describe, expect, it, vi } from 'vitest';

import type { ApiClient, RequestOptions } from '@base/sdk';

import OAuthGrants from '../src/pages/OAuthGrants.svelte';
import {
  listOAuthGrants,
  loadOAuthGrants,
  OAUTH_GRANTS_PATH,
  OAUTH_GRANTS_REVOKE_PATH,
  revokeOAuthGrant,
} from '../src/lib/oauth-api';
import { createRouter, type RouterEnvironment } from '../src/lib/router';
import { createRoutes } from '../src/lib/routes';

/** 一次请求（记录路径 / 方法 / 请求体）。 */
interface Call {
  readonly path: string;
  readonly method: string;
  readonly body: unknown;
}

/** 假客户端：按路径返回预置响应，并记录调用。 */
function fakeClient(responses: Record<string, unknown> = {}): {
  client: ApiClient;
  calls: Call[];
} {
  const calls: Call[] = [];
  const request = async <T>(options: RequestOptions): Promise<T> => {
    calls.push({
      path: options.path,
      method: options.method ?? 'GET',
      body: options.body,
    });
    const hit = responses[options.path];
    if (hit instanceof Error) {
      throw hit;
    }
    return (hit ?? {}) as T;
  };
  const client = {
    baseUrl: '',
    request,
    get: <T>(path: string): Promise<T> => request<T>({ path, method: 'GET' }),
    post: <T>(path: string, body?: unknown): Promise<T> =>
      request<T>({ path, method: 'POST', body }),
    put: <T>(path: string, body?: unknown): Promise<T> => request<T>({ path, method: 'PUT', body }),
    delete: <T>(path: string): Promise<T> => request<T>({ path, method: 'DELETE' }),
    refresh: <T>(path: string, body?: unknown): Promise<T> =>
      request<T>({ path, method: 'POST', body }),
    setContext: (): void => undefined,
    url: (path: string): string => path,
  } as unknown as ApiClient;
  return { client, calls };
}

/** 假环境：只关心 pathname 与写回的地址。 */
function fakeEnvironment(pathname: string): {
  environment: RouterEnvironment;
  written: string[];
} {
  let current = pathname;
  const written: string[] = [];
  const environment: RouterEnvironment = {
    location: {
      get pathname(): string {
        return current;
      },
      search: '',
    },
    history: {
      pushState(_state: unknown, _title: string, url: string): void {
        current = url;
        written.push(url);
      },
      replaceState(_state: unknown, _title: string, url: string): void {
        current = url;
        written.push(url);
      },
    },
    addEventListener: (): void => undefined,
    removeEventListener: (): void => undefined,
  };
  return { environment, written };
}

/** 两条授权记录。 */
const GRANTS = [
  {
    clientId: 'client-A',
    clientName: 'Demo App',
    scopes: ['rbac:role:read'],
    grantedAt: '2026-11-20T10:00:00.000000Z',
    status: 'active',
  },
  {
    clientId: 'client-B',
    clientName: 'Other App',
    scopes: ['audit:events:read'],
    grantedAt: '2026-11-21T10:00:00.000000Z',
    status: 'revoked',
    revokedAt: '2026-11-22T10:00:00.000000Z',
  },
];

describe('「我的授权」页：列表渲染（AC-S-147 步骤 1）', () => {
  it('渲染 client 名 / 权限点码 / 授权时间 / 状态', () => {
    const { client } = fakeClient();
    const { body } = render(OAuthGrants, {
      props: {
        client,
        initial: { grants: GRANTS },
      },
    });

    expect(body).toContain('Demo App');
    expect(body).toContain('rbac:role:read');
    // 授权时间按当前时区**格式化**（不是把 RFC3339 原文直接怼给用户）。
    expect(body).toContain('2026');
    expect(body).not.toContain('2026-11-20T10:00:00.000000Z');
    expect(body).toContain('生效中');
    expect(body).toContain('已撤销');
  });

  it('没有授权记录 → 显示空态提示（**不是**空白页）', () => {
    const { client } = fakeClient();
    const { body } = render(OAuthGrants, { props: { client, initial: { grants: [] } } });

    expect(body).toContain('暂无授权记录');
  });

  it('首屏数据来自**路由的 `load`**（页面自己不重复请求）', async () => {
    const { client, calls } = fakeClient();
    const result = await loadOAuthGrants(client);

    expect(result.grants).toEqual([]);
    expect(result.failure).toBeUndefined();
    expect(calls.filter((call) => call.path === OAUTH_GRANTS_PATH)).toHaveLength(1);
  });
});

describe('「我的授权」页：撤销（AC-S-147 步骤 2）', () => {
  it('撤销 → 请求体 `{ clientId }`', async () => {
    const { client, calls } = fakeClient({
      [OAUTH_GRANTS_REVOKE_PATH]: { status: 'revoked' },
    });

    await revokeOAuthGrant(client, 'client-A');

    const call = calls.find((entry) => entry.path === OAUTH_GRANTS_REVOKE_PATH);
    expect(call?.method).toBe('POST');
    expect(call?.body).toEqual({ clientId: 'client-A' });
  });

  it('列表只包含**当前用户自己的**记录（前端不做租户过滤）', async () => {
    const { client } = fakeClient({ [OAUTH_GRANTS_PATH]: GRANTS });
    const grants = await listOAuthGrants(client);

    expect(grants).toHaveLength(2);
    expect(grants.map((grant) => grant.clientId)).toEqual(['client-A', 'client-B']);
  });
});

describe('守卫：请求在**之前**就拦住（`GET /api/oauth/grants` 的请求计数为 0）', () => {
  it('无 `oauth:grant:read` 的账号 → 落 403 页，且目标接口的请求计数为 0', async () => {
    const { client, calls } = fakeClient();
    const { environment, written } = fakeEnvironment('/oauth/grants');
    const router = createRouter({
      routes: createRoutes(client),
      environment,
      getState: () => ({
        status: 'authenticated',
        authenticated: true,
        userId: 'u-noperm',
        tenantId: 'tenant-a',
        permissions: ['rbac:role:read'],
      }),
      onResolve: () => undefined,
    });

    router.start();
    await vi.waitFor(() => expect(calls.length).toBe(0));

    expect(written.at(-1)).toBe('/403');
    expect(calls.filter((call) => call.path === OAUTH_GRANTS_PATH)).toHaveLength(0);
  });

  it('有 `oauth:grant:read` 的账号 → 放行并请求授权列表', async () => {
    const { client, calls } = fakeClient({ [OAUTH_GRANTS_PATH]: GRANTS });
    const { environment, written } = fakeEnvironment('/oauth/grants');
    const router = createRouter({
      routes: createRoutes(client),
      environment,
      getState: () => ({
        status: 'authenticated',
        authenticated: true,
        userId: 'u-auditor',
        tenantId: 'tenant-a',
        permissions: ['oauth:grant:read'],
      }),
      onResolve: () => undefined,
    });

    router.start();
    await vi.waitFor(() => expect(calls.map((call) => call.path)).toContain(OAUTH_GRANTS_PATH));

    expect(written.at(-1)).toBe('/oauth/grants');
    expect(calls.filter((call) => call.path === OAUTH_GRANTS_PATH)).toHaveLength(1);
  });

  it('未登录 → 落登录页（不是 403），且目标接口的请求计数为 0', async () => {
    const { client, calls } = fakeClient();
    const { environment, written } = fakeEnvironment('/oauth/grants');
    const router = createRouter({
      routes: createRoutes(client),
      environment,
      getState: () => ({
        status: 'anonymous',
        authenticated: false,
        permissions: [],
      }),
      onResolve: () => undefined,
    });

    router.start();
    await vi.waitFor(() => expect(calls.length).toBe(0));

    expect(written.at(-1)).toBe('/login');
    expect(calls.filter((call) => call.path === OAUTH_GRANTS_PATH)).toHaveLength(0);
  });
});
