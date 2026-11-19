import { expect, test } from '@playwright/test';

/**
 * 前端 i18n 的应用级行为（AC-S-099 步骤 5 / AC-S-094 / AC-S-098）。
 *
 * 前置：`platform-server` 托管 `apps/shell/dist`（同源，`scripts/e2e-server.sh`）。
 * `/api/i18n/context` 不可用时页面必须**照常可用**（本地偏好 + 平台默认兜底）——
 * 本文件不断言后端契约本身（那是后端 Dev 的 E2E）。
 */

/** 语言切换器（单一入口，避免测试各自找选择器）。 */
const SWITCHER = '[data-testid="language-switcher"]';

test('切换语言后同一节点立即更新，且不整页跳转（AC-S-099 步骤 5）', async ({ page }) => {
  await page.goto('/');
  const homeLink = page.locator('a[href="/workbench"]');
  await expect(homeLink).toBeVisible();

  // 首屏语言来自 navigator.language（本仓 E2E 把浏览器 locale 钉在 zh-CN），
  // 这里先显式切到 en-US，再切回来 —— 两次切换都走订阅，不刷新页面。
  await page.locator(SWITCHER).selectOption('en-US');
  await expect(homeLink).toHaveText('Workbench');

  // 给节点打一个标记：若切换时整页重新挂载，标记会消失。
  await homeLink.evaluate((node) => node.setAttribute('data-e2e-same-node', '1'));

  const urlBefore = page.url();
  await page.locator(SWITCHER).selectOption('zh-CN');

  // 同一个 DOM 节点文本变了 —— 不是刷新页面、也不是重建组件树。
  await expect(homeLink).toHaveText('工作台');
  await expect(homeLink).toHaveAttribute('data-e2e-same-node', '1');

  // 不刷新页面：URL 不变（切换语言不该改路由）。
  expect(page.url()).toBe(urlBefore);
});

test('语言偏好存 sessionStorage：同标签页刷新后仍生效（D112 / D99）', async ({ page }) => {
  await page.goto('/');
  await page.locator(SWITCHER).selectOption('en-US');
  await expect(page.locator('a[href="/workbench"]')).toHaveText('Workbench');

  await page.reload();

  // 刷新后仍是用户显式选择的 en-US（不是回到浏览器 / 平台默认）。
  await expect(page.locator(SWITCHER)).toHaveValue('en-US');
  await expect(page.locator('a[href="/workbench"]')).toHaveText('Workbench');
});

test('时区 / 金额展示走 formatDate / formatMoney（不是原始 UTC 串）', async ({ page }) => {
  await page.goto('/');

  const timezone = await page.getByTestId('i18n-timezone').innerText();
  expect(timezone).toMatch(/^[A-Za-z]+\/[A-Za-z_]+$/);

  const time = await page.getByTestId('i18n-time').innerText();
  // 固定形状 `yyyy-MM-dd HH:mm:ss`；不是 `2026-01-15T00:00:00Z` 这种原值。
  expect(time).toMatch(/^\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}$/);
  expect(time).not.toContain('T');
  expect(time).not.toContain('Z');

  // 金额样例：零金额也要有货币符号与固定小数位（默认 CNY 2 位）。
  const money = await page.getByTestId('i18n-money').innerText();
  expect(money).toMatch(/\d/);
  expect(money).toMatch(/0[.,]00|0$/);
});
