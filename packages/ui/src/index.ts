/**
 * @base/ui —— 基础组件的**逻辑与样式契约**（REQ-011 / AC-S-075）。
 *
 * # 落地形态（rev 2 裁定 #7）
 *
 * | 位置 | 内容 |
 * |---|---|
 * | `packages/ui/src/**`（本文件） | 变体名、类名、props 类型、行为函数 |
 * | `apps/shell/src/components/*.svelte` | Svelte 组件（薄封装，调用本文件的逻辑） |
 *
 * 包的构建是 `tsc`（`include` 只有 `src` 下的 `.ts`），`.svelte` 文件**进不了产物**；
 * 引 `svelte-package` 又是**新增依赖**（违反本期零新依赖约束）。
 * 所以：**组件库发逻辑，宿主应用发视图**。
 *
 * # 颜色：零字面量
 *
 * 组件只给出**类名**，样式由宿主样式表消费 `@base/theme` 的 CSS 变量
 * （`var(--base-primary)` 等）—— 本包里**一个颜色字面量都没有**（D95）。
 *
 * # Table 只接受 props
 *
 * 表格**不引用** `@base/hooks`：分页与表格状态由消费方用 `@base/hooks` 组合后
 * 以 props 传进来（rows / columns / page / pageSize / total / onPageChange）。
 */

/** 基础色调。 */
export type Tone = 'default' | 'primary' | 'danger';

/** 全部可选色调。 */
export const TONES: readonly Tone[] = ['default', 'primary', 'danger'];

/** 色调对应的样式类名。 */
export function toneClass(tone: Tone): string {
  return `base-tone base-tone--${tone}`;
}

/** 圆角 token，单位 px。 */
export interface RadiusTokens {
  readonly small: number;
  readonly medium: number;
  readonly large: number;
}

/** 默认圆角 token。 */
export function defaultRadius(): RadiusTokens {
  return { small: 2, medium: 4, large: 8 };
}

/** 按钮变体。 */
export type ButtonVariant = 'primary' | 'secondary' | 'danger' | 'ghost';

/** 全部按钮变体。 */
export const BUTTON_VARIANTS: readonly ButtonVariant[] = [
  'primary',
  'secondary',
  'danger',
  'ghost',
];

/** 按钮尺寸。 */
export type ButtonSize = 'small' | 'medium' | 'large';

/** 按钮属性契约。 */
export interface ButtonProps {
  /** 变体（默认 `primary`）。 */
  readonly variant?: ButtonVariant;
  /** 尺寸（默认 `medium`）。 */
  readonly size?: ButtonSize;
  /** 是否禁用。 */
  readonly disabled?: boolean;
  /** 原生 button 类型（默认 `button`）。 */
  readonly type?: 'button' | 'submit' | 'reset';
  /** 追加的自定义类名。 */
  readonly class?: string;
}

/** 按钮类名（样式契约）。 */
export function buttonClass(props: ButtonProps = {}): string {
  const variant = props.variant ?? 'primary';
  const size = props.size ?? 'medium';
  const names = ['base-button', `base-button--${variant}`, `base-button--${size}`];
  if (props.disabled === true) {
    names.push('base-button--disabled');
  }
  if (props.class !== undefined && props.class !== '') {
    names.push(props.class);
  }
  return names.join(' ');
}

/** 输入框类型。 */
export type InputType = 'text' | 'password' | 'email' | 'number' | 'search';

/** 输入框属性契约。 */
export interface InputProps {
  /** 可见标签。 */
  readonly label?: string;
  /** 表单字段名。 */
  readonly name?: string;
  /** 输入类型（默认 `text`）。 */
  readonly type?: InputType;
  /** 当前值。 */
  readonly value?: string;
  /** 占位文案。 */
  readonly placeholder?: string;
  /** 是否禁用。 */
  readonly disabled?: boolean;
  /** 是否必填。 */
  readonly required?: boolean;
  /** 错误提示（有值时组件进入错误态）。 */
  readonly error?: string;
  /** 追加的自定义类名。 */
  readonly class?: string;
}

/** 输入框类名（样式契约）。 */
export function inputClass(props: InputProps = {}): string {
  const names = ['base-input'];
  if (props.disabled === true) {
    names.push('base-input--disabled');
  }
  if (props.error !== undefined && props.error !== '') {
    names.push('base-input--error');
  }
  if (props.class !== undefined && props.class !== '') {
    names.push(props.class);
  }
  return names.join(' ');
}

/** 卡片属性契约。 */
export interface CardProps {
  /** 卡片标题。 */
  readonly title?: string;
  /** 是否带页脚。 */
  readonly footer?: boolean;
  /** 追加的自定义类名。 */
  readonly class?: string;
}

/** 卡片类名（样式契约）。 */
export function cardClass(props: CardProps = {}): string {
  const names = ['base-card'];
  if (props.footer === true) {
    names.push('base-card--with-footer');
  }
  if (props.class !== undefined && props.class !== '') {
    names.push(props.class);
  }
  return names.join(' ');
}

/** 表格列定义。 */
export interface TableColumn<Row = unknown> {
  /** 列键（取行数据的字段名）。 */
  readonly key: string;
  /** 表头文案。 */
  readonly label: string;
  /** 对齐方式。 */
  readonly align?: 'left' | 'center' | 'right';
  /** 列宽（CSS 长度）。 */
  readonly width?: string;
  /** 是否可排序（排序由消费方实现）。 */
  readonly sortable?: boolean;
  /** 自定义单元格取值（不给时按 `key` 取字段）。 */
  readonly render?: (row: Row) => string;
}

/**
 * 表格属性契约。
 *
 * **只接受 props** —— 表格不持有分页 / 排序状态（那是 `@base/hooks` 的事）。
 */
export interface TableProps<Row = unknown> {
  /** 当前页的数据行。 */
  readonly rows: readonly Row[];
  /** 列定义。 */
  readonly columns: readonly TableColumn<Row>[];
  /** 当前页码（从 1 开始）。 */
  readonly page?: number;
  /** 每页行数。 */
  readonly pageSize?: number;
  /** 总行数。 */
  readonly total?: number;
  /** 翻页回调（消费方拿它去改自己的状态）。 */
  readonly onPageChange?: (page: number) => void;
  /** 行键（默认用行索引）。 */
  readonly rowKey?: (row: Row, index: number) => string;
  /** 空数据文案。 */
  readonly emptyText?: string;
  /** 追加的自定义类名。 */
  readonly class?: string;
}

/** 表格类名（样式契约）。 */
export function tableClass(props: { class?: string } = {}): string {
  const names = ['base-table'];
  if (props.class !== undefined && props.class !== '') {
    names.push(props.class);
  }
  return names.join(' ');
}

/** 分页信息（纯计算，不持有状态）。 */
export interface PageInfo {
  /** 当前页码（从 1 开始）。 */
  readonly page: number;
  /** 每页行数。 */
  readonly pageSize: number;
  /** 总行数。 */
  readonly total: number;
  /** 总页数（至少 1）。 */
  readonly pageCount: number;
  /** 当前页第一行的序号（从 1 开始；空数据时为 0）。 */
  readonly from: number;
  /** 当前页最后一行的序号（从 1 开始；空数据时为 0）。 */
  readonly to: number;
}

/** 计算分页信息。 */
export function pageInfo(page: number, pageSize: number, total: number): PageInfo {
  const safeSize = pageSize > 0 ? Math.floor(pageSize) : 1;
  const safeTotal = total > 0 ? Math.floor(total) : 0;
  const pageCount = safeTotal === 0 ? 1 : Math.ceil(safeTotal / safeSize);
  const current = Math.min(Math.max(Math.floor(page) || 1, 1), pageCount);
  const from = safeTotal === 0 ? 0 : (current - 1) * safeSize + 1;
  const to = safeTotal === 0 ? 0 : Math.min(current * safeSize, safeTotal);
  return { page: current, pageSize: safeSize, total: safeTotal, pageCount, from, to };
}

/** 对话框属性契约。 */
export interface DialogProps {
  /** 是否打开。 */
  readonly open?: boolean;
  /** 标题。 */
  readonly title?: string;
  /** 宽度（CSS 长度）。 */
  readonly width?: string;
  /** 是否可以点遮罩 / 按 Esc 关闭。 */
  readonly closable?: boolean;
  /** 追加的自定义类名。 */
  readonly class?: string;
}

/** 对话框类名（样式契约）。 */
export function dialogClass(props: DialogProps = {}): string {
  const names = ['base-dialog'];
  if (props.open === true) {
    names.push('base-dialog--open');
  }
  if (props.closable === false) {
    names.push('base-dialog--static');
  }
  if (props.class !== undefined && props.class !== '') {
    names.push(props.class);
  }
  return names.join(' ');
}

/** 取行的键（默认用行索引）。 */
export function rowKeyOf<Row>(
  row: Row,
  index: number,
  rowKey?: (row: Row, index: number) => string,
): string {
  return rowKey ? rowKey(row, index) : String(index);
}

/** 取单元格的文本（列给了 `render` 就用它，否则按列键取字段）。 */
export function cellValue<Row>(row: Row, column: TableColumn<Row>): string {
  if (column.render !== undefined) {
    return column.render(row);
  }
  const record = row as unknown as Record<string, unknown>;
  const value = record[column.key];
  return value === undefined || value === null ? '' : String(value);
}
