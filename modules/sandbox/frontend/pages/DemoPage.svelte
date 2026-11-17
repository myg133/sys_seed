<script lang="ts">
  import { sandboxRoutes } from '../routes';

  const routes = sandboxRoutes;
</script>

<section class="sandbox-demo">
  <h1>沙箱模块</h1>
  <p>这是沙箱模块的最小前端页面，用作二次开发的模板。</p>
  <ul>
    {#each routes as route (route.path)}
      <li>
        {route.label}
        <code>{route.path}</code>
        <small>权限点：{route.permission}</small>
      </li>
    {/each}
  </ul>
</section>
