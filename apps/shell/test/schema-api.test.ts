import { describe, expect, it } from 'vitest';

import type { ApiEndpoints } from '@base/schema';
import { ApiError, type ApiClient, type RequestOptions } from '@base/sdk';

import {
  createRecord,
  deleteRecord,
  describeError,
  loadEntityMeta,
  loadPage,
  updateRecord,
} from '../src/lib/schema-api';

/** 一次被记录的请求。 */
interface RecordedCall {
  readonly method: string;
  readonly path: string;
  readonly body?: unknown;
  readonly query?: Record<string, string | number | boolean | undefined>;
}

/** 假客户端：记录调用，按处理器返回 / 抛错。 */
function fakeClient(handler: (call: RecordedCall) => unknown = () => undefined): {
  client: ApiClient;
  calls: RecordedCall[];
} {
  const calls: RecordedCall[] = [];

  const request = async <T>(options: RequestOptions): Promise<T> => {
    const call: RecordedCall = {
      method: options.method ?? 'GET',
      path: options.path,
      body: options.body,
      query: options.query,
    };
    calls.push(call);
    return (await handler(call)) as T;
  };

  const client = {
    baseUrl: '',
    request,
    get: <T>(path: string): Promise<T> => request<T>({ path, method: 'GET' }),
    post: <T>(path: string, body?: unknown): Promise<T> =>
      request<T>({ path, method: 'POST', body }),
    put: <T>(path: string, body?: unknown): Promise<T> => request<T>({ path, method: 'PUT', body }),
    delete: <T>(path: string): Promise<T> => request<T>({ path, method: 'DELETE' }),
    refresh: <T>(path: string, body?: unknown): Promise<T> =>
      request<T>({ path, method: 'POST', body }),
    setContext: (): void => undefined,
    url: (path: string): string => path,
  } as unknown as ApiClient;

  return { client, calls };
}

/** 元数据响应（含 api 段）。 */
const META_BODY = {
  module: 'sandbox',
  entity: 'demoItem',
  label: '示例条目',
  api: {
    list: '/api/sandbox/demo-items',
    create: '/api/sandbox/demo-items',
    update: '/api/sandbox/demo-items/{id}',
    delete: '/api/sandbox/demo-items/{id}',
  },
  fields: [{ key: 'title', label: '标题', type: 'text' }],
};

/** 接口声明（与 META_BODY.api 一致）。 */
const API: ApiEndpoints = {
  list: '/api/sandbox/demo-items',
  create: '/api/sandbox/demo-items',
  update: '/api/sandbox/demo-items/{id}',
  delete: '/api/sandbox/demo-items/{id}',
};

describe('loadEntityMeta —— 元数据请求（AC-S-085）', () => {
  it('请求路径 = /api/schema/<module>/<entity>，并解析出 api 段', async () => {
    const { client, calls } = fakeClient(() => META_BODY);
    const meta = await loadEntityMeta(client, 'sandbox', 'demoItem');
    expect(calls).toHaveLength(1);
    expect(calls[0]?.path).toBe('/api/schema/sandbox/demoItem');
    expect(meta.api.list).toBe('/api/sandbox/demo-items');
    expect(meta.fields.map((field) => field.key)).toEqual(['title']);
  });

  it('响应缺 api → 抛错（不静默渲染一个取不到数的页面）', async () => {
    const { client } = fakeClient(() => ({ module: 'sandbox', entity: 'demoItem', fields: [] }));
    await expect(loadEntityMeta(client, 'sandbox', 'demoItem')).rejects.toThrow('api');
  });
});

describe('loadPage —— 游标分页请求（TC-012-038 / 039）', () => {
  it('首屏：不带 cursor，limit=20', async () => {
    const { client, calls } = fakeClient(() => ({ items: [], nextCursor: null }));
    const page = await loadPage(client, API, null, 20);
    expect(calls[0]?.path).toBe('/api/sandbox/demo-items');
    expect(calls[0]?.query).toEqual({ limit: '20' });
    expect(page).toEqual({ items: [], nextCursor: null });
  });

  it('下一页：cursor 取自上一页的 nextCursor', async () => {
    const { client, calls } = fakeClient(() => ({ items: [{ id: 'item-3' }], nextCursor: null }));
    const page = await loadPage(client, API, 'c2VxOjIw', 20);
    expect(calls[0]?.query).toEqual({ cursor: 'c2VxOjIw', limit: '20' });
    expect(page.nextCursor).toBeNull();
    expect(page.items).toEqual([{ id: 'item-3' }]);
  });
});

describe('写请求（AC-S-086）', () => {
  it('创建 → POST api.create，body 原样', async () => {
    const { client, calls } = fakeClient();
    await createRecord(client, API, { title: '示例' });
    expect(calls).toEqual([
      {
        method: 'POST',
        path: '/api/sandbox/demo-items',
        body: { title: '示例' },
        query: undefined,
      },
    ]);
  });

  it('更新 → PATCH，路径替换 {id}', async () => {
    const { client, calls } = fakeClient();
    await updateRecord(client, API, 'item-1', { title: '改后标题' });
    expect(calls.map((call) => call.method)).toEqual(['PATCH']);
    expect(calls[0]?.path).toBe('/api/sandbox/demo-items/item-1');
    expect(calls[0]?.body).toEqual({ title: '改后标题' });
  });

  it('更新遇 405 → 退回 PUT（服务端没处理过，重试安全）', async () => {
    const { client, calls } = fakeClient((call) => {
      if (call.method === 'PATCH') {
        throw new ApiError({ code: 'METHOD_NOT_ALLOWED', status: 405 });
      }
      return undefined;
    });
    await updateRecord(client, API, 'item-1', { title: 'x' });
    expect(calls.map((call) => call.method)).toEqual(['PATCH', 'PUT']);
    expect(calls[1]?.path).toBe('/api/sandbox/demo-items/item-1');
  });

  it('更新遇 500 → 不重试（避免把失败变成两次写入）', async () => {
    const { client, calls } = fakeClient(() => {
      throw new ApiError({ code: 'INTERNAL', status: 500 });
    });
    await expect(updateRecord(client, API, 'item-1', { title: 'x' })).rejects.toThrow('INTERNAL');
    expect(calls.map((call) => call.method)).toEqual(['PATCH']);
  });

  it('删除 → DELETE 路径替换 {id}', async () => {
    const { client, calls } = fakeClient();
    await deleteRecord(client, API, { id: 'item-7' });
    expect(calls).toEqual([
      {
        method: 'DELETE',
        path: '/api/sandbox/demo-items/item-7',
        body: undefined,
        query: undefined,
      },
    ]);
  });

  it('未声明 update / delete → 抛错（调用方本就不该渲染按钮）', async () => {
    const readOnlyApi: ApiEndpoints = { list: '/l', create: '/c' };
    const { client, calls } = fakeClient();
    await expect(updateRecord(client, readOnlyApi, 'item-1', {})).rejects.toThrow('api.update');
    await expect(deleteRecord(client, readOnlyApi, { id: 'item-1' })).rejects.toThrow('api.delete');
    expect(calls).toHaveLength(0);
  });
});

describe('describeError —— 错误码要透出', () => {
  it('ApiError → 码 + 状态', () => {
    expect(describeError(new ApiError({ code: 'SCHEMA_ENTITY_NOT_FOUND', status: 404 }))).toBe(
      'SCHEMA_ENTITY_NOT_FOUND（HTTP 404）',
    );
  });

  it('普通 Error → message；未知值 → 通用文案', () => {
    expect(describeError(new Error('元数据缺少 api 声明'))).toBe('元数据缺少 api 声明');
    expect(describeError(undefined)).toBe('请求失败');
  });
});
