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

  ⚠️ 脚本块写成 **JS 语法**：本仓的 eslint 配置没有给 `.svelte` 文件配
  TypeScript 解析器，写 TS 会让 `eslint .` 直接报解析错误。
  类型契约在 `@base/auth` / `@base/ui` 里（那里是全量类型检查的）。
-->
<script lang="ts">
  import { can } from '@base/auth';

  import { useAuthContext } from '../lib/auth-context';

  // props 整体取值：svelte-check 会把「解构出来的可选 props」当成必填，
  // 消费方就得把每个可选值都传一遍 —— 契约反而没法用。
  interface CanProps {
    permission: string;
    state?: import('@base/auth').AuthState;
    children?: import('svelte').Snippet;
    fallback?: string;
  }
  const props: CanProps = $props();

  const context = useAuthContext();
  const current = $derived(props.state ?? context?.state);
  const allowed = $derived(current !== undefined && can(current, props.permission));
</script>

{#if allowed}
  {@render props.children?.()}
{:else if props.fallback !== undefined}
  <span>{props.fallback}</span>
{/if}
