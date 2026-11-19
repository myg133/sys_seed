<!--
  登录页：输入用户名密码 → 调 SDK 登录 → 拿到权限点集合。

  # 权限点只在登录成功时取一次

  登录响应里带 `permissions`；刷新响应里的 `permissions` 恒为空数组，
  所以刷新**不碰**权限点（见 `src/lib/api.ts`）。

  ⚠️ 脚本块是 **JS 语法**（见 `src/lib/handlers.ts` 的说明）：
  仓库根的 eslint 配置没给 `.svelte` 配 TypeScript 解析器，
  写 TS 注解会让 `eslint .` 报解析错误。
-->
<script lang="ts">
  import Button from '../components/Button.svelte';
  import Input from '../components/Input.svelte';

  import { createValueSetter } from '../lib/handlers';

  const props = $props();

  let username = $state('');
  let password = $state('');
  let error = $state('');
  let busy = $state(false);

  // 输入回调在 `.ts` 里造（那里有完整类型）。
  const setUsername = createValueSetter((value) => {
    username = value;
  });
  const setPassword = createValueSetter((value) => {
    password = value;
  });

  async function submit() {
    busy = true;
    error = '';
    try {
      await props.auth.login(username, password);
      props.onLoggedIn();
    } catch {
      // 失败原因由 SDK 的错误码决定（文案由 i18n 渲染）；这里只提示「登录失败」。
      error = '登录失败：用户名或密码不正确';
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
  <Input label="用户名" name="username" value={username} oninput={setUsername} />
  <Input label="密码" name="password" type="password" value={password} oninput={setPassword} />
  {#if error !== ''}
    <p class="base-login-error">{error}</p>
  {/if}
  <Button type="submit" disabled={busy || username === '' || password === ''}>登录</Button>
</form>
