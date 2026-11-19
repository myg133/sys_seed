import { defineConfig, devices } from '@playwright/test';

/**
 * E2E 走 **同源**：`platform-server` 托管 `apps/shell/dist`，`/api/config/branding`
 * 与页面同源（本仓没有 CORS 层；跨源 fetch 会被浏览器拦掉 —— R17）。
 *
 * 前置：
 *   - `DATABASE_URL_TEST`（管理员连接串，**必须是 `base_test`**）；
 *   - 迁移由 `scripts/e2e-server.sh` 用管理员连接跑，serve 用 `base_app`。
 */
const baseURL = process.env.BASE_E2E_URL ?? 'http://127.0.0.1:8080';

export default defineConfig({
  testDir: './e2e',
  timeout: 30_000,
  // 低配机：E2E 串行跑，不开并行
  fullyParallel: false,
  workers: 1,
  reporter: [['list']],
  use: {
    baseURL,
    trace: 'retain-on-failure',
    // REQ-021：首屏语言来自 `navigator.language`（§7.2）。既有 E2E 断言的是
    // **中文界面**（`登录` / `租户` / `关键字`），所以把浏览器 locale 钉在平台默认
    // `zh-CN`，避免「浏览器 en-US → 首屏英文」把既有用例变成假红。
    // 语言切换本身由 `e2e/i18n.spec.ts` 显式验。
    locale: 'zh-CN',
  },
  projects: [
    {
      name: 'chromium',
      use: { ...devices['Desktop Chrome'] },
    },
  ],
  webServer: {
    command: 'pnpm --filter @base/shell build && ./scripts/e2e-server.sh',
    url: `${baseURL}/health/live`,
    reuseExistingServer: true,
    timeout: 240_000,
  },
});
