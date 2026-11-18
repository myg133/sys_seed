<script lang="ts">
  import { translate } from '@base/i18n';
  import { createApiClient } from '@base/sdk';
  import { toneClass } from '@base/ui';
  import { onMount } from 'svelte';

  import { applyBranding, fetchBranding } from './lib/branding';
  import { NAV_ITEMS, filterNav } from './lib/nav';

  // 同源：页面由 platform-server 的静态目录托管（本仓没有 CORS 层）。
  const client = createApiClient({ baseUrl: '' });
  const messages = { SHELL_READY: '已就绪', SHELL_HINT: '输入关键字过滤菜单' };

  let keyword = $state('');
  let title = $state('');
  const visible = $derived(filterNav(NAV_ITEMS, keyword));

  onMount(async () => {
    try {
      const branding = await fetchBranding();
      title = applyBranding(branding).title;
    } catch (error) {
      // 依赖不可用时不回落到任何旧品牌：标题留空，主色用 app.css 的中性兜底。
      // eslint-disable-next-line no-console -- 依赖不可用时必须留下可发现的告警（AC-S-055 派生用例）
      console.warn('[branding] 读取品牌配置失败，使用中性兜底', error);
    }
  });
</script>

<main>
  <h1 class={toneClass('primary')}>{title}</h1>
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

  <p class="muted">后端健康检查：{client.url('/api/health')}</p>
</main>
