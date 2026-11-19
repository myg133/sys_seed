<!--
  应用外壳（本期最小壳）：登录页 + 极简路由 + 一个受保护页 + 403 页。

  # 关键不变量

  - **守卫放行后**才调用受保护页的数据加载函数 → 403 落点时目标接口请求计数为 **0**；
  - 未登录 → 落**登录页**（不是 403）；已登录无权限 → 落 **403 页**；
  - 品牌主色由 `@base/theme` 统一处理（shell 不再自己判断颜色格式）。

  ⚠️ 脚本块写成 **JS 语法**：本仓的 eslint 配置没有给 `**/*.svelte` 配
  TypeScript 解析器，写 TS 会让 `eslint .` 报解析错误。
  路由 / 鉴权 / API 装配的**逻辑与类型**都在 `src/lib/*.ts` 里（那里是全量类型检查的）。
-->
<script lang="ts">
  import { setThemeMode } from '@base/theme';
  import { onMount, setContext } from 'svelte';

  import { applyBranding, fetchBranding } from './lib/branding';
  import { createAppApi } from './lib/api';
  import { AUTH_CONTEXT_KEY } from './lib/auth-context';
  import { createRouter, initialResolution } from './lib/router';
  import { createRoutes, toRoles } from './lib/routes';

  import Forbidden from './pages/Forbidden.svelte';
  import Home from './pages/Home.svelte';
  import Login from './pages/Login.svelte';
  import ProtectedRoles from './pages/ProtectedRoles.svelte';

  // 同源：页面由 platform-server 的静态目录托管（本仓没有 CORS 层）。
  const { client, auth } = createAppApi();

  let title = $state('');
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

  onMount(() => {
    // 先落令牌，再落品牌 —— 品牌主色覆盖在令牌之上，顺序反了会被模式切换冲掉。
    setThemeMode('light');
    void loadBranding();
    // 当前地址（可能是深链）也要过一遍守卫。
    router.start();
    return () => {
      router.stop();
    };
  });
</script>

{#if resolution.path === '/login'}
  <Login {auth} onLoggedIn={handleLoggedIn} />
{:else if resolution.path === '/403'}
  <Forbidden onback={() => router.navigate('/')} />
{:else if resolution.path === '/admin/roles' && resolution.allowed}
  <ProtectedRoles {roles} />
{:else}
  <Home {title} />
{/if}
