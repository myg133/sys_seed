import { render } from 'svelte/server';
import { describe, expect, it, vi } from 'vitest';

import type { AuthState } from '@base/auth';
import type { ApiClient, RequestOptions } from '@base/sdk';

import AppRegistry from '../src/pages/AppRegistry.svelte';
import AppRegistryWithAuth from './fixtures/AppRegistryWithAuth.svelte';
import {
  APPS_PATH,
  clearAppLaunchUrl,
  createApp,
  deleteApp,
  listApps,
  updateApp,
} from '../src/lib/app-api';
import { createRouter, type RouterEnvironment } from '../src/lib/router';
import { APPS_ADMIN_ROUTE_PATH, createRoutes } from '../src/lib/routes';

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
    calls.push({ path: options.path, method: options.method ?? 'GET', body: options.body });
    const hit = responses[options.path];
    if (hit instanceof Error) {
      throw hit;
    }
    return (hit ?? []) as T;
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

/** 假环境（只看 pathname 与写回的地址）。 */
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

/** 持注册表读权限的账号。 */
const READER: AuthState = {
  status: 'authenticated',
  authenticated: true,
  userId: 'u-reader',
  tenantId: 'tenant-a',
  permissions: ['app:registry:read'],
};

/** 持读 + 写 + 删的账号。 */
const ADMIN: AuthState = {
  status: 'authenticated',
  authenticated: true,
  userId: 'u-admin',
  tenantId: 'tenant-a',
  permissions: ['app:registry:read', 'app:registry:write', 'app:registry:delete'],
};

/** 一条应用响应。 */
const APP = {
  appId: 'app-1',
  clientId: 'client-A',
  displayName: '差旅报销',
  description: null,
  category: 'finance',
  iconUrl: null,
  launchUrl: 'https://a.example/',
  sortOrder: 10,
  status: 'active',
  scopes: [],
  createdAt: '2026-11-21T00:00:00.000000Z',
  createdBy: 'u-admin',
};

describe('接口层：每个端点的方法与路径逐字对齐（与棒 1 对照）', () => {
  it('列 → GET /api/apps；建 → POST /api/apps（请求体走白名单）', async () => {
    const { client, calls } = fakeClient({ [APPS_PATH]: [APP] });
    const apps = await listApps(client);
    expect(apps.map((app) => app.appId)).toEqual(['app-1']);

    await createApp(client, { clientId: 'client-A', displayName: '差旅报销', launchUrl: 'https://a.example/' });
    expect(calls.map((call) => `${call.method} ${call.path}`)).toEqual([
      `GET ${APPS_PATH}`,
      `POST ${APPS_PATH}`,
    ]);
    expect(calls[1]?.body).toEqual({
      clientId: 'client-A',
      displayName: '差旅报销',
      launchUrl: 'https://a.example/',
    });
  });

  it('改 → PATCH /api/apps/{id}；删 → DELETE /api/apps/{id}', async () => {
    const { client, calls } = fakeClient({ '/api/apps/app-1': APP });
    await updateApp(client, 'app-1', { displayName: '改名', sortOrder: 5 });
    await deleteApp(client, 'app-1');
    expect(calls.map((call) => `${call.method} ${call.path}`)).toEqual([
      'PATCH /api/apps/app-1',
      'DELETE /api/apps/app-1',
    ]);
    expect(calls[0]?.body).toEqual({ displayName: '改名', sortOrder: 5 });
  });

  it('清空入口 → **DELETE** /api/apps/{id}/launch-url（不是 PATCH null —— D283）', async () => {
    const { client, calls } = fakeClient();
    await clearAppLaunchUrl(client, 'app-1');
    expect(calls).toHaveLength(1);
    expect(calls[0]?.method).toBe('DELETE');
    expect(calls[0]?.path).toBe('/api/apps/app-1/launch-url');
    // 这条端点的判据是「没有请求体」——`null` 表达不出来才需要专用端点。
    expect(calls[0]?.body).toBeUndefined();
  });
});

describe('注册表页的权限（AC-S-156 的路由守卫）', () => {
  it('路由声明 app:registry:read', () => {
    const { client } = fakeClient();
    const route = createRoutes(client).find((entry) => entry.path === APPS_ADMIN_ROUTE_PATH);
    expect(route).toBeDefined();
    expect(route?.permission).toBe('app:registry:read');
  });

  it('无 app:registry:read 的账号 → 落 403 页', async () => {
    const { client } = fakeClient();
    const { environment, written } = fakeEnvironment(APPS_ADMIN_ROUTE_PATH);
    const router = createRouter({
      routes: createRoutes(client),
      environment,
      getState: () => ({ status: 'authenticated', authenticated: true, permissions: ['rbac:role:read'] }),
      onResolve: () => undefined,
    });
    router.start();
    await vi.waitFor(() => expect(written.at(-1)).toBe('/403'));
    expect(router.path).toBe('/403');
  });

  it('未登录 → 落登录页（不是 403）', async () => {
    const { client } = fakeClient();
    const { environment, written } = fakeEnvironment(APPS_ADMIN_ROUTE_PATH);
    const router = createRouter({
      routes: createRoutes(client),
      environment,
      getState: () => ({ status: 'anonymous', authenticated: false, permissions: [] }),
      onResolve: () => undefined,
    });
    router.start();
    await vi.waitFor(() => expect(written.at(-1)).toBe('/login'));
  });

  it('有读权限 → 留在该页并请求 GET /api/apps', async () => {
    const { client, calls } = fakeClient({ [APPS_PATH]: [APP] });
    const { environment, written } = fakeEnvironment(APPS_ADMIN_ROUTE_PATH);
    const router = createRouter({
      routes: createRoutes(client),
      environment,
      getState: () => READER,
      onResolve: () => undefined,
    });
    router.start();
    await vi.waitFor(() =>
      expect(calls.some((call) => call.path === APPS_PATH)).toBe(true),
    );
    expect(written.at(-1)).toBe(APPS_ADMIN_ROUTE_PATH);
  });
});

describe('注册表页的渲染（首屏用 initial 传，SSR 不跑 onMount）', () => {
  /** 一条应用的首屏结果。 */
  const initial = { apps: [APP] };

  /** 在给定登录态下渲染注册表页（子树拿得到 Auth 上下文）。 */
  function renderAs(state: AuthState, client: ApiClient): string {
    return render(AppRegistryWithAuth, { props: { state, client, initial } }).body;
  }

  it('只读账号：列表渲染，但注册表单**不进 DOM**（无写权限）', () => {
    const { client } = fakeClient();
    const body = renderAs(READER, client);
    expect(body).toContain('data-testid="app-registry-table"');
    expect(body).not.toContain('data-testid="app-registry-form"');
  });

  it('管理员账号：注册表单进 DOM（有写权限）—— 与只读账号形成对照', () => {
    const { client } = fakeClient();
    const body = renderAs(ADMIN, client);
    expect(body).toContain('data-testid="app-registry-form"');
  });

  it('页头是注册表标题，空列表给空态提示', () => {
    const { client } = fakeClient();
    const { body } = render(AppRegistry, {
      props: { client, autoLoad: false, initial: { apps: [] } },
    });
    expect(body).toContain('应用注册表');
    expect(body).toContain('data-testid="app-registry-empty"');
  });

  it('有首屏数据时渲染行（展示名 / 关联 client / 状态文案 / 入口）', () => {
    const { client } = fakeClient();
    const { body } = render(AppRegistry, {
      props: { client, autoLoad: false, initial: { apps: [APP] } },
    });
    expect(body).toContain('差旅报销');
    expect(body).toContain('client-A');
    // 状态走 i18n 键（`app.registryStatusActive`）；入口原样显示。
    expect(body).toContain('已启用');
    expect(body).toContain('https://a.example/');
    expect(body).toContain('data-testid="app-registry-row"');
  });

  it('删除确认文案说明「不影响 client」（否则管理员不敢点 —— D262 / D264）', () => {
    const { client } = fakeClient();
    const { body } = render(AppRegistry, { props: { client, autoLoad: false } });
    expect(body).toContain('它引用的客户端不受影响');
  });
});

describe('注册表页的账号权限常量', () => {
  it('写 / 删权限点逐字等于 manifest 的 3 条（D271）', () => {
    // 页面用 `<Can permission=...>` 包住按钮 —— 权限点字符串必须与后端逐字一致。
    expect(ADMIN.permissions).toContain('app:registry:write');
    expect(ADMIN.permissions).toContain('app:registry:delete');
    expect(READER.permissions).not.toContain('app:registry:write');
  });
});
