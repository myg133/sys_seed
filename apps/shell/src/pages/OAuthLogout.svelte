<!--
  登出页（`/oauth/logout`，**公开路由**）。

  # front-channel 的 iframe 是**前端渲染的**（D225）

  平台的会话是 **Bearer** → 登出必须由前端 `fetch` 发起（`Authorization` 头）
  → 浏览器**导航**无法发起 `POST` → 后端只能回 **JSON**
  `{ loggedOut: {...}, frontChannel: [{ clientId, logoutUrl, logoutToken }] }`
  → **iframe 由本页渲染**（后端**不**返回 HTML）。

  # iframe 的三条硬要求（AC-S-150 步骤 2）

  1. 数量**恰好** = `frontChannel.length`（**没有就一个都不渲染**）；
  2. `src` **逐字** = `logoutUrl` + `?logout_token=` + `logoutToken`；
  3. `sandbox` 属性**含** `allow-scripts`、**不含** `allow-same-origin`
     —— `src` 是**第三方域** → `sandbox` 是**安全要求**，不是可选优化
     （`allow-same-origin` 会让被沙箱的文档拿到自己的 origin）。

  # 登出本身不影响主流程

  后端投递 back-channel 失败**只记日志**（D211）→ 前端也不因为
  iframe 加载失败而报错（iframe 的加载失败我们观测不到，也**不该**观测）。
-->
<script lang="ts">
  import type { ApiClient } from '@base/sdk';
  import { onMount, untrack } from 'svelte';

  import {
    describeOAuthError,
    frontChannelSrc,
    logoutOAuth,
    type OAuthErrorView,
    type OAuthLogoutResult,
  } from '../lib/oauth-api';
  import { i18nRuntime } from '../lib/i18n';

  /** 登出页 props。 */
  interface OAuthLogoutProps {
    /** API 客户端（宿主装配，页面不自己造）。 */
    readonly client: ApiClient;
    /**
     * 首屏登出结果（宿主 / 单测注入）。
     *
     * 缺省时页面**自己**在挂载后调 `POST /api/oauth/logout`。
     */
    readonly initial?: OAuthLogoutResult;
  }

  const props: OAuthLogoutProps = $props();
  const i18n = i18nRuntime();

  /**
   * 首屏登出结果（`untrack` 只取**初值**）。
   *
   * 刻意与 `Audit.svelte` 的 `pageSize` 初值同一款写法：
   * props 之后变了也不跟着变（首屏结果由宿主在**路由的 `load`** 里给一次）。
   */
  const initialResult = untrack(() => props.initial);

  /** 登出结果（`undefined` = 还没拿到）。 */
  let result = $state<OAuthLogoutResult | undefined>(initialResult);
  /** 失败提示（`undefined` = 没有）。 */
  let error = $state<OAuthErrorView | undefined>(undefined);
  let loading = $state(initialResult === undefined);

  /** 错误文案（没有该错误码的文案时退回通用文案）。 */
  function errorText(view: OAuthErrorView): string {
    const text = i18n.t(view.key, view.params);
    return text === '?' ? i18n.t('common.unexpectedError') : text;
  }

  onMount(() => {
    if (props.initial !== undefined) {
      return;
    }
    void (async () => {
      try {
        result = await logoutOAuth(props.client);
        error = undefined;
      } catch (caught) {
        error = describeOAuthError(caught);
      } finally {
        loading = false;
      }
    })();
  });
</script>

<section class="base-oauth-logout" data-testid="oauth-logout">
  <h2>{$i18n.t('oauth.logoutTitle')}</h2>

  {#if loading}
    <p class="base-schema-loading">{$i18n.t('common.loading')}</p>
  {/if}

  {#if error !== undefined}
    <p class="base-schema-error" role="alert" data-testid="oauth-logout-error">
      {errorText(error)}
    </p>
  {/if}

  {#if result !== undefined}
    <p role="status" data-testid="oauth-logout-done">{$i18n.t('oauth.logoutTitle')}</p>
    <p data-testid="oauth-logout-summary">
      {$i18n.t('oauth.logoutSummary', {
        grants: result.loggedOut.grants,
        sessions: result.loggedOut.sessions,
      })}
    </p>
    {#if result.frontChannel.length > 0}
      <p data-testid="oauth-logout-notifying">{$i18n.t('oauth.logoutNotifying')}</p>
      <!--
        每个 front-channel 目标**一个** iframe（`clientId` 不同的项各一个）；
        数量**恰好** = `frontChannel.length`。
      -->
      {#each result.frontChannel as item (item.clientId)}
        <iframe
          class="base-oauth-front-channel"
          data-testid="oauth-front-channel"
          src={frontChannelSrc(item)}
          sandbox="allow-scripts"
          title={item.clientId}
        ></iframe>
      {/each}
    {/if}
  {/if}
</section>
