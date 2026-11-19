import { expect, test, type Page, type Route } from '@playwright/test';

/**
 * REQ-013 的**应用级**行为：企业登录入口 / 强制 SSO 提示 / 回调交接页。
 *
 * # 用 route 拦截测「前端契约」，用真后端测「端到端」
 *
 * 前四条用例把 `/api/iam/sso/**` 与 `/api/iam/login` 的响应用 `page.route` 拦下来 ——
 * 它们验的是**前端**对规格的落实（AC-S-021 / AC-S-109 / AC-S-100 预期 3），
 * 不依赖租户里真的配了 IdP。最后一条对**真后端**打一次，验真实接线；
 * 后端路由还不存在时按 D108 跳过并写明原因（「被测系统未就绪」），
 * 但**响应为 200 + 空列表**属于夹具/配置没准备好 → 必须 fail，不能跳过。
 *
 * # 前置
 *
 * `scripts/e2e-server.sh` 起同源服务（`platform-server` 托管 `apps/shell/dist`）。
 * 浏览器 locale 在 `playwright.config.ts` 钉成 `zh-CN`，所以文案断言用中文。
 */

const TENANT = 'tenant-a';

/** 两个 provider（与服务端 design §3.1 的形状一致）。 */
const PROVIDERS = [
  {
    type: 'oidc',
    id: 'oidc',
    label: 'corporate-sso',
    authorizeUrl: '/api/iam/sso/oidc/authorize',
  },
  { type: 'ldap', id: 'ldap', label: 'ldap-ad', loginUrl: '/api/iam/sso/ldap/login' },
];

/** 企业登录区块（provider 列表的渲染落点）。 */
const SSO_SECTION = '[data-testid="sso-buttons"]';

/** 当前地址的 pathname（不比较 origin / query）。 */
function pathOf(page: Page): string {
  return new URL(page.url()).pathname;
}

/** 把 providers 端点的响应换成给定列表。 */
async function stubProviders(page: Page, providers: readonly unknown[]): Promise<void> {
  await page.route('**/api/iam/sso/providers*', (route: Route) =>
    route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({ providers }),
    }),
  );
}

/** 填租户（会触发 providers 请求：登录页有 250ms 防抖）。 */
async function fillTenant(page: Page, tenant: string): Promise<void> {
  await page.getByLabel('租户').fill(tenant);
}

test('providers 非空 → 登录页按类型渲染 OIDC 入口与 LDAP 表单（AC-S-109）', async ({ page }) => {
  await stubProviders(page, PROVIDERS);
  const providersRequest = page.waitForRequest((request) =>
    new URL(request.url()).pathname.endsWith('/api/iam/sso/providers'),
  );

  await page.goto('/login');
  await fillTenant(page, TENANT);

  // 租户必须由登录页显式带给 providers 端点（此刻没有会话，SDK 上下文里没有租户）。
  expect((await providersRequest).headers()['x-tenant-id']).toBe(TENANT);

  const section = page.locator(SSO_SECTION);
  await expect(section).toBeVisible();
  await expect(page.getByRole('button', { name: /OIDC/ })).toBeVisible();
  await expect(page.getByLabel('目录用户名')).toBeVisible();
  await expect(page.getByLabel('目录密码')).toBeVisible();

  // 本地登录仍然可用（企业登录是**并列入口**，不是替换）。
  // `exact`：企业登录按钮的文案里也有「登录」二字，不精确匹配会命中三个按钮。
  await expect(page.getByRole('button', { name: '登录', exact: true })).toBeVisible();
});

test('providers 为空数组 → 不渲染企业登录区块（不是禁用按钮）', async ({ page }) => {
  await stubProviders(page, []);
  const providersRequest = page.waitForRequest((request) =>
    new URL(request.url()).pathname.endsWith('/api/iam/sso/providers'),
  );

  await page.goto('/login');
  await fillTenant(page, 'tenant-c');
  await providersRequest;

  await expect(page.locator(SSO_SECTION)).toHaveCount(0);
  await expect(page.getByRole('button', { name: '登录' })).toBeEnabled();
});

test('providers 接口失败 → 静默隐藏区块，本地登录不受影响（AC-S-109）', async ({ page }) => {
  await page.route('**/api/iam/sso/providers*', (route: Route) =>
    route.fulfill({
      status: 500,
      contentType: 'application/json',
      body: JSON.stringify({ code: 'INTERNAL_ERROR', title: 'boom' }),
    }),
  );
  const providersRequest = page.waitForRequest((request) =>
    new URL(request.url()).pathname.endsWith('/api/iam/sso/providers'),
  );

  await page.goto('/login');
  await fillTenant(page, TENANT);
  await providersRequest;

  await expect(page.locator(SSO_SECTION)).toHaveCount(0);
  // 不能因为 providers 挂了就挡住本地登录。
  await expect(page.getByRole('button', { name: '登录' })).toBeEnabled();
  await expect(page.locator('[role="alert"]')).toHaveCount(0);
});

test('强制 SSO：403 SSO_REQUIRED → 提示企业登录并给出可点入口，不说「密码不正确」（AC-S-021）', async ({
  page,
}) => {
  await stubProviders(page, []);
  await page.route('**/api/iam/login', (route: Route) =>
    route.fulfill({
      status: 403,
      contentType: 'application/json',
      body: JSON.stringify({
        code: 'SSO_REQUIRED',
        title: 'SSO Required',
        // 后端把可用入口**写死在 params**（D109）—— 前端要能直接渲染它。
        params: { ssoProviders: PROVIDERS },
      }),
    }),
  );

  await page.goto('/login');
  await fillTenant(page, TENANT);
  await page.getByLabel('用户名').fill('alice@example.com');
  await page.getByLabel('密码').fill('whatever');
  await page.getByRole('button', { name: '登录' }).click();

  await expect(page.getByRole('alert')).toHaveText('该组织要求使用企业账号登录');
  // 关键：不能把 SSO_REQUIRED 显示成「用户名或密码错误」（AC-S-021 步骤 2/3）。
  await expect(page.getByRole('alert')).not.toHaveText('用户名或密码不正确');

  // 请求体里带回来的入口必须可点（OIDC 是整页跳转，LDAP 是表单）。
  await expect(page.locator(SSO_SECTION)).toBeVisible();
  await expect(page.getByRole('button', { name: /OIDC/ })).toBeVisible();
  await expect(page.getByLabel('目录用户名')).toBeVisible();
});

test('回调页缺 ticket → 「登录链接无效或已过期」并可返回登录页（P4）', async ({ page }) => {
  await page.goto('/sso/callback');

  const alert = page.getByRole('alert');
  await expect(alert).toHaveText('登录链接无效或已过期');
  // 处理中不能白屏：首帧有明确的状态文案（交换失败后切到错误态）。
  await expect(page.getByText('正在完成企业登录…')).toHaveCount(0);

  await page.getByRole('button', { name: '返回登录页' }).click();
  await expect(page).toHaveURL(/\/login$/);
  expect(pathOf(page)).toBe('/login');
});

test('回调页 ticket 已用 / 过期（401 SSO_TICKET_INVALID）→ 「登录链接已失效，请重新登录」', async ({
  page,
}) => {
  await page.route('**/api/iam/sso/exchange', (route: Route) =>
    route.fulfill({
      status: 401,
      contentType: 'application/json',
      body: JSON.stringify({ code: 'SSO_TICKET_INVALID', title: 'SSO Ticket Invalid' }),
    }),
  );

  await page.goto('/sso/callback?ticket=already-used');

  await expect(page.getByRole('alert')).toHaveText('登录链接已失效，请重新登录');
  await expect(page.getByRole('button', { name: '返回登录页' })).toBeVisible();
  // 失败不能跳首页（会话没建立）。
  expect(pathOf(page)).toBe('/sso/callback');
});

test('回调页网络失败 → 「网络不可用」；与后端错误分开提示', async ({ page }) => {
  await page.route('**/api/iam/sso/exchange', (route: Route) => route.abort('failed'));

  await page.goto('/sso/callback?ticket=t-1');

  await expect(page.getByRole('alert')).toHaveText('网络不可用，请检查连接');
});

test('回调页成功 → 先显示处理中，再把 ticket 换成会话并跳首页（token 不进 URL）', async ({
  page,
}) => {
  // 把 exchange 卡住，证明**成功路径也有 loading 态**（不白屏）。
  let release: () => void = () => {};
  const gate = new Promise<void>((resolve) => {
    release = resolve;
  });
  await page.route('**/api/iam/sso/exchange', async (route: Route) => {
    await gate;
    await route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({
        accessToken: 'e2e-access-token',
        refreshToken: 'e2e-refresh-token',
        tokenType: 'Bearer',
        expiresIn: 900,
        user: { id: 'u-bob', username: 'bob', tenantId: TENANT },
        permissions: [],
      }),
    });
  });

  await page.goto('/sso/callback?ticket=e2e-ticket');

  await expect(page.getByRole('status')).toHaveText('正在完成企业登录…');
  expect(pathOf(page)).toBe('/sso/callback');

  release();
  await expect(page).toHaveURL(/\/$/);
  // ticket 被 `replace` 掉，地址栏里不留一次性凭据（安全约束 §8）。
  expect(page.url()).not.toContain('ticket=');
  expect(page.url()).not.toContain('e2e-access-token');

  const stored = await page.evaluate(
    (key) => window.sessionStorage.getItem(key),
    'base.shell.session.v1',
  );
  expect(stored).not.toBeNull();
  const session = JSON.parse(stored ?? '{}') as { accessToken?: string; tenantId?: string };
  expect(session.accessToken).toBe('e2e-access-token');
  expect(session.tenantId).toBe(TENANT);

  // 会话**只**进 sessionStorage：URL 与 localStorage 都不得出现令牌（D99）。
  const localStorageDump = await page.evaluate(() => JSON.stringify(window.localStorage));
  expect(localStorageDump).not.toContain('e2e-access-token');
  expect(localStorageDump).not.toContain('e2e-refresh-token');
});

test('真后端：providers 非空时登录页出现企业登录入口（端到端）', async ({ page, request }) => {
  const probe = await request.get('/api/iam/sso/providers', {
    headers: { 'x-tenant-id': TENANT },
  });
  if (probe.status() === 404 || probe.status() === 405) {
    // D108：**只**允许因「被测系统未就绪」跳过，且必须写明原因。
    test.skip(
      true,
      `被测系统未就绪：/api/iam/sso/providers 返回 ${probe.status()}（REQ-013 后端未合入本分支）`,
    );
  }
  expect(probe.status()).toBe(200);

  const payload = (await probe.json()) as { providers?: unknown };
  const providers = Array.isArray(payload.providers) ? payload.providers : [];
  // 200 + 空列表 = 夹具 / 租户配置没准备好（不是「功能未就绪」）→ 必须 fail。
  expect(
    providers.length,
    'tenant-a 未配置任何 SSO provider：需要 seed 脚本写入 12 个 SSO 配置键（见 acceptance.md P1）',
  ).toBeGreaterThan(0);

  await page.goto('/login');
  await fillTenant(page, TENANT);
  await expect(page.locator(SSO_SECTION)).toBeVisible();
  await expect(page.getByRole('button', { name: /OIDC|LDAP/ }).first()).toBeVisible();
});
