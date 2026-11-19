import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

import { describe, expect, it } from 'vitest';

import {
  BUTTON_VARIANTS,
  TONES,
  buttonClass,
  cardClass,
  defaultRadius,
  dialogClass,
  inputClass,
  pageInfo,
  rowKeyOf,
  tableClass,
  toneClass,
} from '../src/index';

/** 仓库根目录（本文件在 `packages/ui/test/` 下）。 */
const REPO_ROOT = join(dirname(fileURLToPath(import.meta.url)), '..', '..', '..');

/** `@base/ui` 的源码。 */
function uiSource(): string {
  return readFileSync(join(REPO_ROOT, 'packages/ui', 'src', 'index.ts'), 'utf-8');
}

/** 取出 `TableProps` 接口的属性名（从源码里读，避免「测了另一个形状」）。 */
function tablePropNames(): string[] {
  const source = uiSource();
  const start = source.indexOf('export interface TableProps');
  expect(start).toBeGreaterThan(-1);
  const body = source.slice(start);
  const end = body.indexOf('\n}');
  const inner = body.slice(0, end === -1 ? body.length : end);
  return [...inner.matchAll(/readonly\s+([A-Za-z_][A-Za-z0-9_]*)\??\s*:/g)].map(
    (match) => match[1] ?? '',
  );
}

describe('@base/ui 的 token', () => {
  it('色调类名可预期', () => {
    expect(toneClass('primary')).toBe('base-tone base-tone--primary');
    expect(toneClass('danger')).toBe('base-tone base-tone--danger');
  });

  it('默认圆角 token 递增', () => {
    const radius = defaultRadius();
    expect(radius.small).toBeLessThan(radius.medium);
    expect(radius.medium).toBeLessThan(radius.large);
  });

  it('色调清单包含全部可选值', () => {
    expect(TONES).toEqual(['default', 'primary', 'danger']);
  });
});

describe('@base/ui 的组件契约', () => {
  it('按钮类名带变体与尺寸', () => {
    expect(buttonClass()).toBe('base-button base-button--primary base-button--medium');
    expect(buttonClass({ variant: 'danger', size: 'small' })).toBe(
      'base-button base-button--danger base-button--small',
    );
    expect(buttonClass({ disabled: true })).toContain('base-button--disabled');
    expect(buttonClass({ class: 'extra' })).toContain('extra');
    expect(BUTTON_VARIANTS).toEqual(['primary', 'secondary', 'danger', 'ghost']);
  });

  it('输入框类名在禁用 / 错误时带状态类', () => {
    expect(inputClass()).toBe('base-input');
    expect(inputClass({ disabled: true })).toBe('base-input base-input--disabled');
    expect(inputClass({ error: '用户名不能为空' })).toBe('base-input base-input--error');
  });

  it('卡片类名区分有无页脚', () => {
    expect(cardClass()).toBe('base-card');
    expect(cardClass({ footer: true })).toBe('base-card base-card--with-footer');
  });

  it('表格类名稳定', () => {
    expect(tableClass()).toBe('base-table');
    expect(tableClass({ class: 'wide' })).toBe('base-table wide');
  });

  it('对话框类名区分打开态与不可关闭', () => {
    expect(dialogClass()).toBe('base-dialog');
    expect(dialogClass({ open: true })).toBe('base-dialog base-dialog--open');
    expect(dialogClass({ closable: false })).toBe('base-dialog base-dialog--static');
  });

  it('行键默认用行索引', () => {
    expect(rowKeyOf({ id: 'r-1' }, 3)).toBe('3');
    expect(rowKeyOf({ id: 'r-1' }, 3, (row) => row.id)).toBe('r-1');
  });
});

describe('@base/ui 的分页计算', () => {
  it('页数与区间算得对', () => {
    expect(pageInfo(1, 10, 25)).toEqual({
      page: 1,
      pageSize: 10,
      total: 25,
      pageCount: 3,
      from: 1,
      to: 10,
    });
    expect(pageInfo(3, 10, 25)).toEqual({
      page: 3,
      pageSize: 10,
      total: 25,
      pageCount: 3,
      from: 21,
      to: 25,
    });
  });

  it('空数据时页数为 1、区间为 0', () => {
    expect(pageInfo(1, 10, 0)).toEqual({
      page: 1,
      pageSize: 10,
      total: 0,
      pageCount: 1,
      from: 0,
      to: 0,
    });
  });

  it('越界页码被夹到有效范围', () => {
    expect(pageInfo(99, 10, 25).page).toBe(3);
    expect(pageInfo(0, 10, 25).page).toBe(1);
  });
});

describe('@base/ui 的依赖方向（AC-S-075）', () => {
  it('TC-011-048：表格只接受 props，不持有内部状态容器', () => {
    const names = tablePropNames();
    expect(names).toContain('rows');
    expect(names).toContain('columns');
    expect(names).toContain('page');
    expect(names).toContain('pageSize');
    expect(names).toContain('total');
    expect(names).toContain('onPageChange');

    for (const forbidden of [
      'state',
      'store',
      'pageState',
      'tableState',
      'usePagination',
      'createTableState',
    ]) {
      expect(names).not.toContain(forbidden);
    }
  });

  it('TC-011-048：@base/ui 不引用 @base/hooks', () => {
    expect(uiSource()).not.toMatch(/from\s+['"]@base\/hooks['"]/);
  });
});
