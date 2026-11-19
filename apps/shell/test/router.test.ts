import { describe, expect, it } from 'vitest';

import type { AuthState } from '@base/auth';

import { createRouter, type RouteDefinition, type RouterEnvironment } from '../src/lib/router';

/** 已登录、只有 `rbac:role:read` 的用户（没有 `rbac:role:write`）。 */
const READ_ONLY: AuthState = {
  status: 'authenticated',
  authenticated: true,
  userId: 'u-1',
  tenantId: 'tenant-a',
  permissions: ['rbac:role:read'],
};

/** 拥有 `rbac:role:write` 的用户。 */
const WRITER: AuthState = {
  status: 'authenticated',
  authenticated: true,
  userId: 'u-2',
  tenantId: 'tenant-a',
  permissions: ['rbac:role:read', 'rbac:role:write'],
};

/** 未登录。 */
const ANONYMOUS: AuthState = {
  status: 'anonymous',
  authenticated: false,
  permissions: [],
};

/** 假环境：可换地址、可触发 popstate。 */
function fakeEnvironment(
  pathname: string,
  search = '',
): {
  environment: RouterEnvironment;
  written: string[];
  goTo: (path: string) => void;
} {
  let current = pathname;
  const written: string[] = [];
  const listeners: (() => void)[] = [];

  const environment: RouterEnvironment = {
    location: {
      get pathname(): string {
        return current;
      },
      get search(): string {
        return search;
      },
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
    addEventListener(_type: 'popstate', listener: () => void): void {
      listeners.push(listener);
    },
    removeEventListener(_type: 'popstate', listener: () => void): void {
      const index = listeners.indexOf(listener);
      if (index >= 0) {
        listeners.splice(index, 1);
      }
    },
  };

  return {
    environment,
    written,
    goTo(path: string): void {
      current = path;
      for (const listener of [...listeners]) {
        listener();
      }
    },
  };
}

/** 受保护路由（`load` 扮演「目标接口的请求」）。 */
function protectedRoute(): { routes: RouteDefinition[]; calls: () => number } {
  let calls = 0;
  const routes: RouteDefinition[] = [
    { path: '/', title: '工作台' },
    { path: '/login', title: '登录' },
    { path: '/403', title: '无权访问' },
    {
      path: '/admin/roles',
      title: '角色管理',
      permission: 'rbac:role:write',
      load: () => {
        calls += 1;
      },
    },
  ];
  return { routes, calls: () => calls };
}

describe('shell 的路由守卫（AC-S-068）', () => {
  it('TC-011-021：已登录但无权限 → 落 403 页，且目标接口的请求计数为 0', () => {
    const { routes, calls } = protectedRoute();
    const { environment, written } = fakeEnvironment('/admin/roles');
    const router = createRouter({
      routes,
      environment,
      getState: () => READ_ONLY,
      onResolve: () => undefined,
    });

    router.start();

    expect(written.at(-1)).toBe('/403');
    expect(router.path).toBe('/403');
    expect(calls()).toBe(0);
  });

  it('TC-011-020（应用级）：程序化跳转与深链的判定完全一致', () => {
    const { routes, calls } = protectedRoute();
    const { environment, written } = fakeEnvironment('/');
    const router = createRouter({
      routes,
      environment,
      getState: () => READ_ONLY,
      onResolve: () => undefined,
    });
    router.start();

    router.navigate('/admin/roles');

    expect(written.at(-1)).toBe('/403');
    expect(calls()).toBe(0);
  });

  it('TC-011-022：未登录 → 落登录页（不是 403），且目标接口的请求计数为 0', () => {
    const { routes, calls } = protectedRoute();
    const { environment, written } = fakeEnvironment('/admin/roles');
    const router = createRouter({
      routes,
      environment,
      getState: () => ANONYMOUS,
      onResolve: () => undefined,
    });

    router.start();

    expect(written.at(-1)).toBe('/login');
    expect(router.path).toBe('/login');
    expect(calls()).toBe(0);
  });

  it('有权限 → 放行并调用数据加载函数', () => {
    const { routes, calls } = protectedRoute();
    const { environment, written } = fakeEnvironment('/admin/roles');
    const router = createRouter({
      routes,
      environment,
      getState: () => WRITER,
      onResolve: () => undefined,
    });

    router.start();

    expect(written.at(-1)).toBe('/admin/roles');
    expect(router.path).toBe('/admin/roles');
    expect(calls()).toBe(1);
  });

  it('前进 / 后退（popstate）同样要过守卫', () => {
    const { routes, calls } = protectedRoute();
    const { environment, written, goTo } = fakeEnvironment('/');
    const router = createRouter({
      routes,
      environment,
      getState: () => READ_ONLY,
      onResolve: () => undefined,
    });
    router.start();

    goTo('/admin/roles');

    expect(written.at(-1)).toBe('/403');
    expect(router.path).toBe('/403');
    expect(calls()).toBe(0);
  });

  it('被拦截时保留原本要去的路径（登录后可以回去）', () => {
    const { routes } = protectedRoute();
    const { environment } = fakeEnvironment('/admin/roles');
    let intended: string | undefined;
    const router = createRouter({
      routes,
      environment,
      getState: () => ANONYMOUS,
      onResolve: (resolution) => {
        intended = resolution.intendedPath;
      },
    });

    router.start();

    expect(intended).toBe('/admin/roles');
  });

  it('同路径写回地址时保留查询串（REQ-013 / P4：/sso/callback?ticket= 不能被抹掉）', () => {
    const routes: RouteDefinition[] = [
      { path: '/', title: '工作台' },
      { path: '/sso/callback', title: '企业登录' },
    ];
    const { environment, written } = fakeEnvironment('/sso/callback', '?ticket=abc');
    const router = createRouter({
      routes,
      environment,
      getState: () => ANONYMOUS,
      onResolve: () => undefined,
    });

    router.start();

    // 路由只认 pathname，但写回时查询串必须原样带回（否则一次性 ticket 丢了）。
    expect(written.at(-1)).toBe('/sso/callback?ticket=abc');
    expect(router.path).toBe('/sso/callback');
  });

  it('公开路由不需要权限点', () => {
    const { routes, calls } = protectedRoute();
    const { environment, written } = fakeEnvironment('/');
    const router = createRouter({
      routes,
      environment,
      getState: () => ANONYMOUS,
      onResolve: () => undefined,
    });

    router.start();

    expect(written.at(-1)).toBe('/');
    expect(calls()).toBe(0);
  });
});
