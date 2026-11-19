<!--
  按钮（薄封装）—— 逻辑与样式契约在 `@base/ui`。
-->
<script lang="ts">
  import { buttonClass, type ButtonComponentProps } from '@base/ui';

  // props 整体取值：svelte-check 会把「解构出来的可选 props」当成必填，
  // 消费方就得把每个可选值都传一遍 —— 契约反而没法用。
  const props: ButtonComponentProps = $props();
</script>

<button
  class={buttonClass({
    variant: props.variant,
    size: props.size,
    disabled: props.disabled,
    type: props.type ?? 'button',
    class: props.class,
  })}
  type={props.type ?? 'button'}
  disabled={props.disabled}
  onclick={props.onclick}
>
  {@render props.children?.()}
</button>
