# REQ-021 修复轮 2 —— D119「平台配置层加入 i18n 协商」

- **需求**：REQ-021（i18n / 本地化）
- **本轮范围**：**只实现 D119** —— 把「平台配置层」加入协商链。不重做其它实现、不重构。
- **分支 / 工作目录**：`feature/REQ-021` @ `/home/myg133/Projects/system_seed/feature-REQ-021`
- **提交状态**：**未提交**（按 BA 要求，Dev 不执行 `git commit` / `git push` / `git checkout` / `git stash` / `git reset` / `git restore`）
- **日期**：2026-11-19

---

## 1. 改动清单

### 1.1 `crates/platform-server/src/i18n.rs`（唯一改动的实现文件）

| 位置 | 改动 | 为什么 |
|------|------|--------|
| L13–L31（模块文档「优先级」） | 协商链由四级改为**五级**：`用户显式偏好 > Accept-Language > 租户配置 > 平台配置 > manifest 默认`，并写明「平台层的值就是组织级默认值」与 `source` 的 5 值集合 | D119 裁定；原文与代码必须一致 |
| L41–L43（模块文档「时区不看 Accept-Language」） | 时区 `source` 取值集合补上 `platform` | 同上（注释与代码一致） |
| L130–L146 `Setting::layer_source()`（**新增**） | 唯一的层级判定入口，**只按 `self.source` 分派**：租户/空间 → `Tenant`；平台 → `Platform`；默认 → `None` | 任务要求「不要新造一套判定逻辑，直接按 `self.source` 分派」 |
| L148–L154 `Setting::is_explicit()` | 由 `matches!(Tenant \| Space)` 改为 `self.layer_source().is_some()`，即**覆盖 `Platform`** | D119：`is_explicit()` 必须覆盖平台层 |
| L173–L186 `LocaleSource` | **新增 `Platform` 取值**（`#[serde(rename_all = "kebab-case")]` → 序列化为小写 `"platform"`） | D119：`source` 集合扩为 5 值 |
| L188–L199 `LocaleSource::as_str()` | 新增 `Self::Platform => "platform"`（与 `ConfigSource::as_str()` 一致） | 同上 |
| L599–L639 `negotiate_locale()` | 第 3/4 级合并为一次 `layer_source()` 判定（租户/空间 → `Tenant`，平台 → `Platform`） | D119 的语言层 |
| L647–L659 `negotiate_timezone()` | 同上（`tenant.is_explicit()` → `tenant.layer_source()`） | D119 的时区层 |
| L664–L673 `resolve_currency()` | 同上（`tenant.is_explicit()` → `tenant.layer_source()`） | D119 的币种层 |
| L560–L566 `LocaleLayers::tenant` 字段文档 | 从「租户配置的解析结果」改为「租户层 / 空间层 / 平台层 / 默认层**已解析好**的配置结果」 | 字段实际承载的是**已解析的那一层** |
| L806–L836 `FieldSources` / `ContextResponse` 文档 | 写明 `source` 是 5 值集合 | 响应形状不变，只取值集合变大 |
| L1062–L1074 测试辅助函数 | `tenant_setting(...)` → `setting_with_source(...)`（**纯改名**，语义不变：来源由调用方给） | 该构造函数现在也用来造**平台层**的值，原名会误导 |
| L1343–L1366 `currency_has_tenant_platform_and_default_tiers` | **改名**（原 `currency_has_only_tenant_and_default_tiers`）+ 补平台层用例 | 见 §5 |
| L1368–L1420 `d119_each_config_layer_maps_to_its_own_source`（**新增**） | 钉死层级→来源的映射、5 值集合与 `"platform"` 的序列化 | D119 契约 |
| L1422–L1460 `tc021_096_...`（**新增**） | 纯函数层的 TC-021-096 | 见 §4 |
| L1462–L1502 `tc021_097_...`（**新增**） | 纯函数层的 TC-021-097 | 见 §4 |

### 1.2 `crates/platform-server/tests/i18n_api.rs`（HTTP / 路由层）

| 位置 | 改动 | 为什么 |
|------|------|--------|
| L33–L36 | 新增两个夹具租户：`TENANT_T3`（平台层+租户层都配 → 租户赢）、`TENANT_T4`（**只有平台层**配了） | D119 需要能分别构造两种层级 |
| L43–L57 `FakeTokens` | 令牌决定租户：`good`→T1、`good-t3`→T3、`good-t4`→T4 | 认证中间件要求「请求头租户 == 令牌租户」，切租户必须切令牌 |
| L66–L115 `FakeSettings` | T3 返回**租户层**的值（`ConfigSource::Tenant`），T4 返回**平台层**的值（`ConfigSource::Platform`），T1/T2 行为不变 | 复现「配置层已解析好、协商层只如实报层级」 |
| L155–L178 `state_with_locales()`（**新增**） | 可换已注册语言集合；`state()` 保持 `{zh-CN, en-US}` 不变 | D119 用例需要「租户值 ≠ 平台值 ≠ manifest 默认值」三者两两不同 |
| L420–L456 `tc021_096_...`（**新增**） | HTTP 层 TC-021-096 | 见 §4 |
| L458–L487 `tc021_097_...`（**新增**） | HTTP 层 TC-021-097 | 见 §4 |
| L489–L513 `the_platform_layer_still_loses_to_the_accept_language_header`（**新增**） | `Accept-Language` 仍压过平台层 | 五级链没有被平台层插队 |

### 1.3 `crates/platform-server/tests/i18n_settings_real_db.rs`（真实库层）

| 位置 | 改动 | 为什么 |
|------|------|--------|
| L1–L14 文件文档 | 补 D119 与「平台层参与协商」的说明 | 注释与事实一致 |
| L20 | 新增 `use platform_core::extension::config::ConfigSource;` | 断言用 |
| L70–L131 `setup()` | ① 清理**平台级** i18n 行（+推进平台 epoch）；② 返回的池由「应用角色池」改为「**管理员池**」 | ①夹具必须自足（见 §5.3）；②应用角色对 `platform_config` 只有 SELECT，无法做夹具清理（实测 42501） |
| L138–L158 `clear_platform_i18n()`（**新增**） | 删平台级 i18n 行 + `UPDATE platform_config_epoch SET epoch = epoch + 1 WHERE scope_key='platform'` | 写通道 `platform_config_upsert` 本来就在同事务自增 epoch，直接删除必须自己补上 |
| L253–L348 `the_platform_layer_takes_part_in_the_negotiation_and_the_tenant_wins_over_it`（**新增**） | 真实库复现 M1 现场：平台层写 → `source=platform`；租户再写 → `source=tenant` | 见 §4 |
| L226（注释） | 「协商的第 4 级」→「协商的**第 5 级**」 | 链变了，注释要跟着变 |

### 1.4 **未触碰**（按任务要求）

- `packages/**`、`apps/**`、`e2e/**`（前端本轮不动）
- `AGENTS.md`
- 已应用的迁移文件（`2026111800*.sql`、`20261119000100_*`、`20261119000200_i18n_fx_rate.*`）—— **未改**，checksum 不变
- 前两轮已交付的实现（`platform-core/src/{titles,money}.rs`、`platform-db/src/fx.rs`、`i18n_no_chinese_titles.rs` 等）—— **未改**

---

## 2. `source` 取值集合的最终形态（5 值）

| `LocaleSource` 变体 | `as_str()` | serde 序列化 | 何时出现 |
|---|---|---|---|
| `User` | `"user"` | `"user"` | 请求带 `?locale=`（已注册） |
| `AcceptLanguage` | `"accept-language"` | `"accept-language"` | `Accept-Language` 命中已注册语言 |
| `Tenant` | `"tenant"` | `"tenant"` | **租户/空间层**的配置生效 |
| `Platform` | `"platform"` | `"platform"` | **只有平台层**配置生效（D119 新增） |
| `Default` | `"default"` | `"default"` | 谁都没配 → manifest 默认值 |

- 序列化结果：`serde_json::to_value(LocaleSource::Platform) == "platform"`（**小写**，与 `ConfigSource::as_str()` 一致）。
- **响应形状不变**：`{locale, timezone, currency, source, sources{locale, timezone, currency}}` —— 只是 `source` 的**取值集合**多了 `platform`。
- `sources.locale` / `sources.timezone` / `sources.currency` 同样可以是 `"platform"`。
- 时区的 `source` **永不**出现 `"accept-language"`（时区协商不看请求头）。

---

## 3. 三条协商函数各自的判定逻辑

三个函数**共用**同一个判定入口 `Setting::layer_source()`（只按 `self.source` 分派）：

```rust
// crates/platform-server/src/i18n.rs:140
pub fn layer_source(&self) -> Option<LocaleSource> {
    match self.source {
        ConfigSource::Tenant | ConfigSource::Space => Some(LocaleSource::Tenant),
        ConfigSource::Platform => Some(LocaleSource::Platform),
        ConfigSource::Default => None,
    }
}

// crates/platform-server/src/i18n.rs:152
pub fn is_explicit(&self) -> bool {
    self.layer_source().is_some()
}
```

### 3.1 `negotiate_locale()`（L599）

```rust
    // 1) 用户显式偏好：只有已注册才生效，否则视为「没设」。
    if let Some(user) = layers.user
        && layers.registered.contains(user)
    {
        return (user.to_string(), LocaleSource::User);
    }

    // 2) Accept-Language：按 q 降序取第一个已注册的。
    if let Some(raw) = layers.accept_language {
        for range in parse_accept_language(raw) {
            if range.quality == 0 { continue; }
            if range.is_wildcard() {
                return (layers.tenant.manifest_default.clone(), LocaleSource::Default);
            }
            if layers.registered.contains(&range.tag) {
                return (range.tag, LocaleSource::AcceptLanguage);
            }
        }
    }

    // 3) 租户 / 空间层配置，4) 平台层配置（D119）。
    //    值已注册才算数 —— 配置可能写在翻译包之前。
    if let Some(layer) = layers.tenant.layer_source()
        && layers.registered.contains(&layers.tenant.value)
    {
        return (layers.tenant.value.clone(), layer);
    }

    // 5) 平台默认（manifest 的 `default`，裁定 #17）。
    (layers.tenant.manifest_default.clone(), LocaleSource::Default)
```

### 3.2 `negotiate_timezone()`（L647）

```rust
pub fn negotiate_timezone(user: Option<&str>, tenant: &Setting) -> (String, LocaleSource) {
    if let Some(user) = user
        && platform_config::registry::is_iana_timezone(user)
    {
        return (user.to_string(), LocaleSource::User);
    }
    // 租户 / 空间层 → `tenant`；平台层 → `platform`（D119）。
    if let Some(layer) = tenant.layer_source() {
        return (tenant.value.clone(), layer);
    }
    (tenant.manifest_default.clone(), LocaleSource::Default)
}
```

### 3.3 `resolve_currency()`（L664）

```rust
pub fn resolve_currency(tenant: &Setting) -> (String, LocaleSource) {
    // 租户 / 空间层 → `tenant`；平台层 → `platform`（D119）。
    if let Some(layer) = tenant.layer_source() {
        return (tenant.value.clone(), layer);
    }
    (tenant.manifest_default.clone(), LocaleSource::Default)
}
```

**取值优先级不变**：租户 > 平台（跨层优先级由**配置解析器**保证 —— `DbConfigResolver::resolve` 先查租户层再查平台层，见 `crates/platform-config/src/resolver.rs:263`）。平台层**只在租户没配时**生效。

---

## 4. TC-021-096 / TC-021-097 的测试名与断言原文

### 4.1 纯函数层（`crates/platform-server/src/i18n.rs`）

**TC-021-096**

- 测试名：`i18n::tests::tc021_096_the_platform_layer_is_used_when_the_tenant_has_not_configured_it`
- 断言原文：

```rust
        // 语言：平台层的值生效，来源报 `platform`。
        let platform_locale = setting_with_source("en-US", ConfigSource::Platform, "zh-CN");
        let (locale, locale_source) = negotiate_locale(LocaleLayers {
            user: None,
            accept_language: None,
            tenant: &platform_locale,
            registered: &reg,
        });
        assert_eq!(locale, "en-US", "平台级的语言必须生效");
        assert_eq!(locale_source, LocaleSource::Platform, "如实报出是平台层");
        assert_ne!(locale, "zh-CN", "不能退回 manifest 默认值");
        assert_ne!(locale_source, LocaleSource::Tenant, "平台层不是租户层");
        assert_ne!(locale_source, LocaleSource::Default, "平台层不是默认层");

        // 时区：平台层的值生效，来源报 `platform`。
        let platform_timezone =
            setting_with_source("America/New_York", ConfigSource::Platform, "Asia/Shanghai");
        let (timezone, timezone_source) = negotiate_timezone(None, &platform_timezone);
        assert_eq!(timezone, "America/New_York", "平台级的时区必须生效");
        assert_eq!(timezone_source, LocaleSource::Platform, "如实报出是平台层");
        assert_ne!(timezone, "Asia/Shanghai", "不能退回 manifest 默认值");

        // 币种：平台层的值生效，来源报 `platform`。
        let platform_currency = setting_with_source("USD", ConfigSource::Platform, "CNY");
        let (currency, currency_source) = resolve_currency(&platform_currency);
        assert_eq!(currency, "USD", "平台级的币种必须生效");
        assert_eq!(currency_source, LocaleSource::Platform, "如实报出是平台层");
        assert_ne!(currency, "CNY", "不能退回 manifest 默认值");
```

**TC-021-097**

- 测试名：`i18n::tests::tc021_097_the_tenant_layer_wins_over_the_platform_layer`
- 断言原文：

```rust
        // 语言：租户层的值（ja-JP）生效，来源报 `tenant`。
        let tenant_locale = setting_with_source("ja-JP", ConfigSource::Tenant, "zh-CN");
        let (locale, locale_source) = negotiate_locale(LocaleLayers {
            user: None,
            accept_language: None,
            tenant: &tenant_locale,
            registered: &reg,
        });
        assert_eq!(locale, "ja-JP", "租户层的语言必须压过平台层");
        assert_eq!(locale_source, LocaleSource::Tenant, "如实报出是租户层");
        assert_ne!(locale, "en-US", "不能取平台层的值");
        assert_ne!(locale_source, LocaleSource::Platform, "租户层不是平台层");

        // 时区：租户层的值生效，来源报 `tenant`。
        let tenant_timezone =
            setting_with_source("Europe/London", ConfigSource::Tenant, "Asia/Shanghai");
        let (timezone, timezone_source) = negotiate_timezone(None, &tenant_timezone);
        assert_eq!(timezone, "Europe/London", "租户层的时区必须压过平台层");
        assert_eq!(timezone_source, LocaleSource::Tenant, "如实报出是租户层");
        assert_ne!(timezone, "America/New_York", "不能取平台层的值");

        // 币种：租户层的值生效，来源报 `tenant`。
        let tenant_currency = setting_with_source("JPY", ConfigSource::Tenant, "CNY");
        let (currency, currency_source) = resolve_currency(&tenant_currency);
        assert_eq!(currency, "JPY", "租户层的币种必须压过平台层");
        assert_eq!(currency_source, LocaleSource::Tenant, "如实报出是租户层");
        assert_ne!(currency, "USD", "不能取平台层的值");
```

### 4.2 HTTP / 路由层（`crates/platform-server/tests/i18n_api.rs`）

**TC-021-096**

- 测试名：`tc021_096_the_platform_layer_is_used_when_the_tenant_has_not_configured_it`
- 断言原文：

```rust
    assert_eq!(status, StatusCode::OK, "body = {body}");

    // 语言：平台层的 `en-US` 生效，来源如实报 `platform`。
    assert_eq!(body["locale"], "en-US", "平台级的语言必须生效");
    assert_eq!(body["source"], "platform", "如实报出是平台层生效");
    assert_eq!(body["sources"]["locale"], "platform");
    assert_ne!(body["locale"], "zh-CN", "不能退回 manifest 默认值");

    // 时区：平台层的 `America/New_York` 生效。
    assert_eq!(body["timezone"], "America/New_York", "平台级的时区必须生效");
    assert_eq!(body["sources"]["timezone"], "platform");
    assert_ne!(body["timezone"], "Asia/Shanghai", "不能退回 manifest 默认值");

    // 币种：平台层的 `USD` 生效。
    assert_eq!(body["currency"], "USD", "平台级的币种必须生效");
    assert_eq!(body["sources"]["currency"], "platform");
    assert_ne!(body["currency"], "CNY", "不能退回 manifest 默认值");
```

**TC-021-097**

- 测试名：`tc021_097_the_tenant_layer_wins_over_the_platform_layer`
- 断言原文：

```rust
    assert_eq!(status, StatusCode::OK, "body = {body}");

    assert_eq!(body["locale"], "ja-JP", "租户层的语言必须压过平台层");
    assert_eq!(body["source"], "tenant", "如实报出是租户层生效");
    assert_eq!(body["sources"]["locale"], "tenant");
    assert_ne!(body["locale"], "en-US", "不能取平台层的值");

    assert_eq!(body["timezone"], "Europe/London", "租户层的时区必须压过平台层");
    assert_eq!(body["sources"]["timezone"], "tenant");
    assert_ne!(body["timezone"], "America/New_York", "不能取平台层的值");

    assert_eq!(body["currency"], "JPY", "租户层的币种必须压过平台层");
    assert_eq!(body["sources"]["currency"], "tenant");
    assert_ne!(body["currency"], "USD", "不能取平台层的值");
```

### 4.3 真实库层（`crates/platform-server/tests/i18n_settings_real_db.rs`）

- 测试名：`the_platform_layer_takes_part_in_the_negotiation_and_the_tenant_wins_over_it`
- 断言原文（TC-021-096 段 + TC-021-097 段）：

```rust
    // **TC-021-096**：租户没配 → 读到**平台层**的值，来源 `platform`。
    let settings = I18nSettingsSource::settings(&service, &ctx())
        .await
        .expect("读 i18n 设置应成功");
    for (key, expected) in [
        ("locale", "en-US"),
        ("timezone", "America/New_York"),
        ("currency", "USD"),
    ] {
        let setting = value_of(&settings, key);
        assert_eq!(setting.value, expected, "{key} 应当取平台层的值");
        assert_eq!(
            setting.source,
            ConfigSource::Platform,
            "{key} 的来源必须是 platform（D119）—— 报成 default 就是 M1 没修"
        );
        assert!(setting.is_explicit(), "{key} 来自平台层 = 显式配置");
    }

    // **TC-021-097**：租户再写 → **租户赢**，来源 `tenant`。
    ...
    for (key, expected) in [
        ("locale", "zh-CN"),
        ("timezone", "Europe/London"),
        ("currency", "JPY"),
    ] {
        let setting = value_of(&settings, key);
        assert_eq!(setting.value, expected, "{key} 应当取租户层的值");
        assert_eq!(
            setting.source,
            ConfigSource::Tenant,
            "{key} 的来源必须是 tenant —— 租户层压过平台层"
        );
    }
```

---

## 5. 哪些旧用例的期望值被改了、为什么

### 5.1 `currency_has_only_tenant_and_default_tiers` → `currency_has_tenant_platform_and_default_tiers`

- **改的是**：**测试名** + **新增一条平台层用例**。
- **没改的**：原有两条断言（租户 → `Tenant`、默认 → `Default`）**一字未动**。
- **原因（与 D119 冲突，不是实现不对）**：原名断言的是「币种只有两级」这个**模型**，而 D119 把链扩为五级 → 币种变成「租户 > 平台 > 默认」三级。原名继续留着会与实现直接矛盾（注释与事实不一致）。因此**改期望（名字 + 补用例）**，而不是把实现退回两级。

### 5.2 `is_explicit()` 的语义被拓宽（覆盖 `Platform`）

- **旧语义**：`matches!(self.source, ConfigSource::Tenant | ConfigSource::Space)`。
- **新语义**：`self.layer_source().is_some()`（含 `Platform`）。
- **现有旧用例的断言一条都没改**，因为它们的场景**平台层确实没配**：
  - `i18n_settings_real_db.rs::defaults_come_from_the_manifest_not_from_code_constants`：
    `assert_eq!(setting.source, ConfigSource::Default)` + `assert!(!setting.is_explicit())` → 默认层仍不是显式配置 ✓
  - `i18n_settings_real_db.rs::tenant_values_override_and_keep_the_manifest_default_visible`：
    `assert!(timezone.is_explicit())`（租户层）✓ / `assert!(!locale.is_explicit())`（默认层）✓
- **原因（与 D119 冲突）**：D119 裁定「平台层就是组织级默认值」，平台层的值**不能**被当成「没配」。语义不变则 M1 依旧。

### 5.3 `i18n_settings_real_db.rs::setup()` 夹具增补（**不是**期望值改动）

- **改的是**：夹具 —— 每条用例开始前**删掉平台级的 locale/timezone/currency 行**并推进平台 epoch。
- **为什么必须改**：D119 之后，**平台层的值会参与协商**。原来这条用例断言「没配 → manifest 默认值 / `source=default`」，它成立的前提是「**平台层确实没配**」——而这个前提原先**靠数据库恰好干净**。
  - **实测**：本轮开工时 `base_test` 的 `platform_config` 里**确实有** QA 复现 M1 时写的 `locale="en-US"`（`updated_at = 2026-11-19 13:24:07+00`）。只要该行在库中，这条旧用例读到的就是 `en-US` / `Platform`，断言 `locale == "zh-CN"` / `source == Default` 会失败（**D119 之前也会失败**，因为值断言与来源断言都不满足）。
  - 现在夹具**自己保证**「平台层确实没配」，符合任务要求「若那个场景平台层确实没配，仍然是 `default`」。
- **同时**：`setup()` 返回的池由「应用角色池」改为「**管理员池**」。原因：应用角色对 `platform_config` **只有 SELECT**（`deploy/seed/02-app-role.sql` 显式 REVOKE 了 INSERT/UPDATE/DELETE），用应用池做夹具清理实测报 `42501 permission denied for table platform_config`。

### 5.4 测试辅助函数改名 `tenant_setting` → `setting_with_source`

- **纯改名**（同一文件内 10 处调用点），语义不变。该构造函数现在也用来造**平台层**的值，原名会让人以为只能造租户层的值。

### 5.5 注释 / 文档随链变化更新

- 模块文档「优先级」四级 → 五级；时区 `source` 集合补 `platform`；`LocaleLayers::tenant` 字段说明；`FieldSources` / `ContextResponse` 的 `source` 说明；`i18n_settings_real_db.rs` 的「协商的第 4 级」→「第 5 级」。
- **原因**：注释里的行为必须与代码一致；旧注释会与 D119 直接矛盾。

### 5.6 **没有**被改动的旧期望值（明确记录）

以下旧用例的**断言原文全部保留**，且全部通过（见 §6 的 passed/failed 数字）：
`ac090_step1..step8`、`a_zero_quality_language_is_skipped`、`the_result_is_always_a_registered_locale`、
`an_unregistered_tenant_locale_falls_back_to_the_platform_default`、`timezone_negotiation_never_uses_accept_language`、
`an_invalid_user_timezone_is_ignored`、`accept_language_*`、`rfc3339_shape_check_accepts_the_contract_shapes`、
`error_codes_have_registered_ascii_titles`、
以及 HTTP 层的 `context_requires_authentication`、`context_reports_the_user_tier_when_it_contradicts_the_header`、
`context_ignores_an_unregistered_user_locale`、`context_prefers_the_header_for_locale_but_never_for_timezone`、
`context_falls_back_to_the_tenant_when_the_header_is_unregistered`、`a_cross_tenant_header_is_not_found`、
`the_context_response_shape_is_pinned`、`the_convert_response_shape_is_pinned` 及全部 `convert_*` 用例。

---

## 6. 测试命令 + passed/failed（原文照抄）

### 6.1 环境

```bash
export CARGO_TARGET_DIR=/home/myg133/Projects/system_seed/code/target
export XDG_RUNTIME_DIR=/run/user/$(id -u)
export DATABASE_URL_TEST="postgres://base:base@127.0.0.1:5432/base_test"
cd /home/myg133/Projects/system_seed/feature-REQ-021
```

### 6.2 逐 crate（最终一次全量回归）

命令（每个 crate 一条，`--` 只有一个）：

```bash
for c in platform-core platform-config platform-iam platform-rbac platform-org \
         platform-db platform-schema platform-server sandbox; do
  cargo test -p $c --all-targets -- --test-threads=2 --include-ignored
done
```

输出（每个 crate 的 `test result:` 行按测试目标汇总）：

```
CRATE platform-core    exit=0  passed=127 failed=0 ignored=0
CRATE platform-config  exit=0  passed=83  failed=0 ignored=0
CRATE platform-iam     exit=0  passed=58  failed=0 ignored=0
CRATE platform-rbac    exit=0  passed=78  failed=0 ignored=0
CRATE platform-org     exit=0  passed=91  failed=0 ignored=0
CRATE platform-db      exit=0  passed=46  failed=0 ignored=0
CRATE platform-schema  exit=0  passed=20  failed=0 ignored=0
CRATE platform-server  exit=0  passed=101 failed=0 ignored=0
CRATE sandbox          exit=0  passed=33  failed=0 ignored=0
```

**合计：637 passed / 0 failed / 0 ignored**（9 个 crate 全部 `exit=0`）。
637 = 630（旧）+ 7（本次新增：单元 3 + HTTP 3 + 真实库 1）→ **旧用例一条没挂**。

`platform-server` 的分目标原文：

```
     Running unittests src/lib.rs
test result: ok. 61 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
     Running unittests src/main.rs
test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
     Running tests/i18n_api.rs
test result: ok. 17 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
     Running tests/i18n_no_chinese_titles.rs
test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.14s
     Running tests/i18n_settings_real_db.rs
test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.90s
     Running tests/i18n_titles.rs
test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

跨 crate 顺序复查（`platform-server` 之后再跑 `platform-config`，防止平台层行污染别的 crate）：

```
===== 跨 crate 顺序复查：platform-server 之后再跑 platform-config =====
exit=0
platform-config(after) passed=83 failed=0 ignored=0
```

### 6.3 假绿自查：把实现退回 D119 之前，新用例必须红

把 `layer_source()` 的 `ConfigSource::Platform` 分支临时改成 `None`（= 修复前的行为）后重跑：

```
test i18n::tests::currency_has_tenant_platform_and_default_tiers ... FAILED
test i18n::tests::d119_each_config_layer_maps_to_its_own_source ... FAILED
test i18n::tests::tc021_096_the_platform_layer_is_used_when_the_tenant_has_not_configured_it ... FAILED
test result: FAILED. 58 passed; 3 failed; 0 ignored; 0 measured; 0 filtered out

thread 'i18n::tests::tc021_096_the_platform_layer_is_used_when_the_tenant_has_not_configured_it' panicked at crates/platform-server/src/i18n.rs:1433:9:
assertion `left == right` failed: 平台级的语言必须生效
  left: "zh-CN"
 right: "en-US"

test tc021_096_the_platform_layer_is_used_when_the_tenant_has_not_configured_it ... FAILED
test the_platform_layer_still_loses_to_the_accept_language_header ... FAILED
test result: FAILED. 15 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out

thread 'the_platform_layer_takes_part_in_the_negotiation_and_the_tenant_wins_over_it' panicked at crates/platform-server/tests/i18n_settings_real_db.rs:292:9:
locale 来自平台层 = 显式配置
test result: FAILED. 2 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out
```

随后**已恢复** D119 实现并重跑（§6.2 的 637 passed 即恢复后的结果）。
注：`tc021_097` 在退回后**仍然绿** —— 这是**预期**的：TC-021-097 守的是「租户层压过平台层」，修复前租户层本来就赢；它防的是「平台层反过来压过租户层」这种改坏。

### 6.4 库状态

- 本轮所有真实库用例只碰 `base_test`，**未写 `base_dev`**。
- 真实库用例结束后 `platform_config` 里**不留** locale/timezone/currency 行（收尾清理已验证）：

```
===== DB AFTER =====
（空 —— 无输出）
```

---

## 7. 未做 / 做不到 / 有疑虑的项

| # | 项 | 状态 | 原因 / 证据 |
|---|----|------|------------|
| 1 | 前端（`packages/**`、`apps/**`、`e2e/**`）适配 | **未做（按任务要求）** | 任务明确「前端本轮不动」。**风险很低**：`apps/shell/src/lib/i18n.ts:71` 的 `I18nContextPayload.source` 类型是 `readonly source?: unknown`（**不是**闭合联合类型），且前端**没有**按 `source` 值分支（只在一句注释里提到 `source === "user"`）→ 新增 `"platform"` 不会让前端类型报错或行为分叉 |
| 2 | 「租户配了但值非法」时回落到平台层 | **未做，且是 BA 裁定的范围外** | BA 裁定原文：「平台层只是**在租户没配时**才生效」。当前 `Setting` 只携带**已解析的那一层**（单一 value + source），要实现「租户值非法 → 回落平台值」需要改 `Setting` 的形状（同时携带租户层与平台层）= 超出「只改一件事」的范围。现状：租户值未注册 → 落 manifest 默认值（`source=default`），与修复前一致 |
| 3 | 空间层 → `source` 的取值 | **报 `tenant`**（不是 `platform`） | D119 明确要求的 5 值里**没有** `space`；空间层比租户层更深，对外报 `tenant`。`LocaleSource` 没有 `space` 变体（加了就是第 6 值，违反 D119） |
| 4 | `crates/platform-config/tests/i18n_config_real_db.rs` 未改 | **未改** | 该文件的断言只针对**配置层**（`ConfigSource`），与协商层无关；D119 不改变配置层的解析结果。§6.2 的 83 passed 即证据 |
| 5 | `source` 的前端文案 / 文档 | **未做** | 前端本轮不动；若后续要给 `platform` 写展示文案，需要前端 Dev 单独一轮 |
| 6 | 迁移文件 | **未动** | `20261119000200_i18n_fx_rate.*` 及已应用迁移**一字未改** → checksum 不变，`sqlx::migrate` 不会拒绝启动 |
| 7 | 提交 | **未提交** | 按 BA 要求由 BA 统一提交。本轮只用了 `git status` / `git diff` / `git log`（只读） |
| 8 | `Setting::is_explicit()` 的**外部调用方** | **已全仓核查** | 全仓 `is_explicit` 的引用只出现在两个文件里：`crates/platform-server/src/i18n.rs`（定义）与 `crates/platform-server/tests/i18n_settings_real_db.rs`（5 个断言点：L188 / L230 / L238 / L243 / L299）—— 均已随本次改动更新或保持通过 |
| 9 | 平台层 i18n 行的**清理权限** | **已解决（用管理员池）** | 应用角色对 `platform_config` 只有 SELECT（实测 `42501 permission denied for table platform_config`）；夹具改用管理员池，业务访问仍走应用角色的 `TenantDb` |

---

## 8. 交付自检（对照任务的成功标准）

| 成功标准 | 证据 |
|---|---|
| 平台级配置**生效**且 `source == "platform"` | §4.1 单元 / §4.2 HTTP / §4.3 真实库 三层各一条用例；§6.3 负向验证（退回实现即红） |
| 租户覆盖仍然优先（`source == "tenant"`） | §4.3 真实库：平台层 `en-US` 在先，租户层 `zh-CN` 在后 → 读到 `zh-CN` / `Tenant` |
| 语言 / 时区 / 币种**三个字段**都修好 | 三个协商函数全部改（§3.1–§3.3）；每条 D119 用例都断言三个字段的**值**与**来源** |
| 旧的 630 条测试不挂 | §6.2：9 个 crate 全绿，637 = 630 + 7；被改的旧期望值只有「测试名」一项（§5.1），断言原文全部保留 |
| 新增测试覆盖 TC-021-096 / TC-021-097 | 单元 2 + HTTP 2 + 真实库 1（真实库那条同时覆盖 096 与 097 两段） |
| 报告无「应该可以」「大概通过」 | 全部结论附命令输出（§6.2 / §6.3）或具体文件行号 |
