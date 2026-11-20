import { expect, test, type APIRequestContext, type Page } from '@playwright/test';

/**
 * 审计查询页的 E2E（REQ-014 / AC-S-132；QA 用例 TC-014-111 ~ 120）。
 *
 * # 夹具
 *
 * | 账号 | 权限点 | 用途 |
 * |---|---|---|
 * | `u-perm-audit@example.com` | `audit:events:read` | 能进审计页（数据类用例） |
 * | `u-perm-none@example.com` | （无） | 落 403 页（守卫用例） |
 *
 * 租户 `tenant-a`，密码 `Tr0ub4dor&3xKq`（与 `scripts/seed-e2e.sh` 一致）。
 *
 * 夹具（账号 + 授权 + **≥ 11 条**审计记录）由 `scripts/seed-e2e.sh` 播种：
 * 审计记录走平台的写通道 `platform_audit_append` → 链自洽、幂等（重跑不重复）。
 * ⚠️ 夹具缺失**必须 fail**（D108），不许用 skip 掩盖 —— 所以探测逻辑只把
 * 「接口还不存在」当成可跳过。
 *
 * # `skip` 的唯一理由（D108 / dev-guide §9.9）
 *
 * 后端 `/api/audit/*` 由另一个 Dev 实现 → **数据类用例**在接口还不存在时
 * 按「**被测系统未就绪**」跳过。
 * 守卫类用例（403 页）**不需要后端** → **一律不 skip**。
 *
 * # 分页是**游标**分页（D169）
 *
 * 请求里带 `cursor` + `limit`，**不带 `page`** → 翻页用「下一页」按钮，
 * 页码指示是「**本次浏览**第 N 页」（游标分页算不出全量页码）。
 *
 * # 为什么翻页用例先调小页大小
 *
 * 默认页大小是 20（契约 C7 的默认 `limit`）；夹具里的审计记录通常不到 20 条
 * → 把页大小调到 10 才可能有两页。翻页后**记录不重复**这条判据要求
 * 夹具里至少有 11 条记录。
 */

const TENANT = 'tenant-a';
const PASSWORD = 'Tr0ub4dor&3xKq';

/** 审计查询接口。 */
const AUDIT_EVENTS_PATH = '/api/audit/events';

/** 有 `audit:events:read` 的账号。 */
const AUDITOR = 'u-perm-audit@example.com';

/** 无任何权限点的账号（fail closed）。 */
const NO_PERMISSION = 'u-perm-none@example.com';

/** 会话存储键（与 `src/lib/session.ts` 的 `SESSION_STORAGE_KEY` 一致）。 */
const SESSION_KEY = 'base.shell.session.v1';

/**
 * 跳过理由：**只能**是「被测系统未就绪」。
 *
 * ❌ 不允许因为「夹具不对」「授权没配好」而 skip —— 那些必须 **fail**。
 */
const NOT_READY = '审计接口尚未实现（被测系统未就绪）';

/** 当前地址的 pathname。 */
function pathOf(page: Page): string {
  return new URL(page.url()).pathname;
}

/**
 * 填表并登录（等到外壳启动完成）。
 *
 * 带上 `X-Forwarded-For`：登录本身也会写审计事件（`iam.login.succeeded`），
 * 平台中间件把该头的**第一跳**填进事件的 `ip`（D149）→ 不带这个头的话，
 * 登录事件的「来源 IP」是空串，TC-014-112 的「五格都非空」会被脏数据顶掉。
 */
async function login(page: Page, email: string): Promise<void> {
  await page.setExtraHTTPHeaders({ 'x-forwarded-for': '203.0.113.9' });
  await page.goto('/login');
  await page.getByLabel('租户').fill(TENANT);
  await page.getByLabel('用户名').fill(email);
  await page.getByLabel('密码').fill(PASSWORD);
  await page.getByRole('button', { name: '登录' }).click();
  await expect(page.getByText('已就绪')).toBeVisible();
}

/**
 * 审计接口**是否已就绪**。
 *
 * 404 / 501 → 路由还不存在（后端这一轮还没实现）→ 被测系统未就绪 → 允许 skip。
 * 其它状态（401 / 403 / 200 …）→ 路由在 → 被测系统已就绪 → 用例**必须跑**。
 *
 * ⚠️ **只判状态码会误判**：静态资源回落把**任何没有扩展名的路径**
 * （含未注册的 `/api/...`）回落成 `index.html`
 * （`crates/platform-server/src/static_files.rs:33`）→ 未实现的接口返回的是
 * **200 + text/html**，不是 404。
 * → 所以还要看**内容类型**：只有 **JSON** 响应才算「接口在」。
 * 拿到 HTML = 路由还没实现 → 同样按「被测系统未就绪」跳过。
 */
async function auditApiReady(request: APIRequestContext): Promise<boolean> {
  const response = await request.get(AUDIT_EVENTS_PATH, { failOnStatusCode: false });
  if (response.status() === 404 || response.status() === 501) {
    return false;
  }
  const contentType = response.headers()['content-type'] ?? '';
  return contentType.includes('json');
}

/** 会话里的 access token（用于直接调后端接口做交叉验证）。 */
async function accessToken(page: Page): Promise<string> {
  const raw = await page.evaluate((key) => window.sessionStorage.getItem(key), SESSION_KEY);
  const session = JSON.parse(raw ?? '{}') as { accessToken?: string };
  return session.accessToken ?? '';
}

/** 审计接口的请求（记 URL 供断言）。 */
function trackAuditRequests(page: Page): URL[] {
  const urls: URL[] = [];
  page.on('request', (request) => {
    if (new URL(request.url()).pathname === AUDIT_EVENTS_PATH) {
      urls.push(new URL(request.url()));
    }
  });
  return urls;
}

/** 表格里当前页的记录 id（第一列是 `ID`）。 */
async function rowIds(page: Page): Promise<string[]> {
  return await page.locator('table.base-table tbody tr td:first-child').allTextContents();
}

test('TC-014-118 / TC-014-119：无权限账号访问审计页 → 落 403 页，目标接口请求数 == 0', async ({
  page,
}) => {
  const targetCalls = trackAuditRequests(page);

  await login(page, NO_PERMISSION);
  // 直接敲 URL = 整页加载（深链也走守卫）。
  await page.goto('/admin/audit');

  await expect(page.getByTestId('forbidden')).toBeVisible();
  expect(pathOf(page)).toBe('/403');

  // 403 页**不是空白页**：有可见文本 + 403 标识。
  const forbidden = page.getByTestId('forbidden');
  expect(((await forbidden.textContent()) ?? '').trim().length).toBeGreaterThan(0);
  await expect(page.getByRole('heading', { name: '403' })).toBeVisible();

  // 403 页里**没有任何审计记录内容**。
  await expect(forbidden).not.toContainText('rbac.role.grant');
  await expect(forbidden).not.toContainText('before');
  await expect(forbidden).not.toContainText('after');

  // 守卫在**请求之前**拦下：目标接口一次都没被请求。
  expect(targetCalls).toHaveLength(0);
});

test('TC-014-111 / TC-014-112：进入审计查询页 → 列出记录，操作人 / 时间 / 动作 / 对象 / 来源 IP 同时显示', async ({
  page,
  request,
}) => {
  test.skip(!(await auditApiReady(request)), NOT_READY);

  await login(page, AUDITOR);
  await page.goto('/admin/audit');

  await expect(page.getByRole('heading', { name: '审计日志' })).toBeVisible();

  // 五项必显字段（列标签是 i18n 渲染出的文案）。
  for (const label of ['操作人', '发生时间', '动作', '对象', '来源 IP']) {
    await expect(page.getByRole('columnheader', { name: label })).toBeVisible();
  }

  // 有记录（不是空态）。
  await expect(page.getByText('暂无审计记录')).toHaveCount(0);
  const ids = await rowIds(page);
  expect(ids.length).toBeGreaterThanOrEqual(1);

  // 第一行的五个单元格都非空。
  const firstRow = page.locator('table.base-table tbody tr').first();
  for (const index of [1, 2, 3, 4, 5]) {
    const cell = firstRow.locator('td').nth(index);
    expect(((await cell.textContent()) ?? '').trim()).not.toBe('');
  }
});

test('TC-014-113 / TC-014-114：翻页 → 请求带 cursor（游标分页，不是 page），且记录不重复', async ({
  page,
  request,
}) => {
  test.skip(!(await auditApiReady(request)), NOT_READY);

  await login(page, AUDITOR);
  await page.goto('/admin/audit');

  // 页大小调到 10（默认 20 的话夹具记录数不够凑出两页）。
  await page.getByLabel('每页条数').selectOption('10');
  await expect(page.getByTestId('audit-pager')).toBeVisible();

  const targetCalls = trackAuditRequests(page);
  const firstPageIds = await rowIds(page);
  expect(firstPageIds.length).toBeGreaterThan(0);

  await page.getByTestId('audit-next-page').click();

  // 服务端**游标**分页：游标进**请求参数**（不是前端切片）。
  await expect.poll(() => targetCalls.length).toBeGreaterThan(0);
  const last = targetCalls.at(-1);
  expect(last?.searchParams.get('cursor')).toBeTruthy();
  expect(last?.searchParams.get('limit')).toBe('10');
  // ⚠️ **不带 `page`**：页码分页在持续追加的审计表上必然重复 / 遗漏（D169）。
  expect(last?.searchParams.get('page')).toBeNull();

  // 页码指示 = 「**本次浏览**第 N 页」（不是「第 N 页 / 共 M 页」）。
  await expect(page.getByTestId('audit-page-indicator')).toHaveText('本次浏览第 2 页');

  // 记录不重复：两页的 id 没有交集。
  const secondPageIds = await rowIds(page);
  expect(secondPageIds.length).toBeGreaterThan(0);
  const first = new Set(firstPageIds);
  for (const id of secondPageIds) {
    expect(first.has(id)).toBe(false);
  }
});

test('TC-014-113（回退）：「上一页」→ 请求回到第 1 页（不带 cursor）', async ({
  page,
  request,
}) => {
  test.skip(!(await auditApiReady(request)), NOT_READY);

  await login(page, AUDITOR);
  await page.goto('/admin/audit');

  // 页大小调到 10（默认 20 的话夹具记录数不够凑出两页）。
  // ⚠️ 不许省这一行：夹具只播 11 条（`scripts/seed-e2e.sh` 的 `FOR v_index IN 1..11`），
  // 默认 `limit=20` 时 `total(11) < limit(20)` → `nextCursor == null`
  // →「下一页」按钮 `disabled` → 点击超时。省掉它就会偷偷依赖「历史运行把库喂到 > 20 条」，
  // 干净库（`DROP SCHEMA public CASCADE` 后重播 seed）上**必红**。
  await page.getByLabel('每页条数').selectOption('10');
  await expect(page.getByTestId('audit-pager')).toBeVisible();

  const targetCalls = trackAuditRequests(page);
  await page.getByTestId('audit-next-page').click();
  await expect(page.getByTestId('audit-page-indicator')).toHaveText('本次浏览第 2 页');

  await page.getByTestId('audit-prev-page').click();
  await expect(page.getByTestId('audit-page-indicator')).toHaveText('本次浏览第 1 页');

  // 回到首页 → 请求**不带游标**（游标栈弹回哨兵）。
  const last = targetCalls.at(-1);
  expect(last?.searchParams.get('cursor')).toBeNull();
});

test('TC-014-115 / TC-014-117：按操作人过滤 → 过滤下发到后端，且与后端接口结果一致', async ({
  page,
  request,
}) => {
  test.skip(!(await auditApiReady(request)), NOT_READY);

  await login(page, AUDITOR);
  await page.goto('/admin/audit');

  const targetCalls = trackAuditRequests(page);
  // 取当前第一行的操作人（夹具里的记录，操作人互不相同）。
  const actor = (
    (await page.locator('table.base-table tbody tr').first().locator('td').nth(1).textContent()) ??
    ''
  ).trim();
  expect(actor).not.toBe('');

  await page.getByLabel('按操作人过滤').fill(actor);
  await page.getByRole('button', { name: '筛选' }).click();

  // 过滤**下发到后端**（不是在前端过滤已加载的那一页）。
  await expect.poll(() => targetCalls.length).toBeGreaterThan(0);
  expect(targetCalls.at(-1)?.searchParams.get('filter[actorId][eq]')).toBe(actor);

  // 行里只剩该操作人的记录。
  const actors = await page.locator('table.base-table tbody tr td:nth-child(2)').allTextContents();
  expect(actors.length).toBeGreaterThan(0);
  for (const value of actors) {
    expect(value.trim()).toBe(actor);
  }

  // 与**后端接口**的结果一致（同一过滤条件）。
  const token = await accessToken(page);
  const query = new URLSearchParams();
  query.set('filter[actorId][eq]', actor);
  query.set('limit', '200');
  const response = await request.get(`${AUDIT_EVENTS_PATH}?${query.toString()}`, {
    headers: { authorization: `Bearer ${token}`, 'x-tenant-id': TENANT },
    failOnStatusCode: false,
  });
  expect(response.status()).toBe(200);
  const body = (await response.json()) as { items?: { id?: string }[]; total?: number };
  const backendIds = (body.items ?? []).map((item) => item.id ?? '');
  const frontendIds = await rowIds(page);
  expect(new Set(frontendIds)).toEqual(new Set(backendIds));
});

test('TC-014-116：按动作过滤 → 过滤下发到后端', async ({ page, request }) => {
  test.skip(!(await auditApiReady(request)), NOT_READY);

  await login(page, AUDITOR);
  await page.goto('/admin/audit');

  const targetCalls = trackAuditRequests(page);
  const action = (
    (await page.locator('table.base-table tbody tr').first().locator('td').nth(3).textContent()) ??
    ''
  ).trim();
  expect(action).not.toBe('');

  await page.getByLabel('按动作过滤').fill(action);
  await page.getByRole('button', { name: '筛选' }).click();

  await expect.poll(() => targetCalls.length).toBeGreaterThan(0);
  expect(targetCalls.at(-1)?.searchParams.get('filter[action][eq]')).toBe(action);

  const actions = await page.locator('table.base-table tbody tr td:nth-child(4)').allTextContents();
  expect(actions.length).toBeGreaterThan(0);
  for (const value of actions) {
    expect(value.trim()).toBe(action);
  }
});
