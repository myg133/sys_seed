/**
 * 认证占位包。
 *
 * 本期只定义登录态的形状与判定，实际的令牌获取与刷新在 REQ-007 实现。
 */

/** 登录态。 */
export interface AuthState {
  /** 是否已登录。 */
  readonly authenticated: boolean;
  /** 当前用户 ID。 */
  readonly userId?: string;
  /** 当前租户 ID。 */
  readonly tenantId?: string;
}

/** 未登录态。 */
export const ANONYMOUS: AuthState = Object.freeze({ authenticated: false });

/** 是否已登录。 */
export function isAuthenticated(state: AuthState): boolean {
  return state.authenticated && state.userId !== undefined && state.userId !== '';
}

/** 取当前租户 ID，未登录或未绑定租户时为 `undefined`。 */
export function tenantId(state: AuthState): string | undefined {
  return isAuthenticated(state) ? state.tenantId : undefined;
}
