<script lang="ts">
  import { translate } from '@base/i18n';
  import { createApiClient } from '@base/sdk';
  import { toneClass } from '@base/ui';

  import { NAV_ITEMS, filterNav } from './lib/nav';

  const client = createApiClient({ baseUrl: 'http://127.0.0.1:8080' });
  const messages = { SHELL_READY: '底座壳已就绪', SHELL_HINT: '输入关键字过滤菜单' };

  let keyword = $state('');
  const visible = $derived(filterNav(NAV_ITEMS, keyword));
</script>

<main>
  <h1 class={toneClass('primary')}>底座壳</h1>
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
