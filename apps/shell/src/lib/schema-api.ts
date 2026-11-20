/**
 * 数据页面的接口装配（REQ-012 / AC-S-082 ~ 086）。
 *
 * # 为什么单拉一层
 *
 * 组件只负责**渲染**；取元数据、取列表、写记录都是可单测的纯请求逻辑。
 * 组件里直接写 `client.get(...)` 会让「路径怎么拼」「写请求发几次」无法单测 ——
 * 而这两件事正是 AC-S-083 / 086 的判据。
 *
 * # 更新方法：PATCH，405 时退回 PUT，**并记住结果**
 *
 * 前端与后端并行开发，更新动词没有在元数据里声明。平台既有写接口用 PUT
 * （`platform-config` 的 `put(entries)`），而 `@base/sdk` 同时支持 PATCH / PUT。
 * 这里先发 PATCH；**只有**收到 405（路径存在但方法不允许）才退回 PUT ——
 * 405 意味着服务端**什么都没做**，重试是安全的；其它错误一律不重试，
 * 否则会把「真正的失败」变成两次写入。
 *
 * 退回一次之后**把方法记下来**：同一个 `api.update` 路径模板的后续编辑直接发 PUT。
 * 否则每次编辑都要先吃一个必定 405 的请求（多一次写请求、多一行服务端日志），
 * 在这条路径上是纯粹的浪费（L-2）。缓存按**路径模板**而不是按记录 id ——
 * 服务端方法由路由决定，与具体记录无关。
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

/**
 * `describeError` 已搬到 `lib/i18n.ts`（它是**通用**的「错误码 → 文案」helper，
 * 审计页等新页面也要用）。
 *
 * 这里**继续导出**同一个函数：既有调用点（`SchemaPage.svelte`）与既有测试
 * 的 import 路径不变 —— 搬家不该逼着别人改 import。
 */
export { describeError, type Translate } from './i18n';

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
    // dev-facing（ASCII）：用户可见文案由 `describeError` 按错误码渲染。
    throw new Error('entity metadata is missing api.list / api.create');
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
 * 更新方法缓存：`api.update` 路径模板 → 已知可用的方法（L-2）。
 *
 * 首次 PATCH 成功就记 `PATCH`，遇到 405 记 `PUT`；之后同一实体的编辑直接发缓存的方法，
 * 不再固定先吃一次 405。模块级（前端只有一个页面进程），按**路径模板**区分实体。
 */
const updateMethods = new Map<string, 'PATCH' | 'PUT'>();

/**
 * 清空更新方法缓存（**仅供测试**）。
 *
 * 缓存是模块级的，会跨用例存活；断言「首个请求是 PATCH」的用例需要干净的起点。
 */
export function resetUpdateMethodCache(): void {
  updateMethods.clear();
}

/**
 * 更新：按已知方法写 `api.update`（`{id}` 替换为记录 id）。
 *
 * 未缓存时先发 `PATCH`，**只有** 405 才退回 `PUT` 并记住（见文件头注释）。
 * 未声明 `update` 时**抛错**（调用方本来就不该渲染「编辑」按钮）。
 */
export async function updateRecord(
  client: ApiClient,
  api: ApiEndpoints,
  id: string,
  body: Record<string, unknown>,
): Promise<void> {
  if (api.update === undefined) {
    throw new Error('entity metadata does not declare api.update');
  }
  const path = resolveApiPath(api.update, id);
  const remembered = updateMethods.get(api.update);
  if (remembered !== undefined) {
    await client.request({ method: remembered, path, body });
    return;
  }
  try {
    await client.request({ method: 'PATCH', path, body });
    updateMethods.set(api.update, 'PATCH');
  } catch (error) {
    if (error instanceof ApiError && error.status === 405) {
      // 405 = 路径存在但方法不允许 → 服务端没有处理，退回 PUT 不会重复写入。
      await client.request({ method: 'PUT', path, body });
      updateMethods.set(api.update, 'PUT');
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
    throw new Error('entity metadata does not declare api.delete');
  }
  await client.delete(resolveApiPath(api.delete, recordId(record)));
}
