/**
 * 登录态的上下文（`@base/auth` 的状态）。
 *
 * 上下文里放的是**取值器**而不是快照 —— 登录 / 登出后，
 * `<Can>` 里的 `can()` 会读到最新的登录态。
 *
 * # 为什么取值器放在 `.ts` 里
 *
 * `.svelte` 的脚本块**可以**写 TS（根 `eslint.config.js` 已给 `.svelte` 配了
 * TypeScript 解析器，见 D97）；这里放在 `.ts` 是为了让上下文**键与类型**集中在一处、
 * 由 `<Can>` 与后续页面共用同一份契约（也便于单测直接 import）。
 */

import { getContext } from 'svelte';
import type { AuthState } from '@base/auth';

/** 上下文键。 */
export const AUTH_CONTEXT_KEY = 'base-auth';

/** 上下文内容。 */
export interface AuthContext {
  /** 当前登录态。 */
  readonly state: AuthState;
}

/** 取当前登录态的上下文（没有就是 `undefined`）。 */
export function useAuthContext(): AuthContext | undefined {
  return getContext<AuthContext | undefined>(AUTH_CONTEXT_KEY);
}
