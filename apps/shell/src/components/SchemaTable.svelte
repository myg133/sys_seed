<!--
  元数据驱动的列表（AC-S-082 / 084 / 087）。

  # 为什么**不复用** `@base/ui` 的 Table（D106）

  `Table.svelte` **恒渲染 `thead`**、空态**渲染一行**、分页需要 `total`；
  本组件要「空态不渲染空壳表」+ **游标分页（没有 total）** —— 语义不同。
  改 Table 会动 REQ-011 已通过的契约（AC-S-075 的双向断言）。

  # 表头 = 元数据字段的 label（顺序即契约）

  `th` 的文本数组必须**精确等于**元数据 `fields` 的 label 数组。
  所以行操作列**不生成 `th`**：它没有字段标签，生成一个空 `th` 会让表头
  多出一列（AC-S-082 步骤 2 的可判定性就没了）。

  # 未知类型：降级 + 一条告警（AC-S-087）

  一个未识别的 `type` 不该让整页白屏 —— 降级为纯文本，`console.warn` 指名字段与类型
  （每个字段实例只告警一次，不是每行一次）。
-->
<script lang="ts">
  import {
    formatCell,
    isKnownFieldType,
    unknownTypeWarning,
    visibleFields,
    type EntityMeta,
  } from '@base/schema';

  import { i18nRuntime } from '../lib/i18n';

  /** 组件 props（D97：必须有类型，且**整体取值**而不是解构）。 */
  interface SchemaTableProps {
    /** 实体元数据（后端已按权限过滤）。 */
    meta: EntityMeta;
    /** 当前页记录。 */
    items: readonly Record<string, unknown>[];
    /** 下一页游标；`null` = 没有下一页（「下一页」按钮禁用）。 */
    nextCursor?: string | null;
    /** 是否还有上一页（游标分页靠消费方的游标栈）。 */
    hasPrev?: boolean;
    /** 首次加载中。 */
    loading?: boolean;
    /** 加载失败文案（有值时只渲染它，不渲染表格）。 */
    error?: string;
    /** 空态文案（默认「暂无数据」）。 */
    emptyText?: string;
    /** 新建（只有元数据声明了 `api.create` 才渲染按钮）。 */
    onCreate?: () => void;
    /** 编辑（只有元数据声明了 `api.update` 才渲染按钮）。 */
    onEdit?: (record: Record<string, unknown>) => void;
    /** 删除（只有元数据声明了 `api.delete` 才渲染按钮）。 */
    onDelete?: (record: Record<string, unknown>) => void;
    /** 上一页。 */
    onPrevPage?: () => void;
    /** 下一页。 */
    onNextPage?: () => void;
  }

  const props: SchemaTableProps = $props();
  const i18n = i18nRuntime();

  const fields = $derived(visibleFields(props.meta));
  const canCreate = $derived(props.onCreate !== undefined && props.meta.api.create !== '');
  const canEdit = $derived(props.onEdit !== undefined && props.meta.api.update !== undefined);
  const canDelete = $derived(props.onDelete !== undefined && props.meta.api.delete !== undefined);
  const hasActions = $derived(canEdit || canDelete);
  const canNext = $derived(typeof props.nextCursor === 'string' && props.nextCursor !== '');

  // 未知类型：每个字段告警**一次**（整页不崩，问题仍可发现）。
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
</script>

<section class="base-schema-table">
  {#if props.error}
    <p class="base-schema-error" role="alert">{props.error}</p>
  {:else}
    {#if canCreate}
      <div class="base-schema-toolbar">
        <button
          type="button"
          class="base-button base-button--primary"
          onclick={() => props.onCreate?.()}
        >
          {$i18n.t('common.newRecord')}
        </button>
      </div>
    {/if}

    {#if props.loading}
      <p class="base-schema-loading">{$i18n.t('common.loading')}</p>
    {:else if props.items.length === 0}
      <!-- 空态：**不渲染表头空壳**（AC-S-082 步骤 5）。 -->
      <p class="base-schema-empty">{props.emptyText ?? $i18n.t('schema.emptyTable')}</p>
    {:else}
      <table class="base-table base-schema-grid">
        <thead>
          <tr>
            {#each fields as field (field.key)}
              <th scope="col">{field.label}</th>
            {/each}
            {#if hasActions}
              <!--
                行操作列没有字段标签，但**必须**是 `<th scope="col">`：
                `<tr>` 的 content model 只允许 th/td 混排时保持「表头单元格」语义 ——
                在 `<thead><tr>` 里放 `<td>` 违反 HTML content model，
                读屏软件会把它当成数据单元格（L-4）。
                列本身没有文案，所以留空；断言表头文案的用例按
                `th:not(.base-schema-actions)` 取值（AC-S-082 步骤 2：th 文本与顺序 = 元数据 label）。
              -->
              <th scope="col" class="base-schema-actions"></th>
            {/if}
          </tr>
        </thead>
        <tbody>
          {#each props.items as item, index (item['id'] ?? index)}
            <tr>
              {#each fields as field (field.key)}
                {@const cell = formatCell(field, item[field.key])}
                <td>{cell.text}</td>
              {/each}
              {#if hasActions}
                <td class="base-schema-actions">
                  {#if canEdit}
                    <button type="button" class="base-button" onclick={() => props.onEdit?.(item)}>
                      {$i18n.t('common.edit')}
                    </button>
                  {/if}
                  {#if canDelete}
                    <button
                      type="button"
                      class="base-button base-button--danger"
                      onclick={() => props.onDelete?.(item)}
                    >
                      {$i18n.t('common.delete')}
                    </button>
                  {/if}
                </td>
              {/if}
            </tr>
          {/each}
        </tbody>
      </table>

      <nav class="base-table-pager">
        <button
          type="button"
          class="base-button"
          disabled={props.hasPrev !== true}
          onclick={() => props.onPrevPage?.()}
        >
          {$i18n.t('common.previousPage')}
        </button>
        <button
          type="button"
          class="base-button"
          disabled={!canNext}
          onclick={() => props.onNextPage?.()}
        >
          {$i18n.t('common.nextPage')}
        </button>
      </nav>
    {/if}
  {/if}
</section>
