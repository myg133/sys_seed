/**
 * 审计查询接口封装（REQ-014 / AC-S-130 · AC-S-132）。
 *
 * # 参数口径 = 契约 C7（D145）
 *
 * | 分页方式 | 参数 | 说明 |
 * |---|---|---|
 * | 游标 | `cursor` + `limit` | C7 的**主口径**；`limit` 默认 20、上限 200 |
 * | 页码 | `page` + `page_size` | C7 的**兼容分页**（「需要页码跳转时」用）；与 `cursor` **互斥** |
 *
 * 两者同时给 → 后端 **400**（契约 C7）。所以这里在**发请求前**就拒绝：
 * 非法请求没有诊断价值，只会浪费一次往返。
 *
 * 其余参数：`sort`（逗号分隔，`-` 前缀降序，字段白名单）、`filter[field][op]`
 * （`op` 白名单 `eq` / `ne` / `in` / `contains` / `range`）。
 *
 * # 界面**默认走游标**（D169）
 *
 * 审计表**持续追加**：按页码翻页在变动的数据集上必然出现重复 / 遗漏 ——
 * 这正是契约 C7 把游标定为主口径的原话理由，也是 AC-S-132 步骤 2
 * 「翻页后记录**不重复**」的硬判据。
 * → **取数一律用游标**；页码指示靠**游标栈**（见 `AuditCursorStack`）。
 *
 * 页码模式**保留**（C7 允许的兼容路径），只是**默认不走**：
 * 删掉它只会多一个「看起来更干净」的改动面，不会带来任何能力。
 *
 * ⚠️ **两种模式都不做客户端切片**：行数据与 `total` **都来自服务端**。
 *
 * # 字段名 = camelCase（契约 C13 / D167）
 *
 * `before` / `after` 在库里是 **TEXT 列** → 接口给的是 **JSON 文本**，
 * 前端要解析后展示；解析失败**优雅降级**（显示原文），**不得**让整页崩掉。
 */

import type { ApiClient } from '@base/sdk';

/** 审计查询接口。 */
export const AUDIT_EVENTS_PATH = '/api/audit/events';

/** 审计查询的权限点（契约 C4：`module:resource:action`）。 */
export const AUDIT_EVENTS_PERMISSION = 'audit:events:read';

/** 排序字段白名单（契约 C7；D155 的默认排序是 `createdAt DESC, id DESC`）。 */
export const AUDIT_SORT_FIELDS = [
  'createdAt',
  'actorId',
  'action',
  'objectType',
  'objectId',
] as const;

/** 可过滤字段白名单（契约 C7 的 `filter[field][op]`；与排序字段同集）。 */
export const AUDIT_FILTER_FIELDS = AUDIT_SORT_FIELDS;

/** 过滤算子白名单（契约 C7）。 */
export const AUDIT_FILTER_OPS = ['eq', 'ne', 'in', 'contains', 'range'] as const;

/** 默认每页条数（契约 C7：`limit` 默认 20）。 */
export const AUDIT_DEFAULT_LIMIT = 20;

/** 每页条数上限（契约 C7：上限 200）。 */
export const AUDIT_MAX_LIMIT = 200;

/**
 * 界面可选的每页条数。
 *
 * 契约 C7 的默认是 20、上限 200；这里给的是**管理台常用的三档**
 * （数字不是文案，不需要 i18n）。
 */
export const AUDIT_PAGE_SIZE_OPTIONS = [10, 20, 50] as const;

/** 默认排序（契约 C7 / D155）。 */
export const AUDIT_DEFAULT_SORT = '-createdAt';

/** 空值的展示文案（`before` / `after` 没有内容时）。 */
export const AUDIT_EMPTY_VALUE = '-';

/** 排序字段。 */
export type AuditSortField = (typeof AUDIT_SORT_FIELDS)[number];

/** 过滤算子。 */
export type AuditFilterOp = (typeof AUDIT_FILTER_OPS)[number];

/** 一条审计事件（响应字段名 camelCase —— 契约 C13 / D167）。 */
export interface AuditEvent {
  /** 记录 id。 */
  readonly id: string;
  /** 操作人。 */
  readonly actorId: string;
  /** 事件码（如 `rbac.role.grant`）。 */
  readonly action: string;
  /** 对象类型。 */
  readonly objectType: string;
  /** 对象标识。 */
  readonly objectId: string;
  /** 变更前快照（**JSON 文本**；后端是 TEXT 列）。 */
  readonly before: string | null;
  /** 变更后快照（**JSON 文本**；后端是 TEXT 列）。 */
  readonly after: string | null;
  /** 来源 IP（可能为空串 —— 无值时不得缺键）。 */
  readonly ip: string;
  /** 客户端标识。 */
  readonly userAgent: string;
  /** 发生时间（UTC、RFC3339、微秒 6 位）。 */
  readonly createdAt: string;
  /** 链上前一条的哈希。 */
  readonly prevHash: string;
  /** 本条的哈希。 */
  readonly hash: string;
}

/** 一页审计事件。 */
export interface AuditEventsPage {
  /** 当前页的行（**服务端**返回的那一批）。 */
  readonly items: readonly AuditEvent[];
  /** 总条数（**服务端**给的；客户端分页会把它错当成当前页行数）。 */
  readonly total: number;
  /** 下一页游标（没有下一页时为 `null`）。 */
  readonly nextCursor: string | null;
}

/** 一个过滤条件（契约 C7 的 `filter[field][op]`）。 */
export interface AuditFilter {
  /** 字段名（白名单见 `AUDIT_FILTER_FIELDS`）。 */
  readonly field: string;
  /** 算子（白名单见 `AUDIT_FILTER_OPS`）。 */
  readonly op: AuditFilterOp;
  /** 值。 */
  readonly value: string;
}

/** 查询参数（游标分页与页码分页**互斥**）。 */
export interface AuditQuery {
  /** 页码（从 1 开始；与 `cursor` 互斥）。 */
  readonly page?: number;
  /** 每页条数（页码分页 → `page_size`）。 */
  readonly pageSize?: number;
  /** 游标（与 `page` 互斥）。 */
  readonly cursor?: string;
  /** 每页条数（游标分页 → `limit`）。 */
  readonly limit?: number;
  /** 排序（逗号分隔，`-` 前缀降序）。 */
  readonly sort?: string;
  /** 过滤条件。 */
  readonly filters?: readonly AuditFilter[];
}

/**
 * 游标栈（**页码指示的来源**，D169）。
 *
 * `stack[i]` = 第 `i + 1` 页的**入参游标**；下标 0 是首页的哨兵
 * （`null` = 请求里不带 `cursor`）。
 *
 * - 「下一页」= 把响应的 `nextCursor` **压栈**；
 * - 「上一页」= **弹栈**（用新的栈顶游标重新请求）；
 * - **页码指示 = 栈长度**（首页 = 1）。
 *
 * ⚠️ 游标分页**算不出**「全量第几页」：栈只记录**本次浏览**走过的路径。
 * 所以界面上的页码指示必须如实写成「本次浏览翻到第几页」，
 * **不能**写成「第 N 页 / 共 M 页」（那会被理解成全量页码，D169 明令禁止）。
 */
export type AuditCursorStack = readonly (string | null)[];

/** 新游标栈（只有首页）。 */
export function newAuditCursorStack(): AuditCursorStack {
  return [null];
}

/** 页码指示（= 栈长度；首页 = 1）。 */
export function auditCursorPage(stack: AuditCursorStack): number {
  return stack.length;
}

/** 当前页的**入参游标**（栈顶；首页 = `null` = 不带 `cursor`）。 */
export function auditCursorOf(stack: AuditCursorStack): string | null {
  return stack.length === 0 ? null : (stack[stack.length - 1] ?? null);
}

/** 「下一页」：压栈（截断到当前页之后 —— 走过又退回时，栈尾要重写）。 */
export function pushAuditCursor(stack: AuditCursorStack, cursor: string): AuditCursorStack {
  return [...stack, cursor];
}

/** 「上一页」：弹栈（首页不能再往前 —— 栈至少留一个哨兵）。 */
export function popAuditCursor(stack: AuditCursorStack): AuditCursorStack {
  return stack.length > 1 ? stack.slice(0, stack.length - 1) : stack;
}

/** 表格行（展示用的扁平结构）。 */
export interface AuditRow {
  /** 记录 id（行键）。 */
  readonly id: string;
  /** 操作人。 */
  readonly actorId: string;
  /** 发生时间（原始文本；格式化在渲染层做）。 */
  readonly createdAt: string;
  /** 动作。 */
  readonly action: string;
  /** 对象（类型 + 标识）。 */
  readonly object: string;
  /** 来源 IP。 */
  readonly ip: string;
  /** 变更前（已解析；解析失败时是原文）。 */
  readonly before: string;
  /** 变更后（已解析；解析失败时是原文）。 */
  readonly after: string;
}

/** 值是不是一个可用的对象（`null` / 数组不算）。 */
function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

/** 取文本（缺键 → 空串；**不得缺键**是 AC-S-122 的判据）。 */
function text(value: unknown): string {
  if (typeof value === 'string') {
    return value;
  }
  if (value === undefined || value === null) {
    return '';
  }
  return String(value);
}

/**
 * 取「可能是 JSON 文本」的字段。
 *
 * 后端给的是 JSON **文本**（TEXT 列）；万一给的是已解析的对象，也照样能用
 * （转回文本再走同一条解析路径）。
 */
function jsonText(value: unknown): string | null {
  if (typeof value === 'string') {
    return value;
  }
  if (value === undefined || value === null) {
    return null;
  }
  if (isRecord(value) || Array.isArray(value)) {
    return JSON.stringify(value);
  }
  return String(value);
}

/** 把一条原始记录规整成 `AuditEvent`（字段缺失 → 空串，不抛错）。 */
export function toAuditEvent(raw: unknown): AuditEvent {
  const record = isRecord(raw) ? raw : {};
  return {
    id: text(record['id']),
    actorId: text(record['actorId']),
    action: text(record['action']),
    objectType: text(record['objectType']),
    objectId: text(record['objectId']),
    before: jsonText(record['before']),
    after: jsonText(record['after']),
    ip: text(record['ip']),
    userAgent: text(record['userAgent']),
    createdAt: text(record['createdAt']),
    prevHash: text(record['prevHash']),
    hash: text(record['hash']),
  };
}

/**
 * 规整响应（形状不对 → 空列表，**不抛错**）。
 *
 * 列表页不该因为形状不对而白屏；解析不出来就当「没有数据」。
 */
export function parseAuditEvents(raw: unknown): AuditEventsPage {
  const record = isRecord(raw) ? raw : {};
  const list = Array.isArray(record['items']) ? record['items'] : [];
  const items = list.map((entry) => toAuditEvent(entry));
  const rawTotal = record['total'];
  const total =
    typeof rawTotal === 'number' && Number.isFinite(rawTotal)
      ? rawTotal
      : typeof rawTotal === 'string' && rawTotal.trim() !== '' && Number.isFinite(Number(rawTotal))
        ? Number(rawTotal)
        : items.length;
  const cursor = record['nextCursor'];
  const nextCursor = typeof cursor === 'string' && cursor !== '' ? cursor : null;
  return { items, total, nextCursor };
}

/** 把每页条数夹到 `[1, AUDIT_MAX_LIMIT]`（后端也会夹；前端不发明非法值）。 */
function clampSize(size: number | undefined, fallback: number): number {
  if (size === undefined || !Number.isFinite(size)) {
    return fallback;
  }
  const floored = Math.floor(size);
  if (floored < 1) {
    return fallback;
  }
  return Math.min(floored, AUDIT_MAX_LIMIT);
}

/** 校验排序字段（白名单外的字段后端会 400 —— 前端不发）。 */
function assertSortField(field: string): void {
  if (!(AUDIT_SORT_FIELDS as readonly string[]).includes(field)) {
    throw new RangeError(`unsupported audit sort field: ${field}`);
  }
}

/**
 * 构造查询串（契约 C7 的参数名，**逐字**）。
 *
 * - 游标分页：`cursor` + `limit`（**不发 `sort`** —— 见下）；
 * - 页码分页：`page` + `page_size`；
 * - 两者**互斥**（同时给 → 抛错，不发非法请求）；
 * - `sort` 默认 `-createdAt`（D155）；
 * - 过滤 → `filter[field][op]`（空值不入参）。
 *
 * # 为什么游标模式**不带** `sort`
 *
 * 后端**拒绝**「游标 + 任何排序」：
 * `crates/platform-audit/src/query.rs:215` → 「the cursor is only valid with
 * the default sort order」→ 带 `sort` 的游标请求会被 **400** 掉。
 * 前端发的是**默认排序**（`-createdAt` = 后端的默认排序，D155）→
 * **不发** `sort` 与发默认排序等价，所以干脆不发（少一个可能被后端改动的旋钮）。
 */
export function buildAuditQuery(query: AuditQuery): Record<string, string> {
  const params: Record<string, string> = {};
  const cursor = query.cursor === undefined ? '' : query.cursor;
  const hasCursor = cursor !== '';
  const hasPage = query.page !== undefined;

  if (hasCursor && hasPage) {
    // 契约 C7：`cursor` 与 `page` 互斥（同时给 → 后端 400）。
    throw new RangeError('audit query accepts either cursor or page, not both');
  }

  const sort = query.sort ?? AUDIT_DEFAULT_SORT;
  for (const part of sort.split(',')) {
    const trimmed = part.trim();
    if (trimmed === '') {
      continue;
    }
    assertSortField(trimmed.startsWith('-') ? trimmed.slice(1) : trimmed);
  }

  if (hasCursor) {
    // 游标只对**默认排序**有效（后端口径，见上面的说明）→ 非默认排序直接拒。
    if (sort !== AUDIT_DEFAULT_SORT) {
      throw new RangeError('the audit cursor is only valid with the default sort order');
    }
    params['cursor'] = cursor;
    params['limit'] = String(clampSize(query.limit, AUDIT_DEFAULT_LIMIT));
  } else {
    if (hasPage) {
      params['page'] = String(Math.max(1, Math.floor(query.page ?? 1)));
      params['page_size'] = String(clampSize(query.pageSize, AUDIT_DEFAULT_LIMIT));
    } else if (query.limit !== undefined) {
      params['limit'] = String(clampSize(query.limit, AUDIT_DEFAULT_LIMIT));
    }
    params['sort'] = sort;
  }

  for (const filter of query.filters ?? []) {
    const value = filter.value.trim();
    if (value === '') {
      continue;
    }
    if (!(AUDIT_FILTER_FIELDS as readonly string[]).includes(filter.field)) {
      throw new RangeError(`unsupported audit filter field: ${filter.field}`);
    }
    if (!(AUDIT_FILTER_OPS as readonly string[]).includes(filter.op)) {
      throw new RangeError(`unsupported audit filter op: ${filter.op}`);
    }
    params[`filter[${filter.field}][${filter.op}]`] = value;
  }

  return params;
}

/**
 * 把界面上的过滤输入变成过滤条件（空值 → 不入参）。
 *
 * 过滤**必须下发到后端**（不是在前端过滤已加载的那一页）。
 */
export function auditFilters(actorId: string, action: string): AuditFilter[] {
  const filters: AuditFilter[] = [];
  const actor = actorId.trim();
  if (actor !== '') {
    filters.push({ field: 'actorId', op: 'eq', value: actor });
  }
  const act = action.trim();
  if (act !== '') {
    filters.push({ field: 'action', op: 'eq', value: act });
  }
  return filters;
}

/**
 * 展示 `before` / `after`。
 *
 * 解析成功 → 规范化后的 JSON 文本；解析失败 → **原文**（优雅降级）；
 * 没有值 → `-`。**任何情况下都不抛错**（一条脏数据不能让整页崩掉）。
 */
export function formatAuditPayload(value: string | null): string {
  if (value === null) {
    return AUDIT_EMPTY_VALUE;
  }
  const trimmed = value.trim();
  if (trimmed === '') {
    return AUDIT_EMPTY_VALUE;
  }
  try {
    const parsed: unknown = JSON.parse(trimmed);
    const normalized = JSON.stringify(parsed);
    return normalized === undefined ? value : normalized;
  } catch {
    return value;
  }
}

/**
 * 展示时间。
 *
 * 按用户 / 租户时区渲染（契约 C13）；原始值不合法时**退回原文**
 * （`Intl` 遇到非法日期会抛错 —— 一条脏数据不该让整页崩掉）。
 */
export function formatAuditTimestamp(iso: string, format: (value: string) => string): string {
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

/** 「对象」列的展示：类型与标识都有就都显示（`role / r-1`）。 */
export function auditObjectLabel(objectType: string, objectId: string): string {
  if (objectType !== '' && objectId !== '') {
    return `${objectType} / ${objectId}`;
  }
  return objectType !== '' ? objectType : objectId;
}

/** 把事件列表转成展示行。 */
export function toAuditRows(events: readonly AuditEvent[]): AuditRow[] {
  return events.map((event) => ({
    id: event.id,
    actorId: event.actorId,
    createdAt: event.createdAt,
    action: event.action,
    object: auditObjectLabel(event.objectType, event.objectId),
    ip: event.ip,
    before: formatAuditPayload(event.before),
    after: formatAuditPayload(event.after),
  }));
}

/**
 * 取一页审计事件（**服务端分页**：请求里带 `cursor` + `limit`，
 * 返回的行就是服务端给的那一批）。
 *
 * 不传 `cursor` = 首页（游标栈的哨兵页）。
 */
export async function loadAuditEvents(
  client: ApiClient,
  query: AuditQuery = {},
): Promise<AuditEventsPage> {
  const raw = await client.request<unknown>({
    path: AUDIT_EVENTS_PATH,
    query: buildAuditQuery(query),
  });
  return parseAuditEvents(raw);
}

/** 路由 `load` 的产物（首屏数据 + 失败原因）。 */
export interface AuditLoadResult {
  /** 首屏的行。 */
  readonly items: readonly AuditEvent[];
  /** 总条数。 */
  readonly total: number;
  /** 下一页游标。 */
  readonly nextCursor: string | null;
  /** 失败原因（`undefined` = 没失败）。 */
  readonly failure?: unknown;
}

/**
 * 取首屏（路由 `load` 用）。
 *
 * 首屏 = 游标栈的第 1 页 = **不带游标** + `limit`（D169）。
 *
 * **不抛错**：失败也返回结果（把原因放在 `failure` 里）——
 * 路由的 `load` 是被 `void` 调用的（`router.ts:248`），
 * 抛错会变成**未处理的 Promise 拒绝**（页面上看不到任何提示）。
 */
export async function loadAuditFirstPage(
  client: ApiClient,
  pageSize: number = AUDIT_DEFAULT_LIMIT,
): Promise<AuditLoadResult> {
  try {
    const page = await loadAuditEvents(client, { limit: pageSize });
    return { items: page.items, total: page.total, nextCursor: page.nextCursor };
  } catch (failure) {
    return { items: [], total: 0, nextCursor: null, failure };
  }
}
