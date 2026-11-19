import { describe, expect, it, vi } from 'vitest';

import {
  ApiError,
  CLIENT_INTERNAL_ERROR,
  CLIENT_NETWORK_ERROR,
  createApiClient,
} from '../src/index';

/** 假件：按路径与调用序号返回响应，并记录每次收到的 `Request`。 */
function fakeFetch(handler: (request: Request, index: number) => Response | Promise<Response>): {
  fetch: typeof fetch;
  calls: Request[];
} {
  const calls: Request[] = [];
  const impl = (async (input: RequestInfo | URL, init?: RequestInit) => {
    const request = input instanceof Request ? input : new Request(input, init);
    calls.push(request);
    return handler(request, calls.length - 1);
  }) as unknown as typeof fetch;
  return { fetch: impl, calls };
}

/** JSON 响应。 */
function jsonResponse(body: unknown, status: number): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { 'content-type': 'application/json' },
  });
}

/** 取请求路径。 */
function pathOf(request: Request): string {
  return new URL(request.url).pathname;
}

/** 后端真实的 403 问题详情（`crates/platform-core/src/problem.rs` 的形状）。 */
const PROBLEM_403 = {
  type: 'https://errors.base.example/insufficient-permission',
  title: '权限不足',
  status: 403,
  detail: 'the current role does not grant the permission `rbac:role:write`',
  instance: '/api/rbac/roles',
  code: 'INSUFFICIENT_PERMISSION',
  trace_id: 'trace-abc',
  params: { required: 'rbac:role:write' },
};

/** UUID v4 正则（AC-S-071 步骤 5）。 */
const UUID_V4 = /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;

describe('@base/sdk 的 URL 拼接', () => {
  it('去掉结尾斜杠', () => {
    expect(createApiClient({ baseUrl: 'https://api.base.example/' }).baseUrl).toBe(
      'https://api.base.example',
    );
  });

  it('拼接路径时补上分隔斜杠', () => {
    const client = createApiClient({ baseUrl: 'https://api.base.example' });
    expect(client.url('/api/iam/users')).toBe('https://api.base.example/api/iam/users');
    expect(client.url('api/iam/users')).toBe('https://api.base.example/api/iam/users');
  });
});

describe('@base/sdk 的错误模型（AC-S-069）', () => {
  it('TC-011-023：code 精确等于响应体的 code', async () => {
    const { fetch: fake } = fakeFetch(() => jsonResponse(PROBLEM_403, 403));
    const client = createApiClient({ baseUrl: '', fetch: fake });

    const error = await client.request({ method: 'GET', path: '/api/rbac/roles' }).then(
      () => undefined,
      (reason: unknown) => reason,
    );

    expect(error).toBeInstanceOf(ApiError);
    const apiError = error as ApiError;
    expect(apiError.code).toBe('INSUFFICIENT_PERMISSION');
    expect(apiError.status).toBe(403);
    expect(apiError.name).not.toBe('TypeError');
    expect(apiError.code).not.toBe('权限不足');
    expect(apiError.code).not.toBe(
      'the current role does not grant the permission `rbac:role:write`',
    );
  });

  it('TC-011-024：响应体不是 JSON → client.INTERNAL_ERROR', async () => {
    const { fetch: fake } = fakeFetch(
      () =>
        new Response('<!doctype html><html><body>500 Internal Server Error</body></html>', {
          status: 500,
          headers: { 'content-type': 'text/html' },
        }),
    );
    const client = createApiClient({ baseUrl: '', fetch: fake });

    const error = await client.request({ method: 'GET', path: '/api/anything' }).then(
      () => undefined,
      (reason: unknown) => reason,
    );

    expect(error).toBeInstanceOf(ApiError);
    expect((error as ApiError).code).toBe(CLIENT_INTERNAL_ERROR);
    expect((error as ApiError).status).toBe(500);
    expect((error as ApiError).name).not.toBe('TypeError');
    expect((error as ApiError).name).not.toBe('SyntaxError');
  });

  it('TC-011-025：网络层失败 → client.NETWORK_ERROR', async () => {
    const { fetch: fake } = fakeFetch(() => {
      throw new TypeError('Failed to fetch');
    });
    const client = createApiClient({ baseUrl: '', fetch: fake });

    const error = await client.request({ method: 'GET', path: '/api/anything' }).then(
      () => undefined,
      (reason: unknown) => reason,
    );

    expect(error).toBeInstanceOf(ApiError);
    expect((error as ApiError).code).toBe(CLIENT_NETWORK_ERROR);
    expect((error as ApiError).status).toBe(0);
    expect((error as ApiError).name).not.toBe('TypeError');
  });

  it('TC-011-026：错误对象不携带原始响应体', async () => {
    const { fetch: fake } = fakeFetch(
      () => new Response('SECRET-UPSTREAM-DETAIL-XYZ', { status: 500 }),
    );
    const client = createApiClient({ baseUrl: '', fetch: fake });

    const error = (await client.request({ method: 'GET', path: '/api/anything' }).then(
      () => undefined,
      (reason: unknown) => reason,
    )) as ApiError;

    const inspected = [error.message, error.detail, error.title, JSON.stringify(error)];
    for (const text of inspected) {
      expect(text ?? '').not.toContain('SECRET-UPSTREAM-DETAIL-XYZ');
    }
    expect(error.code).toBe(CLIENT_INTERNAL_ERROR);
  });

  it('2xx + 非 JSON 体 → client.INTERNAL_ERROR（解析失败，不是原样返回）', async () => {
    const { fetch: fake } = fakeFetch(() => new Response('not-json', { status: 200 }));
    const client = createApiClient({ baseUrl: '', fetch: fake });

    const error = await client.request({ method: 'GET', path: '/api/anything' }).then(
      () => undefined,
      (reason: unknown) => reason,
    );

    expect((error as ApiError).code).toBe(CLIENT_INTERNAL_ERROR);
    expect((error as ApiError).status).toBe(200);
  });

  it('2xx + 空体 → 返回空值，不抛错', async () => {
    const { fetch: fake } = fakeFetch(() => new Response(null, { status: 204 }));
    const client = createApiClient({ baseUrl: '', fetch: fake });

    const result = await client.request({ method: 'GET', path: '/api/anything' });
    expect(result).toBeUndefined();
  });

  it('错误状态 + 空体 → client.INTERNAL_ERROR', async () => {
    const { fetch: fake } = fakeFetch(() => new Response(null, { status: 500 }));
    const client = createApiClient({ baseUrl: '', fetch: fake });

    const error = await client.request({ method: 'GET', path: '/api/anything' }).then(
      () => undefined,
      (reason: unknown) => reason,
    );

    expect((error as ApiError).code).toBe(CLIENT_INTERNAL_ERROR);
    expect((error as ApiError).status).toBe(500);
  });
});

describe('@base/sdk 的令牌刷新（AC-S-070）', () => {
  it('TC-011-027：401 → 刷新 → 重放 → 调用方拿到 200 的业务结果', async () => {
    let accessToken = 'expired-token';
    const { fetch: fake, calls } = fakeFetch(async (request) => {
      const path = pathOf(request);
      if (path === '/api/iam/refresh') {
        return jsonResponse({ accessToken: 'new-token', refreshToken: 'new-refresh' }, 200);
      }
      if (request.headers.get('authorization') === 'Bearer expired-token') {
        return jsonResponse({ code: 'TOKEN_EXPIRED', status: 401 }, 401);
      }
      return jsonResponse({ permissions: ['rbac:role:read'] }, 200);
    });

    const client = createApiClient({
      baseUrl: '',
      fetch: fake,
      getAccessToken: () => accessToken,
      refreshToken: async () => {
        const refreshed = await client.refresh<{ accessToken: string }>('/api/iam/refresh', {
          refreshToken: 'valid-refresh-token',
        });
        accessToken = refreshed.accessToken;
        return accessToken;
      },
    });

    const result = await client.request<{ permissions: string[] }>({
      method: 'GET',
      path: '/api/rbac/me/permissions',
    });

    expect(result).toEqual({ permissions: ['rbac:role:read'] });
    expect(calls.filter((item) => pathOf(item) === '/api/iam/refresh')).toHaveLength(1);
    expect(calls.filter((item) => pathOf(item) === '/api/rbac/me/permissions')).toHaveLength(2);
    const replay = calls.find(
      (item) =>
        pathOf(item) === '/api/rbac/me/permissions' &&
        item.headers.get('authorization') === 'Bearer new-token',
    );
    expect(replay).toBeDefined();
  });

  it('TC-011-028：5 个并发请求同时 401 → 刷新接口只被调用 1 次', async () => {
    let accessToken = 'expired-token';
    let releaseRefresh: ((response: Response) => void) | undefined;
    const refreshPending = new Promise<Response>((resolve) => {
      releaseRefresh = resolve;
    });
    const { fetch: fake, calls } = fakeFetch(async (request) => {
      const path = pathOf(request);
      if (path === '/api/iam/refresh') {
        return refreshPending;
      }
      if (request.headers.get('authorization') === 'Bearer expired-token') {
        return jsonResponse({ code: 'TOKEN_EXPIRED', status: 401 }, 401);
      }
      return jsonResponse({ ok: true }, 200);
    });

    const client = createApiClient({
      baseUrl: '',
      fetch: fake,
      getAccessToken: () => accessToken,
      refreshToken: async () => {
        const refreshed = await client.refresh<{ accessToken: string }>('/api/iam/refresh', {});
        accessToken = refreshed.accessToken;
        return accessToken;
      },
    });

    const pending = Array.from({ length: 5 }, () =>
      client.request<{ ok: boolean }>({ method: 'GET', path: '/api/business/1' }),
    );

    await vi.waitFor(() => {
      expect(calls.filter((item) => pathOf(item) === '/api/business/1')).toHaveLength(5);
      // 并发窗口内：5 个请求都撞上 401，刷新接口**只被调用 1 次**。
      expect(calls.filter((item) => pathOf(item) === '/api/iam/refresh')).toHaveLength(1);
    });

    releaseRefresh?.(jsonResponse({ accessToken: 'new-token' }, 200));

    const settled = await Promise.allSettled(pending);
    expect(settled.map((item) => item.status)).toEqual([
      'fulfilled',
      'fulfilled',
      'fulfilled',
      'fulfilled',
      'fulfilled',
    ]);
    expect(settled.map((item) => (item as PromiseFulfilledResult<{ ok: boolean }>).value)).toEqual(
      Array.from({ length: 5 }, () => ({ ok: true })),
    );
    expect(calls.filter((item) => pathOf(item) === '/api/iam/refresh')).toHaveLength(1);
    expect(calls.filter((item) => pathOf(item) === '/api/business/1')).toHaveLength(10);
  });

  it('TC-011-029：刷新失败 → 抛后端返回的码，不无限重试', async () => {
    const { fetch: fake, calls } = fakeFetch(async (request) => {
      const path = pathOf(request);
      if (path === '/api/iam/refresh') {
        return jsonResponse({ code: 'TOKEN_REVOKED', status: 401 }, 401);
      }
      return jsonResponse({ code: 'TOKEN_EXPIRED', status: 401 }, 401);
    });

    const client = createApiClient({
      baseUrl: '',
      fetch: fake,
      getAccessToken: () => 'expired-token',
      refreshToken: async () => {
        const refreshed = await client.refresh<{ accessToken: string }>('/api/iam/refresh', {});
        return refreshed.accessToken;
      },
    });

    const error = await client.request({ method: 'GET', path: '/api/business/1' }).then(
      () => undefined,
      (reason: unknown) => reason,
    );

    expect(error).toBeInstanceOf(ApiError);
    expect((error as ApiError).code).toBe('TOKEN_REVOKED');
    expect((error as ApiError).status).toBe(401);
    expect(calls.filter((item) => pathOf(item) === '/api/business/1')).toHaveLength(1);
    expect(calls.filter((item) => pathOf(item) === '/api/iam/refresh')).toHaveLength(1);
    expect(calls).toHaveLength(2);
  });

  it('TC-011-030：重放时原请求体逐字节相同', async () => {
    const businessBodies: Uint8Array[] = [];
    let accessToken = 'expired-token';
    const { fetch: fake } = fakeFetch(async (request) => {
      const path = pathOf(request);
      if (path === '/api/iam/refresh') {
        return jsonResponse({ accessToken: 'new-token' }, 200);
      }
      businessBodies.push(new Uint8Array(await request.clone().arrayBuffer()));
      if (request.headers.get('authorization') === 'Bearer expired-token') {
        return jsonResponse({ code: 'TOKEN_EXPIRED', status: 401 }, 401);
      }
      return jsonResponse({ ok: true }, 200);
    });

    const client = createApiClient({
      baseUrl: '',
      fetch: fake,
      getAccessToken: () => accessToken,
      refreshToken: async () => {
        const refreshed = await client.refresh<{ accessToken: string }>('/api/iam/refresh', {});
        accessToken = refreshed.accessToken;
        return accessToken;
      },
    });

    await client.request({ method: 'POST', path: '/api/business/1', body: { title: '订单 A' } });

    expect(businessBodies).toHaveLength(2);
    const [first, second] = businessBodies as [Uint8Array, Uint8Array];
    expect(first.length).toBe(second.length);
    expect(Array.from(first)).toEqual(Array.from(second));
    expect(new TextDecoder().decode(second)).toBe('{"title":"订单 A"}');
  });

  it('没有配置 refreshToken 时，401 直接按后端码抛出', async () => {
    const { fetch: fake, calls } = fakeFetch(() =>
      jsonResponse({ code: 'UNAUTHENTICATED', status: 401 }, 401),
    );
    const client = createApiClient({ baseUrl: '', fetch: fake });

    const error = await client.request({ method: 'GET', path: '/api/business/1' }).then(
      () => undefined,
      (reason: unknown) => reason,
    );

    expect((error as ApiError).code).toBe('UNAUTHENTICATED');
    expect(calls).toHaveLength(1);
  });
});

describe('@base/sdk 的幂等键（AC-S-071）', () => {
  it('TC-011-031：写方法自动注入 idempotency-key（UUID v4）', async () => {
    const { fetch: fake, calls } = fakeFetch(() => jsonResponse({ ok: true }, 200));
    const client = createApiClient({ baseUrl: '', fetch: fake });

    await client.request({ method: 'POST', path: '/api/a' });
    await client.request({ method: 'PUT', path: '/api/b' });
    await client.request({ method: 'DELETE', path: '/api/c' });

    expect(calls).toHaveLength(3);
    for (const call of calls) {
      const key = call.headers.get('idempotency-key');
      expect(key).not.toBeNull();
      expect(key).toMatch(UUID_V4);
    }
  });

  it('TC-011-032：读方法不注入 idempotency-key', async () => {
    const { fetch: fake, calls } = fakeFetch(() => jsonResponse({ ok: true }, 200));
    const client = createApiClient({ baseUrl: '', fetch: fake });

    await client.request({ method: 'GET', path: '/api/a' });
    await client.request({ method: 'HEAD', path: '/api/b' });

    expect(calls).toHaveLength(2);
    expect(calls[0]?.headers.get('idempotency-key')).toBeNull();
    expect(calls[1]?.headers.get('idempotency-key')).toBeNull();
  });

  it('TC-011-033：两次写请求的幂等键不同', async () => {
    const { fetch: fake, calls } = fakeFetch(() => jsonResponse({ ok: true }, 200));
    const client = createApiClient({ baseUrl: '', fetch: fake });

    await client.request({ method: 'POST', path: '/api/a', body: { x: 1 } });
    await client.request({ method: 'POST', path: '/api/a', body: { x: 2 } });

    const first = calls[0]?.headers.get('idempotency-key');
    const second = calls[1]?.headers.get('idempotency-key');
    expect(first).toMatch(UUID_V4);
    expect(second).toMatch(UUID_V4);
    expect(first).not.toBe(second);
    expect(await calls[0]?.clone().text()).toBe('{"x":1}');
    expect(await calls[1]?.clone().text()).toBe('{"x":2}');
  });

  it('TC-011-034：401 刷新后的重放请求不带 idempotency-key（D92）', async () => {
    let accessToken = 'expired-token';
    const { fetch: fake, calls } = fakeFetch(async (request) => {
      const path = pathOf(request);
      if (path === '/api/iam/refresh') {
        return jsonResponse({ accessToken: 'new-token' }, 200);
      }
      if (request.headers.get('authorization') === 'Bearer expired-token') {
        return jsonResponse({ code: 'TOKEN_EXPIRED', status: 401 }, 401);
      }
      return jsonResponse({ ok: true }, 200);
    });

    const client = createApiClient({
      baseUrl: '',
      fetch: fake,
      getAccessToken: () => accessToken,
      refreshToken: async () => {
        const refreshed = await client.refresh<{ accessToken: string }>('/api/iam/refresh', {});
        accessToken = refreshed.accessToken;
        return accessToken;
      },
    });

    const result = await client.request<{ ok: boolean }>({
      method: 'POST',
      path: '/api/iam/password/change',
      body: { old: 'a', new: 'b' },
    });

    expect(result).toEqual({ ok: true });
    const business = calls.filter((item) => pathOf(item) === '/api/iam/password/change');
    expect(business).toHaveLength(2);
    const firstKey = business[0]?.headers.get('idempotency-key');
    const replayKey = business[1]?.headers.get('idempotency-key');
    expect(firstKey).toMatch(UUID_V4);
    // D92：401 意味着请求没执行 → 重放不注入幂等键（否则后端返回 409 CONFLICT）。
    expect(replayKey).toBeNull();
  });
});

describe('@base/sdk 的上下文头（AC-S-072）', () => {
  it('TC-011-035：配置 tenantId 时注入 x-tenant-id', async () => {
    const { fetch: fake, calls } = fakeFetch(() => jsonResponse({ ok: true }, 200));
    const client = createApiClient({ baseUrl: '', tenantId: 'tenant-a', fetch: fake });

    await client.request({ method: 'GET', path: '/api/x' });

    expect(calls).toHaveLength(1);
    expect(calls[0]?.headers.get('x-tenant-id')).toBe('tenant-a');
  });

  it('TC-011-036：未配置 tenantId 时不注入 x-tenant-id', async () => {
    const { fetch: fake, calls } = fakeFetch(() => jsonResponse({ ok: true }, 200));
    const client = createApiClient({ baseUrl: '', fetch: fake });

    await client.request({ method: 'GET', path: '/api/x' });

    expect(calls).toHaveLength(1);
    expect(calls[0]?.headers.get('x-tenant-id')).toBeNull();
  });

  it('TC-011-037：配置 spaceId 时注入 x-space-id', async () => {
    const { fetch: fake, calls } = fakeFetch(() => jsonResponse({ ok: true }, 200));
    const client = createApiClient({
      baseUrl: '',
      tenantId: 'tenant-a',
      spaceId: 'space-sales-1',
      fetch: fake,
    });

    await client.request({ method: 'GET', path: '/api/x' });

    expect(calls[0]?.headers.get('x-space-id')).toBe('space-sales-1');
    expect(calls[0]?.headers.get('x-tenant-id')).toBe('tenant-a');
  });

  it('登录 / 刷新请求也带租户头', async () => {
    const { fetch: fake, calls } = fakeFetch(() => jsonResponse({ ok: true }, 200));
    const client = createApiClient({ baseUrl: '', tenantId: 'tenant-a', fetch: fake });

    await client.post('/api/iam/login', { username: 'u', password: 'p' });
    await client.refresh('/api/iam/refresh', { refreshToken: 'r' });

    expect(calls).toHaveLength(2);
    expect(calls[0]?.headers.get('x-tenant-id')).toBe('tenant-a');
    expect(calls[1]?.headers.get('x-tenant-id')).toBe('tenant-a');
  });

  it('setContext 更新上下文头', async () => {
    const { fetch: fake, calls } = fakeFetch(() => jsonResponse({ ok: true }, 200));
    const client = createApiClient({ baseUrl: '', fetch: fake });

    await client.request({ method: 'GET', path: '/api/x' });
    client.setContext({ tenantId: 'tenant-b', spaceId: 'space-1' });
    await client.request({ method: 'GET', path: '/api/x' });

    expect(calls[0]?.headers.get('x-tenant-id')).toBeNull();
    expect(calls[1]?.headers.get('x-tenant-id')).toBe('tenant-b');
    expect(calls[1]?.headers.get('x-space-id')).toBe('space-1');
  });
});

describe('@base/sdk 的请求构造', () => {
  it('query 里值为 undefined 的键不拼进 query string', async () => {
    const { fetch: fake, calls } = fakeFetch(() => jsonResponse({ ok: true }, 200));
    const client = createApiClient({ baseUrl: '', fetch: fake });

    await client.request({
      method: 'GET',
      path: '/api/x',
      query: { a: '1', b: undefined, c: 2, d: false },
    });

    const url = new URL(calls[0]?.url ?? '');
    expect(url.searchParams.get('a')).toBe('1');
    expect(url.searchParams.has('b')).toBe(false);
    expect(url.searchParams.get('c')).toBe('2');
    expect(url.searchParams.get('d')).toBe('false');
  });

  it('带令牌时写 authorization 头', async () => {
    const { fetch: fake, calls } = fakeFetch(() => jsonResponse({ ok: true }, 200));
    const client = createApiClient({
      baseUrl: '',
      fetch: fake,
      getAccessToken: () => 'token-1',
    });

    await client.get('/api/x');

    expect(calls[0]?.headers.get('authorization')).toBe('Bearer token-1');
  });
});
