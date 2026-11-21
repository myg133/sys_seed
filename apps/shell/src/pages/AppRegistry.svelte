<!--
  应用注册表管理（`/admin/apps`，需要 `app:registry:read`）。

  # 权限点与守卫（与 `/admin/audit` 同一套机制）

  路由声明 `app:registry:read` → 守卫不放行 → 地址改写成 `/403`、
  **路由的 `load` 不会被调用**（`router.ts:244-250`）→ `GET /api/apps` 请求计数为 **0**。
  写 / 删的按钮另外用 `<Can>` 包住（`app:registry:write` / `app:registry:delete`）——
  **无权限时不进 DOM**，不是 `display:none`。

  # 清空 `launch_url` 必须走专用端点（D283 / S-08）

  `Option<String>` 下 `PATCH` 的 `null` 与「省略键」在 serde 里**等价**
  → 显式置空只能用 `DELETE /api/apps/{appId}/launch-url`（**204**）。
  表单里把入口地址留空**不等于**清空：留空 = 不发这个键（保持原值）。

  # 删除不动 client（D262 / D264）

  `platform_app.client_id` 是外键、`ON DELETE CASCADE` 是 **client → app** 方向：
  删应用不会影响它引用的 client。所以确认文案必须写清这一点，
  否则管理员会以为「删应用 = 删 client」而不敢点。
-->
<script lang="ts">
  import type { ApiClient } from '@base/sdk';
  import { onMount, untrack } from 'svelte';

  import Button from '../components/Button.svelte';
  import Can from '../components/Can.svelte';
  import Input from '../components/Input.svelte';

  import {
    APP_EMPTY_VALUE,
    APP_REGISTRY_DELETE_PERMISSION,
    APP_REGISTRY_WRITE_PERMISSION,
    appStatusKey,
    clearAppLaunchUrl,
    createApp,
    deleteApp,
    listApps,
    toAppRows,
    updateApp,
    type AppInput,
    type AppRow,
    type AppView,
  } from '../lib/app-api';
  import { createValueSetter } from '../lib/handlers';
  import { describeError, i18nRuntime } from '../lib/i18n';
  import type { AppsLoadResult } from '../lib/routes';

  /** 注册表页 props。 */
  interface AppRegistryProps {
    /** API 客户端（宿主装配，页面不自己造）。 */
    readonly client: ApiClient;
    /**
     * 首屏数据（由**路由的 `load`** 取）。
     *
     * 守卫不放行时 `load` 不会被调用 → 这里一直是 `undefined` → 页面也不会渲染
     * → `GET /api/apps` 的请求计数为 **0**。
     */
    readonly initial?: AppsLoadResult;
    /** 是否在挂载时自己取数（宿主走路由 `load` 时传 `false` 避免重复请求）。 */
    readonly autoLoad?: boolean;
  }

  const props: AppRegistryProps = $props();
  const i18n = i18nRuntime();

  const translate = (key: string, params?: Record<string, string | number>): string =>
    i18n.t(key, params);

  /** 首屏（只取初值；之后由本页的写操作刷新）。 */
  const initial = untrack(() => props.initial);

  let apps = $state<readonly AppView[]>(initial?.apps ?? []);
  let rows = $state<AppRow[]>(toAppRows(initial?.apps ?? []));
  let loading = $state(initial === undefined);
  /** 列表加载失败。 */
  let loadError = $state(
    initial?.failure === undefined ? '' : describeError(initial.failure, translate),
  );
  /** 写操作失败（不把已渲染的列表顶掉）。 */
  let actionError = $state('');
  /** 正在提交（防重复点击）。 */
  let submitting = $state(false);

  /** 表单：空串 = 没填（不发该键）。 */
  let editingId = $state('');
  let clientId = $state('');
  let displayName = $state('');
  let description = $state('');
  let category = $state('');
  let iconUrl = $state('');
  let launchUrl = $state('');
  let sortOrder = $state('');

  // 列标签是文案，走 `t()`；`ID` 是技术缩写（ASCII），不是翻译文案。
  const columns = $derived([
    { key: 'appId', label: 'ID' },
    { key: 'displayName', label: $i18n.t('common.name') },
    { key: 'clientId', label: $i18n.t('app.registryColClient') },
    { key: 'category', label: $i18n.t('app.fieldCategory') },
    { key: 'status', label: $i18n.t('app.registryColStatus') },
    { key: 'sortOrder', label: $i18n.t('app.fieldSortOrder') },
    { key: 'launchUrl', label: $i18n.t('app.fieldLaunchUrl') },
  ]);

  const setClientId = createValueSetter((value) => {
    clientId = value;
  });
  const setDisplayName = createValueSetter((value) => {
    displayName = value;
  });
  const setDescription = createValueSetter((value) => {
    description = value;
  });
  const setCategory = createValueSetter((value) => {
    category = value;
  });
  const setIconUrl = createValueSetter((value) => {
    iconUrl = value;
  });
  const setLaunchUrl = createValueSetter((value) => {
    launchUrl = value;
  });
  const setSortOrder = createValueSetter((value) => {
    sortOrder = value;
  });

  /** 状态文案（未知值**逐字**显示 —— 后端加了新状态也不显示空白）。 */
  function statusText(status: string): string {
    const key = appStatusKey(status);
    return key === undefined ? status : i18n.t(key);
  }

  /** 清空表单（回到「注册新应用」）。 */
  function resetForm(): void {
    editingId = '';
    clientId = '';
    displayName = '';
    description = '';
    category = '';
    iconUrl = '';
    launchUrl = '';
    sortOrder = '';
  }

  /** 把一条应用填进表单（「编辑」）。 */
  function fillForm(app: AppView): void {
    editingId = app.appId;
    clientId = app.clientId;
    displayName = app.displayName;
    description = app.description ?? '';
    category = app.category;
    iconUrl = app.iconUrl ?? '';
    launchUrl = app.launchUrl ?? '';
    sortOrder = String(app.sortOrder);
  }

  /** 按表单造输入（**只带非空串**；`sortOrder` 是数字，空串 = 不发）。 */
  function currentInput(): AppInput {
    const input: {
      -readonly [K in keyof AppInput]: AppInput[K];
    } = {};
    if (clientId !== '') {
      input.clientId = clientId;
    }
    if (displayName !== '') {
      input.displayName = displayName;
    }
    if (description !== '') {
      input.description = description;
    }
    if (category !== '') {
      input.category = category;
    }
    if (iconUrl !== '') {
      input.iconUrl = iconUrl;
    }
    if (launchUrl !== '') {
      input.launchUrl = launchUrl;
    }
    const order = Number(sortOrder);
    if (sortOrder.trim() !== '' && Number.isFinite(order)) {
      input.sortOrder = Math.trunc(order);
    }
    return input;
  }

  /** 重新取列表。 */
  async function reload(): Promise<void> {
    loading = true;
    try {
      apps = await listApps(props.client);
      rows = toAppRows(apps);
      loadError = '';
    } catch (caught) {
      apps = [];
      rows = [];
      loadError = describeError(caught, translate);
    } finally {
      loading = false;
    }
  }

  /** 提交表单：有 `editingId` → `PATCH`，否则 → `POST`（201）。 */
  async function handleSubmit(): Promise<void> {
    if (submitting) {
      return;
    }
    submitting = true;
    actionError = '';
    try {
      const input = currentInput();
      if (editingId === '') {
        await createApp(props.client, input);
      } else {
        await updateApp(props.client, editingId, input);
      }
      resetForm();
      await reload();
    } catch (caught) {
      actionError = describeError(caught, translate);
    } finally {
      submitting = false;
    }
  }

  /** 删除应用（**不动 client** —— D262 / D264）。 */
  async function handleDelete(appId: string): Promise<void> {
    if (submitting) {
      return;
    }
    submitting = true;
    actionError = '';
    try {
      await deleteApp(props.client, appId);
      if (editingId === appId) {
        resetForm();
      }
      await reload();
    } catch (caught) {
      actionError = describeError(caught, translate);
    } finally {
      submitting = false;
    }
  }

  /**
   * 把 `launch_url` 置 NULL（**专用端点**，204 —— D283 / S-08）。
   *
   * ⚠️ **不是** `PATCH { launchUrl: null }`：serde 分不开 `null` 与「省略键」。
   */
  async function handleClearLaunch(appId: string): Promise<void> {
    if (submitting) {
      return;
    }
    submitting = true;
    actionError = '';
    try {
      await clearAppLaunchUrl(props.client, appId);
      if (editingId === appId) {
        launchUrl = '';
      }
      await reload();
    } catch (caught) {
      actionError = describeError(caught, translate);
    } finally {
      submitting = false;
    }
  }

  /** 首屏是否需要自己取（宿主走路由 `load` 时已取过一次）。 */
  const autoLoad = untrack(() => props.autoLoad ?? true);

  onMount(() => {
    // 首屏数据已给（路由 `load` 取过 / SSR / 单测）→ 不再发请求。
    if (!autoLoad || props.initial !== undefined) {
      return;
    }
    void reload();
  });
</script>

<section class="base-app-registry" data-testid="app-registry">
  <h2>{$i18n.t('nav.appRegistry')}</h2>

  {#if loadError !== ''}
    <p class="base-schema-error" role="alert">{loadError}</p>
  {/if}
  {#if actionError !== ''}
    <p class="base-schema-error" role="alert" data-testid="app-registry-action-error">
      {actionError}
    </p>
  {/if}

  <!--
    表单只在有写权限时渲染（`<Can>`：无权限 → **不进 DOM**）。
    编辑用同一张表单：`editingId` 非空 = 改，空 = 建。
  -->
  <Can permission={APP_REGISTRY_WRITE_PERMISSION}>
    <form
      class="base-schema-form"
      data-testid="app-registry-form"
      onsubmit={(event) => {
        event.preventDefault();
        void handleSubmit();
      }}
    >
      <h3>
        {editingId === '' ? $i18n.t('app.registryCreate') : $i18n.t('app.registryEdit')}
      </h3>
      <Input
        label={$i18n.t('app.fieldClientId')}
        name="app-client-id"
        value={clientId}
        disabled={editingId !== ''}
        oninput={setClientId}
      />
      <Input
        label={$i18n.t('common.name')}
        name="app-display-name"
        value={displayName}
        oninput={setDisplayName}
      />
      <Input
        label={$i18n.t('app.fieldDescription')}
        name="app-description"
        value={description}
        oninput={setDescription}
      />
      <Input
        label={$i18n.t('app.fieldCategory')}
        name="app-category"
        value={category}
        oninput={setCategory}
      />
      <Input
        label={$i18n.t('app.fieldIconUrl')}
        name="app-icon-url"
        type="search"
        value={iconUrl}
        oninput={setIconUrl}
      />
      <Input
        label={$i18n.t('app.fieldLaunchUrl')}
        name="app-launch-url"
        type="search"
        value={launchUrl}
        oninput={setLaunchUrl}
      />
      <Input
        label={$i18n.t('app.fieldSortOrder')}
        name="app-sort-order"
        type="number"
        value={sortOrder}
        oninput={setSortOrder}
      />
      <div class="base-schema-form-actions">
        <Button type="submit" variant="primary" disabled={submitting}>
          {$i18n.t('common.save')}
        </Button>
        <Button disabled={submitting} onclick={resetForm}>{$i18n.t('common.cancel')}</Button>
      </div>
    </form>
  </Can>

  {#if loading}
    <p class="base-schema-loading">{$i18n.t('common.loading')}</p>
  {/if}

  {#if !loading && rows.length === 0}
    <p class="base-schema-empty" data-testid="app-registry-empty">
      {$i18n.t('app.registryEmpty')}
    </p>
  {:else if !loading}
    <table class="base-table" data-testid="app-registry-table">
      <thead>
        <tr>
          {#each columns as column (column.key)}
            <th>{column.label}</th>
          {/each}
          <th></th>
        </tr>
      </thead>
      <tbody>
        {#each apps as app (app.appId)}
          <tr data-testid="app-registry-row">
            <td>{app.appId}</td>
            <td>{app.displayName}</td>
            <td>{app.clientId}</td>
            <td>{app.category}</td>
            <td data-testid="app-registry-status">{statusText(app.status)}</td>
            <td>{app.sortOrder}</td>
            <td data-testid="app-registry-launch">
              {app.launchUrl ?? APP_EMPTY_VALUE}
            </td>
            <td class="base-schema-actions">
              <Can permission={APP_REGISTRY_WRITE_PERMISSION}>
                <Button disabled={submitting} onclick={() => fillForm(app)}>
                  {$i18n.t('common.edit')}
                </Button>
                <Button
                  disabled={submitting || app.launchUrl === null}
                  onclick={() => void handleClearLaunch(app.appId)}
                >
                  {$i18n.t('app.registryClearLaunch')}
                </Button>
              </Can>
              <Can permission={APP_REGISTRY_DELETE_PERMISSION}>
                <Button
                  variant="danger"
                  disabled={submitting}
                  onclick={() => void handleDelete(app.appId)}
                >
                  {$i18n.t('common.delete')}
                </Button>
              </Can>
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  {/if}

  <p class="base-schema-empty" data-testid="app-registry-delete-hint">
    {$i18n.t('app.deleteConfirm')}
  </p>
</section>
