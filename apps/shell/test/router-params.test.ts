import { describe, expect, it } from 'vitest';

import type { AuthState } from '@base/auth';

import {
  createRouter,
  type RouteDefinition,
  type RouterEnvironment,
  type RouteResolution,
} from '../src/lib/router';

/** 已登录且拥有 `rbac:role:write` 的用户。 */
const WRITER: AuthState = {
  status: 'authenticated',
  authenticated: true,
  userId: 'u-1',
  tenantId: 'tenant-a',
  permissions: ['rbac:role:read', 'rbac:role:write'],
};

/** 已登录但只有一个无关权限点（通配路由只要求「已登录」）。 */
const PLAIN: AuthState = {
  status: 'authenticated',
  authenticated: true,
  userId: 'u-2',
  tenantId: 'tenant-a',
  permissions: [],
};

/** 未登录。 */
const ANONYMOUS: AuthState = {
  status: 'anonymous',
  authenticated: false,
  permissions: [],
};

/** 假环境：可换地址、可触发 popstate。 */
function fakeEnvironment(pathname: string): {
  environment: RouterEnvironment;
  written: string[];
} {
  let current = pathname;
  const written: string[] = [];
  const listeners: (() => void)[] = [];

  return {
    written,
    environment: {
      location: {
        get pathname(): string {
          return current;
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
        listeners.splice(listeners.indexOf(listener), 1);
      },
    },
  };
}

/** 路由表：一条精确路由 + 一条通配路由（同形，用来验证优先级）。 */
function routes(): RouteDefinition[] {
  return [
    { path: '/', title: '工作台' },
    { path: '/login', title: '登录' },
    { path: '/403', title: '无权访问' },
    { path: '/schema/handmade/one', title: '精确页' },
    {
      path: '/schema/:moduleId/:entityKey',
      title: '通配页',
      requiresAuth: true,
      load: () => undefined,
    },
    { path: '/secure/:id', title: '受保护通配', permission: 'rbac:role:write' },
  ];
}

/** 只做解析，不启动监听。 */
function resolver(state: AuthState, path: string): RouteResolution {
  const { environment } = fakeEnvironment('/');
  const router = createRouter({
    routes: routes(),
    environment,
    getState: () => state,
    onResolve: () => undefined,
  });
  return router.resolve(path);
}

describe('router 的 :param 扩展（D104 / AC-S-085）', () => {
  it('精确路径仍按精确匹配（REQ-011 的现有路由不受影响）', () => {
    const resolution = resolver(WRITER, '/');
    expect(resolution.route?.title).toBe('工作台');
    expect(resolution.params).toEqual({});
    expect(resolution.allowed).toBe(true);
  });

  it('通配路由提取参数：/schema/a/b → { moduleId: a, entityKey: b }', () => {
    const resolution = resolver(PLAIN, '/schema/sandbox/demoItem');
    expect(resolution.route?.title).toBe('通配页');
    expect(resolution.params).toEqual({ moduleId: 'sandbox', entityKey: 'demoItem' });
    expect(resolution.allowed).toBe(true);
  });

  it('精确优先于通配（同形路径命中精确路由）', () => {
    const resolution = resolver(WRITER, '/schema/handmade/one');
    expect(resolution.route?.title).toBe('精确页');
    expect(resolution.params).toEqual({});
  });

  it('参数段数量不同 → 不命中', () => {
    const resolution = resolver(PLAIN, '/schema/sandbox');
    expect(resolution.route).toBeUndefined();
    expect(resolution.params).toEqual({});
  });

  it('静态段不匹配 → 不命中', () => {
    const resolution = resolver(PLAIN, '/other/sandbox/demoItem');
    expect(resolution.route).toBeUndefined();
  });

  it('参数值做百分号解码（非法编码保留原文，不抛错）', () => {
    expect(resolver(PLAIN, '/schema/sandbox/demo%20Item').params['entityKey']).toBe('demo Item');
    expect(resolver(PLAIN, '/schema/sandbox/%E0%A4%A').params['entityKey']).toBe('%E0%A4%A');
  });

  it('通配路由的权限点仍按静态声明判定（无权限 → 403）', () => {
    const resolution = resolver(PLAIN, '/secure/item-1');
    expect(resolution.route?.title).toBe('受保护通配');
    expect(resolution.allowed).toBe(false);
    expect(resolution.redirectTo).toBe('/403');
    expect(resolution.params).toEqual({ id: 'item-1' });
  });

  it('requiresAuth：未登录 → 落登录页；已登录（无任何权限点）→ 放行', () => {
    const anonymous = resolver(ANONYMOUS, '/schema/sandbox/demoItem');
    expect(anonymous.allowed).toBe(false);
    expect(anonymous.redirectTo).toBe('/login');

    const plain = resolver(PLAIN, '/schema/sandbox/demoItem');
    expect(plain.allowed).toBe(true);
  });

  it('导航到通配路径时 onResolve 拿到参数，且放行后调用 load', () => {
    let calls = 0;
    const { environment, written } = fakeEnvironment('/');
    let resolved: RouteResolution | undefined;
    const router = createRouter({
      routes: [
        {
          path: '/schema/:moduleId/:entityKey',
          title: '通配页',
          load: () => {
            calls += 1;
          },
        },
      ],
      environment,
      getState: () => PLAIN,
      onResolve: (resolution) => {
        resolved = resolution;
      },
    });
    router.start();
    router.navigate('/schema/sandbox/demoItem');

    expect(written.at(-1)).toBe('/schema/sandbox/demoItem');
    expect(resolved?.params).toEqual({ moduleId: 'sandbox', entityKey: 'demoItem' });
    expect(calls).toBe(1);
  });
});
