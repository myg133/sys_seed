<!--
  受保护页（`/admin/roles`，需要 `rbac:role:write`）。

  数据由路由的 `load` 加载 —— **守卫放行后**才会被调用。

  # 分页状态为什么在消费方

  `@base/ui` 的表格**只接受 props**（不引 `@base/hooks`，AC-S-075），
  分页状态由这里用 `@base/hooks` 组合后传进去。
-->
<script lang="ts">
  import { createPagination } from '@base/hooks';

  import Table from '../components/Table.svelte';

  import { createPageChangeHandler } from '../lib/handlers';

  const props = $props();

  const columns = [
    { key: 'id', label: 'ID' },
    { key: 'name', label: '名称' },
  ];

  // 分页状态由消费方用 `@base/hooks` 组合，再以 props 传给表格
  // （`@base/ui` 的表格**不持有**分页状态 → 两个包互不依赖）。
  const pagination = createPagination({ pageSize: 10 });
  // hooks 的状态容器是**纯对象**（不绑框架）→ 页码要镜像进 `$state` 才响应式。
  let page = $state(pagination.page);
  const handlePageChange = createPageChangeHandler(
    (next) => {
      pagination.setPage(next);
      page = pagination.page;
    },
    // 在闭包里读 props（不在初始化时取快照）→ 回调换了也能用。
    (next) => props.onPageChange?.(next),
  );
</script>

<section class="base-protected">
  <h2>角色管理</h2>
  <Table
    rows={props.roles}
    {columns}
    {page}
    pageSize={pagination.pageSize}
    total={props.roles.length}
    onPageChange={handlePageChange}
  />
</section>
