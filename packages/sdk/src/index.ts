/**
 * @base/sdk —— 类型安全的 API 客户端（REQ-011 / AC-S-069 ~ AC-S-072）。
 *
 * # 四件必须做对的事
 *
 * | 行为 | 约定 |
 * |---|---|
 * | 错误模型 | RFC 9457 → `ApiError { code, status, title, detail }`；`code` 优先；**原始响应体不进错误信息** |
 * | token 刷新 | 401 → 刷新 → 重放；**并发去重**（共享同一个 in-flight Promise）；请求体先缓冲成字符串 |
 * | 认证端点 | 登录 / 刷新（`skipAuthRetry`）的 401 **不刷新、不上报会话失效**，错误码原样透传（D100） |
 * | 幂等键 | 写方法自动带 UUID v4；读方法不带；每次请求新键；**401 重放不带**（D92） |
 * | 上下文头 | `x-tenant-id` / `x-space-id`；未配置时**不带头**；注入于所有请求（含 login / refresh） |
 *
 * # 为什么公开 API 与实现同文件
 *
 * `ApiError` 既是**公开类型**（消费方要按 `code` 分派文案），又被内部抛出 ——
 * 拆到子模块就会形成 `index → client → index` 的循环依赖。
 * 这个包没有状态、没有副作用，留在一个文件里更省事，也让能力索引
 * （`cargo xtask capabilities` 只扫 `src/index.ts`）拿得到完整导出清单。
 */

/** 客户端合成码：fetch 抛错（网络层失败，根本没有后端响应）。 */
export const CLIENT_NETWORK_ERROR = 'client.NETWORK_ERROR';

/** 客户端合成码：收到响应但无法解析为错误体（非 JSON / 空体 / 截断）。 */
export const CLIENT_INTERNAL_ERROR = 'client.INTERNAL_ERROR';

/** API 错误的构造参数。 */
export interface ApiErrorParams {
  /** 稳定错误码：后端码原样透传，或 `client.*` 合成码。 */
  readonly code: string;
  /** HTTP 状态码；网络层失败时为 `0`。 */
  readonly status: number;
  /** 问题详情的标题（后端给的、人可读的一行）。 */
  readonly title?: string;
  /** 问题详情的描述。 */
  readonly detail?: string;
}

/**
 * API 错误。
 *
 * `code` 是**唯一**该被消费方信任的字段（文案由 i18n 按码渲染）；
 * `message` 是固定文案，**不含**响应体的任何内容。
 */
export class ApiError extends Error {
  /** 稳定错误码。 */
  readonly code: string;
  /** HTTP 状态码（网络层失败为 `0`）。 */
  readonly status: number;
  /** 问题详情的标题。 */
  readonly title?: string;
  /** 问题详情的描述。 */
  readonly detail?: string;

  constructor(params: ApiErrorParams) {
    super(`API 请求失败（code=${params.code}）`);
    this.name = 'ApiError';
    this.code = params.code;
    this.status = params.status;
    this.title = params.title;
    this.detail = params.detail;
  }
}

/** 请求方法。 */
export type HttpMethod = 'GET' | 'POST' | 'PUT' | 'DELETE' | 'PATCH' | 'HEAD';

/** 请求选项。 */
export interface RequestOptions {
  /** 请求方法（默认 `GET`）。 */
  readonly method?: HttpMethod;
  /** 请求路径（相对 `baseUrl`）。 */
  readonly path: string;
  /** 请求体（会被 JSON 序列化；`undefined` 表示不带体）。 */
  readonly body?: unknown;
  /** 查询参数；值为 `undefined` 的键**不拼进** query string。 */
  readonly query?: Record<string, string | number | boolean | undefined>;
  /** 额外请求头。 */
  readonly headers?: Record<string, string>;
  /** 是否带幂等键（默认：写方法 true / 读方法 false）。 */
  readonly idempotent?: boolean;
  /**
   * 该请求是**认证端点**（登录 / 刷新）：401 时不触发自动刷新，也不上报会话失效。
   *
   * # 为什么必须有这个开关（D100）
   *
   * 「刷新救不了密码错」。若登录的 401 也去刷新：
   * ① 此刻会话里还没有 refresh token → 刷新必然失败 → 抛 `TOKEN_REVOKED`，
   *    **把真正的 `INVALID_CREDENTIALS` 覆盖掉**（用户看到错误的失败原因）；
   * ② 每次登录失败多打一次 `/api/iam/refresh`；
   * ③ 已登录用户误在登录页输错密码时，还会把**仍然有效**的会话清掉。
   *
   * 刷新请求自身还受「不递归刷新」的保护（`Client.refresh()` 的 scoped 语义），
   * 两者是不同的问题：scoped 解决「刷新等自己 → 死锁」，本开关解决「认证 401 被误当会话失效」。
   */
  readonly skipAuthRetry?: boolean;
}

/** 请求选项（去掉 `path` / `method` 后的快捷方式入参）。 */
export type RequestOptionsWithoutTarget = Omit<RequestOptions, 'path' | 'method'>;

/** API 客户端。 */
export interface ApiClient {
  /** 规范化后的服务端地址（去掉结尾斜杠）。 */
  readonly baseUrl: string;
  /** 发起请求；失败抛 `ApiError`。 */
  request<T = unknown>(options: RequestOptions): Promise<T>;
  /** GET 快捷方式。 */
  get<T = unknown>(path: string, options?: RequestOptionsWithoutTarget): Promise<T>;
  /** POST 快捷方式。 */
  post<T = unknown>(
    path: string,
    body?: unknown,
    options?: RequestOptionsWithoutTarget,
  ): Promise<T>;
  /** PUT 快捷方式。 */
  put<T = unknown>(path: string, body?: unknown, options?: RequestOptionsWithoutTarget): Promise<T>;
  /** DELETE 快捷方式。 */
  delete<T = unknown>(path: string, options?: RequestOptionsWithoutTarget): Promise<T>;
  /**
   * 发起**刷新**请求（`POST`）。
   *
   * 刷新请求走同一个客户端时，它的 401 **不会**触发再次刷新 ——
   * 否则刷新请求会等自己（死锁）。宿主实现 `refreshToken` 回调时
   * 应优先用这个方法。
   */
  refresh<T = unknown>(
    path: string,
    body?: unknown,
    options?: RequestOptionsWithoutTarget,
  ): Promise<T>;
  /** 更新租户 / 空间上下文（`undefined` 表示「不变」）。 */
  setContext(context: { tenantId?: string; spaceId?: string }): void;
  /** 拼接完整 URL（与 `baseUrl` 拼接，不带查询串）。 */
  url(path: string): string;
}

/** API 客户端构造参数。 */
export interface ApiClientOptions {
  /** 服务端地址，例如 `https://api.base.example`；空串表示同源相对路径。 */
  readonly baseUrl: string;
  /** 可注入的 fetch（单测用假件；默认用全局 fetch）。 */
  readonly fetch?: typeof fetch;
  /** 取当前 access token（每次发请求时读一次 —— 刷新后要能拿到新令牌）。 */
  readonly getAccessToken?: () => string | undefined;
  /** 刷新 access token；返回新的 access token，失败时抛错。 */
  readonly refreshToken?: () => Promise<string | undefined>;
  /** 租户上下文（`x-tenant-id`）；未配置时**不带头**。 */
  readonly tenantId?: string;
  /** 空间上下文（`x-space-id`）；未配置时**不带头**。 */
  readonly spaceId?: string;
  /** 会话失效时的回调（刷新失败 / 401 无法恢复）。 */
  readonly onUnauthorized?: () => void;
}

/** 读方法：不带幂等键（后端幂等中间件对它们直接放行）。 */
const READ_METHODS = new Set<string>(['GET', 'HEAD']);

/** 客户端内部的可变上下文。 */
interface ClientContext {
  tenantId?: string;
  spaceId?: string;
}

/** 判断是否要带幂等键。 */
function wantsIdempotency(options: RequestOptions, method: string): boolean {
  if (options.idempotent !== undefined) {
    return options.idempotent;
  }
  return !READ_METHODS.has(method);
}

/** 生成 UUID v4（幂等键的格式约束：AC-S-071 步骤 5）。 */
function randomUuidV4(): string {
  const webCrypto = globalThis.crypto;
  if (webCrypto && typeof webCrypto.randomUUID === 'function') {
    return webCrypto.randomUUID();
  }
  const bytes = new Uint8Array(16);
  if (webCrypto && typeof webCrypto.getRandomValues === 'function') {
    webCrypto.getRandomValues(bytes);
  } else {
    for (let index = 0; index < bytes.length; index += 1) {
      bytes[index] = Math.floor(Math.random() * 256);
    }
  }
  const byte = (index: number): number => bytes[index] ?? 0;
  bytes[6] = (byte(6) & 0x0f) | 0x40;
  bytes[8] = (byte(8) & 0x3f) | 0x80;
  const hex = Array.from(bytes, (value) => value.toString(16).padStart(2, '0')).join('');
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
}

/** 同源相对路径的基址：浏览器取当前地址；Node（单测）用占位基址。 */
function currentBase(): string {
  const candidate = (globalThis as { location?: { href?: string } }).location;
  return candidate?.href ?? 'http://localhost';
}

/** 把（可能相对的）请求地址补成绝对地址（`Request` 只收绝对地址）。 */
function toAbsoluteUrl(url: string): string {
  try {
    return new URL(url).toString();
  } catch {
    return new URL(url, currentBase()).toString();
  }
}

/** 拼查询串；值为 `undefined` 的键不拼进去。 */
function buildQueryString(query: RequestOptions['query']): string {
  const search = new URLSearchParams();
  for (const [key, value] of Object.entries(query ?? {})) {
    if (value === undefined) {
      continue;
    }
    search.append(key, String(value));
  }
  const text = search.toString();
  return text === '' ? '' : `?${text}`;
}

/** 从错误体里取出可用的字段（**只取白名单字段**，原始响应体不进错误信息）。 */
function readProblemFields(payload: unknown): {
  code: string;
  title?: string;
  detail?: string;
} {
  if (typeof payload !== 'object' || payload === null || Array.isArray(payload)) {
    return { code: CLIENT_INTERNAL_ERROR };
  }
  const record = payload as Record<string, unknown>;
  const code = record['code'];
  if (typeof code !== 'string' || code === '') {
    return { code: CLIENT_INTERNAL_ERROR };
  }
  const title = record['title'];
  const detail = record['detail'];
  return {
    code,
    title: typeof title === 'string' ? title : undefined,
    detail: typeof detail === 'string' ? detail : undefined,
  };
}

/** 构造 API 客户端。 */
export function createApiClient(options: ApiClientOptions): ApiClient {
  const baseUrl = options.baseUrl.replace(/\/+$/, '');
  const fetchImpl = options.fetch ?? ((input, init) => globalThis.fetch(input, init));
  const context: ClientContext = {
    tenantId: options.tenantId,
    spaceId: options.spaceId,
  };
  /** 正在进行的刷新（并发去重靠它）。 */
  let refreshInFlight: Promise<string | undefined> | null = null;
  /**
   * 刷新回调的**同步执行窗口**：为真时发出的请求被视为「刷新请求本身」。
   *
   * 没有它，宿主若在 `refreshToken` 回调里用同一个客户端发刷新请求，
   * 那个请求的 401 会去等正在进行的刷新 → **死锁**。
   */
  let refreshScopeArmed = false;

  /** 拼接完整 URL。 */
  function url(path: string): string {
    const suffix = path.startsWith('/') ? path : `/${path}`;
    return `${baseUrl}${suffix}`;
  }

  /**
   * 发起一次请求。
   *
   * @param attempt 0 = 首次，1 = 401 刷新后的重放
   * @param scopedToRefresh 该请求是否就是刷新请求本身（它的 401 不触发刷新）
   */
  async function send<T>(
    requestOptions: RequestOptions,
    method: string,
    bodyText: string | undefined,
    attempt: number,
    scopedToRefresh: boolean,
  ): Promise<T> {
    const headers = new Headers();
    headers.set('accept', 'application/json');
    if (bodyText !== undefined) {
      headers.set('content-type', 'application/json');
    }

    const token = options.getAccessToken?.();
    if (token !== undefined && token !== '') {
      headers.set('authorization', `Bearer ${token}`);
    }
    // 上下文头：未配置时**不带头**（空串 ≠ 不带 —— 后端会从令牌推断租户）。
    if (context.tenantId !== undefined && context.tenantId !== '') {
      headers.set('x-tenant-id', context.tenantId);
    }
    if (context.spaceId !== undefined && context.spaceId !== '') {
      headers.set('x-space-id', context.spaceId);
    }
    // 幂等键：每次请求新生成；**重放不带**（D92 —— 401 意味着请求没执行，
    // 沿用旧键会被后端判为「还在处理」→ 409 CONFLICT）。
    if (attempt === 0 && wantsIdempotency(requestOptions, method)) {
      headers.set('idempotency-key', randomUuidV4());
    }
    for (const [name, value] of Object.entries(requestOptions.headers ?? {})) {
      headers.set(name, value);
    }
    if (attempt > 0) {
      headers.delete('idempotency-key');
    }

    const init: RequestInit = { method, headers };
    if (bodyText !== undefined && !READ_METHODS.has(method)) {
      init.body = bodyText;
    }

    let response: Response;
    try {
      response = await fetchImpl(
        new Request(
          toAbsoluteUrl(`${url(requestOptions.path)}${buildQueryString(requestOptions.query)}`),
          init,
        ),
      );
    } catch {
      // 网络层失败：没有后端响应 → 合成码；不抛裸 TypeError，也不带原始响应体。
      throw new ApiError({ code: CLIENT_NETWORK_ERROR, status: 0 });
    }

    // 认证端点（登录 / 刷新）的 401 不参与自动刷新（D100）：
    // 密码错时刷新必然失败，会把真正的错误码覆盖成 `TOKEN_REVOKED`。
    const canRefresh =
      attempt === 0 &&
      options.refreshToken !== undefined &&
      !scopedToRefresh &&
      requestOptions.skipAuthRetry !== true;
    if (response.status === 401 && canRefresh) {
      try {
        if (refreshInFlight === null) {
          await refreshAccessToken();
        } else {
          // 并发窗口：等**正在进行的**那一次刷新（只发一次 refresh）。
          await refreshInFlight;
        }
      } catch (error) {
        options.onUnauthorized?.();
        throw error;
      }
      return send<T>(requestOptions, method, bodyText, 1, false);
    }

    if (!response.ok) {
      // 认证端点的 401 不是「会话已死」，而是「这次凭据不对」——
      // 不能借它清掉可能仍然有效的会话（D100 ③）。
      if (response.status === 401 && refreshInFlight === null && !requestOptions.skipAuthRetry) {
        options.onUnauthorized?.();
      }
      throw await toApiError(response);
    }

    return parseSuccessBody<T>(response);
  }

  /** 刷新令牌；并发调用共享同一个 in-flight Promise（只发一次 refresh）。 */
  function refreshAccessToken(): Promise<string | undefined> {
    if (refreshInFlight !== null) {
      return refreshInFlight;
    }
    const refresh = options.refreshToken;
    if (refresh === undefined) {
      return Promise.reject(new ApiError({ code: CLIENT_INTERNAL_ERROR, status: 401 }));
    }
    refreshScopeArmed = true;
    let pending: Promise<string | undefined>;
    try {
      // 回调的同步执行段里发出的请求 = 刷新请求本身。
      pending = refresh();
    } finally {
      refreshScopeArmed = false;
    }
    refreshInFlight = pending;
    const clear = (): void => {
      if (refreshInFlight === pending) {
        refreshInFlight = null;
      }
    };
    void pending.then(clear, clear);
    return pending;
  }

  async function request<T>(requestOptions: RequestOptions): Promise<T> {
    const method = requestOptions.method ?? 'GET';
    // 请求体先缓冲成字符串 —— 流式 body 不可复用，重放会发空 body。
    const bodyText =
      requestOptions.body === undefined ? undefined : JSON.stringify(requestOptions.body);
    return send<T>(requestOptions, method, bodyText, 0, refreshScopeArmed);
  }

  return {
    baseUrl,
    url,
    request,
    get<T>(path: string, requestOptions?: RequestOptionsWithoutTarget): Promise<T> {
      return request<T>({ ...requestOptions, path, method: 'GET' });
    },
    post<T>(
      path: string,
      body?: unknown,
      requestOptions?: RequestOptionsWithoutTarget,
    ): Promise<T> {
      return request<T>({ ...requestOptions, path, method: 'POST', body });
    },
    put<T>(path: string, body?: unknown, requestOptions?: RequestOptionsWithoutTarget): Promise<T> {
      return request<T>({ ...requestOptions, path, method: 'PUT', body });
    },
    delete<T>(path: string, requestOptions?: RequestOptionsWithoutTarget): Promise<T> {
      return request<T>({ ...requestOptions, path, method: 'DELETE' });
    },
    refresh<T>(
      path: string,
      body?: unknown,
      requestOptions?: RequestOptionsWithoutTarget,
    ): Promise<T> {
      const method = 'POST';
      const bodyText = body === undefined ? undefined : JSON.stringify(body);
      return send<T>({ ...requestOptions, path, method }, method, bodyText, 0, true);
    },
    setContext(next: { tenantId?: string; spaceId?: string }): void {
      if (next.tenantId !== undefined) {
        context.tenantId = next.tenantId;
      }
      if (next.spaceId !== undefined) {
        context.spaceId = next.spaceId;
      }
    },
  };
}

/** 成功响应 → 解析后的业务结果；空体 → `undefined`；非 JSON → `client.INTERNAL_ERROR`。 */
async function parseSuccessBody<T>(response: Response): Promise<T> {
  const raw = await response.text();
  if (raw.trim() === '') {
    // 空体（204 / 无内容）不是错误。
    return undefined as unknown as T;
  }
  try {
    return JSON.parse(raw) as T;
  } catch {
    throw new ApiError({ code: CLIENT_INTERNAL_ERROR, status: response.status });
  }
}

/** 失败响应 → `ApiError`（`code` 优先；原始响应体不进错误信息）。 */
async function toApiError(response: Response): Promise<ApiError> {
  let payload: unknown;
  try {
    const raw = await response.text();
    payload = raw.trim() === '' ? undefined : JSON.parse(raw);
  } catch {
    payload = undefined;
  }
  const fields = readProblemFields(payload);
  return new ApiError({
    code: fields.code,
    status: response.status,
    title: fields.title,
    detail: fields.detail,
  });
}
