<!--
  对话框（薄封装）—— 逻辑与样式契约在 `@base/ui`。

  没有 `svelte-*` 组件库可用（零新依赖），所以用普通的 `div` + `role="dialog"`。

  带参数的键盘处理器在 `src/lib/handlers.ts` 里造（那里有完整类型）。
-->
<script lang="ts">
  import { dialogClass, type DialogComponentProps } from '@base/ui';

  import { createEscapeHandler } from '../lib/handlers';

  // props 整体取值：svelte-check 会把「解构出来的可选 props」当成必填，
  // 消费方就得把每个可选值都传一遍 —— 契约反而没法用。
  const props: DialogComponentProps = $props();

  const open = $derived(props.open ?? false);
  const closable = $derived(props.closable ?? true);

  const onkeydown = createEscapeHandler(
    () => open && closable,
    () => {
      if (closable) props.onclose?.();
    },
  );
</script>

<svelte:window {onkeydown} />

{#if open}
  <!-- 遮罩用 button：可聚焦、可回车关闭（不用 div + onclick，避开 a11y 告警）。 -->
  <button
    type="button"
    class="base-dialog-mask"
    aria-label="关闭对话框"
    onclick={() => {
      if (closable) props.onclose?.();
    }}
  ></button>
  <div
    class={dialogClass({
      open,
      title: props.title,
      width: props.width,
      closable,
      class: props.class,
    })}
    role="dialog"
    aria-modal="true"
    style={props.width === undefined ? undefined : `width: ${props.width}`}
  >
    {#if props.title}
      <header class="base-dialog-header">
        {props.title}
        {#if closable}
          <button type="button" class="base-dialog-close" onclick={() => props.onclose?.()}
            >×</button
          >
        {/if}
      </header>
    {/if}
    <div class="base-dialog-body">
      {@render props.children?.()}
    </div>
    {#if props.footer}
      <footer class="base-dialog-footer">
        {@render props.footer()}
      </footer>
    {/if}
  </div>
{/if}
