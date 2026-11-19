import { readFileSync, readdirSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

import { describe, expect, it } from 'vitest';

/**
 * 零代码的**源码级证据**（AC-S-085 步骤 5 / 6 / TC-012-051）。
 *
 * 哈希未变只能证明「文件没被改」，不能证明「没有按实体分支」。更强的判据是：
 * 渲染器的源码里**根本不存在**任何实体的键 / 展示名。
 *
 * 这同时是一条**防回归**：谁要是给某个实体加了特判，这里会红。
 */

/** 仓库根（apps/shell/test → feature-REQ-012）。 */
const repoRoot = fileURLToPath(new URL('../../../', import.meta.url));

/** 参与零代码判定的源码目录（渲染器逻辑 + 视图）。 */
const SOURCE_DIRS = ['packages/schema/src', 'apps/shell/src'];

/** 实体专属的 token：任何一条出现在源码里都说明「为它写过代码」。 */
const ENTITY_TOKENS = ['demoItem', 'demoNote', 'demo-notes', '备注簿'];

/** 递归列出源码文件（只要 .ts / .svelte）。 */
function sourceFiles(): string[] {
  const files: string[] = [];
  for (const dir of SOURCE_DIRS) {
    for (const entry of readdirSync(`${repoRoot}${dir}`, { recursive: true, encoding: 'utf8' })) {
      if (entry.endsWith('.ts') || entry.endsWith('.svelte')) {
        files.push(`${repoRoot}${dir}/${entry}`);
      }
    }
  }
  return files;
}

describe('零前端代码：源码里没有任何实体特化（AC-S-085 / TC-012-051）', () => {
  it('参与判定的源码集合非空（防止扫描退化成「扫了个空目录」而恒绿）', () => {
    const files = sourceFiles();
    expect(files.length).toBeGreaterThan(5);
    expect(files.some((file) => file.endsWith('SchemaTable.svelte'))).toBe(true);
    expect(files.some((file) => file.endsWith('SchemaForm.svelte'))).toBe(true);
  });

  it('源码全文不含任何实体键 / 实体专属文案', () => {
    const files = sourceFiles();
    for (const token of ENTITY_TOKENS) {
      const matched = files.filter((file) => readFileSync(file, 'utf8').includes(token));
      expect({ token, matched }).toEqual({ token, matched: [] });
    }
  });

  it('对照自检：同一扫描能命中已知文案（证明检索不是恒不命中）', () => {
    const hits = sourceFiles().filter(
      (file) =>
        file.endsWith('SchemaTable.svelte') && readFileSync(file, 'utf8').includes('暂无数据'),
    );
    expect(hits).toHaveLength(1);
  });
});
