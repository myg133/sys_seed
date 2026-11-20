//! `TITLES` 的**完备性**（REQ-021 / AC-S-088 / D113）。
//!
//! # 为什么这个测试是本需求的核心
//!
//! rev 1 的验收方式是「扫 `ProblemDetails` 构造点的字符串字面量」——
//! 但 **54 处中文 `title` 是由 `title_of()` / `title()` 函数产出的**，
//! 构造点拿到的是**函数调用** → 那种扫法**扫完 0 命中、全部通过**。
//! 这不是「数错了」，是「验收方式必然漏报」。
//!
//! 所以判据改成两件**机械可判**的事：
//!
//! 1. `TITLES` 表里每一行的值都是 ASCII（[`titles_are_ascii_and_unique`]）；
//! 2. 每个 crate 显式导出的 `ALL_CODES` 都被 `TITLES` 覆盖
//!    （[`titles_cover_every_code`]）。
//!
//! 第 2 条是**唯一**能发现「表漏了一项」的手段：`code()` 是 `match` 里的
//! `&'static str`，运行时无法自动枚举 —— 没有 `ALL_CODES`，漏项就静默走
//! ASCII 兜底，看起来「都通过了」（design R9）。

use std::collections::BTreeSet;

use platform_core::titles::{FALLBACK_TITLE, TITLES, is_registered, title_of_code};

/// 各 crate 导出的全部 code（键 = 来源 crate，值 = 码）。
///
/// 这个函数故意把所有清单**并排**写出来：新增一个 crate 的 code 清单时，
/// 必须在这里加一行，漏了就是「表里有、测试不知道」—— 反向的
/// [`no_orphan_titles`] 会发现它。
fn all_codes() -> Vec<(&'static str, &'static str)> {
    let mut codes: Vec<(&'static str, &'static str)> = Vec::new();

    for code in platform_core::problem::ALL_CODES {
        codes.push(("platform-core", code));
    }
    for code in platform_core::money::ALL_CODES {
        codes.push(("platform-core::money", code));
    }
    for code in platform_iam::error::ALL_CODES {
        codes.push(("platform-iam", code));
    }
    for code in platform_org::error::ALL_CODES {
        codes.push(("platform-org", code));
    }
    for code in platform_rbac::error::ALL_CODES {
        codes.push(("platform-rbac", code));
    }
    for code in platform_config::error::ALL_CODES {
        codes.push(("platform-config", code));
    }
    // 数据访问层的码（REQ-021 补齐）：`DbError::code()` 是稳定机器可读码，
    // 经 `ConfigError::Database(code)` 进响应体的 `detail` 字段
    // （`the config store is unavailable ({code})`）→ 必须有标题。
    for code in platform_db::error::ALL_CODES {
        codes.push(("platform-db", code));
    }
    // 组织事件码（通知标题，不是 HTTP 错误码）—— 同一个 `TITLES` 表。
    for code in platform_org::notify::OrgEventKind::ALL_CODES {
        codes.push(("platform-org::notify", code));
    }
    // `/health/ready` 的 503 错误体的三个码。
    for code in [
        platform_server::health::CODE_DB_UNAVAILABLE,
        platform_server::health::CODE_MIGRATIONS_PENDING,
        platform_server::health::CODE_MIGRATION_DIRTY,
    ] {
        codes.push(("platform-server::health", code));
    }

    codes
}

/// 每个 code 都能从 `TITLES` 里取到一个**非兜底**的标题。
///
/// 「取到兜底」也算失败：兜底意味着「这一项漏登记了」，而那正是本测试要抓的东西。
#[test]
fn titles_cover_every_code() {
    let codes = all_codes();
    assert!(!codes.is_empty(), "code 清单不能为空");

    let mut missing: Vec<String> = Vec::new();
    for (source, code) in &codes {
        if !is_registered(code) {
            missing.push(format!("{source}: {code}"));
        }
        assert_eq!(
            title_of_code(code) == FALLBACK_TITLE,
            !is_registered(code),
            "`{code}`（{source}）的兜底判定与登记状态不一致"
        );
    }
    assert!(
        missing.is_empty(),
        "TITLES 缺少 {} 个 code 的标题：{missing:#?}",
        missing.len()
    );

    // 汇总数量（报告用，同时防止「清单被改成空数组」这类退化）。
    let unique: BTreeSet<&str> = codes.iter().map(|(_, code)| *code).collect();
    assert!(
        unique.len() >= 45,
        "登记的 code 数量异常偏少（{}），检查 ALL_CODES 是否被截断",
        unique.len()
    );
    println!(
        "TITLES = {} 行；各 crate 的 code 合计 {} 条（去重后 {} 条）",
        TITLES.len(),
        codes.len(),
        unique.len()
    );
}

/// 钉死 `TITLES` 的行数（D125：新增错误码必须同步登记标题）。
///
/// # 为什么是 79 而不是 acceptance 写的 77
///
/// REQ-013 的 acceptance 写「`TITLES.len()` 期望值 = 56（基线）+ 21 = **77**」。
/// **基线实测是 58 行**，不是 56 —— 规格里的 56 是数错了（漏了
/// `platform-db` 的 5 个码里的 2 个与 `platform-server::health` 的 3 个码
/// 之外的偏差；精确计数见 `git show HEAD:...titles.rs`）。
///
/// 已上报 BA。这里钉的是**实测值** 58 + 21 = 79：
/// 断言的目的（「加码必须同时加标题」）不受影响，被钉住的仍然是真实的表。
///
/// REQ-014（审计日志）再加 **5** 个码（`AUDIT_*`）→ 79 + 5 = **84**。
#[test]
fn the_title_table_has_the_pinned_number_of_rows() {
    const BASELINE_ROWS_BEFORE_REQ_013: usize = 58;
    const REQ_013_NEW_CODES: usize = 21;
    const REQ_014_NEW_CODES: usize = 5;
    assert_eq!(
        TITLES.len(),
        BASELINE_ROWS_BEFORE_REQ_013 + REQ_013_NEW_CODES + REQ_014_NEW_CODES,
        "TITLES 行数变了：新增错误码必须同时登记标题（D125）"
    );
}

/// 表的每个值都是 ASCII，且键唯一。
#[test]
fn titles_are_ascii_and_unique() {
    let mut seen = BTreeSet::new();
    for (code, title) in TITLES {
        assert!(
            title.is_ascii(),
            "title 必须是 ASCII（不得是中文）：{code} → {title}"
        );
        assert!(!title.trim().is_empty(), "title 不得为空：{code}");
        assert_ne!(
            *title, *code,
            "title 不得等于 code 本身（那等于把机器码漏给用户）：{code}"
        );
        assert!(seen.insert(*code), "TITLES 出现重复键：{code}");
    }
}

/// **反向**：`TITLES` 里的每个键都必须被某个 `ALL_CODES` 认领。
///
/// 没有这一条，往表里加一个拼错的键永远不会被发现（`title_of_code` 查不到它，
/// 而它又不在任何清单里）。
#[test]
fn no_orphan_titles() {
    let claimed: BTreeSet<&str> = all_codes().iter().map(|(_, code)| *code).collect();
    let orphans: Vec<&str> = TITLES
        .iter()
        .map(|(code, _)| *code)
        .filter(|code| !claimed.contains(code))
        .collect();
    assert!(
        orphans.is_empty(),
        "TITLES 里有 {} 个 code 没有任何 crate 清单认领：{orphans:?}",
        orphans.len()
    );
}

/// 错误码的形状是 `UPPER_SNAKE`；事件码是 `org.kebab-case`（两套命名空间都钉死）。
#[test]
fn codes_follow_their_namespace_convention() {
    for (code, _) in TITLES {
        let is_error_code = code
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_');
        let is_event_code = code.starts_with("org.")
            && code.bytes().all(|byte| {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'.' || byte == b'-'
            });
        assert!(
            is_error_code || is_event_code,
            "code 不符合任一命名空间（错误码 UPPER_SNAKE / 事件码 org.kebab-case）：{code}"
        );
    }
}

/// **运行时**判据（AC-S-088 步骤 3/4）：把每个 code 装进真实的错误体再序列化，
/// 断言 `title` / `detail` / `type` / `instance` / `code` 五个字段都**不含 CJK**。
///
/// 为什么不「触发每个错误再抓响应」：很多 code 没有可触发的条件（例如
/// `TOKEN_REUSE_DETECTED` 需要构造重放）。这里直接走
/// [`platform_core::problem::ProblemDetails`] 的**同一个构造函数**与**同一张表**
/// —— 与生产响应体是同一段代码（D85），且有触发条件的错误另在
/// `i18n_api.rs` 的 HTTP 用例里真实抓过响应体。
#[test]
fn every_registered_code_produces_a_cjk_free_problem_body() {
    let ctx = platform_core::RequestContext::new().with_tenant("tenant-a");
    for (source, code) in all_codes() {
        let details = platform_core::problem::ProblemDetails::from_context(
            code,
            title_of_code(code),
            400,
            "the request was rejected",
            &ctx,
            "/api/x",
            None,
        );
        let value = serde_json::to_value(&details).expect("错误体应当能序列化");
        for field in ["title", "detail", "type", "instance", "code"] {
            let text = value[field].as_str().unwrap_or_default();
            assert!(
                !text.chars().any(
                    |ch| matches!(ch as u32, 0x4E00..=0x9FFF | 0x3000..=0x303F | 0xFF00..=0xFFEF)
                ),
                "{source} 的 `{code}`：`{field}` 含 CJK：{text}"
            );
        }
        assert_eq!(
            value["title"],
            title_of_code(code),
            "响应体的 title 必须来自 TITLES（{code}）"
        );
    }
}

/// 未登记的 code 走 ASCII 兜底，**不返回 code 本身**（也不返回中文）。
#[test]
fn unknown_codes_fall_back_to_an_ascii_placeholder() {
    let title = title_of_code("NOT_A_REGISTERED_CODE");
    assert_eq!(title, FALLBACK_TITLE);
    assert!(title.is_ascii());
    assert_ne!(title, "NOT_A_REGISTERED_CODE");
}
