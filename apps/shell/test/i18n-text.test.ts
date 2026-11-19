import { readFileSync, readdirSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

import { describe, expect, it } from 'vitest';

/**
 * 组件零硬编码文案（AC-S-099 步骤 4，rev 2 裁定 #14）。
 *
 * `apps/shell` 是**纯 CSR**（`mount()` + 无 Kit），所以「SSR 与 CSR 逐字相同」
 * 物理上无法验。可判定的替代判据是：**组件里没有文案字面量**，全部走 `t()`。
 *
 * 注释里的中文是**允许**的（design §2.6：代码注释 ✅）→ 扫描前先剥注释。
 */

/** 壳层源码目录（本文件在 `apps/shell/test/` 下）。 */
const SOURCE_DIR = fileURLToPath(new URL('../src/', import.meta.url));

/** 递归列出 `.svelte` 组件。 */
function svelteFiles(): string[] {
  const files: string[] = [];
  for (const entry of readdirSync(SOURCE_DIR, { recursive: true, encoding: 'utf8' })) {
    if (entry.endsWith('.svelte')) {
      files.push(`${SOURCE_DIR}${entry}`);
    }
  }
  return files;
}

/**
 * 剥掉注释（HTML `<!-- -->`、`//`、`/* *\/`），保留字符串与模板文本。
 *
 * 写状态机而不是正则：字符串里的 `//`（URL）与注释里的引号都会把正则带偏。
 */
export function stripComments(source: string): string {
  type Mode = 'code' | 'line' | 'block' | 'html' | 'single' | 'double' | 'template';
  let mode: Mode = 'code';
  let out = '';
  let index = 0;

  while (index < source.length) {
    const char = source[index] ?? '';
    const next = source[index + 1] ?? '';

    if (mode === 'code') {
      if (char === '/' && next === '/') {
        mode = 'line';
        index += 2;
        continue;
      }
      if (char === '/' && next === '*') {
        mode = 'block';
        index += 2;
        continue;
      }
      if (source.startsWith('<!--', index)) {
        mode = 'html';
        index += 4;
        continue;
      }
      if (char === "'") {
        mode = 'single';
      } else if (char === '"') {
        mode = 'double';
      } else if (char === '`') {
        mode = 'template';
      }
      out += char;
      index += 1;
      continue;
    }

    if (mode === 'line') {
      if (char === '\n') {
        mode = 'code';
        out += char;
      }
      index += 1;
      continue;
    }
    if (mode === 'block') {
      if (char === '*' && next === '/') {
        mode = 'code';
        index += 2;
      } else {
        index += 1;
      }
      continue;
    }
    if (mode === 'html') {
      if (source.startsWith('-->', index)) {
        mode = 'code';
        index += 3;
      } else {
        index += 1;
      }
      continue;
    }

    // 字符串 / 模板字面量：原样保留（有没有中文由调用方判定）。
    out += char;
    if (char === '\\') {
      out += source[index + 1] ?? '';
      index += 2;
      continue;
    }
    if (
      (mode === 'single' && char === "'") ||
      (mode === 'double' && char === '"') ||
      (mode === 'template' && char === '`')
    ) {
      mode = 'code';
    }
    index += 1;
  }

  return out;
}

/** CJK（含中文标点区；只判汉字块已足够发现文案）。 */
const CJK = /[\u3400-\u4dbf\u4e00-\u9fff]/;

describe('壳层组件零硬编码文案（AC-S-099 步骤 4）', () => {
  it('扫描集非空（防止退化成「扫了个空目录」而恒绿）', () => {
    const files = svelteFiles();
    expect(files.length).toBeGreaterThanOrEqual(8);
    expect(files.some((file) => file.endsWith('Home.svelte'))).toBe(true);
    expect(files.some((file) => file.endsWith('SchemaTable.svelte'))).toBe(true);
  });

  it('剥注释后，组件源码里没有任何中文文案字面量', () => {
    const offenders: { file: string; text: string }[] = [];
    for (const file of svelteFiles()) {
      const stripped = stripComments(readFileSync(file, 'utf8'));
      if (CJK.test(stripped)) {
        const first = /\n.*[\u3400-\u4dbf\u4e00-\u9fff].*\n/.exec(stripped)?.[0]?.trim() ?? '';
        offenders.push({ file: file.replace(SOURCE_DIR, ''), text: first });
      }
    }
    expect(offenders).toEqual([]);
  });

  it('对照自检：剥注释确实去掉了已知中文注释，而原文里有（证明扫描不是恒不命中）', () => {
    const raw = readFileSync(`${SOURCE_DIR}components/SchemaTable.svelte`, 'utf8');
    // 该文件的 props 注释里写着「暂无数据」。
    expect(raw).toMatch(CJK);
    expect(stripComments(raw)).not.toMatch(CJK);
  });

  it('同一扫描能命中注入的中文字面量（阳性对照，证明断言有牙齿）', () => {
    const fake = '<script lang="ts">const label = "保存";</script><p>加载中…</p>';
    expect(stripComments(fake)).toMatch(CJK);
    expect(stripComments('<!-- 中文注释 -->\n<p>Loading</p>')).not.toMatch(CJK);
  });
});
