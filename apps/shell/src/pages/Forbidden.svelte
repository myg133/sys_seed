<!--
  403 页：守卫拒绝时的落点（已登录但没有权限）。

  ⚠️ 页面里**不能**出现目标页的内容 —— 目标接口的请求计数必须是 0。
-->
<script lang="ts">
  const props = $props();
</script>

<section class="base-forbidden" data-testid="forbidden">
  <h2>403</h2>
  <p>当前账号没有访问该页面的权限。</p>
  {#if props.onback !== undefined}
    <button type="button" onclick={() => props.onback?.()}>返回工作台</button>
  {/if}
</section>
