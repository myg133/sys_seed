<!--
  表格（薄封装）—— 逻辑与样式契约在 `@base/ui`。

  # 表格只接受 props

  分页 / 排序状态**不由表格持有**：消费方用 `@base/hooks` 的状态容器组合后
  以 props 传进来（`rows` / `columns` / `page` / `pageSize` / `total` / `onPageChange`）。
  这样 `@base/ui` 不需要依赖 `@base/hooks`（AC-S-075）。
-->
<script lang="ts" generics="Row">
  import { cellValue, pageInfo, rowKeyOf, tableClass, type TableProps } from '@base/ui';

  import { i18nRuntime } from '../lib/i18n';

  // props 整体取值：svelte-check 会把「解构出来的可选 props」当成必填，
  // 消费方就得把每个可选值都传一遍 —— 契约反而没法用。
  const props: TableProps<Row> = $props();
  const i18n = i18nRuntime();

  // `total` 没给（或给了 0）就退回当前页的行数。
  const total = $derived(
    props.total !== undefined && props.total > 0 ? props.total : props.rows.length,
  );

  const info = $derived(pageInfo(props.page ?? 1, props.pageSize ?? 10, total));
</script>

<table class={tableClass({ class: props.class })}>
  <thead>
    <tr>
      {#each props.columns as column (column.key)}
        <th style={column.align === undefined ? undefined : `text-align: ${column.align}`}>
          {column.label}
        </th>
      {/each}
    </tr>
  </thead>
  <tbody>
    {#each props.rows as row, index (rowKeyOf(row, index, props.rowKey))}
      <tr>
        {#each props.columns as column (column.key)}
          <td style={column.align === undefined ? undefined : `text-align: ${column.align}`}>
            {cellValue(row, column)}
          </td>
        {/each}
      </tr>
    {:else}
      <tr>
        <td colspan={props.columns.length}>{props.emptyText ?? $i18n.t('schema.emptyTable')}</td>
      </tr>
    {/each}
  </tbody>
</table>

{#if info.pageCount > 1}
  <nav class="base-table-pager">
    <button
      type="button"
      disabled={info.page <= 1}
      onclick={() => props.onPageChange?.(info.page - 1)}
    >
      {$i18n.t('common.previousPage')}
    </button>
    <span class="base-table-pager-info">{info.page} / {info.pageCount}</span>
    <button
      type="button"
      disabled={info.page >= info.pageCount}
      onclick={() => props.onPageChange?.(info.page + 1)}
    >
      {$i18n.t('common.nextPage')}
    </button>
  </nav>
{/if}
