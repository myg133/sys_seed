<!--
  `<Can>` —— 有权限才渲染子内容（AC-S-067）。

  # 为什么组件落在应用侧

  包的构建是 `tsc`（`.svelte` 进不了产物），引 `svelte-package` 又是**新增依赖**；
  所以 `@base/auth` 出**判定**（`can()`），组件由宿主应用提供（rev 2 裁定 #7）。

  # 无权限时「不渲染」，不是「隐藏」

  `display:none` 的内容**仍在 DOM 里** —— 用户在开发者工具里能看到、
  复制粘贴也能拿到。这里的语义是：**子内容根本不进 DOM**。

  落点：无权限且给了 `fallback` → 渲染 `fallback`（纯 `<span>`）；
  无权限且没给 `fallback` → **什么都不渲染**。

  ⚠️ 类型契约住在 `@base/auth`（`CanComponentProps`）——
  包不能产出 `.svelte` 组件（构建是 `tsc`），但**类型**可以、也应该由包提供，
  否则每个宿主应用都要各写一份 props 接口。
-->
<script lang="ts">
  import { can } from '@base/auth';
  import type { CanComponentProps } from '@base/auth';
  import type { Snippet } from 'svelte';

  import { useAuthContext } from '../lib/auth-context';

  // props 整体取值：svelte-check 会把「解构出来的可选 props」当成必填，
  // 消费方就得把每个可选值都传一遍 —— 契约反而没法用。
  //
  // `Snippet` 作为泛型实参：包的契约保持框架无关，
  // 具体内容类型由**宿主框架**给出（这里是 Svelte）。
  const props: CanComponentProps<Snippet> = $props();

  const context = useAuthContext();
  const current = $derived(props.state ?? context?.state);
  const allowed = $derived(current !== undefined && can(current, props.permission));
</script>

{#if allowed}
  {@render props.children?.()}
{:else if props.fallback !== undefined}
  <span>{props.fallback}</span>
{/if}
