<!--
  请假审批页（REQ-020 / AC-S-192 · D356）。

  # 为什么是**自定义页面**，不是通配的 schema 页
  通配页（`/schema/:moduleId/:entityKey`）的动作列只有编辑 / 删除，
  manifest 的 `api` 只有 `list` / `create` / `update` / `delete` 四个键
  → **没有「审批」这类行内动作**（TD-087 / D356）。
  所以带审批流的业务必须写自定义页面 + 在 shell 加一条硬编码路由（C 类改动）。

  # 审批**必须**走 `POST .../decide`（红线）
  页面**没有**任何「把 `status` 改成 approved」的表单字段：直接改状态会绕过
  业务状态机与 **409 `LEAVE_INVALID_TRANSITION`** 校验（AC-S-189 的安全判据被 UI 旁路）。
  唯一的审批出口是 `decideLeaveRequest`（`lib/leave-api.ts`）。

  # 复用了什么、没复用什么
  - **复用** `@base/ui` 的样式契约：`buttonClass` / `inputClass` / `tableClass`
    （与 shell 的 Button / Input / Table 组件同一套类名，视觉一致）；
  - **复用** `@base/auth` 的 `can()` 做按钮可见性判定（与后端同一份权限点）；
  - **没复用** shell 的 `Button.svelte` / `Input.svelte` / `Table.svelte`：
    那些组件在 `apps/shell/src/components/` 里，业务模块 import shell 的内部组件
    会让依赖方向反过来（模块 → 宿主），也会把页面绑死在宿主目录结构上；
  - **没复用** `@base/schema` 的 `SchemaForm` / `SchemaTable` 对应能力：
    它们吃的是后端 manifest 的 `EntityMeta`（`api` 4 个键、字段类型闭集），
    为请假单伪造一份 `EntityMeta` 只是把「没有审批动作」这个缺口藏起来；
  - **文案硬编码中文**：`packages/i18n` 的 domain 是闭集
    （`error` / `common` / `schema` / `nav` / `oauth` / `app`，见该包的键格式测试），
    **没有模块级命名空间** → 按脚手架范式（`modules/sandbox/frontend/pages/DemoPage.svelte`
    同样硬编码中文）处理。这是底座缺口，已写进度度报告。

  ⚠️ 脚本块是 `<script lang="ts">`：根 `eslint.config.js` 已给 `.svelte` 配了 TS 解析器。
-->
<script lang="ts">
  import { can, type AuthState } from '@base/auth';
  import { ApiError, type ApiClient } from '@base/sdk';
  import { buttonClass, inputClass, tableClass } from '@base/ui';
  import { onMount, untrack } from 'svelte';

  import {
    createLeaveRequest,
    decideLeaveRequest,
    LEAVE_PERMISSION_APPROVE,
    LEAVE_PERMISSION_WRITE,
    LEAVE_STATUS_LABELS,
    listLeaveRequests,
    submitLeaveRequest,
    validateNewLeave,
    type LeaveDecision,
    type LeaveRequest,
  } from '../lib/leave-api';

  /** 页面 props（宿主装配，页面不自己造客户端）。 */
  interface LeavePageProps {
    /** API 客户端。 */
    readonly client: ApiClient;
    /** 当前登录态（`can()` 用它判审批 / 建单按钮可见性）。 */
    readonly state: AuthState;
    /**
     * 首屏行（可选）。
     *
     * 浏览器里不传 —— 页面挂载时自己取（同一页要能反复刷新）；
     * 单测（SSR 渲染，没有 `onMount`）传它才能验「有 `pending` 行时的按钮可见性」。
     */
    readonly initial?: readonly LeaveRequest[];
  }

  const props: LeavePageProps = $props();

  /** 列表行（服务端给的那一批）。 */
  let rows = $state<LeaveRequest[]>(
    // `untrack` 是刻意的：只取**初值**（与 `Audit.svelte` 同一款写法）——
    // 浏览器里 props 没有 `initial`，数据由 `onMount` 自己取。
    untrack(() => (props.initial === undefined ? [] : [...props.initial])),
  );
  /** 首屏 / 刷新中的加载态（有 `initial` = 已经有数据 → 不算加载中）。 */
  let loading = $state(untrack(() => props.initial === undefined));
  /** 有请求在飞（建单 / 提交 / 审批）→ 按钮禁用，避免重复提交。 */
  let busy = $state(false);
  /** 错误提示（空串 = 不显示）。 */
  let error = $state('');

  /** 新建表单是否展开。 */
  let formOpen = $state(false);
  /** 表单字段（**没有** `status` —— 状态只能由端点推进）。 */
  let startsOn = $state('');
  let endsOn = $state('');
  let reason = $state('');

  /** 有建单 / 提交权限。 */
  const canWrite = $derived(can(props.state, LEAVE_PERMISSION_WRITE));
  /** 有审批权限（决定「通过 / 驳回」按钮是否可见）。 */
  const canApprove = $derived(can(props.state, LEAVE_PERMISSION_APPROVE));

  /** 把异常转成可读文案（不把内部错误码藏起来，也不把英文堆到界面上）。 */
  function describe(caught: unknown): string {
    if (caught instanceof ApiError) {
      if (caught.code === 'LEAVE_INVALID_TRANSITION') {
        return '当前状态不允许该操作（列表可能已过期，请刷新后重试）';
      }
      return `请求失败：${caught.code}（HTTP ${caught.status}）`;
    }
    return '请求失败，请稍后重试';
  }

  /**
   * 取列表。
   *
   * 失败时**清空列表并显示原因**（不保留旧数据 —— 否则用户会以为操作生效了）。
   */
  async function load(): Promise<void> {
    loading = true;
    try {
      rows = await listLeaveRequests(props.client);
      error = '';
    } catch (caught) {
      rows = [];
      error = describe(caught);
    } finally {
      loading = false;
    }
  }

  /** 打开 / 关闭新建表单（关闭时清空字段，避免上一单的残留被再次提交）。 */
  function toggleForm(): void {
    formOpen = !formOpen;
    if (!formOpen) {
      startsOn = '';
      endsOn = '';
      reason = '';
    }
  }

  /**
   * 建单并提交（`POST /requests` → `POST /requests/{id}/submit`）。
   *
   * 两个**真实端点**分两步调：建单只做状态初始化（`draft`），提交才进 `pending`。
   * 任一步失败都如实显示；建单成功而提交失败时，`draft` 会留在列表里，
   * 可用该行的「提交」补上。
   */
  async function handleCreate(): Promise<void> {
    const input = { startsOn: startsOn.trim(), endsOn: endsOn.trim(), reason: reason.trim() };
    const invalid = validateNewLeave(input);
    if (invalid !== '') {
      error = invalid;
      return;
    }
    busy = true;
    try {
      const created = await createLeaveRequest(props.client, input);
      if (created === undefined) {
        error = '建单失败：响应形状无法解析';
        return;
      }
      await submitLeaveRequest(props.client, created.id);
      formOpen = false;
      startsOn = '';
      endsOn = '';
      reason = '';
      error = '';
    } catch (caught) {
      error = describe(caught);
    } finally {
      busy = false;
      await load();
    }
  }

  /** 提交一张草稿（`draft → pending`）。 */
  async function handleSubmit(row: LeaveRequest): Promise<void> {
    busy = true;
    try {
      await submitLeaveRequest(props.client, row.id);
      error = '';
    } catch (caught) {
      error = describe(caught);
    } finally {
      busy = false;
      await load();
    }
  }

  /** 审批（通过 / 驳回）—— **只**走 `decide` 端点。 */
  async function handleDecide(row: LeaveRequest, decision: LeaveDecision): Promise<void> {
    busy = true;
    try {
      await decideLeaveRequest(props.client, row.id, decision);
      error = '';
    } catch (caught) {
      error = describe(caught);
    } finally {
      busy = false;
      await load();
    }
  }

  onMount(() => {
    void load();
  });
</script>

<section class="base-card" data-testid="leave-page">
  <header class="base-card-header">
    <h2>请假审批</h2>
  </header>

  <div class="base-schema-toolbar">
    {#if canWrite}
      <button
        class={buttonClass({ variant: 'primary' })}
        type="button"
        data-testid="leave-new"
        disabled={busy}
        onclick={toggleForm}
      >
        {formOpen ? '收起表单' : '新建'}
      </button>
    {/if}
    <button
      class={buttonClass({ variant: 'secondary' })}
      type="button"
      data-testid="leave-refresh"
      disabled={busy || loading}
      onclick={() => void load()}
    >
      刷新
    </button>
  </div>

  {#if error !== ''}
    <p class="base-schema-error" role="alert" data-testid="leave-error">{error}</p>
  {/if}

  {#if formOpen}
    <!-- 表单里**没有** status 字段：状态只能由 submit / decide 端点推进（D356）。 -->
    <div class="base-schema-form" data-testid="leave-form">
      <label class="base-field" for="leave-starts-on">
        <span class="base-field-label">开始日期</span>
        <input
          id="leave-starts-on"
          class={inputClass()}
          type="date"
          data-testid="leave-starts-on"
          bind:value={startsOn}
        />
      </label>
      <label class="base-field" for="leave-ends-on">
        <span class="base-field-label">结束日期</span>
        <input
          id="leave-ends-on"
          class={inputClass()}
          type="date"
          data-testid="leave-ends-on"
          bind:value={endsOn}
        />
      </label>
      <label class="base-field" for="leave-reason">
        <span class="base-field-label">请假理由</span>
        <input
          id="leave-reason"
          class={inputClass()}
          type="text"
          data-testid="leave-reason"
          bind:value={reason}
        />
      </label>
      <div class="base-schema-form-actions">
        <button
          class={buttonClass({ variant: 'primary' })}
          type="button"
          data-testid="leave-create"
          disabled={busy}
          onclick={() => void handleCreate()}
        >
          提交申请
        </button>
        <button
          class={buttonClass({ variant: 'secondary' })}
          type="button"
          data-testid="leave-cancel"
          disabled={busy}
          onclick={toggleForm}
        >
          取消
        </button>
      </div>
    </div>
  {/if}

  {#if loading}
    <p class="base-schema-loading" data-testid="leave-loading">加载中…</p>
  {/if}

  <table class={tableClass()} data-testid="leave-table">
    <thead>
      <tr>
        <th>申请人</th>
        <th>请假时间</th>
        <th>理由</th>
        <th>状态</th>
        <th>操作</th>
      </tr>
    </thead>
    <tbody>
      {#each rows as row (row.id)}
        <tr data-testid="leave-row" data-status={row.status}>
          <td data-testid="leave-applicant">{row.applicantId}</td>
          <td data-testid="leave-range">{row.startsOn} ~ {row.endsOn}</td>
          <td data-testid="leave-reason-cell">{row.reason}</td>
          <td data-testid="leave-status">{LEAVE_STATUS_LABELS[row.status]}</td>
          <td class="base-schema-actions">
            {#if canWrite && row.status === 'draft'}
              <button
                class={buttonClass({ variant: 'secondary', size: 'small' })}
                type="button"
                data-testid="leave-submit"
                disabled={busy}
                onclick={() => void handleSubmit(row)}
              >
                提交
              </button>
            {/if}
            {#if canApprove && row.status === 'pending'}
              <button
                class={buttonClass({ variant: 'primary', size: 'small' })}
                type="button"
                data-testid="leave-approve"
                disabled={busy}
                onclick={() => void handleDecide(row, 'approve')}
              >
                通过
              </button>
              <button
                class={buttonClass({ variant: 'danger', size: 'small' })}
                type="button"
                data-testid="leave-reject"
                disabled={busy}
                onclick={() => void handleDecide(row, 'reject')}
              >
                驳回
              </button>
            {/if}
          </td>
        </tr>
      {:else}
        <tr>
          <td class="base-schema-empty" colspan="5" data-testid="leave-empty">暂无请假单</td>
        </tr>
      {/each}
    </tbody>
  </table>
</section>
