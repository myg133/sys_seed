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
  },
  preview: {
    host: '127.0.0.1',
    port: 4173,
    strictPort: true,
  },
});
