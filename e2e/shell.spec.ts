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

  // 主色来自配置：不得回落到 `packages/theme` 的兜底默认 `#1677ff`。
  const color = await page
    .getByRole('heading', { name: title })
    .evaluate((element) => getComputedStyle(element).color);
  expect(color).not.toBe('rgb(22, 119, 255)');
});

test('shell 渲染并可过滤菜单', async ({ page }) => {
  await page.goto('/');
  await expect(page.getByRole('heading')).toBeVisible();

  await page.getByPlaceholder('关键字').fill('租户');
  await expect(page.getByRole('link', { name: '租户' })).toBeVisible();
  await expect(page.getByRole('link', { name: '工作台' })).toBeHidden();
});
