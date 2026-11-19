//! i18n 三个配置键的**真实库**行为（REQ-021 / AC-S-093 / D111）。
//!
//! 验四件事：
//!
//! 1. 三个键都登记为 `scope: tenant`，写入后**租户隔离**生效
//!    （租户 A 读到自己的值，租户 B 读到平台默认）；
//! 2. `format` 校验在**写入时**生效：非法时区 / 非法币种 / 未注册语言 → **400**，
//!    错误码 `CONFIG_VALUE_FORMAT_INVALID`（D111），**不是**存进去以后在渲染时炸；
//! 3. 被拒的写**不留痕**（重新读出来还是旧值 —— D84 的快照判据）；
//! 4. 平台默认值来自 manifest 的 `default`（裁定 #17），不是代码常量。
//!
//! # 与前端产物的耦合（如实记录）
//!
//! 「已注册语言」的来源是 `packages/i18n/src/locales/*.json` 的**文件名**（裁定 #16）。
//! 那是前端 Dev 的产物 —— 所以本用例**不硬编码** `en-US`：
//! 它取注册表里第一个 `!= zh-CN` 的语言（没有就退回 `zh-CN`），
//! 并单独断言「形状合法但未注册」的 `xx-YY` 一定被拒。

mod common;

use common::*;
use platform_config::error::{CODE_VALUE_FORMAT_INVALID, ConfigError};
use platform_config::service::{WriteEntry, WriteRequest, WriteRoute};
use platform_core::extension::config::ConfigSource;
use serde_json::{Value, json};

/// 写一条租户级配置。
async fn write_tenant(
    fx: &Fixture,
    ctx: &platform_core::RequestContext,
    key: &str,
    value: Value,
) -> Result<usize, ConfigError> {
    fx.service
        .write_entries(
            ctx,
            &WriteRequest {
                scope: "tenant".to_string(),
                entries: vec![WriteEntry {
                    key: key.to_string(),
                    value,
                }],
            },
            WriteRoute::Scoped,
        )
        .await
}

/// 读三个键的值与来源。
async fn read_i18n(
    fx: &Fixture,
    ctx: &platform_core::RequestContext,
) -> Vec<(String, Value, ConfigSource)> {
    let keys = vec![
        "locale".to_string(),
        "timezone".to_string(),
        "currency".to_string(),
    ];
    fx.service
        .read_entries(ctx, &keys)
        .await
        .expect("读 i18n 配置应成功")
        .into_iter()
        .map(|entry| (entry.key, entry.value, entry.source))
        .collect()
}

fn value_of(entries: &[(String, Value, ConfigSource)], key: &str) -> (Value, ConfigSource) {
    entries
        .iter()
        .find(|(name, _, _)| name == key)
        .map(|(_, value, source)| (value.clone(), *source))
        .unwrap_or_else(|| panic!("缺少配置项 {key}"))
}

/// 注册表里第一个非基准语言（没有就用 `zh-CN` 本身）。
fn alternative_locale(fx: &Fixture) -> String {
    fx.registry
        .locales()
        .iter()
        .find(|code| code.as_str() != "zh-CN")
        .cloned()
        .unwrap_or_else(|| "zh-CN".to_string())
}

/// 三个键的租户隔离 + 平台默认值来源。
#[tokio::test]
async fn the_three_i18n_keys_are_tenant_scoped_with_manifest_defaults() {
    let _guard = serial().await;
    let fx = setup().await;

    // 前提：三个键都登记为 tenant（未登记的话写入会 400 CONFIG_KEY_NOT_REGISTERED）。
    for key in ["locale", "timezone", "currency"] {
        let registered = fx
            .registry
            .get(key)
            .unwrap_or_else(|| panic!("未登记：{key}"));
        assert_eq!(registered.scope, platform_core::config::ConfigScope::Tenant);
        assert_eq!(
            registered.default,
            match key {
                "locale" => json!("zh-CN"),
                "timezone" => json!("Asia/Shanghai"),
                _ => json!("CNY"),
            },
            "{key} 的默认值必须来自 manifest（裁定 #17）"
        );
    }

    let locale = alternative_locale(&fx);
    for (key, value) in [
        ("locale", json!(locale.clone())),
        ("timezone", json!("America/New_York")),
        ("currency", json!("USD")),
    ] {
        write_tenant(&fx, &ctx(TENANT_A), key, value)
            .await
            .unwrap_or_else(|error| panic!("写 {key} 应成功：{error}"));
    }

    // 租户 A：读到自己的值，来源是 tenant。
    let a = read_i18n(&fx, &ctx(TENANT_A)).await;
    let (a_locale, a_locale_source) = value_of(&a, "locale");
    assert_eq!(a_locale, json!(locale));
    assert_eq!(a_locale_source, ConfigSource::Tenant);
    assert_eq!(
        value_of(&a, "timezone"),
        (json!("America/New_York"), ConfigSource::Tenant)
    );
    assert_eq!(
        value_of(&a, "currency"),
        (json!("USD"), ConfigSource::Tenant)
    );

    println!(
        "tenant-a: locale={:?}({}) timezone={:?}({}) currency={:?}({})",
        a_locale,
        a_locale_source.as_str(),
        value_of(&a, "timezone").0,
        value_of(&a, "timezone").1.as_str(),
        value_of(&a, "currency").0,
        value_of(&a, "currency").1.as_str()
    );

    // 租户 B：谁都没配 → manifest 默认值，来源 default。
    let b = read_i18n(&fx, &ctx(TENANT_B)).await;
    println!(
        "tenant-b: locale={:?}({}) timezone={:?}({}) currency={:?}({})",
        value_of(&b, "locale").0,
        value_of(&b, "locale").1.as_str(),
        value_of(&b, "timezone").0,
        value_of(&b, "timezone").1.as_str(),
        value_of(&b, "currency").0,
        value_of(&b, "currency").1.as_str()
    );
    assert_eq!(
        value_of(&b, "locale"),
        (json!("zh-CN"), ConfigSource::Default)
    );
    assert_eq!(
        value_of(&b, "timezone"),
        (json!("Asia/Shanghai"), ConfigSource::Default)
    );
    assert_eq!(
        value_of(&b, "currency"),
        (json!("CNY"), ConfigSource::Default)
    );
}

/// 非法值在**写入时**被拒（400 + 明确的 code），且不留痕。
#[tokio::test]
async fn invalid_format_values_are_rejected_on_write_and_leave_no_trace() {
    let _guard = serial().await;
    let fx = setup().await;

    // 先写入一组合法值，作为「被拒的写没有覆盖它」的基准。
    for (key, value) in [
        ("locale", json!("zh-CN")),
        ("timezone", json!("Asia/Shanghai")),
        ("currency", json!("CNY")),
    ] {
        write_tenant(&fx, &ctx(TENANT_A), key, value)
            .await
            .expect("合法值应写入");
    }
    let before = read_i18n(&fx, &ctx(TENANT_A)).await;

    let cases = [
        ("timezone", json!("Not/AZone"), "iana-timezone"),
        ("currency", json!("XXXX"), "iso4217"),
        ("currency", json!("XXX"), "iso4217"),
        // 形状合法但**未注册**的语言（`xx-YY` 不会有人建翻译包）。
        ("locale", json!("xx-YY"), "locale"),
    ];
    for (key, value, format) in cases {
        let error = write_tenant(&fx, &ctx(TENANT_A), key, value.clone())
            .await
            .expect_err("非法值必须被拒绝（不能 200 落库）");
        assert_eq!(
            error.code(),
            CODE_VALUE_FORMAT_INVALID,
            "{key}={value} 应当返回 CONFIG_VALUE_FORMAT_INVALID，实际：{error}"
        );
        assert_eq!(error.status(), 400, "非法值是请求问题，不是服务端错");
        let params = error.params();
        assert_eq!(params.get("key"), Some(&json!(key)));
        assert_eq!(
            params.get("format"),
            Some(&json!(format)),
            "错误参数必须点名是哪种格式没通过"
        );
        assert_eq!(params.get("value"), Some(&value));
        assert!(error.detail().is_ascii(), "detail 必须是英文 ASCII");
        println!(
            "rejected {key}={value} → {} ({}) detail={}",
            error.code(),
            error.status(),
            error.detail()
        );
    }

    // 被拒的写不留痕：重新读出来与写入前一致（D84 的快照判据）。
    let after = read_i18n(&fx, &ctx(TENANT_A)).await;
    assert_eq!(before, after, "被拒绝的写入不得改动任何值");
    for key in ["timezone", "currency", "locale"] {
        let (value, source) = value_of(&after, key);
        assert_eq!(source, ConfigSource::Tenant, "{key} 仍是显式配置");
        assert!(!value.as_str().unwrap_or_default().is_empty());
    }
}

/// 每个**已注册**语言都能写入；未注册的同形状语言被拒。
///
/// 这一条把「已注册语言」的判据钉在行为上：注册表说有哪些，写入就必须认哪些
/// —— 两处不一致（例如硬编码了 `en-US`）会在这里红。
#[tokio::test]
async fn every_registered_locale_is_writable_and_nothing_else_is() {
    let _guard = serial().await;
    let fx = setup().await;

    let registered = fx.registry.locales().clone();
    assert!(
        registered.contains("zh-CN"),
        "基准语言 zh-CN 必须始终已注册（回退链的最后一环）"
    );

    for locale in &registered {
        write_tenant(&fx, &ctx(TENANT_A), "locale", json!(locale))
            .await
            .unwrap_or_else(|error| panic!("已注册语言 `{locale}` 必须可写：{error}"));
        let entries = read_i18n(&fx, &ctx(TENANT_A)).await;
        assert_eq!(value_of(&entries, "locale").0, json!(locale));
    }

    let error = write_tenant(&fx, &ctx(TENANT_A), "locale", json!("xx-YY"))
        .await
        .expect_err("未注册语言必须被拒");
    assert_eq!(error.code(), CODE_VALUE_FORMAT_INVALID);
    assert!(
        error.detail().contains("not registered"),
        "错误原因必须点明「未注册」：{}",
        error.detail()
    );
}
