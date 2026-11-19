<!--
  按钮（薄封装）—— 逻辑与样式契约在 `@base/ui`。
-->
<script lang="ts">
  import { buttonClass } from '@base/ui';

  const props = $props();
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
