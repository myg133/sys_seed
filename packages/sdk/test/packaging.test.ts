import { existsSync, readdirSync, readFileSync } from 'node:fs';
import { dirname, join, relative } from 'node:path';
import { fileURLToPath } from 'node:url';

import { describe, expect, it } from 'vitest';

/** 仓库根目录（本文件在 `packages/sdk/test/` 下）。 */
const REPO_ROOT = join(dirname(fileURLToPath(import.meta.url)), '..', '..', '..');

/** 本期涉及的 5 个包。 */
const PACKAGES = ['sdk', 'auth', 'ui', 'theme', 'hooks'] as const;

/** 读包的 manifest。 */
function manifestOf(packageName: string): Record<string, unknown> {
  const raw = readFileSync(join(REPO_ROOT, 'packages', packageName, 'package.json'), 'utf-8');
  return JSON.parse(raw) as Record<string, unknown>;
}

/** 包的内部依赖键集（`dependencies` + `devDependencies`）。 */
function dependencyKeys(packageName: string): string[] {
  const manifest = manifestOf(packageName);
  const dependencies = manifest['dependencies'];
  const devDependencies = manifest['devDependencies'];
  return [
    ...Object.keys((dependencies as Record<string, string> | undefined) ?? {}),
    ...Object.keys((devDependencies as Record<string, string> | undefined) ?? {}),
  ];
}

/** 包的内部依赖（只算 `dependencies`，`devDependencies` 不进产物）。 */
function runtimeDependencyKeys(packageName: string): string[] {
  const manifest = manifestOf(packageName);
  const dependencies = manifest['dependencies'];
  return Object.keys((dependencies as Record<string, string> | undefined) ?? {});
}

/** 递归收集目录下的文件。 */
function collectFiles(dir: string): string[] {
  if (!existsSync(dir)) {
    return [];
  }
  const found: string[] = [];
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const full = join(dir, entry.name);
    if (entry.isDirectory()) {
      found.push(...collectFiles(full));
    } else {
      found.push(full);
    }
  }
  return found;
}

/** 产物里出现的内部包导入目标。 */
function internalImportsOf(packageName: string): string[] {
  const dist = join(REPO_ROOT, 'packages', packageName, 'dist');
  const targets = new Set<string>();
  for (const file of collectFiles(dist)) {
    if (!/\.(js|d\.ts)$/.test(file)) {
      continue;
    }
    const source = readFileSync(file, 'utf-8');
    for (const match of source.matchAll(/from\s+['"](@base\/[^'"]+)['"]/g)) {
      targets.add(match[1] ?? '');
    }
    for (const match of source.matchAll(/import\(\s*['"](@base\/[^'"]+)['"]\s*\)/g)) {
      targets.add(match[1] ?? '');
    }
  }
  return [...targets].sort();
}

/** 是否已经构建过（`dist` 被 gitignore，CI 的 test job 不会先构建）。 */
const built = PACKAGES.every((name) => existsSync(join(REPO_ROOT, 'packages', name, 'dist')));

describe('@base 的依赖方向（AC-S-075）', () => {
  it('TC-011-045：禁止项不存在，且规定项存在', () => {
    const sdk = dependencyKeys('sdk');
    const auth = dependencyKeys('auth');
    const ui = dependencyKeys('ui');
    const hooks = dependencyKeys('hooks');
    const theme = dependencyKeys('theme');

    expect(sdk.filter((key) => ['@base/ui', '@base/theme', '@base/hooks'].includes(key))).toEqual(
      [],
    );
    expect(auth.filter((key) => key === '@base/ui')).toEqual([]);
    expect(ui.filter((key) => key === '@base/hooks')).toEqual([]);
    expect(hooks.filter((key) => key === '@base/ui')).toEqual([]);
    expect(theme).toEqual([]);

    // 规定项必须存在（否则「五个包完全互不相连」也能通过 —— 那是假绿）。
    expect(auth).toContain('@base/sdk');
    expect(ui).toContain('@base/theme');
  });

  it('TC-011-045：内部依赖一律写成 workspace:*', () => {
    for (const name of PACKAGES) {
      const manifest = manifestOf(name);
      const dependencies = (manifest['dependencies'] as Record<string, string> | undefined) ?? {};
      for (const [key, value] of Object.entries(dependencies)) {
        if (key.startsWith('@base/')) {
          expect(`${name}:${key}=${value}`).toBe(`${name}:${key}=workspace:*`);
        }
      }
    }
  });

  it('TC-011-045：内部依赖写在 dependencies（不是 devDependencies）', () => {
    expect(runtimeDependencyKeys('auth')).toContain('@base/sdk');
    expect(runtimeDependencyKeys('ui')).toContain('@base/theme');
    expect(runtimeDependencyKeys('sdk')).toEqual([]);
    expect(runtimeDependencyKeys('theme')).toEqual([]);
    expect(runtimeDependencyKeys('hooks')).toEqual([]);
  });
});

describe.skipIf(!built)('@base 的产物边界（AC-S-075）', () => {
  it('TC-011-046：各包产物只 import 自己的依赖', () => {
    for (const name of PACKAGES) {
      const dist = join(REPO_ROOT, 'packages', name, 'dist');
      expect(existsSync(dist)).toBe(true);
      const jsFiles = collectFiles(dist).filter((file) => file.endsWith('.js'));
      expect(jsFiles.length).toBeGreaterThanOrEqual(1);
    }

    expect(internalImportsOf('sdk')).toEqual([]);
    expect(internalImportsOf('theme')).toEqual([]);
    expect(internalImportsOf('hooks')).toEqual([]);
    expect(internalImportsOf('ui').filter((item) => item !== '@base/theme')).toEqual([]);
    expect(internalImportsOf('auth').filter((item) => item !== '@base/sdk')).toEqual([]);
  });

  it('TC-011-047：@base/sdk 的产物不含 UI / 组件代码，也不引用 DOM API', () => {
    const dist = join(REPO_ROOT, 'packages', 'sdk', 'dist');
    const files = collectFiles(dist);
    expect(files.length).toBeGreaterThanOrEqual(1);

    const importPattern =
      /from\s+['"]@base\/(ui|theme|hooks)['"]|require\(\s*['"]@base\/(ui|theme|hooks)['"]\s*\)/;
    const domPattern = /\bdocument\.|\bwindow\.|\bHTMLElement\b|\bcustomElements\b/;

    const withImports: string[] = [];
    const withDomApi: string[] = [];
    for (const file of files) {
      const source = readFileSync(file, 'utf-8');
      const relativePath = relative(REPO_ROOT, file);
      if (importPattern.test(source)) {
        withImports.push(relativePath);
      }
      if (domPattern.test(source)) {
        withDomApi.push(relativePath);
      }
    }

    expect(withImports).toEqual([]);
    expect(withDomApi).toEqual([]);
  });
});
