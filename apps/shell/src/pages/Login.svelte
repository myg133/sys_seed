<!--
  登录页：输入**租户** + 用户名密码 → 调 SDK 登录 → 拿到权限点集合。

  # 租户为什么由用户填（D98）

  后端要求 `x-tenant-id`（缺 → 401 `TENANT_CONTEXT_MISSING`）。
  壳**不**硬编码、也**不**从环境变量读死 —— 那是单租户部署的假设，
  会把多租户底座做成单租户。租户为空时**不发登录请求**（必然 401），
  直接在页面上给出可理解的提示。

  # 权限点只在登录成功时取一次

  登录响应里带 `permissions`；刷新响应里的 `permissions` 恒为空数组，
  所以刷新**不碰**权限点（见 `src/lib/api.ts`）。

  # 文案按**稳定错误码**渲染（AC-S-099 步骤 4）

  失败文案不是字符串常量，而是「翻译键 + 后端 `params`」两个值：
  语言切换后错误提示也跟着变，且 `INSUFFICIENT_PERMISSION` 这类错误
  能带出 `{required}`（`ApiError.params`，D109）。

  ⚠️ 脚本块是 `<script lang="ts">`：根 `eslint.config.js` 已给 `.svelte` 配了
  TypeScript 解析器（D97）→ 这里可以写 TS；带参数的事件处理器仍放 `lib/handlers.ts`。
-->
<script lang="ts">
  import { ApiError } from '@base/sdk';

  import Button from '../components/Button.svelte';
  import Input from '../components/Input.svelte';

  import { TENANT_REQUIRED, type AppAuth } from '../lib/api';
  import { createValueSetter } from '../lib/handlers';
  import { errorMessageKey, i18nRuntime, toMessageParams } from '../lib/i18n';

  /** 登录页的 props。 */
  interface LoginProps {
    /** 认证入口（登录 + 租户绑定）。 */
    readonly auth: AppAuth;
    /** 登录成功后的回调（外壳据此跳转）。 */
    readonly onLoggedIn: () => void;
  }

  const props: LoginProps = $props();
  const i18n = i18nRuntime();

  let tenantId = $state('');
  let username = $state('');
  let password = $state('');
  /** 失败提示的翻译键（空串 = 不显示）。 */
  let errorKey = $state('');
  /** 失败提示的插值参数（后端 `params`）。 */
  let errorParams = $state<Record<string, string | number>>({});
  let busy = $state(false);

  // 输入回调在 `.ts` 里造（那里有完整类型）。
  const setTenantId = createValueSetter((value) => {
    tenantId = value;
  });
  const setUsername = createValueSetter((value) => {
    username = value;
  });
  const setPassword = createValueSetter((value) => {
    password = value;
  });

  /** 失败 → 「翻译键 + 参数」。 */
  function describeLoginError(caught: unknown): {
    key: string;
    params: Record<string, string | number>;
  } {
    if (caught instanceof Error && caught.message === TENANT_REQUIRED) {
      return { key: 'common.tenantRequired', params: {} };
    }
    if (caught instanceof ApiError) {
      const key = errorMessageKey(caught.code);
      const params = toMessageParams(caught.params);
      // 没有该错误码的文案时 `t()` 返回 "?" —— 不把问号显示给用户，
      // 改用通用失败文案（原始 code 仍在 console / 网络面板里可查）。
      if (i18n.t(key, params) === '?') {
        return { key: 'common.unexpectedError', params: {} };
      }
      return { key, params };
    }
    return { key: 'common.unexpectedError', params: {} };
  }

  function showError(next: { key: string; params: Record<string, string | number> }): void {
    errorKey = next.key;
    errorParams = next.params;
  }

  async function submit() {
    const tenant = tenantId.trim();
    if (tenant === '') {
      // 不发一个必然 401 的请求（后端缺租户头 → TENANT_CONTEXT_MISSING）。
      showError({ key: 'common.tenantRequired', params: {} });
      return;
    }
    if (username === '' || password === '') {
      showError({ key: 'common.credentialsRequired', params: {} });
      return;
    }
    busy = true;
    errorKey = '';
    try {
      await props.auth.login({ tenantId: tenant, username, password });
      props.onLoggedIn();
    } catch (caught) {
      showError(describeLoginError(caught));
    } finally {
      busy = false;
    }
  }
</script>

<form
  class="base-login"
  onsubmit={(event) => {
    event.preventDefault();
    void submit();
  }}
>
  <h2>{$i18n.t('common.login')}</h2>
  <Input
    label={$i18n.t('common.tenant')}
    name="tenant"
    placeholder={$i18n.t('common.tenantPlaceholder')}
    value={tenantId}
    oninput={setTenantId}
  />
  <Input
    label={$i18n.t('common.username')}
    name="username"
    value={username}
    oninput={setUsername}
  />
  <Input
    label={$i18n.t('common.password')}
    name="password"
    type="password"
    value={password}
    oninput={setPassword}
  />
  {#if errorKey !== ''}
    <p class="base-login-error">{$i18n.t(errorKey, errorParams)}</p>
  {/if}
  <Button type="submit" disabled={busy}>{$i18n.t('common.login')}</Button>
</form>
