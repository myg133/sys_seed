<!--
  本地化信息（AC-S-094 / AC-S-098 在壳层的真实使用点）。

  - 时间：`formatDate` —— 时区渲染**只在前端**（D114），浏览器自带 tzdb；
  - 金额：`formatMoney` —— 小数位由**货币 exponent** 决定，不是 locale；
    这里展示的是「零金额的格式样例」，不是业务数据（本轮不动业务表）。
-->
<script lang="ts">
  import { i18nRuntime } from '../lib/i18n';

  const i18n = i18nRuntime();
  // 渲染时刻取一次即可：这个区块展示的是「格式」而不是实时时钟。
  const nowIso = new Date().toISOString();
</script>

<dl class="base-locale-info">
  <dt>{$i18n.t('common.timezone')}</dt>
  <dd data-testid="i18n-timezone">{$i18n.timezone}</dd>
  <dt>{$i18n.t('common.currentTime')}</dt>
  <dd data-testid="i18n-time">{$i18n.formatDate(nowIso)}</dd>
  <dt>{$i18n.t('common.amountPreview')}</dt>
  <dd data-testid="i18n-money">{$i18n.formatMoney(0, $i18n.currency)}</dd>
</dl>
