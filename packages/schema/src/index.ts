/**
 * 元数据 schema 占位包。
 *
 * 依据 `api-conventions v1` C11，schema 渲染器导出 `SchemaTable` / `SchemaForm`；
 * 本期只定义字段元数据的形状与描述入口，渲染器在 REQ-012 实现。
 */

/** 字段类型。 */
export type FieldType = 'text' | 'number' | 'boolean' | 'date' | 'reference';

/** 字段元数据。 */
export interface FieldMeta {
  /** 字段名，camelCase。 */
  readonly key: string;
  /** 展示标签。 */
  readonly label: string;
  /** 字段类型。 */
  readonly type: FieldType;
  /** 是否必填。 */
  readonly required?: boolean;
}

/** 字段的可读描述，用于无障碍标签与提示文案。 */
export function describeField(field: FieldMeta): string {
  return field.required ? `${field.label}（必填）` : field.label;
}

/** 按字段名查找元数据。 */
export function findField(fields: readonly FieldMeta[], key: string): FieldMeta | undefined {
  return fields.find((field) => field.key === key);
}
