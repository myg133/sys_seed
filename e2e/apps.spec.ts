import { expect, test, type APIRequestContext, type Page } from '@playwright/test';

/**
 * 应用接入（REQ-016）的 **E2E**：工作台 / 导航 / 注册表 / 守卫（AC-S-156 · 160 · 162 · 163）。
 *
 * # 夹具（都由 `scripts/seed-e2e.sh` 播种 —— D101 / D279）
 *
 * | 应用 | client | category | sortOrder | displayName | launch_url | status |
 * |---|---|---|---|---|---|---|
 * | A | `client-app-a` | finance | 20 | `E2E 差旅报销` | `https://a.example/` | active |
 * | B | `client-app-b` | finance | 10 | `E2E 费用申请` | `https://b.example/` | active |
 * | C | `client-app-c` | general | 0 | `E2E C 应用` | `https://c.example/` | active |
 * | D | `client-app-d` | general | 0 | `E2E D 应用` | `https://d.example/` | active |
 * | E | `client-app-e` | general | 0 | `E2E E 应用` | `https://e.example/` | **hidden** |
 *
 * 账号（租户 `tenant-a`，密码 `Tr0ub4dor&3xKq`）：
 * - `u-perm-read@example.com` —— **不持** `app:registry:*`、不持 `audit:events:read`
 *   （但持 `oauth:grant:read` / `sandbox:demo:read`，所以导航里能看到那两项）；
 * - `u-perm-audit@example.com` —— 持 `audit:events:read`（导航里应出现审计项）；
 * - `u-perm-app@example.com` —— 持 `app:registry:read/write/delete`。
 *
 * # ⚠️ 播种里**没有** `launch_url` 为空的应用，也没有 `https://expense.example/**`
 *
 * 派单要求「工作台不按 `launch_url` 过滤」（D279）与「`external` 项整页跳」这两条
 * 必须各有一个**能观测到差异**的样本 → 本 spec 用 API 现造、跑完即删
 * （见 `createTempApp` / `deleteTempApp`）。**不自造样本就只能写成永远通过的断言**。
 *
 * # ⚠️ `launch_url` / `client_id` 的唯一约束
 *
 * `platform_app` 上有 `(tenant_id, client_id)` 与 `(tenant_id, display_name)` 两个唯一索引
 * → 临时应用必须用**未被占用的 client**（`client-A` / `client-B` / `public-client` 三个
 * 播种的 client 都没被任何应用引用）与**独占的 displayName**，并在跑完删除。
 * 每个用例前还会按 displayName 先清一次残留（上次失败留下的行会让本用例假红）。
 *
 * # 0 skipped（D108）
 *
 * 夹具缺失 / 被测系统没起来 → **必须 fail**。本 spec 里**没有** `test.skip`。
 */

const TENANT = 'tenant-a';
const PASSWORD = 'Tr0ub4dor&3xKq';

/** 会话存储键（与 `src/lib/session.ts` 的 `SESSION_STORAGE_KEY` 一致）。 */
const SESSION_KEY = 'base.shell.session.v1';

/** 只读账号（登录即可，但没有 `app:registry:*`）。 */
const READER = 'u-perm-read@example.com';
/** 持 `audit:events:read` 的账号（导航里应有审计项）。 */
const AUDITOR = 'u-perm-audit@example.com';
/** 应用注册表管理员（`app:registry:read/write/delete`）。 */
const APP_ADMIN = 'u-perm-app@example.com';

/** 工作台路由（REQ-016 新增；`/` 仍是既有首页）。 */
const WORKBENCH = '/workbench';
/** 应用注册表管理页。 */
const APPS_ADMIN = '/admin/apps';

/** 播种的应用展示名（**逐字** —— 去 `scripts/seed-e2e.sh` 核过）。 */
const SEEDED = {
  travel: 'E2E 差旅报销',
  expense: 'E2E 费用申请',
  c: 'E2E C 应用',
  d: 'E2E D 应用',
  e: 'E2E E 应用',
} as const;

/** 临时应用的展示名（每个用例独占；跑完删除）。 */
const TEMP_NO_LAUNCH = 'E2E 临时无入口应用';
const TEMP_EXTERNAL = 'E2E 临时外部应用';
const TEMP_REGISTRY = 'E2E 临时注册表应用';

/** `external` 用例的入口地址（AC-S-163 步骤 3 逐字要求的目标）。 */
const EXTERNAL_URL = 'https://expense.example/app';

/* -------------------------------------------------------------------------- */
/* 小工具                                                                      */
/* -------------------------------------------------------------------------- */

/** 当前地址的 pathname（不比较 origin / query）。 */
function pathOf(page: Page): string {
  return new URL(page.url()).pathname;
}

/**
 * 登录请求固定带的来源 IP。
 *
 * # ⚠️ 为什么必须带（不是可选项）
 *
 * 登录会写审计事件 `iam.login.succeeded`（`object_type='iam_session'`、`object_id` 为空）。
 * 平台中间件把 `X-Forwarded-For` 的**第一跳**填进事件的 `ip`（D149）——
 * 不带这个头时 `ip` 是**空串**，而 `object` 列（`auditObjectLabel(objectType, objectId)`）
 * 同样因为 `object_id` 为空而渲染成空串 → 这条事件在审计页上**有五格中的两格是空的**。
 *
 * 这条事件是**最新**记录 → 会占据审计页首行 → 把既有
 * `e2e/audit.spec.ts` 的「首行五格都非空」（TC-014-112）**打红**（实测：全量跑时
 * 49 passed / 1 failed，失败的就是它）。
 *
 * → 本 spec 的**每一次**登录都必须带这个头（与 `audit.spec.ts` 同款），
 * 否则我就是在给别人的用例制造红。
 */
const FORWARDED_FOR = '203.0.113.9';

/** 填表并登录（等到外壳启动完成：`已就绪` 只在首页渲染）。 */
async function login(page: Page, email: string): Promise<void> {
  // 见 `FORWARDED_FOR` 的说明：不带这个头会污染审计页首行的 `ip`。
  await page.setExtraHTTPHeaders({ 'x-forwarded-for': FORWARDED_FOR });
  await page.goto('/login');
  await page.getByLabel('租户').fill(TENANT);
  await page.getByLabel('用户名').fill(email);
  await page.getByLabel('密码').fill(PASSWORD);
  await page.getByRole('button', { name: '登录' }).click();
  await expect(page.getByText('已就绪')).toBeVisible();
}

/**
 * 用 `request`（**独立于页面会话**的 API 上下文）登录并取 Bearer 令牌。
 *
 * ⚠️ 不能借 `page.request` + 页面会话：本 spec 要让**页面**用只读账号、
 * 同时由**管理员**造数 —— 两个身份必须分开（`page.request` 会共享页面上下文）。
 */
async function apiToken(request: APIRequestContext, email: string): Promise<string> {
  const response = await request.post('/api/iam/login', {
    headers: { 'x-tenant-id': TENANT },
    data: { username: email, password: PASSWORD },
  });
  expect(response.status(), `登录 ${email} 应成功（夹具由 seed-e2e.sh 播种）`).toBe(200);
  const body = (await response.json()) as { accessToken?: string };
  const token = body.accessToken ?? '';
  expect(token, `${email} 的登录响应里应有 accessToken`).not.toBe('');
  return token;
}

/** 造一个临时应用（**201**）；返回 `appId`。 */
async function createTempApp(
  request: APIRequestContext,
  token: string,
  data: Record<string, unknown>,
): Promise<string> {
  const response = await request.post('/api/apps', {
    headers: { authorization: `Bearer ${token}`, 'x-tenant-id': TENANT },
    data,
  });
  expect(response.status(), `创建应用应返回 201：${await response.text()}`).toBe(201);
  const body = (await response.json()) as { appId?: string };
  const appId = body.appId ?? '';
  expect(appId, '创建响应里应有 appId').not.toBe('');
  return appId;
}

/**
 * 取一个**当前没有被任何应用引用**的 client（`platform_app` 上有
 * `(tenant_id, client_id)` 唯一索引 → 同一 client 只能接一个应用）。
 *
 * ⚠️ 不能硬编码 `client-A`：集成测试（`crates/platform-app` / `platform-server`
 * 的真实库用例）也往**同一个** `base_test` 里播种 `client-A`/`client-B` 上的应用 ——
 * 硬编码会让本 spec 在「跑过集成测试之后」得到 `APP_DUPLICATED`（假红）。
 */
async function pickFreeClient(request: APIRequestContext, token: string): Promise<string> {
  const list = await request.get('/api/apps', {
    headers: { authorization: `Bearer ${token}`, 'x-tenant-id': TENANT },
  });
  expect(list.status(), '管理员应能列出应用（`GET /api/apps`）').toBe(200);
  const apps = (await list.json()) as { clientId: string }[];
  const bound = new Set(apps.map((app) => app.clientId));

  // 播种的 3 个 client 都是 E2E 夹具（REQ-022/016 共有）；挑第一个没被引用的。
  const candidates = ['public-client', 'client-B', 'client-A', 'client-app-c', 'client-app-d'];
  const free = candidates.find((clientId) => !bound.has(clientId));
  expect(free, `找不到空闲 client（已被引用的：${[...bound].join(', ')}）`).toBeDefined();
  return free ?? '';
}

/** 按展示名删掉残留的同名应用（幂等；上次失败的用例可能留下它）。 */
async function purgeByDisplayName(
  request: APIRequestContext,
  token: string,
  displayName: string,
): Promise<void> {
  const list = await request.get('/api/apps', {
    headers: { authorization: `Bearer ${token}`, 'x-tenant-id': TENANT },
  });
  expect(list.status(), '管理员应能列出应用（`GET /api/apps`）').toBe(200);
  const apps = (await list.json()) as { appId: string; displayName: string }[];
  for (const app of apps) {
    if (app.displayName !== displayName) {
      continue;
    }
    const removed = await request.delete(`/api/apps/${app.appId}`, {
      headers: { authorization: `Bearer ${token}`, 'x-tenant-id': TENANT },
    });
    expect(removed.status(), `删除残留应用 ${app.appId} 应返回 204`).toBe(204);
  }
}

/** 删除一个应用（**204**；不存在也当成功 —— 清理不该盖掉真正的失败）。 */
async function deleteTempApp(
  request: APIRequestContext,
  token: string,
  appId: string | undefined,
): Promise<void> {
  if (appId === undefined || appId === '') {
    return;
  }
  await request.delete(`/api/apps/${appId}`, {
    headers: { authorization: `Bearer ${token}`, 'x-tenant-id': TENANT },
  });
}

/**
 * 在**页面**里装 `history.pushState` 计数器，计数经 `window.name` **跨整页跳转**保留。
 *
 * # 为什么必须跨文档计数
 *
 * AC-S-163 步骤 3 的判据是「**没有**触发 SPA 内部导航」。`pushState` 计数器若只存在
 * 当前文档的变量里，整页跳转后**读到的是新文档的 0** —— 那证明不了任何事（整页跳转
 * 天然会归零）。`window.name` 是唯一跨文档保留的字符串槽（同源/跨源都保留，
 * 由浏览器规范保证；本产品**没有**用过它 —— 见 `grep window.name apps/shell/src`）。
 */
function installPushStateCounter(): void {
  const read = (): Record<string, unknown> => {
    try {
      const parsed: unknown = JSON.parse(window.name === '' ? '{}' : window.name);
      return typeof parsed === 'object' && parsed !== null
        ? (parsed as Record<string, unknown>)
        : {};
    } catch {
      return {};
    }
  };
  const state = read();
  if (typeof state['appPushState'] !== 'number') {
    state['appPushState'] = 0;
  }
  window.name = JSON.stringify(state);

  const original = window.history.pushState.bind(window.history);
  window.history.pushState = (...args: Parameters<History['pushState']>): void => {
    const current = read();
    const count = typeof current['appPushState'] === 'number' ? current['appPushState'] : 0;
    current['appPushState'] = count + 1;
    window.name = JSON.stringify(current);
    original(...args);
  };
}

/** 读当前 `pushState` 计数（跨文档累计）。 */
async function pushStateCount(page: Page): Promise<number> {
  return page.evaluate(() => {
    try {
      const parsed: unknown = JSON.parse(window.name === '' ? '{}' : window.name);
      const value =
        typeof parsed === 'object' && parsed !== null
          ? (parsed as Record<string, unknown>)['appPushState']
          : undefined;
      return typeof value === 'number' ? value : 0;
    } catch {
      return 0;
    }
  });
}

/* -------------------------------------------------------------------------- */
/* 1. 工作台：分组 + 组内排序 + hidden 过滤（AC-S-160）                        */
/* -------------------------------------------------------------------------- */

test('工作台：finance 在 general 前、组内按 (sortOrder, displayName)，hidden 应用不出现', async ({
  page,
  request,
}) => {
  // 先清掉本 spec 临时造的残留（它们若在 general 组里会让「组内 C→D」假红）。
  const token = await apiToken(request, APP_ADMIN);
  for (const name of [TEMP_NO_LAUNCH, TEMP_EXTERNAL, TEMP_REGISTRY]) {
    await purgeByDisplayName(request, token, name);
  }

  // 工作台**登录即可** —— 用只读账号（**不持** `app:registry:*`）。
  await login(page, READER);
  await page.goto(WORKBENCH);

  const groups = page.getByTestId('app-workspace-group');
  // 先等首屏渲染（`allInnerTexts()` 不等待 —— 立刻读会拿到空数组）。
  await expect(groups.first()).toBeVisible();
  const categories = await groups.getByTestId('app-workspace-category').allInnerTexts();

  // 判据 1：组**按 category 字典序升序**（确定性分组顺序）。
  // ⚠️ 只断「2 个组」不够稳：`base_test` 是**共享**库，别的集成测试会往里播种
  //    自己的应用（实测 `crates/platform-app/tests` 会写 `created_by='fixture'`
  //    的应用）→ 组数会随环境变化。这里断的是**实现性质**（有序），不是库的内容快照。
  expect(categories).toEqual([...categories].sort());

  // 判据 2：finance **排在** general 之前。
  const financeIndex = categories.indexOf('finance');
  const generalIndex = categories.indexOf('general');
  expect(financeIndex, `组里应有 finance（实际：${categories.join(', ')}）`).toBeGreaterThanOrEqual(0);
  expect(generalIndex, `组里应有 general（实际：${categories.join(', ')}）`).toBeGreaterThanOrEqual(0);
  expect(financeIndex).toBeLessThan(generalIndex);

  /** 取某组内**属于本 spec 夹具**的卡片名（忽略别的测试播种的应用）。 */
  async function seededNames(category: string): Promise<string[]> {
    const all = await groups
      .filter({ has: page.getByTestId('app-workspace-category').filter({ hasText: category }) })
      .locator('.base-workbench-card-name')
      .allInnerTexts();
    const wanted = new Set<string>(Object.values(SEEDED));
    return all.filter((name) => wanted.has(name));
  }

  // 判据 3：finance 组内 B(10) → A(20) —— 按 sortOrder 数值序，不是 displayName。
  expect(await seededNames('finance')).toEqual([SEEDED.expense, SEEDED.travel]);

  // 判据 4：general 组内 C → D —— sortOrder 并列(0) → 按 displayName 升序。
  // ⚠️ 播种 deliberately 让 id 顺序与 displayName 相反（C 的 id 是 `e2e-app-z`）：
  //    按 id 兜底排序会得到 D → C，只有按 displayName 才过。
  expect(await seededNames('general')).toEqual([SEEDED.c, SEEDED.d]);

  // 判据 5：hidden 的 E **不在页面上任何位置**（卡片 / 导航都不该有）。
  await expect(page.getByText(SEEDED.e, { exact: true })).toHaveCount(0);

  // 判据 6：4 个 active 夹具**各自恰好出现一次**（既不多也不少 —— E 被过滤）。
  for (const name of [SEEDED.travel, SEEDED.expense, SEEDED.c, SEEDED.d]) {
    await expect(page.locator('.base-workbench-card-name', { hasText: name })).toHaveCount(1);
  }
});

/* -------------------------------------------------------------------------- */
/* 2. 工作台不按 launch_url 过滤（D279）—— 没入口的应用**仍显示**、但点不开   */
/* -------------------------------------------------------------------------- */

test('工作台：launch_url 为空的应应用仍显示卡片，但没有可点入口（D279）', async ({
  page,
  request,
}) => {
  const token = await apiToken(request, APP_ADMIN);
  await purgeByDisplayName(request, token, TEMP_NO_LAUNCH);

  // 造样本：`POST /api/apps` **不给** `launchUrl`（播种里没有这样的应用）。
  let appId: string | undefined;
  try {
    appId = await createTempApp(request, token, {
      clientId: await pickFreeClient(request, token),
      displayName: TEMP_NO_LAUNCH,
      description: 'D279：没有入口地址，但工作台必须显示它',
      category: 'general',
      sortOrder: 90,
    });

    await login(page, READER);
    await page.goto(WORKBENCH);

    const card = page.getByTestId('app-workspace-card').filter({ hasText: TEMP_NO_LAUNCH });
    // 判据 1：卡片**在**（工作台是「接入了哪些应用」的总览，不看 launch_url）。
    await expect(card).toHaveCount(1);

    // 判据 2：**没有可点入口** —— 卡片里没有 `<a href>`，而是「未配置入口地址」提示。
    await expect(card.getByTestId('app-workspace-open')).toHaveCount(0);
    await expect(card.getByTestId('app-workspace-no-entry')).toBeVisible();
    await expect(card.locator('a')).toHaveCount(0);

    // 判据 3：点它**不会**跳走（可观测后果：地址仍是工作台）。
    await card.locator('.base-workbench-card-name').click();
    await expect(page).toHaveURL(new RegExp(`${WORKBENCH}$`));
    expect(pathOf(page)).toBe(WORKBENCH);
  } finally {
    await deleteTempApp(request, token, appId);
  }
});

/* -------------------------------------------------------------------------- */
/* 3. 导航：external 项点击 → **整页跳转**（AC-S-163 步骤 3）                  */
/* -------------------------------------------------------------------------- */

test('导航：external 应用项点击 → 整页跳到 launch_url，且未触发 SPA 内部导航', async ({
  page,
  request,
}) => {
  const token = await apiToken(request, APP_ADMIN);
  await purgeByDisplayName(request, token, TEMP_EXTERNAL);

  // 播种里没有指向 `https://expense.example/app` 的应用 → 现造一个（AC 逐字要求的目标）。
  let appId: string | undefined;
  try {
    appId = await createTempApp(request, token, {
      clientId: await pickFreeClient(request, token),
      displayName: TEMP_EXTERNAL,
      description: 'AC-S-163：external 项的整页跳转目标',
      category: 'general',
      launchUrl: EXTERNAL_URL,
      sortOrder: 91,
    });

    // 目标域名不存在 → 必须就地拦截满足（否则 `location.assign` 打到黑洞，
    // `waitForURL` 只能看到 `chrome-error://`）。
    await page.route('https://expense.example/**', (route) =>
      route.fulfill({
        status: 200,
        contentType: 'text/html; charset=utf-8',
        body: '<html><body data-testid="stub">stub-ok</body></html>',
      }),
    );
    // 跨整页跳转的 pushState 计数器（见 `installPushStateCounter` 的说明）。
    await page.addInitScript(installPushStateCounter);

    await login(page, READER);
    await page.goto(WORKBENCH);

    const link = page
      .getByTestId('app-navigation')
      .getByTestId('app-nav-external')
      .filter({ hasText: TEMP_EXTERNAL });
    await expect(link).toHaveCount(1);

    // 跳转前的基线：登录后 SPA 自己可能 pushState 过（例如落到 `/`），所以比**增量**。
    const before = await pushStateCount(page);

    await Promise.all([page.waitForURL(EXTERNAL_URL), link.click()]);

    // 判据 1：地址**逐字**等于 launch_url（整页替换，不是 SPA 路由改写）。
    await expect(page).toHaveURL(EXTERNAL_URL);
    expect(page.url()).toBe(EXTERNAL_URL);

    // 判据 2：跳转后是**桩页**（证明真发生了顶层文档导航，而不是只改了地址栏）。
    await expect(page.getByTestId('stub')).toHaveText('stub-ok');

    // 判据 3：pushState **没有**增加 —— 走的是 `location.assign`（整页跳），
    // 不是 `router.navigate`。若有人把 external 项塞进 SPA 路由表，这条会红。
    expect(await pushStateCount(page)).toBe(before);
  } finally {
    await deleteTempApp(request, token, appId);
  }
});

/* -------------------------------------------------------------------------- */
/* 4. 导航：按权限点过滤（AC-S-162 步骤 2 的对照面）                           */
/* -------------------------------------------------------------------------- */

test('导航：audit 项随 audit:events:read 出现/消失，应用项两边都在', async ({ page }) => {
  // 判据 1：持 `audit:events:read` 的账号 → 导航里有审计项。
  await login(page, AUDITOR);
  await page.goto(WORKBENCH);

  const auditorNav = page.getByTestId('app-navigation');
  await expect(auditorNav.getByTestId('app-nav-internal').filter({ hasText: '审计日志' })).toHaveCount(
    1,
  );
  // 判据 2：应用项**同时在**（应用不是权限点 —— `requiredPermission` 恒为 null）。
  await expect(
    auditorNav.getByTestId('app-nav-external').filter({ hasText: SEEDED.travel }),
  ).toHaveCount(1);

  // 判据 3：**不持** `audit:events:read` 的账号 → 同一页面上审计项**消失**。
  await login(page, READER);
  await page.goto(WORKBENCH);

  const readerNav = page.getByTestId('app-navigation');
  await expect(readerNav.getByTestId('app-nav-internal').filter({ hasText: '审计日志' })).toHaveCount(
    0,
  );
  // 判据 4：应用项**仍在**（与判据 3 形成对照：消失的是权限项，不是整个导航）。
  await expect(
    readerNav.getByTestId('app-nav-external').filter({ hasText: SEEDED.travel }),
  ).toHaveCount(1);
  // 判据 5：`perm` 缺省的工作台项**总是**下发（D269）。
  await expect(readerNav.getByTestId('app-nav-internal').filter({ hasText: '工作台' })).toHaveCount(
    1,
  );
});

/* -------------------------------------------------------------------------- */
/* 5. 应用注册表：注册 / 改名 / 清空 launch_url / 删除（AC-S-156 步骤 1~6）     */
/* -------------------------------------------------------------------------- */

test('应用注册表：注册 → 改名 → 清空入口（专用端点）→ 删除，每步在页面上可观察', async ({
  page,
  request,
}) => {
  // 残留清理用 API（管理员身份，与页面会话分开）。
  const token = await apiToken(request, APP_ADMIN);
  await purgeByDisplayName(request, token, TEMP_REGISTRY);

  // 持 `app:registry:read` → 能进页面（守卫放行 → 路由 `load` 取列表）。
  await login(page, APP_ADMIN);
  await page.goto(APPS_ADMIN);
  await expect(page.getByTestId('app-registry')).toBeVisible();

  // 注册：表单只在有写权限时渲染（`<Can>`）→ 先确认表单在，否则用例前提没达成。
  const form = page.getByTestId('app-registry-form');
  await expect(form).toBeVisible();
  // client 动态挑（同一个 client 只能接一个应用 —— 见 `pickFreeClient` 的说明）。
  const freeClient = await pickFreeClient(request, token);
  await page.getByLabel('客户端标识').fill(freeClient);
  await page.getByLabel('名称').fill(TEMP_REGISTRY);
  await page.getByLabel('分类').fill('general');
  await page.getByLabel('入口地址').fill('https://probe.example/app');
  await page.getByRole('button', { name: '保存' }).click();

  const created = page.getByTestId('app-registry-row').filter({ hasText: TEMP_REGISTRY });
  // 判据 1：新行出现在表里，且入口地址是刚填的那个。
  await expect(created).toHaveCount(1);
  await expect(created.getByTestId('app-registry-launch')).toHaveText('https://probe.example/app');

  // 改名：点该行的「编辑」→ 表单进编辑态（客户端标识应被禁用）→ 改展示名 → 保存。
  const renamed = `${TEMP_REGISTRY}-改`;
  await created.getByRole('button', { name: '编辑' }).click();
  await expect(page.getByLabel('客户端标识')).toBeDisabled();
  await page.getByLabel('名称').fill(renamed);
  await page.getByRole('button', { name: '保存' }).click();

  const renamedRow = page.getByTestId('app-registry-row').filter({ hasText: renamed });
  // 判据 2：改名生效 —— 该行展示名那一格**逐字**是新名字。
  // ⚠️ 不能用「表里只剩 1 行含旧名」：`renamed` 以 `TEMP_REGISTRY` 开头，
  //    子串匹配永远命中（零区分力的假断言）。
  await expect(renamedRow).toHaveCount(1);
  await expect(renamedRow.locator('td').nth(1)).toHaveText(renamed);

  // 清空入口：走**专用端点** `DELETE /api/apps/{id}/launch-url`（D283）→ 204 → 列表刷新。
  const clearResponse = page.waitForResponse(
    (response) =>
      response.request().method() === 'DELETE' &&
      new URL(response.url()).pathname.endsWith('/launch-url'),
  );
  await renamedRow.getByRole('button', { name: '清空入口' }).click();
  expect((await clearResponse).status()).toBe(204);
  // 判据 3：入口列变成空值占位 `-`（**可观测**的页面后果，不只是接口 204）。
  await expect(renamedRow.getByTestId('app-registry-launch')).toHaveText('-');

  // 删除：行消失，且页面上再没有它。
  await renamedRow.getByRole('button', { name: '删除' }).click();
  // 判据 4：该行从表里消失。
  await expect(page.getByTestId('app-registry-row').filter({ hasText: renamed })).toHaveCount(0);

  // 收尾自证：库里也确实没有这一行（避免「只是没刷新」的假绿）。
  const listed = await request.get('/api/apps', {
    headers: { authorization: `Bearer ${token}`, 'x-tenant-id': TENANT },
  });
  expect(listed.status()).toBe(200);
  const remaining = (await listed.json()) as { displayName: string }[];
  expect(remaining.map((app) => app.displayName)).not.toContain(renamed);
});

/* -------------------------------------------------------------------------- */
/* 6. 只读账号进 `/admin/apps` → 403 页，且 `GET /api/apps` **零请求**         */
/* -------------------------------------------------------------------------- */

test('只读账号进 /admin/apps → 落 403 页，且守卫不放行（GET /api/apps 请求数为 0）', async ({
  page,
}) => {
  await login(page, READER);

  // 整页重载一次：确认会话跨重载存活后再走深链（与 `shell-auth.spec.ts` 同款）。
  await page.reload();
  await expect(page.getByText('已就绪')).toBeVisible();

  const listRequests: string[] = [];
  page.on('request', (request) => {
    // **精确**匹配列表端点：`/api/apps/workspace` 等子路径不算。
    if (new URL(request.url()).pathname === '/api/apps') {
      listRequests.push(request.url());
    }
  });

  // 直接输入 URL = 整页加载（不是前端路由跳转）。
  await page.goto(APPS_ADMIN);

  // 判据 1：落 403 页（`resolution.allowed == false` → 页面不渲染）。
  await expect(page.getByTestId('forbidden')).toBeVisible();
  expect(pathOf(page)).toBe('/403');

  // 判据 2：守卫不放行 → 路由的 `load` 不被调用 → 列表接口**一次都没发**。
  expect(listRequests).toHaveLength(0);

  // 判据 3：注册表页本身**没有**渲染（不是「渲染了但空」）。
  await expect(page.getByTestId('app-registry')).toHaveCount(0);
});
