<!--
  卡片（薄封装）—— 逻辑与样式契约在 `@base/ui`。
-->
<script lang="ts">
  import { cardClass } from '@base/ui';

  const props = $props();

  const hasFooter = $derived(props.footer !== undefined);
</script>

<section class={cardClass({ title: props.title, footer: hasFooter, class: props.class })}>
  {#if props.title}
    <header class="base-card-header">{props.title}</header>
  {/if}
  <div class="base-card-body">
    {@render props.children?.()}
  </div>
  {#if props.footer}
    <footer class="base-card-footer">
      {@render props.footer()}
    </footer>
  {/if}
</section>
