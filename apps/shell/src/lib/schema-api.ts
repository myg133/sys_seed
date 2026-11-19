/**
 * 数据页面的接口装配（REQ-012 / AC-S-082 ~ 086）。
 *
 * # 为什么单拉一层
 *
 * 组件只负责**渲染**；取元数据、取列表、写记录都是可单测的纯请求逻辑。
 * 组件里直接写 `client.get(...)` 会让「路径怎么拼」「写请求发几次」无法单测 ——
 * 而这两件事正是 AC-S-083 / 086 的判据。
 *
 * # 更新方法：PATCH，405 时退回 PUT
 *
 * 前端与后端并行开发，更新动词没有在元数据里声明。平台既有写接口用 PUT
 * （`platform-config` 的 `put(entries)`），而 `@base/sdk` 同时支持 PATCH / PUT。
 * 这里先发 PATCH；**只有**收到 405（路径存在但方法不允许）才退回 PUT ——
 * 405 意味着服务端**什么都没做**，重试是安全的；其它错误一律不重试，
 * 否则会把「真正的失败」变成两次写入。
 */

import type { ListPage, ApiEndpoints } from '@base/schema';
import {
  buildListQuery,
  parseEntityMeta,
  parseListResponse,
  recordId,
  resolveApiPath,
  type EntityMeta,
} from '@base/schema';
import { ApiError, type ApiClient } from '@base/sdk';

/** 元数据接口路径。 */
export function schemaMetaPath(moduleId: string, entityKey: string): string {
  return `/api/schema/${encodeURIComponent(moduleId)}/${encodeURIComponent(entityKey)}`;
}

/**
 * 读取实体元数据。
 *
 * 响应不是合法元数据（缺 `api`）时抛错 —— 没有数据接口地址就取不到数据，
 * 与其白屏不如把原因显示出来。
 */
export async function loadEntityMeta(
  client: ApiClient,
  moduleId: string,
  entityKey: string,
): Promise<EntityMeta> {
  const raw = await client.get<unknown>(schemaMetaPath(moduleId, entityKey));
  const meta = parseEntityMeta(raw);
  if (meta === undefined) {
    throw new Error('元数据缺少 api 声明（list / create 必填）');
  }
  return meta;
}

/** 取一页列表（游标分页；响应形状 `{ items, nextCursor }`）。 */
export async function loadPage(
  client: ApiClient,
  api: ApiEndpoints,
  cursor: string | null,
  limit?: number,
): Promise<ListPage> {
  const raw = await client.request<unknown>({
    path: api.list,
    query: buildListQuery(cursor, limit),
  });
  return parseListResponse(raw);
}

/** 创建：`POST api.create`。 */
export async function createRecord(
  client: ApiClient,
  api: ApiEndpoints,
  body: Record<string, unknown>,
): Promise<void> {
  await client.post(api.create, body);
}

/**
 * 更新：`PATCH api.update`（`{id}` 替换为记录 id）；405 时退回 `PUT`。
 *
 * 未声明 `update` 时**抛错**（调用方本来就不该渲染「编辑」按钮）。
 */
export async function updateRecord(
  client: ApiClient,
  api: ApiEndpoints,
  id: string,
  body: Record<string, unknown>,
): Promise<void> {
  if (api.update === undefined) {
    throw new Error('该实体未声明更新接口（api.update）');
  }
  const path = resolveApiPath(api.update, id);
  try {
    await client.request({ method: 'PATCH', path, body });
  } catch (error) {
    if (error instanceof ApiError && error.status === 405) {
      // 405 = 路径存在但方法不允许 → 服务端没有处理，退回 PUT 不会重复写入。
      await client.request({ method: 'PUT', path, body });
      return;
    }
    throw error;
  }
}

/** 删除：`DELETE api.delete`（`{id}` 替换为记录 id）。 */
export async function deleteRecord(
  client: ApiClient,
  api: ApiEndpoints,
  record: Record<string, unknown>,
): Promise<void> {
  if (api.delete === undefined) {
    throw new Error('该实体未声明删除接口（api.delete）');
  }
  await client.delete(resolveApiPath(api.delete, recordId(record)));
}

/** 把异常转成可展示的文案（错误码要透出，不能只剩「出错了」）。 */
export function describeError(error: unknown): string {
  if (error instanceof ApiError) {
    return `${error.code}（HTTP ${error.status}）`;
  }
  if (error instanceof Error && error.message !== '') {
    return error.message;
  }
  return '请求失败';
}
