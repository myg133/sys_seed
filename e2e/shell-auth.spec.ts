import { expect, test, type Page } from '@playwright/test';

/**
 * REQ-011 的应用级 E2E：**租户来源**（D98）+ **会话跨整页重载**（D99）。
 *
 * 为什么必须用真浏览器：QA 的 HIGH-1 是「单测全绿但真机登录失败」——
 * 缺租户头只有打真后端才看得出来（`401 TENANT_CONTEXT_MISSING`）。
 *
 * 前置（`scripts/e2e-server.sh`）：`base_test` 里的夹具用户，密码统一为
 * `Tr0ub4dor&3xKq`，租户 `tenant-a`。三组权限：
 *   - `u-perm-read@example.com` → 只有 `rbac:role:read`
 *   - `u-perm-write@example.com` → `rbac:role:read` + `rbac:role:write`
 *   - `u-perm-none@example.com` → 无任何权限点
 *
 * ⚠️ **夹具必须由 `scripts/seed-e2e.sh` 显式播种**（D101）：
 * 跑过 `./scripts/test-integration.sh` 之后库会被重置，本 spec 依赖的用户
 * 既不存在也不可复用（旧版本靠后端测试的副作用建号 → 「跑过集成测试就不能跑 E2E」）。
 * 顺序：`test-integration.sh` → `seed-e2e.sh` → 起服务 → 本 spec。
 */

const TENANT = 'tenant-a';
const PASSWORD = 'Tr0ub4dor&3xKq';
/** 会话存储键（与 `src/lib/session.ts` 的 `SESSION_STORAGE_KEY` 一致）。 */
const SESSION_KEY = 'base.shell.session.v1';

/** 当前地址的 pathname（不比较 origin / query）。 */
function pathOf(page: Page): string {
  return new URL(page.url()).pathname;
}

/** 填表（租户可留空，用于「空租户不发请求」的用例）。 */
async function fillLogin(page: Page, email: string, tenant: string): Promise<void> {
  await page.getByLabel('租户').fill(tenant);
  await page.getByLabel('用户名').fill(email);
  await page.getByLabel('密码').fill(PASSWORD);
}

/** 登录并等到首页渲染（`已就绪` 只在外壳启动完成后出现）。 */
async function login(page: Page, email: string): Promise<void> {
  await page.goto('/login');
  await fillLogin(page, email, TENANT);
  await page.getByRole('button', { name: '登录' }).click();
  await expect(page.getByText('已就绪')).toBeVisible();
  expect(pathOf(page)).toBe('/');
}

test('登录页填租户 → 登录请求带 x-tenant-id，租户随会话持久化', async ({ page }) => {
  const loginRequest = page.waitForRequest((request) =>
    new URL(request.url()).pathname.endsWith('/api/iam/login'),
  );

  await page.goto('/login');
  await fillLogin(page, 'u-perm-read@example.com', TENANT);
  await page.getByRole('button', { name: '登录' }).click();

  // HIGH-1 的根因：登录请求必须带租户头，且值等于用户填的。
  const request = await loginRequest;
  expect(request.headers()['x-tenant-id']).toBe(TENANT);

  await expect(page.getByText('已就绪')).toBeVisible();
  expect(pathOf(page)).toBe('/');

  // D99：令牌 + 租户都进了 sessionStorage（租户不跟着走 → 重载后请求又缺头）。
  const stored = await page.evaluate((key) => window.sessionStorage.getItem(key), SESSION_KEY);
  expect(stored).not.toBeNull();
  const session = JSON.parse(stored ?? '{}') as {
    accessToken?: string;
    refreshToken?: string;
    tenantId?: string;
  };
  expect(session.tenantId).toBe(TENANT);
  expect(typeof session.accessToken).toBe('string');
  expect((session.accessToken ?? '').length).toBeGreaterThan(0);

  // 整页重载：会话恢复，不需要重新登录。
  await page.reload();
  await expect(page.getByText('已就绪')).toBeVisible();
  expect(pathOf(page)).toBe('/');
});

test('错误密码 → 错误码透传 INVALID_CREDENTIALS，且**不**触发 refresh（MED-NEW-1）', async ({
  page,
}) => {
  const refreshCalls: string[] = [];
  page.on('request', (request) => {
    if (new URL(request.url()).pathname.endsWith('/api/iam/refresh')) {
      refreshCalls.push(request.url());
    }
  });

  await page.goto('/login');
  await page.getByLabel('租户').fill(TENANT);
  await page.getByLabel('用户名').fill('u-perm-read@example.com');
  await page.getByLabel('密码').fill('definitely-the-wrong-password');

  const loginResponse = page.waitForResponse((response) =>
    new URL(response.url()).pathname.endsWith('/api/iam/login'),
  );
  await page.getByRole('button', { name: '登录' }).click();

  const response = await loginResponse;
  expect(response.status()).toBe(401);
  const body = (await response.json()) as { code?: string };
  // 判据 1：码必须是后端真正的 INVALID_CREDENTIALS —— 不是刷新失败的 TOKEN_REVOKED。
  expect(body.code).toBe('INVALID_CREDENTIALS');

  // 判据 3：页面文案按真正的码分派。
  await expect(page.getByText('用户名或密码不正确')).toBeVisible();
  await expect(page.locator('body')).not.toContainText('TOKEN_REVOKED');
  expect(pathOf(page)).toBe('/login');

  // 判据 2：刷新接口一次都没有被调用（刷新救不了密码错）。
  expect(refreshCalls).toHaveLength(0);
});

test('租户为空 → 给出提示且不发登录请求', async ({ page }) => {
  const loginRequests: string[] = [];
  page.on('request', (request) => {
    if (new URL(request.url()).pathname.endsWith('/api/iam/login')) {
      loginRequests.push(request.url());
    }
  });

  await page.goto('/login');
  await fillLogin(page, 'u-perm-read@example.com', '');
  await page.getByRole('button', { name: '登录' }).click();

  await expect(page.getByText('请填写租户标识')).toBeVisible();
  expect(loginRequests).toHaveLength(0);
  expect(pathOf(page)).toBe('/login');
});

test('只读用户直接敲 /admin/roles → 403 页，目标接口不被请求（重载后仍 403）', async ({ page }) => {
  await login(page, 'u-perm-read@example.com');
  // 先整页重载：确认会话（令牌 + 租户）跨重载存活，再走深链。
  await page.reload();
  await expect(page.getByText('已就绪')).toBeVisible();

  const targetCalls: string[] = [];
  page.on('request', (request) => {
    if (new URL(request.url()).pathname === '/api/rbac/roles') {
      targetCalls.push(request.url());
    }
  });

  // 直接输入 URL = 整页加载（不是前端路由跳转）—— AC-S-068 步骤 3。
  await page.goto('/admin/roles');
  await expect(page.getByTestId('forbidden')).toBeVisible();
  expect(pathOf(page)).toBe('/403');
  expect(targetCalls).toHaveLength(0);

  // 在 /403 上再重载一次：仍然不是登录页。
  await page.reload();
  await expect(page.getByTestId('forbidden')).toBeVisible();
  expect(pathOf(page)).toBe('/403');
  expect(targetCalls).toHaveLength(0);
});

test('有写权限的用户直接敲 /admin/roles → 落在该页面并加载数据', async ({ page }) => {
  await login(page, 'u-perm-write@example.com');

  const rolesResponse = page.waitForResponse(
    (response) => new URL(response.url()).pathname === '/api/rbac/roles',
  );
  await page.goto('/admin/roles');

  await expect(page.getByRole('heading', { name: '角色管理' })).toBeVisible();
  expect(pathOf(page)).toBe('/admin/roles');
  expect((await rolesResponse).status()).toBe(200);

  // 重载后仍在受保护页：令牌与租户都从 sessionStorage 恢复。
  await page.reload();
  await expect(page.getByRole('heading', { name: '角色管理' })).toBeVisible();
  expect(pathOf(page)).toBe('/admin/roles');
});

test('无任何权限的用户：登录成功但权限点为空，受保护页仍拦', async ({ page }) => {
  const loginResponse = page.waitForResponse((response) =>
    new URL(response.url()).pathname.endsWith('/api/iam/login'),
  );

  await login(page, 'u-perm-none@example.com');

  // 登录成功（不是 401），但权限点是**空数组**（fail closed 的是判定，不是登录）。
  const body = (await (await loginResponse).json()) as { permissions?: string[] };
  expect(body.permissions).toEqual([]);

  // 首页的写操作入口不渲染，`<Can>` 落 fallback。
  await expect(page.locator('a.base-home-action')).toHaveCount(0);
  await expect(page.getByText('无权限')).toBeVisible();

  await page.goto('/admin/roles');
  await expect(page.getByTestId('forbidden')).toBeVisible();
  expect(pathOf(page)).toBe('/403');
});

test('未登录直接敲受保护 URL → 落登录页，页面不出现 403 字样', async ({ page }) => {
  await page.goto('/admin/roles');

  await expect(page.getByRole('heading', { name: '登录' })).toBeVisible();
  expect(pathOf(page)).toBe('/login');
  await expect(page.getByText('403')).toHaveCount(0);
});

test('新开标签页不继承会话（sessionStorage 语义）', async ({ page, context }) => {
  await login(page, 'u-perm-write@example.com');

  // 新标签页读不到另一个标签页的 `sessionStorage` —— 这正是 D99 选它的原因。
  // 若有人把它换成 `localStorage`，这个用例会变红。
  const other = await context.newPage();
  await other.goto('/admin/roles');
  await expect(other.getByRole('heading', { name: '登录' })).toBeVisible();
  expect(pathOf(other)).toBe('/login');
  await other.close();
});
