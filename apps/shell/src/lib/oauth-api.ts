/**
 * 出站 OAuth（授权页 / 设备码页 / 我的授权）与单点登出的前端封装
 * （REQ-022 / AC-S-137 · 142 · 146 · 147 · 148 · 150 · 152）。
 *
 * # 字段名 = camelCase（契约 C13 / D258）
 *
 * 平台端点（`/api/oauth/**`）的 JSON 字段**一律 camelCase**；
 * 协议端点（`/api/oauth/token` 等）是 RFC 的 snake_case —— **两套不混**，
 * 而本文件**只调平台端点**（协议端点是第三方应用调的）。
 *
 * # 授权端点为什么是「前端路由 + 后端三个 POST」（D203）
 *
 * 平台的会话是 **Bearer token 存在 `sessionStorage`** → 浏览器**顶层导航不会带
 * `Authorization` 头** → 后端的 authorize 端点拿不到登录态。
 * → 授权请求由**前端路由**（`/oauth/authorize`）承接，后端提供
 * `preview` / `confirm` / `deny` 三个 POST。
 *
 * # 为什么逻辑不写在 `.svelte` 里
 *
 * `svelte/server` 的 `render` 只能渲染**首帧**、不能点击、不能等 Promise。
 * 状态机（未登录 → 登录并继续 / `login_required`；已登录 → preview →
 * 同意页 / 免登直发 / 错误）与「`error=` 透出到 URL」都搬到这里，
 * 单测就能逐条断言；页面只负责「把结局渲染成文案」。
 *
 * # `return_to` 是**开放重定向**风险点（安全要求，不是可选优化）
 *
 * `readReturnTo` 只放行**站内相对路径**：必须以**单个** `/` 开头、
 * 不得以 `//` 开头（协议相对 URL）、不得含 scheme。
 * 判定用 **URL 解析器**做（比正则准）：`new URL(value, base)` 的 origin
 * 必须仍是 base 的 origin —— `http://evil.com` / `//evil.com` / `/\evil.com`
 * 都会被它挡下。
 * 不合格 → 返回 `undefined` → **保持现行为**（跳首页 / `intendedPath`）。
 */

import type { ApiClient } from '@base/sdk';

import { errorMessageKey, toMessageParams } from './i18n';

/* -------------------------------------------------------------------------- */
/* 路径常量（逐字 —— 与棒 1 的实现对照用）                                      */
/* -------------------------------------------------------------------------- */

/** 授权预览（校验参数 + 签同意票据）。 */
export const OAUTH_AUTHORIZE_PREVIEW_PATH = '/api/oauth/authorize/preview';

/** 同意（用户点「允许」→ 签发授权码）。 */
export const OAUTH_AUTHORIZE_CONFIRM_PATH = '/api/oauth/authorize/confirm';

/** 拒绝（用户点「拒绝」→ `error=access_denied`）。 */
export const OAUTH_AUTHORIZE_DENY_PATH = '/api/oauth/authorize/deny';

/** 我的授权列表。 */
export const OAUTH_GRANTS_PATH = '/api/oauth/grants';

/** 撤销授权（按 client）。 */
export const OAUTH_GRANTS_REVOKE_PATH = '/api/oauth/grants/revoke';

/** 全局登出（返回 `frontChannel` 数组，**前端**渲染 iframe，D225）。 */
export const OAUTH_LOGOUT_PATH = '/api/oauth/logout';

/** 设备码查询（批准前展示 client 名与 scope）。 */
export const OAUTH_DEVICE_VERIFY_PATH = '/api/oauth/device/verify';

/** 设备码批准 / 拒绝。 */
export const OAUTH_DEVICE_APPROVE_PATH = '/api/oauth/device/approve';

/* -------------------------------------------------------------------------- */
/* 路由（4 条；公开 / 受保护见 `routes.ts`）                                     */
/* -------------------------------------------------------------------------- */

/** 授权页 / 同意页（**公开路由** —— 页面自己处理未登录）。 */
export const OAUTH_AUTHORIZE_ROUTE_PATH = '/oauth/authorize';

/** 设备码批准页（**公开路由** —— query 里带 `user_code`）。 */
export const OAUTH_DEVICE_ROUTE_PATH = '/oauth/device';

/** 「我的授权」页（**受保护** —— 权限点 `oauth:grant:read`）。 */
export const OAUTH_GRANTS_ROUTE_PATH = '/oauth/grants';

/** 登出页（**公开路由** —— 渲染 front-channel iframe）。 */
export const OAUTH_LOGOUT_ROUTE_PATH = '/oauth/logout';

/** 登录页路径（未登录回跳的落点）。 */
export const LOGIN_ROUTE_PATH = '/login';

/* -------------------------------------------------------------------------- */
/* 参数名与常量                                                                 */
/* -------------------------------------------------------------------------- */

/** 登录回跳参数名（D221）。 */
export const RETURN_TO_PARAM = 'return_to';

/** `prompt=none`（RFC 6749 §4.1.1）—— **不许交互**。 */
export const OAUTH_PROMPT_NONE = 'none';

/** 设备码参数名（`verification_uri_complete` 里带的那个）。 */
export const OAUTH_USER_CODE_PARAM = 'user_code';

/** 设备码的字符集（RFC 8628 §6.1 的 base20：**8 位、无易混字符**）。 */
export const OAUTH_USER_CODE_ALPHABET = 'BCDFGHJKLMNPQRSTVWXZ';

/** 设备码长度（8 位）。 */
export const OAUTH_USER_CODE_LENGTH = 8;

/**
 * 「我的授权」页的权限点（契约 C4：`module:resource:action`）。
 *
 * 与棒 1 的 `manifest.json` / 种子迁移**逐字一致**。
 */
export const OAUTH_GRANTS_PERMISSION = 'oauth:grant:read';

/**
 * 平台错误码白名单（**恰好 9 个**，D226 / D257）。
 *
 * 码逐字等于棒 1 的 `crates/platform-oauth/src/error.rs` —— 改一个就要改两处。
 * ⚠️ **没有**第 10 个：设备码查不到也是 `OAUTH_INVALID_REQUEST`（D257）。
 */
export const OAUTH_ERROR_CODES = [
  'OAUTH_CLIENT_NOT_FOUND',
  'OAUTH_CLIENT_DISABLED',
  'OAUTH_CLIENT_EXPIRED',
  'OAUTH_INVALID_REQUEST',
  'OAUTH_INVALID_REDIRECT_URI',
  'OAUTH_INVALID_SCOPE',
  'OAUTH_UNSUPPORTED_CODE_CHALLENGE_METHOD',
  'OAUTH_TICKET_INVALID',
  'OAUTH_GRANT_NOT_FOUND',
] as const;

/** 平台错误码（联合类型）。 */
export type OAuthErrorCode = (typeof OAUTH_ERROR_CODES)[number];

/* -------------------------------------------------------------------------- */
/* 浏览器环境（可注入 → 单测不碰真实浏览器）                                    */
/* -------------------------------------------------------------------------- */

/**
 * 页面用到的浏览器能力。
 *
 * 之所以抽成接口：地址栏、跳转、只改地址这四件事在单测里必须是**假件**
 * （既要能观测「跳到哪了」，又不能真的导航）。
 */
export interface OAuthPageEnv {
  /** 当前路径（`location.pathname`）。 */
  readonly pathname: string;
  /** 当前查询串（`location.search`）。 */
  readonly search: string;
  /** 整页跳转（`location.href = url`）。 */
  assign(url: string): void;
  /** 替换当前地址（`location.replace(url)`）—— 回跳地址**不进浏览器历史**。 */
  replace(url: string): void;
  /** 只改地址（`history.replaceState`）—— **不重新导航**（页面状态不丢）。 */
  replaceState(url: string): void;
}

/** 取全局的 `window`（SSR / Node 单测里可能没有）。 */
function globalWindow(): (Window & typeof globalThis) | undefined {
  return (globalThis as { window?: Window & typeof globalThis }).window;
}

/**
 * 浏览器环境的默认实现。
 *
 * `search` / `pathname` 是**取值器** → `history.replaceState` 之后能读到新值。
 * 非浏览器环境（SSR / Node 单测）→ `search` 为空串、跳转为**无操作**
 * （页面不该因为没有浏览器就崩掉）。
 */
export function browserEnv(): OAuthPageEnv {
  const win = globalWindow();
  return {
    get pathname(): string {
      return win?.location.pathname ?? '';
    },
    get search(): string {
      return win?.location.search ?? '';
    },
    assign(url: string): void {
      if (win === undefined) {
        return;
      }
      win.location.href = url;
    },
    replace(url: string): void {
      if (win === undefined) {
        return;
      }
      win.location.replace(url);
    },
    replaceState(url: string): void {
      win?.history.replaceState(null, '', url);
    },
  };
}

/* -------------------------------------------------------------------------- */
/* `return_to`：读 + 校验（D221）                                               */
/* -------------------------------------------------------------------------- */

/** 校验用的基准地址（只用它的 origin；不会发请求）。 */
const RETURN_TO_BASE = 'https://base.invalid';

/** 基准地址的 origin。 */
const RETURN_TO_BASE_ORIGIN = new URL(RETURN_TO_BASE).origin;

/**
 * 判定一个 `return_to` 值是否是**站内相对路径**。
 *
 * 三条规则（安全要求）：
 *
 * 1. 必须以 **`/`** 开头（相对路径）；
 * 2. **不得**以 `//` 开头（协议相对 URL → **开放重定向**）；
 * 3. **不得**含 scheme（`http:` / `https:` 等）。
 *
 * 第 3 条用 **URL 解析器**判定（比正则准）：解析结果的 origin 必须仍是
 * 基准 origin —— `http://evil.com`、`//evil.com`、`/\evil.com`
 * （浏览器把 `\` 当 `/`）都会换掉 origin → 全部被挡下。
 * 控制字符（TAB / LF / CR）也会被 URL 解析器剥掉 → `"/\t/evil.com"`
 * 同样落到不同的 origin → 被挡下。
 */
export function isSafeReturnTo(value: string): boolean {
  if (!value.startsWith('/')) {
    return false;
  }
  if (value.startsWith('//')) {
    return false;
  }
  let resolved: URL;
  try {
    resolved = new URL(value, RETURN_TO_BASE);
  } catch {
    return false;
  }
  return resolved.origin === RETURN_TO_BASE_ORIGIN;
}

/**
 * 从查询串里取并**校验** `return_to`；不合格返回 `undefined`。
 *
 * 解码用 `URLSearchParams`（与 `buildLoginUrl` 的编码**成对**）：
 * `+` 解码成空格、`%2B` 解码成 `+` —— 两者不会互相串味。
 */
export function readReturnTo(search: string): string | undefined {
  let raw: string | null;
  try {
    raw = new URLSearchParams(search).get(RETURN_TO_PARAM);
  } catch {
    return undefined;
  }
  if (raw === null || raw === '') {
    return undefined;
  }
  return isSafeReturnTo(raw) ? raw : undefined;
}

/**
 * 构造登录页地址（把回跳目标编码进 `?return_to=`）。
 *
 * 编码用 `URLSearchParams`（`application/x-www-form-urlencoded` 序列化），
 * 与 `readReturnTo` 的解码**成对**。
 */
export function buildLoginUrl(returnTo: string): string {
  const params = new URLSearchParams();
  params.set(RETURN_TO_PARAM, returnTo);
  return `${LOGIN_ROUTE_PATH}?${params.toString()}`;
}

/**
 * 当前的「完整 URL」（`pathname + search`）—— 未登录回跳时进 `return_to`。
 *
 * ⚠️ **query 一个都不能少**（D221 的核心判据）：授权参数（`client_id` /
 * `state` / `code_challenge` / `redirect_uri`）与设备码（`user_code`）
 * 都在 query 里 → 只带 pathname 会让整条链路不可用。
 */
export function currentTarget(env: OAuthPageEnv): string {
  return `${env.pathname}${env.search}`;
}

/**
 * 登录成功后的落点（**纯判定** → 可单测）。
 *
 * - 有**合格**的 `return_to` → `replace`（整页跳转 → query 一个不丢，
 *   且回跳地址**不进浏览器历史**）；
 * - 没有 / 不合格 → `navigate`（**保持现行为**：跳 `intendedPath` 或首页）。
 */
export type LoginRedirect =
  | { readonly kind: 'replace'; readonly url: string }
  | { readonly kind: 'navigate'; readonly path: string };

/** 判定登录成功后跳哪儿（`intendedPath` = 守卫记的原路径，可能没有）。 */
export function resolveLoginRedirect(
  search: string,
  intendedPath: string | undefined,
): LoginRedirect {
  const returnTo = readReturnTo(search);
  if (returnTo !== undefined) {
    return { kind: 'replace', url: returnTo };
  }
  return { kind: 'navigate', path: intendedPath ?? '/' };
}

/* -------------------------------------------------------------------------- */
/* 授权请求参数                                                                 */
/* -------------------------------------------------------------------------- */

/** 授权请求的参数（RFC 6749 §4.1.1 的 query 名 → D258 的 camelCase 体字段）。 */
export interface OAuthAuthorizeParams {
  /** `response_type`（本需求只支持 `code`）。 */
  readonly responseType?: string;
  /** `client_id`。 */
  readonly clientId?: string;
  /** `redirect_uri`。 */
  readonly redirectUri?: string;
  /** `scope`（**空格分隔的字符串**，D258）。 */
  readonly scope?: string;
  /** `state`（可选）。 */
  readonly state?: string;
  /** `code_challenge`（强制 PKCE S256，D206）。 */
  readonly codeChallenge?: string;
  /** `code_challenge_method`（只接受 `S256`）。 */
  readonly codeChallengeMethod?: string;
  /** `prompt`（可选；**缺省 = 允许交互**，AC-S-148 步骤 5）。 */
  readonly prompt?: string;
}

/** 授权请求的 query 参数名（RFC 6749 §4.1.1，**逐字**）。 */
export const OAUTH_AUTHORIZE_PARAM_NAMES = {
  responseType: 'response_type',
  clientId: 'client_id',
  redirectUri: 'redirect_uri',
  scope: 'scope',
  state: 'state',
  codeChallenge: 'code_challenge',
  codeChallengeMethod: 'code_challenge_method',
  prompt: 'prompt',
} as const;

/**
 * 从查询串里读授权参数。
 *
 * 页面**自己**读 `window.location.search`（不靠路由）——
 * 路由的 `resolve()` 用带 query 的字符串 `match()` 会失败（`router.ts:204-207`），
 * 而守卫的 `intendedPath` 只有 pathname → query 会被抹掉（D221）。
 */
export function readAuthorizeParams(search: string): OAuthAuthorizeParams {
  let params: URLSearchParams;
  try {
    params = new URLSearchParams(search);
  } catch {
    params = new URLSearchParams();
  }
  const pick = (name: string): string | undefined => {
    const value = params.get(name);
    return value === null || value === '' ? undefined : value;
  };
  return {
    responseType: pick(OAUTH_AUTHORIZE_PARAM_NAMES.responseType),
    clientId: pick(OAUTH_AUTHORIZE_PARAM_NAMES.clientId),
    redirectUri: pick(OAUTH_AUTHORIZE_PARAM_NAMES.redirectUri),
    scope: pick(OAUTH_AUTHORIZE_PARAM_NAMES.scope),
    state: pick(OAUTH_AUTHORIZE_PARAM_NAMES.state),
    codeChallenge: pick(OAUTH_AUTHORIZE_PARAM_NAMES.codeChallenge),
    codeChallengeMethod: pick(OAUTH_AUTHORIZE_PARAM_NAMES.codeChallengeMethod),
    prompt: pick(OAUTH_AUTHORIZE_PARAM_NAMES.prompt),
  };
}

/**
 * `prompt` 是不是 `none`（**不许交互**）。
 *
 * ⚠️ 只有 `none` 是特殊值：RFC 6749 §4.1.1 还有 `login` / `consent` /
 * `select_account`，本需求**只**实现 `none` 与「缺省」——
 * **其它值一律当缺省处理**（= 允许交互）。理由：
 *
 * - `prompt=consent` → 允许交互 = 走同意页 → 语义**正好**对上；
 * - `prompt=login` → 允许交互 = 未登录时引导登录 → 不比 `none` 更宽松；
 * - `prompt=select_account` → 本需求**不支持**选账号 → 落到「允许交互」
 *   是**更保守**的一侧（绝不会静默签发授权码）。
 *
 * 「缺省 = 允许交互」是 AC-S-148 步骤 5 的硬判据。
 */
export function isPromptNone(prompt: string | undefined): boolean {
  return prompt === OAUTH_PROMPT_NONE;
}

/** 读 query 里的 `prompt`（缺省 → `undefined`）。 */
export function readPrompt(search: string): string | undefined {
  let value: string | null;
  try {
    value = new URLSearchParams(search).get(OAUTH_AUTHORIZE_PARAM_NAMES.prompt);
  } catch {
    return undefined;
  }
  return value === null || value === '' ? undefined : value;
}

/**
 * 读 query 里的 `user_code`（设备码页）。
 *
 * 参数名逐字为 `user_code`（RFC 8628 §3.3.1 的 `verification_uri_complete`）。
 */
export function readUserCode(search: string): string | undefined {
  let value: string | null;
  try {
    value = new URLSearchParams(search).get(OAUTH_USER_CODE_PARAM);
  } catch {
    return undefined;
  }
  return value === null || value === '' ? undefined : value;
}

/**
 * 给查询串加上 `error=`（**保留**原有的全部参数）。
 *
 * ⚠️ 用 `URLSearchParams` 的 `set` → 其它参数（`client_id` / `state` /
 * `code_challenge` / `redirect_uri`）**一个都不会丢**（AC-S-137 步骤 3 的判据）。
 */
export function withErrorParam(search: string, code: string): string {
  const params = new URLSearchParams(search);
  params.set('error', code);
  const text = params.toString();
  return text === '' ? '' : `?${text}`;
}

/* -------------------------------------------------------------------------- */
/* 响应形状与解析（形状兜底：不抛错）                                            */
/* -------------------------------------------------------------------------- */

/** 值是不是一个可用的对象（`null` / 数组不算）。 */
function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

/** 取文本（缺键 → 空串）。 */
function text(value: unknown): string {
  if (typeof value === 'string') {
    return value;
  }
  if (value === undefined || value === null) {
    return '';
  }
  return String(value);
}

/** 取字符串数组（非数组 → 空数组）。 */
function textList(value: unknown): string[] {
  if (!Array.isArray(value)) {
    return [];
  }
  return value.map((entry) => text(entry));
}

/** preview 里的应用信息。 */
export interface OAuthApplication {
  /** 客户端标识。 */
  readonly clientId: string;
  /** 应用名（同意页**展示**它，AC-S-146 步骤 1）。 */
  readonly name: string;
}

/** `POST /api/oauth/authorize/preview` 的响应（200）。 */
export interface OAuthPreviewResult {
  /** 是否需要用户同意。 */
  readonly needsConsent: boolean;
  /** 同意票据（**仅** `needsConsent === true` 时有；只存页面内存，不落盘）。 */
  readonly ticket?: string;
  /** 应用信息。 */
  readonly application: OAuthApplication;
  /** 实际授予的 scope（**数组**；逐字展示给用户）。 */
  readonly scopes: readonly string[];
  /** 直发地址（**仅** `needsConsent === false`（免登直发）时有）。 */
  readonly redirectTo?: string;
}

/** 解析 preview 响应（形状不对 → 缺字段，不抛错）。 */
export function parsePreviewResult(raw: unknown): OAuthPreviewResult {
  const record = isRecord(raw) ? raw : {};
  const application = isRecord(record['application']) ? record['application'] : {};
  const ticket = text(record['ticket']);
  const redirectTo = text(record['redirectTo']);
  return {
    needsConsent: record['needsConsent'] === true,
    ticket: ticket === '' ? undefined : ticket,
    application: {
      clientId: text(application['clientId']),
      name: text(application['name']),
    },
    scopes: textList(record['scopes']),
    redirectTo: redirectTo === '' ? undefined : redirectTo,
  };
}

/** 一条授权记录（`GET /api/oauth/grants` 的元素）。 */
export interface OAuthGrant {
  /** 客户端标识。 */
  readonly clientId: string;
  /** 应用名（后端可选给）。 */
  readonly clientName?: string;
  /** 已授予的 scope。 */
  readonly scopes: readonly string[];
  /** 授权时间（RFC3339）。 */
  readonly grantedAt: string;
  /** 状态（`active` / `revoked`）。 */
  readonly status: string;
  /** 撤销时间（未撤销时没有）。 */
  readonly revokedAt?: string;
}

/** 解析一条授权记录。 */
export function toOAuthGrant(raw: unknown): OAuthGrant {
  const record = isRecord(raw) ? raw : {};
  const clientName = text(record['clientName']);
  const revokedAt = text(record['revokedAt']);
  return {
    clientId: text(record['clientId']),
    clientName: clientName === '' ? undefined : clientName,
    scopes: textList(record['scopes']),
    grantedAt: text(record['grantedAt']),
    status: text(record['status']),
    revokedAt: revokedAt === '' ? undefined : revokedAt,
  };
}

/** 解析授权列表（形状不对 → 空列表，**不抛错**）。 */
export function parseOAuthGrants(raw: unknown): OAuthGrant[] {
  const list = Array.isArray(raw) ? raw : [];
  return list.map((entry) => toOAuthGrant(entry));
}

/** front-channel 的一个通知目标（`POST /api/oauth/logout` 的响应元素）。 */
export interface OAuthFrontChannel {
  /** 客户端标识。 */
  readonly clientId: string;
  /** 第三方登出地址（**逐字**用，不改写）。 */
  readonly logoutUrl: string;
  /** `logout_token`（JWT）。 */
  readonly logoutToken: string;
}

/** `POST /api/oauth/logout` 的响应（200）。 */
export interface OAuthLogoutResult {
  /** 本次登出撤销了什么。 */
  readonly loggedOut: {
    /** 撤销的授权记录数。 */
    readonly grants: number;
    /** 作废的会话数。 */
    readonly sessions: number;
  };
  /** front-channel 通知目标（**可能为空数组**，不是 `null`）。 */
  readonly frontChannel: readonly OAuthFrontChannel[];
}

/** 解析登出响应。 */
export function parseLogoutResult(raw: unknown): OAuthLogoutResult {
  const record = isRecord(raw) ? raw : {};
  const loggedOut = isRecord(record['loggedOut']) ? record['loggedOut'] : {};
  const list = Array.isArray(record['frontChannel']) ? record['frontChannel'] : [];
  const frontChannel = list.map((entry): OAuthFrontChannel => {
    const item = isRecord(entry) ? entry : {};
    return {
      clientId: text(item['clientId']),
      logoutUrl: text(item['logoutUrl']),
      logoutToken: text(item['logoutToken']),
    };
  });
  return {
    loggedOut: {
      grants: typeof loggedOut['grants'] === 'number' ? loggedOut['grants'] : 0,
      sessions: typeof loggedOut['sessions'] === 'number' ? loggedOut['sessions'] : 0,
    },
    frontChannel,
  };
}

/**
 * front-channel iframe 的 `src`（**逐字**拼接，AC-S-150 步骤 2）。
 *
 * `logoutUrl` + `?logout_token=` + `logoutToken` —— 不编码、不改写。
 */
export function frontChannelSrc(item: OAuthFrontChannel): string {
  return `${item.logoutUrl}?logout_token=${item.logoutToken}`;
}

/** 设备码信息（`GET /api/oauth/device/verify` 的响应）。 */
export interface OAuthDeviceInfo {
  /** 客户端标识。 */
  readonly clientId: string;
  /** 应用名（批准前展示给用户）。 */
  readonly clientName: string;
  /** 请求的 scope。 */
  readonly scopes: readonly string[];
}

/** 解析设备码查询响应。 */
export function parseDeviceInfo(raw: unknown): OAuthDeviceInfo {
  const record = isRecord(raw) ? raw : {};
  return {
    clientId: text(record['clientId']),
    clientName: text(record['clientName']),
    scopes: textList(record['scopes']),
  };
}

/** 设备码批准的动作。 */
export type OAuthDeviceAction = 'approve' | 'deny';

/** 设备码批准的结果。 */
export type OAuthDeviceStatus = 'approved' | 'denied';

/* -------------------------------------------------------------------------- */
/* 接口调用                                                                     */
/* -------------------------------------------------------------------------- */

/** 预览授权请求（已登录才调；Bearer 由 SDK 自动带）。 */
export async function previewAuthorization(
  client: ApiClient,
  params: OAuthAuthorizeParams,
): Promise<OAuthPreviewResult> {
  const body: Record<string, string> = {};
  const put = (key: string, value: string | undefined): void => {
    if (value !== undefined) {
      body[key] = value;
    }
  };
  put('responseType', params.responseType);
  put('clientId', params.clientId);
  put('redirectUri', params.redirectUri);
  put('scope', params.scope);
  put('state', params.state);
  put('codeChallenge', params.codeChallenge);
  put('codeChallengeMethod', params.codeChallengeMethod);
  put('prompt', params.prompt);
  const raw = await client.post<unknown>(OAUTH_AUTHORIZE_PREVIEW_PATH, body);
  return parsePreviewResult(raw);
}

/** 从 `{ redirectTo }` 里取地址（缺键 → 抛错：没有落点就没有可跳的地方）。 */
function redirectToOf(raw: unknown): string {
  const record = isRecord(raw) ? raw : {};
  const redirectTo = text(record['redirectTo']);
  if (redirectTo === '') {
    throw new RangeError('oauth response has no redirectTo');
  }
  return redirectTo;
}

/** 同意（返回 `redirectTo`，页面负责跳转）。 */
export async function confirmAuthorization(client: ApiClient, ticket: string): Promise<string> {
  const raw = await client.post<unknown>(OAUTH_AUTHORIZE_CONFIRM_PATH, { ticket });
  return redirectToOf(raw);
}

/** 拒绝（返回 `redirectTo`，query 里有 `error=access_denied` 与 `state`）。 */
export async function denyAuthorization(client: ApiClient, ticket: string): Promise<string> {
  const raw = await client.post<unknown>(OAUTH_AUTHORIZE_DENY_PATH, { ticket });
  return redirectToOf(raw);
}

/** 同意页的动作。 */
export type ConsentDecision = 'allow' | 'deny';

/**
 * 同意页的动作 → **用后端返回的 `redirectTo` 跳转**。
 *
 * 逻辑放在这里（而不是 `.svelte` 里）→ 单测能直接断言「跳到哪了」，
 * 不必真的点按钮（`svelte/server` 的 `render` 不能交互）。
 */
export async function submitConsentDecision(
  client: ApiClient,
  env: OAuthPageEnv,
  ticket: string,
  decision: ConsentDecision,
): Promise<string> {
  const redirectTo =
    decision === 'allow'
      ? await confirmAuthorization(client, ticket)
      : await denyAuthorization(client, ticket);
  env.assign(redirectTo);
  return redirectTo;
}

/** 我的授权列表（**只返回当前用户自己的** —— 前端不做租户过滤）。 */
export async function listOAuthGrants(client: ApiClient): Promise<OAuthGrant[]> {
  const raw = await client.get<unknown>(OAUTH_GRANTS_PATH);
  return parseOAuthGrants(raw);
}

/** 撤销某个 client 的授权（grant 级撤销，D210）。 */
export async function revokeOAuthGrant(client: ApiClient, clientId: string): Promise<void> {
  await client.post<unknown>(OAUTH_GRANTS_REVOKE_PATH, { clientId });
}

/** 路由 `load` 的产物（首屏数据 + 失败原因）。 */
export interface OAuthGrantsLoadResult {
  /** 授权记录。 */
  readonly grants: readonly OAuthGrant[];
  /** 失败原因（`undefined` = 没失败）。 */
  readonly failure?: unknown;
}

/**
 * 取首屏（路由 `load` 用）。
 *
 * **不抛错**：失败也返回结果（把原因放在 `failure` 里）——
 * 路由的 `load` 是被 `void` 调用的（`router.ts:248`），
 * 抛错会变成**未处理的 Promise 拒绝**（页面上看不到任何提示）。
 */
export async function loadOAuthGrants(client: ApiClient): Promise<OAuthGrantsLoadResult> {
  try {
    return { grants: await listOAuthGrants(client) };
  } catch (failure) {
    return { grants: [], failure };
  }
}

/** 全局登出（返回 `frontChannel` 数组；**iframe 由前端渲染**，D225）。 */
export async function logoutOAuth(client: ApiClient): Promise<OAuthLogoutResult> {
  const raw = await client.post<unknown>(OAUTH_LOGOUT_PATH);
  return parseLogoutResult(raw);
}

/** 查设备码（批准前展示 client 名与 scope）。 */
export async function verifyDeviceCode(
  client: ApiClient,
  userCode: string,
): Promise<OAuthDeviceInfo> {
  const raw = await client.get<unknown>(OAUTH_DEVICE_VERIFY_PATH, {
    query: { userCode },
  });
  return parseDeviceInfo(raw);
}

/** 批准 / 拒绝设备码。 */
export async function approveDeviceCode(
  client: ApiClient,
  userCode: string,
  action: OAuthDeviceAction,
): Promise<OAuthDeviceStatus> {
  const raw = await client.post<unknown>(OAUTH_DEVICE_APPROVE_PATH, { userCode, action });
  const record = isRecord(raw) ? raw : {};
  return text(record['status']) === 'approved' ? 'approved' : 'denied';
}

/* -------------------------------------------------------------------------- */
/* 错误码 → 文案                                                                */
/* -------------------------------------------------------------------------- */

/** 一条可展示的错误（翻译键 + 插值参数）。 */
export interface OAuthErrorView {
  /** 翻译键（`error.<CODE>` 或 `common.*`）。 */
  readonly key: string;
  /** 插值参数（后端 `params`）。 */
  readonly params: Record<string, string | number>;
}

/**
 * 把异常转成「翻译键 + 参数」。
 *
 * 平台端点的错误体是 **RFC 9457 Problem Details** → `code` 字段是
 * `OAUTH_*`（大写）→ `errorMessageKey` 映射成 `error.<CODE>`。
 * **没有**该码的文案时退回通用文案（不把 `"?"` 显示给用户）。
 */
export function describeOAuthError(caught: unknown): OAuthErrorView {
  const code = isRecord(caught) ? text(caught['code']) : '';
  if (code !== '') {
    return { key: errorMessageKey(code), params: toMessageParams(paramsOf(caught)) };
  }
  return { key: 'common.unexpectedError', params: {} };
}

/** 取异常对象上的 `params`（`ApiError` 有；别的对象没有）。 */
function paramsOf(caught: unknown): Record<string, unknown> | undefined {
  if (!isRecord(caught)) {
    return undefined;
  }
  const params = caught['params'];
  return isRecord(params) ? params : undefined;
}

/* -------------------------------------------------------------------------- */
/* 授权页状态机                                                                 */
/* -------------------------------------------------------------------------- */

/** 授权页的结局。 */
export type AuthorizeOutcome =
  /** 未登录且**允许交互** → 渲染「登录并继续」（**不调 preview**）。 */
  | { readonly kind: 'sign-in-required' }
  /** 未登录且 `prompt=none` → 不跳登录页，URL 上带 `error=login_required`。 */
  | { readonly kind: 'login-required' }
  /** 正在取 preview。 */
  | { readonly kind: 'loading' }
  /** 需要同意 → 渲染同意页。 */
  | { readonly kind: 'consent'; readonly preview: OAuthPreviewResult }
  /** 已跳转（免登直发 / 同意 / 拒绝之后）。 */
  | { readonly kind: 'redirected' }
  /** 出错了。 */
  | { readonly kind: 'error'; readonly error: OAuthErrorView };

/** 授权页状态机的入参。 */
export interface AuthorizeFlowInput {
  /** API 客户端。 */
  readonly client: ApiClient;
  /** 授权参数（从 query 读的）。 */
  readonly params: OAuthAuthorizeParams;
  /** 是否已登录。 */
  readonly authenticated: boolean;
  /** 浏览器环境。 */
  readonly env: OAuthPageEnv;
}

/**
 * 授权页的**首屏**结局（**同步可判定**的部分）。
 *
 * 页面用它决定首帧渲染什么；`runAuthorizeFlow` 从同一个函数出发 →
 * **判定逻辑只有一份**（不会两处漂移）。
 *
 * - 宿主给了 `preview` → 直接渲染同意页（或已免登直发）；
 * - 未登录 + `prompt=none` → `history.replaceState` 透出 `error=login_required`
 *   （D233），**不跳登录页**；
 * - 未登录 + 其它 → 渲染「登录并继续」（**不调 preview**）；
 * - 已登录 → `loading`（由 `runAuthorizeFlow` 接着取 preview）。
 */
export function initialAuthorizeOutcome(input: {
  readonly authenticated: boolean;
  readonly params: OAuthAuthorizeParams;
  readonly env: OAuthPageEnv;
  readonly preview?: OAuthPreviewResult;
}): AuthorizeOutcome {
  const { authenticated, params, env, preview } = input;
  if (preview !== undefined) {
    return preview.needsConsent ? { kind: 'consent', preview } : { kind: 'redirected' };
  }
  if (!authenticated) {
    if (isPromptNone(params.prompt)) {
      env.replaceState(`${env.pathname}${withErrorParam(env.search, 'login_required')}`);
      return { kind: 'login-required' };
    }
    return { kind: 'sign-in-required' };
  }
  return { kind: 'loading' };
}

/**
 * 跑授权页的状态机。
 *
 * 顺序（**硬要求**）：
 *
 * 1. **未登录** → **不调 preview**（它需要 Bearer）：
 *    - `prompt=none` → `history.replaceState` 把 URL 的 query 加上
 *      `error=login_required`（D233）→ **不跳登录页**（`prompt=none` 不许交互）；
 *    - 其它 → 渲染「登录并继续」；
 * 2. **已登录** → 调 `preview`：
 *    - `needsConsent === false` → **直发**（`location.href = redirectTo`）；
 *    - `needsConsent === true` → 渲染同意页；
 *      `prompt=none` 时**先**把 `error=consent_required` 透出到 URL，
 *      **但仍然渲染同意页**（D233）。
 */
export async function runAuthorizeFlow(input: AuthorizeFlowInput): Promise<AuthorizeOutcome> {
  const { client, params, authenticated, env } = input;
  const promptNone = isPromptNone(params.prompt);

  const initial = initialAuthorizeOutcome({ authenticated, params, env });
  if (initial.kind !== 'loading') {
    return initial;
  }

  let preview: OAuthPreviewResult;
  try {
    preview = await previewAuthorization(client, params);
  } catch (caught) {
    return { kind: 'error', error: describeOAuthError(caught) };
  }

  if (!preview.needsConsent) {
    const redirectTo = preview.redirectTo ?? '';
    if (redirectTo !== '') {
      env.assign(redirectTo);
    }
    return { kind: 'redirected' };
  }

  if (promptNone) {
    env.replaceState(`${env.pathname}${withErrorParam(env.search, 'consent_required')}`);
  }
  return { kind: 'consent', preview };
}

/**
 * 展示授权时间（按用户 / 租户时区渲染，契约 C13）。
 *
 * 原始值不合法时**退回原文**（`Intl` 遇到非法日期会抛错 ——
 * 一条脏数据不该让整页崩掉）。与 `audit-api.ts` 的 `formatAuditTimestamp` 同款。
 */
export function formatGrantTimestamp(iso: string, format: (value: string) => string): string {
  if (iso.trim() === '') {
    return '';
  }
  try {
    const text = format(iso);
    return text === '' ? iso : text;
  } catch {
    return iso;
  }
}

/* -------------------------------------------------------------------------- */
/* 设备码页状态机                                                               */
/* -------------------------------------------------------------------------- */

/** 设备码页的结局。 */
export type DeviceOutcome =
  /** 未登录且允许交互 → 渲染「登录并继续」。 */
  | { readonly kind: 'sign-in-required' }
  /** 未登录且 `prompt=none` → URL 上带 `error=login_required`。 */
  | { readonly kind: 'login-required' }
  /** 已登录但没有 `user_code` → 渲染输入框。 */
  | { readonly kind: 'input' }
  /** 正在查设备码。 */
  | { readonly kind: 'loading' }
  /** 已查到 → 展示 client 名与 scope，等用户点「允许 / 拒绝」。 */
  | { readonly kind: 'pending'; readonly info: OAuthDeviceInfo }
  /** 已批准。 */
  | { readonly kind: 'approved' }
  /** 已拒绝。 */
  | { readonly kind: 'denied' }
  /** 出错了。 */
  | { readonly kind: 'error'; readonly error: OAuthErrorView };

/** 设备码页状态机的入参。 */
export interface DeviceFlowInput {
  /** API 客户端。 */
  readonly client: ApiClient;
  /** 已登录？ */
  readonly authenticated: boolean;
  /** 浏览器环境。 */
  readonly env: OAuthPageEnv;
  /** query 里的 `user_code`（没有则 `undefined`）。 */
  readonly userCode?: string;
}

/**
 * 设备码页的**首屏**结局（**同步可判定**的部分）。
 *
 * 与授权页同款：判定只有一份，页面用它渲染首帧，`runDeviceFlow` 从它出发。
 */
export function initialDeviceOutcome(input: {
  readonly authenticated: boolean;
  readonly env: OAuthPageEnv;
  readonly userCode?: string;
  readonly prompt?: string;
}): DeviceOutcome {
  const { authenticated, env, userCode, prompt } = input;
  if (!authenticated) {
    // 与授权页同款：`prompt=none` **不许交互** → 不跳登录页，只在 URL 上透出协议码。
    if (isPromptNone(prompt)) {
      env.replaceState(`${env.pathname}${withErrorParam(env.search, 'login_required')}`);
      return { kind: 'login-required' };
    }
    // 其它情况 → 渲染「登录并继续」（`return_to` **保留** `?user_code=`）。
    return { kind: 'sign-in-required' };
  }
  if (normalizeUserCode(userCode) === '') {
    return { kind: 'input' };
  }
  return { kind: 'loading' };
}

/** 跑设备码页的状态机。 */
export async function runDeviceFlow(input: DeviceFlowInput): Promise<DeviceOutcome> {
  const { client, authenticated, env, userCode } = input;

  const initial = initialDeviceOutcome({
    authenticated,
    env,
    userCode,
    prompt: readPrompt(env.search),
  });
  if (initial.kind !== 'loading') {
    return initial;
  }

  try {
    const info = await verifyDeviceCode(client, normalizeUserCode(userCode));
    return { kind: 'pending', info };
  } catch (caught) {
    return { kind: 'error', error: describeOAuthError(caught) };
  }
}

/**
 * 规整设备码（去掉空白并**转大写**）。
 *
 * ⚠️ **不补连字符**（RFC 8628 的 `user_code` 在库里是**无分隔**的 8 位）；
 * 大写是因为字符集 `BCDFGHJKLMNPQRSTVWXZ` 全是大写。
 */
export function normalizeUserCode(raw: string | undefined): string {
  return (raw ?? '').trim().replace(/\s+/g, '').toUpperCase();
}

/** 设备码是否**形态**合法（8 位 base20）—— 只是本地提示，不代替后端判定。 */
export function isWellFormedUserCode(raw: string): boolean {
  const code = normalizeUserCode(raw);
  if (code.length !== OAUTH_USER_CODE_LENGTH) {
    return false;
  }
  for (const char of code) {
    if (!OAUTH_USER_CODE_ALPHABET.includes(char)) {
      return false;
    }
  }
  return true;
}
