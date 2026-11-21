import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

import { render } from 'svelte/server';
import { describe, expect, it } from 'vitest';

import type { ApiClient, RequestOptions } from '@base/sdk';

import Workbench from '../src/pages/Workbench.svelte';
import {
  type AppPageEnv,
  type AppView,
  type NavigationItem,
} from '../src/lib/app-api';
import { routesFromNavigation } from '../src/lib/nav';
import { createRouter, type RouterEnvironment } from '../src/lib/router';
import { createRoutes, WORKBENCH_ROUTE_PATH, APPS_ADMIN_ROUTE_PATH } from '../src/lib/routes';

/**
 * AC-S-163 的三条独立判据（D278 rev 4）：
 *
 * 1. **函数级**：`routesFromNavigation` 对 `internal` 产出路由、对 `external` 产出 0 条；
 * 2. **入口级**：`external` 项在工作台渲染成**整页跳转**入口，不经过 SPA 路由；
 * 3. **回归级**：`router.ts` 零改动（`git diff --stat` 见报告；这里用「源码无
 *    `routesFromNavigation` / 无 `addRoute`」+ 运行时 `pushState` 计数兜住）。
 *
 * ⚠️ 明确**不做**：「点导航项落到 SPA 内的新页面」（AC-S-163.b / TD-048）。
 */

/** 一条 module 导航项（站内路径）。 */
function moduleItem(target: string, label: string): NavigationItem {
  return {
    source: 'module',
    label,
    icon: null,
    target,
    kind: 'internal',
    requiredPermission: null,
  };
}

/** 一条 app 导航项（外部 URL）。 */
function appItem(target: string, label: string): NavigationItem {
  return {
    source: 'app',
    appId: 'app-1',
    label,
    icon: null,
    target,
    kind: 'external',
    requiredPermission: null,
  };
}

/** 假客户端：按路径返回预置响应，并记录调用。 */
function fakeClient(responses: Record<string, unknown> = {}): {
  client: ApiClient;
  calls: { path: string; method: string }[];
} {
  const calls: { path: string; method: string }[] = [];
  const request = async <T>(options: RequestOptions): Promise<T> => {
    calls.push({ path: options.path, method: options.method ?? 'GET' });
    return (responses[options.path] ?? []) as T;
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

/** 记录跳转的假环境（整页跳转 = `assign`，**不是** `pushState`）。 */
function fakeEnv(): { env: AppPageEnv; assigned: string[] } {
  const assigned: string[] = [];
  return {
    env: {
      assign(url: string): void {
        assigned.push(url);
      },
    },
    assigned,
  };
}

/** 按 `data-kind="external"` 抓出外部项（判据不依赖文案）。 */
function externalHrefs(html: string): string[] {
  return [...html.matchAll(/<a[^>]*data-kind="external"[^>]*>/g)].map((match) => {
    const href = /href="([^"]*)"/.exec(match[0]);
    return href?.[1] ?? '';
  });
}

/* -------------------------------------------------------------------------- */
/* 判据 1：函数级                                                              */
/* -------------------------------------------------------------------------- */

describe('routesFromNavigation —— 函数级（AC-S-163 步骤 1 / 2）', () => {
  it('2 个 internal 项 → 恰好 2 条路由，path 逐字等于 target、title 等于 label 文案', () => {
    const routes = routesFromNavigation([
      moduleItem('/a', 'A'),
      moduleItem('/b', 'B'),
      appItem('https://x.example/', 'X'),
    ]);
    expect(routes).toHaveLength(2);
    expect(routes[0]?.path).toBe('/a');
    expect(routes[0]?.title).toBe('A');
    expect(routes[1]?.path).toBe('/b');
    expect(routes[1]?.title).toBe('B');
  });

  it('只传 1 个 external 项 → 空数组（这条是关键：外部 URL 进路由表会让 match() 静默失败）', () => {
    expect(routesFromNavigation([appItem('https://x.example/', 'X')])).toEqual([]);
  });

  it('title 用 label 的文案而不是翻译键（D284：MenuSpec 里没有 labelKey）', () => {
    const routes = routesFromNavigation([moduleItem('/admin/audit', '审计日志')]);
    expect(routes[0]?.title).toBe('审计日志');
    // 不是 `nav.audit` 这种键（后者会被渲染层再翻一次）。
    expect(routes[0]?.title).not.toMatch(/^[a-z]+\.[a-z]/);
  });

  it('空输入 → 空数组；不产出 load / permission（没有可渲染的组件，见 AC-S-163.b）', () => {
    expect(routesFromNavigation([])).toEqual([]);
    const [route] = routesFromNavigation([moduleItem('/a', 'A')]);
    expect(route).not.toHaveProperty('load');
    expect(route).not.toHaveProperty('permission');
  });

  it('路由表里**没有**对外部 URL 的条目（把导航拼进路由表也无从发生）', () => {
    const { client } = fakeClient();
    const routes = createRoutes(client);
    const external = routesFromNavigation([appItem('https://x.example/', 'X')]);
    const merged = [...routes, ...external];
    expect(merged.some((route) => route.path.startsWith('http'))).toBe(false);
  });
});

/* -------------------------------------------------------------------------- */
/* 判据 2：入口级                                                              */
/* -------------------------------------------------------------------------- */

describe('工作台把 external 项渲染成整页跳转入口（AC-S-163 步骤 3 / D278）', () => {
  it('external 项的 href 逐字等于 target（外部 URL 直接进 href）', () => {
    const { client } = fakeClient();
    const { env } = fakeEnv();
    const { body } = render(Workbench, {
      props: {
        client,
        env,
        autoLoad: false,
        navigation: [appItem('https://x.example/', '第三方应用')],
      },
    });
    expect(externalHrefs(body)).toEqual(['https://x.example/']);
    // 文案直接渲染（不过 t()：MenuSpec 只有 label —— TD-049）。
    expect(body).toContain('第三方应用');
  });

  it('internal 项是站内链接（href 以 / 开头），且与 external 项用不同的入口形态', () => {
    const { client } = fakeClient();
    const { env } = fakeEnv();
    const { body } = render(Workbench, {
      props: {
        client,
        env,
        autoLoad: false,
        navigation: [moduleItem('/admin/audit', '审计日志'), appItem('https://x.example/', 'X')],
      },
    });
    expect(body).toContain('href="/admin/audit"');
    expect(externalHrefs(body)).toEqual(['https://x.example/']);
  });

  it('整页跳转**不经过** SPA 路由：`/workbench` 之外的路径不写历史、不 resolve 外部 URL', () => {
    // 把 external 的 target 当路径交给路由器 —— 它**不**是路由表里的条目，
    // 所以匹配不到（`route` 为 undefined），这正是「不该进路由表」的运行时证据。
    const { client } = fakeClient();
    let current = WORKBENCH_ROUTE_PATH;
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
    const router = createRouter({
      routes: createRoutes(client),
      environment,
      getState: () => ({ status: 'authenticated', authenticated: true, permissions: [] }),
      onResolve: () => undefined,
    });
    const resolution = router.resolve('https://x.example/');
    expect(resolution.route).toBeUndefined();
    // 放行是「无路由」的默认行为 —— 所以它落到 `<Home/>`，**不会**跳出去。
    expect(resolution.allowed).toBe(true);
    // 外部跳转由 `env.assign` 承担（整页导航），不是 `history.pushState`。
    expect(written).toEqual([]);
  });

  it('scheme 非法的外部项：不渲染成链接（只保留文案），且 href 里绝不出现 javascript:', () => {
    const { client } = fakeClient();
    const { env } = fakeEnv();
    const { body } = render(Workbench, {
      props: {
        client,
        env,
        autoLoad: false,
        navigation: [
          { ...appItem('javascript:alert(1)', '恶意入口') },
          appItem('https://ok.example/', '正常入口'),
        ],
      },
    });
    // 合法的那条进 href；非法的那条**不进** `href`（打开即执行脚本）。
    expect(externalHrefs(body)).toEqual(['https://ok.example/']);
    expect(body).not.toContain('javascript:alert(1)');
    // 非法项仍保留文案 + 明确的「被挡」标记（不静默丢掉这一项）。
    expect(body).toContain('恶意入口');
    expect(body).toContain('data-kind="external-blocked"');
  });
});

/* -------------------------------------------------------------------------- */
/* 回归级：router.ts 不被引入导航相关能力                                        */
/* -------------------------------------------------------------------------- */

describe('router.ts 零改动（AC-S-163 步骤 4 / D270）', () => {
  /** 路由核心源码（`apps/shell/test` → `../src/lib/router.ts`）。 */
  const routerSource = readFileSync(
    fileURLToPath(new URL('../src/lib/router.ts', import.meta.url)),
    'utf8',
  );

  it('Router 没有 addRoute / setRoutes（运行时可注入路由仍然做不到）', () => {
    expect(routerSource).not.toMatch(/addRoute|setRoutes/);
  });

  it('router.ts 不 import 任何导航 / 应用模块（导航不进路由核心）', () => {
    expect(routerSource).not.toMatch(/from '\.\/nav'/);
    expect(routerSource).not.toMatch(/from '\.\/app-api'/);
  });

  it('createRouter 仍然捕获 routes 数组引用（语义未变：重赋值不生效）', () => {
    expect(routerSource).toContain('const { routes, environment, getState, onResolve } = options;');
  });

  it('工作台 / 注册表两条新路由在路由表里（新页面是**编译期**登记的，不是运行时注入）', () => {
    const { client } = fakeClient();
    const paths = createRoutes(client).map((route) => route.path);
    expect(paths).toContain(WORKBENCH_ROUTE_PATH);
    expect(paths).toContain(APPS_ADMIN_ROUTE_PATH);
  });

  it('注册表路由声明 app:registry:read；工作台只要求登录（不声明权限点）', () => {
    const { client } = fakeClient();
    const routes = createRoutes(client);
    const admin = routes.find((route) => route.path === APPS_ADMIN_ROUTE_PATH);
    expect(admin?.permission).toBe('app:registry:read');
    const workbench = routes.find((route) => route.path === WORKBENCH_ROUTE_PATH);
    expect(workbench?.permission).toBeUndefined();
    expect(workbench?.requiresAuth).toBe(true);
  });
});

/* -------------------------------------------------------------------------- */
/* 工作台：不按 launch_url 过滤（D279 = 方案 A）                                 */
/* -------------------------------------------------------------------------- */

describe('工作台渲染（AC-S-160 的展示面）', () => {
  /** 造一条应用（渲染断言只用 displayName / sortOrder / launchUrl / category）。 */
  function view(overrides: Partial<AppView> = {}): AppView {
    return {
      appId: 'a',
      clientId: 'client-A',
      displayName: 'A',
      description: null,
      category: 'finance',
      iconUrl: null,
      launchUrl: 'https://a.example/',
      sortOrder: 0,
      status: 'active',
      scopes: [],
      createdAt: '2026-11-21T00:00:00.000000Z',
      createdBy: 'u-admin',
      ...overrides,
    };
  }

  it('分组按 category 升序、组内按 (sortOrder, displayName) 渲染（后端顺序打乱也排好）', () => {
    const { client } = fakeClient();
    const { env } = fakeEnv();
    const { body } = render(Workbench, {
      props: {
        client,
        env,
        autoLoad: false,
        initial: {
          groups: [
            // 故意给「乱序」的输入：general 在前、finance 组内 sortOrder 倒序。
            {
              category: 'general',
              apps: [
                // ⚠️ 输入顺序是 `D` 在前、`C` 在后（与期望输出**相反**）——
                // 否则「并列 sortOrder 时按 displayName」这条判据没有区分力：
                // 稳定排序会原样保留输入顺序 → 去掉次级排序也照样绿。
                view({ appId: 'd', displayName: 'D 应用', category: 'general', sortOrder: 0 }),
                view({ appId: 'c', displayName: 'C 应用', category: 'general', sortOrder: 0 }),
              ],
            },
            {
              category: 'finance',
              apps: [
                view({ appId: 'a', displayName: '差旅报销', sortOrder: 20 }),
                view({ appId: 'b', displayName: '费用申请', sortOrder: 10 }),
              ],
            },
          ],
          recent: [],
          frequent: [],
        },
      },
    });
    const categories = [...body.matchAll(/data-testid="app-workspace-category">([^<]*)</g)].map(
      (match) => match[1],
    );
    expect(categories).toEqual(['finance', 'general']);
    const names = [...body.matchAll(/class="base-workbench-card-name">([^<]*)</g)].map(
      (match) => match[1],
    );
    // finance: sortOrder 10 的「费用申请」在前；general: 两个 0 按 displayName（C→D）。
    expect(names).toEqual(['费用申请', '差旅报销', 'C 应用', 'D 应用']);
  });

  it('没有 launch_url 的应用**仍渲染**在分组里，只是没有可点入口（D279 方案 A）', () => {
    const { client } = fakeClient();
    const { env } = fakeEnv();
    const { body } = render(Workbench, {
      props: {
        client,
        env,
        autoLoad: false,
        initial: {
          groups: [
            {
              category: 'general',
              apps: [
                view({ appId: 'no-entry', displayName: '无入口应用', launchUrl: null }),
                view({ appId: 'has-entry', displayName: '有入口应用', launchUrl: 'https://x.example/' }),
              ],
            },
          ],
          recent: [],
          frequent: [],
        },
      },
    });
    // 两张卡片都在（工作台不按 launch_url 过滤）。
    expect(body).toContain('无入口应用');
    expect(body).toContain('有入口应用');
    // 只有有入口的那张渲染 `app-workspace-open`；另一张给「未配置入口」提示。
    const opens = [...body.matchAll(/data-testid="app-workspace-open"/g)];
    expect(opens).toHaveLength(1);
    expect(body).toContain('app-workspace-no-entry');
  });

  it('scheme 非法（javascript:）的入口在卡片上**不渲染**可点链接（打开即 XSS 的前端防线）', () => {
    const { client } = fakeClient();
    const { env } = fakeEnv();
    const { body } = render(Workbench, {
      props: {
        client,
        env,
        autoLoad: false,
        initial: {
          groups: [
            {
              category: 'general',
              apps: [view({ appId: 'evil', displayName: '恶意应用', launchUrl: 'javascript:alert(1)' })],
            },
          ],
          recent: [],
          frequent: [],
        },
      },
    });
    expect(body).toContain('恶意应用');
    expect(body).not.toContain('javascript:alert(1)');
    expect(body).toContain('app-workspace-no-entry');
  });

  it('最近访问 / 常用应用各自渲染（空列表各自给出提示）', () => {
    const { client } = fakeClient();
    const { env } = fakeEnv();
    const { body } = render(Workbench, {
      props: {
        client,
        env,
        autoLoad: false,
        initial: {
          groups: [],
          recent: [view({ appId: 'r1', displayName: '最近用的' })],
          frequent: [],
        },
      },
    });
    expect(body).toContain('最近用的');
    expect(body).toContain('data-testid="app-recent-item"');
    // frequent 是空的 → 提示文案（`app.sectionEmpty`）。
    expect(body).toContain('暂无访问记录');
  });

  it('分组为空时给空态提示（不是白屏）', () => {
    const { client } = fakeClient();
    const { env } = fakeEnv();
    const { body } = render(Workbench, {
      props: {
        client,
        env,
        autoLoad: false,
        initial: { groups: [], recent: [], frequent: [] },
      },
    });
    expect(body).toContain('data-testid="app-workbench-empty"');
  });
});
