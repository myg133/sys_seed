import { svelte } from '@sveltejs/vite-plugin-svelte';
import { fileURLToPath } from 'node:url';
import { defineConfig } from 'vite';

/** 指向 workspace 包的源码，开发时无需先构建。 */
const workspacePackage = (name: string): string =>
  fileURLToPath(new URL(`../../packages/${name}/src/index.ts`, import.meta.url));

export default defineConfig({
  plugins: [svelte()],
  resolve: {
    alias: {
      '@base/auth': workspacePackage('auth'),
      '@base/hooks': workspacePackage('hooks'),
      '@base/i18n': workspacePackage('i18n'),
      '@base/schema': workspacePackage('schema'),
      '@base/sdk': workspacePackage('sdk'),
      '@base/theme': workspacePackage('theme'),
      '@base/ui': workspacePackage('ui'),
    },
  },
  server: {
    host: '127.0.0.1',
    port: 5173,
    strictPort: true,
    // 开发形态同源：页面在 5173、接口在 8080 —— 用 Vite 代理而不是 CORS
    // （本仓没有 CORS 层；浏览器只看到同源请求）。
    proxy: {
      '/api': 'http://127.0.0.1:8080',
    },
  },
  preview: {
    host: '127.0.0.1',
    port: 4173,
    strictPort: true,
    proxy: {
      '/api': 'http://127.0.0.1:8080',
    },
  },
});
