<!--
  设备码批准页（`/oauth/device`，**公开路由**）。

  # 为什么是**公开路由**（D221）

  `verification_uri_complete` = `{授权页基址}/oauth/device?user_code=…`
  → **query 里带 `user_code`** → 任何「重定向丢 query」的路径都会让设备码流程
  **不可用**（守卫的 `intendedPath` 只有 pathname，`router.ts:261`）。
  → 页面**自己**处理未登录，**不动** `router.ts`。

  # `user_code` 的形态

  RFC 8628 §6.1 的 **base20**（`BCDFGHJKLMNPQRSTVWXZ`）、**8 位**、无易混字符。
  ⚠️ 输入时**不补连字符**（后端存的是**无分隔**的 8 位）。
  本地只做**形态**提示（`isWellFormedUserCode`），**判定权在后端**。

  # 状态机在 `lib/oauth-api.ts`

  理由同授权页：`svelte/server` 的 `render` 只能渲染首帧。
-->
<script lang="ts">
  import type { ApiClient } from '@base/sdk';
  import { onMount } from 'svelte';

  import Button from '../components/Button.svelte';
  import Input from '../components/Input.svelte';

  import {
    approveDeviceCode,
    browserEnv,
    buildLoginUrl,
    currentTarget,
    describeOAuthError,
    initialDeviceOutcome,
    normalizeUserCode,
    readPrompt,
    readUserCode,
    runDeviceFlow,
    verifyDeviceCode,
    type DeviceOutcome,
    type OAuthDeviceInfo,
    type OAuthErrorView,
    type OAuthPageEnv,
  } from '../lib/oauth-api';
  import { createValueSetter } from '../lib/handlers';
  import { i18nRuntime } from '../lib/i18n';

  /** 设备码页 props。 */
  interface OAuthDeviceProps {
    /** API 客户端（宿主装配，页面不自己造）。 */
    readonly client: ApiClient;
    /** 是否已登录（宿主传 `auth.state`）。 */
    readonly authenticated: boolean;
    /** 浏览器环境（单测可注入假件；缺省 = 真实浏览器）。 */
    readonly env?: OAuthPageEnv;
    /**
     * 首屏设备码信息（宿主 / 单测注入）。
     *
     * 缺省时页面**自己**在挂载后查（query 里有 `user_code` 时自动查，
     * 没有则渲染输入框让用户手输）。
     */
    readonly initial?: OAuthDeviceInfo;
  }

  const props: OAuthDeviceProps = $props();
  const i18n = i18nRuntime();

  const env = $derived(props.env ?? browserEnv());

  /** 设备码页的结局。 */
  let outcome = $state<DeviceOutcome>({ kind: 'loading' });
  /** 用户手输的设备码。 */
  let userCode = $state('');
  /** 提交中（避免重复点击）。 */
  let busy = $state(false);

  /** 错误文案（没有该错误码的文案时退回通用文案）。 */
  function errorText(error: OAuthErrorView): string {
    const text = i18n.t(error.key, error.params);
    return text === '?' ? i18n.t('common.unexpectedError') : text;
  }

  const setUserCode = createValueSetter((value) => {
    userCode = value;
  });

  /**
   * 首屏结局（**同步可判定**的部分）。
   *
   * ⚠️ **必须把 URL 里的 `user_code` 同步进 `userCode` 状态**：否则
   * `verification_uri_complete`（`/oauth/device?user_code=…`）这条主路径下
   * `userCode` 一直是空串 → 「允许此设备」的 `handleDecision` 读到空码后**直接 return**
   * → 状态机停在 `pending`、页面什么都不发生（按钮点了没反应）。
   * 这条缺陷由终版 E2E 抓到（TC 用例点按钮后等 `oauth-device-approved` 超时）。
   */
  function initialOutcome(): DeviceOutcome {
    const info = props.initial;
    // `readUserCode` 返回 `string | undefined`（query 里没有该键时是 undefined）
    // → 统一收敛成空串，避免 `userCode` 状态出现 undefined。
    const fromQuery = readUserCode(env.search) ?? '';
    if (info !== undefined) {
      // 宿主注入了首屏信息 → 仍然要把 query 里的码同步进输入框状态，
      // 否则后续「允许 / 拒绝」提交的是空串。
      userCode = fromQuery;
      return { kind: 'pending', info };
    }
    userCode = fromQuery;
    return initialDeviceOutcome({
      authenticated: props.authenticated,
      env,
      userCode: fromQuery,
      prompt: readPrompt(env.search),
    });
  }

  outcome = initialOutcome();

  onMount(() => {
    if (props.initial !== undefined || !props.authenticated) {
      return;
    }
    void (async () => {
      const result = await runDeviceFlow({
        client: props.client,
        authenticated: true,
        env,
        userCode: readUserCode(env.search),
      });
      outcome = result;
    })();
  });

  /** 「登录并继续」→ 跳登录页（`return_to` **保留** `?user_code=`）。 */
  function handleSignIn(): void {
    env.assign(buildLoginUrl(currentTarget(env)));
  }

  /** 查设备码（输入框提交，或已从 query 拿到码时的自动查询）。 */
  async function handleVerify(): Promise<void> {
    const code = normalizeUserCode(userCode);
    if (code === '' || busy) {
      return;
    }
    busy = true;
    try {
      const info = await verifyDeviceCode(props.client, code);
      outcome = { kind: 'pending', info };
    } catch (caught) {
      outcome = { kind: 'error', error: describeOAuthError(caught) };
    } finally {
      busy = false;
    }
  }

  /** 「允许」/「拒绝」→ 批准端点。 */
  async function handleDecision(action: 'approve' | 'deny'): Promise<void> {
    const code = normalizeUserCode(userCode);
    if (code === '' || busy) {
      return;
    }
    busy = true;
    try {
      const status = await approveDeviceCode(props.client, code, action);
      outcome = status === 'approved' ? { kind: 'approved' } : { kind: 'denied' };
    } catch (caught) {
      outcome = { kind: 'error', error: describeOAuthError(caught) };
    } finally {
      busy = false;
    }
  }
</script>

<section class="base-oauth-device" data-testid="oauth-device">
  <h2>{$i18n.t('oauth.deviceTitle')}</h2>

  {#if outcome.kind === 'loading'}
    <p class="base-booting" role="status">{$i18n.t('common.loading')}</p>
  {:else if outcome.kind === 'sign-in-required'}
    <p data-testid="oauth-device-sign-in-hint">{$i18n.t('oauth.signInRequiredHint')}</p>
    <Button onclick={handleSignIn}>{$i18n.t('oauth.signInAndContinue')}</Button>
  {:else if outcome.kind === 'login-required'}
    <p role="alert" data-testid="oauth-device-login-required">
      {$i18n.t('oauth.loginRequiredNotice')}
    </p>
  {:else if outcome.kind === 'input'}
    <!-- 没有 `user_code` → 让用户**手输** 8 位码（不补连字符）。 -->
    <form
      onsubmit={(event) => {
        event.preventDefault();
        void handleVerify();
      }}
    >
      <Input
        label={$i18n.t('oauth.deviceCodeLabel')}
        name="oauth-user-code"
        value={userCode}
        oninput={setUserCode}
      />
      <p class="base-schema-empty">{$i18n.t('oauth.deviceCodeHint')}</p>
      <Button type="submit" disabled={busy}>{$i18n.t('oauth.deviceVerify')}</Button>
    </form>
  {:else if outcome.kind === 'pending'}
    <!-- 批准前展示 client 名与**全部**权限点码（逐字）。 -->
    <p class="base-oauth-application" data-testid="oauth-device-application">
      {outcome.info.clientName}
    </p>
    <p>{$i18n.t('oauth.deviceRequestedScopes')}</p>
    <ul class="base-oauth-scopes" data-testid="oauth-device-scopes">
      {#each outcome.info.scopes as scope (scope)}
        <li>{scope}</li>
      {/each}
    </ul>
    <Button onclick={() => void handleDecision('approve')} disabled={busy}>
      {$i18n.t('oauth.allowDevice')}
    </Button>
    <Button onclick={() => void handleDecision('deny')} disabled={busy}>
      {$i18n.t('oauth.denyDevice')}
    </Button>
  {:else if outcome.kind === 'approved'}
    <p role="status" data-testid="oauth-device-approved">{$i18n.t('oauth.deviceApproved')}</p>
  {:else if outcome.kind === 'denied'}
    <p role="status" data-testid="oauth-device-denied">{$i18n.t('oauth.deviceDenied')}</p>
  {:else if outcome.kind === 'error'}
    <p class="base-schema-error" role="alert" data-testid="oauth-device-error">
      {errorText(outcome.error)}
    </p>
  {/if}
</section>
