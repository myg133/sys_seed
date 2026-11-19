import { expect, test } from '@playwright/test';

/**
 * shell 的标题 / `<h1>` / 主色**从品牌接口读取**（REQ-010 / AC-S-055）。
 *
 * 本用例走 `platform-server` 静态目录的同源访问（`scripts/e2e-server.sh`）——
 * 本仓没有 CORS 层，跨源 `fetch` 会被浏览器拦掉，标题就永远不会变。
 */
test('shell 从品牌接口读取标题并渲染（同源）', async ({ page }) => {
  await page.goto('/');

  // 以运行时接口的返回值为准（不写死具体品牌值）。
  const response = await page.request.get('/api/config/branding');
  expect(response.ok()).toBeTruthy();
  const branding = (await response.json()) as { title?: string; primaryColor?: string };
  const title = typeof branding.title === 'string' ? branding.title : '';
  expect(title.length).toBeGreaterThan(0);

  await expect(page).toHaveTitle(title);
  await expect(page.getByRole('heading', { name: title })).toBeVisible();

  // 主色来自配置：**断言等于**由接口 `primaryColor` 换算出的 rgb。
  //
  // ⚠️ 这里曾经写的是 `expect(color).not.toBe('rgb(22, 119, 255)')` ——
  // **否定式断言**。若 `primaryColor` 完全没被应用（元素用默认黑色），
  // 或 `app.css` 不再使用 `var(--base-primary)`，计算色是任意非默认值，
  // 该断言**仍然成立** → E2E 假绿（REQ-010 QA 判 MEDIUM，D83）。
  //
  // 断言「等于期望值」才能守住回归。
  const primaryColor = typeof branding.primaryColor === 'string' ? branding.primaryColor : '';
  expect(primaryColor).toMatch(/^#[0-9a-fA-F]{6}$/);
  const expected = hexToRgb(primaryColor);
  const color = await page
    .getByRole('heading', { name: title })
    .evaluate((element) => getComputedStyle(element).color);
  expect(color).toBe(expected);
});

/** `#0055aa` → `rgb(0, 85, 170)`（与 `getComputedStyle().color` 的格式一致）。 */
function hexToRgb(hex: string): string {
  const value = hex.replace('#', '');
  const r = Number.parseInt(value.slice(0, 2), 16);
  const g = Number.parseInt(value.slice(2, 4), 16);
  const b = Number.parseInt(value.slice(4, 6), 16);
  return `rgb(${r}, ${g}, ${b})`;
}

test('shell 渲染并可过滤菜单', async ({ page }) => {
  await page.goto('/');
  await expect(page.getByRole('heading')).toBeVisible();

  // 本用例验证的是**中文界面下**的菜单过滤；首屏语言来自 `navigator.language`
  // （REQ-021 / AC-S-099），这里显式切到 zh-CN，不依赖运行环境的浏览器 locale。
  await page.locator('[data-testid="language-switcher"]').selectOption('zh-CN');

  await page.getByPlaceholder('关键字').fill('租户');
  await expect(page.getByRole('link', { name: '租户管理' })).toBeVisible();
  await expect(page.getByRole('link', { name: '工作台' })).toBeHidden();
});
