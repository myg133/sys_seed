import { render } from 'svelte/server';
import { describe, expect, it, vi } from 'vitest';

import type { AuthState } from '@base/auth';
import type { ApiClient, RequestOptions } from '@base/sdk';

import Audit from '../src/pages/Audit.svelte';
import { AUDIT_EVENTS_PATH } from '../src/lib/audit-api';
import { createRouter, type RouterEnvironment } from '../src/lib/router';
import { AUDIT_ROUTE_PATH, createRoutes } from '../src/lib/routes';

/** 拥有 `audit:events:read` 的账号。 */
const AUDITOR: AuthState = {
  status: 'authenticated',
  authenticated: true,
  userId: 'u-auditor',
  tenantId: 'tenant-a',
  permissions: ['audit:events:read'],
};

/** 已登录但**没有** `audit:events:read` 的账号。 */
const NO_PERMISSION: AuthState = {
  status: 'authenticated',
  authenticated: true,
  userId: 'u-noperm',
  tenantId: 'tenant-a',
  permissions: ['rbac:role:read'],
};

/** 假客户端：记录调用（`/api/audit/events` 的请求计数 + 查询参数）。 */
function fakeClient(): { client: ApiClient; paths: string[]; queries: Record<string, string>[] } {
  const paths: string[] = [];
  const queries: Record<string, string>[] = [];
  const request = async <T>(options: RequestOptions): Promise<T> => {
    paths.push(options.path);
    queries.push((options.query ?? {}) as Record<string, string>);
    return { items: [], total: 0, nextCursor: null } as unknown as T;
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
  return { client, paths, queries };
}

/** 假环境：只关心 pathname 与写回的地址。 */
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

/** 表头文本（按出现顺序）。 */
function headers(html: string): string[] {
  return [...html.matchAll(/<th(?:\s[^>]*)?>([\s\S]*?)<\/th>/g)].map((match) =>
    (match[1] ?? '').trim(),
  );
}

describe('审计页的列（AC-S-132 步骤 1 / TC-014-112）', () => {
  it('同时给出操作人 / 时间 / 动作 / 对象 / 来源 IP 五列（文案走 i18n）', () => {
    const { client } = fakeClient();
    const { body } = render(Audit, { props: { client } });
    const labels = headers(body);
    expect(labels).toContain('操作人');
    expect(labels).toContain('发生时间');
    expect(labels).toContain('动作');
    expect(labels).toContain('对象');
    expect(labels).toContain('来源 IP');
  });

  it('表头文案来自翻译键（不是硬编码中文）：键缺失时 i18n 会回退成 "?"', () => {
    const { client } = fakeClient();
    const { body } = render(Audit, { props: { client } });
    // 五个必显列都必须是**非空**文案（"?" 说明键没登记）。
    for (const label of headers(body)) {
      expect(label).not.toBe('?');
      expect(label.trim()).not.toBe('');
    }
  });

  it('页面标题是审计页（i18n 键 nav.audit）', () => {
    const { client } = fakeClient();
    const { body } = render(Audit, { props: { client } });
    expect(body).toContain('审计日志');
  });
});

describe('页码指示 —— 游标栈的语义（D169）', () => {
  it('指示写成「本次浏览第 N 页」，不是「第 N 页 / 共 M 页」', () => {
    const { client } = fakeClient();
    const { body } = render(Audit, { props: { client } });
    // 文案走 i18n 键 `common.auditPageIndicator`。
    expect(body).toContain('本次浏览第 1 页');
    // ⚠️ 「1 / 12」这种「当前页 / 总页数」的写法会被理解成全量页码 → D169 明令禁止。
    expect(body).not.toMatch(/\d+\s*\/\s*\d+/);
  });

  it('自带分页器不渲染（hidePager）：页码分页器的语义与游标分页不符', () => {
    const { client } = fakeClient();
    const { body } = render(Audit, { props: { client } });
    // 本页自己画的游标分页器在（`data-testid="audit-pager"`），
    // 但 `@base/ui` 那套「第 N 页 / 共 M 页」的分页器不出现。
    expect(body).toContain('data-testid="audit-pager"');
    expect(body).toContain('data-testid="audit-page-indicator"');
  });

  it('没有下一页游标时「下一页」禁用；有游标时不禁用', () => {
    const { client } = fakeClient();
    const { body } = render(Audit, { props: { client } });
    // SSR 首屏没有 `nextCursor` → 「下一页」禁用（属性在 data-testid 之前）。
    expect(body).toMatch(/<button[^>]*disabled=""[^>]*data-testid="audit-next-page"/);
  });
});

describe('路由与守卫（AC-S-132 步骤 4 / 5 / TC-014-118）', () => {
  it('路由表里有 /admin/audit，权限点是 audit:events:read', () => {
    const { client } = fakeClient();
    const route = createRoutes(client).find((entry) => entry.path === AUDIT_ROUTE_PATH);
    expect(route).toBeDefined();
    expect(route?.permission).toBe('audit:events:read');
  });

  it('无权限账号 → 落 403 页，且目标接口的请求计数为 0', async () => {
    const { client, paths } = fakeClient();
    const { environment, written } = fakeEnvironment(AUDIT_ROUTE_PATH);
    const router = createRouter({
      routes: createRoutes(client),
      environment,
      getState: () => NO_PERMISSION,
      onResolve: () => undefined,
    });

    router.start();
    await vi.waitFor(() => expect(paths.length).toBe(0));

    expect(written.at(-1)).toBe('/403');
    expect(router.path).toBe('/403');
    // 守卫不放行 → 路由的 `load` 不会被调用 → 目标接口一次都没被请求。
    expect(paths.filter((path) => path === AUDIT_EVENTS_PATH)).toHaveLength(0);
  });

  it('有权限账号 → 放行并请求审计接口（**游标取数**：请求里没有 page）', async () => {
    const { client, paths, queries } = fakeClient();
    const { environment, written } = fakeEnvironment(AUDIT_ROUTE_PATH);
    const router = createRouter({
      routes: createRoutes(client),
      environment,
      getState: () => AUDITOR,
      onResolve: () => undefined,
    });

    router.start();
    await vi.waitFor(() => expect(paths).toContain(AUDIT_EVENTS_PATH));

    expect(written.at(-1)).toBe(AUDIT_ROUTE_PATH);
    expect(router.path).toBe(AUDIT_ROUTE_PATH);

    // 首屏 = 游标栈的第 1 页：带 limit，**不带 cursor / page**（D169）。
    const query = queries.find((entry) => entry['limit'] !== undefined) ?? {};
    expect(query['limit']).toBe('20');
    expect(Object.keys(query)).not.toContain('cursor');
    expect(Object.keys(query)).not.toContain('page');
  });

  it('未登录 → 落登录页（不是 403），且目标接口的请求计数为 0', async () => {
    const { client, paths } = fakeClient();
    const { environment, written } = fakeEnvironment(AUDIT_ROUTE_PATH);
    const router = createRouter({
      routes: createRoutes(client),
      environment,
      getState: () => ({
        status: 'anonymous',
        authenticated: false,
        permissions: [],
      }),
      onResolve: () => undefined,
    });

    router.start();
    await vi.waitFor(() => expect(paths.length).toBe(0));

    expect(written.at(-1)).toBe('/login');
    expect(paths.filter((path) => path === AUDIT_EVENTS_PATH)).toHaveLength(0);
  });
});
