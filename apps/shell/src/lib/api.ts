/**
 * 宿主侧的会话与 API 装配。
 *
 * # 令牌由宿主保管
 *
 * SDK 不存令牌：它通过 `getAccessToken` / `refreshToken` 回调取 ——
 * 刷新成功后**只更新令牌**，**绝不碰权限点集合**
 * （后端的 refresh 响应里 `permissions` 恒为空数组，有意为之）。
 * 权限点只在**登录成功**时从登录响应取一次，之后由会话恢复现取
 * （`GET /api/rbac/me/permissions`）—— **不持久化**，以免授权撤销后仍用旧集合。
 *
 * # 租户由用户在登录页填（D98）
 *
 * 后端要求 `x-tenant-id`；壳不硬编码、也不从环境变量读死（那是单租户假设）。
 * 登录前把用户填的租户写进 SDK 上下文，登录成功后租户**随会话一起持久化**，
 * 重载恢复会话时再写回 SDK 上下文 —— 否则恢复后的请求又会缺头。
 */

import { ANONYMOUS, createAuthStore, type AuthState } from '@base/auth';
import { ApiError, CLIENT_INTERNAL_ERROR, createApiClient, type ApiClient } from '@base/sdk';

import { createSessionStore, type KeyValueStorage } from './session';
import {
  exchangeTicket,
  ldapLogin,
  type SsoLdapCredentials,
  type SsoLoginResponse,
} from './sso-api';

/** 登录接口。 */
export const LOGIN_PATH = '/api/iam/login';

/** 刷新接口。 */
export const REFRESH_PATH = '/api/iam/refresh';

/** 权限点接口。 */
export const PERMISSIONS_PATH = '/api/rbac/me/permissions';

/** 登录凭据（租户由登录表单提供）。 */
export interface Credentials {
  /** 租户标识（如 `tenant-a`）。 */
  readonly tenantId: string;
  /** 用户名。 */
  readonly username: string;
  /** 密码。 */
  readonly password: string;
}

/** 租户为空时 `login` 抛出的错误消息（登录页据此给可理解的提示，且**不发请求**）。 */
export const TENANT_REQUIRED = 'TENANT_REQUIRED';

/** 应用侧认证入口（包装 `@base/auth` 的存储，补上「租户 + 会话恢复」）。 */
export interface AppAuth {
  /** 当前登录态（登录 / 恢复后更新）。 */
  readonly state: AuthState;
  /** 登录：先绑定租户，再走 SDK 登录。 */
  login(credentials: Credentials): Promise<void>;
  /**
   * SSO 登录：LDAP bind（AC-S-104）。
   *
   * 响应与本地登录**同构**（AC-S-107），所以落会话的代码路径也共用
   * （`applySsoLogin`），只有「怎么拿到 LoginResponse」不同。
   */
  loginWithLdap(credentials: SsoLdapCredentials): Promise<void>;
  /**
   * SSO 登录：一次性 ticket 换会话（P4）。
   *
   * 租户来自响应里的 `user.tenantId`（回调的时候浏览器**没有** `x-tenant-id`）。
   */
  loginWithTicket(ticket: string): Promise<void>;
  /** 登出：清空登录态与持久化会话。 */
  logout(): Promise<void>;
}

/** 应用侧的 API 装配结果。 */
export interface AppApi {
  /** SDK 客户端。 */
  readonly client: ApiClient;
  /** 认证入口（登录态 + 权限点）。 */
  readonly auth: AppAuth;
  /**
   * 启动时恢复会话（整页重载 / 直接敲 URL）。
   *
   * 返回恢复后的登录态：拿不到权限点就按**未登录**处理（fail closed），
   * 不会因为「有令牌」就放行。
   */
  restoreSession(): Promise<AuthState>;
}

/** 装配参数（存储与 fetch 可注入，便于单测不碰真实浏览器 / 网络）。 */
export interface AppApiOptions {
  /** 会话存储；`null` = 仅内存，不传 = 自动探测 `sessionStorage`。 */
  readonly storage?: KeyValueStorage | null;
  /** fetch 实现（默认 `globalThis.fetch`）。 */
  readonly fetch?: typeof globalThis.fetch;
}

/** 权限点规范化：去重 + 升序（与后端 `effective_permissions` 的口径一致）。 */
function normalizePermissions(permissions: readonly string[] | undefined): readonly string[] {
  return [...new Set(permissions ?? [])].sort();
}

/** 装配 API 客户端与认证存储。 */
export function createAppApi(options: AppApiOptions = {}): AppApi {
  const session = createSessionStore(options.storage);
  const fetchImpl: typeof globalThis.fetch =
    options.fetch ?? ((input, init) => globalThis.fetch(input, init));

  /** 认证存储对外暴露的登录态（登录 / 恢复 / 失效时更新）。 */
  let current: AuthState = ANONYMOUS;

  const client = createApiClient({
    baseUrl: '',
    fetch: fetchImpl,
    getAccessToken: () => session.state.accessToken,
    refreshToken: async () => {
      // ⚠️ 刷新响应里的 `permissions` 是**空数组**（后端有意为之）——
      //    这里只取令牌，绝不覆盖权限点集合。
      const refreshed = await client.refresh<{ accessToken: string; refreshToken?: string }>(
        REFRESH_PATH,
        { refreshToken: session.state.refreshToken ?? '' },
      );
      session.setTokens({
        accessToken: refreshed.accessToken,
        refreshToken: refreshed.refreshToken,
      });
      return refreshed.accessToken;
    },
    tenantId: session.state.tenantId,
    onUnauthorized: () => {
      // 刷新失败 = 会话已死（令牌过期 / 被撤销）：清掉持久化会话，按未登录处理。
      session.clear();
      current = ANONYMOUS;
    },
  });

  const store = createAuthStore(client, {
    loginPath: LOGIN_PATH,
    permissionsPath: PERMISSIONS_PATH,
    onLogin: (loginSession) => {
      session.setTokens({
        accessToken: loginSession.accessToken,
        refreshToken: loginSession.refreshToken === '' ? undefined : loginSession.refreshToken,
      });
    },
    onLogout: () => {
      session.clear();
    },
  });

  async function restoreSession(): Promise<AuthState> {
    const saved = session.state;
    if (saved.accessToken === undefined || saved.tenantId === undefined) {
      current = ANONYMOUS;
      return current;
    }
    // 租户先写回 SDK 上下文 —— 恢复后的请求（含这次权限点请求）都要带 `x-tenant-id`。
    client.setContext({ tenantId: saved.tenantId });
    try {
      const response = await client.get<{ permissions?: readonly string[] }>(PERMISSIONS_PATH);
      current = {
        status: 'authenticated',
        authenticated: true,
        // 用户信息不在本接口响应里：恢复后的会话只保证「租户 + 权限点」，
        // 守卫与 `<Can>` 都只看这两项。
        tenantId: saved.tenantId,
        permissions: normalizePermissions(response?.permissions),
      };
    } catch (error) {
      // 令牌失效（401 / 403）→ 清会话，按未登录处理（fail closed）。
      // 网络层失败（离线等）**不**清会话：令牌可能仍有效，重载后还能再试。
      if (error instanceof ApiError && (error.status === 401 || error.status === 403)) {
        session.clear();
      }
      current = ANONYMOUS;
      // eslint-disable-next-line no-console -- 恢复失败要可发现，否则「深链落登录页」无从排查
      console.warn('[session] 恢复会话失败，已按未登录处理', error);
    }
    return current;
  }

  /**
   * 把 SSO 的 `LoginResponse` 落成会话（AC-S-107：与本地登录**同构**）。
   *
   * 顺序是硬要求：**先** `setTenant` 再 `setTokens` ——
   * `SessionStore` 的持久化在「没有租户」时直接跳过（`session.ts`），
   * 反过来写会得到「内存里有令牌、重载后没有会话」。
   *
   * @param fallbackTenantId 响应里没带 `user.tenantId` 时的兜底（LDAP 登录由用户填的租户提供）
   */
  const applySsoLogin = (
    response: SsoLoginResponse | undefined,
    fallbackTenantId?: string,
  ): void => {
    const accessToken = response?.accessToken;
    if (typeof accessToken !== 'string' || accessToken === '') {
      // 形状不对**不能**当成功：否则会出现「界面已登录、请求全 401」。
      throw new ApiError({ code: CLIENT_INTERNAL_ERROR, status: 0 });
    }
    const tenantId = response?.user?.tenantId ?? fallbackTenantId;
    if (tenantId === undefined || tenantId === '') {
      throw new ApiError({ code: CLIENT_INTERNAL_ERROR, status: 0 });
    }
    const refreshToken =
      typeof response?.refreshToken === 'string' && response.refreshToken !== ''
        ? response.refreshToken
        : undefined;
    session.setTenant(tenantId);
    client.setContext({ tenantId });
    session.setTokens({ accessToken, refreshToken });
    current = {
      status: 'authenticated',
      authenticated: true,
      userId: response?.user?.id,
      tenantId,
      // 登录响应里的权限点与 `GET /api/rbac/me/permissions` 同源；
      // 刷新响应才是空数组（这里不是刷新）。
      permissions: normalizePermissions(response?.permissions),
    };
  };

  const auth: AppAuth = {
    get state(): AuthState {
      return current;
    },

    async login(credentials): Promise<void> {
      const tenantId = credentials.tenantId.trim();
      if (tenantId === '') {
        // 空租户**不发请求**：后端必然 401 `TENANT_CONTEXT_MISSING`，
        // 用户看到的却会是「用户名或密码不正确」这种误导文案。
        throw new Error(TENANT_REQUIRED);
      }
      // 先绑租户（登录请求本身就要带 `x-tenant-id`，AC-S-072 步骤 4）。
      // ⚠️ 这是**尝试用**的租户：只进内存，落盘由登录成功后的 `setTokens` 完成
      // （`session.setTenant` 不 persist）—— 否则登录失败会覆盖已持久化会话的租户。
      const previousTenant = session.state.tenantId;
      session.setTenant(tenantId);
      client.setContext({ tenantId });
      try {
        await store.login(credentials.username, credentials.password);
      } catch (error) {
        // 登录失败：回滚内存里的租户，免得同页后续请求 / 刷新带错 `x-tenant-id`。
        if (previousTenant !== undefined) {
          session.setTenant(previousTenant);
          client.setContext({ tenantId: previousTenant });
        }
        throw error;
      }
      current = store.state;
    },

    async logout(): Promise<void> {
      await store.logout();
      current = store.state;
    },

    async loginWithLdap(credentials): Promise<void> {
      const tenantId = credentials.tenantId.trim();
      if (tenantId === '') {
        // 与本地登录一致：空租户**不发请求**（后端无法决定用哪套 LDAP 配置）。
        throw new Error(TENANT_REQUIRED);
      }
      // 与 `login()` 完全一致：先绑「尝试用」的租户（请求要带 `x-tenant-id`），
      // 失败则回滚 —— 已登录用户误在登录页试错时，不能把既有会话的租户改掉。
      const previousTenant = session.state.tenantId;
      session.setTenant(tenantId);
      client.setContext({ tenantId });
      try {
        const response = await ldapLogin(client, {
          tenantId,
          username: credentials.username,
          password: credentials.password,
          ...(credentials.loginUrl === undefined ? {} : { loginUrl: credentials.loginUrl }),
        });
        applySsoLogin(response, tenantId);
      } catch (error) {
        if (previousTenant !== undefined) {
          session.setTenant(previousTenant);
          client.setContext({ tenantId: previousTenant });
        }
        throw error;
      }
    },

    async loginWithTicket(ticket): Promise<void> {
      const trimmed = ticket.trim();
      if (trimmed === '') {
        // 回调页先做 `readTicket`，走不到这里；真走到说明调用方漏了前置校验。
        throw new ApiError({ code: CLIENT_INTERNAL_ERROR, status: 0 });
      }
      // 不回滚任何上下文：回调落点时没有既有会话，且 ticket 自己绑了租户。
      const response = await exchangeTicket(client, trimmed);
      applySsoLogin(response);
    },
  };

  return { client, auth, restoreSession };
}
