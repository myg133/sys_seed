/**
 * SDK 占位包。
 *
 * 本期只提供 API 客户端的构造入口与 URL 拼接，不发真实请求。
 * 具体的接口调用在 REQ-011 展开。
 */

/** API 客户端构造参数。 */
export interface ApiClientOptions {
  /** 服务端地址，例如 `https://api.base.example`。 */
  readonly baseUrl: string;
}

/** API 客户端。 */
export interface ApiClient {
  /** 规范化后的服务端地址（去掉结尾斜杠）。 */
  readonly baseUrl: string;
  /** 拼接完整 URL。 */
  url(path: string): string;
}

/** 构造 API 客户端。 */
export function createApiClient(options: ApiClientOptions): ApiClient {
  const baseUrl = options.baseUrl.replace(/\/+$/, '');
  return {
    baseUrl,
    url(path: string): string {
      const suffix = path.startsWith('/') ? path : `/${path}`;
      return `${baseUrl}${suffix}`;
    },
  };
}
