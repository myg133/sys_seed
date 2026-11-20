import { render } from 'svelte/server';
import { describe, expect, it } from 'vitest';

import type { ApiClient, RequestOptions } from '@base/sdk';

import OAuthDevice from '../src/pages/OAuthDevice.svelte';
import {
  approveDeviceCode,
  OAUTH_DEVICE_APPROVE_PATH,
  OAUTH_DEVICE_VERIFY_PATH,
  runDeviceFlow,
  verifyDeviceCode,
  type OAuthPageEnv,
} from '../src/lib/oauth-api';

/** 一次请求（记录路径 / 方法 / 请求体 / 查询串）。 */
interface Call {
  readonly path: string;
  readonly method: string;
  readonly body: unknown;
  readonly query: Record<string, string>;
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
      query: (options.query ?? {}) as Record<string, string>,
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
    get: <T>(path: string, options?: { query?: Record<string, string> }): Promise<T> =>
      request<T>({ path, method: 'GET', query: options?.query }),
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

/** 假环境（记录跳转 / replaceState）。 */
function fakeEnv(
  pathname: string,
  search: string,
): {
  env: OAuthPageEnv;
  assigned: string[];
  states: string[];
} {
  let currentSearch = search;
  const assigned: string[] = [];
  const states: string[] = [];
  const env: OAuthPageEnv = {
    pathname,
    get search(): string {
      return currentSearch;
    },
    assign(url: string): void {
      assigned.push(url);
    },
    replace(url: string): void {
      assigned.push(url);
    },
    replaceState(url: string): void {
      states.push(url);
      const index = url.indexOf('?');
      currentSearch = index === -1 ? '' : url.slice(index);
    },
  };
  return { env, assigned, states };
}

describe('设备码页：未登录 → 引导登录（且**保留** `?user_code=`）', () => {
  it('渲染「登录并继续」，且不查设备码', () => {
    const { client, calls } = fakeClient();
    const { env } = fakeEnv('/oauth/device', '?user_code=BCDFGHJK');
    const { body } = render(OAuthDevice, { props: { client, authenticated: false, env } });

    expect(body).toContain('登录并继续');
    expect(calls.filter((call) => call.path === OAUTH_DEVICE_VERIFY_PATH)).toHaveLength(0);
  });
});

describe('设备码页：已登录', () => {
  it('query 里有 `user_code` → 自动查（带上 `userCode` 查询参数）', async () => {
    const { client, calls } = fakeClient({
      [OAUTH_DEVICE_VERIFY_PATH]: {
        clientId: 'client-A',
        clientName: 'Demo App',
        scopes: ['rbac:role:read'],
      },
    });
    const { env } = fakeEnv('/oauth/device', '?user_code=BCDFGHJK');

    const outcome = await runDeviceFlow({
      client,
      authenticated: true,
      env,
      userCode: 'BCDFGHJK',
    });

    expect(outcome.kind).toBe('pending');
    const call = calls.find((entry) => entry.path === OAUTH_DEVICE_VERIFY_PATH);
    expect(call?.query['userCode']).toBe('BCDFGHJK');
  });

  it('query 里**没有** `user_code` → 渲染输入框让用户手输', async () => {
    const { client, calls } = fakeClient();
    const { env } = fakeEnv('/oauth/device', '');

    const outcome = await runDeviceFlow({ client, authenticated: true, env });

    expect(outcome.kind).toBe('input');
    expect(calls.filter((call) => call.path === OAUTH_DEVICE_VERIFY_PATH)).toHaveLength(0);

    const { body } = render(OAuthDevice, { props: { client, authenticated: true, env } });
    expect(body).toContain('设备码');
    expect(body).toContain('查询设备');
  });

  it('查到的设备信息**逐字**展示 client 名与全部权限点码', () => {
    const { client } = fakeClient();
    const { env } = fakeEnv('/oauth/device', '?user_code=BCDFGHJK');
    const { body } = render(OAuthDevice, {
      props: {
        client,
        authenticated: true,
        env,
        initial: {
          clientId: 'client-A',
          clientName: 'Demo App',
          scopes: ['rbac:role:read', 'audit:events:read'],
        },
      },
    });

    expect(body).toContain('Demo App');
    expect(body).toContain('rbac:role:read');
    expect(body).toContain('audit:events:read');
    expect(body).toContain('允许此设备');
    expect(body).toContain('拒绝此设备');
  });
});

describe('设备码页：批准 / 拒绝（AC-S-142 步骤 4）', () => {
  it('「允许」→ 请求体 `{ userCode, action: "approve" }`', async () => {
    const { client, calls } = fakeClient({
      [OAUTH_DEVICE_APPROVE_PATH]: { status: 'approved' },
    });

    const status = await approveDeviceCode(client, 'BCDFGHJK', 'approve');

    expect(status).toBe('approved');
    const call = calls.find((entry) => entry.path === OAUTH_DEVICE_APPROVE_PATH);
    expect(call?.method).toBe('POST');
    expect(call?.body).toEqual({ userCode: 'BCDFGHJK', action: 'approve' });
  });

  it('「拒绝」→ 请求体 `{ userCode, action: "deny" }`', async () => {
    const { client, calls } = fakeClient({
      [OAUTH_DEVICE_APPROVE_PATH]: { status: 'denied' },
    });

    const status = await approveDeviceCode(client, 'BCDFGHJK', 'deny');

    expect(status).toBe('denied');
    const call = calls.find((entry) => entry.path === OAUTH_DEVICE_APPROVE_PATH);
    expect(call?.body).toEqual({ userCode: 'BCDFGHJK', action: 'deny' });
  });

  it('查不到设备码 → 400 `OAUTH_INVALID_REQUEST`（D257：不新增第 10 个码）', async () => {
    const { client } = fakeClient({
      [OAUTH_DEVICE_VERIFY_PATH]: Object.assign(new Error('bad request'), {
        name: 'ApiError',
        code: 'OAUTH_INVALID_REQUEST',
        status: 400,
      }),
    });
    const { env } = fakeEnv('/oauth/device', '?user_code=BCDFGHJK');

    const outcome = await runDeviceFlow({
      client,
      authenticated: true,
      env,
      userCode: 'BCDFGHJK',
    });

    expect(outcome.kind).toBe('error');
    if (outcome.kind === 'error') {
      expect(outcome.error.key).toBe('error.OAUTH_INVALID_REQUEST');
    }
  });

  it('查设备码的请求带上 `userCode` 查询参数（逐字）', async () => {
    const { client, calls } = fakeClient({
      [OAUTH_DEVICE_VERIFY_PATH]: { clientId: 'client-A', clientName: 'Demo', scopes: [] },
    });
    await verifyDeviceCode(client, 'BCDFGHJK');
    const call = calls.find((entry) => entry.path === OAUTH_DEVICE_VERIFY_PATH);
    expect(call?.query).toEqual({ userCode: 'BCDFGHJK' });
  });
});
