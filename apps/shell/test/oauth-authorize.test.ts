import { render } from 'svelte/server';
import { describe, expect, it } from 'vitest';

import type { ApiClient, RequestOptions } from '@base/sdk';

import OAuthAuthorize from '../src/pages/OAuthAuthorize.svelte';
import {
  buildLoginUrl,
  currentTarget,
  OAUTH_AUTHORIZE_PREVIEW_PATH,
  OAUTH_GRANTS_PERMISSION,
  readReturnTo,
  runAuthorizeFlow,
  submitConsentDecision,
  withErrorParam,
  type OAuthPageEnv,
} from '../src/lib/oauth-api';
import { createRoutes } from '../src/lib/routes';

/** 一次请求（记录路径 / 方法 / 请求体）。 */
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
  replaced: string[];
  states: string[];
} {
  let currentSearch = search;
  const assigned: string[] = [];
  const replaced: string[] = [];
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
      replaced.push(url);
    },
    replaceState(url: string): void {
      states.push(url);
      const index = url.indexOf('?');
      currentSearch = index === -1 ? '' : url.slice(index);
    },
  };
  return { env, assigned, replaced, states };
}

/** 一个典型的授权请求 query。 */
const AUTHORIZE_QUERY =
  '?response_type=code&client_id=client-A' +
  '&redirect_uri=https%3A%2F%2Fclient.example%2Fcb' +
  '&scope=rbac%3Arole%3Aread&state=xyz' +
  '&code_challenge=abc&code_challenge_method=S256';

describe('未登录：页面自己引导登录（公开路由，D221）', () => {
  it('`prompt` 缺省 → 渲染「登录并继续」，且**不调 preview**', () => {
    const { client, calls } = fakeClient();
    const { env, assigned } = fakeEnv('/oauth/authorize', AUTHORIZE_QUERY);
    const { body } = render(OAuthAuthorize, { props: { client, authenticated: false, env } });

    expect(body).toContain('登录并继续');
    // preview 需要 Bearer —— 未登录**根本不发**这个请求。
    expect(calls.filter((call) => call.path === OAUTH_AUTHORIZE_PREVIEW_PATH)).toHaveLength(0);
    // 没有 `prompt=none` → **不**改 URL，**不**跳登录页。
    expect(assigned).toHaveLength(0);
  });

  it('`prompt=none` → query 出现 `error=login_required`，且**不跳登录页**', () => {
    const { client, calls } = fakeClient();
    const { env, assigned, states } = fakeEnv('/oauth/authorize', `${AUTHORIZE_QUERY}&prompt=none`);
    const { body } = render(OAuthAuthorize, { props: { client, authenticated: false, env } });

    expect(calls.filter((call) => call.path === OAUTH_AUTHORIZE_PREVIEW_PATH)).toHaveLength(0);
    // 不跳登录页（`prompt=none` 不许交互）。
    expect(assigned).toHaveLength(0);
    expect(states).toHaveLength(1);
    const url = new URL(states[0] ?? '', 'https://base.invalid');
    expect(url.searchParams.get('error')).toBe('login_required');
    // ⚠️ 原有授权参数**一个都不能丢**（AC-S-137 步骤 3 的判据）。
    expect(url.searchParams.get('client_id')).toBe('client-A');
    expect(url.searchParams.get('state')).toBe('xyz');
    expect(url.searchParams.get('code_challenge')).toBe('abc');
    expect(url.searchParams.get('redirect_uri')).toBe('https://client.example/cb');
    expect(body).toContain('需要登录');
  });

  it('`withErrorParam` 保留其它参数（`error` 不覆盖它们）', () => {
    const query = withErrorParam(AUTHORIZE_QUERY, 'consent_required');
    const params = new URLSearchParams(query);
    expect(params.get('error')).toBe('consent_required');
    expect(params.get('client_id')).toBe('client-A');
    expect(params.get('scope')).toBe('rbac:role:read');
  });
});

describe('未登录回跳：`return_to` 必须**保留全部 query**（D221 的核心判据）', () => {
  it('登录页地址里的 `return_to` = 完整 URL（pathname + search，一个参数都不能少）', () => {
    const { env } = fakeEnv('/oauth/authorize', AUTHORIZE_QUERY);

    const url = buildLoginUrl(currentTarget(env));
    const decoded = readReturnTo(url.slice('/login'.length));

    expect(decoded).toBe(
      '/oauth/authorize?response_type=code&client_id=client-A' +
        '&redirect_uri=https%3A%2F%2Fclient.example%2Fcb' +
        '&scope=rbac%3Arole%3Aread&state=xyz' +
        '&code_challenge=abc&code_challenge_method=S256',
    );
    // 逐个参数都在（不只是「有个 return_to」）。
    const params = new URLSearchParams(decoded ?? '');
    expect(params.get('client_id')).toBe('client-A');
    expect(params.get('state')).toBe('xyz');
    expect(params.get('code_challenge')).toBe('abc');
    expect(params.get('code_challenge_method')).toBe('S256');
    expect(params.get('redirect_uri')).toBe('https://client.example/cb');
    expect(params.get('scope')).toBe('rbac:role:read');
  });

  it('设备码页的 `return_to` **保留** `?user_code=`（否则设备码流程不可用）', () => {
    const { env } = fakeEnv('/oauth/device', '?user_code=BCDFGHJK');

    const url = buildLoginUrl(currentTarget(env));
    const decoded = readReturnTo(url.slice('/login'.length));

    expect(decoded).toBe('/oauth/device?user_code=BCDFGHJK');
  });
});

describe('同意页：用户所见 = 实际授予（AC-S-146 步骤 1）', () => {
  it('`needsConsent=true` → 渲染同意页，页面文本里**逐字出现**权限点码', () => {
    const { client, calls } = fakeClient();
    const { env } = fakeEnv('/oauth/authorize', AUTHORIZE_QUERY);
    const { body } = render(OAuthAuthorize, {
      props: {
        client,
        authenticated: true,
        env,
        initial: {
          needsConsent: true,
          ticket: 'jwt-ticket',
          application: { clientId: 'client-A', name: 'Demo App' },
          scopes: ['rbac:role:read', 'audit:events:read'],
        },
      },
    });

    // client 的 name + **全部**权限点码都逐字出现。
    expect(body).toContain('Demo App');
    expect(body).toContain('rbac:role:read');
    expect(body).toContain('audit:events:read');
    // 「允许」/「拒绝」两个动作都在。
    expect(body).toContain('允许该应用');
    expect(body).toContain('拒绝该应用');
    // 给了首屏 preview → 页面**不再**自己发 preview 请求。
    expect(calls.filter((call) => call.path === OAUTH_AUTHORIZE_PREVIEW_PATH)).toHaveLength(0);
  });

  it('`prompt=none` + `needsConsent=true` → query 出现 `error=consent_required`，**仍然渲染同意页**（D233）', async () => {
    const { client } = fakeClient({
      [OAUTH_AUTHORIZE_PREVIEW_PATH]: {
        needsConsent: true,
        ticket: 'jwt-ticket',
        application: { clientId: 'client-A', name: 'Demo App' },
        scopes: ['rbac:role:read'],
      },
    });
    const { env, states } = fakeEnv('/oauth/authorize', `${AUTHORIZE_QUERY}&prompt=none`);

    const outcome = await runAuthorizeFlow({
      client,
      params: {
        responseType: 'code',
        clientId: 'client-A',
        redirectUri: 'https://client.example/cb',
        scope: 'rbac:role:read',
        state: 'xyz',
        codeChallenge: 'abc',
        codeChallengeMethod: 'S256',
        prompt: 'none',
      },
      authenticated: true,
      env,
    });

    expect(outcome.kind).toBe('consent');
    expect(states).toHaveLength(1);
    const url = new URL(states[0] ?? '', 'https://base.invalid');
    expect(url.searchParams.get('error')).toBe('consent_required');
    // 原有参数仍然在。
    expect(url.searchParams.get('client_id')).toBe('client-A');
    expect(url.searchParams.get('state')).toBe('xyz');
  });

  it('preview 返 400 → 渲染 `error.<CODE>` 的文案（不是空白页）', async () => {
    const { client } = fakeClient({
      [OAUTH_AUTHORIZE_PREVIEW_PATH]: Object.assign(new Error('bad request'), {
        name: 'ApiError',
        code: 'OAUTH_INVALID_REDIRECT_URI',
        status: 400,
      }),
    });
    const { env } = fakeEnv('/oauth/authorize', AUTHORIZE_QUERY);

    const outcome = await runAuthorizeFlow({
      client,
      params: { clientId: 'client-A' },
      authenticated: true,
      env,
    });

    expect(outcome.kind).toBe('error');
    if (outcome.kind === 'error') {
      expect(outcome.error.key).toBe('error.OAUTH_INVALID_REDIRECT_URI');
    }
  });
});

describe('同意 / 拒绝 → 用后端返回的 `redirectTo` 跳转', () => {
  it('确认 → 请求体 `{ ticket }`，跳到返回的 `redirectTo`', async () => {
    const { client, calls } = fakeClient({
      '/api/oauth/authorize/confirm': {
        redirectTo: 'https://client.example/cb?code=abc&state=xyz',
      },
    });
    const { env, assigned } = fakeEnv('/oauth/authorize', AUTHORIZE_QUERY);

    const redirectTo = await submitConsentDecision(client, env, 'jwt-ticket', 'allow');

    expect(redirectTo).toBe('https://client.example/cb?code=abc&state=xyz');
    expect(assigned).toEqual(['https://client.example/cb?code=abc&state=xyz']);
    const call = calls.find((entry) => entry.path === '/api/oauth/authorize/confirm');
    expect(call?.method).toBe('POST');
    expect(call?.body).toEqual({ ticket: 'jwt-ticket' });
  });

  it('拒绝 → 请求体 `{ ticket }`，跳到返回的 `redirectTo`（query 里有 `error=access_denied`）', async () => {
    const { client, calls } = fakeClient({
      '/api/oauth/authorize/deny': {
        redirectTo: 'https://client.example/cb?error=access_denied&state=xyz',
      },
    });
    const { env, assigned } = fakeEnv('/oauth/authorize', AUTHORIZE_QUERY);

    const redirectTo = await submitConsentDecision(client, env, 'jwt-ticket', 'deny');

    expect(redirectTo).toContain('error=access_denied');
    // AC-S-146 步骤 4：`state` 也必须**逐字**带回原请求的 `state`。
    expect(redirectTo).toContain('state=xyz');
    expect(assigned).toEqual(['https://client.example/cb?error=access_denied&state=xyz']);
    const call = calls.find((entry) => entry.path === '/api/oauth/authorize/deny');
    expect(call?.method).toBe('POST');
    expect(call?.body).toEqual({ ticket: 'jwt-ticket' });
  });
});

describe('免登直发（needsConsent=false）', () => {
  it('直接跳 `redirectTo`，不出同意页', async () => {
    const { client } = fakeClient({
      [OAUTH_AUTHORIZE_PREVIEW_PATH]: {
        needsConsent: false,
        application: { clientId: 'client-A', name: 'Demo App' },
        scopes: ['rbac:role:read'],
        redirectTo: 'https://client.example/cb?code=abc&state=xyz',
      },
    });
    const { env, assigned } = fakeEnv('/oauth/authorize', AUTHORIZE_QUERY);

    const outcome = await runAuthorizeFlow({
      client,
      params: { clientId: 'client-A', prompt: 'none' },
      authenticated: true,
      env,
    });

    expect(outcome.kind).toBe('redirected');
    expect(assigned).toEqual(['https://client.example/cb?code=abc&state=xyz']);
  });
});

describe('路由登记（D221 / D256）', () => {
  it('`/oauth/authorize` 是**公开路由**（不声明 permission / requiresAuth）', () => {
    const { client } = fakeClient();
    const route = createRoutes(client).find((entry) => entry.path === '/oauth/authorize');
    expect(route).toBeDefined();
    expect(route?.permission).toBeUndefined();
    expect(route?.requiresAuth).toBeUndefined();
  });

  it('`/oauth/grants` 是受保护路由，权限点 `oauth:grant:read`', () => {
    const { client } = fakeClient();
    const route = createRoutes(client).find((entry) => entry.path === '/oauth/grants');
    expect(route?.permission).toBe(OAUTH_GRANTS_PERMISSION);
  });
});
