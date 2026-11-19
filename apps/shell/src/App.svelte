<!--
  应用外壳（本期最小壳）：登录页 + 极简路由 + 一个受保护页 + 403 页。

  # 关键不变量

  - **会话跨整页重载存活**（`sessionStorage`，D99）：启动时**先恢复会话、再过守卫**
    → 直接敲受保护 URL 落 **403 页**（不是登录页）；
  - **守卫放行后**才调用受保护页的数据加载函数 → 403 落点时目标接口请求计数为 **0**；
  - 未登录 → 落**登录页**（不是 403）；已登录无权限 → 落 **403 页**；
  - 租户由用户在登录页填（D98），登录后整个会话沿用它；
  - 品牌主色由 `@base/theme` 统一处理（shell 不再自己判断颜色格式）。

  ⚠️ 脚本块是 `<script lang="ts">`：根 `eslint.config.js` 已给 `**/*.svelte` 配了
  TypeScript 解析器（D97）→ 这里**可以**写 TS 类型。带参数的事件处理器仍放
  `src/lib/handlers.ts` —— 模板内联箭头函数没有上下文类型。
-->
<script lang="ts">
  import { setThemeMode } from '@base/theme';
  import { onMount, setContext } from 'svelte';

  import { applyBranding, fetchBranding } from './lib/branding';
  import { createAppApi } from './lib/api';
  import { AUTH_CONTEXT_KEY } from './lib/auth-context';
  import { createRouter, initialResolution } from './lib/router';
  import { createRoutes, SCHEMA_ROUTE_PATH, toRoles } from './lib/routes';

  import Forbidden from './pages/Forbidden.svelte';
  import Home from './pages/Home.svelte';
  import Login from './pages/Login.svelte';
  import ProtectedRoles from './pages/ProtectedRoles.svelte';
  import SchemaPage from './pages/SchemaPage.svelte';

  // 同源：页面由 platform-server 的静态目录托管（本仓没有 CORS 层）。
  // 租户在登录时由用户填（D98），这里不传死值。
  const { client, auth, restoreSession } = createAppApi();

  let title = $state('');
  let booted = $state(false);
  let authState = $state(auth.state);
  let roles = $state(toRoles(undefined));
  let resolution = $state(initialResolution(window.location.pathname));

  const routes = createRoutes(client, (loaded) => {
    roles = toRoles(loaded);
  });

  const router = createRouter({
    routes,
    environment: {
      location: window.location,
      history: window.history,
      addEventListener: (type, listener) => {
        window.addEventListener(type, listener);
      },
      removeEventListener: (type, listener) => {
        window.removeEventListener(type, listener);
      },
    },
    getState: () => authState,
    onResolve: (next) => {
      resolution = next;
    },
  });

  // 上下文里放的是**取值器**：登录 / 登出后 `<Can>` 会读到最新的登录态。
  setContext(AUTH_CONTEXT_KEY, {
    get state() {
      return authState;
    },
  });

  async function loadBranding() {
    try {
      const branding = await fetchBranding();
      title = applyBranding(branding).title;
    } catch (error) {
      // 依赖不可用时不回落到任何旧品牌：标题留空，主色用**中性**兜底
      // （由 `@base/theme` 写 `currentColor`，不依赖宿主样式表）。
      applyBranding({});
      // eslint-disable-next-line no-console -- 依赖不可用时必须留下可发现的告警（AC-S-055 派生用例）
      console.warn('[branding] 读取品牌配置失败，使用中性兜底', error);
    }
  }

  function handleLoggedIn() {
    authState = auth.state;
    router.navigate(resolution.intendedPath ?? '/');
  }

  /**
   * 启动顺序：**先恢复会话，再过守卫**。
   *
   * 反过来的话，深链会因「内存里没有令牌」被判成未登录 → 改写到登录页，
   * AC-S-068 步骤 3（直接敲 URL 落 403）就达不成。
   */
  async function boot(): Promise<void> {
    await restoreSession();
    authState = auth.state;
    booted = true;
    router.start();
  }

  onMount(() => {
    // 先落令牌，再落品牌 —— 品牌主色覆盖在令牌之上，顺序反了会被模式切换冲掉。
    setThemeMode('light');
    void loadBranding();
    void boot();
    return () => {
      router.stop();
    };
  });
</script>

{#if !booted}
  <!-- 会话恢复完成前**不渲染任何页面**：否则深链会先闪一下目标页 / 首页。 -->
  <p class="base-booting">正在恢复会话…</p>
{:else if resolution.path === '/login'}
  <Login {auth} onLoggedIn={handleLoggedIn} />
{:else if resolution.path === '/403'}
  <Forbidden onback={() => router.navigate('/')} />
{:else if resolution.path === '/admin/roles' && resolution.allowed}
  <ProtectedRoles {roles} />
{:else if resolution.route?.path === SCHEMA_ROUTE_PATH && resolution.allowed}
  <!-- 通配路由（D104）：模块 / 实体只来自 URL 参数，页面不认识任何具体实体。 -->
  {#key resolution.path}
    <SchemaPage
      {client}
      moduleId={resolution.params.moduleId ?? ''}
      entityKey={resolution.params.entityKey ?? ''}
    />
  {/key}
{:else}
  <Home {title} />
{/if}
