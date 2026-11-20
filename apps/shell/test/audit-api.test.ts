import { describe, expect, it } from 'vitest';

import type { ApiClient, RequestOptions } from '@base/sdk';

import {
  AUDIT_DEFAULT_LIMIT,
  AUDIT_EVENTS_PATH,
  AUDIT_EVENTS_PERMISSION,
  AUDIT_MAX_LIMIT,
  auditCursorOf,
  auditCursorPage,
  auditFilters,
  auditObjectLabel,
  buildAuditQuery,
  formatAuditPayload,
  formatAuditTimestamp,
  loadAuditEvents,
  loadAuditFirstPage,
  newAuditCursorStack,
  parseAuditEvents,
  popAuditCursor,
  pushAuditCursor,
  toAuditRows,
} from '../src/lib/audit-api';

/** 一次被记录的请求。 */
interface RecordedCall {
  readonly method: string;
  readonly path: string;
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

/** 一条完整的审计记录（响应形状：camelCase，`before` / `after` 是 JSON 文本）。 */
const EVENT = {
  id: 'evt-1',
  actorId: 'u-1',
  action: 'rbac.role.grant',
  objectType: 'role',
  objectId: 'r-1',
  before: '{"permissions":[]}',
  after: '{"permissions":["p-1"]}',
  ip: '203.0.113.9',
  userAgent: 'audit-test-agent',
  createdAt: '2026-11-20T01:02:03.456789Z',
  prevHash: '0'.repeat(64),
  hash: 'a'.repeat(64),
};

describe('buildAuditQuery —— 契约 C7 的参数（AC-S-130 / D145 / D169）', () => {
  it('页码分页 → page + page_size + 默认排序', () => {
    expect(buildAuditQuery({ page: 2, pageSize: 20 })).toEqual({
      page: '2',
      page_size: '20',
      sort: '-createdAt',
    });
  });

  it('游标分页 → cursor + limit（与 page 互斥；**不带 sort**）', () => {
    // ⚠️ 不带 `sort`：后端拒绝「游标 + 任何排序」
    // （crates/platform-audit/src/query.rs:215）→ 带了就 400。
    expect(buildAuditQuery({ cursor: 'eyJ2Ijo0MH0', limit: 20 })).toEqual({
      cursor: 'eyJ2Ijo0MH0',
      limit: '20',
    });
  });

  it('游标 + 非默认排序 → 抛错（后端只认默认排序，前端不发非法请求）', () => {
    expect(() => buildAuditQuery({ cursor: 'c1', sort: 'actorId' })).toThrow(RangeError);
  });

  it('首页（既无游标也无页码）→ limit + 默认排序', () => {
    expect(buildAuditQuery({ limit: 10 })).toEqual({ limit: '10', sort: '-createdAt' });
  });

  it('cursor 与 page 同时给 → 抛错（后端会 400，前端不发非法请求）', () => {
    expect(() => buildAuditQuery({ cursor: 'c1', page: 2 })).toThrow(RangeError);
  });

  it('limit 超过上限 → 夹到 200', () => {
    expect(buildAuditQuery({ cursor: 'c1', limit: 1000 })['limit']).toBe(String(AUDIT_MAX_LIMIT));
  });

  it('limit 不是正整数 → 退回默认 20（不发明 0 这种非法值）', () => {
    expect(buildAuditQuery({ cursor: 'c1', limit: 0 })['limit']).toBe(String(AUDIT_DEFAULT_LIMIT));
  });

  it('排序字段不在白名单 → 抛错（未声明字段后端会 400）', () => {
    expect(() => buildAuditQuery({ page: 1, sort: 'unknownField' })).toThrow(RangeError);
  });

  it('过滤 → filter[field][op]（C7 的形状）', () => {
    const query = buildAuditQuery({
      page: 1,
      filters: [
        { field: 'actorId', op: 'eq', value: 'u-1' },
        { field: 'action', op: 'eq', value: 'rbac.role.grant' },
      ],
    });
    expect(query['filter[actorId][eq]']).toBe('u-1');
    expect(query['filter[action][eq]']).toBe('rbac.role.grant');
  });

  it('过滤值是空串 → 不入参（不下发无意义的过滤）', () => {
    const query = buildAuditQuery({
      page: 1,
      filters: [{ field: 'actorId', op: 'eq', value: '  ' }],
    });
    expect(Object.keys(query)).not.toContain('filter[actorId][eq]');
  });

  it('过滤字段 / 算子不在白名单 → 抛错', () => {
    expect(() =>
      buildAuditQuery({ page: 1, filters: [{ field: 'hash', op: 'eq', value: 'x' }] }),
    ).toThrow(RangeError);
    expect(() =>
      buildAuditQuery({
        page: 1,
        filters: [{ field: 'actorId', op: 'like' as 'eq', value: 'x' }],
      }),
    ).toThrow(RangeError);
  });
});

describe('auditFilters —— 界面输入 → 过滤条件', () => {
  it('操作人与动作各出一条（都是 eq）', () => {
    expect(auditFilters(' u-1 ', ' rbac.role.grant ')).toEqual([
      { field: 'actorId', op: 'eq', value: 'u-1' },
      { field: 'action', op: 'eq', value: 'rbac.role.grant' },
    ]);
  });

  it('空输入 → 没有过滤条件', () => {
    expect(auditFilters('', '   ')).toEqual([]);
  });
});

describe('loadAuditEvents —— 服务端**游标**分页的请求（AC-S-132 步骤 2 / D169）', () => {
  it('请求打在 /api/audit/events，页码**不进 query**（游标进 query，不是前端切片）', async () => {
    const { client, calls } = fakeClient(() => ({ items: [EVENT], total: 5, nextCursor: 'c2' }));
    const page = await loadAuditEvents(client, { limit: 2 });
    expect(calls).toHaveLength(1);
    expect(calls[0]?.path).toBe(AUDIT_EVENTS_PATH);
    expect(calls[0]?.query).toEqual({ limit: '2', sort: '-createdAt' });
    expect(page.items).toHaveLength(1);
    expect(page.total).toBe(5);
    expect(page.nextCursor).toBe('c2');
  });

  it('翻到第 2 页 → 请求带 cursor + limit（**不带 page**）', async () => {
    const { client, calls } = fakeClient(() => ({ items: [EVENT], total: 5, nextCursor: 'c3' }));
    await loadAuditEvents(client, { cursor: 'c2', limit: 2 });
    expect(calls[0]?.query).toEqual({ cursor: 'c2', limit: '2' });
  });

  it('权限点常量 = audit:events:read（契约 C4）', () => {
    expect(AUDIT_EVENTS_PERMISSION).toBe('audit:events:read');
  });
});

describe('游标栈 —— 页码指示的来源（D169）', () => {
  it('新栈 = 只有首页（哨兵 = 不带游标）；页码指示 = 1', () => {
    const stack = newAuditCursorStack();
    expect(stack).toEqual([null]);
    expect(auditCursorPage(stack)).toBe(1);
    expect(auditCursorOf(stack)).toBeNull();
  });

  it('「下一页」= 压栈：指示 +1，入参游标 = 栈顶', () => {
    const stack = pushAuditCursor(newAuditCursorStack(), 'c2');
    expect(stack).toEqual([null, 'c2']);
    expect(auditCursorPage(stack)).toBe(2);
    expect(auditCursorOf(stack)).toBe('c2');
  });

  it('「上一页」= 弹栈：指示 -1，入参游标回到首页（不带游标）', () => {
    const stack = popAuditCursor(pushAuditCursor(newAuditCursorStack(), 'c2'));
    expect(stack).toEqual([null]);
    expect(auditCursorPage(stack)).toBe(1);
    expect(auditCursorOf(stack)).toBeNull();
  });

  it('首页不能再往前（弹栈不越界）', () => {
    const stack = popAuditCursor(newAuditCursorStack());
    expect(stack).toEqual([null]);
    expect(auditCursorPage(stack)).toBe(1);
  });

  it('翻三页 → 指示 3，栈顶是第 3 页的入参游标', () => {
    let stack = newAuditCursorStack();
    stack = pushAuditCursor(stack, 'c2');
    stack = pushAuditCursor(stack, 'c3');
    expect(auditCursorPage(stack)).toBe(3);
    expect(auditCursorOf(stack)).toBe('c3');
  });
});

describe('parseAuditEvents —— 响应形状不对也不能白屏', () => {
  it('正常响应 → 行、总数、游标', () => {
    const page = parseAuditEvents({ items: [EVENT], total: 5, nextCursor: 'c2' });
    expect(page.total).toBe(5);
    expect(page.nextCursor).toBe('c2');
    expect(page.items[0]?.actorId).toBe('u-1');
  });

  it('缺 items → 空列表（不抛错）', () => {
    expect(parseAuditEvents({}).items).toEqual([]);
  });

  it('缺 total → 用当前页行数兜底', () => {
    expect(parseAuditEvents({ items: [EVENT] }).total).toBe(1);
  });

  it('nextCursor 为 null → null（不是空串）', () => {
    expect(parseAuditEvents({ items: [], total: 0, nextCursor: null }).nextCursor).toBeNull();
  });

  it('字段缺失 → 空串（键不得缺 —— AC-S-122 的判据）', () => {
    const event = parseAuditEvents({ items: [{ id: 'evt-9' }] }).items[0];
    expect(event?.ip).toBe('');
    expect(event?.userAgent).toBe('');
    expect(event?.before).toBeNull();
  });
});

describe('formatAuditPayload —— JSON 文本的展示与优雅降级', () => {
  it('合法 JSON → 规范化后的 JSON 文本', () => {
    expect(formatAuditPayload('{"permissions":["p-1"]}')).toBe('{"permissions":["p-1"]}');
  });

  it('带空白的 JSON → 规范化（键序按 JSON 序列化）', () => {
    expect(formatAuditPayload('{ "b" : 1 , "a" : 2 }')).toBe('{"b":1,"a":2}');
  });

  it('解析失败 → 显示原文（**不抛错**：一条脏数据不能让整页崩掉）', () => {
    expect(formatAuditPayload('{not json')).toBe('{not json');
  });

  it('没有值 → "-"', () => {
    expect(formatAuditPayload(null)).toBe('-');
    expect(formatAuditPayload('   ')).toBe('-');
  });
});

describe('formatAuditTimestamp —— 时间展示不因脏数据崩掉', () => {
  it('合法时间 → 用给定格式化函数渲染', () => {
    expect(formatAuditTimestamp('2026-11-20T01:02:03.456789Z', () => '2026-11-20 09:02:03')).toBe(
      '2026-11-20 09:02:03',
    );
  });

  it('格式化函数抛错（非法日期）→ 退回原文', () => {
    const boom = (): string => {
      throw new RangeError('invalid ISO datetime');
    };
    expect(formatAuditTimestamp('not-a-date', boom)).toBe('not-a-date');
  });

  it('空串 → 空串', () => {
    expect(formatAuditTimestamp('', () => 'x')).toBe('');
  });
});

describe('toAuditRows —— 展示行', () => {
  it('五项必显字段都非空（操作人 / 时间 / 动作 / 对象 / 来源 IP）', () => {
    const row = toAuditRows(parseAuditEvents({ items: [EVENT] }).items)[0];
    expect(row?.actorId).toBe('u-1');
    expect(row?.createdAt).toBe('2026-11-20T01:02:03.456789Z');
    expect(row?.action).toBe('rbac.role.grant');
    expect(row?.object).toBe('role / r-1');
    expect(row?.ip).toBe('203.0.113.9');
  });

  it('before / after 解析后展示（JSON 文本 → 文本）', () => {
    const row = toAuditRows(parseAuditEvents({ items: [EVENT] }).items)[0];
    expect(row?.before).toBe('{"permissions":[]}');
    expect(row?.after).toBe('{"permissions":["p-1"]}');
  });

  it('对象列：类型与标识都有就都显示', () => {
    expect(auditObjectLabel('role', 'r-1')).toBe('role / r-1');
    expect(auditObjectLabel('role', '')).toBe('role');
    expect(auditObjectLabel('', 'r-1')).toBe('r-1');
  });
});

describe('loadAuditFirstPage —— 路由 load 的产物（失败也不抛错）', () => {
  it('成功 → 行 + 总数 + 游标；首页**不带游标**（D169）', async () => {
    const { client, calls } = fakeClient(() => ({ items: [EVENT], total: 5, nextCursor: null }));
    const result = await loadAuditFirstPage(client);
    expect(result.items).toHaveLength(1);
    expect(result.total).toBe(5);
    expect(result.failure).toBeUndefined();
    expect(calls[0]?.query).toEqual({ limit: String(AUDIT_DEFAULT_LIMIT), sort: '-createdAt' });
    expect(Object.keys(calls[0]?.query ?? {})).not.toContain('cursor');
    expect(Object.keys(calls[0]?.query ?? {})).not.toContain('page');
  });

  it('失败 → 带 failure 的结果（路由的 load 是 void 调用的，抛错会变成未处理的 Promise 拒绝）', async () => {
    const { client } = fakeClient(() => {
      throw new Error('boom');
    });
    const result = await loadAuditFirstPage(client);
    expect(result.items).toEqual([]);
    expect(result.total).toBe(0);
    expect(result.failure).toBeInstanceOf(Error);
  });
});
