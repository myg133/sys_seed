<!--
  企业账号登录入口（REQ-013 / AC-S-109）。

  只负责**渲染**，不碰网络与导航：

  - `type: "oidc"` → 一个按钮，点击交给宿主（宿主 `window.location.assign`，因为要 302 到 IdP）；
  - `type: "ldap"` → 直接展示用户名 / 密码表单，提交交给宿主（`POST /api/iam/sso/ldap/login`）。

  # 为什么 LDAP 表单不折叠

  折叠要点一次才看得见入口；强制 SSO 时用户已经在找「怎么登」，多一次点击
  只是增加失败率。两种入口同时可见，语义也更容易读。

  # 为什么 providers 为空时整块不渲染

  规格要求「空数组 → 不显示企业登录区块」（**不是**禁用按钮）。
  把判定放在组件里（而不是只靠调用方）→ 调用方忘了判空也不会在页面上留一块空白。

  # 为什么不用后端给的 `label`

  后端只允许返回**错误码 + ASCII title**（REQ-021 / D113），`label` 是配置值的投影，
  不该决定界面语言。按钮文案按 `type` 取本语言的翻译（未知类型不渲染）。

  # 标签为什么不复用「用户名 / 密码」

  登录页上方已有同名的本地账号输入框；同名标签会让「页面上的唯一可访问名称」
  出现两份（真实浏览器里 `getByLabel('用户名')` 会变成歧义）。
  目录账号用「目录用户名 / 目录密码」，视觉与语义都更清楚。
-->
<script lang="ts">
  import Button from './Button.svelte';
  import Input from './Input.svelte';

  import { createValueSetter } from '../lib/handlers';
  import { i18nRuntime } from '../lib/i18n';
  import type { SsoProviderView } from '../lib/sso-api';

  /** 组件 props。 */
  interface SsoButtonsProps {
    /** 后端返回的可用 provider（空数组 → 什么都不渲染）。 */
    readonly providers: readonly SsoProviderView[];
    /** OIDC 入口被点击（参数是那个 provider，宿主据此整页跳转）。 */
    readonly onOidc: (provider: SsoProviderView) => void;
    /** LDAP 表单提交（宿主负责请求与错误文案）。 */
    readonly onLdapSubmit: (username: string, password: string) => void;
    /** 登录进行中：禁用所有入口（避免重复提交）。 */
    readonly disabled?: boolean;
  }

  const props: SsoButtonsProps = $props();
  const i18n = i18nRuntime();

  /** OIDC 入口（按 id 做 key：同一类型将来可能有多个）。 */
  const oidcProviders = $derived(props.providers.filter((provider) => provider.type === 'oidc'));
  /** LDAP 入口（设计里只有一个，多个也只渲染一张表单）。 */
  const ldapProviders = $derived(props.providers.filter((provider) => provider.type === 'ldap'));

  let ldapUsername = $state('');
  let ldapPassword = $state('');

  const setLdapUsername = createValueSetter((value) => {
    ldapUsername = value;
  });
  const setLdapPassword = createValueSetter((value) => {
    ldapPassword = value;
  });
</script>

{#if oidcProviders.length + ldapProviders.length > 0}
  <section class="base-sso" data-testid="sso-buttons">
    <h3 class="base-sso-title">{$i18n.t('common.ssoSectionTitle')}</h3>

    {#each oidcProviders as provider (provider.id)}
      <Button variant="primary" disabled={props.disabled} onclick={() => props.onOidc(provider)}>
        {$i18n.t('common.ssoOidc')}
      </Button>
    {/each}

    {#if ldapProviders.length > 0}
      <form
        class="base-sso-ldap"
        onsubmit={(event) => {
          event.preventDefault();
          props.onLdapSubmit(ldapUsername, ldapPassword);
        }}
      >
        <Input
          label={$i18n.t('common.ssoLdapUsername')}
          name="sso-ldap-username"
          value={ldapUsername}
          disabled={props.disabled}
          oninput={setLdapUsername}
        />
        <Input
          label={$i18n.t('common.ssoLdapPassword')}
          name="sso-ldap-password"
          type="password"
          value={ldapPassword}
          disabled={props.disabled}
          oninput={setLdapPassword}
        />
        <Button type="submit" disabled={props.disabled}>{$i18n.t('common.ssoLdap')}</Button>
      </form>
    {/if}
  </section>
{/if}
