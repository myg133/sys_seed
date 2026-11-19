/**
 * SSO 入站的前端 API 封装（REQ-013 / AC-S-100 · AC-S-104 · AC-S-109）。
 *
 * # 为什么重造一层
 *
 * 不重造 fetch：三个端点都走 `@base/sdk` 的 `ApiClient`
 * （错误模型 `ApiError { code, status, params }`、401 不自动刷新、`x-tenant-id` 注入
 * 都是既有能力）。本文件只做三件事：
 *
 * 1. **路径与响应形状**（后端 `design-summary.md` §3.1）；
 * 2. **形状兜底**：`providers` 是外部输入（后端 / 错误 `params`），
 *    形状不对时返回空数组而**不抛错** —— 登录页不能因为一个字段缺失就白屏；
 * 3. **浏览器交接**：OIDC 的 `authorizeUrl` 要**整页跳转**（顶层导航），
 *    不能走 `fetch`（fetch 跟随 302 只会把 IdP 的登录页当 JSON 解析）。
 *
 * # ⚠️ 租户头在「顶层导航」上传不过去
 *
 * `GET /api/iam/sso/oidc/authorize` 按 design §3 需要 `x-tenant-id`，
 * 但 `window.location.assign` **无法设置请求头**。所以相对地址（同源，也就是我们自己的
 * 端点）会附带 `?tenant=<id>`；绝对地址（IdP 自己的地址）原样保留。
 * 这一点与 P4（回调带不上租户头）是同一类缺口，**必须由后端在 authorize 上接受该查询参数**
 * —— 见自验报告「偏离规格」一节。
 *
 * # token 不进 URL（安全约束 §8 / D99）
 *
 * 本文件**不**构造任何带 token 的地址：回调只把**一次性 ticket** 交给 `/sso/callback`，
 * 换到的令牌只进 `sessionStorage`（`src/lib/session.ts`）。
 */

import { CLIENT_INTERNAL_ERROR, ApiError, type ApiClient } from '@base/sdk';

/** provider 列表（公开端点，但需要租户头）。 */
export const SSO_PROVIDERS_PATH = '/api/iam/sso/providers';

/** OIDC 授权入口（302 到 IdP）。 */
export const SSO_OIDC_AUTHORIZE_PATH = '/api/iam/sso/oidc/authorize';

/** LDAP bind 登录。 */
export const SSO_LDAP_LOGIN_PATH = '/api/iam/sso/ldap/login';

/** 一次性 ticket 换 `LoginResponse`（P4）。 */
export const SSO_EXCHANGE_PATH = '/api/iam/sso/exchange';

/** 回调交接页（公开路由；后端 302 的落点）。 */
export const SSO_CALLBACK_ROUTE_PATH = '/sso/callback';

/** 回调 URL 里的一次性 ticket 参数名（P4）。 */
export const SSO_TICKET_PARAM = 'ticket';

/** 顶层导航时补的租户参数名（见文件头「租户头传不过去」）。 */
export const SSO_TENANT_PARAM = 'tenant';

/** 租户输入后等多久再拉 providers（避免逐字符发请求）。 */
export const PROVIDER_FETCH_DEBOUNCE_MS = 250;

/** 一个可用 provider（design §3.1 的响应形状）。 */
export interface SsoProviderView {
  /** 协议类型：`oidc` / `ldap`（未知类型不渲染）。 */
  readonly type: string;
  /** 稳定标识（同一类型可能有多个时区分用）。 */
  readonly id: string;
  /** 后端给的可选展示名（前端优先用自己的文案）。 */
  readonly label?: string;
  /** OIDC：302 到 IdP 的入口。 */
  readonly authorizeUrl?: string;
  /** LDAP：bind 登录接口。 */
  readonly loginUrl?: string;
}

/**
 * `LoginResponse` 的最小形状（与 `POST /api/iam/login` 同构，AC-S-107）。
 *
 * 只声明前端用到的字段；多出来的字段不影响解析。
 */
export interface SsoLoginResponse {
  /** access token（JWT）。 */
  readonly accessToken?: string;
  /** refresh token。 */
  readonly refreshToken?: string;
  /** 令牌类型（`Bearer`）。 */
  readonly tokenType?: string;
  /** access token 剩余有效期（秒）。 */
  readonly expiresIn?: number;
  /** 账号信息（`tenantId` 决定会话落到哪个租户）。 */
  readonly user?: {
    readonly id?: string;
    readonly username?: string;
    readonly tenantId?: string;
  };
  /** 权限点（登录响应里有；refresh 响应里恒为空）。 */
  readonly permissions?: readonly string[];
}

/** LDAP 登录入参。 */
export interface SsoLdapCredentials {
  /** 租户标识（LDAP 配置是租户级的）。 */
  readonly tenantId: string;
  /** 目录用户名。 */
  readonly username: string;
  /** 目录密码。 */
  readonly password: string;
  /**
   * provider 里给的登录入口（缺省用 `SSO_LDAP_LOGIN_PATH`）。
   *
   * 遵守后端给的地址而不是写死常量：将来多 LDAP 源时路径可能变。
   */
  readonly loginUrl?: string;
}

/** 可注入的顶层导航目标（单测用假件，不碰真实浏览器）。 */
export interface LocationAssigner {
  /** 整页跳转。 */
  assign(url: string): void;
}

/** 取字符串字段（非字符串 / 空串 → `undefined`）。 */
function pickString(source: Record<string, unknown>, key: string): string | undefined {
  const value = source[key];
  return typeof value === 'string' && value !== '' ? value : undefined;
}

/** 规范化单个 provider；缺 `type` 的条目丢弃（渲染不出按钮）。 */
export function normalizeProvider(value: unknown): SsoProviderView | undefined {
  if (typeof value !== 'object' || value === null || Array.isArray(value)) {
    return undefined;
  }
  const record = value as Record<string, unknown>;
  const type = pickString(record, 'type');
  if (type === undefined) {
    return undefined;
  }
  const label = pickString(record, 'label');
  const authorizeUrl = pickString(record, 'authorizeUrl');
  const loginUrl = pickString(record, 'loginUrl');
  return {
    type,
    // `id` 缺失时退回类型：设计里一种协议只有一个 provider，缺 id 不至于丢掉整个入口。
    id: pickString(record, 'id') ?? type,
    ...(label === undefined ? {} : { label }),
    ...(authorizeUrl === undefined ? {} : { authorizeUrl }),
    ...(loginUrl === undefined ? {} : { loginUrl }),
  };
}

/**
 * 规范化 provider 列表。
 *
 * 接受两种形状：裸数组（错误 `params.ssoProviders` 可能直接是数组）
 * 与 `{ providers: [...] }`（providers 端点的响应）。其它形状 → 空数组。
 */
export function normalizeProviders(payload: unknown): readonly SsoProviderView[] {
  const list = Array.isArray(payload)
    ? payload
    : ((payload as { providers?: unknown } | undefined)?.providers ?? undefined);
  if (!Array.isArray(list)) {
    return [];
  }
  const providers: SsoProviderView[] = [];
  for (const item of list) {
    const provider = normalizeProvider(item);
    if (provider !== undefined) {
      providers.push(provider);
    }
  }
  return providers;
}

/**
 * 从 `SSO_REQUIRED` 的 `params` 里取 provider 列表（AC-S-021 步骤 1）。
 *
 * 后端把 `ssoProviders` **写死在 `params` 里**（D109）；
 * 它既可能是数组，也可能被中间层序列化成 JSON 字符串 —— 两种都认。
 * 取不到 → 空数组（页面因此不渲染企业登录区块，而不是显示一个坏按钮）。
 */
export function providersFromParams(
  params: Record<string, unknown> | undefined,
): readonly SsoProviderView[] {
  const raw = params?.['ssoProviders'];
  if (typeof raw === 'string') {
    if (raw.trim() === '') {
      return [];
    }
    try {
      return normalizeProviders(JSON.parse(raw));
    } catch {
      return [];
    }
  }
  return normalizeProviders(raw);
}

/**
 * 读当前租户可用的 provider（AC-S-109 步骤 1）。
 *
 * 显式带 `x-tenant-id`：登录页此刻还没有会话，SDK 上下文里也没有租户。
 * 租户为空 → **不发请求**（后端必然无法决定用哪个 IdP），直接返回空列表。
 */
export async function listProviders(
  client: ApiClient,
  tenantId: string,
): Promise<readonly SsoProviderView[]> {
  const tenant = tenantId.trim();
  if (tenant === '') {
    return [];
  }
  const payload = await client.get<unknown>(SSO_PROVIDERS_PATH, {
    headers: { 'x-tenant-id': tenant },
  });
  return normalizeProviders(payload);
}

/**
 * LDAP bind 登录（AC-S-104）。
 *
 * `skipAuthRetry: true` 与本地登录同理（D100）：此刻没有 refresh token，
 * 让 401 触发刷新会把 `SSO_LDAP_BIND_FAILED` 覆盖成 `TOKEN_REVOKED`。
 */
export async function ldapLogin(
  client: ApiClient,
  credentials: SsoLdapCredentials,
): Promise<SsoLoginResponse> {
  const tenant = credentials.tenantId.trim();
  return client.post<SsoLoginResponse>(
    credentials.loginUrl ?? SSO_LDAP_LOGIN_PATH,
    { username: credentials.username, password: credentials.password },
    {
      ...(tenant === '' ? {} : { headers: { 'x-tenant-id': tenant } }),
      skipAuthRetry: true,
    },
  );
}

/**
 * 用一次性 ticket 换会话（P4）。
 *
 * 不带 `x-tenant-id`：ticket 本身绑租户（后端按 `state.tenant_id` 落库），
 * 浏览器从 IdP 跳回来时也没有租户头（P3）。
 */
export async function exchangeTicket(client: ApiClient, ticket: string): Promise<SsoLoginResponse> {
  return client.post<SsoLoginResponse>(SSO_EXCHANGE_PATH, { ticket }, { skipAuthRetry: true });
}

/**
 * 从 `location.search` 里读 ticket。
 *
 * 缺参数 / 空值 → `undefined`（回调页据此显示「登录链接无效或已过期」，
 * **不发**一个必然 401 的 exchange 请求）。
 */
export function readTicket(search: string): string | undefined {
  const query = search.startsWith('?') ? search.slice(1) : search;
  if (query.trim() === '') {
    return undefined;
  }
  let params: URLSearchParams;
  try {
    params = new URLSearchParams(query);
  } catch {
    return undefined;
  }
  const ticket = params.get(SSO_TICKET_PARAM)?.trim();
  return ticket === undefined || ticket === '' ? undefined : ticket;
}

/**
 * 顶层导航的目标地址。
 *
 * 相对地址（同源端点）：补 `?tenant=`（无法设置请求头，见文件头）。
 * 绝对地址（IdP 自己）：原样返回 —— 不往第三方地址塞我们的租户。
 */
export function authorizeNavigationUrl(authorizeUrl: string, tenantId: string): string {
  const tenant = tenantId.trim();
  if (tenant === '') {
    return authorizeUrl;
  }
  // `//host/path` 是协议相对地址（属于**另一个源**），按绝对地址处理。
  if (!authorizeUrl.startsWith('/') || authorizeUrl.startsWith('//')) {
    return authorizeUrl;
  }
  const hashIndex = authorizeUrl.indexOf('#');
  const hash = hashIndex === -1 ? '' : authorizeUrl.slice(hashIndex);
  const pathAndQuery = hashIndex === -1 ? authorizeUrl : authorizeUrl.slice(0, hashIndex);
  let parsed: URL;
  try {
    parsed = new URL(pathAndQuery, 'http://sso.invalid');
  } catch {
    return authorizeUrl;
  }
  if (!parsed.searchParams.has(SSO_TENANT_PARAM)) {
    parsed.searchParams.set(SSO_TENANT_PARAM, tenant);
  }
  return `${parsed.pathname}${parsed.search}${hash}`;
}

/**
 * 整页跳转到 IdP 授权入口（`type: "oidc"` 的点击行为）。
 *
 * 必须是**顶层导航**：`fetch` 跟随 302 会把 IdP 的登录页当响应体解析掉。
 */
export function startOidcAuthorize(
  authorizeUrl: string,
  tenantId: string,
  target?: LocationAssigner,
): void {
  const assigner = target ?? (globalThis as { location?: LocationAssigner }).location;
  if (assigner === undefined || typeof assigner.assign !== 'function') {
    // 非浏览器环境（单测 / SSR）没有导航能力：调用方没注入假件时按客户端错误处理。
    throw new ApiError({ code: CLIENT_INTERNAL_ERROR, status: 0 });
  }
  assigner.assign(authorizeNavigationUrl(authorizeUrl, tenantId));
}
