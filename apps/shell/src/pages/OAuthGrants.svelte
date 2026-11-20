<!--
  「我的授权」页（`/oauth/grants`，**受保护**：权限点 `oauth:grant:read`）。

  # 首屏数据在**路由的 `load`** 里取（与 `AUDIT_ROUTE_PATH` 同一套机制）

  守卫不放行 → **路由的 `load` 不会被调用**（`router.ts:244-250`）
  → `GET /api/oauth/grants` 的请求计数为 **0**（AC-S-147 的前置判据）。

  ⚠️ 既有坑：`onAudit?.(await …)` 会在没有回调时把整个表达式**短路**掉
  → 请求根本不会发 → 数据永远不来。`routes.ts` 里本路由的写法是
  **先取数据、再回调**（`const result = await …; onGrants?.(result);`）。

  # 不做租户过滤

  `GET /api/oauth/grants` **只返回当前用户自己的**（后端保证）→
  前端**不做**租户过滤（AC-S-147 步骤 5：别的租户的用户查不到任何记录）。

  # 撤销是 **grant 级**（D210）

  `POST /api/oauth/grants/revoke` body `{ clientId }` → 200 `{ status: 'revoked' }`
  → 该 grant 签发的令牌**立即失效** → 撤销后**刷新列表**。
-->
<script lang="ts">
  import type { ApiClient } from '@base/sdk';
  import { onMount, untrack } from 'svelte';

  import Button from '../components/Button.svelte';

  import {
    formatGrantTimestamp,
    listOAuthGrants,
    revokeOAuthGrant,
    type OAuthGrant,
    type OAuthGrantsLoadResult,
  } from '../lib/oauth-api';
  import { describeError, i18nRuntime } from '../lib/i18n';

  /** 「我的授权」页 props。 */
  interface OAuthGrantsProps {
    /** API 客户端（宿主装配，页面不自己造）。 */
    readonly client: ApiClient;
    /**
     * 首屏数据（由**路由的 `load`** 取）。
     *
     * 守卫不放行时 `load` 不会被调用 → 这里一直是 `undefined` → 页面也不会渲染
     * → 目标接口的请求计数为 **0**。
     */
    readonly initial?: OAuthGrantsLoadResult;
  }

  const props: OAuthGrantsProps = $props();
  const i18n = i18nRuntime();

  /**
   * 首屏结果（`untrack` 只取**初值**，与 `Audit.svelte` 的 `pageSize` 同一款写法）。
   *
   * 宿主在**路由的 `load`** 里取一次 → 首屏结果晚到时由下面的 `$effect` 铺上。
   */
  const initialResult = untrack(() => props.initial);
  /** 首屏的失败原因（`undefined` = 没失败）。 */
  const initialFailure = initialResult?.failure;

  /** 授权记录（首屏由路由的 `load` 给；之后由本页自己刷新）。 */
  let grants = $state<readonly OAuthGrant[]>(initialResult?.grants ?? []);
  let loading = $state(initialResult === undefined);
  /** 失败提示（空串 = 不显示）。 */
  let error = $state(
    initialFailure === undefined
      ? ''
      : describeError(initialFailure, (key, params) => i18n.t(key, params)),
  );
  /** 正在撤销的 clientId（空串 = 没有）。 */
  let revoking = $state('');
  /**
   * 本页是否**自己**发过请求。
   *
   * 用户撤销之后列表是本页刷新的 → 迟到的首屏结果**不能**把它盖回去。
   */
  let selfLoaded = false;

  /** 状态文案（`active` / `revoked`；未知值**逐字**显示）。 */
  function statusText(grant: OAuthGrant): string {
    if (grant.status === 'revoked') {
      return i18n.t('oauth.grantStatusRevoked');
    }
    if (grant.status === 'active') {
      return i18n.t('oauth.grantStatusActive');
    }
    return grant.status;
  }

  /** 应用名（后端没给 name 时退回 clientId）。 */
  function clientName(grant: OAuthGrant): string {
    return grant.clientName ?? grant.clientId;
  }

  /** 重新取列表（撤销后刷新用）。 */
  async function reload(): Promise<void> {
    selfLoaded = true;
    loading = true;
    try {
      grants = await listOAuthGrants(props.client);
      error = '';
    } catch (caught) {
      grants = [];
      error = describeError(caught, (key, params) => i18n.t(key, params));
    } finally {
      loading = false;
    }
  }

  /** 撤销某个 client 的授权 → 刷新列表。 */
  async function handleRevoke(clientId: string): Promise<void> {
    if (revoking !== '') {
      return;
    }
    revoking = clientId;
    try {
      await revokeOAuthGrant(props.client, clientId);
      await reload();
    } catch (caught) {
      error = describeError(caught, (key, params) => i18n.t(key, params));
    } finally {
      revoking = '';
    }
  }

  /**
   * 首屏数据一到就铺上。
   *
   * 首屏来自**路由的 `load`**（`props.initial`）—— 页面**不**在挂载时
   * 自己再取一次（那会发两个重复请求）。
   *
   * `selfLoaded` 为真（用户撤销过）时**忽略**首屏结果 ——
   * 否则迟到的首屏会把用户刚看到的撤销结果盖回去。
   */
  $effect(() => {
    const initial = props.initial;
    if (initial === undefined || selfLoaded) {
      return;
    }
    grants = initial.grants;
    error =
      initial.failure === undefined
        ? ''
        : describeError(initial.failure, (key, params) => i18n.t(key, params));
    loading = false;
  });

  onMount(() => {
    // 宿主没有给首屏数据（例如直接进页面而不是走路由的 `load`）→ 自己取一次。
    if (props.initial === undefined) {
      void reload();
    }
  });
</script>

<section class="base-oauth-grants" data-testid="oauth-grants">
  <h2>{$i18n.t('oauth.grantsTitle')}</h2>

  {#if error !== ''}
    <p class="base-schema-error" role="alert">{error}</p>
  {/if}

  {#if loading}
    <p class="base-schema-loading">{$i18n.t('common.loading')}</p>
  {/if}

  {#if grants.length === 0 && !loading}
    <p class="base-schema-empty" data-testid="oauth-grants-empty">
      {$i18n.t('oauth.grantsEmpty')}
    </p>
  {:else}
    <table class="base-table" data-testid="oauth-grants-table">
      <thead>
        <tr>
          <th>{$i18n.t('oauth.grantClient')}</th>
          <th>{$i18n.t('oauth.grantScopes')}</th>
          <th>{$i18n.t('oauth.grantGrantedAt')}</th>
          <th>{$i18n.t('oauth.grantStatus')}</th>
          <th></th>
        </tr>
      </thead>
      <tbody>
        {#each grants as grant (grant.clientId)}
          <tr data-testid="oauth-grant-row">
            <td>{clientName(grant)}</td>
            <td>
              {#each grant.scopes as scope (scope)}
                <span class="base-oauth-scope">{scope}</span>
              {/each}
            </td>
            <td>{formatGrantTimestamp(grant.grantedAt, (value) => i18n.formatDate(value))}</td>
            <td>{statusText(grant)}</td>
            <td>
              {#if grant.status !== 'revoked'}
                <Button
                  disabled={revoking !== ''}
                  onclick={() => void handleRevoke(grant.clientId)}
                >
                  {$i18n.t('oauth.grantRevoke')}
                </Button>
              {/if}
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  {/if}
</section>
