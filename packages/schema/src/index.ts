/**
 * 元数据 schema 包 —— **纯逻辑**（不依赖 DOM / 框架 / 其它包）。
 *
 * # 分工（沿用 REQ-011 裁定 #7）
 *
 * | 位置 | 内容 |
 * |---|---|
 * | `packages/schema/src/**`（本文件） | 类型 + 渲染 / 校验 / 查询构造的**纯函数** |
 * | `apps/shell/src/components/SchemaTable.svelte` | 列表视图（薄封装，调用本文件） |
 * | `apps/shell/src/components/SchemaForm.svelte` | 表单视图（薄封装，调用本文件） |
 *
 * 包内**零颜色字面量**、**零 DOM 依赖**（D95）：组件只给类名，样式在 `apps/shell/src/app.css`。
 *
 * # 渲染器不做权限判断（AC-S-084）
 *
 * 后端已经按字段级权限过滤过元数据（AC-S-079）与数据（AC-S-080）。
 * 前端**只是忠实渲染后端给的东西** —— 它不会读元数据里没有的键，
 * 也不会自造字段。权限的正确性由后端负责。
 *
 * # 未知字段类型：降级，不崩（AC-S-087）
 *
 * 「运行时拿到未知类型」与「manifest 声明未知类型」是两个场景：
 * 前者（老前端 + 新后端）**降级为纯文本** + 一条可发现的告警；后者的启动校验是后端的事。
 */

/** 字段类型（本期允许集合，与 manifest 校验的枚举一致）。 */
export type FieldType = 'text' | 'number' | 'boolean' | 'date' | 'reference';

/** 本期允许的字段类型（顺序即文档顺序）。 */
export const FIELD_TYPES: readonly FieldType[] = ['text', 'number', 'boolean', 'date', 'reference'];

/** `reference` 指向的实体（本期只读展示 id，不做下拉 —— D105）。 */
export interface ReferenceTarget {
  /** 模块标识。 */
  readonly module: string;
  /** 实体键。 */
  readonly entity: string;
}

/**
 * 字段元数据。
 *
 * `type` 声明为 `string`（不是 `FieldType`）：运行时可能拿到**未知类型**
 * （老前端 + 新后端）。用联合类型会让「降级渲染」这条路径在编译期就被堵死，
 * 进而诱使实现方用 `as` 强转 —— 那只是把问题藏起来。
 */
export interface FieldMeta {
  /** 字段名，camelCase。 */
  readonly key: string;
  /** 展示标签。 */
  readonly label: string;
  /** 字段类型；已知值见 `FieldType`，未知值按 AC-S-087 降级。 */
  readonly type: string;
  /** 是否必填。 */
  readonly required?: boolean;
  /** 字段级权限点（后端据此过滤；前端不判断）。 */
  readonly permission?: string;
  /** 只读（表单里不渲染输入控件）。 */
  readonly readonly?: boolean;
  /** 文本最大长度。 */
  readonly maxLength?: number;
  /** 文本最小长度。 */
  readonly minLength?: number;
  /** 文本正则约束（字符串形式）。 */
  readonly pattern?: string;
  /** 数字下界。 */
  readonly min?: number;
  /** 数字上界。 */
  readonly max?: number;
  /** `reference` 的目标（本期只展示 id）。 */
  readonly reference?: ReferenceTarget;
}

/**
 * 数据接口声明（D102）。
 *
 * - `list` / `create` **必填**
 * - `update` / `delete` 可选：不声明 → 前端不渲染对应按钮
 * - `{id}` 是唯一占位符，渲染时替换为记录 id
 *
 * 路径必须以 `/api/` 开头（后端启动校验；前端不做字符串拼接猜测）。
 */
export interface ApiEndpoints {
  /** 列表接口。 */
  readonly list: string;
  /** 创建接口。 */
  readonly create: string;
  /** 更新接口（可选）。 */
  readonly update?: string;
  /** 删除接口（可选）。 */
  readonly delete?: string;
}

/** 实体元数据（`GET /api/schema/<module>/<entity>` 的响应形状）。 */
export interface EntityMeta {
  /** 模块标识。 */
  readonly module: string;
  /** 实体键。 */
  readonly entity: string;
  /** 展示名。 */
  readonly label: string;
  /** 数据接口。 */
  readonly api: ApiEndpoints;
  /** 字段（顺序 = manifest 声明顺序 = 表头顺序）。 */
  readonly fields: readonly FieldMeta[];
}

/** 单元格的格式化结果。 */
export interface FormattedCell {
  /** 展示文本（空值 → 空串）。 */
  readonly text: string;
  /** 该字段的类型渲染器不认识（已降级为纯文本）。 */
  readonly unknownType: boolean;
}

/** 一页列表数据（游标分页；**没有** `total`）。 */
export interface ListPage {
  /** 当前页记录。 */
  readonly items: Record<string, unknown>[];
  /** 下一页游标；`null` = 没有下一页。 */
  readonly nextCursor: string | null;
}

/** 表单提交的准备结果。 */
export interface SubmitPreparation {
  /** 「字段 → 错误文案」；空对象 = 通过。 */
  readonly errors: Record<string, string>;
  /** 校验通过时的提交体；**不通过时为 `undefined`（不得发请求）**。 */
  readonly body?: Record<string, unknown>;
}

/** 默认每页条数（与 sandbox `config.pageSize` 的默认值一致）。 */
export const DEFAULT_PAGE_SIZE = 20;

/** 更新 / 删除路径里的 id 占位符。 */
export const ID_PLACEHOLDER = '{id}';

/** 字段的可读描述，用于无障碍标签与提示文案。 */
export function describeField(field: FieldMeta): string {
  return field.required ? `${field.label}（必填）` : field.label;
}

/** 按字段名查找元数据。 */
export function findField(fields: readonly FieldMeta[], key: string): FieldMeta | undefined {
  return fields.find((field) => field.key === key);
}

/** 类型是否属于本期允许集合。 */
export function isKnownFieldType(type: string): type is FieldType {
  return (FIELD_TYPES as readonly string[]).includes(type);
}

/**
 * 过滤出当前元数据里**可见**的字段（后端已过滤，前端只是忠实渲染）。
 *
 * 只做**结构容错**：缺 `key` / `type` 的条目丢弃；缺 `label` 时回退为 `key`
 * （宁可显示字段名，也不要整页白屏）。**不按权限过滤** —— 那不是前端的事。
 */
export function visibleFields(meta: EntityMeta): FieldMeta[] {
  const rawFields: unknown = meta?.fields;
  if (!Array.isArray(rawFields)) {
    return [];
  }
  const fields: FieldMeta[] = [];
  for (const raw of rawFields) {
    if (raw === null || typeof raw !== 'object') {
      continue;
    }
    const candidate = raw as Partial<FieldMeta>;
    if (typeof candidate.key !== 'string' || candidate.key === '') {
      continue;
    }
    if (typeof candidate.type !== 'string' || candidate.type === '') {
      continue;
    }
    const label =
      typeof candidate.label === 'string' && candidate.label !== ''
        ? candidate.label
        : candidate.key;
    fields.push({ ...(candidate as FieldMeta), label });
  }
  return fields;
}

/** 值是否算「空」（`null` / `undefined` / 空白字符串）。 */
function isEmptyValue(value: unknown): boolean {
  return (
    value === undefined || value === null || (typeof value === 'string' && value.trim() === '')
  );
}

/**
 * 初始化表单值：**只留元数据声明的字段**。
 *
 * 页面 / 游标等内部状态、以及数据里多出来的键（AC-S-087 的「元数据删了字段但数据还带」）
 * 都**不会**进入表单 —— 这样提交体不可能夹带渲染器内部状态。
 * 缺值的字段：`boolean` → `false`，其余 → 空串（不是 `undefined` 字样）。
 */
export function formValues(
  meta: EntityMeta,
  record: Record<string, unknown> = {},
): Record<string, unknown> {
  const values: Record<string, unknown> = {};
  for (const field of visibleFields(meta)) {
    const raw = record[field.key];
    values[field.key] =
      raw === undefined || raw === null ? (field.type === 'boolean' ? false : '') : raw;
  }
  return values;
}

/**
 * 表单校验：返回「字段 → 错误文案」；**空映射 = 通过**。
 *
 * 错误文案钉死（与用例集一致）：
 * - 必填留空 → `<label>不能为空`
 * - `number` 非数字 → `<label>必须是数字`
 * - 长度 / 正则 / 上下界 → 各自点名字段
 *
 * `readonly` 字段**不参与校验**（表单里不渲染控件，也**不提交** —— TC-012-045）。
 */
export function validateForm(
  meta: EntityMeta,
  values: Record<string, unknown>,
): Record<string, string> {
  const errors: Record<string, string> = {};
  for (const field of visibleFields(meta)) {
    if (field.readonly === true) {
      continue;
    }
    const value = values?.[field.key];
    if (isEmptyValue(value)) {
      if (field.required === true) {
        errors[field.key] = `${field.label}不能为空`;
      }
      continue;
    }
    const text = typeof value === 'string' ? value : String(value);
    if (field.type === 'number') {
      const numeric = typeof value === 'number' ? value : Number(text.trim());
      if (!Number.isFinite(numeric)) {
        errors[field.key] = `${field.label}必须是数字`;
        continue;
      }
      if (field.min !== undefined && numeric < field.min) {
        errors[field.key] = `${field.label}不能小于${field.min}`;
        continue;
      }
      if (field.max !== undefined && numeric > field.max) {
        errors[field.key] = `${field.label}不能大于${field.max}`;
        continue;
      }
    }
    if (field.maxLength !== undefined && text.length > field.maxLength) {
      errors[field.key] = `${field.label}不能超过${field.maxLength}个字符`;
      continue;
    }
    if (field.minLength !== undefined && text.length < field.minLength) {
      errors[field.key] = `${field.label}不能少于${field.minLength}个字符`;
      continue;
    }
    if (field.pattern !== undefined) {
      let pattern: RegExp | undefined;
      try {
        pattern = new RegExp(field.pattern);
      } catch {
        // 后端给了非法正则：**不拦用户**（元数据问题不该表现为「表单永远提交不了」）。
        pattern = undefined;
      }
      if (pattern !== undefined && !pattern.test(text)) {
        errors[field.key] = `${field.label}格式不正确`;
      }
    }
  }
  return errors;
}

/**
 * 校验 + 构造提交体。
 *
 * **校验不通过时 `body` 为 `undefined`** —— 视图层据此**不发请求**（AC-S-083 步骤 3）。
 * 提交体只含元数据声明过的**可写**字段（`readonly` 不提交，TC-012-045），
 * 不会夹带 `$` / `_` 开头的渲染器内部状态。
 */
export function prepareSubmit(
  meta: EntityMeta,
  values: Record<string, unknown>,
): SubmitPreparation {
  const errors = validateForm(meta, values);
  if (Object.keys(errors).length > 0) {
    return { errors };
  }
  const body: Record<string, unknown> = {};
  for (const field of visibleFields(meta)) {
    if (field.readonly === true) {
      continue;
    }
    const value = values?.[field.key];
    if (value === undefined || value === null) {
      continue;
    }
    if (field.type === 'number') {
      if (typeof value === 'string' && value.trim() === '') {
        continue;
      }
      body[field.key] = typeof value === 'number' ? value : Number(String(value).trim());
      continue;
    }
    if (field.type === 'boolean') {
      body[field.key] = value === true || value === 'true';
      continue;
    }
    if (
      typeof value === 'string' &&
      value.trim() === '' &&
      (field.type === 'date' || field.type === 'reference')
    ) {
      // 空日期 / 空引用**不下发空串**：后端解析日期列会因为 `""` 直接 400。
      // 空文本仍然下发（清空一个文本字段是有效的编辑动作）。
      continue;
    }
    body[field.key] = value;
  }
  return { errors: {}, body };
}

/** 日期值 → `YYYY-MM-DD`（**不做时区换算**：库里存什么显示什么）。 */
function formatDateValue(value: unknown): string {
  if (value instanceof Date) {
    return Number.isNaN(value.getTime()) ? '' : value.toISOString().slice(0, 10);
  }
  const text = String(value).trim();
  const match = /^(\d{4}-\d{2}-\d{2})/.exec(text);
  return match?.[1] ?? text;
}

/** 数字值 → 十进制文本（`NaN` / `Infinity` → 空串，不显示 `NaN`）。 */
function formatNumberValue(value: unknown): string {
  if (typeof value === 'number') {
    return Number.isFinite(value) ? String(value) : '';
  }
  if (typeof value === 'bigint') {
    return value.toString();
  }
  return String(value);
}

/**
 * 按类型格式化单元格（AC-S-082 步骤 3）。
 *
 * | 类型 | 文本 |
 * |---|---|
 * | `boolean` | `是` / `否` |
 * | `date` | `YYYY-MM-DD`（不换算时区） |
 * | `number` | 十进制文本 |
 * | `text` / `reference` | 原值文本（`reference` 是 id 原值 —— D105） |
 * | 未知类型 | 原值文本 + `unknownType: true`（**不抛错**） |
 * | 空值（`null` / `undefined` / 缺字段） | 空串 |
 */
export function formatCell(field: FieldMeta, value: unknown): FormattedCell {
  const known = isKnownFieldType(field.type);
  if (value === undefined || value === null) {
    return { text: '', unknownType: !known };
  }
  switch (field.type) {
    case 'boolean':
      return {
        text: typeof value === 'boolean' ? (value ? '是' : '否') : String(value),
        unknownType: false,
      };
    case 'date':
      return { text: formatDateValue(value), unknownType: false };
    case 'number':
      return { text: formatNumberValue(value), unknownType: false };
    default:
      // text / reference / 未知类型：原值文本。
      return { text: String(value), unknownType: !known };
  }
}

/**
 * 构造列表查询参数（游标分页）。
 *
 * 形状钉死（TC-012-038）：首屏**不带** `cursor` 键；`limit` 缺省 20；
 * 键顺序 `cursor` 在前、`limit` 在后（序列化后是 `cursor=...&limit=20`）。
 */
export function buildListQuery(cursor?: string | null, limit?: number): Record<string, string> {
  const query: Record<string, string> = {};
  if (typeof cursor === 'string' && cursor !== '') {
    query.cursor = cursor;
  }
  query.limit = String(limit ?? DEFAULT_PAGE_SIZE);
  return query;
}

/** 记录 id 的提取（用于 update / delete 的 `{id}` 替换）。 */
export function recordId(record: Record<string, unknown>): string {
  const value = record?.['id'];
  if (value === undefined || value === null) {
    return '';
  }
  return String(value);
}

/** 把 `api.update` / `api.delete` 模板里的 `{id}` 替换为记录 id。 */
export function resolveApiPath(template: string, id: string): string {
  return template.split(ID_PLACEHOLDER).join(encodeURIComponent(id));
}

/**
 * 解析元数据响应（容错）。
 *
 * 缺 `api.list` / `api.create` 视为**非法元数据**（返回 `undefined`）——
 * 没有数据接口地址，渲染器不知道该向哪取数（D102 就是为了消除这个问题）。
 * 其余字段尽量宽容（缺 `module` / `label` 不影响渲染）。
 */
export function parseEntityMeta(raw: unknown): EntityMeta | undefined {
  if (raw === null || typeof raw !== 'object' || Array.isArray(raw)) {
    return undefined;
  }
  const body = raw as Record<string, unknown>;
  const rawApi = body['api'];
  if (rawApi === null || typeof rawApi !== 'object' || Array.isArray(rawApi)) {
    return undefined;
  }
  const api = rawApi as Record<string, unknown>;
  const list = typeof api['list'] === 'string' ? api['list'] : '';
  const create = typeof api['create'] === 'string' ? api['create'] : '';
  if (list === '' || create === '') {
    return undefined;
  }
  const update =
    typeof api['update'] === 'string' && api['update'] !== '' ? api['update'] : undefined;
  const remove =
    typeof api['delete'] === 'string' && api['delete'] !== '' ? api['delete'] : undefined;
  const rawFields = Array.isArray(body['fields']) ? (body['fields'] as readonly FieldMeta[]) : [];
  return {
    module: typeof body['module'] === 'string' ? body['module'] : '',
    entity: typeof body['entity'] === 'string' ? body['entity'] : '',
    label: typeof body['label'] === 'string' ? body['label'] : '',
    // 未声明的可选键**省略**（不是 `undefined` 值）—— 与后端的响应约定一致。
    api: {
      list,
      create,
      ...(update === undefined ? {} : { update }),
      ...(remove === undefined ? {} : { delete: remove }),
    },
    fields: rawFields,
  };
}

/** 只保留对象行（数组 / 标量一律丢弃）。 */
function toRows(items: readonly unknown[]): Record<string, unknown>[] {
  const rows: Record<string, unknown>[] = [];
  for (const item of items) {
    if (item !== null && typeof item === 'object' && !Array.isArray(item)) {
      rows.push(item as Record<string, unknown>);
    }
  }
  return rows;
}

/**
 * 解析列表响应（容错）。
 *
 * 形状钉死为 `{ items: [...], nextCursor: string | null }`；
 * 同时容忍裸数组（老接口）与 `next_cursor`（Rust 字段名的直译）——
 * 形状不对时**返回空页**，不抛错（列表页不该白屏）。
 */
export function parseListResponse(raw: unknown): ListPage {
  if (Array.isArray(raw)) {
    return { items: toRows(raw), nextCursor: null };
  }
  if (raw === null || typeof raw !== 'object') {
    return { items: [], nextCursor: null };
  }
  const body = raw as Record<string, unknown>;
  const rawItems = Array.isArray(body['items']) ? body['items'] : [];
  const rawCursor = body['nextCursor'] ?? body['next_cursor'];
  return {
    items: toRows(rawItems),
    nextCursor: typeof rawCursor === 'string' && rawCursor !== '' ? rawCursor : null,
  };
}

/**
 * 未知字段类型的告警文案（AC-S-087：一条**可发现**的告警）。
 *
 * 文案包含类型名与字段 key —— 否则 `console.warn('oops')` 也能「通过」。
 */
export function unknownTypeWarning(field: FieldMeta): string {
  return `[schema] 未知字段类型 "${field.type}"（字段 "${field.key}"），已降级为纯文本展示`;
}
