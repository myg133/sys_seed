<!--
  403 页：守卫拒绝时的落点（已登录但没有权限）。

  ⚠️ 页面里**不能**出现目标页的内容 —— 目标接口的请求计数必须是 0。
-->
<script lang="ts">
  import { i18nRuntime } from '../lib/i18n';

  /** 403 页 props。 */
  interface ForbiddenProps {
    /** 返回工作台（可选：直接打开 403 深链时没有来源页）。 */
    readonly onback?: () => void;
  }

  const props: ForbiddenProps = $props();
  const i18n = i18nRuntime();
</script>

<section class="base-forbidden" data-testid="forbidden">
  <h2>403</h2>
  <p>{$i18n.t('common.forbiddenMessage')}</p>
  {#if props.onback !== undefined}
    <button type="button" onclick={() => props.onback?.()}>
      {$i18n.t('common.backToWorkbench')}
    </button>
  {/if}
</section>
