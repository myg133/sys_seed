import { expect, test } from '@playwright/test';

test('底座壳渲染并可过滤菜单', async ({ page }) => {
  await page.goto('/');
  await expect(page.getByRole('heading', { name: '底座壳' })).toBeVisible();

  await page.getByPlaceholder('关键字').fill('租户');
  await expect(page.getByRole('link', { name: '租户' })).toBeVisible();
  await expect(page.getByRole('link', { name: '工作台' })).toBeHidden();
});
