import { render } from 'svelte/server';
import { describe, expect, it } from 'vitest';

import type { ApiClient, RequestOptions } from '@base/sdk';

import OAuthLogout from '../src/pages/OAuthLogout.svelte';
import { logoutOAuth, OAUTH_LOGOUT_PATH, type OAuthLogoutResult } from '../src/lib/oauth-api';

/** 一次请求（记录路径 / 方法）。 */
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

/** 从渲染结果里取出全部 iframe 标签。 */
function iframes(html: string): string[] {
  return [...html.matchAll(/<iframe\b[^>]*>/g)].map((match) => match[0]);
}

/** 一个 front-channel 目标。 */
const FRONT_CHANNEL: OAuthLogoutResult['frontChannel'][number] = {
  clientId: 'client-A',
  logoutUrl: 'https://client.example/logout',
  logoutToken: 'jwt-logout-token',
};

describe('登出页：front-channel 的 iframe 由**前端**渲染（AC-S-150 步骤 2）', () => {
  it('iframe 数量**恰好** = frontChannel.length', () => {
    const { client } = fakeClient();
    const { body } = render(OAuthLogout, {
      props: {
        client,
        initial: {
          loggedOut: { grants: 2, sessions: 3 },
          frontChannel: [
            FRONT_CHANNEL,
            { clientId: 'client-C', logoutUrl: 'https://c.example/out', logoutToken: 'jwt-c' },
          ],
        },
      },
    });

    expect(iframes(body)).toHaveLength(2);
  });

  it('`src` **逐字** = logoutUrl + `?logout_token=` + logoutToken', () => {
    const { client } = fakeClient();
    const { body } = render(OAuthLogout, {
      props: {
        client,
        initial: { loggedOut: { grants: 1, sessions: 1 }, frontChannel: [FRONT_CHANNEL] },
      },
    });

    const [frame] = iframes(body);
    expect(frame).toContain('src="https://client.example/logout?logout_token=jwt-logout-token"');
  });

  it('`sandbox` **含** `allow-scripts` 且**不含** `allow-same-origin`（安全要求）', () => {
    const { client } = fakeClient();
    const { body } = render(OAuthLogout, {
      props: {
        client,
        initial: { loggedOut: { grants: 1, sessions: 1 }, frontChannel: [FRONT_CHANNEL] },
      },
    });

    const [frame] = iframes(body);
    expect(frame).toContain('sandbox="allow-scripts"');
    expect(frame).not.toContain('allow-same-origin');
  });

  it('`frontChannel` 是空数组 → **一个 iframe 都不渲染**', () => {
    const { client } = fakeClient();
    const { body } = render(OAuthLogout, {
      props: {
        client,
        initial: { loggedOut: { grants: 0, sessions: 1 }, frontChannel: [] },
      },
    });

    expect(iframes(body)).toHaveLength(0);
    // 「已登出」提示仍在（不是空白页）。
    expect(body).toContain('已登出');
  });

  it('未配 front-channel URI 的 client **不在**其中（每个目标一个 iframe）', () => {
    const { client } = fakeClient();
    const { body } = render(OAuthLogout, {
      props: {
        client,
        initial: {
          loggedOut: { grants: 2, sessions: 1 },
          frontChannel: [FRONT_CHANNEL],
        },
      },
    });

    expect(iframes(body)).toHaveLength(1);
    expect(body).not.toContain('client-B');
    expect(body).not.toContain('client-B.logout');
  });
});

describe('登出页：调后端的登出端点', () => {
  it('`POST /api/oauth/logout`（Bearer 由 SDK 带）→ 解析 `frontChannel`', async () => {
    const { client, calls } = fakeClient({
      [OAUTH_LOGOUT_PATH]: {
        loggedOut: { grants: 2, sessions: 3 },
        frontChannel: [FRONT_CHANNEL],
      },
    });

    const result = await logoutOAuth(client);

    expect(result.loggedOut.grants).toBe(2);
    expect(result.loggedOut.sessions).toBe(3);
    expect(result.frontChannel).toHaveLength(1);
    const call = calls.find((entry) => entry.path === OAUTH_LOGOUT_PATH);
    expect(call?.method).toBe('POST');
  });

  it('`frontChannel` 缺键 → 空数组（不是 `null`）', async () => {
    const { client } = fakeClient({
      [OAUTH_LOGOUT_PATH]: { loggedOut: { grants: 0, sessions: 0 } },
    });

    const result = await logoutOAuth(client);

    expect(result.frontChannel).toEqual([]);
  });
});
