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
  import type { AuditLoadResult } from './lib/audit-api';
  import { i18nRuntime } from './lib/i18n';
  import {
    OAUTH_AUTHORIZE_ROUTE_PATH,
    OAUTH_DEVICE_ROUTE_PATH,
    OAUTH_GRANTS_ROUTE_PATH,
    OAUTH_LOGOUT_ROUTE_PATH,
    resolveLoginRedirect,
    type OAuthGrantsLoadResult,
  } from './lib/oauth-api';
  import { createRouter, initialResolution } from './lib/router';
  import { createRoutes, AUDIT_ROUTE_PATH, SCHEMA_ROUTE_PATH, toRoles } from './lib/routes';
  import { SSO_CALLBACK_ROUTE_PATH } from './lib/sso-api';

  import LanguageSwitcher from './components/LanguageSwitcher.svelte';
  import LocaleInfo from './components/LocaleInfo.svelte';

  import Audit from './pages/Audit.svelte';
  import Forbidden from './pages/Forbidden.svelte';
  import Home from './pages/Home.svelte';
  import Login from './pages/Login.svelte';
  import OAuthAuthorize from './pages/OAuthAuthorize.svelte';
  import OAuthDevice from './pages/OAuthDevice.svelte';
  import OAuthGrants from './pages/OAuthGrants.svelte';
  import OAuthLogout from './pages/OAuthLogout.svelte';
  import ProtectedRoles from './pages/ProtectedRoles.svelte';
  import SchemaPage from './pages/SchemaPage.svelte';
  import SsoCallback from './pages/SsoCallback.svelte';

  // 同源：页面由 platform-server 的静态目录托管（本仓没有 CORS 层）。
  // 租户在登录时由用户填（D98），这里不传死值。
  const { client, auth, restoreSession } = createAppApi();
  const i18n = i18nRuntime();

  let title = $state('');
  let booted = $state(false);
  let authState = $state(auth.state);
  let roles = $state(toRoles(undefined));
  /** 审计页首屏（由路由的 `load` 取；`undefined` = 还没取到）。 */
  let audit = $state<AuditLoadResult | undefined>(undefined);
  /** 「我的授权」页首屏（由路由的 `load` 取；`undefined` = 还没取到）。 */
  let oauthGrants = $state<OAuthGrantsLoadResult | undefined>(undefined);
  let resolution = $state(initialResolution(window.location.pathname));

  const routes = createRoutes(
    client,
    (loaded) => {
      roles = toRoles(loaded);
    },
    (result) => {
      audit = result;
    },
    (result) => {
      oauthGrants = result;
    },
  );

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
      console.warn('[branding] failed to read branding config; using neutral fallback', error);
    }
  }

  /**
   * 读取租户级 i18n 上下文（D110 / D116）。
   *
   * 后端未就绪（404 / 网络失败）时**不阻塞启动**：本地偏好与平台默认值仍然可用。
   * 用户偏好（`sessionStorage`）已经随请求带上去（`?locale=` / `?timezone=`）。
   */
  async function loadI18nContext(): Promise<void> {
    try {
      await i18n.refreshContext();
    } catch (error) {
      // eslint-disable-next-line no-console -- 接口不可用时必须可发现（否则「租户时区没生效」无从排查）
      console.warn(
        '[i18n] failed to load /api/i18n/context; using local preference and defaults',
        error,
      );
    }
  }

  /**
   * 登录成功后的落点。
   *
   * # `?return_to=` 优先（REQ-022 / D221）
   *
   * 平台的 `intendedPath` **只有 pathname**（`router.ts:261`）→ 未登录回跳会
   * **丢掉全部 query**（授权参数 / `user_code`）→ 授权页与设备码页改成
   * **公开路由**，由页面自己把**完整 URL**（`pathname + search`）编码进
   * `?return_to=` 交给登录页。
   *
   * - `readReturnTo` 会**校验** `return_to`（站内相对路径；不合格 → `undefined`）
   *   —— 这是**开放重定向**的防线，不是可选优化；
   * - 用 **`location.replace`**（不是 `href`）→ 回跳地址**不进浏览器历史**
   *   （与 SSO 回调的 `?ticket=` 同一款理由：安全约束 §8）；
   * - 没有 `return_to` → **保持现行为**（跳 `intendedPath` / 首页）。
   */
  function handleLoggedIn() {
    authState = auth.state;
    const redirect = resolveLoginRedirect(window.location.search, resolution.intendedPath);
    if (redirect.kind === 'replace') {
      // 整页跳转 → query 一个不丢。页面会在新文档里自己加载 i18n 上下文。
      window.location.replace(redirect.url);
      return;
    }
    router.navigate(redirect.path);
    // 登录后才拿得到租户级配置（`/api/i18n/context` 需认证）。
    void loadI18nContext();
  }

  /**
   * SSO 回调成功后的落点。
   *
   * `replace: true` 是安全要求（安全约束 §8）：地址栏里的 `?ticket=` 是一次性凭据，
   * 用 push 会在历史记录里留一条带 ticket 的地址。
   */
  function handleSsoLoggedIn() {
    authState = auth.state;
    router.navigate('/', { replace: true });
    void loadI18nContext();
  }

  /** 回调页的「返回登录页」（缺 ticket / ticket 已失效）。 */
  function handleBackToLogin() {
    router.navigate('/login', { replace: true });
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
    // 会话恢复后再读租户配置：未登录时该接口是 401，拿不到租户级时区/币种。
    void loadI18nContext();
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

<!--
  语言切换器常驻：语言 / 时区 / 金额格式都要能在任意页面切换与观察
  （AC-S-099 步骤 5：切换后**同一节点**更新，不刷新页面）。
-->
<header class="base-shell-topbar">
  <LanguageSwitcher />
  <LocaleInfo />
</header>

{#if !booted}
  <!-- 会话恢复完成前**不渲染任何页面**：否则深链会先闪一下目标页 / 首页。 -->
  <p class="base-booting">{$i18n.t('common.booting')}</p>
{:else if resolution.path === '/login'}
  <Login {auth} {client} onLoggedIn={handleLoggedIn} />
{:else if resolution.path === SSO_CALLBACK_ROUTE_PATH}
  <!-- P4：回调交接页（公开路由）。ticket 只在这里被消费一次。 -->
  <SsoCallback {auth} onLoggedIn={handleSsoLoggedIn} onBackToLogin={handleBackToLogin} />
{:else if resolution.path === OAUTH_AUTHORIZE_ROUTE_PATH}
  <!--
    REQ-022：授权页 / 同意页（**公开路由**，D221）。
    未登录由**页面自己**处理（渲染「登录并继续」）→ 守卫不参与。
  -->
  <OAuthAuthorize {client} authenticated={authState.authenticated} />
{:else if resolution.path === OAUTH_DEVICE_ROUTE_PATH}
  <!-- REQ-022：设备码批准页（**公开路由**，D221；query 里带 `user_code`）。 -->
  <OAuthDevice {client} authenticated={authState.authenticated} />
{:else if resolution.path === OAUTH_LOGOUT_ROUTE_PATH}
  <!-- REQ-022：登出页（**公开路由**；front-channel 的 iframe 由前端渲染）。 -->
  <OAuthLogout {client} />
{:else if resolution.path === OAUTH_GRANTS_ROUTE_PATH && resolution.allowed}
  <!--
    REQ-022：「我的授权」页：`resolution.allowed` 是**硬要求** ——
    守卫不放行时**路由的 `load` 不会被调用**、页面也不渲染
    → `GET /api/oauth/grants` 的请求计数为 **0**。
  -->
  <OAuthGrants {client} initial={oauthGrants} />
{:else if resolution.path === '/403'}
  <Forbidden onback={() => router.navigate('/')} />
{:else if resolution.path === AUDIT_ROUTE_PATH && resolution.allowed}
  <!--
    审计查询页（REQ-014 / AC-S-132）：`resolution.allowed` 是**硬要求** ——
    守卫不放行时**路由的 `load` 不会被调用**、页面也不渲染
    → `GET /api/audit/events` 的请求计数为 **0**。
  -->
  <Audit {client} initial={audit} />
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
