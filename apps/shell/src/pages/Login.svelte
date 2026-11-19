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

  ⚠️ 脚本块是 `<script lang="ts">`：根 `eslint.config.js` 已给 `.svelte` 配了
  TypeScript 解析器（D97）→ 这里可以写 TS；带参数的事件处理器仍放 `lib/handlers.ts`。
-->
<script lang="ts">
  import { ApiError } from '@base/sdk';

  import Button from '../components/Button.svelte';
  import Input from '../components/Input.svelte';

  import { TENANT_REQUIRED, type AppAuth } from '../lib/api';
  import { createValueSetter } from '../lib/handlers';

  /** 登录页的 props。 */
  interface LoginProps {
    /** 认证入口（登录 + 租户绑定）。 */
    readonly auth: AppAuth;
    /** 登录成功后的回调（外壳据此跳转）。 */
    readonly onLoggedIn: () => void;
  }

  const props: LoginProps = $props();

  let tenantId = $state('');
  let username = $state('');
  let password = $state('');
  let error = $state('');
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

  /**
   * 失败文案按**稳定错误码**分派（D94 的精神：文案由前端按 `code` 渲染）。
   *
   * 不再把所有失败都说成「用户名或密码不正确」——
   * 租户缺失 / 账号锁定 / 限流都与密码无关，统一文案会把排查带偏。
   */
  function messageFor(caught: unknown): string {
    if (caught instanceof Error && caught.message === TENANT_REQUIRED) {
      return '请填写租户标识';
    }
    if (caught instanceof ApiError) {
      switch (caught.code) {
        case 'INVALID_CREDENTIALS':
          return '登录失败：用户名或密码不正确';
        case 'TENANT_CONTEXT_MISSING':
          return '登录失败：缺少租户标识';
        case 'ACCOUNT_LOCKED':
          return '登录失败：账号已锁定';
        case 'ACCOUNT_DISABLED':
          return '登录失败：账号已停用';
        case 'LOGIN_RATE_LIMITED':
          return '登录失败：尝试过于频繁，请稍后再试';
        default:
          return `登录失败（${caught.code}）`;
      }
    }
    return '登录失败：请稍后重试';
  }

  async function submit() {
    const tenant = tenantId.trim();
    if (tenant === '') {
      // 不发一个必然 401 的请求（后端缺租户头 → TENANT_CONTEXT_MISSING）。
      error = '请填写租户标识';
      return;
    }
    if (username === '' || password === '') {
      error = '请填写用户名和密码';
      return;
    }
    busy = true;
    error = '';
    try {
      await props.auth.login({ tenantId: tenant, username, password });
      props.onLoggedIn();
    } catch (caught) {
      error = messageFor(caught);
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
  <h2>登录</h2>
  <Input
    label="租户"
    name="tenant"
    placeholder="例如 tenant-a"
    value={tenantId}
    oninput={setTenantId}
  />
  <Input label="用户名" name="username" value={username} oninput={setUsername} />
  <Input label="密码" name="password" type="password" value={password} oninput={setPassword} />
  {#if error !== ''}
    <p class="base-login-error">{error}</p>
  {/if}
  <Button type="submit" disabled={busy}>登录</Button>
</form>
