<!--
  测试夹具：给子树提供 Auth 上下文（`<Can>` 与页面都从上下文读登录态）。

  # 为什么需要它

  `svelte/server` 的 `render` 不能点、不能等 Promise，但**能**读上下文。
  应用注册表页用 `<Can>` 决定「写 / 删按钮是否进 DOM」—— 不给上下文时
  一律无权限（fail closed），那就**分不出**「只读账号」与「管理员」两种形态
  → 断言没有区分力（把权限点改错也照样绿）。

  这个包装组件把登录态放进上下文，再把**同一个**应用注册表页渲染出来，
  让测试能真的对照两种账号。
-->
<script lang="ts">
  import { setContext } from 'svelte';

  import type { AuthState } from '@base/auth';
  import type { ApiClient } from '@base/sdk';

  import AppRegistry from '../../src/pages/AppRegistry.svelte';
  import { AUTH_CONTEXT_KEY } from '../../src/lib/auth-context';
  import type { AppsLoadResult } from '../../src/lib/routes';

  /** 夹具 props。 */
  interface AppRegistryWithAuthProps {
    /** 放上下文里的登录态。 */
    readonly state: AuthState;
    /** API 客户端。 */
    readonly client: ApiClient;
    /** 首屏数据（SSR 不跑 `onMount` → 必须给，否则只有加载态）。 */
    readonly initial?: AppsLoadResult;
  }

  const props: AppRegistryWithAuthProps = $props();

  // 与 `App.svelte` 同一形状：上下文里放的是**取值器**（登录后能读到最新态）。
  setContext(AUTH_CONTEXT_KEY, {
    get state() {
      return props.state;
    },
  });
</script>

<AppRegistry client={props.client} initial={props.initial} />
