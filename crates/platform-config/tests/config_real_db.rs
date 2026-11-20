//! REQ-010 真实库验收用例（`base_test`，应用角色 `base_app`）。
//!
//! 覆盖：AC-S-049 ~ 056、AC-S-057 ~ 059（单元/服务层）、AC-S-060（服务层）、
//! AC-S-062、AC-S-063（服务层）、D55 / D56 / D66 / D70 / D71 / D72。

mod common;

use std::collections::BTreeSet;
use std::io::Write;
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::*;
use platform_config::error::{
    CODE_FLAG_NAME_INVALID, CODE_FLAG_USE_FLAG_ENDPOINT, CODE_KEY_INVALID, CODE_KEY_NOT_REGISTERED,
    CODE_SCOPE_NOT_ALLOWED, CODE_TENANT_RESERVED, CODE_TYPE_MISMATCH, CODE_VALUE_OUT_OF_RANGE,
};
use platform_config::{
    ConfigError, DbConfigResolver, FlagWriteRequest, WriteEntry, WriteRequest, WriteRoute,
};
use platform_core::config::ConfigKey;
use platform_core::extension::config::{ConfigResolver, ConfigSource};
use platform_db::unscoped_row_count;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

// ---------------------------------------------------------------------------
// 小工具
// ---------------------------------------------------------------------------

fn key_list(keys: &[&str]) -> Vec<String> {
    keys.iter().map(|key| (*key).to_string()).collect()
}

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

async fn write_space(
    fx: &Fixture,
    ctx: &platform_core::RequestContext,
    key: &str,
    value: Value,
) -> Result<usize, ConfigError> {
    fx.service
        .write_entries(
            ctx,
            &WriteRequest {
                scope: "space".to_string(),
                entries: vec![WriteEntry {
                    key: key.to_string(),
                    value,
                }],
            },
            WriteRoute::Scoped,
        )
        .await
}

async fn write_platform(fx: &Fixture, key: &str, value: Value) -> Result<usize, ConfigError> {
    fx.service
        .write_entries(
            &ctx(TENANT_A),
            &WriteRequest {
                scope: "platform".to_string(),
                entries: vec![WriteEntry {
                    key: key.to_string(),
                    value,
                }],
            },
            WriteRoute::Platform,
        )
        .await
}

async fn resolve(
    fx: &Fixture,
    ctx: &platform_core::RequestContext,
    key: &str,
) -> Option<platform_core::ResolvedConfig> {
    let resolver = fx.service.resolver();
    resolver.refresh(ctx).await.expect("刷新应成功");
    let config_key = ConfigKey::parse(key).expect("测试用的键必须合法");
    resolver.resolve(ctx, &config_key)
}

async fn set_beta(fx: &Fixture, enabled: Option<bool>, percent: Option<i64>) -> bool {
    let decision = fx
        .service
        .write_flag(
            &ctx_user(TENANT_A, USER_M),
            "beta",
            &FlagWriteRequest {
                enabled: enabled.map(Value::from),
                rollout_percent: percent.map(Value::from),
            },
        )
        .await
        .expect("写 beta 应成功");
    decision.enabled
}

/// 用**独立的解析器实例**对 200 个用户判定，返回命中集合。
async fn hit_set(fx: &Fixture, percent: i64, users: &[String]) -> BTreeSet<String> {
    // p 由调用方先写入；这里重新建实例 = 冷缓存 + 共享 DB。
    set_beta(fx, None, Some(percent)).await;
    let resolver = fx.new_resolver();
    let base_ctx = ctx_user(TENANT_A, USER_M);
    resolver.refresh(&base_ctx).await.expect("刷新应成功");
    users
        .iter()
        .filter(|user| resolver.flag(&ctx_user(TENANT_A, user), "beta").enabled)
        .cloned()
        .collect()
}

fn rollout_users() -> Vec<String> {
    (3001..=3200).map(|index| format!("u-{index}")).collect()
}

/// 独立复算灰度公式（**不复用被测实现**）。
fn expected_hit(flag: &str, user: &str, percent: i64) -> bool {
    let mut hasher = Sha256::new();
    hasher.update(format!("{flag}:{user}").as_bytes());
    let digest = hasher.finalize();
    let bucket = u32::from_be_bytes([digest[0], digest[1], digest[2], digest[3]]);
    (bucket % 100) < u32::try_from(percent).expect("比例应为 u32")
}

// ---------------------------------------------------------------------------
// AC-S-049 / AC-S-050 / AC-S-051：三层覆盖、作用域、类型
// ---------------------------------------------------------------------------

#[tokio::test]
async fn three_layer_resolution_reports_the_hit_layer() {
    let _guard = serial().await;
    let fx = setup().await;
    let resolver = fx.service.resolver();
    let logo = ConfigKey::parse("logo").unwrap();

    // 1) 平台上下文（无租户）—— HTTP 层不可达，单元层验证。
    let none = platform_core::RequestContext::new();
    resolver.refresh(&none).await.unwrap();
    let hit = resolver.resolve(&none, &logo).expect("应有平台值");
    assert_eq!(hit.value, json!("/branding/platform-a.svg"));
    assert_eq!(hit.source, ConfigSource::Platform);

    // 2) T1 → 租户级。
    let a = ctx(TENANT_A);
    resolver.refresh(&a).await.unwrap();
    let hit = resolver.resolve(&a, &logo).expect("应有租户值");
    assert_eq!(hit.value, json!("/branding/tenant-a.svg"));
    assert_eq!(hit.source, ConfigSource::Tenant);

    // 3) T1+S1 → 空间级。
    let a_s1 = ctx_space(TENANT_A, SPACE_S1);
    resolver.refresh(&a_s1).await.unwrap();
    let hit = resolver.resolve(&a_s1, &logo).expect("应有空间值");
    assert_eq!(hit.value, json!("/branding/space-s1.svg"));
    assert_eq!(hit.source, ConfigSource::Space);

    // 4) T2+S1 → 同一 space_id 字符串，另一租户的另一行。
    let b_s1 = ctx_space(TENANT_B, SPACE_S1);
    resolver.refresh(&b_s1).await.unwrap();
    let hit = resolver.resolve(&b_s1, &logo).expect("应有 T2 的空间值");
    assert_eq!(hit.value, json!("/branding/space-t2-s1.svg"));
    assert_eq!(hit.source, ConfigSource::Space);

    // 5) T2 无空间头 → T2 的租户值（不能回落到平台值）。
    let b = ctx(TENANT_B);
    resolver.refresh(&b).await.unwrap();
    let hit = resolver.resolve(&b, &logo).expect("应有 T2 租户值");
    assert_eq!(hit.value, json!("/branding/tenant-b.svg"));
    assert_eq!(hit.source, ConfigSource::Tenant);

    // 6) T1+S2 → 未被 S1 波及（局部性）。
    let a_s2 = ctx_space(TENANT_A, SPACE_S2);
    resolver.refresh(&a_s2).await.unwrap();
    let hit = resolver.resolve(&a_s2, &logo).expect("应有 S2 的值");
    assert_eq!(hit.value, json!("/branding/space-s2.svg"));
    assert_eq!(hit.source, ConfigSource::Space);
}

#[tokio::test]
async fn source_default_is_distinct_from_an_explicit_platform_value() {
    let _guard = serial().await;
    let fx = setup().await;

    // title：三层都没配（平台没写、T2 没写）→ default。
    let title = resolve(&fx, &ctx(TENANT_B), "title")
        .await
        .expect("有默认值");
    assert_eq!(title.value, json!("示例平台"));
    assert_eq!(title.source, ConfigSource::Default);

    // primaryColor：平台层**显式**写了与默认值相同的值 → source 必须是 platform。
    // 若实现用「值比对」判来源，这里会退化成 default。
    let primary = resolve(&fx, &ctx(TENANT_B), "primaryColor")
        .await
        .expect("有平台值");
    assert_eq!(primary.value, json!("#111111"));
    assert_eq!(primary.source, ConfigSource::Platform);

    // 未登记键：解析器返回 None，HTTP 层是 404。
    let none = resolve(&fx, &ctx(TENANT_A), "notRegistered").await;
    assert!(none.is_none());

    // 批量读含未登记键 → 整请求 404（不返回 logo 的部分结果）。
    let error = fx
        .service
        .read_entries(&ctx(TENANT_A), &key_list(&["logo", "notRegistered"]))
        .await
        .expect_err("含未登记键应整请求失败");
    assert_eq!(error.code(), CODE_KEY_NOT_REGISTERED);
    assert_eq!(error.status(), 404);
    assert_eq!(
        error.params().get("unknownKeys"),
        Some(&json!(["notRegistered"]))
    );

    // 重复键去重。
    let entries = fx
        .service
        .read_entries(&ctx(TENANT_A), &key_list(&["logo", "logo"]))
        .await
        .expect("重复键应去重");
    assert_eq!(entries.len(), 1);
}

#[tokio::test]
async fn invalid_key_names_are_rejected_with_the_exact_code() {
    let _guard = serial().await;
    let fx = setup().await;
    for bad in ["Theme.radius", "theme-radius"] {
        let error = fx
            .service
            .read_entries(&ctx(TENANT_A), &[bad.to_string()])
            .await
            .expect_err("非法键名应被拒");
        assert_eq!(error.code(), CODE_KEY_INVALID);
        assert_eq!(error.status(), 400);
    }
}

#[tokio::test]
async fn scope_escalation_is_rejected_and_leaves_no_rows() {
    let _guard = serial().await;
    let fx = setup().await;

    // 「写前快照」—— 必须在**任何**越级写之前取。
    //
    // ⚠️ 这里曾经取在步骤 1/2 **之后**（QA 复核折出）：那样前两次越级写若真的泄漏，
    // 污染已经进了「写前」快照 → 快照必然「一致」→ **漏检**。
    let before_snapshot = conflict_strategy_snapshot(&fx.admin_pool).await;

    // 1) platform 作用域的键，租户级写 → 400。
    let error = write_tenant(
        &fx,
        &ctx(TENANT_A),
        "conflictStrategy",
        json!("remote_wins"),
    )
    .await
    .expect_err("租户级越级写应被拒");
    assert_eq!(error.code(), CODE_SCOPE_NOT_ALLOWED);
    assert_eq!(error.status(), 400);
    assert_eq!(error.params().get("allowedScope"), Some(&json!("platform")));
    assert_eq!(error.params().get("attemptedScope"), Some(&json!("tenant")));

    // 2) 空间级写同一键 → 400。
    let error = write_space(
        &fx,
        &ctx_space(TENANT_A, SPACE_S1),
        "conflictStrategy",
        json!("remote_wins"),
    )
    .await
    .expect_err("空间级越级写应被拒");
    assert_eq!(error.code(), CODE_SCOPE_NOT_ALLOWED);

    // 3) tenant 作用域的键：租户级成功。
    write_tenant(&fx, &ctx(TENANT_A), "pageSize", json!(50))
        .await
        .expect("租户级写 pageSize 应成功");

    // 4) tenant 作用域的键：空间级 → 400。
    let error = write_space(&fx, &ctx_space(TENANT_A, SPACE_S1), "pageSize", json!(50))
        .await
        .expect_err("空间级越级写应被拒");
    assert_eq!(error.code(), CODE_SCOPE_NOT_ALLOWED);
    assert_eq!(error.params().get("allowedScope"), Some(&json!("tenant")));

    // 5) space 作用域的键：租户级 / 空间级都可写。
    write_tenant(
        &fx,
        &ctx(TENANT_A),
        "logo",
        json!("/branding/tenant-a-new.svg"),
    )
    .await
    .expect("租户级写 logo 应成功");
    write_space(
        &fx,
        &ctx_space(TENANT_A, SPACE_S1),
        "logo",
        json!("/branding/space-s1-new.svg"),
    )
    .await
    .expect("空间级写 logo 应成功");

    // 读回（三层）。
    let a = resolve(&fx, &ctx(TENANT_A), "logo").await.unwrap();
    assert_eq!(a.value, json!("/branding/tenant-a-new.svg"));
    assert_eq!(a.source, ConfigSource::Tenant);
    let a_s1 = resolve(&fx, &ctx_space(TENANT_A, SPACE_S1), "logo")
        .await
        .unwrap();
    assert_eq!(a_s1.value, json!("/branding/space-s1-new.svg"));
    assert_eq!(a_s1.source, ConfigSource::Space);

    // 7) 平台级正对照：独立路由可写。
    write_platform(&fx, "conflictStrategy", json!("local_wins"))
        .await
        .expect("平台级写应成功");
    let platform = resolve(&fx, &ctx(TENANT_B), "conflictStrategy")
        .await
        .unwrap();
    assert_eq!(platform.value, json!("local_wins"));
    assert_eq!(platform.source, ConfigSource::Platform);

    // 8) 越级写**没有改动任何既有行**。
    //
    // ⚠️ 这里曾经断言「行数为 0」—— 但夹具里本来就有
    // `(tenant-a, space-sales-1, conflictStrategy='remote_wins')`，而用例尝试写的
    // 也恰好是 `remote_wins`。于是**若越级写被错误放行并覆盖这一行，行数断言抓不到**
    // （REQ-010 QA 判 LOW，D84）。
    //
    // 改成**快照比对**：对 `count` + `value` + `updated_at` 做写前/写后对比，全部必须不变。
    // 这对「多了行」「改了值」「更新了时间戳」三种泄漏都敏感。
    let after = conflict_strategy_snapshot(&fx.admin_pool).await;
    assert_eq!(
        after, before_snapshot,
        "越级写改动了既有行（快照不一致）：before={before_snapshot:?} after={after:?}"
    );
}

/// **租户级与空间级**两张表里 `conflictStrategy` 的快照：
/// `(表, tenant, space, count, max(value), max(updated_at))`。
///
/// 只看这两层，因为「越级写」的落点就是它们 —— 平台级表由步骤 7 的**正对照**故意改写，
/// 把它纳入比对会与本用例自己的正对照冲突（这正是快照型断言要处理好的边界）。
///
/// `count` 抓「多了行」，`max(value)` 抓「改了值」，**`max(updated_at)` 抓「同值覆盖」**
/// —— 最后这一项是必需的：越级写把值写成**夹具已有的同一个值**时，
/// `count` 与 `value` 都不变，只有 `updated_at` 会变。
/// （QA 复核折出：早期版本漏了 `updated_at`，注释却声称覆盖它 → 同值覆盖漏检。）
async fn conflict_strategy_snapshot(
    pool: &sqlx::PgPool,
) -> Vec<(
    String,
    String,
    Option<String>,
    i64,
    Option<String>,
    Option<String>,
)> {
    sqlx::query_as(
        "SELECT 'tenant' AS tbl, tenant_id, NULL::text AS space_id, \
                count(*)::bigint, max(value::text), max(updated_at)::text \
           FROM platform_tenant_config WHERE key = 'conflictStrategy' GROUP BY tenant_id \
         UNION ALL \
         SELECT 'space', tenant_id, space_id, count(*)::bigint, max(value::text), max(updated_at)::text \
           FROM platform_space_config WHERE key = 'conflictStrategy' GROUP BY tenant_id, space_id \
         ORDER BY 1, 2, 3",
    )
    .fetch_all(pool)
    .await
    .unwrap()
}

#[tokio::test]
async fn unknown_keys_and_type_errors_follow_the_fixed_order() {
    let _guard = serial().await;
    let fx = setup().await;

    // 未登记 → 400（写入）。
    let error = write_tenant(&fx, &ctx(TENANT_A), "notRegistered", json!(1))
        .await
        .expect_err("未登记键应被拒");
    assert_eq!(error.code(), CODE_KEY_NOT_REGISTERED);
    assert_eq!(error.status(), 400);
    assert_eq!(error.params().get("key"), Some(&json!("notRegistered")));

    // 类型：字符串形式的数字 ≠ 数字。
    let error = write_tenant(&fx, &ctx(TENANT_A), "pageSize", json!("20"))
        .await
        .expect_err("数字字符串应被拒");
    assert_eq!(error.code(), CODE_TYPE_MISMATCH);
    assert_eq!(error.params().get("expected"), Some(&json!("number")));
    assert_eq!(error.params().get("actual"), Some(&json!("string")));

    // 类型：布尔不是数字。
    let error = write_tenant(&fx, &ctx(TENANT_A), "pageSize", json!(true))
        .await
        .expect_err("布尔应被拒");
    assert_eq!(error.code(), CODE_TYPE_MISMATCH);
    assert_eq!(error.params().get("actual"), Some(&json!("boolean")));

    // 正确类型 → 成功。
    write_tenant(&fx, &ctx(TENANT_A), "pageSize", json!(20))
        .await
        .expect("数字应成功");

    // 同一批里「未登记 + 类型错」→ 注册表先于类型。
    let error = fx
        .service
        .write_entries(
            &ctx(TENANT_A),
            &WriteRequest {
                scope: "tenant".to_string(),
                entries: vec![
                    WriteEntry {
                        key: "notRegistered".to_string(),
                        value: json!(1),
                    },
                    WriteEntry {
                        key: "pageSize".to_string(),
                        value: json!("20"),
                    },
                ],
            },
            WriteRoute::Scoped,
        )
        .await
        .expect_err("应整批失败");
    assert_eq!(error.code(), CODE_KEY_NOT_REGISTERED);

    // 注册表先于作用域：space 级写未登记键 → 未登记码（不是 scope 码）。
    let error = write_space(
        &fx,
        &ctx_space(TENANT_A, SPACE_S1),
        "notRegistered",
        json!(1),
    )
    .await
    .expect_err("应整请求失败");
    assert_eq!(error.code(), CODE_KEY_NOT_REGISTERED);

    // 作用域先于类型：space 级写 tenant 作用域的 pageSize + 字符串值。
    let error = write_space(&fx, &ctx_space(TENANT_A, SPACE_S1), "pageSize", json!("20"))
        .await
        .expect_err("作用域应先报");
    assert_eq!(error.code(), CODE_SCOPE_NOT_ALLOWED);

    // 键名格式先于注册表。
    let error = write_tenant(&fx, &ctx(TENANT_A), "Theme.radius", json!(1))
        .await
        .expect_err("键名格式应先报");
    assert_eq!(error.code(), CODE_KEY_INVALID);

    // 未登记键没有留下任何行。
    for table in [
        "platform_config",
        "platform_tenant_config",
        "platform_space_config",
    ] {
        let rows: (i64,) = sqlx::query_as(&format!(
            "SELECT count(*) FROM {table} WHERE key = 'notRegistered'"
        ))
        .fetch_one(&fx.admin_pool)
        .await
        .unwrap();
        assert_eq!(rows.0, 0, "{table} 不应有未登记键的行");
    }
}

#[tokio::test]
async fn five_registered_types_are_checked_by_json_type() {
    let _guard = serial().await;
    let fx = setup().await;

    // ⚠️ `newInvoice` 是 flag 本体，通用写路由会先命中 flag 命名空间（D70）；
    // 布尔类型校验走 flag 接口（见下面的 write_flag 分支）。
    let wrong = [
        ("title", json!(123), "string", "number"),
        ("pageSize", json!("4"), "number", "string"),
        ("theme.fontStack", json!({}), "array", "object"),
        ("theme.tokens", json!([]), "object", "array"),
    ];
    for (key, value, expected, actual) in wrong {
        let error = write_tenant(&fx, &ctx(TENANT_A), key, value)
            .await
            .expect_err("错误类型应被拒");
        assert_eq!(error.code(), CODE_TYPE_MISMATCH, "key={key}");
        assert_eq!(error.params().get("expected"), Some(&json!(expected)));
        assert_eq!(error.params().get("actual"), Some(&json!(actual)));
    }

    // 正确类型全部成功。
    write_tenant(&fx, &ctx(TENANT_A), "title", json!("示例平台2"))
        .await
        .unwrap();
    write_tenant(&fx, &ctx(TENANT_A), "pageSize", json!(8))
        .await
        .unwrap();
    fx.service
        .write_flag(
            &ctx(TENANT_A),
            "newInvoice",
            &FlagWriteRequest {
                enabled: Some(json!(true)),
                rollout_percent: None,
            },
        )
        .await
        .expect("布尔类型经 flag 接口写入");
    // 布尔类型错（数字 0）→ 类型错误（不是「0 当真」）。
    let error = fx
        .service
        .write_flag(
            &ctx(TENANT_A),
            "newInvoice",
            &FlagWriteRequest {
                enabled: Some(json!(0)),
                rollout_percent: None,
            },
        )
        .await
        .expect_err("布尔类型错应被拒");
    assert_eq!(error.code(), CODE_TYPE_MISMATCH);
    assert_eq!(error.params().get("expected"), Some(&json!("boolean")));
    assert_eq!(error.params().get("actual"), Some(&json!("number")));
    write_tenant(
        &fx,
        &ctx(TENANT_A),
        "theme.fontStack",
        json!(["system-ui", "sans-serif"]),
    )
    .await
    .unwrap();
    write_tenant(
        &fx,
        &ctx(TENANT_A),
        "theme.tokens",
        json!({"density": "compact"}),
    )
    .await
    .unwrap();

    // 读回类型。
    assert!(
        resolve(&fx, &ctx(TENANT_A), "title")
            .await
            .unwrap()
            .value
            .is_string()
    );
    assert!(
        resolve(&fx, &ctx(TENANT_A), "pageSize")
            .await
            .unwrap()
            .value
            .is_number()
    );
    assert!(
        fx.service
            .read_flag(&ctx_user(TENANT_A, USER_M), "newInvoice")
            .await
            .unwrap()
            .enabled
    );
    let fonts = resolve(&fx, &ctx(TENANT_A), "theme.fontStack")
        .await
        .unwrap();
    assert_eq!(fonts.value.as_array().map(Vec::len), Some(2));
    let tokens = resolve(&fx, &ctx(TENANT_A), "theme.tokens").await.unwrap();
    assert_eq!(tokens.value["density"], json!("compact"));

    // 库侧也必须是 jsonb 类型（不是双重编码的字符串）。
    let types: Vec<(String, String)> = sqlx::query_as(
        "SELECT key, jsonb_typeof(value) FROM platform_tenant_config \
         WHERE tenant_id = 'tenant-a' AND key IN ('pageSize','newInvoice','theme.tokens') \
         ORDER BY key",
    )
    .fetch_all(&fx.admin_pool)
    .await
    .unwrap();
    assert_eq!(
        types,
        vec![
            ("newInvoice".to_string(), "boolean".to_string()),
            ("pageSize".to_string(), "number".to_string()),
            ("theme.tokens".to_string(), "object".to_string()),
        ]
    );
}

#[tokio::test]
async fn space_writes_require_the_branding_whitelist() {
    let _guard = serial().await;
    let fx = setup().await;

    // loginBanner 登记为 space，但**不在**白名单 → 空间级写 400（fail closed）。
    let error = write_space(
        &fx,
        &ctx_space(TENANT_A, SPACE_S1),
        "loginBanner",
        json!("/banner.png"),
    )
    .await
    .expect_err("非白名单键的空间级写应被拒");
    assert_eq!(error.code(), CODE_SCOPE_NOT_ALLOWED);
    assert_eq!(
        error.params().get("reason").and_then(Value::as_str),
        Some("whitelist")
    );

    // 租户级仍可写（非白名单只拦空间级）。
    write_tenant(
        &fx,
        &ctx(TENANT_A),
        "loginBanner",
        json!("/banner-tenant.png"),
    )
    .await
    .expect("租户级写应成功");
    let hit = resolve(&fx, &ctx_space(TENANT_A, SPACE_S1), "loginBanner")
        .await
        .unwrap();
    assert_eq!(hit.value, json!("/banner-tenant.png"));
    assert_eq!(hit.source, ConfigSource::Tenant);

    // 白名单内的 space 键不受影响。
    write_space(
        &fx,
        &ctx_space(TENANT_A, SPACE_S1),
        "logo",
        json!("/branding/space-ok.svg"),
    )
    .await
    .expect("白名单键应能写");

    // 库侧证据：空间表没有 loginBanner 行。
    let rows: (i64,) =
        sqlx::query_as("SELECT count(*) FROM platform_space_config WHERE key = 'loginBanner'")
            .fetch_one(&fx.admin_pool)
            .await
            .unwrap();
    assert_eq!(rows.0, 0);
}

// ---------------------------------------------------------------------------
// AC-S-054：品牌公开接口
// ---------------------------------------------------------------------------

#[tokio::test]
async fn branding_returns_only_whitelisted_keys_and_resolves_the_tenant() {
    let _guard = serial().await;
    let fx = setup().await;

    let t1 = fx.service.branding(Some(TENANT_A)).await.unwrap();
    let map: std::collections::BTreeMap<String, Value> = t1.into_iter().collect();
    assert_eq!(map.get("logo"), Some(&json!("/branding/tenant-a.svg")));
    assert_eq!(map.get("title"), Some(&json!("T1平台")));
    assert_eq!(map.get("primaryColor"), Some(&json!("#111111")));
    // 显式断言 ≠ 空间值（空间级被忽略）。
    assert_ne!(map.get("logo"), Some(&json!("/branding/space-s1.svg")));

    // 键集合 ⊆ 白名单；运维键缺席。
    let keys: BTreeSet<&str> = map.keys().map(String::as_str).collect();
    for key in &keys {
        assert!(
            platform_config::is_branding_key(key),
            "公开品牌接口返回了非白名单键：{key}"
        );
    }
    for forbidden in [
        "conflictStrategy",
        "scimBaseUrl",
        "cacheTtlSeconds",
        "loginBanner",
    ] {
        assert!(!keys.contains(forbidden), "不得泄露 {forbidden}");
    }
    for required in ["logo", "primaryColor", "title", "favicon"] {
        assert!(keys.contains(required), "白名单键必须出现：{required}");
    }

    // 无租户 → 平台默认。
    let platform = fx
        .service
        .branding(None)
        .await
        .unwrap()
        .into_iter()
        .collect::<std::collections::BTreeMap<_, _>>();
    assert_eq!(
        platform.get("logo"),
        Some(&json!("/branding/platform-a.svg"))
    );
    assert_eq!(platform.get("title"), Some(&json!("示例平台")));

    // T2 → 自己的租户值（证明真的按租户解析）。
    let t2 = fx
        .service
        .branding(Some(TENANT_B))
        .await
        .unwrap()
        .into_iter()
        .collect::<std::collections::BTreeMap<_, _>>();
    assert_eq!(t2.get("logo"), Some(&json!("/branding/tenant-b.svg")));

    // 未知 / 空租户 → 平台默认（本仓没有租户注册表）。
    let unknown = fx
        .service
        .branding(Some("tenant-does-not-exist"))
        .await
        .unwrap()
        .into_iter()
        .collect::<std::collections::BTreeMap<_, _>>();
    assert_eq!(
        unknown.get("logo"),
        Some(&json!("/branding/platform-a.svg"))
    );
    let blank = fx
        .service
        .branding(Some(""))
        .await
        .unwrap()
        .into_iter()
        .collect::<std::collections::BTreeMap<_, _>>();
    assert_eq!(blank.get("logo"), Some(&json!("/branding/platform-a.svg")));

    // 与认证读接口取值一致（不是另一套实现）。
    let entries = fx
        .service
        .read_entries(&ctx(TENANT_A), &key_list(&["logo"]))
        .await
        .unwrap();
    assert_eq!(entries[0].value, json!("/branding/tenant-a.svg"));
}

// ---------------------------------------------------------------------------
// AC-S-056 ~ AC-S-059：Feature Flag
// ---------------------------------------------------------------------------

#[tokio::test]
async fn flags_are_tenant_isolated_and_close_returns_200_semantics() {
    let _guard = serial().await;
    let fx = setup().await;

    let t1 = fx
        .service
        .read_flag(&ctx_user(TENANT_A, USER_M), "newInvoice")
        .await
        .unwrap();
    assert!(t1.enabled);
    assert_eq!(t1.source, ConfigSource::Tenant);

    let t2 = fx
        .service
        .read_flag(&ctx_user(TENANT_B, USER_M), "newInvoice")
        .await
        .unwrap();
    assert!(!t2.enabled);
    assert_eq!(t2.source, ConfigSource::Default);

    // 无用户身份 → fail closed（即使总开关为 true）。
    let anonymous = fx
        .service
        .read_flag(&ctx(TENANT_A), "newInvoice")
        .await
        .unwrap();
    assert!(!anonymous.enabled);
}

#[tokio::test]
async fn unknown_and_invalid_flag_names_follow_the_contract() {
    let _guard = serial().await;
    let fx = setup().await;

    for name in ["newinvoce", "newinvoice"] {
        let decision = fx
            .service
            .read_flag(&ctx_user(TENANT_A, USER_M), name)
            .await
            .expect("未登记 flag 不是 404");
        assert!(!decision.enabled, "flag={name}");
        assert_eq!(decision.source, ConfigSource::Default, "flag={name}");
        assert_eq!(decision.flag, name);
    }

    // ⚠️ 规格冲突：AC-S-059 步骤 4 要求 `new-invoice`（kebab）→ 200 + false，
    // 但同一节又写死 flag 名规范 `^[a-z][a-zA-Z0-9]{0,63}$`（**无连字符**）。
    // 按**更严格**的字符集规则实现（D65 的理由：非法名不进日志，防日志注入），
    // 因此 kebab 名走「非法名 → 400」这一侧。冲突已写进 verification-report。
    for name in ["new-invoice", "bad\nINJECTED", &"a".repeat(1024)] {
        let error = fx
            .service
            .read_flag(&ctx_user(TENANT_A, USER_M), name)
            .await
            .expect_err("非法 flag 名应被拒");
        assert_eq!(error.code(), CODE_FLAG_NAME_INVALID);
        assert_eq!(error.status(), 400);
    }

    // 对照：合法且已登记的 flag。
    let ok = fx
        .service
        .read_flag(&ctx_user(TENANT_A, USER_M), "newInvoice")
        .await
        .unwrap();
    assert!(ok.enabled);
}

/// 日志缓冲（`tracing` 订阅器写内存）。
#[derive(Clone, Default)]
struct LogBuffer(Arc<Mutex<Vec<u8>>>);

impl Write for LogBuffer {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().expect("日志缓冲锁").write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.0.lock().expect("日志缓冲锁").flush()
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for LogBuffer {
    type Writer = LogBuffer;

    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

/// 用**生产的**过滤器建 subscriber。
///
/// ⚠️ 这里曾经写的是 `EnvFilter::new("platform_config=warn")` —— 显式放行了
/// `platform_config` 的 target。结果：**生产环境的默认配置把那些 WARN 全吞掉**，
/// 而测试一直绿（REQ-010 QA 判 HIGH）。
///
/// 现在走 `platform_core::log_filter_ignoring_env` —— 它构造的是**没有 `RUST_LOG` 时**
/// 与 `platform-server` 生产启动**完全相同的 directive 集合**（生产用 `log_filter`，
/// 它优先读 `RUST_LOG`；本测试断言的是「默认部署形态」，所以要用不受环境影响的入口）。
/// 「测试放行但生产吞掉」在结构上不可能。
fn subscriber(buffer: &LogBuffer) -> impl tracing::Subscriber + Send + Sync + 'static {
    tracing_subscriber::fmt()
        .with_env_filter(platform_core::log_filter_ignoring_env("info"))
        .with_ansi(false)
        .with_writer(buffer.clone())
        .finish()
}

fn text_of(buffer: &LogBuffer) -> String {
    let bytes = buffer.0.lock().expect("日志缓冲锁").clone();
    String::from_utf8(bytes).expect("日志是 UTF-8")
}

/// AC-S-059：未登记（但名字合法）的 flag → **恰好一条**结构化 WARN，字段含 flag 名；
/// 非法名（换行 / 超长）**一条都不留**（否则可伪造日志行 / 刷爆日志）。
#[tokio::test]
async fn unknown_flags_warn_once_and_invalid_names_do_not_reach_the_log() {
    let _guard = serial().await;
    let fx = setup().await;

    let unknown_buffer = LogBuffer::default();
    {
        let guard = tracing::subscriber::set_default(subscriber(&unknown_buffer));
        let decision = fx
            .service
            .read_flag(&ctx_user(TENANT_A, USER_M), "newinvoce")
            .await
            .expect("未登记 flag 不是 404");
        assert!(!decision.enabled);
        drop(guard);
    }
    let text = text_of(&unknown_buffer);
    assert_eq!(
        text.matches("WARN").count(),
        1,
        "未登记 flag 应恰好一条 WARN：{text}"
    );
    assert!(text.contains("newinvoce"), "WARN 必须带 flag 名：{text}");
    assert!(text.contains("not registered"), "WARN 必须说明原因：{text}");

    let invalid_buffer = LogBuffer::default();
    {
        let guard = tracing::subscriber::set_default(subscriber(&invalid_buffer));
        let long = "a".repeat(1024);
        for name in ["bad\nINJECTED", long.as_str(), "new-invoice"] {
            let _ = fx
                .service
                .read_flag(&ctx_user(TENANT_A, USER_M), name)
                .await
                .expect_err("非法 flag 名应被拒");
        }
        drop(guard);
    }
    let text = text_of(&invalid_buffer);
    assert!(text.is_empty(), "非法 flag 名不得进日志（D65）：{text}");
}

#[tokio::test]
async fn flags_cannot_be_written_through_the_generic_config_route() {
    let _guard = serial().await;
    let fx = setup().await;

    for key in ["beta", "flags.beta.rolloutPercent"] {
        let error = write_tenant(&fx, &ctx(TENANT_A), key, json!(true))
            .await
            .expect_err("flag 只能经 flag 接口改");
        assert_eq!(error.code(), CODE_FLAG_USE_FLAG_ENDPOINT);
        assert_eq!(error.status(), 400);
    }
    // 空间级写也拒绝（先命中 flag 命名空间）。
    let error = write_space(
        &fx,
        &ctx_space(TENANT_A, SPACE_S1),
        "flags.beta.rolloutPercent",
        json!(50),
    )
    .await
    .expect_err("flag 只能经 flag 接口改");
    assert_eq!(error.code(), CODE_FLAG_USE_FLAG_ENDPOINT);

    // 经 flag 接口可以改，且派生键与开关是**同一条数据**。
    set_beta(&fx, Some(true), Some(50)).await;
    let stored = raw_value(
        &fx.admin_pool,
        "platform_tenant_config",
        "flags.beta.rolloutPercent",
    )
    .await;
    assert_eq!(stored.as_deref(), Some("50"));
    let stored_switch = raw_value(&fx.admin_pool, "platform_tenant_config", "beta").await;
    assert_eq!(stored_switch.as_deref(), Some("true"));

    // flags.nosuch.rolloutPercent 未声明 → 400。
    let error = write_tenant(
        &fx,
        &ctx(TENANT_A),
        "flags.nosuch.rolloutPercent",
        json!(50),
    )
    .await
    .expect_err("未登记的派生键应被拒");
    assert_eq!(error.code(), CODE_KEY_NOT_REGISTERED);
}

#[tokio::test]
async fn rollout_is_deterministic_monotonic_and_bounded() {
    let _guard = serial().await;
    let fx = setup().await;
    set_beta(&fx, Some(true), None).await;
    let users = rollout_users();

    // 1) 同一用户连读 10 次完全一致。
    set_beta(&fx, None, Some(50)).await;
    let resolver = fx.new_resolver();
    let base_ctx = ctx_user(TENANT_A, USER_M);
    resolver.refresh(&base_ctx).await.unwrap();
    let first = resolver.flag(&ctx_user(TENANT_A, "u-3001"), "beta").enabled;
    for _ in 0..10 {
        assert_eq!(
            resolver.flag(&ctx_user(TENANT_A, "u-3001"), "beta").enabled,
            first
        );
    }

    // 2) 分布落在 [70, 130]。
    let s50 = hit_set(&fx, 50, &users).await;
    assert!(
        (70..=130).contains(&s50.len()),
        "p=50 命中数 {} 不在 [70,130]",
        s50.len()
    );

    // 3) 单调性 / 可复原。
    let s60 = hit_set(&fx, 60, &users).await;
    assert!(
        s50.is_subset(&s60) && s60.len() > s50.len(),
        "S50 必须严格包含于 S60：|S50|={} |S60|={}",
        s50.len(),
        s60.len()
    );
    let s50_again = hit_set(&fx, 50, &users).await;
    assert_eq!(s50_again, s50, "改回原比例后集合必须完全复原");
    let s10 = hit_set(&fx, 10, &users).await;
    assert!(s10.is_subset(&s50));

    // 4) 边界。
    assert!(hit_set(&fx, 0, &users).await.is_empty());
    assert_eq!(hit_set(&fx, 100, &users).await.len(), users.len());

    // 5) 两个独立实例 + 独立公式。
    set_beta(&fx, None, Some(50)).await;
    let r1 = fx.new_resolver();
    let r2 = fx.new_resolver();
    r1.refresh(&base_ctx).await.unwrap();
    r2.refresh(&base_ctx).await.unwrap();
    for user in &users {
        let a = r1.flag(&ctx_user(TENANT_A, user), "beta").enabled;
        let b = r2.flag(&ctx_user(TENANT_A, user), "beta").enabled;
        assert_eq!(a, b, "两个独立实例必须一致：{user}");
        assert_eq!(a, expected_hit("beta", user, 50), "公式必须一致：{user}");
    }

    // 6) 无身份：p=100 也 false。
    set_beta(&fx, None, Some(100)).await;
    let resolver = fx.new_resolver();
    resolver.refresh(&ctx(TENANT_A)).await.unwrap();
    assert!(!resolver.flag(&ctx(TENANT_A), "beta").enabled);
    let empty = platform_core::RequestContext::new();
    resolver.refresh(&empty).await.unwrap();
    assert!(!resolver.flag(&empty, "beta").enabled);
    // 对照：有身份 + p=100 → true。
    resolver.refresh(&base_ctx).await.unwrap();
    assert!(resolver.flag(&ctx_user(TENANT_A, "u-3001"), "beta").enabled);
}

/// 子进程里只做一件事：把 p=50 的命中集合打到 stdout（由下面的用例拉起）。
#[tokio::test]
#[ignore = "只由 rollout_across_two_processes_is_identical 通过子进程调用"]
async fn rollout_child_process_helper() {
    if std::env::var("REQ010_ROLLOUT_CHILD").as_deref() != Ok("1") {
        return;
    }
    let _guard = serial().await;
    // **不重置**夹具：父进程已经把 beta / p=50 写进共享 DB。
    let fx = setup_without_reset().await;
    let users = rollout_users();
    let resolver = fx.new_resolver();
    let base = ctx_user(TENANT_A, USER_M);
    resolver.refresh(&base).await.expect("刷新应成功");
    let set: BTreeSet<String> = users
        .iter()
        .filter(|user| resolver.flag(&ctx_user(TENANT_A, user), "beta").enabled)
        .cloned()
        .collect();
    println!(
        "ROLLOUT_SET:{}",
        set.into_iter().collect::<Vec<_>>().join(",")
    );
}

/// AC-S-057 步骤 7：**两个独立进程**对同一批用户的判定必须完全相同。
///
/// 同进程的两个解析器实例**抓不到 `RandomState`**（每个进程只播种一次，
/// 两个实例拿到同一份种子）；只有真进程才能暴露「多实例下同一用户一半请求命中」。
#[tokio::test]
async fn rollout_across_two_processes_is_identical() {
    let _guard = serial().await;
    let fx = setup().await;
    set_beta(&fx, Some(true), Some(50)).await;
    let users = rollout_users();

    let run_set = |resolver: &DbConfigResolver| {
        users
            .iter()
            .filter(|user| resolver.flag(&ctx_user(TENANT_A, user), "beta").enabled)
            .cloned()
            .collect::<BTreeSet<String>>()
    };
    let resolver = fx.new_resolver();
    resolver
        .refresh(&ctx_user(TENANT_A, USER_M))
        .await
        .expect("刷新应成功");
    let parent = run_set(&resolver);

    let exe = std::env::current_exe().expect("测试可执行文件路径");
    let output = std::process::Command::new(exe)
        .args([
            "--exact",
            "rollout_child_process_helper",
            "--ignored",
            "--nocapture",
        ])
        .env("REQ010_ROLLOUT_CHILD", "1")
        .env("DATABASE_URL_TEST", required_test_database_url())
        .output()
        .expect("子进程应能启动");
    assert!(
        output.status.success(),
        "子进程失败：{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let line = stdout
        .lines()
        .find(|line| line.starts_with("ROLLOUT_SET:"))
        .unwrap_or_else(|| panic!("子进程没有输出命中集合：{stdout}"));
    let child: BTreeSet<String> = line
        .trim_start_matches("ROLLOUT_SET:")
        .split(',')
        .filter(|item| !item.is_empty())
        .map(str::to_string)
        .collect();

    assert_eq!(
        child, parent,
        "两个进程的命中集合必须完全相同（RandomState 会在这里失败）"
    );
}

#[tokio::test]
async fn rollout_out_of_range_is_rejected_on_write_and_fails_closed_on_read() {
    let _guard = serial().await;
    let fx = setup().await;
    set_beta(&fx, Some(true), None).await;

    for percent in [json!(101), json!(-1), json!(50.5)] {
        let error = fx
            .service
            .write_flag(
                &ctx(TENANT_A),
                "beta",
                &FlagWriteRequest {
                    enabled: None,
                    rollout_percent: Some(percent.clone()),
                },
            )
            .await
            .expect_err("越界比例应被拒");
        assert_eq!(error.code(), CODE_VALUE_OUT_OF_RANGE, "percent={percent}");
        assert_eq!(error.status(), 400);
    }

    // 经通用写路由写派生键 101 也走同一套范围校验。
    let error = write_tenant(&fx, &ctx(TENANT_A), "flags.beta.rolloutPercent", json!(101))
        .await
        .expect_err("派生键也校验范围");
    // 通用路由先命中 flag 命名空间：这是 D70 的预期顺序。
    assert_eq!(error.code(), CODE_FLAG_USE_FLAG_ENDPOINT);

    // 读路径：库里的脏数据 → enabled=false + WARN，**不是 400/500**。
    set_beta(&fx, None, Some(50)).await;
    sqlx::query(
        "UPDATE platform_tenant_config SET value = '101'::jsonb \
         WHERE tenant_id = 'tenant-a' AND key = 'flags.beta.rolloutPercent'",
    )
    .execute(&fx.admin_pool)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE platform_config_epoch SET epoch = epoch + 1 WHERE scope_key = 'tenant:tenant-a'",
    )
    .execute(&fx.admin_pool)
    .await
    .unwrap();

    let resolver = fx.new_resolver();
    let base_ctx = ctx_user(TENANT_A, USER_M);
    resolver.refresh(&base_ctx).await.unwrap();
    assert!(
        !resolver.flag(&base_ctx, "beta").enabled,
        "脏比例必须 fail closed"
    );
}

// ---------------------------------------------------------------------------
// AC-S-052 / AC-S-053 / D55：缓存失效（成对比较）
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_platform_change_invalidates_every_tenant_and_survives_a_higher_tenant_epoch() {
    let _guard = serial().await;
    let fx = setup().await;

    // 两个独立实例（各自缓存，共享 DB）；TTL 设 300s（明知很长也不影响结论）。
    let r1 = std::sync::Arc::new(DbConfigResolver::with_ttl(
        platform_config::store::ConfigStore::new(fx.app_db.clone()),
        fx.registry.clone(),
        Duration::from_secs(300),
    ));
    let r2 = fx.new_resolver();

    let c_a = ctx(TENANT_B); // 用 T2 读 favicon（T1/T2 都没有覆盖）
    r1.refresh(&c_a).await.unwrap();
    r2.refresh(&c_a).await.unwrap();
    let favicon = ConfigKey::parse("favicon").unwrap();
    assert_eq!(
        r1.resolve(&c_a, &favicon).unwrap().value,
        json!("/favicon-platform.svg")
    );

    // 平台级改值（不重启、不清缓存、不等待）。
    write_platform(&fx, "favicon", json!("/favicon-platform2.svg"))
        .await
        .unwrap();
    r2.refresh(&c_a).await.unwrap();
    assert_eq!(
        r2.resolve(&c_a, &favicon).unwrap().value,
        json!("/favicon-platform2.svg"),
        "实例 2 的下一次读必须是新值（失效信号来自共享 DB）"
    );
    r1.refresh(&c_a).await.unwrap();
    assert_eq!(
        r1.resolve(&c_a, &favicon).unwrap().value,
        json!("/favicon-platform2.svg")
    );

    // 构造掩蔽顺序：先把 T1 的租户 epoch 推到高于 platform。
    for value in 51..=55 {
        write_tenant(&fx, &ctx(TENANT_A), "pageSize", json!(value))
            .await
            .unwrap();
    }
    let tenant_epoch = epoch(&fx.admin_pool, "tenant:tenant-a").await;
    let platform_epoch = epoch(&fx.admin_pool, "platform").await;
    assert!(
        tenant_epoch > platform_epoch,
        "掩蔽前提不成立：tenant={tenant_epoch} platform={platform_epoch}"
    );

    // T1 读一次，缓存以当时的两个 epoch 为版本。
    let c_1 = ctx(TENANT_A);
    r1.refresh(&c_1).await.unwrap();
    assert_eq!(
        r1.resolve(&c_1, &favicon).unwrap().value,
        json!("/favicon-platform2.svg")
    );

    // 改平台级 → platform epoch 仍**小于** tenant epoch。
    write_platform(&fx, "favicon", json!("/favicon-platform3.svg"))
        .await
        .unwrap();
    let platform_epoch = epoch(&fx.admin_pool, "platform").await;
    assert!(platform_epoch < tenant_epoch, "掩蔽顺序必须成立");

    r1.refresh(&c_1).await.unwrap();
    assert_eq!(
        r1.resolve(&c_1, &favicon).unwrap().value,
        json!("/favicon-platform3.svg"),
        "用 max(epoch) 的实现会在这里返回旧值"
    );

    // epoch 归属：平台写入不伪装成租户写入。
    assert_eq!(epoch(&fx.admin_pool, "tenant:tenant-a").await, 5);
    assert_eq!(epoch(&fx.admin_pool, "tenant:tenant-b").await, 0);
}

#[tokio::test]
async fn every_tenant_sees_a_platform_change_on_the_next_request() {
    let _guard = serial().await;
    let fx = setup().await;

    let r = fx.new_resolver();
    let favicon = ConfigKey::parse("favicon").unwrap();
    let a = ctx(TENANT_A);
    let b = ctx(TENANT_B);
    r.refresh(&a).await.unwrap();
    r.refresh(&b).await.unwrap();
    assert_eq!(
        r.resolve(&a, &favicon).unwrap().value,
        json!("/favicon-platform.svg")
    );
    assert_eq!(
        r.resolve(&b, &favicon).unwrap().value,
        json!("/favicon-platform.svg")
    );

    write_platform(&fx, "favicon", json!("/favicon-f2.svg"))
        .await
        .unwrap();

    for ctx in [&a, &b, &ctx_space(TENANT_A, SPACE_S1)] {
        r.refresh(ctx).await.unwrap();
        assert_eq!(
            r.resolve(ctx, &favicon).unwrap().value,
            json!("/favicon-f2.svg")
        );
    }
}

// ---------------------------------------------------------------------------
// AC-S-062 / D56 / D71：表权限、唯一写通道、jsonb
// ---------------------------------------------------------------------------

#[tokio::test]
async fn base_app_can_read_but_cannot_write_the_platform_table_directly() {
    let _guard = serial().await;
    let fx = setup().await;

    // 1) SELECT 可以（平台级行全体可见是设计）。
    let rows: (i64,) = sqlx::query_as("SELECT count(*) FROM platform_config")
        .fetch_one(&fx.app_pool)
        .await
        .expect("base_app 应能 SELECT");
    assert!(rows.0 > 0);

    // 2) 三种直接 DML 全部被拒（SQLSTATE 42501）。
    for sql in [
        "INSERT INTO platform_config (key, value) VALUES ('hijack', '\"x\"'::jsonb)",
        "UPDATE platform_config SET value = '\"hijack\"'::jsonb WHERE key = 'logo'",
        "DELETE FROM platform_config WHERE key = 'logo'",
    ] {
        let error = raw_error(&fx.app_pool, sql)
            .await
            .unwrap_or_else(|| panic!("直接 DML 必须被拒：{sql}"));
        assert_eq!(
            sqlstate(&error).as_deref(),
            Some("42501"),
            "sql={sql} error={error}"
        );
    }

    // 3) 唯一写通道：SECURITY DEFINER 函数。
    sqlx::query("SELECT platform_config_upsert('logo', '\"/branding/definer.svg\"'::jsonb)")
        .execute(&fx.app_pool)
        .await
        .expect("函数应可执行");
    assert_eq!(
        raw_value(&fx.admin_pool, "platform_config", "logo").await,
        Some("\"/branding/definer.svg\"".to_string())
    );
    assert_eq!(
        epoch(&fx.admin_pool, "platform").await,
        1,
        "函数必须同事务 bump epoch"
    );

    // 4) 权限矩阵（逐项）。
    let privileges: (bool, bool, bool, bool, bool) = sqlx::query_as(
        "SELECT has_table_privilege('base_app','platform_config','SELECT'), \
                has_table_privilege('base_app','platform_config','INSERT'), \
                has_table_privilege('base_app','platform_config','UPDATE'), \
                has_table_privilege('base_app','platform_config','DELETE'), \
                has_function_privilege('base_app','platform_config_upsert(text,jsonb)','EXECUTE')",
    )
    .fetch_one(&fx.admin_pool)
    .await
    .unwrap();
    assert_eq!(privileges, (true, false, false, false, true));

    // 5) 值 / 行数没被越权写改掉。
    assert_eq!(
        raw_value(&fx.admin_pool, "platform_config", "logo").await,
        Some("\"/branding/definer.svg\"".to_string())
    );
    let hijack: (i64,) =
        sqlx::query_as("SELECT count(*) FROM platform_config WHERE key = 'hijack'")
            .fetch_one(&fx.admin_pool)
            .await
            .unwrap();
    assert_eq!(hijack.0, 0);
}

#[tokio::test]
async fn tenant_and_space_rows_are_isolated_by_rls() {
    let _guard = serial().await;
    let fx = setup().await;

    // 租户级：只看到自己租户。
    let query = platform_db::TenantQuery::new(
        "SELECT DISTINCT tenant_id FROM platform_tenant_config WHERE tenant_id = {tenant}",
    );
    let a: Vec<(String,)> = fx.app_db.fetch_all(&ctx(TENANT_A), &query).await.unwrap();
    assert_eq!(a, vec![(TENANT_A.to_string(),)]);
    let b: Vec<(String,)> = fx.app_db.fetch_all(&ctx(TENANT_B), &query).await.unwrap();
    assert_eq!(b, vec![(TENANT_B.to_string(),)]);

    // 不设租户上下文 → 0 行（fail closed，不是报错）。
    assert_eq!(
        unscoped_row_count(&fx.app_db, "platform_tenant_config")
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        unscoped_row_count(&fx.app_db, "platform_space_config")
            .await
            .unwrap(),
        0
    );

    // 空间级：复合维度 (tenant_id, space_id)。
    let space_query = platform_db::TenantQuery::new(
        "SELECT tenant_id, space_id, value::text FROM platform_space_config \
         WHERE tenant_id = {tenant} AND space_id = ? AND key = 'logo'",
    )
    .bind(SPACE_S1.to_string());
    let rows: Vec<(String, String, String)> = fx
        .app_db
        .fetch_all(&ctx_space(TENANT_A, SPACE_S1), &space_query)
        .await
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].2, "\"/branding/space-s1.svg\"");
    let rows_b: Vec<(String, String, String)> = fx
        .app_db
        .fetch_all(&ctx_space(TENANT_B, SPACE_S1), &space_query)
        .await
        .unwrap();
    assert_eq!(rows_b.len(), 1);
    assert_eq!(rows_b[0].2, "\"/branding/space-t2-s1.svg\"");

    // 空间表的**写侧** RLS（WITH CHECK）：写不进他租户的行 / 不能把本租户的行改挂到别租。
    {
        // 第一条失败后事务会进入 aborted 状态 —— 每条拒绝断言各开一个事务。
        let mut tx = fx.app_pool.begin().await.expect("应用连接应能开事务");
        sqlx::query("SELECT set_config('app.tenant_id', 'tenant-a', true)")
            .execute(&mut *tx)
            .await
            .expect("应能设置租户上下文");
        let foreign_insert = sqlx::query(
            "INSERT INTO platform_space_config (tenant_id, space_id, key, value) \
             VALUES ('tenant-b','space-sales-1','footer','\"hijack\"'::jsonb)",
        )
        .execute(&mut *tx)
        .await;
        let error = foreign_insert.expect_err("跨租户空间写必须被 WITH CHECK 拒绝");
        assert_eq!(sqlstate(&error).as_deref(), Some("42501"), "error={error}");
        tx.rollback().await.expect("回滚应成功");
    }
    {
        let mut tx = fx.app_pool.begin().await.expect("应用连接应能开事务");
        sqlx::query("SELECT set_config('app.tenant_id', 'tenant-a', true)")
            .execute(&mut *tx)
            .await
            .expect("应能设置租户上下文");
        let foreign_update = sqlx::query(
            "UPDATE platform_space_config SET tenant_id = 'tenant-b' \
             WHERE tenant_id = 'tenant-a' AND space_id = 'space-sales-1' AND key = 'logo'",
        )
        .execute(&mut *tx)
        .await;
        let error = foreign_update.expect_err("把行改挂到别租必须被拒绝");
        assert_eq!(sqlstate(&error).as_deref(), Some("42501"), "error={error}");
        tx.rollback().await.expect("回滚应成功");
    }

    // 两张表都开了 RLS 且各至少一条 policy，条件引用 tenant_id。
    let policies: Vec<(String, String, String, String)> = sqlx::query_as(
        "SELECT tablename, policyname, qual, with_check FROM pg_policies \
         WHERE schemaname = 'public' \
           AND tablename IN ('platform_tenant_config','platform_space_config') \
         ORDER BY tablename",
    )
    .fetch_all(&fx.admin_pool)
    .await
    .unwrap();
    assert_eq!(policies.len(), 2, "policies={policies:?}");
    for (table, _policy, qual, check) in policies {
        assert!(
            qual.contains("tenant_id"),
            "{table} 的 USING 必须引用 tenant_id：{qual}"
        );
        assert!(
            check.contains("tenant_id"),
            "{table} 的 WITH CHECK 必须引用 tenant_id：{check}"
        );
    }
}

#[tokio::test]
async fn all_three_value_columns_are_jsonb() {
    let _guard = serial().await;
    let fx = setup().await;
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT table_name, data_type FROM information_schema.columns \
         WHERE table_schema = 'public' AND column_name = 'value' \
           AND table_name IN ('platform_config','platform_tenant_config','platform_space_config') \
         ORDER BY table_name",
    )
    .fetch_all(&fx.admin_pool)
    .await
    .unwrap();
    assert_eq!(
        rows,
        vec![
            ("platform_config".to_string(), "jsonb".to_string()),
            ("platform_space_config".to_string(), "jsonb".to_string()),
            ("platform_tenant_config".to_string(), "jsonb".to_string()),
        ]
    );
}

#[tokio::test]
async fn the_schema_migration_is_reversible_without_dropping_the_historical_table() {
    let _guard = serial().await;
    let _fx = setup().await;

    let root = repo_root();
    let up = std::fs::read_to_string(root.join("migrations/20261118001000_config_schema.up.sql"))
        .expect("up 迁移应可读");
    let down =
        std::fs::read_to_string(root.join("migrations/20261118001000_config_schema.down.sql"))
            .expect("down 迁移应可读");
    assert!(
        !down.contains("DROP TABLE platform_tenant_config"),
        "down 不得删掉 REQ-006 的历史表"
    );

    // 在**独立的数据库**里跑 up/down/up（不碰 base_test 的 public schema）。
    let admin_url = required_test_database_url();
    let suffix = uuid::Uuid::new_v4().to_string().replace('-', "");
    let name = format!("cfg_mig_{}", &suffix[..12]);
    let pool = sqlx::PgPool::connect(&admin_url).await.expect("管理员连接");
    sqlx::query(&format!("CREATE DATABASE {name}"))
        .execute(&pool)
        .await
        .expect("应能建测试库");

    let probe_options = sqlx::postgres::PgConnectOptions::from_str(&admin_url)
        .expect("管理员连接串应可解析")
        .database(&name);
    let probe = sqlx::postgres::PgPoolOptions::new()
        .max_connections(2)
        .connect_with(probe_options)
        .await
        .expect("应能连测试库");

    // 先建 REQ-006 的 TEXT 版本表（模拟迁移前状态）。
    sqlx::raw_sql(
        "CREATE TABLE platform_tenant_config (\
           tenant_id TEXT NOT NULL, key TEXT NOT NULL, value TEXT NOT NULL, \
           updated_at TIMESTAMPTZ NOT NULL DEFAULT now(), PRIMARY KEY (tenant_id, key)); \
         INSERT INTO platform_tenant_config (tenant_id, key, value) VALUES \
           ('t','s','\"s\"'), ('t','n','60'), ('t','b','true');",
    )
    .execute(&probe)
    .await
    .expect("前置表应能创建");

    sqlx::raw_sql(&up).execute(&probe).await.expect("up 应成功");
    let types: Vec<(String, String)> =
        sqlx::query_as("SELECT key, jsonb_typeof(value) FROM platform_tenant_config ORDER BY key")
            .fetch_all(&probe)
            .await
            .unwrap();
    assert_eq!(
        types,
        vec![
            ("b".to_string(), "boolean".to_string()),
            ("n".to_string(), "number".to_string()),
            ("s".to_string(), "string".to_string()),
        ]
    );

    sqlx::raw_sql(&down)
        .execute(&probe)
        .await
        .expect("down 应成功");
    let data_type: String = sqlx::query_scalar(
        "SELECT data_type FROM information_schema.columns \
         WHERE table_name = 'platform_tenant_config' AND column_name = 'value'",
    )
    .fetch_one(&probe)
    .await
    .unwrap();
    assert_eq!(data_type, "text", "down 后 value 应回到 TEXT");

    sqlx::raw_sql(&up)
        .execute(&probe)
        .await
        .expect("再次 up 应成功");
    let types_again: Vec<(String, String)> =
        sqlx::query_as("SELECT key, jsonb_typeof(value) FROM platform_tenant_config ORDER BY key")
            .fetch_all(&probe)
            .await
            .unwrap();
    assert_eq!(types_again, types, "up/down/up 后类型与值语义必须完全一致");

    probe.close().await;
    sqlx::query(&format!("DROP DATABASE {name}"))
        .execute(&pool)
        .await
        .expect("应能删测试库");
    pool.close().await;
}

/// D67 的三件套：manifest + **权限点种子迁移** + capabilities。
/// 这里在真实库上做双向对账：manifest 侧（扫全部 `crates/*` 与 `modules/*`）
/// 与 `platform_permission` 必须完全一致 —— 少了任何一条，真实启动会 fail-fast。
#[tokio::test]
async fn the_permission_registry_matches_every_manifest() {
    let _guard = serial().await;
    let fx = setup().await;
    let manifests = platform_rbac::collect_manifest_permissions(&repo_root())
        .expect("manifest 侧的权限点应能收集");
    let store = platform_rbac::RbacStore::new(fx.app_db.clone());
    platform_rbac::registry::require_consistent(&store, &manifests)
        .await
        .expect("权限点注册表必须与 manifest 双向一致（D67）");

    let codes = store.permission_codes().await.expect("注册表应可读");
    for code in [
        "config:entry:write",
        "config:flag:write",
        "config:platform:write",
        "config:platform:read",
    ] {
        assert!(codes.iter().any(|found| found == code), "缺少种子：{code}");
    }
}

#[tokio::test]
async fn the_reserved_tenant_id_is_rejected_on_every_entry_point() {
    let _guard = serial().await;
    let fx = setup().await;
    let reserved = ctx("platform");

    let error = fx
        .service
        .read_entries(&reserved, &key_list(&["logo"]))
        .await
        .expect_err("保留租户 id 应被拒");
    assert_eq!(error.code(), CODE_TENANT_RESERVED);
    assert_eq!(error.status(), 400);

    let error = fx
        .service
        .write_entries(
            &reserved,
            &WriteRequest {
                scope: "tenant".to_string(),
                entries: vec![WriteEntry {
                    key: "pageSize".to_string(),
                    value: json!(1),
                }],
            },
            WriteRoute::Scoped,
        )
        .await
        .expect_err("保留租户 id 应被拒");
    assert_eq!(error.code(), CODE_TENANT_RESERVED);

    let error = fx
        .service
        .branding(Some("platform"))
        .await
        .expect_err("应被拒");
    assert_eq!(error.code(), CODE_TENANT_RESERVED);

    let resolver = fx.new_resolver();
    let error = resolver.refresh(&reserved).await.expect_err("应被拒");
    assert_eq!(error.code(), CODE_TENANT_RESERVED);
}

#[tokio::test]
async fn the_platform_read_endpoint_only_serves_platform_scoped_keys() {
    let _guard = serial().await;
    let fx = setup().await;

    // 平台级键：读到平台值。
    let entries = fx
        .service
        .read_platform_entries(&ctx(TENANT_A), &key_list(&["scimBaseUrl"]))
        .await
        .unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].value, json!("https://scim.example.com"));
    assert_eq!(entries[0].source, ConfigSource::Platform);

    // 租户级键 → 400（不是静默返回租户值）。
    let error = fx
        .service
        .read_platform_entries(&ctx(TENANT_A), &key_list(&["pageSize"]))
        .await
        .expect_err("平台端点不服务租户级键");
    assert_eq!(error.code(), CODE_SCOPE_NOT_ALLOWED);

    // 未登记键 → 404。
    let error = fx
        .service
        .read_platform_entries(&ctx(TENANT_A), &key_list(&["notRegistered"]))
        .await
        .expect_err("未登记键应 404");
    assert_eq!(error.code(), CODE_KEY_NOT_REGISTERED);
    assert_eq!(error.status(), 404);
}

// ---------------------------------------------------------------------------
// P17（D128 / Q7）：D127 的 `boolean ⇒ flag` 推导约束要有专测
// ---------------------------------------------------------------------------

/// 带点号的布尔键（REQ-013 的 `sso.*`）**走 Scoped 写路由成功、走 Flag 路由被拒**。
///
/// 这是 D127 的实质行为：推导规则加了 `&& is_valid_flag_name(...)` 之后，
/// 这类键不再落进 flag 命名空间 —— 而在此之前，它**两条路都走不通**：
///
/// | 路由 | 加约束前 | 加约束后 |
/// |------|----------|----------|
/// | `WriteRoute::Scoped` | 400 `CONFIG_FLAG_USE_FLAG_ENDPOINT` | **200（本用例第 1 段）** |
/// | flag 接口 | 400 `FLAG_NAME_INVALID`（点号非法） | 400 `FLAG_NAME_INVALID`（**不是 flag**，本用例第 2 段） |
///
/// 会红的情形：
/// - 约束被删回 `boolean ⇒ flag`（Scoped 写又会 400 `CONFIG_FLAG_USE_FLAG_ENDPOINT`）；
/// - 反过来把点号键误放行到 flag 接口（第 2 段会拿到 200 而不是 400）。
#[tokio::test]
async fn a_dotted_boolean_key_is_writable_as_config_and_rejected_as_a_flag() {
    let _guard = serial().await;
    let fx = setup().await;
    let ctx = ctx(TENANT_A);

    // 1) Scoped 写路由：**成功**，且值真的落到**本租户**那一行。
    for (key, value) in [
        ("sso.enforce", json!(true)),
        ("sso.jitProvisioning", json!(false)),
    ] {
        let written = write_tenant(&fx, &ctx, key, value.clone())
            .await
            .unwrap_or_else(|error| {
                panic!(
                    "{key} 必须能经通用写路由写入（D127 约束前的症状是 {}）：{error:?}",
                    error.code()
                )
            });
        assert_eq!(written, 1);
        // ⚠️ 按 `(tenant_id, key)` 读 —— 同一张表里有别的租户的行
        //    （且别的测试**二进制**共用 base_test，裸 `WHERE key = $1` 会读到别人的值）。
        let stored: Option<String> = sqlx::query_scalar(
            "SELECT value::text FROM platform_tenant_config WHERE tenant_id = $1 AND key = $2",
        )
        .bind(TENANT_A)
        .bind(key)
        .fetch_optional(&fx.admin_pool)
        .await
        .expect("应能读到租户级行");
        assert_eq!(
            stored,
            Some(value.to_string()),
            "{key} 的写入值应落到 TENANT_A 那一行"
        );
    }
    // 读回也是「租户」来源，而不是「没有这个键」。
    let entries = fx
        .service
        .read_entries(&ctx, &key_list(&["sso.enforce"]))
        .await
        .expect("读回应成功");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].value, json!(true));
    assert_eq!(entries[0].source, ConfigSource::Tenant);

    // 2) flag 路由：**被拒**（它根本不是 flag）。
    //    点号先撞上 flag 名规范 → `FLAG_NAME_INVALID`（400）。
    //    ⚠️ 这里断言的是「被拒」这件事本身：若约束反过来把点号键当 flag，
    //    则第一步（Scoped 写）会先红；若 flag 名规范被放宽，这里会变成 200。
    for key in ["sso.enforce", "sso.jitProvisioning"] {
        let error = fx
            .service
            .write_flag(
                &ctx_user(TENANT_A, USER_A),
                key,
                &FlagWriteRequest {
                    enabled: Some(json!(false)),
                    rollout_percent: None,
                },
            )
            .await
            .expect_err("带点号的布尔键不得经 flag 路由写入");
        assert_eq!(error.status(), 400, "{key}：应 400，实际 {error:?}");
        assert_eq!(
            error.code(),
            CODE_FLAG_NAME_INVALID,
            "{key}：应因非法 flag 名被拒，实际 {error:?}"
        );
    }

    // 3) 既有契约**不变**：`beta` 仍是 flag（Scoped 写仍被拒），经 flag 接口可改。
    let error = write_tenant(&fx, &ctx, "beta", json!(true))
        .await
        .expect_err("beta 是 flag，通用路由仍应拒绝");
    assert_eq!(error.code(), CODE_FLAG_USE_FLAG_ENDPOINT);
    // `rolloutPercent = 100`：全量生效，所以 `enabled` 与总开关一致
    // （40 之类的比例会按 `sha256(flag:user)` 的桶决定，断言会依赖哈希值）。
    let decision = fx
        .service
        .write_flag(
            &ctx_user(TENANT_A, USER_A),
            "beta",
            &FlagWriteRequest {
                enabled: Some(json!(true)),
                rollout_percent: Some(json!(100)),
            },
        )
        .await
        .expect("beta 经 flag 路由应可写");
    assert!(decision.enabled);
    assert_eq!(decision.flag, "beta");
}

// ---------------------------------------------------------------------------
// REQ-014 D182 / D186：审计分页的**登记值**必须与契约 C7 的口径一致
// ---------------------------------------------------------------------------

/// `audit.page.defaultSize` 的**登记值**必须与契约 C7 的口径一致。
///
/// # 这条断言盯的是「登记在骗人」
///
/// 复验（H-01）查出：manifest 登记 `50`、`DEFAULT_LIMIT`（运行时）是 `20` →
/// 运维在配置面板看到 50 而接口默认返回 20。**计数类断言抓不到它**，
/// 必须把**登记侧的两处**与**契约口径**逐字比对。
///
/// # 本用例实际覆盖的**两处**（不是三处）
///
/// 1. `crates/platform-config/manifest.json`（登记值，经 `ConfigRegistry` 读）；
/// 2. `platform_config` 表里的行（迁移种子写进去的值）。
///
/// 第 2 处比对的目标是**契约 C7 的口径常量**（下面写死的 `CONTRACT_DEFAULT_LIMIT`），
/// **不是**运行时常量 `platform_audit::DEFAULT_LIMIT` —— `platform-config`
/// **不依赖** `platform-audit`（刻意的分层，本轮也不许加依赖）。
///
/// # 运行时默认由谁钉住
///
/// 「运行时默认 == 20」由 `platform-audit` 自己的
/// `the_defaults_are_the_contract_defaults`（`crates/platform-audit/src/query.rs` 单测）钉住。
/// 那条单测**看不到** manifest / DB 的登记值 → 本用例与它**各自只钉一半**。
///
/// 把「运行时默认 / manifest 登记 / DB 登记」**三者接起来**的跨 crate 断言在
/// `crates/platform-server/tests/audit_default_size_real_db.rs`
/// （装配层是唯一同时依赖 `platform-audit` 与 `platform-config` 的地方）。
#[tokio::test]
async fn the_audit_page_default_size_matches_the_contract_in_manifest_and_db() {
    let _guard = serial().await;
    let fx = setup().await;

    const KEY: &str = "audit.page.defaultSize";

    // 1) manifest 登记值。
    let registry = platform_config::ConfigRegistry::from_manifest_root(&repo_root())
        .expect("配置注册表应能加载");
    let registered = registry.get(KEY).expect("该键必须已登记");
    assert_eq!(
        registered.default,
        json!(20),
        "manifest 登记的 {KEY} 必须是 20（D182 / D186）"
    );

    // 2) 契约 C7 的口径（20）—— **常量**，不是运行时值。
    //
    // ⚠️ 不在这里引用 `platform_audit::DEFAULT_LIMIT` —— `platform-config`
    // **不依赖** `platform-audit`（那是刻意的分层），本轮也不许加依赖。
    // 「运行时默认 == 20」由 `platform-audit` 自己的
    // `the_defaults_are_the_contract_defaults`（`query.rs` 单测）钉住；
    // 本用例负责的是「**登记值**不能与契约口径不一致」。
    // 三者接起来的断言在 `platform-server/tests/audit_default_size_real_db.rs`。
    const CONTRACT_DEFAULT_LIMIT: i64 = 20;
    assert_eq!(
        registered.default,
        json!(CONTRACT_DEFAULT_LIMIT),
        "登记值与契约 C7 的口径（20）**必须相等** —— 否则就是「登记在骗人」（D182）"
    );

    // 3) DB 里的登记行（迁移种子；已进 `_sqlx_migrations` 不会重跑 → 也会漂）。
    let stored: Option<Value> =
        sqlx::query_scalar("SELECT value FROM platform_config WHERE key = $1")
            .bind(KEY)
            .fetch_optional(&fx.admin_pool)
            .await
            .expect("应当能读 DB 配置行");
    assert_eq!(
        stored,
        Some(json!(CONTRACT_DEFAULT_LIMIT)),
        "DB 里的 {KEY} 必须是 20（迁移种子 20261120000400）"
    );
}
