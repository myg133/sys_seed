<!--
  工作台（首页）：品牌标题 + 菜单过滤（REQ-010 的既有 E2E 依赖这里的结构）。

  # 全部文案走 `t()`（AC-S-099 步骤 4）

  菜单标签、提示、占位符、无权限文案都由 i18n 渲染；语言切换后
  `$derived` 会随 `$i18n` 重算（同一节点更新，不刷新页面）。
-->
<script lang="ts">
  import { toneClass } from '@base/ui';

  import Can from '../components/Can.svelte';

  import { i18nRuntime } from '../lib/i18n';
  import { NAV_ITEMS, filterNav, localizeNav } from '../lib/nav';

  /** 首页 props。 */
  interface HomeProps {
    /** 品牌标题（来自品牌配置，不是 i18n 文案）。 */
    readonly title: string;
  }

  const props: HomeProps = $props();
  const i18n = i18nRuntime();

  let keyword = $state('');
  // 在 `$derived` 里调用 `$i18n.t` → 语言切换会重新求值（菜单跟着变）。
  const items = $derived(localizeNav(NAV_ITEMS, (key) => $i18n.t(key)));
  const visible = $derived(filterNav(items, keyword));
</script>

<main>
  <h1 class={toneClass('primary')}>{props.title}</h1>
  <p>{$i18n.t('common.shellReady')}</p>

  <label>
    {$i18n.t('common.shellHint')}
    <input bind:value={keyword} placeholder={$i18n.t('common.searchKeyword')} />
  </label>

  <ul>
    {#each visible as item (item.key)}
      <li><a href={item.path}>{item.label}</a></li>
    {/each}
  </ul>

  <!-- 无权限时这段内容**不渲染**（不是 display:none）—— AC-S-067。 -->
  <Can permission="rbac:role:write" fallback={$i18n.t('common.noPermission')}>
    <a class="base-home-action" href="/admin/roles">{$i18n.t('common.createRole')}</a>
  </Can>
</main>
