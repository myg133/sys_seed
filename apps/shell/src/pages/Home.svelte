<!--
  工作台（首页）：品牌标题 + 菜单过滤（REQ-010 的既有 E2E 依赖这里的结构）。
-->
<script lang="ts">
  import { translate } from '@base/i18n';
  import { toneClass } from '@base/ui';

  import Can from '../components/Can.svelte';
  import { NAV_ITEMS, filterNav } from '../lib/nav';

  const props = $props();

  const messages = { SHELL_READY: '已就绪', SHELL_HINT: '输入关键字过滤菜单' };

  let keyword = $state('');
  const visible = $derived(filterNav(NAV_ITEMS, keyword));
</script>

<main>
  <h1 class={toneClass('primary')}>{props.title}</h1>
  <p>{translate(messages, 'SHELL_READY')}</p>

  <label>
    {translate(messages, 'SHELL_HINT')}
    <input bind:value={keyword} placeholder="关键字" />
  </label>

  <ul>
    {#each visible as item (item.key)}
      <li><a href={item.path}>{item.label}</a></li>
    {/each}
  </ul>

  <!-- 无权限时这段内容**不渲染**（不是 display:none）—— AC-S-067。 -->
  <Can permission="rbac:role:write" fallback="无权限">
    <a class="base-home-action" href="/admin/roles">新建角色</a>
  </Can>
</main>
