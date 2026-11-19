import { expect, test, type Page } from '@playwright/test';

/**
 * REQ-012 的应用级 E2E：**元数据驱动的渲染闭环**（AC-S-082 ~ 087）。
 *
 * # 为什么必须真浏览器
 *
 * 本仓前端单测没有 jsdom（零新依赖），组件单测走 `svelte/server` 的 SSR 渲染 ——
 * 它能守住「表头 / 单元格 / 空态」这类静态输出，但守不住：
 * 点击「下一页」发出的**请求**、表单校验失败时**零请求**、字段级权限的 DOM 取证。
 * REQ-011 的教训是「单测全绿 ≠ 能用」，这些只能真浏览器验。
 *
 * # 前置（依赖后端；未就绪时用例 **skip 并写明原因**，不写成通过）
 *
 * - 后端已挂载 `sandbox` 模块并声明 `demoItem` 的 schema 段（D103）；
 * - `GET /api/schema/sandbox/demoItem` 返回 200 且响应含 `api` 子段（D102）；
 * - `scripts/seed-e2e.sh` 的夹具用户可登录（REQ-011 的既有夹具）。
 *
 * 第二个实体（零代码用例）默认 `demoNote`，可用 `SCHEMA_SECOND_ENTITY` 覆盖；
 * 它不存在时该用例 skip（不会假装通过）。
 */

const TENANT = 'tenant-a';
const PASSWORD = 'Tr0ub4dor&3xKq';
/** 会话存储键（与 `src/lib/session.ts` 的 `SESSION_STORAGE_KEY` 一致）。 */
const SESSION_KEY = 'base.shell.session.v1';

const MODULE = 'sandbox';
const PRIMARY_ENTITY = 'demoItem';
const SECOND_ENTITY = process.env.SCHEMA_SECOND_ENTITY ?? 'demoNote';

/** 有写权限的夹具用户。 */
const USER_WRITE = process.env.SCHEMA_USER_WRITE ?? 'u-perm-write@example.com';
/** 无写权限的夹具用户。 */
const USER_READ = process.env.SCHEMA_USER_READ ?? 'u-perm-read@example.com';

/** 登录（REQ-011 的夹具账号）。 */
async function login(page: Page, email: string): Promise<void> {
  await page.goto('/login');
  await page.getByLabel('租户').fill(TENANT);
  await page.getByLabel('用户名').fill(email);
  await page.getByLabel('密码').fill(PASSWORD);
  await page.getByRole('button', { name: '登录' }).click();
  await expect(page.getByText('已就绪')).toBeVisible();
}

/** 用当前会话（sessionStorage 里的令牌）发一次 API 请求。 */
async function apiRequest(
  page: Page,
  path: string,
  init?: { method?: string; body?: unknown },
): Promise<{ status: number; body: unknown }> {
  return page.evaluate(
    async ({ path: requestPath, init: requestInit, sessionKey }) => {
      const stored = window.sessionStorage.getItem(sessionKey);
      const session = stored === null ? null : (JSON.parse(stored) as Record<string, unknown>);
      const headers: Record<string, string> = {
        'x-tenant-id': String(session?.['tenantId'] ?? ''),
        'content-type': 'application/json',
      };
      if (typeof session?.['accessToken'] === 'string') {
        headers['authorization'] = `Bearer ${session.accessToken}`;
      }
      const response = await fetch(requestPath, {
        method: requestInit?.method ?? 'GET',
        headers,
        body: requestInit?.body === undefined ? undefined : JSON.stringify(requestInit.body),
      });
      const body: unknown = await response.json().catch(() => null);
      return { status: response.status, body };
    },
    { path, init, sessionKey: SESSION_KEY },
  );
}

/** 元数据（含 `api` 段）。 */
interface EntityMetaBody {
  module?: string;
  entity?: string;
  label?: string;
  api?: { list?: string; create?: string; update?: string; delete?: string };
  fields?: { key?: string; label?: string; type?: string }[];
}

/** 取元数据；后端未就绪时返回 `undefined`（调用方据此 skip）。 */
async function fetchMeta(page: Page, entity: string): Promise<EntityMetaBody | undefined> {
  const response = await apiRequest(page, `/api/schema/${MODULE}/${entity}`);
  if (response.status !== 200 || response.body === null || typeof response.body !== 'object') {
    return undefined;
  }
  return response.body as EntityMetaBody;
}

/** 表头文本（按 DOM 顺序）。 */
async function headerTexts(page: Page): Promise<string[]> {
  return page.locator('table thead th').allTextContents();
}

/**
 * 列表接口是否可读（200）。
 *
 * 后端把 `/api/sandbox/demo-items` 绑在 `sandbox:demo:read` 上；
 * E2E 夹具用户若没有该权限点，渲染用例没有判据 —— **skip 并写明原因**，
 * 不把「没权限」伪装成「渲染通过」。修夹具（种子授权 / `SCHEMA_USER_*` 换人）后即可跑。
 */
async function listStatus(page: Page, meta: EntityMetaBody | undefined): Promise<number> {
  const listPath = meta?.api?.list;
  if (listPath === undefined) {
    return 0;
  }
  const response = await apiRequest(page, `${listPath}?limit=1`);
  return response.status;
}

/** 打开数据页面并等到「表头 / 空态 / 错误」之一出现。 */
async function openSchema(page: Page, entity: string): Promise<void> {
  await page.goto(`/schema/${MODULE}/${entity}`);
  await expect(
    page.locator('table thead, .base-schema-empty, .base-schema-error').first(),
  ).toBeVisible();
}

test('表头文本与顺序精确等于元数据 fields 的 label（AC-S-082）', async ({ page }) => {
  await login(page, USER_WRITE);
  const meta = await fetchMeta(page, PRIMARY_ENTITY);
  test.skip(meta === undefined, `后端未就绪：/api/schema/${MODULE}/${PRIMARY_ENTITY} 非 200`);
  const labels = (meta?.fields ?? []).map((field) => field.label ?? '');
  expect(labels.length).toBeGreaterThan(0);

  const status = await listStatus(page, meta);
  test.skip(status !== 200, `夹具不可读列表（HTTP ${status}）—— 需要 sandbox:demo:read`);

  await openSchema(page, PRIMARY_ENTITY);
  test.skip(
    (await page.locator('.base-schema-empty').count()) > 0,
    '夹具没有数据（AC-S-082 前置要求 ≥ 3 条）',
  );

  // 逐项相等（不是「包含」—— 否则乱序也会通过）。
  expect(await headerTexts(page)).toEqual(labels);
});

test('空态：不渲染表头空壳，只渲染「暂无数据」（AC-S-082 步骤 5）', async ({ page }) => {
  await login(page, USER_WRITE);
  const meta = await fetchMeta(page, SECOND_ENTITY);
  test.skip(meta === undefined, `后端未声明第二个实体 ${SECOND_ENTITY}，空态用例待就绪后复跑`);
  const status = await listStatus(page, meta);
  test.skip(status !== 200, `夹具不可读 ${SECOND_ENTITY} 列表（HTTP ${status}）`);

  await page.goto(`/schema/${MODULE}/${SECOND_ENTITY}`);

  // 该实体没有数据时：没有表格、没有 thead，只有一个非空文案。
  const empty = page.locator('.base-schema-empty');
  await expect(empty).toBeVisible();
  expect((await empty.textContent())?.trim()).toBe('暂无数据');
  expect(await page.locator('table thead').count()).toBe(0);
  const text = (await page.locator('body').innerText()).toLowerCase();
  expect(text).not.toContain('undefined');
  expect(text).not.toContain('null');
  expect(text).not.toContain('nan');
});

test('字段级权限：无权用户看不到字段，接口也不返回该键（AC-S-084）', async ({ page }) => {
  await login(page, USER_WRITE);
  const full = await fetchMeta(page, PRIMARY_ENTITY);
  test.skip(full === undefined, '后端未就绪：元数据接口不可用');
  const fullKeys = (full?.fields ?? []).map((field) => field.key ?? '');

  await login(page, USER_READ);
  const limited = await fetchMeta(page, PRIMARY_ENTITY);
  test.skip(limited === undefined, '无写权限用户也读不到元数据（夹具权限配置待就绪）');
  const limitedKeys = (limited?.fields ?? []).map((field) => field.key ?? '');
  test.skip(
    limitedKeys.length === fullKeys.length,
    '夹具用户之间没有字段级权限差异（后端种子未授予 sandbox:demo:secret）',
  );

  const limitedStatus = await listStatus(page, limited);
  test.skip(
    limitedStatus !== 200,
    `无权用户也读不到列表（HTTP ${limitedStatus}）—— 夹具权限待就绪`,
  );

  await openSchema(page, PRIMARY_ENTITY);
  test.skip(
    (await page.locator('.base-schema-empty').count()) > 0,
    '夹具没有数据（字段可见性无法在空表上取证）',
  );
  expect(await headerTexts(page)).toEqual((limited?.fields ?? []).map((f) => f.label ?? ''));

  // 被过滤字段的 label 不能出现在 DOM 里（不是 display:none）。
  const bodyText = await page.locator('body').innerText();
  for (const key of fullKeys.filter((candidate) => !limitedKeys.includes(candidate))) {
    const label = (full?.fields ?? []).find((field) => field.key === key)?.label ?? key;
    expect(bodyText).not.toContain(label);
  }
});

test('零代码：同一个页面渲染两个不同实体（AC-S-085 / AC-S-086）', async ({ page }) => {
  await login(page, USER_WRITE);
  const primary = await fetchMeta(page, PRIMARY_ENTITY);
  const second = await fetchMeta(page, SECOND_ENTITY);
  test.skip(primary === undefined, '后端未就绪：demoItem 元数据不可用');
  test.skip(second === undefined, `后端未声明第二个实体 ${SECOND_ENTITY}，零代码用例待就绪后复跑`);

  const primaryStatus = await listStatus(page, primary);
  const secondStatus = await listStatus(page, second);
  test.skip(
    primaryStatus !== 200 || secondStatus !== 200,
    `实体的列表不可读（demoItem=${primaryStatus}，${SECOND_ENTITY}=${secondStatus}）`,
  );

  await openSchema(page, PRIMARY_ENTITY);
  expect(await headerTexts(page)).toEqual((primary?.fields ?? []).map((f) => f.label ?? ''));

  await openSchema(page, SECOND_ENTITY);
  expect(await headerTexts(page)).toEqual((second?.fields ?? []).map((f) => f.label ?? ''));
});

test('CRUD 闭环：新建 → 编辑 → 删除（AC-S-086）', async ({ page }) => {
  await login(page, USER_WRITE);
  const meta = await fetchMeta(page, PRIMARY_ENTITY);
  test.skip(meta === undefined, '后端未就绪：demoItem 元数据不可用');
  const api = meta?.api ?? {};
  test.skip(
    api.update === undefined || api.delete === undefined,
    '元数据未声明 update / delete，CRUD 用例待后端就绪后复跑',
  );
  const status = await listStatus(page, meta);
  test.skip(
    status !== 200,
    `夹具不可读 / 写 demoItem（HTTP ${status}）—— 需要 sandbox:demo:read + write`,
  );

  await openSchema(page, PRIMARY_ENTITY);

  const marker = `e2e-${Date.now()}`;

  // 新建：用真实 UI（不用 API 直连 —— 这条用例守的就是渲染器 + 表单）。
  await page.getByRole('button', { name: '新建' }).click();
  await page.getByLabel('标题').fill(marker);
  await page.getByRole('button', { name: '创建' }).click();

  // 新记录出现在列表里（按唯一标识断言，不是「行数 +1」）。
  const row = page.locator('tbody tr', { hasText: marker });
  await expect(row).toHaveCount(1);

  // 编辑：改成新标题，列表反映新值。
  await row.getByRole('button', { name: '编辑' }).click();
  await page.getByLabel('标题').fill(`${marker}-edited`);
  await page.getByRole('button', { name: '保存' }).click();
  await expect(page.locator('tbody tr', { hasText: `${marker}-edited` })).toHaveCount(1);

  // 删除：先确认对话框，再断言记录不在列表里。
  await page
    .locator('tbody tr', { hasText: `${marker}-edited` })
    .getByRole('button', { name: '删除' })
    .click();
  await page.getByRole('button', { name: '确认' }).click();
  await expect(page.locator('tbody tr', { hasText: `${marker}-edited` })).toHaveCount(0);
});

test('游标分页：下一页请求带上一页的 nextCursor（AC-S-082 步骤 4）', async ({ page }) => {
  await login(page, USER_WRITE);
  const meta = await fetchMeta(page, PRIMARY_ENTITY);
  test.skip(meta === undefined, '后端未就绪：demoItem 元数据不可用');
  const listPath = meta?.api?.list;
  const createPath = meta?.api?.create;
  const deletePath = meta?.api?.delete;
  test.skip(
    listPath === undefined || createPath === undefined || deletePath === undefined,
    '元数据缺 api.list / create / delete，分页用例待后端就绪后复跑',
  );
  const status = await listStatus(page, meta);
  test.skip(status !== 200, `夹具不可读列表（HTTP ${status}）—— 需要 sandbox:demo:read`);

  // 先看首页是否已有下一页；没有就通过 API 播种到超过一页（20 条）。
  const created: string[] = [];
  const first = await apiRequest(page, `${String(listPath)}?limit=20`);
  const firstBody = (first.body ?? {}) as {
    items?: Record<string, unknown>[];
    nextCursor?: string | null;
  };
  if (typeof firstBody.nextCursor !== 'string') {
    for (let index = 0; index < 21; index += 1) {
      const createdResponse = await apiRequest(page, String(createPath), {
        method: 'POST',
        body: { title: `page-seed-${index}` },
      });
      const createdBody = (createdResponse.body ?? {}) as { id?: unknown };
      if (typeof createdBody.id === 'string') {
        created.push(createdBody.id);
      }
    }
  }

  try {
    await openSchema(page, PRIMARY_ENTITY);
    const firstPageIds = await page.locator('tbody tr td:first-child').allTextContents();

    const nextRequest = page.waitForRequest(
      (request) =>
        new URL(request.url()).pathname === listPath &&
        new URL(request.url()).searchParams.has('cursor'),
    );
    await page.getByRole('button', { name: '下一页' }).click();
    const request = await nextRequest;
    const cursor = new URL(request.url()).searchParams.get('cursor');
    expect(typeof cursor).toBe('string');
    expect((cursor ?? '').length).toBeGreaterThan(0);

    await expect(page.locator('table thead')).toBeVisible();
    const secondPageIds = await page.locator('tbody tr td:first-child').allTextContents();
    expect(secondPageIds).not.toEqual(firstPageIds);
  } finally {
    for (const id of created) {
      await apiRequest(page, String(deletePath).replace('{id}', encodeURIComponent(id)), {
        method: 'DELETE',
      });
    }
  }
});
