import { render } from 'svelte/server';
import { describe, expect, it } from 'vitest';

import type { AuthState } from '@base/auth';
import type { ApiClient, RequestOptions } from '@base/sdk';

import LeavePage from '../../../modules/leave/frontend/pages/LeavePage.svelte';
import {
  createLeaveRequest,
  decideLeaveRequest,
  LEAVE_PERMISSION_APPROVE,
  LEAVE_PERMISSION_READ,
  LEAVE_PERMISSION_WRITE,
  LEAVE_REQUESTS_PATH,
  parseLeaveRequest,
  submitLeaveRequest,
  validateNewLeave,
  type LeaveRequest,
} from '../../../modules/leave/frontend/lib/leave-api';
import { LEAVE_ROUTE_PERMISSION } from '../../../modules/leave/frontend/routes';
import { createRoutes } from '../src/lib/routes';

/** 申请人（只有 read + write）。 */
const APPLICANT: AuthState = {
  status: 'authenticated',
  authenticated: true,
  userId: 'u-applicant',
  tenantId: 'tenant-a',
  permissions: [LEAVE_PERMISSION_READ, LEAVE_PERMISSION_WRITE],
};

/** 审批人（只有 read + approve —— 不能建单）。 */
const APPROVER: AuthState = {
  status: 'authenticated',
  authenticated: true,
  userId: 'u-approver',
  tenantId: 'tenant-a',
  permissions: [LEAVE_PERMISSION_READ, LEAVE_PERMISSION_APPROVE],
};

/** 一条待审批的单子。 */
const PENDING_ROW: LeaveRequest = {
  id: 'leave-1',
  applicantId: 'u-applicant',
  startsOn: '2026-12-01',
  endsOn: '2026-12-03',
  reason: '年假',
  status: 'pending',
  decidedBy: null,
  decidedAt: null,
};

/** 记录请求的假客户端。 */
function fakeClient(): {
  client: ApiClient;
  calls: { path: string; method: string; body: unknown }[];
} {
  const calls: { path: string; method: string; body: unknown }[] = [];
  const request = async <T>(options: RequestOptions): Promise<T> => {
    calls.push({
      path: options.path,
      method: options.method ?? 'GET',
      body: options.body,
    });
    // 与真实响应同形的最小对象（闭集内的状态）。
    return {
      id: 'leave-1',
      applicantId: 'u-applicant',
      startsOn: '2026-12-01',
      endsOn: '2026-12-03',
      reason: '年假',
      status: 'pending',
      decidedBy: null,
      decidedAt: null,
    } as unknown as T;
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

describe('请假页的装配（AC-S-192 步骤 1）', () => {
  it('shell 路由表里有 /leave，权限点是 leave:request:read', () => {
    const { client } = fakeClient();
    const route = createRoutes(client).find((entry) => entry.path === '/leave');
    expect(route).toBeDefined();
    expect(route?.permission).toBe('leave:request:read');
    expect(LEAVE_ROUTE_PERMISSION).toBe(LEAVE_PERMISSION_READ);
  });

  it('列表给出申请人 / 请假时间 / 状态三列（表格可渲染）', () => {
    const { client } = fakeClient();
    const { body } = render(LeavePage, { props: { client, state: APPROVER } });
    expect(body).toContain('申请人');
    expect(body).toContain('请假时间');
    expect(body).toContain('状态');
    expect(body).toContain('data-testid="leave-table"');
  });

  it('申请人账号看到「新建」，审批人账号看不到（按钮按权限点出现）', () => {
    const { client } = fakeClient();
    const asApplicant = render(LeavePage, { props: { client, state: APPLICANT } }).body;
    const asApprover = render(LeavePage, { props: { client, state: APPROVER } }).body;
    expect(asApplicant).toContain('data-testid="leave-new"');
    expect(asApprover).not.toContain('data-testid="leave-new"');
  });
});

describe('审批按钮的条件（D356 红线）', () => {
  it('审批人 + pending 行 → 出现「通过」「驳回」', () => {
    const { client } = fakeClient();
    const { body } = render(LeavePage, {
      props: { client, state: APPROVER, initial: [PENDING_ROW] },
    });
    expect(body).toContain('data-testid="leave-approve"');
    expect(body).toContain('data-testid="leave-reject"');
    expect(body).toContain('待审批');
  });

  it('申请人账号即使是 pending 行也没有审批按钮', () => {
    const { client } = fakeClient();
    const { body } = render(LeavePage, {
      props: { client, state: APPLICANT, initial: [PENDING_ROW] },
    });
    expect(body).not.toContain('data-testid="leave-approve"');
    expect(body).not.toContain('data-testid="leave-reject"');
  });

  it('新建表单里**没有** status 字段（只能由端点推进状态）', () => {
    const { client } = fakeClient();
    const { body } = render(LeavePage, { props: { client, state: APPLICANT } });
    // 表单容器在（脚本里的表单是展开态才渲染 → 这里只断不存在 status 输入）。
    expect(body).not.toMatch(/name="status"/);
    expect(body).not.toContain('data-testid="leave-status-input"');
  });

  it('审批请求打到 /decide，请求体只有 decision（不是改 status）', async () => {
    const { client, calls } = fakeClient();
    await decideLeaveRequest(client, 'leave-1', 'approve');
    expect(calls).toHaveLength(1);
    expect(calls[0]?.path).toBe(`${LEAVE_REQUESTS_PATH}/leave-1/decide`);
    expect(calls[0]?.method).toBe('POST');
    expect(calls[0]?.body).toEqual({ decision: 'approve' });
  });

  it('提交请求打到 /submit（建单后先进 pending 再审批）', async () => {
    const { client, calls } = fakeClient();
    await submitLeaveRequest(client, 'leave-1');
    expect(calls[0]?.path).toBe(`${LEAVE_REQUESTS_PATH}/leave-1/submit`);
  });

  it('建单请求体是 camelCase（契约 C13），且不含 applicantId / status', async () => {
    const { client, calls } = fakeClient();
    await createLeaveRequest(client, {
      startsOn: '2026-12-01',
      endsOn: '2026-12-03',
      reason: '年假',
    });
    expect(calls[0]?.path).toBe(LEAVE_REQUESTS_PATH);
    expect(calls[0]?.body).toEqual({
      startsOn: '2026-12-01',
      endsOn: '2026-12-03',
      reason: '年假',
    });
    expect(Object.keys(calls[0]?.body as object).sort()).toEqual(['endsOn', 'reason', 'startsOn']);
  });
});

describe('响应解析与本地校验', () => {
  it('闭集外的状态**不被**静默映射成 draft', () => {
    expect(
      parseLeaveRequest({
        id: 'x',
        applicantId: 'u',
        startsOn: '2026-12-01',
        endsOn: '2026-12-02',
        reason: 'r',
        status: 'cancelled',
      }),
    ).toBeUndefined();
  });

  it('闭集内的四个状态都能解析', () => {
    for (const status of ['draft', 'pending', 'approved', 'rejected'] as const) {
      const parsed = parseLeaveRequest({
        id: 'x',
        applicantId: 'u',
        startsOn: '2026-12-01',
        endsOn: '2026-12-02',
        reason: 'r',
        status,
      });
      expect(parsed?.status).toBe(status);
    }
  });

  it('本地校验只挡空值与起止倒置', () => {
    expect(validateNewLeave({ startsOn: '', endsOn: '', reason: '' })).not.toBe('');
    expect(
      validateNewLeave({ startsOn: '2026-12-03', endsOn: '2026-12-01', reason: 'x' }),
    ).not.toBe('');
    expect(validateNewLeave({ startsOn: '2026-12-01', endsOn: '2026-12-03', reason: 'x' })).toBe(
      '',
    );
  });
});
