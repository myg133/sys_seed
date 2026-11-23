import { expect, test, type Page } from '@playwright/test';

/**
 * REQ-020 / AC-S-192：请假审批的**端到端**闭环（列表 → 建单 → 审批 → 刷新保持）。
 *
 * # `skipped` 必须为 0
 *
 * 本文件**没有**任何 `test.skip` / 条件跳过（D108）：前置不满足（登录失败、
 * 权限点没播、接口 403/500）一律让它**红**，而不是「假装通过」。
 * AC-S-192 的判据就是**这一个文件**的 `skipped == 0`（不是全量 suite）。
 *
 * # 前置（见 `scripts/seed-e2e.sh`）
 *
 * | 账号 | 权限点 | 用途 |
 * |---|---|---|
 * | `u-leave-applicant@example.com` | `leave:request:read` + `write` | 建单 / 提交 |
 * | `u-leave-approver@example.com`  | `leave:request:read` + `approve` | 通过 / 驳回 |
 *
 * 两个账号是**分开**的：AC-S-192 步骤 3 明确要求「切审批人账号」。
 *
 * # 为什么先通过 API 造一条单
 *
 * 步骤 1 要「列表渲染出已有单据」——干净库上表是空的。用例**自己**造前置数据
 * （H-2：不依赖「库里恰好有什么」），再断言页面能把它渲染出来。
 *
 * # 审批只走 `decide` 端点
 *
 * 页面没有「改 status」的入口（D356 红线）；这里的断言也只看**界面结果**，
 * 不直接写库 —— 直奔 UI 的路径才是 AC 要验的。
 */

const TENANT = 'tenant-a';
const PASSWORD = 'Tr0ub4dor&3xKq';
/** 会话存储键（与 `src/lib/session.ts` 的 `SESSION_STORAGE_KEY` 一致）。 */
const SESSION_KEY = 'base.shell.session.v1';

const APPLICANT = 'u-leave-applicant@example.com';
const APPROVER = 'u-leave-approver@example.com';

/** 页面路由与接口（与 `modules/leave/frontend/routes.ts` / 后端路由一致）。 */
const LEAVE_PATH = '/leave';
const LEAVE_API = '/api/leave/requests';

/** 登录（平台夹具密码；登录后首页有「已就绪」）。 */
async function login(page: Page, email: string): Promise<void> {
  await page.goto('/login');
  await page.getByLabel('租户').fill(TENANT);
  await page.getByLabel('用户名').fill(email);
  await page.getByLabel('密码').fill(PASSWORD);
  await page.getByRole('button', { name: '登录' }).click();
  await expect(page.getByText('已就绪')).toBeVisible();
}

/** 切账号：清掉会话存储再整页重载（内存里的登录态随重载一起没了）。 */
async function logout(page: Page): Promise<void> {
  await page.evaluate(() => {
    window.sessionStorage.clear();
  });
  await page.goto('/login');
}

/** 用当前会话（`sessionStorage` 里的令牌）发一次 API 请求。 */
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

/** 取响应里的 `id`（形状不对 → 空串，随后断言会红得明确）。 */
function idOf(body: unknown): string {
  if (body !== null && typeof body === 'object' && 'id' in body) {
    const value = (body as { id?: unknown }).id;
    return typeof value === 'string' ? value : '';
  }
  return '';
}

/** 按理由定位列表行（理由带唯一后缀 → 正序 / 反序 / 重跑都只命中自己那行）。 */
function rowOf(page: Page, reason: string) {
  return page.locator('[data-testid="leave-row"]').filter({ hasText: reason });
}

test('请假：列表 → 建单 → 审批通过 → 刷新保持（AC-S-192）', async ({ page }) => {
  const stamp = Date.now();
  const seededReason = `E2E 种子单据 ${stamp}`;

  await login(page, APPLICANT);

  // ---- 前置：用真实端点造一条 `pending` 单（步骤 1 要求「列表已有单据」）----
  const seeded = await apiRequest(page, LEAVE_API, {
    method: 'POST',
    body: { startsOn: '2026-12-01', endsOn: '2026-12-02', reason: seededReason },
  });
  expect(seeded.status, '夹具建单应返回 201').toBe(201);
  const seededId = idOf(seeded.body);
  expect(seededId, '建单响应里应有 id').not.toBe('');
  const seededSubmit = await apiRequest(page, `${LEAVE_API}/${seededId}/submit`, {
    method: 'POST',
  });
  expect(seededSubmit.status, '夹具提交应返回 200').toBe(200);

  // ---- 步骤 1：列表渲染出已有单据（≥ 1 行）----
  await test.step('步骤 1：打开请假页，列表渲染出已有单据', async () => {
    await page.goto(LEAVE_PATH);
    await expect(page.getByTestId('leave-table')).toBeVisible();
    const rows = page.locator('[data-testid="leave-row"]');
    await expect(rows.first()).toBeVisible();
    expect(await rows.count(), '列表至少要有一行').toBeGreaterThanOrEqual(1);
    await expect(rowOf(page, seededReason)).toHaveCount(1);
  });

  // ---- 步骤 2：新建 → 填表 → 提交 → 列表出现新行 ----
  const uiReason = `E2E 界面建单 ${stamp}`;
  await test.step('步骤 2：新建表单提交后，列表出现新行', async () => {
    await page.getByTestId('leave-new').click();
    await expect(page.getByTestId('leave-form')).toBeVisible();
    await page.getByTestId('leave-starts-on').fill('2026-12-10');
    await page.getByTestId('leave-ends-on').fill('2026-12-11');
    await page.getByTestId('leave-reason').fill(uiReason);
    await page.getByTestId('leave-create').click();

    const row = rowOf(page, uiReason);
    await expect(row).toHaveCount(1);
    // 建单（draft）+ 提交（pending）两步都走了真实端点 → 新行是「待审批」。
    await expect(row.getByTestId('leave-status')).toHaveText('待审批');
  });

  // ---- 步骤 3：切审批人账号 → 点「通过」→ 状态变「已通过」----
  await test.step('步骤 3：换审批人账号，点「通过」后状态变为已通过', async () => {
    await logout(page);
    await login(page, APPROVER);
    await page.goto(LEAVE_PATH);

    const row = rowOf(page, uiReason);
    await expect(row).toHaveCount(1);
    // 审批按钮只在「有 approve 权限 + pending」时出现（D356 红线）。
    await expect(row.getByTestId('leave-approve')).toBeVisible();
    await row.getByTestId('leave-approve').click();
    await expect(row.getByTestId('leave-status')).toHaveText('已通过');
  });

  // ---- 步骤 4：刷新页面 → 状态保持（不是前端内存态）----
  await test.step('步骤 4：刷新页面后状态保持', async () => {
    await page.reload();
    await expect(page.getByTestId('leave-table')).toBeVisible();
    const row = rowOf(page, uiReason);
    await expect(row).toHaveCount(1);
    await expect(row.getByTestId('leave-status')).toHaveText('已通过');
    // 刷新后审批按钮不再出现（终态 + 不允许重复审批）。
    await expect(row.getByTestId('leave-approve')).toHaveCount(0);
  });
});
