/**
 * 登录态的上下文（`@base/auth` 的状态）。
 *
 * 上下文里放的是**取值器**而不是快照 —— 登录 / 登出后，
 * `<Can>` 里的 `can()` 会读到最新的登录态。
 *
 * # 为什么取值器放在 `.ts` 里
 *
 * `apps/shell` 的 `.svelte` 脚本块是 **JS 语法**（本仓的 eslint 配置没给
 * `.svelte` 文件配 TypeScript 解析器，写 TS 会让 `eslint .` 报解析错误），
 * 而 `.ts` 模块是**全量类型检查**的 —— 所以带类型的部分都放在这里。
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
