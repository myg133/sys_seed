import { describe, expect, it } from 'vitest';

import {
  ANONYMOUS,
  can,
  conditionalRender,
  createAuthStore,
  guard,
  isAuthenticated,
  tenantId,
  type AuthApiClient,
  type AuthState,
} from '../src/index';

/** 已登录、拥有 `rbac:role:read` 的状态。 */
const READ_ONLY: AuthState = {
  status: 'authenticated',
  authenticated: true,
  userId: 'u-1',
  tenantId: 'tenant-a',
  permissions: ['rbac:role:read'],
};

/** 只接受登录、刷新两个接口的假客户端。 */
function fakeClient(permissions: readonly string[]): {
  client: AuthApiClient;
  logins: number;
  lastOptions: { skipAuthRetry?: boolean } | undefined;
} {
  const state: {
    logins: number;
    lastOptions: { skipAuthRetry?: boolean } | undefined;
  } = { logins: 0, lastOptions: undefined };
  const client: AuthApiClient = {
    async post<T>(
      _path: string,
      body?: unknown,
      options?: { skipAuthRetry?: boolean },
    ): Promise<T> {
      state.logins += 1;
      state.lastOptions = options;
      return {
        accessToken: 'token-1',
        refreshToken: 'refresh-1',
        user: { id: 'u-1', username: 'alice', tenantId: 'tenant-a' },
        permissions: [...permissions],
        body,
      } as unknown as T;
    },
    async get<T>(): Promise<T> {
      return { permissions: [...permissions] } as unknown as T;
    },
  };
  return {
    client,
    get logins() {
      return state.logins;
    },
    get lastOptions() {
      return state.lastOptions;
    },
  };
}

describe('@base/auth 的登录态', () => {
  it('匿名态不算已登录', () => {
    expect(isAuthenticated(ANONYMOUS)).toBe(false);
    expect(tenantId(ANONYMOUS)).toBeUndefined();
  });

  it('登录标志成立才算已登录', () => {
    expect(isAuthenticated(READ_ONLY)).toBe(true);
    expect(isAuthenticated({ ...ANONYMOUS, status: 'authenticated' })).toBe(true);
    // 没有 status 字段（历史形状）时按 `authenticated` 判定。
    expect(isAuthenticated({ authenticated: true, permissions: [] } as unknown as AuthState)).toBe(
      true,
    );
    // status 与 authenticated 矛盾时以 status 为准（fail closed）。
    expect(isAuthenticated({ ...ANONYMOUS, status: 'anonymous', authenticated: true })).toBe(false);
    expect(isAuthenticated(ANONYMOUS)).toBe(false);
  });

  it('已登录时返回租户 ID', () => {
    expect(tenantId(READ_ONLY)).toBe('tenant-a');
    expect(tenantId({ ...ANONYMOUS, tenantId: 'tenant-a' })).toBeUndefined();
  });
});

describe('@base/auth 的 can()（AC-S-066）', () => {
  it('TC-011-011：集合内的权限点返回 true', () => {
    expect(can(READ_ONLY, 'rbac:role:read')).toBe(true);
  });

  it('TC-011-012：集合外的权限点返回 false', () => {
    expect(can(READ_ONLY, 'rbac:role:write')).toBe(false);
  });

  it('TC-011-013：未登记的权限点返回 false 且不抛错', () => {
    let thrown: unknown;
    let result: boolean | undefined;
    try {
      result = can(READ_ONLY, 'order:invoice:delete');
    } catch (error) {
      thrown = error;
    }
    expect(thrown).toBeUndefined();
    expect(result).toBe(false);
  });

  it('TC-011-014：权限点集合为空时一律返回 false（fail closed）', () => {
    const empty: AuthState = { ...ANONYMOUS, status: 'authenticated', authenticated: true };
    expect(can(empty, 'rbac:role:read')).toBe(false);
    expect(can(empty, '*')).toBe(false);
    expect(can(empty, 'rbac:*')).toBe(false);
  });

  it('状态里没有 permissions 字段时也判 false', () => {
    const malformed = { status: 'authenticated', authenticated: true } as unknown as AuthState;
    expect(can(malformed, 'rbac:role:read')).toBe(false);
  });
});

describe('@base/auth 的 guard()（AC-S-068）', () => {
  it('TC-011-019：无权限时给出拒绝判定与 403 落点', () => {
    const result = guard(READ_ONLY, 'rbac:role:write');
    expect(result.allowed).toBe(false);
    expect(result.redirectTo).toBe('/403');
    expect(result.redirectTo).not.toBe('/admin/roles');
  });

  it('TC-011-020：判定与「是否程序化跳转」无关（深链同样被拦）', () => {
    const programmatic = guard(READ_ONLY, 'rbac:role:write');
    const deepLink = guard(READ_ONLY, 'rbac:role:write');
    expect(deepLink.allowed).toBe(false);
    expect(deepLink.redirectTo).toBe('/403');
    expect(programmatic.allowed).toBe(deepLink.allowed);
    expect(programmatic.redirectTo).toBe(deepLink.redirectTo);
  });

  it('有权限时放行', () => {
    const result = guard(READ_ONLY, 'rbac:role:read');
    expect(result.allowed).toBe(true);
    expect(result.redirectTo).toBeUndefined();
  });

  it('未登录时落登录页而不是 403', () => {
    const result = guard(ANONYMOUS, 'rbac:role:write');
    expect(result.allowed).toBe(false);
    expect(result.redirectTo).toBe('/login');
    expect(result.redirectTo).not.toBe('/403');
  });

  it('落点可配置', () => {
    expect(guard(ANONYMOUS, 'rbac:role:read', { loginPath: '/sign-in' }).redirectTo).toBe(
      '/sign-in',
    );
    expect(guard(READ_ONLY, 'rbac:role:write', { forbiddenPath: '/denied' }).redirectTo).toBe(
      '/denied',
    );
  });
});

describe('@base/auth 的条件渲染判定（AC-S-067）', () => {
  it('有权限时渲染 children', () => {
    expect(conditionalRender(READ_ONLY, 'rbac:role:read')).toEqual({
      renderChildren: true,
      fallback: undefined,
    });
  });

  it('无权限时不渲染 children，且不产出兜底内容', () => {
    expect(conditionalRender(READ_ONLY, 'rbac:role:write')).toEqual({
      renderChildren: false,
      fallback: undefined,
    });
  });

  it('无权限且给了兜底内容时产出兜底', () => {
    expect(conditionalRender(READ_ONLY, 'rbac:role:write', '无权限')).toEqual({
      renderChildren: false,
      fallback: '无权限',
    });
  });
});

describe('@base/auth 的认证存储', () => {
  it('登录请求带 skipAuthRetry（认证端点不参与自动刷新 / D100）', async () => {
    // ⚠️ 不能解构 `lastOptions`（getter 会被立刻求值）—— 登录后再读。
    const fake = fakeClient(['rbac:role:read']);
    const auth = createAuthStore(fake.client);

    await auth.login('alice', 'wrong-password');

    // 这一条是 MED-NEW-1 的守卫：去掉 skipAuthRetry，错密码的错误码
    // 会被 refresh 的 TOKEN_REVOKED 覆盖（真机立刻可见）。
    expect(fake.lastOptions).toEqual({ skipAuthRetry: true });
  });

  it('登录后拿到权限点集合', async () => {
    const { client } = fakeClient(['rbac:role:read']);
    const sessions: string[] = [];
    const auth = createAuthStore(client, {
      onLogin: (session) => sessions.push(session.accessToken),
    });

    await auth.login('alice', 'secret');

    expect(auth.state.status).toBe('authenticated');
    expect(auth.state.permissions).toEqual(['rbac:role:read']);
    expect(auth.can('rbac:role:read')).toBe(true);
    expect(auth.can('rbac:role:write')).toBe(false);
    expect(sessions).toEqual(['token-1']);
  });

  it('登录响应没有权限点时集合为空（fail closed）', async () => {
    const { client } = fakeClient([]);
    const auth = createAuthStore(client);

    await auth.login('alice', 'secret');

    expect(auth.state.permissions).toEqual([]);
    expect(auth.can('rbac:role:read')).toBe(false);
  });

  it('登出后权限点集合被清空', async () => {
    const { client } = fakeClient(['rbac:role:read']);
    const auth = createAuthStore(client);

    await auth.login('alice', 'secret');
    await auth.logout();

    expect(auth.state.status).toBe('anonymous');
    expect(auth.state.permissions).toEqual([]);
    expect(auth.can('rbac:role:read')).toBe(false);
  });

  it('存储上的 guard 与包级判定一致', async () => {
    const { client } = fakeClient(['rbac:role:read']);
    const auth = createAuthStore(client);

    // 未登录 → 落登录页（不是 403）。
    expect(auth.guard('rbac:role:write')).toEqual({ allowed: false, redirectTo: '/login' });
    await auth.login('alice', 'secret');
    expect(auth.guard('rbac:role:write')).toEqual({ allowed: false, redirectTo: '/403' });
    expect(auth.guard('rbac:role:read')).toEqual({ allowed: true });
  });

  it('重新拉取权限点后判定跟着更新', async () => {
    const { client } = fakeClient(['rbac:role:read']);
    const auth = createAuthStore(client);
    await auth.login('alice', 'secret');

    await auth.refreshPermissions();

    expect(auth.state.permissions).toEqual(['rbac:role:read']);
  });

  it('登录响应缺少令牌时抛错（不建立空会话）', async () => {
    const client: AuthApiClient = {
      async post<T>(): Promise<T> {
        return { user: { id: 'u-1' } } as unknown as T;
      },
      async get<T>(): Promise<T> {
        return { permissions: [] } as unknown as T;
      },
    };
    const auth = createAuthStore(client);

    await expect(auth.login('alice', 'secret')).rejects.toThrow('访问令牌');
    expect(auth.state.status).toBe('anonymous');
  });
});
