/**
 * 回调交接页的**纯逻辑**（P4 / AC-S-100 预期 3）。
 *
 * ```text
 * IdP → GET /api/iam/sso/oidc/callback?code&state
 *     → 后端 302 到 /sso/callback?ticket=<一次性 ticket>
 *     → 本模块 POST /api/iam/sso/exchange {ticket} → LoginResponse
 * ```
 *
 * # 为什么逻辑不在 `.svelte` 里
 *
 * 页面要覆盖四种结局（缺 ticket / 成功 / 401 ticket 失效 / 网络 vs 后端错误），
 * 而 `svelte/server` 只能渲染首帧、不能点击、不能等待 Promise。
 * 把判定搬到这里，单测就能逐条断言，页面只负责「把结局渲染成文案」。
 *
 * # token 不进 URL
 *
 * 本模块只处理 **ticket**；换回来的令牌由宿主的 `AppAuth` 写 `sessionStorage`
 * （D99）。成功时调用 `onSuccess()`（宿主的导航动作用 `replace` 把 ticket 从地址栏抹掉）。
 */

import { CLIENT_NETWORK_ERROR, ApiError } from '@base/sdk';

import { errorMessageKey, toMessageParams } from './i18n';
import { readTicket } from './sso-api';

/** 回调页的三种可见状态。 */
export type SsoCallbackOutcome =
  | { readonly kind: 'processing' }
  | { readonly kind: 'success' }
  | {
      readonly kind: 'error';
      /** 翻译键（`common.*` 或 `error.<CODE>`）。 */
      readonly key: string;
      /** 翻译插值参数。 */
      readonly params: Record<string, string | number>;
    };

/** 缺 ticket 时提示（「登录链接无效或已过期」）。 */
export const SSO_CALLBACK_INVALID_LINK_KEY = 'common.ssoCallbackInvalidLink';

/** 文案是否存在的判定（组件传 `i18n.t(key) !== '?'`；单测传已知集合）。 */
export type HasMessage = (key: string) => boolean;

/** 默认判定：认为所有键都存在（单测不传时的行为）。 */
const ALWAYS_HAS_MESSAGE: HasMessage = () => true;

/**
 * 失败 → 「翻译键 + 参数」。
 *
 * 网络错误与后端错误**必须分开**：
 * - 网络层失败（`client.NETWORK_ERROR`，没有后端响应）→ 「网络不可用」；
 * - 后端给了错误体 → 按稳定错误码渲染（`error.<CODE>`）。
 *
 * 同一个提示会把「我们连不上」和「IdP 说这个 ticket 不认」混成一句话，
 * 用户与运维都无法判断下一步该做什么。
 */
export function describeSsoCallbackError(
  caught: unknown,
  hasMessage: HasMessage = ALWAYS_HAS_MESSAGE,
): { readonly key: string; readonly params: Record<string, string | number> } {
  if (caught instanceof ApiError) {
    if (caught.code === CLIENT_NETWORK_ERROR) {
      return { key: errorMessageKey(CLIENT_NETWORK_ERROR), params: {} };
    }
    const key = errorMessageKey(caught.code);
    if (!hasMessage(key)) {
      // 没有该错误码的文案：不把 "?" 显示给用户（原始 code 仍在网络面板里）。
      return { key: 'common.unexpectedError', params: {} };
    }
    return { key, params: toMessageParams(caught.params) };
  }
  return { key: 'common.unexpectedError', params: {} };
}

/** 执行交接页的入参（全部可注入 → 单测不碰浏览器 / 网络）。 */
export interface CompleteSsoCallbackInput {
  /** `window.location.search`。 */
  readonly search: string;
  /** 用 ticket 换会话（宿主的 `AppAuth.loginWithTicket`）。 */
  readonly loginWithTicket: (ticket: string) => Promise<void>;
  /** 成功后的导航（宿主负责把 ticket 从地址栏抹掉）。 */
  readonly onSuccess: () => void;
  /** 文案存在性判定（缺省认为都存在）。 */
  readonly hasMessage?: HasMessage;
}

/**
 * 跑完一次回调交接。
 *
 * 返回 `error` 时**不**调用 `onSuccess`（页面停在错误态并给「返回登录页」）。
 */
export async function completeSsoCallback(
  input: CompleteSsoCallbackInput,
): Promise<SsoCallbackOutcome> {
  const ticket = readTicket(input.search);
  if (ticket === undefined) {
    return { kind: 'error', key: SSO_CALLBACK_INVALID_LINK_KEY, params: {} };
  }
  try {
    await input.loginWithTicket(ticket);
  } catch (error) {
    return {
      kind: 'error',
      ...describeSsoCallbackError(error, input.hasMessage ?? ALWAYS_HAS_MESSAGE),
    };
  }
  input.onSuccess();
  return { kind: 'success' };
}
