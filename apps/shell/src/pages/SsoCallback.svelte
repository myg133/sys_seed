<!--
  OIDC 回调交接页（P4 / AC-S-100 预期 3；公开路由 `/sso/callback`）。

  ```text
  IdP → GET /api/iam/sso/oidc/callback?code&state
      → 后端 302 到 /sso/callback?ticket=<一次性 ticket>
      → 这里 POST /api/iam/sso/exchange {ticket} → LoginResponse → 会话 → 首页
  ```

  # 为什么必须是独立页面

  后端回调是**顶层浏览器导航**的落点，返回 JSON 会被浏览器渲染成文本，
  SPA 拿不到 `refreshToken`。所以后端把会话换成一个一次性、短命、绑租户的 ticket，
  由本页用 `fetch` 换回来（换到的令牌只进 `sessionStorage`，D99 / 安全约束 §8）。

  # 四个状态都要有可读的界面

  1. 处理中：**不白屏**，明确显示「正在完成企业登录…」；
  2. 缺 ticket：链接无效 / 已过期 + 回登录页；
  3. exchange 失败：网络错误与后端错误**分开提示**（见 `lib/sso-callback.ts`）；
  4. 成功：宿主跳到首页（`replace`，顺手把 ticket 从地址栏抹掉）。

  # 逻辑不在本文件

  `svelte/server` 只能渲染首帧、不能等待 Promise → 判定与错误映射在
  `src/lib/sso-callback.ts`，本文件只负责把结局渲染成文案。
-->
<script lang="ts">
  import { onMount } from 'svelte';

  import Button from '../components/Button.svelte';

  import type { AppAuth } from '../lib/api';
  import { i18nRuntime } from '../lib/i18n';
  import { completeSsoCallback, type SsoCallbackOutcome } from '../lib/sso-callback';

  /** 回调页 props。 */
  interface SsoCallbackProps {
    /** 认证入口（用 ticket 换会话）。 */
    readonly auth: AppAuth;
    /** 换到会话后的导航（外壳跳首页）。 */
    readonly onLoggedIn: () => void;
    /** 回登录页（缺 ticket / ticket 失效时）。 */
    readonly onBackToLogin: () => void;
  }

  const props: SsoCallbackProps = $props();
  const i18n = i18nRuntime();

  let outcome = $state<SsoCallbackOutcome>({ kind: 'processing' });

  onMount(() => {
    void (async () => {
      const result = await completeSsoCallback({
        search: window.location.search,
        loginWithTicket: (ticket) => props.auth.loginWithTicket(ticket),
        onSuccess: () => {
          props.onLoggedIn();
        },
        // 没有该错误码的文案时退回通用文案，不把 "?" 显示给用户。
        hasMessage: (key) => i18n.t(key) !== '?',
      });
      // 成功时宿主已经导航走，组件随即卸载 —— 不再写状态。
      if (result.kind !== 'success') {
        outcome = result;
      }
    })();
  });
</script>

<section class="base-sso-callback" data-testid="sso-callback">
  {#if outcome.kind === 'processing'}
    <p class="base-booting" role="status">{$i18n.t('common.ssoCallbackProcessing')}</p>
  {:else if outcome.kind === 'error'}
    <p class="base-login-error" role="alert">{$i18n.t(outcome.key, outcome.params)}</p>
    <Button onclick={() => props.onBackToLogin()}>{$i18n.t('common.backToLogin')}</Button>
  {/if}
</section>
