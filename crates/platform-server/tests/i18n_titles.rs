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
//!
//! # 跨栈：`TITLES` 与 i18n 词库（REQ-014 的 L-04 / D194）
//!
//! 上面两条只管后端表**内部**自洽 —— 前端按 `code` 取本地化文案时，词库里
//! 少一条同样会让用户看到裸码。真实不变量（D125）是「**每个会进响应体的
//! 错误码，`TITLES` 与 i18n 都要登记**」，机械形式是三条：
//!
//! 1. [`titles_cover_every_code`]：`TITLES` ⊇ `all_codes()`；
//! 2. [`every_error_code_is_registered_in_every_locale`]：`all_codes()` 里
//!    **除组织事件码外**的码，每个 locale 都有 `error.<code>`；
//! 3. [`titles_error_codes_are_registered_in_every_locale`]：`TITLES` 的**错误码**
//!    在每个 locale 里都有 `error.<code>`。
//!
//! ⚠️ **两侧的集合不相等，也不该相等**：`TITLES` 混装 78 个错误码 + 6 个
//! `org.*` 组织事件码，i18n 混装那 78 个 + 6 个**当前不进 HTTP 响应体**的码
//! （`CONFIG_MISSING_REQUIRED` 等）→ 差集 6↔6。`TITLES.len() == i18n 条数`
//! 是**数字巧合**，所以这里钉的是**包含**关系，**不是**相等（D194 作废了
//! D192 的「集合相等」判据）。

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use platform_core::titles::{FALLBACK_TITLE, TITLES, is_registered, title_of_code};

/// 组织事件码在 [`all_codes`] 里的来源标签（用于把它们从**错误码**里排除）。
///
/// 用来源标签而不是「码长什么样」来排除：正则/前缀猜「哪些是事件码」会把
/// 将来某个真的以 `org.` 开头的错误码误伤，也会把前缀写错的事件码漏过去。
const ORG_EVENT_SOURCE: &str = "platform-org::notify";

/// 已注册的语言（与 `packages/i18n/src/locales/*.json` 一一对应）。
///
/// 两条跨栈断言**必须逐个 locale 都查**：只查英文的话，中文词库少一条就抓不到。
const LOCALES: &[&str] = &["en-US", "zh-CN"];

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
    // 出站 IdP 与单点登出（REQ-022）：9 个平台错误码。
    // ⚠️ 8 个 `oauth.*` 审计**事件码不进这里**（D223）—— 事件码不是 HTTP 错误码。
    for code in platform_oauth::error::ALL_CODES {
        codes.push(("platform-oauth", code));
    }
    // 应用接入（REQ-016）：6 个平台错误码。
    // ⚠️ 3 个 `app.*` 审计**事件码不进这里**（D223 同款）—— 事件码不是 HTTP 错误码。
    for code in platform_app::error::ALL_CODES {
        codes.push(("platform-app", code));
    }
    // 通知系统（REQ-015）：8 个平台错误码。
    // ⚠️ 4 个 `notify.*` 审计**事件码不进这里**（D223 同款）—— 事件码不是 HTTP 错误码。
    for code in platform_notify::error::ALL_CODES {
        codes.push(("platform-notify", code));
    }
    // 文件存储与导入导出（REQ-018）：9 个平台错误码。
    // ⚠️ `StorageError::Internal` **不进** `ALL_CODES`（D330）—— 它映射到平台的
    // `INTERNAL_ERROR`（500），凭空发 `STORAGE_*` 就是一枚没人触发的死码。
    for code in platform_storage::error::ALL_CODES {
        codes.push(("platform-storage", code));
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
///
/// REQ-022（出站 IdP）再加 **9** 个码（`OAUTH_*`）→ 84 + 9 = **93**。
///
/// REQ-016（应用接入）再加 **6** 个码（`APP_*`）→ 93 + 6 = **99**。
///
/// ⚠️ **不是** 102：8 个 `oauth.*` 审计**事件码不进 `TITLES`**（D223）——
/// 进了会让本文件的三个既有断言必红，且事件码不进响应体。
/// 同理 3 个 `app.*` 审计事件码也不进（D223 同款）。
#[test]
fn the_title_table_has_the_pinned_number_of_rows() {
    const BASELINE_ROWS_BEFORE_REQ_013: usize = 58;
    const REQ_013_NEW_CODES: usize = 21;
    const REQ_014_NEW_CODES: usize = 5;
    const REQ_022_NEW_CODES: usize = 9;
    const REQ_016_NEW_CODES: usize = 6;
    // REQ-015（通知）再加 **8** 个码（`NOTIFY_*`）→ 99 + 8 = **107**。
    // ⚠️ 4 个 `notify.*` 审计**事件码不进本表**（D223）—— 进了就是 111，本条必红。
    const REQ_015_NEW_CODES: usize = 8;
    // REQ-018（文件存储与导入导出）再加 **9** 个码（`STORAGE_*`）→ 107 + 9 = **116**。
    // ⚠️ `StorageError::Internal` 不进 `ALL_CODES`（D330）—— 进了就是 117，本条必红。
    const REQ_018_NEW_CODES: usize = 10;
    /// REQ-019 新增 1 个平台级码（`INVALID_REQUEST`，D341）。
    const REQ_019_NEW_CODES: usize = 1;
    assert_eq!(
        TITLES.len(),
        BASELINE_ROWS_BEFORE_REQ_013
            + REQ_013_NEW_CODES
            + REQ_014_NEW_CODES
            + REQ_022_NEW_CODES
            + REQ_016_NEW_CODES
            + REQ_015_NEW_CODES
            + REQ_018_NEW_CODES
            + REQ_019_NEW_CODES,
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

// ------------------------------------------------------------------ 跨栈：TITLES ↔ i18n

// 前端侧的词库自检在 `packages/i18n` 的单测里另有实现；
// 这里查的是**后端码 → 词库**（两端都不会替对方兜住漏项）。

/// 仓库根目录（`CARGO_MANIFEST_DIR` 是 `crates/platform-server`，往上两级才是仓库根）。
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/platform-server 的上两级应当是仓库根")
        .to_path_buf()
}

/// 某个 locale 的词库（`packages/i18n/src/locales/<locale>.json`）里的
/// `error.*` 键集合（**去掉 `error.` 前缀**，剩下的是码）。
///
/// 词库是扁平结构（键形如 `"error.MFA_REQUIRED"`，不是嵌套的 `error` 对象），
/// 所以这里按键前缀取，而不是 `value["error"]`。
///
/// ⚠️ 词库文件**读不到就直接失败**：静默返回空集合会让断言「因为没有词库所以通过」，
/// 那是假绿。
fn locale_error_codes(locale: &str) -> BTreeSet<String> {
    let path = repo_root()
        .join("packages/i18n/src/locales")
        .join(format!("{locale}.json"));
    let raw = std::fs::read_to_string(&path).unwrap_or_else(|err| {
        panic!(
            "读不到词库 {}（{err}）—— 跨栈断言必须真的读文件，不能跳过",
            path.display()
        )
    });
    let value: serde_json::Value = serde_json::from_str(&raw)
        .unwrap_or_else(|err| panic!("词库 {} 不是合法 JSON：{err}", path.display()));
    let object = value
        .as_object()
        .unwrap_or_else(|| panic!("词库 {} 的顶层必须是对象", path.display()));

    let codes: BTreeSet<String> = object
        .keys()
        .filter_map(|key| key.strip_prefix("error."))
        .map(str::to_owned)
        .collect();
    assert!(
        !codes.is_empty(),
        "词库 {} 里一个 `error.*` 条目都没有（词库结构变了？）",
        path.display()
    );

    // 词条不得为空：空串等于「登记了但没内容」，前端会渲染成空白。
    let blank: Vec<&str> = object
        .iter()
        .filter(|(key, value)| {
            key.starts_with("error.")
                && value.as_str().map(str::trim).unwrap_or_default().is_empty()
        })
        .map(|(key, _)| key.as_str())
        .collect();
    assert!(
        blank.is_empty(),
        "词库 {} 有 {} 个 `error.*` 条目是空的：{blank:?}",
        path.display(),
        blank.len()
    );

    codes
}

/// [`all_codes`] 里**会进响应体的错误码**（= 全部码去掉组织事件码）。
///
/// 组织事件码从 [`ORG_EVENT_SOURCE`] 这个**已知的来源标签**排除，
/// 不靠正则猜（见该常量的注释）。
fn error_codes() -> BTreeSet<&'static str> {
    all_codes()
        .into_iter()
        .filter(|(source, _)| *source != ORG_EVENT_SOURCE)
        .map(|(_, code)| code)
        .collect()
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

/// **跨栈 1/2（D194 断言 2）**：每个**会进响应体的错误码**，每个 locale 的词库都有
/// `error.<code>` 条目。
///
/// 这是 D125 的真实不变量在前端侧的那一半 —— 后端有标题、前端没文案，用户仍然
/// 看到裸码。判据是**包含**（`error_codes ⊆ 词库`），不是相等：
/// 词库里另有 6 个「当前不进 HTTP 响应体」的码（`IO_ERROR` 等），它们在
/// `TITLES` 缺席是**正确**的，反过来不成立（见模块注释 / D194）。
#[test]
fn every_error_code_is_registered_in_every_locale() {
    let codes = error_codes();
    assert!(!codes.is_empty(), "错误码清单不能为空");

    // 排除动作必须**真的排除了东西**：否则「6 个 org 码被漏掉」会被静默通过。
    let org_codes: BTreeSet<&str> = all_codes()
        .into_iter()
        .filter(|(source, _)| *source == ORG_EVENT_SOURCE)
        .map(|(_, code)| code)
        .collect();
    assert_eq!(
        org_codes.len(),
        platform_org::notify::OrgEventKind::ALL_CODES.len(),
        "从 all_codes() 里按来源 `{ORG_EVENT_SOURCE}` 取到的码数必须等于 OrgEventKind::ALL_CODES"
    );
    assert!(
        codes.is_disjoint(&org_codes),
        "组织事件码不该出现在错误码清单里：{:?}",
        codes.intersection(&org_codes).collect::<Vec<_>>()
    );

    let mut failures: Vec<String> = Vec::new();
    for locale in LOCALES {
        let known = locale_error_codes(locale);
        let missing: Vec<&&str> = codes
            .iter()
            .filter(|code| !known.contains(**code))
            .collect();
        if !missing.is_empty() {
            let mut missing: Vec<&str> = missing.into_iter().copied().collect();
            missing.sort_unstable();
            failures.push(format!("{locale} 缺 {} 条：{missing:?}", missing.len()));
        }
    }
    assert!(
        failures.is_empty(),
        "{} 个错误码在词库里没登记（D125：会进响应体的码必须两边都登记）：\n{}",
        codes.len(),
        failures.join("\n")
    );

    println!(
        "错误码 {} 个（已排除 {} 个组织事件码），{} 个 locale 均齐全",
        codes.len(),
        org_codes.len(),
        LOCALES.len()
    );
}

/// **跨栈 2/2（D194 断言 3）**：`TITLES` 里的**错误码**，每个 locale 的词库都有
/// `error.<code>` 条目。
///
/// 与上一条方向相反：上一条防「后端有、前端没有的响应体码」，这一条防
/// 「`TITLES` 里登记了却没人写文案的键」（改码或改名时最容易漏的一侧）。
/// 同样排除 6 个 `org.*` 组织事件码 —— 前端没有 org 命名空间，也不消费它们。
#[test]
fn titles_error_codes_are_registered_in_every_locale() {
    let codes = error_codes();
    let title_error_codes: BTreeSet<&str> = TITLES
        .iter()
        .map(|(code, _)| *code)
        .filter(|code| codes.contains(code))
        .collect();
    assert!(
        !title_error_codes.is_empty(),
        "TITLES 里一个错误码都没有（表结构变了？）"
    );

    let mut failures: Vec<String> = Vec::new();
    for locale in LOCALES {
        let known = locale_error_codes(locale);
        let mut missing: Vec<&str> = title_error_codes
            .iter()
            .filter(|code| !known.contains(**code))
            .copied()
            .collect();
        if !missing.is_empty() {
            missing.sort_unstable();
            failures.push(format!("{locale} 缺 {} 条：{missing:?}", missing.len()));
        }
    }
    assert!(
        failures.is_empty(),
        "TITLES 里 {} 个错误码在词库里没登记：\n{}",
        title_error_codes.len(),
        failures.join("\n")
    );

    println!(
        "TITLES 的错误码 {} 个（另有 {} 个组织事件码不参与），{} 个 locale 均齐全",
        title_error_codes.len(),
        TITLES.len() - title_error_codes.len(),
        LOCALES.len()
    );
}
