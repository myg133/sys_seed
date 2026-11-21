<!--
  应用工作台（`/workbench`，**登录即可**）。

  # 工作台 ≠ 导航（D279 / S-04）

  工作台是「**这个租户接入了哪些应用**」的总览 → **不按 `launch_url` 过滤**
  （刚注册、还没填入口的应用**必须**出现在这里 —— 那正是管理员最需要看到它的时候）。
  导航才是「点了去哪」，那里才要求 `launch_url` 非空。

  # `external` 入口是**整页跳转**，不经过 SPA 路由（AC-S-163 步骤 3 / D278）

  第三方应用是**独立站点**，**没有本 SPA 的组件** → 它的入口必须是
  `<a href>` 的整页跳转（`location.assign`），**不能**进 SPA 路由表：
  进表后 `resolve()` 会拿 `https://…` 去 `match()` → **静默失败**。

  「能不能打开、打开到哪」的决策是**纯函数** `appOpenAction`（`lib/app-api.ts`）——
  `svelte/server` 的 `render` 不能点击，所以点击**决策**单测、点击**链**由 E2E 兜底。

  # 文案：`app.*` 走 `t()`；导航项的 `label` **不过 `t()`**

  导航项来自 manifest 的 `menus[].label`，是**已渲染的中文文案**（`MenuSpec` 里
  没有 `labelKey`，D284）→ 直接渲染。代价：模块菜单不随语言切换（**TD-049**）。
-->
<script lang="ts">
  import type { ApiClient } from '@base/sdk';
  import { onMount, untrack } from 'svelte';

  import {
    appOpenAction,
    browserAppEnv,
    loadFrequentApps,
    loadNavigation,
    loadRecentApps,
    loadWorkspace,
    navigationClickTarget,
    recordAppAccess,
    safeLaunchUrl,
    sortWorkspace,
    type AppGroup,
    type AppPageEnv,
    type AppView,
    type NavigationItem,
  } from '../lib/app-api';
  import { describeError, i18nRuntime } from '../lib/i18n';

  /** 工作台 props。 */
  interface WorkbenchProps {
    /** API 客户端（宿主装配，页面不自己造）。 */
    readonly client: ApiClient;
    /**
     * 导航项（宿住在登录后取好，一次请求 —— `App.svelte` 的 `loadAppNavigation`）。
     *
     * `undefined` = 宿主没给（单测 / 直接渲染页面）→ 页面自己取。
     * 给了（哪怕是空数组）= 宿主已经取过 → **不再**重复请求。
     */
    readonly navigation?: readonly NavigationItem[];
    /**
     * 首屏数据（可选；与 `Audit.svelte` 的 `initial` 同一款写法）。
     *
     * 给了 = 直接铺上、**不发请求**（SSR 首帧 / 单测断言渲染结构用 ——
     * `svelte/server` 的 `render` 不跑 `onMount`，不传它就只能看到加载态）。
     */
    readonly initial?: {
      readonly groups: readonly AppGroup[];
      readonly recent: readonly AppView[];
      readonly frequent: readonly AppView[];
    };
    /** 整页跳转的环境（单测注入假件；不传 = 用 `window.location.assign`）。 */
    readonly env?: AppPageEnv;
    /** 是否在挂载时自己取数（宿主走路由 `load` 时传 `false` 避免重复请求）。 */
    readonly autoLoad?: boolean;
  }

  const props: WorkbenchProps = $props();
  const i18n = i18nRuntime();
  const env = untrack(() => props.env ?? browserAppEnv());

  /** 首屏（只取初值 —— 之后由用户操作 / 页面取数覆盖）。 */
  const initial = untrack(() => props.initial);

  /** 分组（按 category 升序、组内按 `sortOrder` 再 `displayName` —— D267）。 */
  let groups = $state<AppGroup[]>(
    initial === undefined ? [] : sortWorkspace(initial.groups),
  );
  /** 最近访问。 */
  let recent = $state<readonly AppView[]>(initial?.recent ?? []);
  /** 常用应用。 */
  let frequent = $state<readonly AppView[]>(initial?.frequent ?? []);
  /** 导航项（宿主下发优先；宿主没给时用页面自己取的那份）。 */
  let fetchedNavigation = $state<readonly NavigationItem[]>([]);
  /**
   * 最终渲染用的导航项。
   *
   * 宿主（`App.svelte`）在登录后取一次并以 prop 传进来 —— 这里 `??` 判定是
   * **有意的**：`props.navigation` 是**空数组**也算「宿主给了」（后端确实没下发
   * 任何项），此时页面**不再**发第二个请求。
   */
  const navigation = $derived(props.navigation ?? fetchedNavigation);
  let loading = $state(initial === undefined);
  /** 失败提示（空串 = 不显示）。 */
  let error = $state('');

  /** 翻译 + 错误文案（`describeError` 只取键与参数）。 */
  const translate = (key: string, params?: Record<string, string | number>): string =>
    i18n.t(key, params);

  /**
   * 打开一个应用。
   *
   * 顺序是**硬要求**：先**记访问**（UPSERT），再跳 —— 反过来整页导航会
   * 中断那次 `POST`（浏览器卸载文档时请求被取消 → `accessCount` 永远不涨）。
   *
   * 记访问失败**不挡跳转**：用户点了就是要过去，计数丢一次不是阻塞性错误。
   */
  async function handleOpen(app: AppView): Promise<void> {
    const action = appOpenAction(app);
    if (action.kind === 'none') {
      return;
    }
    try {
      await recordAppAccess(props.client, app.appId);
    } catch {
      // 访问记录失败不阻断入口（工作台上「点不开」比「计数少一次」糟糕得多）。
    }
    env.assign(action.url);
  }

  /** 取全部首屏数据（四路并发；任一路失败 → 该路为空 + 显示失败原因）。 */
  async function loadAll(): Promise<void> {
    loading = true;
    const needsNavigation = props.navigation === undefined;
    try {
      const [workspace, recentApps, frequentApps, items] = await Promise.all([
        loadWorkspace(props.client),
        loadRecentApps(props.client),
        loadFrequentApps(props.client),
        // 宿主已经取过导航 → 不再发第二个请求（`Promise.resolve([])` 不进网络）。
        needsNavigation ? loadNavigation(props.client) : Promise.resolve([]),
      ]);
      groups = sortWorkspace(workspace.groups);
      recent = recentApps;
      frequent = frequentApps;
      fetchedNavigation = items;
      error = '';
    } catch (caught) {
      // 形状不对 → 空列表（列表页不白屏）；真失败 → 显示错误码文案。
      groups = [];
      recent = [];
      frequent = [];
      fetchedNavigation = [];
      error = describeError(caught, translate);
    } finally {
      loading = false;
    }
  }

  onMount(() => {
    // 首屏数据已给（SSR / 单测 / 宿主已经取过）→ 不再发请求。
    if (props.autoLoad === false || props.initial !== undefined) {
      return;
    }
    void loadAll();
  });

  /** 应用的展示名（后端没给时退回 `appId`，避免卡片无标题）。 */
  function displayName(app: AppView): string {
    return app.displayName !== '' ? app.displayName : app.appId;
  }

  /**
   * 点击一个导航项。
   *
   * 跳转形态由纯函数 `navigationClickTarget` 判定：
   * `external` → **整页跳转**（`env.assign`）；`internal` → 交给站内链接
   * （模板里是普通 `<a href>`，浏览器自己走 SPA 路径，这里**不做**任何事）。
   */
  function handleNavigationClick(item: NavigationItem): void {
    const target = navigationClickTarget(item);
    if (target.kind === 'external') {
      env.assign(target.url);
    }
  }

  /**
   * 外部导航项的可点地址（**与工作台卡片同一道 scheme 防线**）。
   *
   * `launch_url` 是租户管理员填的自由文本 → 后端会拒 `javascript:` / `data:`，
   * 但前端仍要在**渲染层**再挡一次：`href` 里出现 `javascript:` 就是
   * 打开即执行脚本（AC-S-156 步骤 9 的安全判据在前端的对应面）。
   * 不合格 → `undefined` → 该导航项**不渲染成链接**（仍保留文案，不凭空消失）。
   */
  function externalHref(item: NavigationItem): string | undefined {
    return item.kind === 'external' ? safeLaunchUrl(item.target) : undefined;
  }
</script>

<section class="base-workbench" data-testid="app-workbench">
  <h2>{$i18n.t('nav.workbench')}</h2>

  {#if error !== ''}
    <p class="base-schema-error" role="alert">{error}</p>
  {/if}

  {#if loading}
    <p class="base-schema-loading">{$i18n.t('common.loading')}</p>
  {/if}

  {#if !loading && groups.length === 0}
    <p class="base-schema-empty" data-testid="app-workbench-empty">
      {$i18n.t('app.workbenchEmpty')}
    </p>
  {/if}

  <!-- 分组卡片：组按 category 升序、组内按 (sortOrder, displayName)。 -->
  {#each groups as group (group.category)}
    <section class="base-workbench-group" data-testid="app-workspace-group">
      <h3 data-testid="app-workspace-category">{group.category}</h3>
      <ul class="base-workbench-cards">
        {#each group.apps as app (app.appId)}
          <li class="base-card base-workbench-card" data-testid="app-workspace-card">
            <span class="base-workbench-card-name">{displayName(app)}</span>
            {#if group.category !== ''}
              <span class="base-workbench-card-category">{group.category}</span>
            {/if}
            {#if app.description !== null}
              <span class="base-schema-readonly">{app.description}</span>
            {/if}
            <!--
              入口是**整页跳转**的 `<a href>`（AC-S-163 步骤 3）：
              `launch_url` 为空 / scheme 非法 → 不渲染入口（只有提示）。
            -->
            {#if safeLaunchUrl(app.launchUrl) !== undefined}
              <a
                class="base-workbench-open"
                href={safeLaunchUrl(app.launchUrl)}
                data-testid="app-workspace-open"
                data-launch-url={safeLaunchUrl(app.launchUrl)}
                onclick={(event) => {
                  event.preventDefault();
                  void handleOpen(app);
                }}
              >
                {$i18n.t('app.open')}
              </a>
            {:else}
              <span class="base-schema-readonly" data-testid="app-workspace-no-entry">
                {$i18n.t('app.noLaunchUrl')}
              </span>
            {/if}
          </li>
        {/each}
      </ul>
    </section>
  {/each}

  <!-- 导航（`module` 项直接渲染后端的文案；`app` 项同样整页跳）。 -->
  <section class="base-workbench-section" data-testid="app-navigation">
    <h3>{$i18n.t('app.navTitle')}</h3>
    {#if navigation.length === 0}
      <p class="base-schema-empty" data-testid="app-navigation-empty">
        {$i18n.t('app.navEmpty')}
      </p>
    {:else}
      <ul class="base-workbench-nav">
        {#each navigation as item (item.source + item.target)}
          <li data-testid="app-nav-item">
            {#if item.kind === 'internal'}
              <!-- 站内项：SPA 路由（`data-kind` 让判据能区分两类，不依赖文案）。 -->
              <a href={item.target} data-kind="internal" data-testid="app-nav-internal">
                {item.label}
              </a>
            {:else}
              <!--
                外部项：**整页跳转**（`href` 是外部 URL，点击处置走 `env.assign`）。
                ⚠️ 这里**不**调 `router.navigate` —— 那是 SPA 内部导航（AC-S-163）。
                处置形态由纯函数 `navigationClickTarget` 判定（可单测、可变异）；
                `href` 过同一道 scheme 白名单（`javascript:` 不渲染成链接）。
              -->
              {#if externalHref(item) !== undefined}
                <a
                  href={externalHref(item)}
                  data-kind="external"
                  data-testid="app-nav-external"
                  onclick={(event) => {
                    event.preventDefault();
                    handleNavigationClick(item);
                  }}
                >
                  {item.label}
                </a>
              {:else}
                <!-- 地址不合法 → 只显示文案，不给可点入口（不静默丢掉这一项）。 -->
                <span data-kind="external-blocked" data-testid="app-nav-external-blocked">
                  {item.label}
                </span>
              {/if}
            {/if}
          </li>
        {/each}
      </ul>
    {/if}
  </section>

  <!-- 最近访问 / 常用应用（各自独立区块；空列表各自提示）。 -->
  <section class="base-workbench-section" data-testid="app-recent">
    <h3>{$i18n.t('app.recentTitle')}</h3>
    {#if recent.length === 0}
      <p class="base-schema-empty">{$i18n.t('app.sectionEmpty')}</p>
    {:else}
      <ul class="base-workbench-nav">
        {#each recent as app (app.appId)}
          <li data-testid="app-recent-item">{displayName(app)}</li>
        {/each}
      </ul>
    {/if}
  </section>

  <section class="base-workbench-section" data-testid="app-frequent">
    <h3>{$i18n.t('app.frequentTitle')}</h3>
    {#if frequent.length === 0}
      <p class="base-schema-empty">{$i18n.t('app.sectionEmpty')}</p>
    {:else}
      <ul class="base-workbench-nav">
        {#each frequent as app (app.appId)}
          <li data-testid="app-frequent-item">{displayName(app)}</li>
        {/each}
      </ul>
    {/if}
  </section>
</section>
