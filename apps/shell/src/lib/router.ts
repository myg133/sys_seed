/**
 * 极简路由（History API）—— **不引路由库**（本期零新依赖）。
 *
 * # 为什么自己写
 *
 * 本仓前端没有路由库依赖（无 `svelte-routing` / `@sveltejs/kit`）；
 * 服务端已支持 SPA fallback（路径无扩展名 → 回落 `index.html`），
 * 所以**深链能到达前端** → 守卫必须在**深链**上也生效。
 *
 * # 守卫在导航**之前**完成
 *
 * 命中受保护路由时：
 *
 * 1. 先用 `@base/auth` 的 `guard()` 判定；
 * 2. 不放行 → 把地址改写成落点（登录页 / 403 页），**目标页的 `load` 不会被调用**
 *    → 目标接口的请求计数为 **0**；
 * 3. 放行 → 才调用目标页的 `load`。
 *
 * 环境（`location` / `history` / 事件监听）**可注入** → 单测不需要真实 DOM。
 *
 * # `:param` 通配（D104，REQ-012）
 *
 * 这是对 REQ-011 产物的**扩展**，不是破坏：
 *
 * - **精确路径仍精确匹配** —— 现有路由的语义与行为一字不改；
 * - **精确优先于通配**：先找 `route.path === path`，再按声明顺序找含 `:param` 的；
 * - 通配段的值进 `RouteResolution.params`；
 * - `permission` 仍按**静态声明**判定（通配路由不能按参数拼权限点 ——
 *   那会让「权限点」变成运行时字符串拼接，后端对账无法覆盖）。
 *
 * # 查询串必须原样保留（REQ-013 / P4）
 *
 * 判定只看 `pathname`，写回地址时**同路径**要带上原来的 `search` / `hash`。
 * 少这一步，`/sso/callback?ticket=…` 在 `start()` 里会被改写成 `/sso/callback`，
 * 回调页再也读不到 ticket（浏览器实测发现，单测里只传 pathname 是看不到的）。
 */

import { guard, isAuthenticated, type AuthState, type GuardOptions } from '@base/auth';

/** 路由定义。 */
export interface RouteDefinition {
  /** 路径（可以含 `:param` 段，如 `/schema/:moduleId/:entityKey`）。 */
  readonly path: string;
  /** 页面标题。 */
  readonly title: string;
  /** 访问该路由需要的权限点（没有 = 公开路由）。 */
  readonly permission?: string;
  /**
   * 只要求「已登录」，不要求具体权限点。
   *
   * 通配路由（`/schema/:moduleId/:entityKey`）用它：实体级权限点由后端按元数据 +
   * 字段过滤裁决，前端没有一个**静态**权限点能表达「任意模块的任意实体」。
   * 未登录 → 落登录页（与 `permission` 守卫一致）。
   */
  readonly requiresAuth?: boolean;
  /** 页面数据加载函数（**守卫放行后**才被调用）。 */
  readonly load?: () => void | Promise<void>;
}

/** 路由环境（浏览器用 `window`，单测用假件）。 */
export interface RouterEnvironment {
  /**
   * 地址栏。
   *
   * `search` / `hash` 是**可选**的（REQ-013 新增）：路由只按 `pathname` 匹配，
   * 但写回地址时必须原样保留查询串 —— 否则 `history.replaceState(null, '', '/sso/callback')`
   * 会把 `?ticket=` 抹掉（P4 的一次性凭据就丢在跳转前）。
   */
  readonly location: {
    readonly pathname: string;
    readonly search?: string;
    readonly hash?: string;
  };
  readonly history: {
    pushState(state: unknown, title: string, url: string): void;
    replaceState(state: unknown, title: string, url: string): void;
  };
  addEventListener(type: 'popstate', listener: () => void): void;
  removeEventListener(type: 'popstate', listener: () => void): void;
}

/** 一次导航的解析结果。 */
export interface RouteResolution {
  /** 最终落点路径（被拦截时是落点路径）。 */
  readonly path: string;
  /** 命中的路由（没命中时为 `undefined`）。 */
  readonly route?: RouteDefinition;
  /** 从 `:param` 段提取的参数（精确匹配时为空对象）。 */
  readonly params: Readonly<Record<string, string>>;
  /** 是否放行。 */
  readonly allowed: boolean;
  /** 拒绝时的落点（登录页 / 403 页）。 */
  readonly redirectTo?: string;
  /** 被拦截时**原本**要去的路径（登录后可以回去）。 */
  readonly intendedPath?: string;
}

/** 路由器的构造参数。 */
export interface RouterOptions {
  /** 路由表。 */
  readonly routes: readonly RouteDefinition[];
  /** 运行环境。 */
  readonly environment: RouterEnvironment;
  /** 当前登录态（守卫判定的输入）。 */
  readonly getState: () => AuthState;
  /** 导航结果回调（应用据此渲染）。 */
  readonly onResolve: (resolution: RouteResolution) => void;
  /** 守卫落点配置（默认 `/login` 与 `/403`）。 */
  readonly guardOptions?: GuardOptions;
}

/** 路由器。 */
export interface Router {
  /** 当前路径。 */
  readonly path: string;
  /** 开始监听（并处理当前地址 —— 深链也走一遍守卫）。 */
  start(): void;
  /** 停止监听。 */
  stop(): void;
  /** 跳转到指定路径。 */
  navigate(path: string, options?: { replace?: boolean }): void;
  /** 只做判定，不改地址。 */
  resolve(path?: string): RouteResolution;
}

/** 初始的导航结果（还没过守卫；`start()` 之后才是最终结果）。 */
export function initialResolution(path: string): RouteResolution {
  return { path, params: {}, allowed: true };
}

/** 一次路径匹配的结果。 */
interface RouteMatch {
  /** 命中的路由。 */
  readonly route: RouteDefinition;
  /** 通配段提取出的参数。 */
  readonly params: Record<string, string>;
}

/** 把路径切成段（去掉首尾斜杠；`/` → 空数组）。 */
function pathSegments(path: string): string[] {
  const trimmed = path.replace(/^\/+|\/+$/g, '');
  return trimmed === '' ? [] : trimmed.split('/');
}

/** 解码路径段（非法百分号编码时保留原文，**不抛错**）。 */
function decodeSegment(segment: string): string {
  try {
    return decodeURIComponent(segment);
  } catch {
    return segment;
  }
}

/** 构造路由器。 */
export function createRouter(options: RouterOptions): Router {
  const { routes, environment, getState, onResolve } = options;
  let currentPath = environment.location.pathname;
  let started = false;

  /**
   * 匹配路由：**精确优先**，其次按声明顺序匹配 `:param` 路由（D104）。
   *
   * 精确匹配放在最前是硬要求：否则一条 `/schema/:a/:b` 会把同形精确路由抢走。
   */
  const match = (path: string): RouteMatch | undefined => {
    const exact = routes.find((route) => route.path === path);
    if (exact !== undefined) {
      return { route: exact, params: {} };
    }
    const target = pathSegments(path);
    for (const route of routes) {
      if (!route.path.includes(':')) {
        continue;
      }
      const pattern = pathSegments(route.path);
      if (pattern.length !== target.length) {
        continue;
      }
      const params: Record<string, string> = {};
      let matched = true;
      for (let index = 0; index < pattern.length; index += 1) {
        const expected = pattern[index] ?? '';
        const actual = target[index] ?? '';
        if (expected.startsWith(':')) {
          if (actual === '') {
            matched = false;
            break;
          }
          params[expected.slice(1)] = decodeSegment(actual);
          continue;
        }
        if (expected !== actual) {
          matched = false;
          break;
        }
      }
      if (matched) {
        return { route, params };
      }
    }
    return undefined;
  };

  const resolve = (path: string): RouteResolution => {
    const matched = match(path);
    if (matched === undefined) {
      return { path, params: {}, allowed: true };
    }
    const { route, params } = matched;
    if (route.permission !== undefined) {
      const result = guard(getState(), route.permission, options.guardOptions);
      if (!result.allowed) {
        return { path, route, params, allowed: false, redirectTo: result.redirectTo };
      }
      return { path, route, params, allowed: true };
    }
    if (route.requiresAuth === true && !isAuthenticated(getState())) {
      return {
        path,
        route,
        params,
        allowed: false,
        redirectTo: options.guardOptions?.loginPath ?? '/login',
      };
    }
    return { path, route, params, allowed: true };
  };

  const write = (path: string, replace: boolean): void => {
    // 路径没变（深链 / 首次 `start()`）→ 保留查询串与 hash：
    // 写回一个不含 `?query` 的地址等于**丢掉**页面正在用的参数（如 `/sso/callback?ticket=`）。
    const keepsQuery = path === environment.location.pathname;
    const suffix = keepsQuery
      ? `${environment.location.search ?? ''}${environment.location.hash ?? ''}`
      : '';
    if (replace) {
      environment.history.replaceState(null, '', `${path}${suffix}`);
    } else {
      environment.history.pushState(null, '', `${path}${suffix}`);
    }
    currentPath = path;
  };

  const settle = (resolution: RouteResolution): void => {
    onResolve(resolution);
    // 守卫放行后才加载数据 —— 被拦截时目标接口的请求计数为 0。
    if (resolution.allowed && resolution.route?.load !== undefined) {
      void resolution.route.load();
    }
  };

  const apply = (path: string, replace: boolean): void => {
    let resolution = resolve(path);
    let target = path;
    let useReplace = replace;
    if (
      !resolution.allowed &&
      resolution.redirectTo !== undefined &&
      resolution.redirectTo !== path
    ) {
      const intendedPath = path;
      target = resolution.redirectTo;
      resolution = { ...resolve(target), intendedPath };
      useReplace = true;
    }
    write(target, useReplace);
    settle(resolution);
  };

  const onPopState = (): void => {
    const path = environment.location.pathname;
    let resolution = resolve(path);
    if (
      !resolution.allowed &&
      resolution.redirectTo !== undefined &&
      resolution.redirectTo !== path
    ) {
      // 深链 / 前进后退同样要过守卫（地址已变，改写成落点）。
      write(resolution.redirectTo, true);
      resolution = resolve(resolution.redirectTo);
    } else {
      currentPath = path;
    }
    settle(resolution);
  };

  return {
    get path(): string {
      return currentPath;
    },
    start(): void {
      if (started) {
        return;
      }
      started = true;
      environment.addEventListener('popstate', onPopState);
      // 当前地址（可能是深链）也要过一遍守卫。
      apply(environment.location.pathname, true);
    },
    stop(): void {
      if (!started) {
        return;
      }
      started = false;
      environment.removeEventListener('popstate', onPopState);
    },
    navigate(path: string, navigateOptions?: { replace?: boolean }): void {
      apply(path, navigateOptions?.replace ?? false);
    },
    resolve(path?: string): RouteResolution {
      return resolve(path ?? currentPath);
    },
  };
}
