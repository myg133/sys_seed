<!--
  卡片（薄封装）—— 逻辑与样式契约在 `@base/ui`。
-->
<script lang="ts">
  import { cardClass, type CardComponentProps } from '@base/ui';

  // props 整体取值：svelte-check 会把「解构出来的可选 props」当成必填，
  // 消费方就得把每个可选值都传一遍 —— 契约反而没法用。
  const props: CardComponentProps = $props();

  // 组件的 `footer` 是**页脚内容**（给了就渲染页脚）；
  // 逻辑契约里的 `footer` 是布尔开关 —— 这里由「有没有内容」推出来。
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
