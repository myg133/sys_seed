<!--
  语言切换器（AC-S-099 步骤 2 / 5）。

  切换后**不刷新页面**：`setLocale` 通知订阅者，`$i18n` 的读者立即重新渲染。
  语言清单来自 `@base/i18n`（文件名 = 已注册语言），这里不硬编码任何语言码。
-->
<script lang="ts">
  import { i18nRuntime, localeOptions } from '../lib/i18n';

  const i18n = i18nRuntime();
  const options = localeOptions();
</script>

<label class="base-language-switcher">
  <span class="base-language-label">{$i18n.t('common.language')}</span>
  <select
    data-testid="language-switcher"
    value={$i18n.locale}
    onchange={(event) => i18n.setLocale(event.currentTarget.value)}
  >
    {#each options as option (option.code)}
      <option value={option.code}>{option.label}</option>
    {/each}
  </select>
</label>
