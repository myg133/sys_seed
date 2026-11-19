<!--
  输入框（薄封装）—— 逻辑与样式契约在 `@base/ui`。
-->
<script lang="ts">
  import { inputClass, type InputComponentProps } from '@base/ui';

  // props 整体取值：svelte-check 会把「解构出来的可选 props」当成必填，
  // 消费方就得把每个可选值都传一遍 —— 契约反而没法用。
  const props: InputComponentProps = $props();

  const fieldId = $derived(props.name ? `base-input-${props.name}` : undefined);
</script>

<label class="base-field" for={fieldId}>
  {#if props.label}
    <span class="base-field-label">{props.label}</span>
  {/if}
  <input
    id={fieldId}
    class={inputClass({
      disabled: props.disabled,
      error: props.error,
      class: props.class,
    })}
    name={props.name}
    type={props.type ?? 'text'}
    placeholder={props.placeholder}
    disabled={props.disabled}
    required={props.required}
    value={props.value ?? ''}
    oninput={(event) => props.oninput?.(event.currentTarget.value)}
  />
  {#if props.error}
    <span class="base-field-error">{props.error}</span>
  {/if}
</label>
