<!--
  数据页面（AC-S-085 / 086）—— **一条通配路由**服务所有实体。

  # 零前端代码

  页面不认识任何具体实体：模块 / 实体来自 URL（`/schema/:moduleId/:entityKey`），
  字段来自 `GET /api/schema/<module>/<entity>`，数据接口来自元数据的 `api` 段（D102）。
  新增实体 = 后端 manifest 加一段 `schema` → 这里**一行不改**。

  # 分页是游标分页

  响应形状 `{ items, nextCursor }`，**没有 `total`**。上一页靠本页维护的游标栈
  （`cursors` + `pageIndex`）—— 游标分页不能像页码分页那样直接跳到第 N 页。
-->
<script lang="ts">
  import { DEFAULT_PAGE_SIZE, recordId, type EntityMeta } from '@base/schema';
  import type { ApiClient } from '@base/sdk';

  import Dialog from '../components/Dialog.svelte';
  import SchemaForm from '../components/SchemaForm.svelte';
  import SchemaTable from '../components/SchemaTable.svelte';

  import {
    createRecord,
    deleteRecord,
    describeError,
    loadEntityMeta,
    loadPage,
    updateRecord,
  } from '../lib/schema-api';

  /** 组件 props（D97：必须有类型）。 */
  interface SchemaPageProps {
    /** API 客户端（宿主装配，页面不自己造）。 */
    client: ApiClient;
    /** URL 上的模块标识。 */
    moduleId: string;
    /** URL 上的实体键。 */
    entityKey: string;
    /** 每页条数（默认 20）。 */
    pageSize?: number;
  }

  const props: SchemaPageProps = $props();

  const pageSize = $derived(props.pageSize ?? DEFAULT_PAGE_SIZE);

  let meta = $state<EntityMeta | undefined>(undefined);
  let items = $state<Record<string, unknown>[]>([]);
  let nextCursor = $state<string | null>(null);
  /** 游标栈：下标 0 = 首页（无游标）。 */
  let cursors = $state<(string | null)[]>([null]);
  let pageIndex = $state(0);

  let loading = $state(true);
  /** 列表 / 元数据加载失败（表格区域显示）。 */
  let loadError = $state('');
  /** 写操作失败（表单区域显示，不把已渲染的表格顶掉）。 */
  let actionError = $state('');

  let submitting = $state(false);
  let formOpen = $state(false);
  let formRecord = $state<Record<string, unknown> | undefined>(undefined);
  let pendingDelete = $state<Record<string, unknown> | undefined>(undefined);

  /** 已经加载过的 `module/entity`（避免同一页面重复拉元数据）。 */
  let loadedKey = '';

  $effect(() => {
    const key = `${props.moduleId}/${props.entityKey}`;
    if (key === loadedKey) {
      return;
    }
    loadedKey = key;
    void bootstrap();
  });

  async function bootstrap(): Promise<void> {
    loading = true;
    loadError = '';
    actionError = '';
    formOpen = false;
    formRecord = undefined;
    try {
      const loaded = await loadEntityMeta(props.client, props.moduleId, props.entityKey);
      meta = loaded;
      cursors = [null];
      pageIndex = 0;
      await refresh(null);
    } catch (error) {
      meta = undefined;
      items = [];
      nextCursor = null;
      loadError = describeError(error);
    } finally {
      loading = false;
    }
  }

  /** 取一页并替换当前列表（游标 `null` = 首页）。 */
  async function refresh(cursor: string | null): Promise<void> {
    if (meta === undefined) {
      return;
    }
    loading = true;
    try {
      const page = await loadPage(props.client, meta.api, cursor, pageSize);
      items = page.items;
      nextCursor = page.nextCursor;
      loadError = '';
    } catch (error) {
      items = [];
      nextCursor = null;
      loadError = describeError(error);
    } finally {
      loading = false;
    }
  }

  function openCreate(): void {
    formRecord = undefined;
    formOpen = true;
    actionError = '';
  }

  function openEdit(record: Record<string, unknown>): void {
    formRecord = record;
    formOpen = true;
    actionError = '';
  }

  function closeForm(): void {
    formOpen = false;
    formRecord = undefined;
  }

  async function submitForm(body: Record<string, unknown>): Promise<void> {
    if (meta === undefined) {
      return;
    }
    submitting = true;
    actionError = '';
    try {
      if (formRecord === undefined) {
        await createRecord(props.client, meta.api, body);
      } else {
        await updateRecord(props.client, meta.api, recordId(formRecord), body);
      }
      closeForm();
      await refresh(cursors[pageIndex] ?? null);
    } catch (error) {
      actionError = describeError(error);
    } finally {
      submitting = false;
    }
  }

  async function confirmDelete(): Promise<void> {
    if (meta === undefined || pendingDelete === undefined) {
      return;
    }
    const record = pendingDelete;
    pendingDelete = undefined;
    actionError = '';
    try {
      await deleteRecord(props.client, meta.api, record);
      await refresh(cursors[pageIndex] ?? null);
    } catch (error) {
      actionError = describeError(error);
    }
  }

  function goNext(): void {
    if (nextCursor === null) {
      return;
    }
    const cursor = nextCursor;
    cursors = [...cursors.slice(0, pageIndex + 1), cursor];
    pageIndex += 1;
    void refresh(cursor);
  }

  function goPrev(): void {
    if (pageIndex === 0) {
      return;
    }
    pageIndex -= 1;
    void refresh(cursors[pageIndex] ?? null);
  }
</script>

<section class="base-schema-page">
  <h2>{meta?.label !== undefined && meta.label !== '' ? meta.label : props.entityKey}</h2>

  {#if actionError !== ''}
    <p class="base-schema-error" role="alert">{actionError}</p>
  {/if}

  {#if formOpen && meta !== undefined}
    {#key formRecord === undefined ? 'create' : recordId(formRecord)}
      <SchemaForm
        {meta}
        record={formRecord}
        mode={formRecord === undefined ? 'create' : 'edit'}
        {submitting}
        onSubmit={submitForm}
        onCancel={closeForm}
      />
    {/key}
  {/if}

  {#if meta !== undefined}
    <SchemaTable
      {meta}
      {items}
      {nextCursor}
      hasPrev={pageIndex > 0}
      {loading}
      error={loadError === '' ? undefined : loadError}
      onCreate={openCreate}
      onEdit={openEdit}
      onDelete={(record) => {
        pendingDelete = record;
      }}
      onPrevPage={goPrev}
      onNextPage={goNext}
    />
  {:else if loading}
    <p class="base-schema-loading">加载中…</p>
  {:else if loadError !== ''}
    <p class="base-schema-error" role="alert">{loadError}</p>
  {/if}
</section>

<Dialog
  open={pendingDelete !== undefined}
  title="删除确认"
  onclose={() => {
    pendingDelete = undefined;
  }}
>
  <p>确定删除该记录？</p>
  <div class="base-schema-form-actions">
    <button
      type="button"
      class="base-button"
      onclick={() => {
        pendingDelete = undefined;
      }}
    >
      取消
    </button>
    <button
      type="button"
      class="base-button base-button--danger"
      onclick={() => void confirmDelete()}
    >
      确认
    </button>
  </div>
</Dialog>
