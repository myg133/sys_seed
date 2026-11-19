<!--
  元数据驱动的表单（AC-S-083 / 084）。

  # 控件映射（本期钉死）

  | type | 控件 |
  |---|---|
  | `text` | 文本框 |
  | `number` | 数字输入 |
  | `boolean` | 复选框（可被 `role` / `aria-checked` 读到） |
  | `date` | 日期选择 |
  | `reference` | **只读文本（id 原值）** —— 本期不做下拉（D105） |
  | `readonly` 字段 | 不渲染输入控件，只显示文本（TC-012-045） |

  # 校验失败**不发请求**

  提交时先走 `gateSubmit()`（`src/lib/schema-submit.ts`，内部即 `prepareSubmit`）：
  有错误就只渲染错误、**不调用** `onSubmit`。这层被单拉成纯函数，是因为 SSR 单测
  点不到按钮 —— 组件里内联的闸门没有任何用例守得住（QA L-5 的假绿）。
  错误文案挂在**该字段旁边**，输入控件带 `aria-invalid="true"` +
  `aria-describedby` 指向错误节点（AC-S-083 步骤 4）。

  # props 整体取值

  svelte-check 会把「解构出来的可选 props」当成必填 —— 消费方就得把每个可选值
  都传一遍，契约反而没法用（D97 / REQ-011 rev 2 裁定 #7 的既有写法）。
-->
<script lang="ts">
  import {
    formatCell,
    formValues,
    isKnownFieldType,
    unknownTypeWarning,
    visibleFields,
    type EntityMeta,
    type FieldMeta,
  } from '@base/schema';

  import { gateSubmit } from '../lib/schema-submit';

  /** 组件 props（D97：必须有类型）。 */
  interface SchemaFormProps {
    /** 实体元数据（后端已按权限过滤）。 */
    meta: EntityMeta;
    /** 编辑时的现有记录（创建时不传）。 */
    record?: Record<string, unknown>;
    /** 表单模式（只用于按钮文案）。 */
    mode?: 'create' | 'edit';
    /** 提交中（按钮禁用，避免重复写）。 */
    submitting?: boolean;
    /** 提交按钮文案。 */
    submitLabel?: string;
    /** 提交（**校验通过后**才会被调用，body 只含元数据声明的可写字段）。 */
    onSubmit?: (body: Record<string, unknown>) => void;
    /** 取消。 */
    onCancel?: () => void;
  }

  const props: SchemaFormProps = $props();

  /** 表单里真正出现的字段（`readonly` 仍展示，但不渲染控件）。 */
  const fields = $derived(visibleFields(props.meta));

  // 初始值**有意**只取一次：之后由输入事件 / 换记录的重置逻辑维护。
  // svelte-ignore state_referenced_locally
  let values = $state<Record<string, unknown>>(formValues(props.meta, props.record));
  let errors = $state<Record<string, string>>({});

  // 换记录（编辑另一行）时重置：父组件也可以直接 `{#key}` 重挂载，两处都能收敛。
  let currentRecord: Record<string, unknown> | undefined = undefined;
  $effect(() => {
    if (props.record === currentRecord) {
      return;
    }
    currentRecord = props.record;
    values = formValues(props.meta, props.record);
    errors = {};
  });

  // 未知类型：降级为文本框，并留下**一条**可发现的告警（AC-S-087）。
  const warned: string[] = [];
  // svelte-ignore state_referenced_locally
  for (const field of visibleFields(props.meta)) {
    if (isKnownFieldType(field.type) || warned.includes(field.key)) {
      continue;
    }
    warned.push(field.key);
    // eslint-disable-next-line no-console -- 未知字段类型必须留下可发现的告警（AC-S-087）
    console.warn(unknownTypeWarning(field));
  }

  function isReadonly(field: FieldMeta): boolean {
    return field.readonly === true || field.type === 'reference';
  }

  function fieldId(field: FieldMeta): string {
    return `schema-field-${field.key}`;
  }

  function errorId(field: FieldMeta): string {
    return `schema-error-${field.key}`;
  }

  function setValue(key: string, value: unknown): void {
    values[key] = value;
    if (errors[key] !== undefined) {
      const next = { ...errors };
      delete next[key];
      errors = next;
    }
  }

  function textValue(field: FieldMeta): string {
    const value = values[field.key];
    if (field.type === 'date') {
      // 日期输入只认 `YYYY-MM-DD`：格式化规则与列表一致（不做时区换算）。
      return formatCell(field, value).text;
    }
    return value === undefined || value === null ? '' : String(value);
  }

  function readonlyText(field: FieldMeta): string {
    return formatCell(field, values[field.key]).text;
  }

  function handleSubmit(event: SubmitEvent): void {
    event.preventDefault();
    // 闸门是纯函数（`src/lib/schema-submit.ts`）：校验失败时**根本不会**碰
    // `props.onSubmit` —— 于是也不会发出写请求（AC-S-083 步骤 3 / 5）。
    // 单拉出来的原因见该文件头：SSR 单测点不到按钮，只能测这层。
    errors = gateSubmit(props.meta, values, (body) => props.onSubmit?.(body)).errors;
  }
</script>

<form class="base-schema-form" novalidate onsubmit={handleSubmit}>
  {#each fields as field (field.key)}
    <div class="base-schema-field">
      {#if isReadonly(field)}
        <span class="base-field-label">{field.label}</span>
        <span class="base-schema-readonly" data-field={field.key}>{readonlyText(field)}</span>
      {:else}
        <label class="base-field" for={fieldId(field)}>
          <span class="base-field-label">
            {field.label}
            {#if field.required === true}
              <span class="base-schema-required">必填</span>
            {/if}
          </span>

          {#if field.type === 'boolean'}
            <input
              id={fieldId(field)}
              class="base-schema-checkbox"
              type="checkbox"
              name={field.key}
              checked={values[field.key] === true}
              aria-invalid={errors[field.key] === undefined ? undefined : 'true'}
              aria-describedby={errors[field.key] === undefined ? undefined : errorId(field)}
              onchange={(event) => setValue(field.key, event.currentTarget.checked)}
            />
          {:else if field.type === 'number'}
            <input
              id={fieldId(field)}
              class="base-input"
              type="number"
              step="any"
              name={field.key}
              value={textValue(field)}
              aria-invalid={errors[field.key] === undefined ? undefined : 'true'}
              aria-describedby={errors[field.key] === undefined ? undefined : errorId(field)}
              oninput={(event) => setValue(field.key, event.currentTarget.value)}
            />
          {:else if field.type === 'date'}
            <input
              id={fieldId(field)}
              class="base-input"
              type="date"
              name={field.key}
              value={textValue(field)}
              aria-invalid={errors[field.key] === undefined ? undefined : 'true'}
              aria-describedby={errors[field.key] === undefined ? undefined : errorId(field)}
              oninput={(event) => setValue(field.key, event.currentTarget.value)}
            />
          {:else}
            <!-- text 与未知类型共用文本框：未知类型按 AC-S-087 降级。 -->
            <input
              id={fieldId(field)}
              class="base-input"
              type="text"
              name={field.key}
              value={textValue(field)}
              aria-invalid={errors[field.key] === undefined ? undefined : 'true'}
              aria-describedby={errors[field.key] === undefined ? undefined : errorId(field)}
              oninput={(event) => setValue(field.key, event.currentTarget.value)}
            />
          {/if}

          {#if errors[field.key] !== undefined}
            <span id={errorId(field)} class="base-field-error" role="alert">
              {errors[field.key]}
            </span>
          {/if}
        </label>
      {/if}
    </div>
  {/each}

  <div class="base-schema-form-actions">
    <button
      type="submit"
      class="base-button base-button--primary"
      disabled={props.submitting === true}
    >
      {props.submitLabel ?? (props.mode === 'edit' ? '保存' : '创建')}
    </button>
    {#if props.onCancel !== undefined}
      <button type="button" class="base-button" onclick={() => props.onCancel?.()}>取消</button>
    {/if}
  </div>
</form>
