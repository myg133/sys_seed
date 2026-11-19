import { describe, expect, it } from 'vitest';

import { ApiError } from '@base/sdk';

import {
  LOGIN_PATH,
  PERMISSIONS_PATH,
  REFRESH_PATH,
  TENANT_REQUIRED,
  createAppApi,
} from '../src/lib/api';
import { SESSION_STORAGE_KEY, type KeyValueStorage } from '../src/lib/session';

/** 内存版 `sessionStorage`。 */
function memoryStorage(): { storage: KeyValueStorage; map: Map<string, string> } {
  const map = new Map<string, string>();
  return {
    map,
    storage: {
      getItem: (key) => map.get(key) ?? null,
      setItem: (key, value) => {
        map.set(key, value);
      },
      removeItem: (key) => {
        map.delete(key);
      },
    },
  };
}

/** 一次被记录的请求。 */
interface RecordedRequest {
  readonly url: string;
  readonly method: string;
  readonly headers: Headers;
  readonly body: string | undefined;
}

/** 假 fetch：按 URL 分派，并记录每次请求（用于断言请求头 / 请求次数）。 */
function fakeFetch(handle: (request: RecordedRequest) => Response | Promise<Response>): {
  fetch: typeof globalThis.fetch;
  requests: RecordedRequest[];
} {
  const requests: RecordedRequest[] = [];
  const fetchImpl = async (input: RequestInfo | URL, init?: RequestInit): Promise<Response> => {
    // SDK 传的是一个 `Request`（URL / 头 / 体都在它身上）—— 这里克隆后读，不动原件。
    const request = input instanceof Request ? input.clone() : new Request(String(input), init);
    const body = await request.text();
    // 相对路径会被解析成绝对 URL（同源）—— 断言只看路径才稳定。
    const recorded: RecordedRequest = {
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

/** 登录成功的响应体（fixture：只有 `rbac:role:read`）。 */
const LOGIN_BODY = {
  accessToken: 'access-1',
  refreshToken: 'refresh-1',
  user: { id: 'u-1', username: 'u-perm-read@example.com', tenantId: 'tenant-a' },
  permissions: ['rbac:role:read'],
};

/** 已登录会话的持久化形态（等价于「重载前写进去的那份」）。 */
const STORED_SESSION = {
  accessToken: 'access-1',
  refreshToken: 'refresh-1',
  tenantId: 'tenant-a',
};

/** 装配一个可注入的后端；`initial` 在 store 构造**之前**写入（模拟重载）。 */
function build(
  handle: (request: RecordedRequest) => Response | Promise<Response>,
  initial?: Record<string, string>,
): {
  api: ReturnType<typeof createAppApi>;
  requests: RecordedRequest[];
  map: Map<string, string>;
} {
  const { storage, map } = memoryStorage();
  for (const [key, value] of Object.entries(initial ?? {})) {
    map.set(key, value);
  }
  const fake = fakeFetch(handle);
  return { api: createAppApi({ fetch: fake.fetch, storage }), requests: fake.requests, map };
}

describe('租户来源（D98 / HIGH-1）', () => {
  it('登录请求带 x-tenant-id，值等于用户填的租户；成功后租户随会话持久化', async () => {
    const { api, requests, map } = build(() => json(LOGIN_BODY));

    await api.auth.login({
      tenantId: ' tenant-a ',
      username: 'u-perm-read@example.com',
      password: 'secret',
    });

    expect(requests).toHaveLength(1);
    const login = requests[0];
    expect(login?.url).toBe(LOGIN_PATH);
    expect(login?.method).toBe('POST');
    expect(login?.headers.get('x-tenant-id')).toBe('tenant-a');

    expect(api.auth.state.authenticated).toBe(true);
    expect(api.auth.state.tenantId).toBe('tenant-a');
    expect(api.auth.state.permissions).toEqual(['rbac:role:read']);

    expect(JSON.parse(map.get(SESSION_STORAGE_KEY) ?? '{}')).toEqual({
      accessToken: 'access-1',
      refreshToken: 'refresh-1',
      tenantId: 'tenant-a',
    });
  });

  it('租户为空时不发任何请求（不发必然 401 的登录）', async () => {
    const { api, requests } = build(() => json(LOGIN_BODY));

    await expect(api.auth.login({ tenantId: '   ', username: 'u', password: 'p' })).rejects.toThrow(
      TENANT_REQUIRED,
    );

    expect(requests).toHaveLength(0);
  });
});

describe('会话恢复（D99 / AC-S-068 步骤 3）', () => {
  it('用持久化的令牌 + 租户恢复会话；权限点**现取**（不读持久化）', async () => {
    const { api, requests } = build(
      (request) => {
        expect(request.url).toBe(PERMISSIONS_PATH);
        // 重复项 + 乱序：恢复后必须去重升序（与后端口径一致）。
        return json({ permissions: ['rbac:role:write', 'rbac:role:read', 'rbac:role:read'] });
      },
      { [SESSION_STORAGE_KEY]: JSON.stringify(STORED_SESSION) },
    );

    const state = await api.restoreSession();

    expect(state.authenticated).toBe(true);
    expect(state.tenantId).toBe('tenant-a');
    expect(state.permissions).toEqual(['rbac:role:read', 'rbac:role:write']);

    // 恢复后的请求两条头都要在：令牌 + 租户。
    expect(requests).toHaveLength(1);
    expect(requests[0]?.headers.get('authorization')).toBe('Bearer access-1');
    expect(requests[0]?.headers.get('x-tenant-id')).toBe('tenant-a');
    expect(requests[0]?.headers.get('idempotency-key')).toBeNull();
  });

  it('没有持久化会话时按未登录处理，不发请求', async () => {
    const { api, requests } = build(() => json({ permissions: [] }));

    const state = await api.restoreSession();

    expect(state).toEqual({ status: 'anonymous', authenticated: false, permissions: [] });
    expect(requests).toHaveLength(0);
  });

  it('令牌失效（401 且刷新失败）→ 未登录且清空持久化会话，刷新只尝试一次', async () => {
    const { api, requests, map } = build(
      (request) =>
        request.url === REFRESH_PATH
          ? json({ code: 'TOKEN_REVOKED' }, 401)
          : json({ code: 'TOKEN_EXPIRED' }, 401),
      { [SESSION_STORAGE_KEY]: JSON.stringify(STORED_SESSION) },
    );

    const state = await api.restoreSession();

    expect(state.authenticated).toBe(false);
    expect(map.has(SESSION_STORAGE_KEY)).toBe(false);
    expect(requests.map((request) => request.url)).toEqual([PERMISSIONS_PATH, REFRESH_PATH]);
  });

  it('网络层失败（离线）不清会话：令牌可能仍有效，重载后还能再试', async () => {
    const map = new Map<string, string>([[SESSION_STORAGE_KEY, JSON.stringify(STORED_SESSION)]]);
    const api = createAppApi({
      storage: {
        getItem: (key) => map.get(key) ?? null,
        setItem: (key, value) => {
          map.set(key, value);
        },
        removeItem: (key) => {
          map.delete(key);
        },
      },
      fetch: (async () => {
        throw new TypeError('offline');
      }) as typeof globalThis.fetch,
    });

    const state = await api.restoreSession();

    expect(state.authenticated).toBe(false);
    expect(map.has(SESSION_STORAGE_KEY)).toBe(true);
  });
});

describe('错密码登录（MED-NEW-1 / D100）', () => {
  it('错误码原样透传 INVALID_CREDENTIALS；不触发 refresh；不清已有会话', async () => {
    const { api, requests, map } = build(
      (request) => {
        if (request.url === REFRESH_PATH) {
          // MED-NEW-1 的现场：登录 401 后又打了一次 refresh，回 TOKEN_REVOKED。
          return json({ code: 'TOKEN_REVOKED' }, 401);
        }
        return json({ code: 'INVALID_CREDENTIALS', status: 401 }, 401);
      },
      // 已登录用户误在登录页输错密码：旧会话仍然有效，不能被清掉。
      { [SESSION_STORAGE_KEY]: JSON.stringify(STORED_SESSION) },
    );

    const error = await api.auth
      .login({ tenantId: 'tenant-a', username: 'u-perm-read@example.com', password: 'wrong' })
      .then(
        () => undefined,
        (reason: unknown) => reason,
      );

    expect(error).toBeInstanceOf(ApiError);
    expect((error as ApiError).code).toBe('INVALID_CREDENTIALS');
    // 只发一次登录（没有那次注定失败的 refresh）。
    expect(requests.map((request) => request.url)).toEqual([LOGIN_PATH]);
    // 会话仍在（登录失败 ≠ 会话失效）。
    expect(map.has(SESSION_STORAGE_KEY)).toBe(true);
    expect(JSON.parse(map.get(SESSION_STORAGE_KEY) ?? '{}').accessToken).toBe('access-1');
  });
});

describe('刷新不覆盖权限点（约束：后端 refresh 的 permissions 是空数组）', () => {
  it('401 → 刷新 → 重放：权限点保持登录时的集合，新令牌落盘且租户沿用', async () => {
    let rolesCalls = 0;
    const { api, requests, map } = build((request) => {
      if (request.url === LOGIN_PATH) {
        return json(LOGIN_BODY);
      }
      if (request.url === REFRESH_PATH) {
        // 后端 refresh 响应确实带一个空的 `permissions` —— 不能拿它覆盖登录时的集合。
        return json({ accessToken: 'access-2', refreshToken: 'refresh-2', permissions: [] });
      }
      rolesCalls += 1;
      if (rolesCalls === 1) {
        return json({ code: 'TOKEN_EXPIRED' }, 401);
      }
      return json([{ id: 'r-1', name: '只读角色' }]);
    });

    await api.auth.login({ tenantId: 'tenant-a', username: 'u', password: 'p' });
    const roles = await api.client.get<{ id: string; name: string }[]>('/api/rbac/roles');

    expect(roles).toEqual([{ id: 'r-1', name: '只读角色' }]);
    expect(api.auth.state.permissions).toEqual(['rbac:role:read']);

    // 刷新请求本身也带租户头（AC-S-072 步骤 4）。
    const refresh = requests.find((request) => request.url === REFRESH_PATH);
    expect(refresh?.headers.get('x-tenant-id')).toBe('tenant-a');
    expect(JSON.parse(refresh?.body ?? '{}')).toEqual({ refreshToken: 'refresh-1' });

    // 新令牌写回持久化会话，租户沿用。
    expect(JSON.parse(map.get(SESSION_STORAGE_KEY) ?? '{}')).toEqual({
      accessToken: 'access-2',
      refreshToken: 'refresh-2',
      tenantId: 'tenant-a',
    });

    // 重放请求用的是新令牌（D92 的「重放不带幂等键」由 SDK 单测覆盖）。
    const replay = requests.filter((request) => request.url === '/api/rbac/roles')[1];
    expect(replay?.headers.get('authorization')).toBe('Bearer access-2');
  });
});
