/**
 * 宿主侧的会话与 API 装配。
 *
 * # 令牌由宿主保管
 *
 * SDK 不存令牌：它通过 `getAccessToken` / `refreshToken` 回调取 ——
 * 刷新成功后**只更新令牌**，**绝不碰权限点集合**
 * （后端的 refresh 响应里 `permissions` 恒为空数组，有意为之）。
 * 权限点只在**登录成功**时从登录响应取一次。
 */

import { createAuthStore, type AuthStore } from '@base/auth';
import { createApiClient, type ApiClient } from '@base/sdk';

/** 登录接口。 */
export const LOGIN_PATH = '/api/iam/login';

/** 刷新接口。 */
export const REFRESH_PATH = '/api/iam/refresh';

/** 权限点接口。 */
export const PERMISSIONS_PATH = '/api/rbac/me/permissions';

/** 会话（令牌）。 */
export interface Session {
  /** access token。 */
  accessToken?: string;
  /** refresh token。 */
  refreshToken?: string;
}

/** 应用侧的 API 装配结果。 */
export interface AppApi {
  /** 会话（令牌持有者）。 */
  readonly session: Session;
  /** SDK 客户端。 */
  readonly client: ApiClient;
  /** 认证存储（登录态 + 权限点）。 */
  readonly auth: AuthStore;
}

/** 装配 API 客户端与认证存储。 */
export function createAppApi(options: { tenantId?: string } = {}): AppApi {
  const session: Session = {};

  const client = createApiClient({
    baseUrl: '',
    fetch: (input, init) => globalThis.fetch(input, init),
    getAccessToken: () => session.accessToken,
    refreshToken: async () => {
      // ⚠️ 刷新响应里的 `permissions` 是**空数组**（后端有意为之）——
      //    这里只取令牌，绝不覆盖权限点集合。
      const refreshed = await client.refresh<{ accessToken: string; refreshToken?: string }>(
        REFRESH_PATH,
        { refreshToken: session.refreshToken ?? '' },
      );
      session.accessToken = refreshed.accessToken;
      if (refreshed.refreshToken !== undefined) {
        session.refreshToken = refreshed.refreshToken;
      }
      return refreshed.accessToken;
    },
    tenantId: options.tenantId,
    onUnauthorized: () => {
      session.accessToken = undefined;
      session.refreshToken = undefined;
    },
  });

  const auth = createAuthStore(client, {
    loginPath: LOGIN_PATH,
    permissionsPath: PERMISSIONS_PATH,
    onLogin: (loginSession) => {
      session.accessToken = loginSession.accessToken;
      session.refreshToken = loginSession.refreshToken;
    },
    onLogout: () => {
      session.accessToken = undefined;
      session.refreshToken = undefined;
    },
  });

  return { session, client, auth };
}
