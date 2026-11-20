<!--
  授权页 / 同意页（`/oauth/authorize`，**公开路由**）。

  # 为什么是**公开路由**（D221）

  平台的会话是 **Bearer token 存在 `sessionStorage`** → 浏览器**顶层导航不会带
  `Authorization` 头** → 守卫若要求登录，未登录时会被改写到登录页，
  而守卫记的 `intendedPath` **只有 pathname**（`router.ts:261`）
  → **授权参数与 query 全丢**。
  → 页面**自己**处理未登录（渲染「登录并继续」），**不动** `router.ts`。

  # 页面自己读 query

  路由的 `resolve()` 用带 query 的字符串 `match()` 会匹配失败
  （`router.ts:204-207`）→ 授权参数一律由页面**自己**从
  `location.search` 读（`readAuthorizeParams`）。

  # 状态机在 `lib/oauth-api.ts`（不在本文件）

  `svelte/server` 的 `render` 只能渲染首帧、不能点击、不能等 Promise
  → 判定与「`error=` 透出到 URL」都放在 `oauth-api.ts` 的 `runAuthorizeFlow`，
  单测逐条断言；本文件只负责把结局渲染成文案。

  # `error=login_required` / `error=consent_required` 只出现在**授权页的 query**

  D233：这两个协议码**不是** preview 的错误体（preview 返 200 + `needsConsent`）
  → 用 `history.replaceState` 透出（**不重新导航** —— 重新导航会把整页重载，
  页面里的 ticket 状态就没了），且**不得**覆盖原有的授权参数。
-->
<script lang="ts">
  import type { ApiClient } from '@base/sdk';
  import { onMount } from 'svelte';

  import Button from '../components/Button.svelte';

  import {
    browserEnv,
    buildLoginUrl,
    currentTarget,
    describeOAuthError,
    initialAuthorizeOutcome,
    readAuthorizeParams,
    runAuthorizeFlow,
    submitConsentDecision,
    type AuthorizeOutcome,
    type OAuthErrorView,
    type OAuthPageEnv,
    type OAuthPreviewResult,
  } from '../lib/oauth-api';
  import { i18nRuntime } from '../lib/i18n';

  /** 授权页 props。 */
  interface OAuthAuthorizeProps {
    /** API 客户端（宿主装配，页面不自己造）。 */
    readonly client: ApiClient;
    /** 是否已登录（宿主传 `auth.state`；**未登录时页面自己引导登录**）。 */
    readonly authenticated: boolean;
    /** 浏览器环境（单测可注入假件；缺省 = 真实浏览器）。 */
    readonly env?: OAuthPageEnv;
    /**
     * 首屏 preview 结果（宿主 / 单测注入）。
     *
     * 缺省时页面**自己**在挂载后调 `preview`。
     * 给了就直接渲染同意页（便于首帧可断言）。
     */
    readonly initial?: OAuthPreviewResult;
  }

  const props: OAuthAuthorizeProps = $props();
  const i18n = i18nRuntime();

  const env = $derived(props.env ?? browserEnv());
  const params = $derived(readAuthorizeParams(env.search));

  /** 授权页的结局（首帧就位；后续由状态机更新）。 */
  let outcome = $state<AuthorizeOutcome>({ kind: 'loading' });

  /** 同意 / 拒绝请求进行中（避免重复点击）。 */
  let busy = $state(false);

  /** 错误文案（没有该错误码的文案时退回通用文案，不把 "?" 显示给用户）。 */
  function errorText(error: OAuthErrorView): string {
    const text = i18n.t(error.key, error.params);
    return text === '?' ? i18n.t('common.unexpectedError') : text;
  }

  /**
   * 首屏结局（**同步可判定**的部分）。
   *
   * 未登录 + `prompt=none` 时**当场**把 `error=login_required` 透出到 URL
   * （`replaceState`，不重新导航）—— 这一步在脚本块里做，
   * 首帧就已经生效（`render` 只渲染首帧，放到挂载后单测就看不到）。
   */
  function initialOutcome(): AuthorizeOutcome {
    return initialAuthorizeOutcome({
      authenticated: props.authenticated,
      params,
      env,
      preview: props.initial,
    });
  }

  outcome = initialOutcome();

  onMount(() => {
    // 宿主给了首屏 preview → 不再自己取（避免两个请求）。
    if (props.initial !== undefined || !props.authenticated) {
      return;
    }
    void (async () => {
      const result = await runAuthorizeFlow({
        client: props.client,
        params: readAuthorizeParams(env.search),
        authenticated: true,
        env,
      });
      // 免登直发时已经跳走了，组件随即卸载 —— 不再写状态。
      if (result.kind !== 'redirected') {
        outcome = result;
      }
    })();
  });

  /**
   * 「登录并继续」→ 跳登录页。
   *
   * `return_to` 装的是**完整 URL**（`pathname + search`）——
   * 授权参数（`client_id` / `state` / `code_challenge` / `redirect_uri`）
   * 一个都不能少（D221 的核心判据）。
   */
  function handleSignIn(): void {
    env.assign(buildLoginUrl(currentTarget(env)));
  }

  /** 「允许」→ confirm → 用返回的 `redirectTo` 跳转。 */
  async function handleAllow(): Promise<void> {
    const ticket = outcome.kind === 'consent' ? outcome.preview.ticket : undefined;
    if (ticket === undefined || busy) {
      return;
    }
    busy = true;
    try {
      await submitConsentDecision(props.client, env, ticket, 'allow');
    } catch (caught) {
      outcome = { kind: 'error', error: describeOAuthError(caught) };
    } finally {
      busy = false;
    }
  }

  /** 「拒绝」→ deny → 用返回的 `redirectTo` 跳转（query 里有 `error=access_denied`）。 */
  async function handleDeny(): Promise<void> {
    const ticket = outcome.kind === 'consent' ? outcome.preview.ticket : undefined;
    if (ticket === undefined || busy) {
      return;
    }
    busy = true;
    try {
      await submitConsentDecision(props.client, env, ticket, 'deny');
    } catch (caught) {
      outcome = { kind: 'error', error: describeOAuthError(caught) };
    } finally {
      busy = false;
    }
  }
</script>

<section class="base-oauth-authorize" data-testid="oauth-authorize">
  <h2>{$i18n.t('oauth.authorizeTitle')}</h2>

  {#if outcome.kind === 'loading'}
    <p class="base-booting" role="status">{$i18n.t('common.loading')}</p>
  {:else if outcome.kind === 'sign-in-required'}
    <!-- 未登录 + 允许交互：引导登录（**不调 preview** —— 它需要 Bearer）。 -->
    <p data-testid="oauth-sign-in-hint">{$i18n.t('oauth.signInRequiredHint')}</p>
    <Button onclick={handleSignIn}>{$i18n.t('oauth.signInAndContinue')}</Button>
  {:else if outcome.kind === 'login-required'}
    <!-- `prompt=none`：**不跳登录页**（不许交互），只在 URL 上透出协议码。 -->
    <p role="alert" data-testid="oauth-login-required">{$i18n.t('oauth.loginRequiredNotice')}</p>
  {:else if outcome.kind === 'consent'}
    <!--
      同意页：展示 client 的 name 与**全部**权限点码（AC-S-146 步骤 1）
      —— 权限点码**逐字**显示，用户所见 = 实际授予（D204）。
    -->
    <p class="base-oauth-application" data-testid="oauth-application">
      {outcome.preview.application.name}
    </p>
    <p>{$i18n.t('oauth.requestedScopes')}</p>
    <ul class="base-oauth-scopes" data-testid="oauth-scopes">
      {#each outcome.preview.scopes as scope (scope)}
        <li>{scope}</li>
      {/each}
    </ul>
    <Button onclick={() => void handleAllow()} disabled={busy}>
      {$i18n.t('oauth.allowApplication')}
    </Button>
    <Button onclick={() => void handleDeny()} disabled={busy}>
      {$i18n.t('oauth.denyApplication')}
    </Button>
  {:else if outcome.kind === 'error'}
    <p class="base-schema-error" role="alert" data-testid="oauth-error">
      {errorText(outcome.error)}
    </p>
  {/if}
</section>
