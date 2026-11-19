import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

import { ANONYMOUS } from '@base/auth';
import { createI18n } from '@base/i18n';
import { ApiError, CLIENT_NETWORK_ERROR, createApiClient } from '@base/sdk';
import { render } from 'svelte/server';
import { describe, expect, it, vi } from 'vitest';

import SsoButtons from '../src/components/SsoButtons.svelte';
import Login from '../src/pages/Login.svelte';
import { SESSION_STORAGE_KEY, createSessionStore, type KeyValueStorage } from '../src/lib/session';
import { createAppApi, type AppAuth } from '../src/lib/api';
import { errorMessageKey } from '../src/lib/i18n';
import {
  SSO_EXCHANGE_PATH,
  SSO_LDAP_LOGIN_PATH,
  SSO_OIDC_AUTHORIZE_PATH,
  SSO_PROVIDERS_PATH,
  authorizeNavigationUrl,
  listProviders,
  normalizeProviders,
  providersFromParams,
  readTicket,
  startOidcAuthorize,
  type SsoProviderView,
} from '../src/lib/sso-api';
import { completeSsoCallback, describeSsoCallbackError } from '../src/lib/sso-callback';

/**
 * REQ-013 的前端行为（AC-S-021 / AC-S-100 / AC-S-106 / AC-S-109）。
 *
 * 渲染类断言走 `svelte/server` 的 `render`（本仓既有做法：不能点击、不能等待 Promise，
 * 所以「状态机」都在 `src/lib/**` 里，由下面的纯函数用例覆盖）。
 */

/** 壳层源码根（本文件在 `apps/shell/test/` 下）。 */
const SOURCE_DIR = fileURLToPath(new URL('../src/', import.meta.url));

/** 一次被记录的请求。 */
interface RecordedRequest {
  readonly url: string;
  readonly method: string;
  readonly headers: Headers;
  readonly body: string | undefined;
}

/** 假 fetch：按 URL 分派，并记录每次请求（断言请求头 / 请求体 / 次数）。 */
function fakeFetch(handle: (request: RecordedRequest) => Response | Promise<Response>): {
  fetch: typeof globalThis.fetch;
  requests: RecordedRequest[];
} {
  const requests: RecordedRequest[] = [];
  const fetchImpl = async (input: RequestInfo | URL, init?: RequestInit): Promise<Response> => {
    const request = input instanceof Request ? input.clone() : new Request(String(input), init);
    const body = await request.text();
    const recorded: RecordedRequest = {
      // 相对路径会被解析成绝对 URL（同源）—— 断言只看路径才稳定。
      url: new URL(request.url, 'http://shell.test').pathname,
      method: request.method,
      headers: request.headers,
      body: body === '' ? undefined : body,
    };
    requests.push(recorded);
    return handle(recorded);
  };
  return { fetch: fetchImpl as typeof globalThis.fetch, requests };
}

/** JSON 响应。 */
function json(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { 'content-type': 'application/json' },
  });
}

/** 内存版 `sessionStorage`。 */
function memoryStorage(): KeyValueStorage & { data: Map<string, string> } {
  const data = new Map<string, string>();
  return {
    data,
    getItem: (key) => data.get(key) ?? null,
    setItem: (key, value) => {
      data.set(key, value);
    },
    removeItem: (key) => {
      data.delete(key);
    },
  };
}

/** 两个 provider 的响应（tenant-a：OIDC + LDAP，AC-S-109 步骤 1）。 */
const TWO_PROVIDERS = {
  providers: [
    { type: 'oidc', id: 'oidc', label: 'corporate-sso', authorizeUrl: SSO_OIDC_AUTHORIZE_PATH },
    { type: 'ldap', id: 'ldap', label: 'ldap-ad', loginUrl: SSO_LDAP_LOGIN_PATH },
  ],
};

/** 只读 props 的渲染辅助（回调都换成断言用的假件）。 */
function renderSsoButtons(
  providers: readonly SsoProviderView[],
  overrides: { disabled?: boolean } = {},
): string {
  const { body } = render(SsoButtons, {
    props: {
      providers,
      disabled: overrides.disabled,
      onOidc: () => {},
      onLdapSubmit: () => {},
    },
  });
  return body;
}

/** 登录页的 props（SSR 不需要真的网络 / 登录）。 */
function loginProps(auth: AppAuth) {
  return {
    auth,
    client: createApiClient({ baseUrl: '' }),
    onLoggedIn: () => {},
  };
}

/** 一个什么都不做的认证入口（渲染用）。 */
function noopAuth(): AppAuth {
  return {
    state: ANONYMOUS,
    login: async () => {},
    loginWithLdap: async () => {},
    loginWithTicket: async () => {},
    logout: async () => {},
  };
}

describe('providers 渲染（AC-S-109 步骤 1 / 步骤 8）', () => {
  it('0 个 provider → 整块不渲染（不是禁用按钮，也不是空标题）', () => {
    const body = renderSsoButtons([]);
    expect(body).not.toContain('sso-buttons');
    expect(body).not.toContain('企业账号登录');
  });

  it('1 个 OIDC → 只出现 OIDC 按钮，不出现目录表单', () => {
    const body = renderSsoButtons([
      { type: 'oidc', id: 'oidc', authorizeUrl: SSO_OIDC_AUTHORIZE_PATH },
    ]);
    expect(body).toContain('data-testid="sso-buttons"');
    expect(body).toContain('OIDC');
    expect(body).not.toContain('目录用户名');
    expect(body).not.toContain('目录密码');
  });

  it('2 个（OIDC + LDAP）→ 两类入口都在，LDAP 表单直接可见', () => {
    const body = renderSsoButtons(TWO_PROVIDERS.providers);
    expect(body).toContain('OIDC');
    expect(body).toContain('目录用户名');
    expect(body).toContain('目录密码');
  });

  it('只有未知协议 → 不渲染（不显示一个点不动的入口）', () => {
    const body = renderSsoButtons([{ type: 'saml', id: 'saml' }]);
    expect(body).not.toContain('sso-buttons');
  });

  it('登录页初始状态（还没拉到 providers）不渲染区块，本地登录表单照常可用', () => {
    const { body } = render(Login, { props: loginProps(noopAuth()) });
    expect(body).not.toContain('sso-buttons');
    expect(body).toContain('租户');
    expect(body).toContain('>用户名<');
    expect(body).toContain('>密码<');
  });

  it('providers 形状兜底：缺 type 的条目丢弃，未知形状 → 空数组（不抛错）', () => {
    expect(normalizeProviders(undefined)).toEqual([]);
    expect(normalizeProviders(null)).toEqual([]);
    expect(normalizeProviders({})).toEqual([]);
    expect(normalizeProviders({ providers: 'oops' })).toEqual([]);
    expect(normalizeProviders({ providers: [{ id: 'x' }, { type: 'oidc' }] })).toEqual([
      { type: 'oidc', id: 'oidc' },
    ]);
  });
});

describe('providers 请求（AC-S-109 步骤 1 / AC-S-106 步骤 5）', () => {
  it('带 x-tenant-id 拉列表，并解析 { providers: [...] }', async () => {
    const { fetch, requests } = fakeFetch(() => json(TWO_PROVIDERS));
    const client = createApiClient({ baseUrl: '', fetch });

    const providers = await listProviders(client, 'tenant-a');

    expect(providers.map((provider) => provider.type)).toEqual(['oidc', 'ldap']);
    expect(requests).toHaveLength(1);
    expect(requests[0]?.url).toBe(SSO_PROVIDERS_PATH);
    expect(requests[0]?.method).toBe('GET');
    // 登录页此刻没有会话：租户必须由请求头显式带上去。
    expect(requests[0]?.headers.get('x-tenant-id')).toBe('tenant-a');
  });

  it('租户为空 → 不发请求（后端无法决定用哪个 IdP）', async () => {
    const { fetch, requests } = fakeFetch(() => json(TWO_PROVIDERS));
    const client = createApiClient({ baseUrl: '', fetch });

    expect(await listProviders(client, '   ')).toEqual([]);
    expect(requests).toHaveLength(0);
  });

  it('请求失败 → 抛 ApiError（登录页据此**静默隐藏**区块，不阻塞本地登录）', async () => {
    const { fetch } = fakeFetch(() => json({ code: 'INTERNAL_ERROR', title: 'boom' }, 500));
    const client = createApiClient({ baseUrl: '', fetch });

    await expect(listProviders(client, 'tenant-a')).rejects.toBeInstanceOf(ApiError);
  });

  it('200 + 空数组 → 空列表（不是 404，也不是错误）', async () => {
    const { fetch } = fakeFetch(() => json({ providers: [] }));
    const client = createApiClient({ baseUrl: '', fetch });
    expect(await listProviders(client, 'tenant-c')).toEqual([]);
  });
});

describe('强制 SSO（AC-S-021 步骤 1 / 步骤 2 / 步骤 8）', () => {
  it('SSO_REQUIRED 映射到 error.SSO_REQUIRED，且与「密码错」不是同一句话', () => {
    expect(errorMessageKey('SSO_REQUIRED')).toBe('error.SSO_REQUIRED');
    const zh = createI18n({ locale: 'zh-CN' });
    expect(zh.t('error.SSO_REQUIRED')).toBe('该组织要求使用企业账号登录');
    expect(zh.t('error.SSO_REQUIRED')).not.toBe(zh.t('error.INVALID_CREDENTIALS'));
  });

  it('params.ssoProviders（数组 / JSON 字符串）都能变成可渲染的入口', () => {
    const providers = TWO_PROVIDERS.providers;
    expect(providersFromParams({ ssoProviders: providers }).map((p) => p.type)).toEqual([
      'oidc',
      'ldap',
    ]);
    expect(providersFromParams({ ssoProviders: JSON.stringify(providers) })).toHaveLength(2);
    // `params` 缺字段 / 是坏 JSON / 是空对象 → 空列表（不抛错，不渲染坏按钮）。
    expect(providersFromParams(undefined)).toEqual([]);
    expect(providersFromParams({})).toEqual([]);
    expect(providersFromParams({ ssoProviders: '{not json' })).toEqual([]);
  });

  it('后端给的入口与 providers 端点渲染出来的按钮完全一致', () => {
    const fromParams = providersFromParams({ ssoProviders: TWO_PROVIDERS.providers });
    const body = renderSsoButtons(fromParams);
    expect(body).toContain('OIDC');
    expect(body).toContain('目录用户名');
  });
});

describe('OIDC 顶层导航（AC-S-100 预期 1；browser 无法带 x-tenant-id）', () => {
  it('相对 authorizeUrl 补 tenant，且不丢原有 query / hash', () => {
    expect(authorizeNavigationUrl(SSO_OIDC_AUTHORIZE_PATH, 'tenant-a')).toBe(
      `${SSO_OIDC_AUTHORIZE_PATH}?tenant=tenant-a`,
    );
    expect(authorizeNavigationUrl(`${SSO_OIDC_AUTHORIZE_PATH}?x=1#frag`, 'tenant-a')).toBe(
      `${SSO_OIDC_AUTHORIZE_PATH}?x=1&tenant=tenant-a#frag`,
    );
    // 已经带 tenant → 不重复添加。
    expect(authorizeNavigationUrl(`${SSO_OIDC_AUTHORIZE_PATH}?tenant=b`, 'tenant-a')).toBe(
      `${SSO_OIDC_AUTHORIZE_PATH}?tenant=b`,
    );
  });

  it('绝对地址（IdP 自己的）原样返回：不把我们的租户塞给第三方', () => {
    const external = 'https://idp.example.com/authorize?client_id=abc';
    expect(authorizeNavigationUrl(external, 'tenant-a')).toBe(external);
    expect(authorizeNavigationUrl('//idp.example.com/authorize', 'tenant-a')).toBe(
      '//idp.example.com/authorize',
    );
  });

  it('点击 = 整页跳转（location.assign），不是 fetch', () => {
    const assigned: string[] = [];
    startOidcAuthorize(SSO_OIDC_AUTHORIZE_PATH, 'tenant-a', {
      assign: (url) => {
        assigned.push(url);
      },
    });
    expect(assigned).toEqual([`${SSO_OIDC_AUTHORIZE_PATH}?tenant=tenant-a`]);
  });

  it('没有导航能力（非浏览器 / 没注入假件）时抛 ApiError，不静默失败', () => {
    expect(() => startOidcAuthorize(SSO_OIDC_AUTHORIZE_PATH, 'tenant-a')).toThrow(ApiError);
  });
});

describe('回调页交接（P4 / AC-S-100 预期 3 / 预期 8）', () => {
  it('readTicket：只读 ticket，空值 / 缺参数 → undefined', () => {
    expect(readTicket('?ticket=abc')).toBe('abc');
    expect(readTicket('?ticket=a%20b')).toBe('a b');
    expect(readTicket('')).toBeUndefined();
    expect(readTicket('?')).toBeUndefined();
    expect(readTicket('?ticket=')).toBeUndefined();
    expect(readTicket('?ticket=%20%20')).toBeUndefined();
    expect(readTicket('?state=x')).toBeUndefined();
  });

  it('缺 ticket → 「登录链接无效或已过期」，且**不发** exchange 请求', async () => {
    const loginWithTicket = vi.fn(async () => {});
    const onSuccess = vi.fn();

    const outcome = await completeSsoCallback({ search: '', loginWithTicket, onSuccess });

    expect(outcome).toEqual({ kind: 'error', key: 'common.ssoCallbackInvalidLink', params: {} });
    expect(loginWithTicket).not.toHaveBeenCalled();
    expect(onSuccess).not.toHaveBeenCalled();
  });

  it('成功 → 用 ticket 换会话，然后才导航（ticket 不进 URL / localStorage）', async () => {
    const events: string[] = [];
    const outcome = await completeSsoCallback({
      search: '?ticket=t-1',
      loginWithTicket: async (ticket) => {
        events.push(`exchange:${ticket}`);
      },
      onSuccess: () => {
        events.push('navigate');
      },
    });

    expect(outcome).toEqual({ kind: 'success' });
    expect(events).toEqual(['exchange:t-1', 'navigate']);
  });

  it('401 SSO_TICKET_INVALID → 「登录链接已失效，请重新登录」', async () => {
    const onSuccess = vi.fn();
    const outcome = await completeSsoCallback({
      search: '?ticket=used',
      loginWithTicket: async () => {
        throw new ApiError({ code: 'SSO_TICKET_INVALID', status: 401 });
      },
      onSuccess,
    });

    expect(outcome.kind).toBe('error');
    expect(outcome.kind === 'error' ? outcome.key : '').toBe('error.SSO_TICKET_INVALID');
    const zh = createI18n({ locale: 'zh-CN' });
    expect(zh.t('error.SSO_TICKET_INVALID')).toBe('登录链接已失效，请重新登录');
    // 失败时**不**导航（页面停在错误态并给「返回登录页」）。
    expect(onSuccess).not.toHaveBeenCalled();
  });

  it('网络错误与后端错误分开提示', () => {
    const network = describeSsoCallbackError(
      new ApiError({ code: CLIENT_NETWORK_ERROR, status: 0 }),
    );
    const backend = describeSsoCallbackError(new ApiError({ code: 'INTERNAL_ERROR', status: 500 }));
    const unavailable = describeSsoCallbackError(
      new ApiError({ code: 'SSO_OIDC_TOKEN_ENDPOINT_UNAVAILABLE', status: 502 }),
    );

    expect(network.key).toBe('common.clientNetworkError');
    expect(backend.key).toBe('error.INTERNAL_ERROR');
    expect(unavailable.key).toBe('error.SSO_OIDC_TOKEN_ENDPOINT_UNAVAILABLE');
    expect(new Set([network.key, backend.key, unavailable.key]).size).toBe(3);
  });

  it('没有该错误码的文案 → 通用失败文案（不把 "?" 给用户）', async () => {
    const outcome = await completeSsoCallback({
      search: '?ticket=t-1',
      loginWithTicket: async () => {
        throw new ApiError({ code: 'SOMETHING_NEW', status: 400, params: { a: 'b' } });
      },
      onSuccess: () => {},
      hasMessage: () => false,
    });
    expect(outcome).toEqual({
      kind: 'error',
      key: 'common.unexpectedError',
      params: {},
    });
  });

  it('非 ApiError（意外异常）→ 通用失败文案', async () => {
    const outcome = await completeSsoCallback({
      search: '?ticket=t-1',
      loginWithTicket: async () => {
        throw new TypeError('boom');
      },
      onSuccess: () => {},
    });
    expect(outcome.kind === 'error' ? outcome.key : '').toBe('common.unexpectedError');
  });
});

describe('SSO 会话落库（D99 / AC-S-107）', () => {
  it('loginWithTicket：会话进 sessionStorage，租户来自 user.tenantId，exchange 不带租户头', async () => {
    const { fetch, requests } = fakeFetch(() =>
      json({
        accessToken: 'sso-access',
        refreshToken: 'sso-refresh',
        tokenType: 'Bearer',
        expiresIn: 900,
        user: { id: 'u-bob', username: 'bob', tenantId: 'tenant-a' },
        permissions: ['rbac:role:read'],
      }),
    );
    const storage = memoryStorage();
    const api = createAppApi({ storage, fetch });

    await api.auth.loginWithTicket('t-1');

    expect(api.auth.state.authenticated).toBe(true);
    expect(api.auth.state.tenantId).toBe('tenant-a');
    expect(api.auth.state.permissions).toEqual(['rbac:role:read']);

    // 令牌只进 sessionStorage（D99）；URL 里只有 ticket，且 ticket 不进存储。
    const stored = JSON.parse(storage.data.get(SESSION_STORAGE_KEY) ?? '{}') as Record<
      string,
      unknown
    >;
    expect(stored).toEqual({
      accessToken: 'sso-access',
      refreshToken: 'sso-refresh',
      tenantId: 'tenant-a',
    });
    expect(JSON.stringify(stored)).not.toContain('t-1');

    const exchange = requests.find((request) => request.url === SSO_EXCHANGE_PATH);
    expect(exchange?.method).toBe('POST');
    expect(exchange?.headers.get('x-tenant-id')).toBeNull();
    expect(JSON.parse(exchange?.body ?? '{}')).toEqual({ ticket: 't-1' });
  });

  it('loginWithLdap：请求带 x-tenant-id，成功后会话落盘', async () => {
    const { fetch, requests } = fakeFetch(() =>
      json({
        accessToken: 'ldap-access',
        refreshToken: 'ldap-refresh',
        user: { id: 'u-erin', username: 'erin', tenantId: 'tenant-a' },
        permissions: [],
      }),
    );
    const storage = memoryStorage();
    const api = createAppApi({ storage, fetch });

    await api.auth.loginWithLdap({ tenantId: 'tenant-a', username: 'erin', password: 'Er1n-pass' });

    const login = requests.find((request) => request.url === SSO_LDAP_LOGIN_PATH);
    expect(login?.headers.get('x-tenant-id')).toBe('tenant-a');
    expect(JSON.parse(login?.body ?? '{}')).toEqual({ username: 'erin', password: 'Er1n-pass' });
    expect(createSessionStore(storage).state.tenantId).toBe('tenant-a');
  });

  it('loginWithLdap 用 provider 给的 loginUrl（不写死常量路径）', async () => {
    const { fetch, requests } = fakeFetch(() =>
      json({
        accessToken: 'ldap-access',
        user: { id: 'u-erin', username: 'erin', tenantId: 'tenant-a' },
        permissions: [],
      }),
    );
    const api = createAppApi({ storage: memoryStorage(), fetch });

    await api.auth.loginWithLdap({
      tenantId: 'tenant-a',
      username: 'erin',
      password: 'Er1n-pass',
      loginUrl: '/api/iam/sso/ldap/other-source',
    });

    expect(requests.map((request) => request.url)).toEqual(['/api/iam/sso/ldap/other-source']);
  });

  it('loginWithLdap 失败：回滚「尝试用」的租户，且不落盘空会话', async () => {
    const { fetch } = fakeFetch(() => json({ code: 'SSO_LDAP_BIND_FAILED' }, 401));
    const storage = memoryStorage();
    const api = createAppApi({ storage, fetch });

    await expect(
      api.auth.loginWithLdap({ tenantId: 'tenant-a', username: 'erin', password: 'wrong' }),
    ).rejects.toBeInstanceOf(ApiError);
    expect(api.auth.state.authenticated).toBe(false);
    expect(storage.data.has(SESSION_STORAGE_KEY)).toBe(false);
  });

  it('响应缺 accessToken → 按客户端错误处理（不能「界面已登录、请求全 401」）', async () => {
    const { fetch } = fakeFetch(() => json({ user: { tenantId: 'tenant-a' } }));
    const api = createAppApi({ storage: memoryStorage(), fetch });

    await expect(api.auth.loginWithTicket('t-1')).rejects.toBeInstanceOf(ApiError);
    expect(api.auth.state.authenticated).toBe(false);
  });
});

describe('token 不进 URL / localStorage（安全约束 §8 / D99）', () => {
  /** 参与扫描的源码（SSO 新增 / 改动的文件）。 */
  const FILES = [
    'lib/sso-api.ts',
    'lib/sso-callback.ts',
    'pages/Login.svelte',
    'pages/SsoCallback.svelte',
    'components/SsoButtons.svelte',
  ];

  it('源码里 0 处 localStorage；阳性对照能命中', () => {
    for (const file of FILES) {
      const source = readFileSync(`${SOURCE_DIR}${file}`, 'utf8');
      expect(source.includes('localStorage'), `${file} 不得使用 localStorage`).toBe(false);
    }
    // 阳性对照：扫描器对注入的样本必须命中，否则上面的 0 命中不可信。
    expect(`const x = globalThis.localStorage;`.includes('localStorage')).toBe(true);
  });
});
