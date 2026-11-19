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
 */

import { guard, type AuthState, type GuardOptions } from '@base/auth';

/** 路由定义。 */
export interface RouteDefinition {
  /** 路径。 */
  readonly path: string;
  /** 页面标题。 */
  readonly title: string;
  /** 访问该路由需要的权限点（没有 = 公开路由）。 */
  readonly permission?: string;
  /** 页面数据加载函数（**守卫放行后**才被调用）。 */
  readonly load?: () => void | Promise<void>;
}

/** 路由环境（浏览器用 `window`，单测用假件）。 */
export interface RouterEnvironment {
  readonly location: { readonly pathname: string };
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
  return { path, allowed: true };
}

/** 构造路由器。 */
export function createRouter(options: RouterOptions): Router {
  const { routes, environment, getState, onResolve } = options;
  let currentPath = environment.location.pathname;
  let started = false;

  const match = (path: string): RouteDefinition | undefined =>
    routes.find((route) => route.path === path);

  const resolve = (path: string): RouteResolution => {
    const route = match(path);
    if (route === undefined || route.permission === undefined) {
      return { path, route, allowed: true };
    }
    const result = guard(getState(), route.permission, options.guardOptions);
    if (!result.allowed) {
      return { path, route, allowed: false, redirectTo: result.redirectTo };
    }
    return { path, route, allowed: true };
  };

  const write = (path: string, replace: boolean): void => {
    if (replace) {
      environment.history.replaceState(null, '', path);
    } else {
      environment.history.pushState(null, '', path);
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
