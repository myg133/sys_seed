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
 * # skip 的判定规则（**D108**，2026-11-19 Pre-merge FAIL 的根因）
 *
 * | 情形 | 处置 | 理由 |
 * |---|---|---|
 * | 元数据 **404**（后端未声明实体） | **skip** | 这是「另一个 Dev 还没做完」的前置 |
 * | 列表 **403** | **fail** | 授权缺失 = 配置 bug，必须红（曾静默 skip → 假绿） |
 * | 列表 **500** | **fail** | 服务端错误 |
 * | 网络不可达 | **skip** | 环境没起（`baseURL` 打不通） |
 *
 * 换句话说：**skip 只用于「被测系统尚未就绪」，「夹具/授权不对」必须 fail。**
 * 旧版本把 403 也 skip 掉，于是「夹具少一个权限点」时 6 条用例全跳过、exit 0 ——
 * CI 绿着但一条都没跑。这类「假绿」比红灯更坏，因为没人会去看。
 *
 * # 数据前置：每条用例**自己管**（H-2）
 *
 * 用例之间不共享数据，也不依赖「库里恰好有什么」：
 * - 需要数据 → 用 API 造自己的记录（标题带唯一后缀），`finally` 里删掉；
 * - 需要空表 → 先把这个实体的记录全删掉（只动 `tenant-a` 的沙箱示例数据）。
 *
 * 所以本 spec **正序 / 反序 / 只跑单条**都必须全绿。
 * （验证方式：逐条按**反序单独执行** —— Playwright 1.58 的 CLI **没有** `--shuffle`，
 *   不要照抄网上较新版本的用法。）
 * 之前「空态用例要求表空、零代码用例要求表非空」是同一实体上互斥的隐含前置，
 * 任何库状态都必有一条红 —— 那不是夹具的问题，是用例设计缺陷。
 *
 * # 夹具
 *
 * `scripts/seed-e2e.sh` 的 `u-perm-write@example.com` 必须持有
 * `sandbox:demo:read` + `sandbox:demo:write` + `sandbox:demo:secret`，
 * `u-perm-read@example.com` 只持有 `sandbox:demo:read`（字段级权限的对照）。
 *
 * 第二个实体（零代码用例）默认 `demoNote`，可用 `SCHEMA_SECOND_ENTITY` 覆盖。
 */

const TENANT = 'tenant-a';
const PASSWORD = 'Tr0ub4dor&3xKq';
/** 会话存储键（与 `src/lib/session.ts` 的 `SESSION_STORAGE_KEY` 一致）。 */
const SESSION_KEY = 'base.shell.session.v1';

const MODULE = 'sandbox';
const PRIMARY_ENTITY = 'demoItem';
const SECOND_ENTITY = process.env.SCHEMA_SECOND_ENTITY ?? 'demoNote';

/** 有写权限的夹具用户（含字段级权限点）。 */
const USER_WRITE = process.env.SCHEMA_USER_WRITE ?? 'u-perm-write@example.com';
/** 无写权限的夹具用户（只读，且不含 `sandbox:demo:secret`）。 */
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

/** 列表信封。 */
interface ListBody {
  items?: Record<string, unknown>[];
  nextCursor?: string | null;
}

/** 元数据探测结果：**保留状态码**，因为 404 与 403 的处置不同（D108）。 */
interface MetaProbe {
  status: number;
  meta?: EntityMetaBody;
}

async function probeMeta(page: Page, entity: string): Promise<MetaProbe> {
  const response = await apiRequest(page, `/api/schema/${MODULE}/${entity}`);
  const body = response.body;
  if (response.status === 200 && body !== null && typeof body === 'object') {
    return { status: 200, meta: body as EntityMetaBody };
  }
  return { status: response.status };
}

/**
 * 元数据前置（D108）：**只有 404 才 skip**。
 *
 * 403 / 500 / 其它状态都 fail —— 后端已声明实体却读不到，是配置或实现缺陷。
 */
function requireMeta(probe: MetaProbe, entity: string): EntityMetaBody {
  if (probe.status === 404) {
    test.skip(true, `被测系统未就绪：/api/schema/${MODULE}/${entity} 返回 404（实体未声明）`);
  }
  expect(
    probe.status,
    `/api/schema/${MODULE}/${entity} 返回 ${probe.status}；只有 404 才允许 skip（D108）`,
  ).toBe(200);
  expect(probe.meta, `元数据 /api/schema/${MODULE}/${entity} 的响应体不是对象`).toBeDefined();
  return probe.meta as EntityMetaBody;
}

/** 取实体的一个必需 API 路径（缺失即 fail，不是 skip）。 */
function requireApiPath(
  meta: EntityMetaBody,
  key: 'list' | 'create' | 'update' | 'delete',
): string {
  const path = meta.api?.[key];
  expect(path, `元数据未声明 api.${key}：渲染器无法走该动作`).toBeDefined();
  return String(path);
}

/** 表头文本（按 DOM 顺序；**排除行操作列** —— 它是 `<th scope="col">` 但没有字段标签）。 */
async function headerTexts(page: Page): Promise<string[]> {
  return page.locator('table thead th:not(.base-schema-actions)').allTextContents();
}

/** 列表接口状态（`api.list` 未声明时为 `0`）。 */
async function listStatus(page: Page, meta: EntityMetaBody): Promise<number> {
  const listPath = meta.api?.list;
  if (listPath === undefined) {
    return 0;
  }
  const response = await apiRequest(page, `${listPath}?limit=1`);
  return response.status;
}

/** 列表必须可读（D108）：**403 是夹具授权缺失，必须 fail，不许 skip**。 */
function expectListReadable(status: number, entity: string): void {
  expect(
    status,
    `${entity} 列表返回 ${status}：403 表示夹具缺 sandbox:demo:* 授权（先跑 scripts/seed-e2e.sh），不是环境前置（D108）`,
  ).toBe(200);
}

/** 一页取的记录（后端 limit 上限 200；E2E 的数据量远小于此）。 */
async function listAll(page: Page, meta: EntityMetaBody): Promise<Record<string, unknown>[]> {
  const listPath = requireApiPath(meta, 'list');
  const response = await apiRequest(page, `${listPath}?limit=200`);
  expect(response.status, `列表 ${listPath} 返回 ${response.status}`).toBe(200);
  return ((response.body ?? {}) as ListBody).items ?? [];
}

/** 用 API 造一条记录，返回 id（用例自己造的数据，`finally` 里必须删）。 */
async function createViaApi(
  page: Page,
  meta: EntityMetaBody,
  body: Record<string, unknown>,
): Promise<string> {
  const createPath = requireApiPath(meta, 'create');
  const response = await apiRequest(page, createPath, { method: 'POST', body });
  expect(response.status, `创建 ${createPath} 返回 ${response.status}`).toBe(201);
  const id = ((response.body ?? {}) as { id?: unknown }).id;
  expect(typeof id, '创建响应没有 id').toBe('string');
  return String(id);
}

/** 删除自己造的数据（失败时 `finally` 也会调到）。 */
async function deleteIds(page: Page, meta: EntityMetaBody, ids: readonly string[]): Promise<void> {
  const deletePath = meta.api?.delete;
  if (deletePath === undefined) {
    return;
  }
  for (const id of ids) {
    await apiRequest(page, deletePath.replace('{id}', encodeURIComponent(id)), {
      method: 'DELETE',
    });
  }
}

/** 记录 id（列表项里的 `id` 字段）。 */
function idOf(item: Record<string, unknown>): string {
  return typeof item['id'] === 'string' ? item['id'] : '';
}

/**
 * 清空某实体的全部记录 → **空态用例自己的前置**（H-2）。
 *
 * 只动 `tenant-a` 的沙箱示例数据（`demoNote`），不碰任何业务表。
 * 其它用例都自己造数据，所以清空不影响它们 —— 这正是不再需要「隐含前置」的原因。
 */
async function clearEntity(page: Page, meta: EntityMetaBody): Promise<number> {
  let deleted = 0;
  // 分页兜底：每轮最多删 200 条，最多 10 轮（防止写逻辑出错时死循环）。
  for (let round = 0; round < 10; round += 1) {
    const items = await listAll(page, meta);
    const ids = items.map(idOf).filter((id) => id !== '');
    if (ids.length === 0) {
      return deleted;
    }
    await deleteIds(page, meta, ids);
    deleted += ids.length;
    if (ids.length < 200) {
      return deleted;
    }
  }
  return deleted;
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
  const meta = requireMeta(await probeMeta(page, PRIMARY_ENTITY), PRIMARY_ENTITY);
  const labels = (meta.fields ?? []).map((field) => field.label ?? '');
  expect(labels.length).toBeGreaterThan(0);
  expectListReadable(await listStatus(page, meta), PRIMARY_ENTITY);

  // 数据前置自己造（AC-S-082 要求 ≥ 3 条）：不依赖夹具残留、也不依赖别的用例。
  const marker = `hdr-${Date.now()}`;
  const ids: string[] = [];
  try {
    for (let index = 0; index < 3; index += 1) {
      ids.push(await createViaApi(page, meta, { title: `${marker}-${index}` }));
    }

    await openSchema(page, PRIMARY_ENTITY);
    // 逐项相等（不是「包含」—— 否则乱序也会通过）。
    expect(await headerTexts(page)).toEqual(labels);
  } finally {
    await deleteIds(page, meta, ids);
  }
});

test('空态：不渲染表头空壳，只渲染「暂无数据」（AC-S-082 步骤 5）', async ({ page }) => {
  await login(page, USER_WRITE);
  const meta = requireMeta(await probeMeta(page, SECOND_ENTITY), SECOND_ENTITY);
  expectListReadable(await listStatus(page, meta), SECOND_ENTITY);

  // 前置：**先把这个实体清空**（H-2）—— 不假设库里「恰好没有数据」。
  await clearEntity(page, meta);
  expect((await listAll(page, meta)).length, '清空后列表仍有数据，空态前置不成立').toBe(0);

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
  const full = requireMeta(await probeMeta(page, PRIMARY_ENTITY), PRIMARY_ENTITY);
  const fullKeys = (full.fields ?? []).map((field) => field.key ?? '');

  // 数据前置：字段可见性要有行才能取证 → 自己造一条。
  let id = '';
  try {
    id = await createViaApi(page, full, { title: `perm-${Date.now()}` });

    await login(page, USER_READ);
    const limited = requireMeta(await probeMeta(page, PRIMARY_ENTITY), PRIMARY_ENTITY);
    const limitedKeys = (limited.fields ?? []).map((field) => field.key ?? '');

    // 两个用户看到的字段必须不同。**没有差异 = 夹具没授 sandbox:demo:secret**，
    // 那是配置缺陷 → fail（D108），不是「待就绪」。
    expect(
      limitedKeys.length,
      '只读用户与写用户看到的字段数相同：夹具缺 sandbox:demo:secret 授权差（D108，不许 skip）',
    ).toBeLessThan(fullKeys.length);

    expectListReadable(await listStatus(page, limited), PRIMARY_ENTITY);

    await openSchema(page, PRIMARY_ENTITY);
    expect(await headerTexts(page)).toEqual((limited.fields ?? []).map((f) => f.label ?? ''));

    // 被过滤字段的 label 不能出现在 DOM 里（不是 display:none）。
    const bodyText = await page.locator('body').innerText();
    for (const key of fullKeys.filter((candidate) => !limitedKeys.includes(candidate))) {
      const label = (full.fields ?? []).find((field) => field.key === key)?.label ?? key;
      expect(bodyText).not.toContain(label);
    }
  } finally {
    // 清理要用写用户（只读用户没有写权限）。
    if (id !== '') {
      await login(page, USER_WRITE);
      await deleteIds(page, full, [id]);
    }
  }
});

test('零代码：同一个页面渲染两个不同实体（AC-S-085 / AC-S-086）', async ({ page }) => {
  await login(page, USER_WRITE);
  const primary = requireMeta(await probeMeta(page, PRIMARY_ENTITY), PRIMARY_ENTITY);
  const second = requireMeta(await probeMeta(page, SECOND_ENTITY), SECOND_ENTITY);
  expectListReadable(await listStatus(page, primary), PRIMARY_ENTITY);
  expectListReadable(await listStatus(page, second), SECOND_ENTITY);

  // 数据前置：两个实体各造一条（**空表不渲染 thead**，所以不能靠夹具残留）。
  const created: { meta: EntityMetaBody; ids: string[] }[] = [
    { meta: primary, ids: [] },
    { meta: second, ids: [] },
  ];
  try {
    created[0]?.ids.push(await createViaApi(page, primary, { title: `zc-${Date.now()}` }));
    created[1]?.ids.push(
      await createViaApi(page, second, { title: `zc-${Date.now()}`, content: 'e2e' }),
    );

    await openSchema(page, PRIMARY_ENTITY);
    expect(await headerTexts(page)).toEqual((primary.fields ?? []).map((f) => f.label ?? ''));

    await openSchema(page, SECOND_ENTITY);
    expect(await headerTexts(page)).toEqual((second.fields ?? []).map((f) => f.label ?? ''));
  } finally {
    for (const entry of created) {
      await deleteIds(page, entry.meta, entry.ids);
    }
  }
});

test('CRUD 闭环：新建 → 编辑 → 删除（AC-S-086）', async ({ page }) => {
  await login(page, USER_WRITE);
  const meta = requireMeta(await probeMeta(page, PRIMARY_ENTITY), PRIMARY_ENTITY);
  // 元数据声明了 update / delete，渲染器才有按钮；缺了就是实现缺陷 → fail。
  requireApiPath(meta, 'update');
  requireApiPath(meta, 'delete');
  expectListReadable(await listStatus(page, meta), PRIMARY_ENTITY);

  await openSchema(page, PRIMARY_ENTITY);

  const marker = `e2e-${Date.now()}`;
  const ids: string[] = [];
  try {
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
  } finally {
    // UI 中途失败时，把自己造的那条兜底删掉（按 marker 从列表里找回来）。
    const leftovers = (await listAll(page, meta))
      .filter((item) => String(item['title'] ?? '').startsWith(marker))
      .map(idOf)
      .filter((candidate) => candidate !== '');
    ids.push(...leftovers);
    await deleteIds(page, meta, ids);
  }
});

test('游标分页：下一页请求带上一页的 nextCursor（AC-S-082 步骤 4）', async ({ page }) => {
  await login(page, USER_WRITE);
  const meta = requireMeta(await probeMeta(page, PRIMARY_ENTITY), PRIMARY_ENTITY);
  const listPath = requireApiPath(meta, 'list');
  requireApiPath(meta, 'create');
  requireApiPath(meta, 'delete');
  expectListReadable(await listStatus(page, meta), PRIMARY_ENTITY);

  // 数据前置：自己播 21 条（一页 20），保证一定有第二页；
  // 不依赖「库里恰好有 20 条以上」这个隐含状态。
  const marker = `page-seed-${Date.now()}`;
  const created: string[] = [];
  try {
    for (let index = 0; index < 21; index += 1) {
      created.push(await createViaApi(page, meta, { title: `${marker}-${index}` }));
    }

    await openSchema(page, PRIMARY_ENTITY);
    const firstPageIds = await page.locator('tbody tr td:first-child').allTextContents();
    expect(firstPageIds.length).toBeGreaterThan(0);

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
    await deleteIds(page, meta, created);
  }
});
