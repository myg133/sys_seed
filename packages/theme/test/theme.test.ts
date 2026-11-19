import { readdirSync, readFileSync } from 'node:fs';
import { dirname, join, relative } from 'node:path';
import { fileURLToPath } from 'node:url';

import { describe, expect, it } from 'vitest';

import {
  applyBranding,
  applyTheme,
  currentMode,
  defaultTheme,
  mergeTheme,
  resolvePrimaryColor,
  setThemeMode,
  themeTokens,
  type ThemeTarget,
} from '../src/index';

/** 仓库根目录（本文件在 `packages/theme/test/` 下）。 */
const REPO_ROOT = join(dirname(fileURLToPath(import.meta.url)), '..', '..', '..');

/** 可注入的假文档：记录 inline style 的变量与根元素属性。 */
function fakeDocument(): {
  target: ThemeTarget;
  properties: Map<string, string>;
  attributes: Map<string, string>;
} {
  const properties = new Map<string, string>();
  const attributes = new Map<string, string>();
  const target = {
    documentElement: {
      style: {
        setProperty: (name: string, value: string) => {
          properties.set(name, value);
        },
      },
      setAttribute: (name: string, value: string) => {
        attributes.set(name, value);
      },
      getAttribute: (name: string) => attributes.get(name) ?? null,
    },
  } as unknown as ThemeTarget;
  return { target, properties, attributes };
}

/** 契约表里的 9 个变量名（精确集合）。 */
const TOKEN_NAMES = [
  '--base-bg',
  '--base-border',
  '--base-danger',
  '--base-font-family',
  '--base-primary',
  '--base-primary-hover',
  '--base-radius',
  '--base-text',
  '--base-text-secondary',
];

/** 颜色字面量正则（AC-S-073 步骤 4 的扫描参数）。 */
const COLOR_LITERAL = /#[0-9a-fA-F]{3,8}\b|\brgba?\s*\(|\bhsla?\s*\(|\boklch\s*\(/;

/** 剔除注释后再匹配（避免注释里的字面量造成误判）。 */
function stripComments(source: string): string {
  return source.replace(/\/\*[\s\S]*?\*\//g, '').replace(/\/\/.*$/gm, '');
}

/** 收集某个包 `src/` 下的源文件（排除测试与测试目录）。 */
function collectSourceFiles(packageName: string): string[] {
  const root = join(REPO_ROOT, 'packages', packageName, 'src');
  const found: string[] = [];
  const walk = (dir: string): void => {
    for (const entry of readdirSync(dir, { withFileTypes: true })) {
      const full = join(dir, entry.name);
      if (entry.isDirectory()) {
        walk(full);
        continue;
      }
      if (!/\.(ts|css)$/.test(entry.name) || entry.name.endsWith('.test.ts')) {
        continue;
      }
      found.push(full);
    }
  };
  walk(root);
  return found;
}

/** 含颜色字面量的文件集合。 */
function filesWithColorLiterals(packageName: string): string[] {
  return collectSourceFiles(packageName).filter((file) =>
    COLOR_LITERAL.test(stripComments(readFileSync(file, 'utf-8'))),
  );
}

describe('@base/theme 的令牌', () => {
  it('默认主题 token 齐全', () => {
    const theme = defaultTheme();
    expect(theme.primaryColor).toMatch(/^#[0-9a-f]{6}$/i);
    expect(theme.radius).toBeGreaterThan(0);
    expect(theme.fontFamily.length).toBeGreaterThan(0);
  });

  it('覆盖只影响指定项', () => {
    const theme = mergeTheme({ radius: 8 });
    expect(theme.radius).toBe(8);
    expect(theme.primaryColor).toBe(defaultTheme().primaryColor);
  });
});

describe('@base/theme 的令牌落地（AC-S-073）', () => {
  it('TC-011-038：根元素上出现约定的 9 个 CSS 变量（精确集合）', () => {
    const { target, properties } = fakeDocument();

    applyTheme(themeTokens('light'), { target });

    const written = [...properties.keys()].sort();
    expect(written).toEqual(TOKEN_NAMES);
    for (const name of TOKEN_NAMES) {
      expect(properties.get(name)?.length ?? 0).toBeGreaterThanOrEqual(1);
    }
    // 后端 `theme.radius` 是 number → 契约表的取值不带单位。
    expect(properties.get('--base-radius')).toBe('4');
    expect(properties.get('--base-font-family')).toBe(
      'system-ui, -apple-system, "Segoe UI", sans-serif',
    );
  });

  it('TC-011-039：切到暗色 → 颜色变量等于暗色期望值', () => {
    const { target, properties, attributes } = fakeDocument();

    setThemeMode('dark', { target });

    expect(properties.get('--base-danger')).toBe('#ff7875');
    expect(properties.get('--base-text')).toBe('#e6e6e6');
    expect(properties.get('--base-text-secondary')).toBe('#a6a6a6');
    expect(properties.get('--base-bg')).toBe('#141414');
    expect(properties.get('--base-border')).toBe('#303030');
    // 模式无关变量：取值与亮色相同，不参与「变了」的判定。
    expect(properties.get('--base-radius')).toBe('4');
    expect(properties.get('--base-font-family')).toBe(
      'system-ui, -apple-system, "Segoe UI", sans-serif',
    );
    expect(attributes.get('data-theme')).toBe('dark');
    expect(properties.get('color-scheme')).toBe('dark');
  });

  it('TC-011-040：从暗色切回亮色 → 变量回到亮色期望值（可逆）', () => {
    const { target, properties, attributes } = fakeDocument();

    setThemeMode('dark', { target });
    setThemeMode('light', { target });

    expect(properties.get('--base-danger')).toBe('#d4380d');
    expect(properties.get('--base-text')).toBe('#1f1f1f');
    expect(properties.get('--base-text-secondary')).toBe('#8c8c8c');
    expect(properties.get('--base-bg')).toBe('#ffffff');
    expect(properties.get('--base-border')).toBe('#d9d9d9');
    expect(properties.get('--base-radius')).toBe('4');
    expect(properties.get('--base-font-family')).toBe(
      'system-ui, -apple-system, "Segoe UI", sans-serif',
    );
    expect(attributes.get('data-theme')).toBe('light');
    expect(properties.get('color-scheme')).toBe('light');
  });

  it('当前模式可从 data-theme 读回', () => {
    const { target } = fakeDocument();
    expect(currentMode({ target })).toBe('light');
    setThemeMode('dark', { target });
    expect(currentMode({ target })).toBe('dark');
  });
});

describe('@base/theme 的品牌主色（AC-S-074）', () => {
  it('TC-011-041：--base-primary 精确等于接口返回值', () => {
    const { target, properties } = fakeDocument();

    applyBranding({ primaryColor: '#0055aa' }, { target });

    expect(properties.get('--base-primary')).toBe('#0055aa');
  });

  it('TC-011-042：非法主色 → 自己写中性兜底 currentColor', () => {
    const first = fakeDocument();
    expect(() => applyBranding({ primaryColor: '#zzz' }, { target: first.target })).not.toThrow();
    expect(first.properties.get('--base-primary')).toBe('currentColor');
    expect(first.properties.get('--base-primary')).not.toBe('#zzz');

    const second = fakeDocument();
    expect(() => applyBranding({ primaryColor: 'red' }, { target: second.target })).not.toThrow();
    expect(second.properties.get('--base-primary')).toBe('currentColor');
    expect(second.properties.get('--base-primary')).not.toBe('red');

    const third = fakeDocument();
    applyBranding({}, { target: third.target });
    expect(third.properties.get('--base-primary')).toBe('currentColor');
  });

  it('只接受合法十六进制', () => {
    expect(resolvePrimaryColor({ primaryColor: '#0055aa' })).toBe('#0055aa');
    expect(resolvePrimaryColor({ primaryColor: '#0af' })).toBe('#0af');
    expect(resolvePrimaryColor({ primaryColor: 'url(https://evil.example/x)' })).toBe(
      'currentColor',
    );
    expect(resolvePrimaryColor({})).toBe('currentColor');
  });

  it('品牌里的 radius / fontStack 映射到对应 CSS 变量', () => {
    const { target, properties } = fakeDocument();

    applyBranding(
      { primaryColor: '#0055aa', 'theme.radius': 8, 'theme.fontStack': ['Inter', 'sans-serif'] },
      { target },
    );

    expect(properties.get('--base-primary')).toBe('#0055aa');
    expect(properties.get('--base-radius')).toBe('8');
    expect(properties.get('--base-font-family')).toBe('Inter, sans-serif');
  });
});

describe('@base/theme 的颜色字面量扫描（AC-S-073 步骤 4 / D95）', () => {
  it('TC-011-043：@base/ui 源码零颜色字面量（且确实扫到了文件）', () => {
    const scanned = collectSourceFiles('ui');
    expect(scanned.length).toBeGreaterThanOrEqual(1);
    expect(filesWithColorLiterals('ui')).toEqual([]);
  });

  it('TC-011-044：@base/theme 的颜色字面量只在令牌定义文件里', () => {
    const scanned = collectSourceFiles('theme');
    expect(scanned.length).toBeGreaterThanOrEqual(1);
    const matched = filesWithColorLiterals('theme').map((file) =>
      relative(REPO_ROOT, file).split('\\').join('/'),
    );
    expect(matched).toEqual(['packages/theme/src/tokens.ts']);
  });
});
