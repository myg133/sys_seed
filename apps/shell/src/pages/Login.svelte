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

  # 企业账号入口（REQ-013 / AC-S-109 / AC-S-021）

  租户填好后拉 `GET /api/iam/sso/providers`（**带 `x-tenant-id`**），
  非空才渲染「企业账号登录」区块：

  - `providers` 为空数组 → **不渲染**区块（不是渲染禁用按钮）；
  - 请求失败 → **静默隐藏**：providers 接口挂了不能让人连本地登录都用不了。

  本地密码登录被拒（`403 SSO_REQUIRED`，强制 SSO）时，后端的 `params.ssoProviders`
  就是可用入口 —— 把它渲染出来，让用户直接走企业账号登录；
  **不**把 `SSO_REQUIRED` 显示成「用户名或密码错误」。

  ⚠️ 脚本块是 `<script lang="ts">`：根 `eslint.config.js` 已给 `.svelte` 配了
  TypeScript 解析器（D97）→ 这里可以写 TS；带参数的事件处理器仍放 `lib/handlers.ts`。
-->
<script lang="ts">
  import { ApiError, type ApiClient } from '@base/sdk';
  import { onDestroy } from 'svelte';

  import Button from '../components/Button.svelte';
  import Input from '../components/Input.svelte';
  import SsoButtons from '../components/SsoButtons.svelte';

  import { TENANT_REQUIRED, type AppAuth } from '../lib/api';
  import { createValueSetter } from '../lib/handlers';
  import { errorMessageKey, i18nRuntime, toMessageParams } from '../lib/i18n';
  import {
    PROVIDER_FETCH_DEBOUNCE_MS,
    listProviders,
    providersFromParams,
    startOidcAuthorize,
    type SsoProviderView,
  } from '../lib/sso-api';

  /** 登录页的 props。 */
  interface LoginProps {
    /** 认证入口（登录 + 租户绑定 + SSO）。 */
    readonly auth: AppAuth;
    /** SDK 客户端（providers 接口用它，且要显式带 `x-tenant-id`）。 */
    readonly client: ApiClient;
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
  /** 当前租户可用的企业登录入口（空数组 = 不渲染区块）。 */
  let providers = $state<readonly SsoProviderView[]>([]);
  /** providers 请求的代号：晚到的旧响应不得覆盖新租户的结果。 */
  let providerSeq = 0;
  let providerTimer: ReturnType<typeof setTimeout> | undefined;

  // 输入回调在 `.ts` 里造（那里有完整类型）。
  const setTenantId = createValueSetter((value) => {
    tenantId = value;
    scheduleProviders(value);
  });
  const setUsername = createValueSetter((value) => {
    username = value;
  });
  const setPassword = createValueSetter((value) => {
    password = value;
  });

  function cancelProviderLoad(): void {
    providerSeq += 1;
    if (providerTimer !== undefined) {
      clearTimeout(providerTimer);
      providerTimer = undefined;
    }
  }

  /** 拉 providers；任何失败都**静默隐藏**区块（不阻塞本地登录）。 */
  async function loadProviders(tenant: string, seq: number): Promise<void> {
    try {
      const loaded = await listProviders(props.client, tenant);
      if (seq !== providerSeq) {
        return;
      }
      providers = loaded;
    } catch {
      if (seq !== providerSeq) {
        return;
      }
      providers = [];
    }
  }

  /** 租户变化 → 防抖后重新拉 providers（逐字符发请求会被后端限流）。 */
  function scheduleProviders(nextTenant: string): void {
    cancelProviderLoad();
    providers = [];
    const tenant = nextTenant.trim();
    if (tenant === '') {
      return;
    }
    const seq = providerSeq;
    providerTimer = setTimeout(() => {
      providerTimer = undefined;
      void loadProviders(tenant, seq);
    }, PROVIDER_FETCH_DEBOUNCE_MS);
  }

  onDestroy(() => {
    if (providerTimer !== undefined) {
      clearTimeout(providerTimer);
      providerTimer = undefined;
    }
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

  /** `403 SSO_REQUIRED`：把后端给的入口渲染出来（不显示成「用户名或密码错误」）。 */
  function adoptForcedSso(caught: unknown): boolean {
    if (!(caught instanceof ApiError) || caught.code !== 'SSO_REQUIRED') {
      return false;
    }
    const fromParams = providersFromParams(caught.params);
    if (fromParams.length > 0) {
      // 强制 SSO 的入口是**后端**给的（`params.ssoProviders`），比之前拉的列表更可信：
      // 走到这里说明本地登录确实被拒了。
      providers = fromParams;
    }
    return true;
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
      adoptForcedSso(caught);
      showError(describeLoginError(caught));
    } finally {
      busy = false;
    }
  }

  /** LDAP 入口（`type: "ldap"`）：用户名 / 密码交给 `POST /api/iam/sso/ldap/login`。 */
  async function submitSsoLdap(name: string, secret: string): Promise<void> {
    const tenant = tenantId.trim();
    if (tenant === '') {
      showError({ key: 'common.tenantRequired', params: {} });
      return;
    }
    if (name === '' || secret === '') {
      // 空密码后端也会拒（`400 SSO_LDAP_EMPTY_PASSWORD`）；这里先省掉一次往返。
      showError({ key: 'common.credentialsRequired', params: {} });
      return;
    }
    if (busy) {
      return;
    }
    busy = true;
    errorKey = '';
    // 用后端给的登录入口（缺省时 `sso-api` 走常量路径）。
    const loginUrl = providers.find((provider) => provider.type === 'ldap')?.loginUrl;
    try {
      await props.auth.loginWithLdap({
        tenantId: tenant,
        username: name,
        password: secret,
        ...(loginUrl === undefined ? {} : { loginUrl }),
      });
      props.onLoggedIn();
    } catch (caught) {
      adoptForcedSso(caught);
      showError(describeLoginError(caught));
    } finally {
      busy = false;
    }
  }

  /** OIDC 入口（`type: "oidc"`）：**整页跳转**（顶层导航，不能走 fetch）。 */
  function startOidc(provider: SsoProviderView): void {
    const authorizeUrl = provider.authorizeUrl ?? '';
    if (authorizeUrl === '') {
      showError({ key: 'error.SSO_PROVIDER_NOT_CONFIGURED', params: {} });
      return;
    }
    try {
      startOidcAuthorize(authorizeUrl, tenantId.trim());
    } catch {
      // 没有导航能力（非浏览器环境）：按客户端错误提示，不静默什么都不做。
      showError({ key: 'common.clientInternalError', params: {} });
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
    <p class="base-login-error" role="alert">{$i18n.t(errorKey, errorParams)}</p>
  {/if}
  <Button type="submit" disabled={busy}>{$i18n.t('common.login')}</Button>
</form>

<!-- providers 为空 / 请求失败时**不渲染**（不是禁用按钮，也不是空区块）。 -->
{#if providers.length > 0}
  <SsoButtons {providers} disabled={busy} onOidc={startOidc} onLdapSubmit={submitSsoLdap} />
{/if}
