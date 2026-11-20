import { describe, expect, it } from 'vitest';

import {
  buildLoginUrl,
  currentTarget,
  frontChannelSrc,
  isPromptNone,
  isSafeReturnTo,
  isWellFormedUserCode,
  normalizeUserCode,
  OAUTH_AUTHORIZE_PREVIEW_PATH,
  OAUTH_AUTHORIZE_CONFIRM_PATH,
  OAUTH_AUTHORIZE_DENY_PATH,
  OAUTH_AUTHORIZE_ROUTE_PATH,
  OAUTH_DEVICE_APPROVE_PATH,
  OAUTH_DEVICE_ROUTE_PATH,
  OAUTH_DEVICE_VERIFY_PATH,
  OAUTH_ERROR_CODES,
  OAUTH_GRANTS_PATH,
  OAUTH_GRANTS_PERMISSION,
  OAUTH_GRANTS_REVOKE_PATH,
  OAUTH_GRANTS_ROUTE_PATH,
  OAUTH_LOGOUT_PATH,
  OAUTH_LOGOUT_ROUTE_PATH,
  OAUTH_PROMPT_NONE,
  parseDeviceInfo,
  parseLogoutResult,
  parseOAuthGrants,
  parsePreviewResult,
  readAuthorizeParams,
  readReturnTo,
  readUserCode,
  type OAuthPageEnv,
} from '../src/lib/oauth-api';

describe('return_to 的安全校验（D221 / 开放重定向防线）', () => {
  it('站内相对路径：放行，且**原样**返回', () => {
    expect(readReturnTo('?return_to=%2Foauth%2Fauthorize%3Fclient_id%3Da')).toBe(
      '/oauth/authorize?client_id=a',
    );
    expect(readReturnTo('?return_to=%2Fadmin%2Faudit')).toBe('/admin/audit');
  });

  it('协议相对 URL（//evil.com）→ 拒绝', () => {
    expect(readReturnTo('?return_to=%2F%2Fevil.com')).toBeUndefined();
    expect(readReturnTo('?return_to=%2F%2Fevil.com%2Fpath')).toBeUndefined();
  });

  it('带 scheme 的绝对地址（http / https）→ 拒绝', () => {
    expect(readReturnTo('?return_to=https%3A%2F%2Fevil.com')).toBeUndefined();
    expect(readReturnTo('?return_to=http%3A%2F%2Fevil.com')).toBeUndefined();
    expect(readReturnTo('?return_to=javascript%3Aalert(1)')).toBeUndefined();
  });

  it('反斜杠伪装（/\\evil.com）→ 拒绝（浏览器会把 \\ 当 /）', () => {
    expect(readReturnTo('?return_to=%2F%5Cevil.com')).toBeUndefined();
    expect(readReturnTo('?return_to=%2F%5C%2Fevil.com')).toBeUndefined();
  });

  it('控制字符伪装（/%09/evil.com）→ 拒绝（URL 解析器会剥掉 TAB）', () => {
    expect(readReturnTo('?return_to=%2F%09%2Fevil.com')).toBeUndefined();
    expect(readReturnTo('?return_to=%2F%0A%2Fevil.com')).toBeUndefined();
  });

  it('空 / 缺参数 / 不以 / 开头 → undefined（保持现行为）', () => {
    expect(readReturnTo('')).toBeUndefined();
    expect(readReturnTo('?other=1')).toBeUndefined();
    expect(readReturnTo('?return_to=')).toBeUndefined();
    expect(readReturnTo('?return_to=admin%2Faudit')).toBeUndefined();
  });

  it('`isSafeReturnTo` 与 `readReturnTo` 口径一致', () => {
    expect(isSafeReturnTo('/oauth/authorize')).toBe(true);
    expect(isSafeReturnTo('//evil.com')).toBe(false);
    expect(isSafeReturnTo('https://evil.com')).toBe(false);
  });
});

describe('return_to 的编码与解码**成对**（`+` / `%` 不能串味）', () => {
  it('编码 → 解码后拿到**原值**', () => {
    const target = '/oauth/authorize?scope=a+b&state=x%y&client_id=c';
    const url = buildLoginUrl(target);
    expect(url.startsWith('/login?return_to=')).toBe(true);
    expect(readReturnTo(url.slice('/login'.length))).toBe(target);
  });

  it('`+` 与 `%2B` 不会互相串味', () => {
    expect(readReturnTo(buildLoginUrl('/a+b').slice('/login'.length))).toBe('/a+b');
    expect(readReturnTo(buildLoginUrl('/a%2Bb').slice('/login'.length))).toBe('/a%2Bb');
  });

  it('回跳目标里**不含** `return_to` 自己（不会「return_to 套 return_to」）', () => {
    const target = '/oauth/authorize?response_type=code&client_id=c';
    const decoded = readReturnTo(buildLoginUrl(target).slice('/login'.length));
    expect(decoded).toBe(target);
    expect(new URLSearchParams(decoded ?? '').get('return_to')).toBeNull();
  });

  it('`currentTarget` = pathname + search（query 一个都不能少）', () => {
    const { env } = fakeEnv('/oauth/authorize', '?client_id=a&state=b');
    expect(currentTarget(env)).toBe('/oauth/authorize?client_id=a&state=b');
  });
});

describe('端点路径与路由路径（与棒 1 的实现逐字对照）', () => {
  it('8 个平台端点的路径逐字', () => {
    expect(OAUTH_AUTHORIZE_PREVIEW_PATH).toBe('/api/oauth/authorize/preview');
    expect(OAUTH_AUTHORIZE_CONFIRM_PATH).toBe('/api/oauth/authorize/confirm');
    expect(OAUTH_AUTHORIZE_DENY_PATH).toBe('/api/oauth/authorize/deny');
    expect(OAUTH_GRANTS_PATH).toBe('/api/oauth/grants');
    expect(OAUTH_GRANTS_REVOKE_PATH).toBe('/api/oauth/grants/revoke');
    expect(OAUTH_LOGOUT_PATH).toBe('/api/oauth/logout');
    expect(OAUTH_DEVICE_VERIFY_PATH).toBe('/api/oauth/device/verify');
    expect(OAUTH_DEVICE_APPROVE_PATH).toBe('/api/oauth/device/approve');
  });

  it('4 条前端路由的路径逐字（manifest 的 `menus[].route` 逐字等于 `/oauth/grants`）', () => {
    expect(OAUTH_AUTHORIZE_ROUTE_PATH).toBe('/oauth/authorize');
    expect(OAUTH_DEVICE_ROUTE_PATH).toBe('/oauth/device');
    expect(OAUTH_GRANTS_ROUTE_PATH).toBe('/oauth/grants');
    expect(OAUTH_LOGOUT_ROUTE_PATH).toBe('/oauth/logout');
  });

  it('「我的授权」页的权限点逐字', () => {
    expect(OAUTH_GRANTS_PERMISSION).toBe('oauth:grant:read');
  });

  it('平台错误码白名单**恰好 9 个**（D226 / D257）', () => {
    expect(OAUTH_ERROR_CODES).toHaveLength(9);
    expect([...OAUTH_ERROR_CODES]).toEqual([
      'OAUTH_CLIENT_NOT_FOUND',
      'OAUTH_CLIENT_DISABLED',
      'OAUTH_CLIENT_EXPIRED',
      'OAUTH_INVALID_REQUEST',
      'OAUTH_INVALID_REDIRECT_URI',
      'OAUTH_INVALID_SCOPE',
      'OAUTH_UNSUPPORTED_CODE_CHALLENGE_METHOD',
      'OAUTH_TICKET_INVALID',
      'OAUTH_GRANT_NOT_FOUND',
    ]);
    // 没有第 10 个：设备码查不到也是 OAUTH_INVALID_REQUEST（D257）。
    expect(OAUTH_ERROR_CODES.filter((code) => code.startsWith('OAUTH_'))).toHaveLength(9);
  });
});

describe('授权参数：RFC 的 query 名 → camelCase 的体字段', () => {
  it('按 RFC 6749 的 query 名逐字读', () => {
    const params = readAuthorizeParams(
      '?response_type=code&client_id=c-A&redirect_uri=https%3A%2F%2Fclient.example%2Fcb' +
        '&scope=rbac%3Arole%3Aread+audit%3Aevents%3Aread&state=xyz' +
        '&code_challenge=abc&code_challenge_method=S256&prompt=none',
    );
    expect(params.responseType).toBe('code');
    expect(params.clientId).toBe('c-A');
    expect(params.redirectUri).toBe('https://client.example/cb');
    // `scope` 是**空格分隔的字符串**（D258），不是数组。
    expect(params.scope).toBe('rbac:role:read audit:events:read');
    expect(params.state).toBe('xyz');
    expect(params.codeChallenge).toBe('abc');
    expect(params.codeChallengeMethod).toBe('S256');
    expect(params.prompt).toBe('none');
  });

  it('缺省 → undefined（不是空串）', () => {
    const params = readAuthorizeParams('');
    expect(params.responseType).toBeUndefined();
    expect(params.prompt).toBeUndefined();
    expect(params.scope).toBeUndefined();
  });

  it('`prompt=none` 才是不许交互；其它值（含缺省）一律当缺省', () => {
    expect(isPromptNone(OAUTH_PROMPT_NONE)).toBe(true);
    expect(isPromptNone(undefined)).toBe(false);
    expect(isPromptNone('login')).toBe(false);
    expect(isPromptNone('consent')).toBe(false);
    expect(isPromptNone('select_account')).toBe(false);
  });

  it('设备码从 `user_code` 读', () => {
    expect(readUserCode('?user_code=BCDFGHJK')).toBe('BCDFGHJK');
    expect(readUserCode('?other=1')).toBeUndefined();
  });
});

describe('设备码的形态（RFC 8628 §6.1：base20、8 位、无分隔）', () => {
  it('8 位 base20 → 形态合法', () => {
    expect(isWellFormedUserCode('BCDFGHJK')).toBe(true);
    expect(isWellFormedUserCode('bcdfghjk')).toBe(true);
  });

  it('位数不对 / 含易混字符 → 形态非法（判定权仍在后端）', () => {
    expect(isWellFormedUserCode('BCDFGHJ')).toBe(false);
    expect(isWellFormedUserCode('BCDFGHJKL')).toBe(false);
    expect(isWellFormedUserCode('BCDFGHJA')).toBe(false);
  });

  it('规整：去空白、转大写，**不补连字符**', () => {
    expect(normalizeUserCode(' bcdfghjk ')).toBe('BCDFGHJK');
    expect(normalizeUserCode('BCDF-GHJK')).toBe('BCDF-GHJK');
  });
});

describe('响应解析：形状兜底，不抛错', () => {
  it('preview：needsConsent / ticket / application / scopes', () => {
    const parsed = parsePreviewResult({
      needsConsent: true,
      ticket: 'jwt-ticket',
      application: { clientId: 'client-A', name: 'Demo App' },
      scopes: ['rbac:role:read'],
    });
    expect(parsed.needsConsent).toBe(true);
    expect(parsed.ticket).toBe('jwt-ticket');
    expect(parsed.application.name).toBe('Demo App');
    expect(parsed.scopes).toEqual(['rbac:role:read']);
    expect(parsed.redirectTo).toBeUndefined();
  });

  it('preview：needsConsent=false 时**没有** ticket，有 redirectTo', () => {
    const parsed = parsePreviewResult({
      needsConsent: false,
      application: { clientId: 'client-A', name: 'Demo App' },
      scopes: ['rbac:role:read'],
      redirectTo: 'https://client.example/cb?code=abc&state=xyz',
    });
    expect(parsed.ticket).toBeUndefined();
    expect(parsed.redirectTo).toBe('https://client.example/cb?code=abc&state=xyz');
  });

  it('preview：形状不对 → 缺字段而不是抛错', () => {
    const parsed = parsePreviewResult(null);
    expect(parsed.needsConsent).toBe(false);
    expect(parsed.scopes).toEqual([]);
    expect(parsed.application.name).toBe('');
  });

  it('授权列表：逐条规整（缺字段 → 空串 / undefined）', () => {
    const grants = parseOAuthGrants([
      {
        clientId: 'client-A',
        clientName: 'Demo App',
        scopes: ['rbac:role:read'],
        grantedAt: '2026-11-20T10:00:00.000000Z',
        status: 'active',
      },
      { clientId: 'client-B', scopes: [], grantedAt: '', status: 'revoked' },
    ]);
    expect(grants).toHaveLength(2);
    expect(grants[0]?.clientName).toBe('Demo App');
    expect(grants[0]?.status).toBe('active');
    expect(grants[1]?.clientName).toBeUndefined();
    expect(grants[1]?.revokedAt).toBeUndefined();
  });

  it('授权列表：不是数组 → 空列表（不抛错）', () => {
    expect(parseOAuthGrants({ items: [] })).toEqual([]);
  });

  it('登出响应：frontChannel 是数组；缺项 → 空数组（不是 null）', () => {
    const parsed = parseLogoutResult({
      loggedOut: { grants: 2, sessions: 3 },
      frontChannel: [
        { clientId: 'client-A', logoutUrl: 'https://client.example/logout', logoutToken: 'jwt' },
      ],
    });
    expect(parsed.loggedOut.grants).toBe(2);
    expect(parsed.loggedOut.sessions).toBe(3);
    expect(parsed.frontChannel).toHaveLength(1);
    expect(parseLogoutResult({ loggedOut: {} }).frontChannel).toEqual([]);
  });

  it('front-channel 的 src 逐字拼接', () => {
    expect(
      frontChannelSrc({
        clientId: 'client-A',
        logoutUrl: 'https://client.example/logout',
        logoutToken: 'jwt-token',
      }),
    ).toBe('https://client.example/logout?logout_token=jwt-token');
  });

  it('设备码信息：clientId / clientName / scopes', () => {
    const info = parseDeviceInfo({
      clientId: 'client-A',
      clientName: 'Demo App',
      scopes: ['rbac:role:read'],
    });
    expect(info.clientId).toBe('client-A');
    expect(info.clientName).toBe('Demo App');
    expect(info.scopes).toEqual(['rbac:role:read']);
  });
});

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
