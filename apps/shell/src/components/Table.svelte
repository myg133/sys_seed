<!--
  表格（薄封装）—— 逻辑与样式契约在 `@base/ui`。

  # 表格只接受 props

  分页 / 排序状态**不由表格持有**：消费方用 `@base/hooks` 的状态容器组合后
  以 props 传进来（`rows` / `columns` / `page` / `pageSize` / `total` / `onPageChange`）。
  这样 `@base/ui` 不需要依赖 `@base/hooks`（AC-S-075）。
-->
<script lang="ts">
  import { cellValue, pageInfo, rowKeyOf, tableClass } from '@base/ui';

  const props = $props();

  const info = $derived(
    pageInfo(
      props.page ?? 1,
      props.pageSize ?? 10,
      (props.total ?? 0) > 0 ? props.total : props.rows.length,
    ),
  );
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
        <td colspan={props.columns.length}>{props.emptyText ?? '暂无数据'}</td>
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
      上一页
    </button>
    <span class="base-table-pager-info">{info.page} / {info.pageCount}</span>
    <button
      type="button"
      disabled={info.page >= info.pageCount}
      onclick={() => props.onPageChange?.(info.page + 1)}
    >
      下一页
    </button>
  </nav>
{/if}
