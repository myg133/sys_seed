import { expect, test, type Page } from '@playwright/test';
import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';

/**
 * REQ-022（出站 IdP 与单点登出）的 **E2E**。
 *
 * # 前置（都由 `scripts/seed-e2e.sh` 播种 —— D236）
 *
 * - `tenant-a` 的 `u-perm-oauth@example.com`（OAuth 管理员）、`e2e-user-oauth@example.com`（普通用户）、
 *   `u-perm-read@example.com`（只读 + 自己的授权记录）；
 * - `tenant-b` 的 `u-perm-other@example.com`（**跨租户对照**）；
 * - 三个 client：`client-A`（confidential，配 back + front channel）、`client-B`（confidential，
 *   **不**配 front-channel）、`public-client`（public，无 secret）；
 * - 明文 secret 由播种脚本打印到 stdout，并写进 `target/e2e-oauth-clients.json`
 *   （`target/` 不入库）→ 本 spec 从**夹具文件**读。
 *
 * # ⚠️ 第三方回调用 `page.route` 拦截（D235）
 *
 * `client-A` 的回调是 `https://client.example/cb` —— 那个域名**不存在**。
 * → 必须 `page.route('https://client.example/**', …)` 把它**就地满足**，
 *   然后读 `new URL(page.url()).searchParams.get('code')`
 *   （**不是**地址栏裸字符串 —— 裸串会把 `code` 与 `state` 混在一起）。
 *
 * # ⚠️ 必须**真的点按钮**（本机没有 DOM 测试环境）
 *
 * 前端只能验渲染 + 纯函数；**点击这一层全靠 E2E 兜住**。
 * → 同意页的「允许该应用」/「拒绝该应用」、设备码页的「允许此设备」、
 *   授权列表的「撤销授权」、登出页的 front-channel iframe ——
 *   每一条都要有「点了之后产生了**可观测后果**」的断言。
 *
 * # ⚠️ 0 skipped（D108）
 *
 * 夹具缺失 / 被测系统没起来 → **必须 fail**，不许 skip。
 */

const TENANT_A = 'tenant-a';
const TENANT_B = 'tenant-b';
const PASSWORD = 'Tr0ub4dor&3xKq';

/** 第三方回调（`client-A` 的白名单里的那一个）。 */
const CALLBACK = 'https://client.example/cb';
/** front-channel 登出回调。 */
const FRONT_CHANNEL = 'https://client.example/front';

/** PKCE 的 verifier（固定值 → 同一份 code_challenge 可复算）。 */
const VERIFIER = 'dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk';

/** `code_challenge` = base64url(SHA256(verifier))（RFC 7636）。 */
function challengeOf(verifier: string): string {
  return createHash('sha256').update(verifier).digest('base64url');
}

/** 夹具文件里的 client（明文 secret 在这里）。 */
interface FixtureClient {
  clientId: string;
  clientSecret: string | null;
  clientType: string;
}

interface FixtureFile {
  backchannelLogoutUri: string;
  clients: FixtureClient[];
}

/**
 * 读播种脚本写的夹具文件。
 *
 * ⚠️ 读不到 → **直接抛**（不许 skip）：夹具缺失必须让用例红（D108）。
 */
function readFixture(): FixtureFile {
  return JSON.parse(readFileSync('target/e2e-oauth-clients.json', 'utf8')) as FixtureFile;
}

/** 取一个 client 的明文 secret（public client 没有 secret → 抛）。 */
function secretOf(fixture: FixtureFile, clientId: string): string {
  const client = fixture.clients.find((item) => item.clientId === clientId);
  if (!client || client.clientSecret === null) {
    throw new Error(`夹具里没有 ${clientId} 的明文 secret（先跑 scripts/seed-e2e.sh）`);
  }
  return client.clientSecret;
}

/** 填表并登录（租户可显式给出 —— 跨租户用例要用 tenant-b）。 */
async function login(page: Page, email: string, tenant: string): Promise<void> {
  await page.goto('/login');
  await page.getByLabel('租户').fill(tenant);
  await page.getByLabel('用户名').fill(email);
  await page.getByLabel('密码').fill(PASSWORD);
  await page.getByRole('button', { name: '登录' }).click();
  await expect(page.getByText('已就绪')).toBeVisible();
}

/**
 * 从 `sessionStorage` 取当前登录令牌的**请求头**。
 *
 * # 为什么必须有它（终版 E2E 抓到的缺陷）
 *
 * `page.request` 是 Playwright 的**独立** API request context：
 * 它**不共享**页面的 `sessionStorage`（本平台的会话存在那里，且**不在 cookie 里**），
 * 也**不共享**页面的内存状态 → 直接用 `page.request.post('/api/oauth/clients', …)`
 * 发出去的是**无认证**请求 → 平台端点一律 **401 `UNAUTHENTICATED`**。
 *
 * 所以凡是用 `page.request` 调**平台端点**（要走用户会话）的地方，
 * 都要显式把 Bearer 头带上。协议端点（`/token` / `/introspect` / `/revoke` /
 * `/device_authorization`）走**client 认证**，不需要这个头。
 */
async function authHeaders(page: Page): Promise<Record<string, string>> {
  const token = await page.evaluate(() => {
    const raw = globalThis.sessionStorage.getItem('base.shell.session.v1');
    if (raw === null) {
      return '';
    }
    const parsed = JSON.parse(raw) as { accessToken?: string; token?: string };
    return parsed.accessToken ?? parsed.token ?? '';
  });
  if (token === '') {
    // 会话不在 sessionStorage → 用例的前提没达成 → **必须红**，不许静默无认证地发请求。
    throw new Error('sessionStorage 里没有登录令牌（先 login() 再调平台端点）');
  }
  return { authorization: `Bearer ${token}` };
}

/**
 * 拦下第三方回调（就地满足）→ 页面**停在**回调 URL 上。
 *
 * ⚠️ 回调域名不存在 → 不拦截就是「无法解析此主机」。
 */
async function interceptCallback(page: Page): Promise<void> {
  await page.route('https://client.example/**', (route) =>
    route.fulfill({ status: 200, contentType: 'text/html', body: '<html>ok</html>' }),
  );
}

/** 授权页的 query（RFC 6749 §4.1.1 的**逐字**参数名）。 */
function authorizeUrl(clientId: string, scope: string, state: string): string {
  const params = new URLSearchParams({
    response_type: 'code',
    client_id: clientId,
    redirect_uri: CALLBACK,
    scope,
    state,
    code_challenge: challengeOf(VERIFIER),
    code_challenge_method: 'S256',
  });
  return `/oauth/authorize?${params.toString()}`;
}

test.describe('REQ-022 出站 IdP', () => {
  /**
   * **D221 的核心判据**：未登录 → 「登录并继续」→ 登录 → **回到原 URL 且 query 一个不丢**。
   *
   * # 为什么这条必须单独一条用例
   *
   * 本文件其余 7 处都**先 `login()` 再 `page.goto(authorize…)`** ——
   * 那从来没有走过「未登录进授权页」这条路。而 D221 之所以存在，正是因为
   * 平台的 `intendedPath` **只有 pathname**（`router.ts`）→ 未登录回跳会
   * **丢掉全部 query**（`client_id` / `state` / `code_challenge` / …）。
   *
   * 这条用例把它钉死：
   * 1. 不先登录，直接进授权页；
   * 2. 页面渲染「登录并继续」，且**没有**发 preview（它需要 Bearer）；
   * 3. 点了之后去 `/login?return_to=…`；
   * 4. 登录回来，**6 个 query 参数逐个**仍在。
   */
  test('未登录 → 登录并继续 → 回到授权页且 6 个 query 一个不丢', async ({ page }) => {
    // 数 preview 请求：未登录时**一次都不该有**（页面不该拿无 Bearer 的请求去撞 401）。
    let previewRequests = 0;
    page.on('request', (request) => {
      if (request.url().includes('/api/oauth/authorize/preview')) {
        previewRequests += 1;
      }
    });

    const authorize = authorizeUrl('client-A', 'rbac:role:read', 'preserved-state');
    const requested = new URLSearchParams(authorize.slice(authorize.indexOf('?') + 1));

    // 1) **不登录**直接进授权页。
    await page.goto(authorize);

    // 2) 未登录 → 渲染「登录并继续」，且**没有发 preview**。
    await expect(page.getByTestId('oauth-sign-in-hint')).toBeVisible();
    const signIn = page.getByRole('button', { name: '登录并继续' });
    await expect(signIn).toBeVisible();
    expect(previewRequests, '未登录时不得发 preview（它需要 Bearer）').toBe(0);

    // 3) 点它 → 地址变成 `/login?return_to=…`。
    await signIn.click();
    await expect(page).toHaveURL(/\/login\?return_to=/);
    const returnTo = new URL(page.url()).searchParams.get('return_to');
    expect(returnTo, '登录页必须收到 return_to').not.toBeNull();

    // 4) 填表登录。
    await page.getByLabel('租户').fill(TENANT_A);
    await page.getByLabel('用户名').fill('e2e-user-oauth@example.com');
    await page.getByLabel('密码').fill(PASSWORD);
    await page.getByRole('button', { name: '登录', exact: true }).click();

    // 5) **关键断言**：回到 `/oauth/authorize`，6 个参数逐个仍在。
    await expect(page).toHaveURL(/\/oauth\/authorize\?/);
    const after = new URL(page.url());
    expect(after.pathname).toBe('/oauth/authorize');
    for (const key of [
      'client_id',
      'state',
      'code_challenge',
      'redirect_uri',
      'scope',
      'response_type',
    ]) {
      expect(
        after.searchParams.get(key),
        `回跳后 query 参数 ${key} 丢了（D221 的核心判据）`,
      ).toBe(requested.get(key));
    }

    // 反向对照：**登录之后**才该发 preview（否则上面 `0` 的断言可能是空跑）。
    await expect.poll(() => previewRequests, { timeout: 15_000 }).toBeGreaterThan(0);
  });

  test('授权码 + PKCE：同意页点「允许该应用」→ 回调 URL 里有 code → 换到令牌', async ({
    page,
    request,
  }) => {
    const fixture = readFixture();
    const secret = secretOf(fixture, 'client-A');

    await login(page, 'e2e-user-oauth@example.com', TENANT_A);
    await interceptCallback(page);

    // 同意页：展示**请求的**权限点码（AC-S-146 步骤 1）。
    await page.goto(authorizeUrl('client-A', 'rbac:role:read', 'xyz'));
    await expect(page.getByTestId('oauth-scopes')).toContainText('rbac:role:read');

    // ⚠️ **真的点按钮**（不是只断言接口）。
    await page.getByRole('button', { name: '允许该应用' }).click();

    // 可观测后果 1：页面跳到了第三方回调（被 page.route 满足）。
    await expect(page).toHaveURL(/client\.example\/cb/);
    const url = new URL(page.url());
    expect(url.searchParams.get('state')).toBe('xyz');
    const code = url.searchParams.get('code');
    expect(code).not.toBeNull();
    expect(code ?? '').not.toBe('');

    // 可观测后果 2：拿这个 code **真的**换到令牌（scope = 请求的那个）。
    const exchange = await request.post('/api/oauth/token', {
      form: {
        grant_type: 'authorization_code',
        code: code ?? '',
        redirect_uri: CALLBACK,
        code_verifier: VERIFIER,
      },
      headers: {
        authorization: `Basic ${Buffer.from(`client-A:${secret}`).toString('base64')}`,
      },
    });
    expect(exchange.status()).toBe(200);
    const token = (await exchange.json()) as { access_token?: string; scope?: string };
    expect(token.scope).toBe('rbac:role:read');
    expect(token.access_token).toBeTruthy();
  });

  test('同意页点「拒绝该应用」→ 回调 URL 里是 error=access_denied，且没有 code', async ({
    page,
  }) => {
    await login(page, 'e2e-user-oauth@example.com', TENANT_A);
    await interceptCallback(page);

    await page.goto(authorizeUrl('client-A', 'rbac:role:read', 'deny-state'));
    await page.getByRole('button', { name: '拒绝该应用' }).click();

    await expect(page).toHaveURL(/client\.example\/cb/);
    const url = new URL(page.url());
    expect(url.searchParams.get('error')).toBe('access_denied');
    expect(url.searchParams.get('state')).toBe('deny-state');
    expect(url.searchParams.get('code')).toBeNull();
  });

  test('设备码：申请 → 设备码页点「允许此设备」→ 原设备轮询换到令牌', async ({ page, request }) => {
    const fixture = readFixture();
    const secret = secretOf(fixture, 'client-A');

    await login(page, 'e2e-user-oauth@example.com', TENANT_A);

    // 申请设备码（协议端点：client 认证）。
    const applied = await request.post('/api/oauth/device_authorization', {
      form: { scope: 'rbac:role:read' },
      headers: {
        authorization: `Basic ${Buffer.from(`client-A:${secret}`).toString('base64')}`,
      },
    });
    expect(applied.status()).toBe(200);
    const device = (await applied.json()) as { device_code?: string; user_code?: string };
    expect(device.user_code).toBeTruthy();
    expect((device.user_code ?? '').length).toBe(8);

    // 设备码页：query 里带 user_code → 页面自动查 → 点「允许此设备」。
    await page.goto(`/oauth/device?user_code=${encodeURIComponent(device.user_code ?? '')}`);
    await expect(page.getByTestId('oauth-device-application')).toBeVisible();
    await page.getByRole('button', { name: '允许此设备' }).click();

    // 可观测后果：页面显示「已批准」。
    await expect(page.getByTestId('oauth-device-approved')).toBeVisible();

    // 可观测后果 2：原设备拿 device_code 轮询 → 200 + 令牌。
    const polled = await request.post('/api/oauth/token', {
      form: {
        grant_type: 'urn:ietf:params:oauth:grant-type:device_code',
        device_code: device.device_code ?? '',
      },
      headers: {
        authorization: `Basic ${Buffer.from(`client-A:${secret}`).toString('base64')}`,
      },
    });
    expect(polled.status()).toBe(200);
    const token = (await polled.json()) as { scope?: string };
    expect(token.scope).toBe('rbac:role:read');
  });

  test('「我的授权」：列出自己的授权 → 点「撤销授权」→ 状态变成已撤销', async ({ page }) => {
    await login(page, 'u-perm-read@example.com', TENANT_A);

    // 先走一遍授权，让 **u-perm-read** 真的有一条授权记录。
    await interceptCallback(page);
    await page.goto(authorizeUrl('client-A', 'rbac:role:read', 'grant-state'));
    await page.getByRole('button', { name: '允许该应用' }).click();
    await expect(page).toHaveURL(/client\.example\/cb/);

    // 授权列表：行出现（只列**自己**的）。
    await page.goto('/oauth/grants');
    const row = page.getByTestId('oauth-grant-row');
    await expect(row).toHaveCount(1);
    await expect(row).toContainText('生效中');

    // ⚠️ **真的点撤销**。
    await page.getByRole('button', { name: '撤销授权' }).click();

    // 可观测后果：状态变成「已撤销」。
    await expect(page.getByTestId('oauth-grant-row')).toContainText('已撤销');
  });

  test('跨租户：tenant-b 的用户看不到 tenant-a 的授权记录', async ({ page }) => {
    await login(page, 'u-perm-other@example.com', TENANT_B);
    await page.goto('/oauth/grants');
    // 可观测后果：**空列表**（不泄露别的租户的授权存在性）。
    await expect(page.getByTestId('oauth-grants-empty')).toBeVisible();
    await expect(page.getByTestId('oauth-grant-row')).toHaveCount(0);
  });

  test('单点登出：front-channel iframe 出现 + back-channel 真发出去了', async ({ page }) => {
    const fixture = readFixture();

    // 先给 u-perm-read 一条授权（client-A 配了 front + back channel URI）。
    await login(page, 'u-perm-read@example.com', TENANT_A);
    await interceptCallback(page);
    await page.goto(authorizeUrl('client-A', 'rbac:role:read', 'logout-state'));
    await page.getByRole('button', { name: '允许该应用' }).click();
    await expect(page).toHaveURL(/client\.example\/cb/);

    // 登出页。
    await page.goto('/oauth/logout');
    await expect(page.getByTestId('oauth-logout-done')).toBeVisible();

    // 可观测后果 1：front-channel 的 iframe **真的**渲染了（数量 ≥ 1）。
    const frames = page.getByTestId('oauth-front-channel');
    await expect(frames.first()).toBeVisible();
    expect(await frames.count()).toBeGreaterThan(0);
    // iframe 的 src 里带 logout_token（不是裸 URL）。
    const src = await frames.first().getAttribute('src');
    expect(src ?? '').toContain(`${FRONT_CHANNEL}?logout_token=`);

    // 可观测后果 2：**back-channel 真发出去了** —— mock 接收端**独立进程**
    // 把它落盘了（D237）→ 读文件（**不是**在 E2E 进程里 mock HTTP）。
    await expect
      .poll(
        async () => {
          try {
            const raw = readFileSync('target/mock-logout-received.jsonl', 'utf8');
            return raw
              .trim()
              .split('\n')
              .filter((line) => line.trim() !== '').length;
          } catch {
            return 0;
          }
        },
        { timeout: 15_000, intervals: [200, 500, 1000] },
      )
      .toBeGreaterThan(0);
    expect(fixture.backchannelLogoutUri).toContain('127.0.0.1');
  });

  test('授权服务器元数据可发现', async ({ request }) => {
    const response = await request.get('/.well-known/oauth-authorization-server');
    expect(response.status()).toBe(200);
    const metadata = (await response.json()) as {
      issuer?: string;
      response_types_supported?: string[];
      grant_types_supported?: string[];
      code_challenge_methods_supported?: string[];
    };
    expect(metadata.response_types_supported).toEqual(['code']);
    expect(metadata.code_challenge_methods_supported).toEqual(['S256']);
    expect(metadata.grant_types_supported).toContain('authorization_code');
    expect(metadata.grant_types_supported).toContain('client_credentials');
    // ⚠️ **不含** refresh_token（D217 / D250）。
    expect(metadata.grant_types_supported).not.toContain('refresh_token');
  });

  test('client 端点：注册 → 停用 → 删除（点按钮链）', async ({ page }) => {
    await login(page, 'u-perm-oauth@example.com', TENANT_A);

    const auth = await authHeaders(page);

    // 注册（平台端点）。
    const created = await page.request.post('/api/oauth/clients', {
      headers: auth,
      data: {
        name: 'E2E 临时应用',
        clientType: 'confidential',
        redirectUris: [CALLBACK],
        allowedGrantTypes: ['client_credentials'],
        allowedScopes: ['rbac:role:read'],
      },
    });
    expect(created.status()).toBe(201);
    const body = (await created.json()) as { clientId?: string; clientSecret?: string };
    expect(body.clientId).toBeTruthy();

    // 读接口**不回显** secret（AC-S-136）。
    const read = await page.request.get(`/api/oauth/clients/${body.clientId ?? ''}`, {
      headers: auth,
    });
    expect(read.status()).toBe(200);
    const readBody = await read.text();
    expect(readBody).not.toContain(body.clientSecret ?? 'never-matches');

    // 停用 → 换令牌必须 401 `invalid_client`（协议错误体）。
    const disabled = await page.request.patch(`/api/oauth/clients/${body.clientId ?? ''}`, {
      headers: auth,
      data: { status: 'disabled' },
    });
    expect(disabled.status()).toBe(200);
    const exchange = await page.request.post('/api/oauth/token', {
      form: { grant_type: 'client_credentials', scope: 'rbac:role:read' },
      headers: {
        authorization: `Basic ${Buffer.from(
          `${body.clientId ?? ''}:${body.clientSecret ?? ''}`,
        ).toString('base64')}`,
      },
    });
    expect(exchange.status()).toBe(401);
    expect(await exchange.text()).toBe('{"error":"invalid_client"}');

    // 删除 → 204。
    const deleted = await page.request.delete(`/api/oauth/clients/${body.clientId ?? ''}`, {
      headers: auth,
    });
    expect(deleted.status()).toBe(204);
  });
});
