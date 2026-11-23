/**
 * 请假模块的前端接口封装（REQ-020 / AC-S-192 · D356）。
 *
 * # 业务前端代码留在模块目录（AC-029「业务代码集中在独立目录」）
 *
 * 页面与接口封装都住在 `modules/leave/frontend/`，**不**塞进 `apps/shell/src/**`：
 * 放进 shell 会让业务代码逃出 AC-S-187 的前端代码量口径（`modules/leave/frontend`），
 * 也会让「业务只写业务目录」变成一句空话。
 *
 * # 审批**只走** `POST /api/leave/requests/{id}/decide`（D356 红线）
 *
 * 这里**没有**任何「改 `status` 字段」的函数，也不提供 `PATCH` / `PUT` 入口：
 * 直接改状态会绕过业务状态机与 **409 `LEAVE_INVALID_TRANSITION`** 校验
 * （AC-S-189 的安全判据被 UI 旁路）。审批只有 [`decideLeaveRequest`] 一个出口。
 *
 * # 字段名 = camelCase（契约 C13）
 *
 * 后端 `#[serde(rename_all = "camelCase")]`：出参是 `applicantId` / `startsOn` /
 * `endsOn` / `decidedBy` / `decidedAt`；建单请求体也是 camelCase。
 */

import type { ApiClient } from '@base/sdk';

/** 请假单接口。 */
export const LEAVE_REQUESTS_PATH = '/api/leave/requests';

/** 列表权限点（与后端 `require_permission` 的字面量逐字一致）。 */
export const LEAVE_PERMISSION_READ = 'leave:request:read';

/** 建单 / 提交权限点。 */
export const LEAVE_PERMISSION_WRITE = 'leave:request:write';

/** 审批权限点。 */
export const LEAVE_PERMISSION_APPROVE = 'leave:request:approve';

/** 列表默认条数（与后端 `DEFAULT_PAGE_SIZE` 一致）。 */
export const LEAVE_DEFAULT_LIMIT = 20;

/** 状态闭集（**恰好 4 个**，与后端 `Status` 的取值逐字一致）。 */
export const LEAVE_STATUSES = ['draft', 'pending', 'approved', 'rejected'] as const;

/** 状态。 */
export type LeaveStatus = (typeof LEAVE_STATUSES)[number];

/** 审批决定（与后端 `Decision` 的 serde 取值逐字一致）。 */
export type LeaveDecision = 'approve' | 'reject';

/** 状态的中文文案（模块页按 scaffold 范式硬编码 —— 见 D356 的限制说明）。 */
export const LEAVE_STATUS_LABELS: Record<LeaveStatus, string> = {
  draft: '草稿',
  pending: '待审批',
  approved: '已通过',
  rejected: '已驳回',
};

/** 一条请假单（出参，camelCase）。 */
export interface LeaveRequest {
  /** 主键。 */
  readonly id: string;
  /** 申请人。 */
  readonly applicantId: string;
  /** 起始日期（`YYYY-MM-DD`）。 */
  readonly startsOn: string;
  /** 结束日期（`YYYY-MM-DD`）。 */
  readonly endsOn: string;
  /** 理由。 */
  readonly reason: string;
  /** 状态（闭集 4 个；闭集外的值**不**静默映射成 draft）。 */
  readonly status: LeaveStatus;
  /** 审批人。 */
  readonly decidedBy: string | null;
  /** 审批时间。 */
  readonly decidedAt: string | null;
}

/** 建单入参（`applicantId` / `status` **不接受**客户端指定）。 */
export interface NewLeaveInput {
  /** 起始日期（`YYYY-MM-DD`）。 */
  readonly startsOn: string;
  /** 结束日期（`YYYY-MM-DD`）。 */
  readonly endsOn: string;
  /** 理由。 */
  readonly reason: string;
}

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

/** 取可空文本。 */
function nullableText(value: unknown): string | null {
  return typeof value === 'string' && value !== '' ? value : null;
}

/** 状态是否在闭集里。 */
export function isLeaveStatus(value: unknown): value is LeaveStatus {
  return typeof value === 'string' && (LEAVE_STATUSES as readonly string[]).includes(value);
}

/**
 * 把一条原始记录规整成 `LeaveRequest`。
 *
 * **闭集外的状态返回 `undefined`**：静默映射成 `draft` 会让界面把
 * 「数据损坏」显示成「草稿」，用户点提交又会走一次非法流转。
 */
export function parseLeaveRequest(raw: unknown): LeaveRequest | undefined {
  const record = isRecord(raw) ? raw : {};
  const status = record['status'];
  if (!isLeaveStatus(status)) {
    return undefined;
  }
  return {
    id: text(record['id']),
    applicantId: text(record['applicantId']),
    startsOn: text(record['startsOn']),
    endsOn: text(record['endsOn']),
    reason: text(record['reason']),
    status,
    decidedBy: nullableText(record['decidedBy']),
    decidedAt: nullableText(record['decidedAt']),
  };
}

/**
 * 解析列表响应。
 *
 * 后端返回**裸数组**（`Vec<LeaveResponse>`）；这里同时容忍 `{ items: [...] }`
 * 形状（与审计页的解析器保持同一套「形状不对不抛错」的约定）。
 */
export function parseLeaveRequests(raw: unknown): LeaveRequest[] {
  const list = Array.isArray(raw)
    ? raw
    : isRecord(raw) && Array.isArray(raw['items'])
      ? raw['items']
      : [];
  return list
    .map((entry) => parseLeaveRequest(entry))
    .filter((entry): entry is LeaveRequest => entry !== undefined);
}

/**
 * 建单前的**本地**校验（返回空串 = 通过）。
 *
 * 只做「显然为空 / 起止倒置」这类可离线判定的检查 —— 日期合法性、
 * 租户、申请人一律由后端裁决，前端**不**复刻后端规则。
 */
export function validateNewLeave(input: NewLeaveInput): string {
  if (input.startsOn.trim() === '' || input.endsOn.trim() === '') {
    return '请填写起止日期';
  }
  if (input.reason.trim() === '') {
    return '请填写请假理由';
  }
  if (input.startsOn > input.endsOn) {
    return '起始日期不能晚于结束日期';
  }
  return '';
}

/** 取列表（服务端只支持 `limit`，没有游标 —— 见模块进度里的说明）。 */
export async function listLeaveRequests(
  client: ApiClient,
  limit: number = LEAVE_DEFAULT_LIMIT,
): Promise<LeaveRequest[]> {
  const raw = await client.get<unknown>(LEAVE_REQUESTS_PATH, { query: { limit: String(limit) } });
  return parseLeaveRequests(raw);
}

/** 建单（初始状态 `draft`）。 */
export async function createLeaveRequest(
  client: ApiClient,
  input: NewLeaveInput,
): Promise<LeaveRequest | undefined> {
  const raw = await client.post<unknown>(LEAVE_REQUESTS_PATH, {
    startsOn: input.startsOn,
    endsOn: input.endsOn,
    reason: input.reason,
  });
  return parseLeaveRequest(raw);
}

/** 提交（`draft → pending`）。 */
export async function submitLeaveRequest(
  client: ApiClient,
  id: string,
): Promise<LeaveRequest | undefined> {
  const raw = await client.post<unknown>(`${LEAVE_REQUESTS_PATH}/${encodeURIComponent(id)}/submit`);
  return parseLeaveRequest(raw);
}

/**
 * 审批（`pending → approved / rejected`）。
 *
 * ⚠️ **唯一**的审批出口：不允许把 `status` 直接写进任何请求体（D356 红线）。
 * 非法流转由后端返回 **409 `LEAVE_INVALID_TRANSITION`**，界面如实显示。
 */
export async function decideLeaveRequest(
  client: ApiClient,
  id: string,
  decision: LeaveDecision,
): Promise<LeaveRequest | undefined> {
  const raw = await client.post<unknown>(
    `${LEAVE_REQUESTS_PATH}/${encodeURIComponent(id)}/decide`,
    { decision },
  );
  return parseLeaveRequest(raw);
}
