<!--
  审计查询页（`/admin/audit`，需要 `audit:events:read`）。

  # 守卫在**请求之前**（AC-S-132 步骤 4 / 5）

  路由 `/admin/audit` 声明了权限点 `audit:events:read`：
  守卫不放行 → 地址改写成 `/403` → **路由的 `load` 不会被调用**
  （`router.ts:244-250`）**且**页面不会被渲染
  → `GET /api/audit/events` 的请求计数为 **0**（与既有受保护页同一套机制）。

  # 分页是**服务端游标分页**（D145 / D169 / S-35）

  审计表量大且**持续追加** → 按页码翻页在变动的数据集上必然重复 / 遗漏
  （契约 C7 把游标定为主口径的原话理由）→ **取数一律用游标**
  （`cursor` + `limit`），**不是**客户端切片。

  页码指示来自**游标栈**（与 `SchemaPage.svelte` 同一套机制）：
  `cursors[i]` = 第 `i + 1` 页的入参游标，**页码指示 = 栈长度**。
  ⚠️ 游标分页**算不出**「全量第几页」→ 指示必须如实写成
  「**本次浏览**翻到第几页」，**不能**写「第 N 页 / 共 M 页」（D169）。
  → 自带的页码分页器不渲染（`hidePager`），本页自己画游标分页器。

  # `before` / `after` 是 JSON 文本（后端是 TEXT 列）

  解析后展示；解析失败**优雅降级**（显示原文），**不得**让整页崩掉。

  ⚠️ 脚本块是 `<script lang="ts">`：根 `eslint.config.js` 已给 `**/*.svelte` 配了
  TypeScript 解析器（D97）→ 这里可以写 TS 类型。带参数的事件处理器仍放
  `src/lib/handlers.ts` —— 模板内联箭头函数没有上下文类型。
-->
<script lang="ts">
  import type { ApiClient } from '@base/sdk';
  import { untrack } from 'svelte';

  import Button from '../components/Button.svelte';
  import Input from '../components/Input.svelte';
  import Table from '../components/Table.svelte';

  import {
    AUDIT_DEFAULT_LIMIT,
    AUDIT_PAGE_SIZE_OPTIONS,
    auditCursorOf,
    auditCursorPage,
    auditFilters,
    formatAuditTimestamp,
    loadAuditEvents,
    newAuditCursorStack,
    popAuditCursor,
    pushAuditCursor,
    toAuditRows,
    type AuditCursorStack,
    type AuditFilter,
    type AuditLoadResult,
    type AuditRow,
  } from '../lib/audit-api';
  import { createValueSetter } from '../lib/handlers';
  import { describeError, i18nRuntime } from '../lib/i18n';

  /** 审计页 props。 */
  interface AuditPageProps {
    /** API 客户端（宿主装配，页面不自己造）。 */
    readonly client: ApiClient;
    /** 每页条数（默认 20 —— 契约 C7 的默认 `limit`）。 */
    readonly pageSize?: number;
    /**
     * 首屏数据（由**路由的 `load`** 取）。
     *
     * 守卫不放行时 `load` 不会被调用 → 这里一直是 `undefined` → 页面也不会渲染
     * → 目标接口的请求计数为 **0**。
     */
    readonly initial?: AuditLoadResult;
  }

  const props: AuditPageProps = $props();
  const i18n = i18nRuntime();

  /** 错误文案用当前语言渲染（`describeError` 只负责取键与参数）。 */
  const translate = (key: string, params?: Record<string, string | number>): string =>
    i18n.t(key, params);

  /**
   * 每页条数（**用户可改**：审计表量大，翻页密度要能调）。
   *
   * 初值来自 props（默认 20 —— 契约 C7 的默认 `limit`）；
   * 改了就**重新请求**（页大小进 query，不是前端切片）。
   *
   * `untrack` 是刻意的：这里只取**初值**，props 之后变了也不跟随
   * （用户改过页大小之后，props 不该把它改回去）。
   */
  let pageSize = $state(untrack(() => props.pageSize ?? AUDIT_DEFAULT_LIMIT));

  /** 可选的每页条数（当前值不在档位里时也列出来，避免下拉框显示空值）。 */
  const pageSizeOptions = $derived(
    (AUDIT_PAGE_SIZE_OPTIONS as readonly number[]).includes(pageSize)
      ? [...AUDIT_PAGE_SIZE_OPTIONS]
      : [...AUDIT_PAGE_SIZE_OPTIONS, pageSize].sort((left, right) => left - right),
  );

  /** 当前页的行（**服务端**返回的那一批）。 */
  let rows = $state<AuditRow[]>([]);
  /** 总条数（**服务端**给的）。 */
  let total = $state(0);
  /** 下一页游标（`null` = 没有下一页 → 「下一页」禁用）。 */
  let nextCursor = $state<string | null>(null);
  /**
   * 游标栈：下标 0 = 首页（`null` = 请求里不带 `cursor`）。
   *
   * **页码指示 = 栈长度**（D169）。它表达的是「**本次浏览**翻到第几页」，
   * 不是「全量第几页」—— 游标分页没有全量页码。
   */
  let cursors = $state<AuditCursorStack>(newAuditCursorStack());
  /** 页码指示（本次浏览）。 */
  const pageIndicator = $derived(auditCursorPage(cursors));
  /** 还有上一页（栈里不止首页）。 */
  const hasPrev = $derived(cursors.length > 1);

  let loading = $state(true);
  /** 加载失败提示（空串 = 不显示）。 */
  let error = $state('');

  /** 过滤框里的输入（**未应用**）。 */
  let actorInput = $state('');
  let actionInput = $state('');
  /** 已应用的过滤值（点「筛选」之后才变）。 */
  let appliedActor = $state('');
  let appliedAction = $state('');
  /**
   * 本页是否**自己**发过请求。
   *
   * 首屏数据来自**路由的 `load`**（`props.initial`）——
   * 页面**不**在挂载时自己再取一次（那会发两个重复请求）。
   * 用户先翻了页 / 施加了过滤之后，首屏结果这时才回来的话
   * **不能**覆盖用户已经看到的筛选结果。
   */
  let selfLoaded = false;

  // 列标签是文案，走 `t()`；`ID` 是技术缩写（ASCII），不是翻译文案。
  const columns = $derived([
    { key: 'id', label: 'ID' },
    { key: 'actorId', label: $i18n.t('common.auditActor') },
    {
      key: 'createdAt',
      label: $i18n.t('common.auditTime'),
      // 时间按用户 / 租户时区渲染（契约 C13）；原始值不合法时退回原文。
      render: (row: AuditRow): string =>
        formatAuditTimestamp(row.createdAt, (value) => i18n.formatDate(value)),
    },
    { key: 'action', label: $i18n.t('common.auditAction') },
    { key: 'object', label: $i18n.t('common.auditObject') },
    { key: 'ip', label: $i18n.t('common.auditIp') },
    { key: 'before', label: $i18n.t('common.auditBefore') },
    { key: 'after', label: $i18n.t('common.auditAfter') },
  ]);

  const setActorInput = createValueSetter((value) => {
    actorInput = value;
  });
  const setActionInput = createValueSetter((value) => {
    actionInput = value;
  });

  /** 改每页条数 → 重新请求（页大小进 query，游标栈回到首页）。 */
  const setPageSize = createValueSetter((value) => {
    const next = Number(value);
    if (!Number.isFinite(next) || next < 1) {
      return;
    }
    pageSize = Math.floor(next);
    void load(null, currentFilters());
  });

  /** 当前已应用的过滤条件。 */
  function currentFilters(): readonly AuditFilter[] {
    return auditFilters(appliedActor, appliedAction);
  }

  /**
   * 取一页并替换当前列表（**每次翻页都是一次新的接口请求**）。
   *
   * `cursor` = `null` → 首页（请求里不带 `cursor`）；否则带上该页的入参游标。
   *
   * 失败时清空列表并显示错误码对应的文案 —— 不保留上一页的数据
   * （否则用户会以为「筛选没生效」）。
   */
  async function load(cursor: string | null, filters: readonly AuditFilter[]): Promise<void> {
    selfLoaded = true;
    loading = true;
    try {
      const result = await loadAuditEvents(props.client, {
        cursor: cursor ?? undefined,
        limit: pageSize,
        filters,
      });
      rows = toAuditRows(result.items);
      total = result.total;
      nextCursor = result.nextCursor;
      error = '';
    } catch (caught) {
      rows = [];
      total = 0;
      nextCursor = null;
      error = describeError(caught, translate);
    } finally {
      loading = false;
    }
  }

  /** 「下一页」：把响应的 `nextCursor` **压栈**并请求（D169）。 */
  function handleNextPage(): void {
    if (nextCursor === null) {
      return;
    }
    const cursor = nextCursor;
    cursors = pushAuditCursor(cursors, cursor);
    void load(cursor, currentFilters());
  }

  /** 「上一页」：**弹栈**并用新的栈顶游标请求（D169）。 */
  function handlePrevPage(): void {
    if (!hasPrev) {
      return;
    }
    const stack = popAuditCursor(cursors);
    cursors = stack;
    void load(auditCursorOf(stack), currentFilters());
  }

  /** 应用过滤（过滤条件**下发到后端**，游标栈回到首页）。 */
  function handleApplyFilter(): void {
    appliedActor = actorInput.trim();
    appliedAction = actionInput.trim();
    cursors = newAuditCursorStack();
    void load(null, currentFilters());
  }

  /** 清空过滤并重新加载。 */
  function handleResetFilter(): void {
    actorInput = '';
    actionInput = '';
    appliedActor = '';
    appliedAction = '';
    cursors = newAuditCursorStack();
    void load(null, []);
  }

  /**
   * 首屏数据一到就铺上（**服务端**给的那一批）。
   *
   * `selfLoaded` 为真（用户已经翻页 / 施加过过滤）时**忽略**首屏结果 ——
   * 否则迟到的首屏会把用户正在看的结果盖掉。
   */
  $effect(() => {
    const initial = props.initial;
    if (initial === undefined || selfLoaded) {
      return;
    }
    rows = toAuditRows(initial.items);
    total = initial.total;
    nextCursor = initial.nextCursor;
    cursors = newAuditCursorStack();
    error = initial.failure === undefined ? '' : describeError(initial.failure, translate);
    loading = false;
  });
</script>

<section class="base-audit">
  <h2>{$i18n.t('nav.audit')}</h2>

  <div class="base-schema-toolbar">
    <Input
      label={$i18n.t('common.auditActorFilter')}
      name="audit-actor"
      type="search"
      value={actorInput}
      oninput={setActorInput}
    />
    <Input
      label={$i18n.t('common.auditActionFilter')}
      name="audit-action"
      type="search"
      value={actionInput}
      oninput={setActionInput}
    />
    <Button onclick={handleApplyFilter}>{$i18n.t('common.auditApplyFilter')}</Button>
    <Button onclick={handleResetFilter}>{$i18n.t('common.auditResetFilter')}</Button>
    <label class="base-field" for="audit-page-size">
      <span class="base-field-label">{$i18n.t('common.auditPageSize')}</span>
      <select
        id="audit-page-size"
        class="base-input"
        value={pageSize}
        onchange={(event) => setPageSize(event.currentTarget.value)}
      >
        {#each pageSizeOptions as option (option)}
          <option value={option}>{option}</option>
        {/each}
      </select>
    </label>
  </div>

  {#if error !== ''}
    <p class="base-schema-error" role="alert">{error}</p>
  {/if}

  {#if loading}
    <p class="base-schema-loading">{$i18n.t('common.loading')}</p>
  {/if}

  <Table
    {rows}
    {columns}
    page={pageIndicator}
    {pageSize}
    {total}
    rowKey={(row: AuditRow): string => row.id}
    emptyText={$i18n.t('common.auditEmpty')}
    hidePager
  />

  <!--
    游标分页器（**本页自己画**）：自带的页码分页器是「第 N 页 / 共 M 页」语义，
    而游标分页**没有**全量页码 → 指示必须如实写成「本次浏览第 N 页」（D169）。
  -->
  <nav class="base-table-pager" data-testid="audit-pager">
    <button
      type="button"
      class="base-button"
      disabled={!hasPrev}
      data-testid="audit-prev-page"
      onclick={handlePrevPage}
    >
      {$i18n.t('common.previousPage')}
    </button>
    <span class="base-table-pager-info" data-testid="audit-page-indicator">
      {$i18n.t('common.auditPageIndicator', { page: pageIndicator })}
    </span>
    <button
      type="button"
      class="base-button"
      disabled={nextCursor === null}
      data-testid="audit-next-page"
      onclick={handleNextPage}
    >
      {$i18n.t('common.nextPage')}
    </button>
  </nav>

  <p class="base-schema-empty" data-testid="audit-cursor-hint">
    {$i18n.t('common.auditCursorHint')}
  </p>

  <p class="base-schema-empty" data-testid="audit-total">
    {$i18n.t('common.auditTotal', { count: total })}
  </p>
</section>
