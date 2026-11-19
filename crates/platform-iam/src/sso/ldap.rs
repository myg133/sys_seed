//! LDAP bind 认证（REQ-013 AC-S-104 / AC-S-105）。
//!
//! # 两个本领域最经典的漏洞，本文件各守一个
//!
//! | 漏洞 | 守在哪 |
//! |------|--------|
//! | **空密码 = anonymous bind 成功** → 任意用户名都能登录 | [`is_empty_password`]：**连接之前**拦截（调用方在 service 层入口判） |
//! | **LDAP 注入**（用户名拼进 filter 不带转义） | [`escape_filter_value`]：RFC 4515 转义 `\ * ( ) NUL` |
//!
//! # `simple_bind` 的返回值陷阱（S18）
//!
//! `ldap3::Ldap::simple_bind(dn, pw)` 返回 `Result<LdapResult, LdapError>`。
//! **密码错误时外层仍是 `Ok`** —— `rc = 49`（`invalidCredentials`）装在 `LdapResult` 里。
//! 直接 `simple_bind(..).await?` 当成功 → **任意密码都能登录**。
//!
//! 所以**判定点不在包装层**（D129）：[`SsoLdapConn::simple_bind`] 原样返回
//! `LdapResult`，由 [`authenticate`] 用 [`bind_result`] 读 `rc == 0` 做判定。
//! 包装层只透传，判定在 service 层 —— 那一层能被 `Ok(LdapResult { rc: 49, .. })`
//! 的测试替身直接驱动，于是「把判定删掉」会被用例抓住，而不是靠人眼看源码。
//! （正确用法先例：`platform-org/src/ldap.rs:481-495`。）
//!
//! # 为什么要有 trait（P12）
//!
//! `ldap3` **没有**服务端/内存实现；`platform-org` 的 `InMemoryLdapSource` 只实现 `search`、
//! **不能测 bind**。没有 [`SsoLdapClient`] 这一层，
//! 「空密码必须 0 次 connect / 0 次 bind」这条断言**没有观测面**。
//!
//! 本 trait 用装箱 future（[`SsoBoxFuture`]）而不是 `async fn` ——
//! `async fn` 在 trait 上不可对象化，而我们需要 `Box<dyn SsoLdapConn>`。
//!
//! # bind 次数期望（P13 / AC-S-104 步骤 8）
//!
//! 正常登录：`connect = 1` / `search = 1` / `bind = 2`
//! （**服务账号 1 次**用于 search + **用户凭据 1 次**用于认证）。
//! 服务账号来自环境变量（`BASE_SSO_LDAP_BIND_DN` / `..._PASSWORD`）；
//! 不配则匿名 bind —— 那**仍然是一次 bind 调用**，计数不变。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::config::{LdapSettings, SsoBoxFuture};

/// 目录条目。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LdapEntry {
    /// 条目的 DN。
    pub dn: String,
    /// 属性（**名已转小写** —— LDAP 的属性名大小写不敏感）。
    pub attributes: BTreeMap<String, Vec<String>>,
}

impl LdapEntry {
    /// 构造。
    pub fn new(dn: impl Into<String>, attributes: BTreeMap<String, Vec<String>>) -> Self {
        Self {
            dn: dn.into(),
            attributes,
        }
    }

    /// 取某属性的第一个值（属性名大小写不敏感）。
    pub fn first(&self, attribute: &str) -> Option<&str> {
        self.attributes
            .get(&attribute.to_ascii_lowercase())
            .and_then(|values| values.first())
            .map(String::as_str)
    }
}

/// LDAP 客户端错误。
///
/// **不含 DN、不含目录错误原文** —— 它们会被调用方映射成对外错误码，
/// 对外的响应体不得携带这些内容（AC-S-104 步骤 2）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SsoLdapError {
    /// 连不上。
    Connection,
    /// bind 失败（凭据错 / 权限不足）。
    Bind,
    /// 查询失败。
    Search,
    /// 配置非法。
    Config,
}

impl std::fmt::Display for SsoLdapError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Connection => f.write_str("the ldap server is unreachable"),
            Self::Bind => f.write_str("the ldap bind failed"),
            Self::Search => f.write_str("the ldap search failed"),
            Self::Config => f.write_str("the ldap configuration is invalid"),
        }
    }
}

impl std::error::Error for SsoLdapError {}

/// 一个已连接（可 bind / search）的 LDAP 会话。
pub trait SsoLdapConn: Send + Sync {
    /// 在 `base` 下按 `filter` 检索 `attrs`。
    fn search(
        &mut self,
        base: &str,
        filter: &str,
        attrs: &[&str],
    ) -> SsoBoxFuture<'_, Result<Vec<LdapEntry>, SsoLdapError>>;

    /// 用 `dn` + `pw` 做 simple bind，**原样**返回协议结果。
    ///
    /// **本方法不做「成功 / 失败」的判定**（D129）—— 外层 `Ok` 只代表
    /// 「请求发出并收到响应」，`rc != 0`（如 `49`）仍在 `LdapResult` 里（S18）。
    ///
    /// 判定统一由调用方用 [`bind_result`] 做（见 [`authenticate`]）。
    /// 这样「把判定改坏」会被 service 层的用例直接抓到：测试替身返回
    /// `Ok(LdapResult { rc: 49, .. })` 时会**流到**那一行判据上。
    fn simple_bind(
        &mut self,
        dn: &str,
        pw: &str,
    ) -> SsoBoxFuture<'_, Result<ldap3::LdapResult, SsoLdapError>>;
}

/// LDAP 客户端（每次登录新建连接，见 design §5.2：不做连接池）。
pub trait SsoLdapClient: Send + Sync {
    /// 建连接。
    fn connect(&self) -> SsoBoxFuture<'_, Result<Box<dyn SsoLdapConn>, SsoLdapError>>;
}

/// 空 / 纯空白密码。
///
/// **必须在发请求之前判**（AC-S-104 步骤 4/5/8：`connect == 0`、`bind == 0`）。
pub fn is_empty_password(password: &str) -> bool {
    password.trim().is_empty()
}

/// `simple_bind` 的结果码 → 认证是否成功。
///
/// # 为什么单独抽成纯函数（P16，D128 / Q4 / D129）
///
/// 「`rc != 0` → 失败」这一行是**「任意密码都能登录」的唯一防线**。
/// 它一旦被写死在 [`Ldap3Conn::simple_bind`] 里就无法被机械验证 ——
/// 要驱动那一层必须有一条真实的 LDAP 连接（`ldap3` 没有服务端/内存实现）。
///
/// D129 之后判定点已经上移到 service 层（[`authenticate`] 里的
/// `bind_result(&conn.simple_bind(..).await?)`）：包装层只透传 `LdapResult`，
/// 于是测试替身可以返回 `Ok(LdapResult { rc: 49, .. })` **流到**判定那一行，
/// 「把判定改坏」会让 service 层的用例直接变红。
///
/// 本函数则继续守住 `rc → bool` 的映射本身（含与 `ldap3::LdapResult::success()`
/// 的逐值对齐），这样判据与「有没有调用它」各有各的观测面。
pub fn bind_succeeded(rc: u32) -> bool {
    rc == 0
}

/// 从一个**真实的** `ldap3::LdapResult` 判定认证是否成功。
///
/// 这一层存在的理由：让「`rc` 被真的读出来」这件事有一个**可被直接驱动的类型**。
/// 它是 [`authenticate`] 两处 bind 调用的判据（D129 把判定点放在 service 层），
/// 于是 `rc → bool` 的映射既被 [`bind_succeeded`] 的纯函数单测覆盖，
/// 又被本函数的真实 `LdapResult` 用例覆盖，还被 service 层那些
/// 注入 `Ok(LdapResult { rc: 49, .. })` 的用例覆盖「有没有真的调用它」。
///
/// 它**不接受** `Result`：外层 `Ok` 只代表「请求发出并收到响应」，
/// 与认证成败无关（S18）—— 把这一区分做进签名，误用就写不出来。
pub fn bind_result(result: &ldap3::LdapResult) -> bool {
    bind_succeeded(result.rc)
}

/// RFC 4515 的 filter 值转义。
///
/// 五个字符：`\` → `\5c`、`*` → `\2a`、`(` → `\28`、`)` → `\29`、NUL → `\00`。
///
/// 不转义 = LDAP 注入：用户名填 `*` 或 `)(uid=*` 可绕过匹配
/// （LDAP 集成第二经典的漏洞，§7）。
pub fn escape_filter_value(raw: &str) -> String {
    let mut escaped = String::with_capacity(raw.len());
    for ch in raw.chars() {
        match ch {
            '\\' => escaped.push_str("\\5c"),
            '*' => escaped.push_str("\\2a"),
            '(' => escaped.push_str("\\28"),
            ')' => escaped.push_str("\\29"),
            '\0' => escaped.push_str("\\00"),
            other => escaped.push(other),
        }
    }
    escaped
}

/// 把用户名代入配置的 filter（用户名**必须**先转义）。
pub fn build_user_filter(settings: &LdapSettings, username: &str) -> String {
    settings
        .user_search_filter
        .replace("{username}", &escape_filter_value(username))
}

/// 认证成功后的身份信息。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LdapIdentity {
    /// 用户的 DN —— 这是 `external_id`（P13：DN 比 uid / mail 稳定）。
    pub dn: String,
    /// 映射属性（`mailAttribute`）的取值。
    pub email: Option<String>,
}

/// 认证过程里的**可观测计数**（测试用来断言「发请求之前就拦住了」）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LdapCounters {
    /// `connect` 次数。
    pub connects: u32,
    /// `search` 次数。
    pub searches: u32,
    /// `simple_bind` 次数。
    pub binds: u32,
}

/// 用 LDAP 认证一个用户。
///
/// 顺序（design §5）：连接 → **服务账号 bind** → search 出用户 DN →
/// **用户凭据 bind** → 取映射字段。
///
/// 用户不存在（search 空）与密码错（bind `rc != 0`）都返回 [`SsoLdapError::Bind`]
/// —— 调用方把它们映射成**同一个** `401 SSO_LDAP_BIND_FAILED`（防账号枚举）。
pub async fn authenticate(
    client: &dyn SsoLdapClient,
    settings: &LdapSettings,
    service_bind: Option<(&str, &str)>,
    username: &str,
    password: &str,
) -> Result<LdapIdentity, SsoLdapError> {
    let mut conn = client.connect().await?;

    // 1) 服务账号 bind（用于 search）。未配置凭据 → 匿名 bind（**仍是一次 bind**）。
    let (service_dn, service_password) = service_bind.unwrap_or(("", ""));
    if !bind_result(&conn.simple_bind(service_dn, service_password).await?) {
        return Err(SsoLdapError::Bind);
    }

    // 2) search 出用户条目。用户名已按 RFC 4515 转义。
    let filter = build_user_filter(settings, username);
    let mail_attribute = settings.mail_attribute.to_ascii_lowercase();
    let entries = conn
        .search(&settings.base_dn, &filter, &[mail_attribute.as_str()])
        .await?;

    // 0 条 = 用户不存在；多条 = 有歧义。两者都当作「认证失败」（不暴露细节）。
    let [entry] = entries.as_slice() else {
        // ⚠️ 不为「用户不存在」再发一次 bind —— 那会让 bind 次数随账号是否存在而变，
        //    正是账号枚举的观测面。这里直接按失败处理。
        return Err(SsoLdapError::Bind);
    };

    // 3) 用用户凭据 bind —— **必须**判 `rc == 0`（S18）。
    //
    //    判定在这里，不在包装层：包装层只透传 `LdapResult`（D129），
    //    所以这一行是「任意密码都能登录」的**唯一防线**，且它在 service 层，
    //    能被注入 `Ok(LdapResult { rc: 49, .. })` 的替身直接驱动。
    if !bind_result(&conn.simple_bind(&entry.dn, password).await?) {
        return Err(SsoLdapError::Bind);
    }

    Ok(LdapIdentity {
        dn: entry.dn.clone(),
        email: entry
            .first(&mail_attribute)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string),
    })
}

// ---------------------------------------------------------------------------
// 生产实现：包 `ldap3`
// ---------------------------------------------------------------------------

/// 基于 `ldap3` 的客户端（生产用）。
#[derive(Debug, Clone)]
pub struct Ldap3SsoClient {
    url: String,
}

impl Ldap3SsoClient {
    /// 用连接串构造（调用方已校验过 `ldap://`）。
    pub fn new(url: impl Into<String>) -> Self {
        Self { url: url.into() }
    }
}

impl SsoLdapClient for Ldap3SsoClient {
    fn connect(&self) -> SsoBoxFuture<'_, Result<Box<dyn SsoLdapConn>, SsoLdapError>> {
        let url = self.url.clone();
        Box::pin(async move {
            let (conn, ldap) = ldap3::LdapConnAsync::new(&url)
                .await
                .map_err(|_| SsoLdapError::Connection)?;
            // 连接的后台读取循环放进独立任务。
            ldap3::drive!(conn);
            Ok(Box::new(Ldap3Conn { ldap }) as Box<dyn SsoLdapConn>)
        })
    }
}

/// `ldap3::Ldap` 的包装。
struct Ldap3Conn {
    ldap: ldap3::Ldap,
}

impl SsoLdapConn for Ldap3Conn {
    fn search(
        &mut self,
        base: &str,
        filter: &str,
        attrs: &[&str],
    ) -> SsoBoxFuture<'_, Result<Vec<LdapEntry>, SsoLdapError>> {
        let base = base.to_string();
        let filter = filter.to_string();
        let attrs: Vec<String> = attrs.iter().map(|attr| (*attr).to_string()).collect();
        Box::pin(async move {
            let requested: Vec<&str> = attrs.iter().map(String::as_str).collect();
            let (entries, _result) = self
                .ldap
                .search(&base, ldap3::Scope::Subtree, &filter, &requested)
                .await
                .map_err(|_| SsoLdapError::Search)?
                .success()
                .map_err(|_| SsoLdapError::Search)?;
            Ok(entries
                .into_iter()
                .map(ldap3::SearchEntry::construct)
                .map(|entry| {
                    // 属性名转小写（LDAP 属性名大小写不敏感）。
                    let attributes = entry
                        .attrs
                        .into_iter()
                        .map(|(name, values)| (name.to_ascii_lowercase(), values))
                        .collect();
                    LdapEntry::new(entry.dn, attributes)
                })
                .collect())
        })
    }

    fn simple_bind(
        &mut self,
        dn: &str,
        pw: &str,
    ) -> SsoBoxFuture<'_, Result<ldap3::LdapResult, SsoLdapError>> {
        let dn = dn.to_string();
        let pw = pw.to_string();
        Box::pin(async move {
            // 本层**不做判定**，只把协议结果原样透传（D129）：
            // 「外层 `Ok` 即成功」是 S18 的坑，把判定留在这里就无法被测试驱动
            // （`ldap3` 没有服务端/内存实现）。判定在 [`authenticate`] 里，
            // 用 [`bind_result`] 读 `rc`。
            self.ldap
                .simple_bind(&dn, &pw)
                .await
                .map_err(|_| SsoLdapError::Bind)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_five_rfc4515_characters_are_escaped() {
        assert_eq!(escape_filter_value(r"a\b"), r"a\5cb");
        assert_eq!(escape_filter_value("a*b"), "a\\2ab");
        assert_eq!(escape_filter_value("a(b"), "a\\28b");
        assert_eq!(escape_filter_value("a)b"), "a\\29b");
        assert_eq!(escape_filter_value("a\0b"), "a\\00b");
        assert_eq!(escape_filter_value("erin"), "erin");
    }

    /// 三个注入载荷都必须被转义成**字面量**（AC-S-104 步骤 9）。
    #[test]
    fn injection_payloads_become_literals() {
        let settings = LdapSettings {
            enabled: true,
            url: "ldap://localhost:389".to_string(),
            base_dn: "dc=example,dc=com".to_string(),
            user_search_filter: "(uid={username})".to_string(),
            mail_attribute: "mail".to_string(),
        };
        for (payload, expected) in [
            ("*", "(uid=\\2a)"),
            (") (uid=*", "(uid=\\29 \\28uid=\\2a)"),
            ("admin*", "(uid=admin\\2a)"),
        ] {
            let filter = build_user_filter(&settings, payload);
            assert_eq!(filter, expected, "载荷 {payload} 未被正确转义");
            // 转义后的形态里不会再有裸 `*` / 裸 `(` —— 那才是「绕过匹配」的载体。
            assert!(
                !filter.contains('*'),
                "转义后仍有裸 *（注入未消除）：{filter}"
            );
            assert!(
                filter.matches('(').count() == 1 && filter.matches(')').count() == 1,
                "转义后仍有多余的括号（注入未消除）：{filter}"
            );
        }
    }

    #[test]
    fn empty_and_whitespace_passwords_are_rejected() {
        assert!(is_empty_password(""));
        assert!(is_empty_password("   "));
        assert!(is_empty_password("\t\n"));
        assert!(!is_empty_password("x"));
    }

    #[test]
    fn entry_attribute_lookup_is_case_insensitive() {
        let mut attributes = BTreeMap::new();
        attributes.insert("mail".to_string(), vec!["erin@example.com".to_string()]);
        let entry = LdapEntry::new("uid=erin,ou=people,dc=example,dc=com", attributes);
        assert_eq!(entry.first("MAIL"), Some("erin@example.com"));
        assert_eq!(entry.first("mail"), Some("erin@example.com"));
        assert_eq!(entry.first("none"), None);
    }

    // -----------------------------------------------------------------------
    // P16（D128 / Q4）：`rc → 认证成功`的映射必须可机械验证
    //
    // 这一组是「任意密码都能登录」（S18）的**唯一**防线。若有人把它改回
    // `simple_bind(..).await.is_ok()`（外层 Ok 即成功），下面第 2、3 条会红。
    // -----------------------------------------------------------------------

    /// `rc = 0` → 成功；`rc != 0` → **失败**（49 = invalidCredentials、32 = noSuchObject）。
    #[test]
    fn bind_succeeded_only_accepts_rc_zero() {
        assert!(bind_succeeded(0), "rc=0 是认证成功");
        assert!(!bind_succeeded(49), "rc=49（invalidCredentials）必须是失败");
        assert!(!bind_succeeded(32), "rc=32（noSuchObject）必须是失败");
        assert!(!bind_succeeded(1), "rc=1（operationsError）必须是失败");
        assert!(
            !bind_succeeded(u32::MAX),
            "任何非 0 码都必须是失败 —— 这是「任意密码都能登录」的防线"
        );
    }

    /// **对齐 ldap3 的真实语义**：本函数的判定必须与 `LdapResult::success()` 逐值一致。
    ///
    /// 这条不依赖网络：用构造的 `ldap3::LdapResult` 直接驱动。
    /// 它守的是「我们自己的判据」与「库的判据」不会各自漂移。
    #[test]
    fn bind_succeeded_agrees_with_ldap3_success_semantics() {
        let result_with = |rc: u32| ldap3::LdapResult {
            rc,
            matched: String::new(),
            text: String::new(),
            refs: Vec::new(),
            ctrls: Vec::new(),
        };

        for rc in [0u32, 1, 32, 48, 49, 50, 51, 53] {
            let expected = result_with(rc).success().is_ok();
            assert_eq!(
                bind_succeeded(rc),
                expected,
                "rc={rc}：bind_succeeded 与 ldap3::LdapResult::success() 不一致"
            );
        }

        // 且「成功」的那一侧**恰好只有 rc=0**（不是「rc 小就算成功」）。
        let successes: Vec<u32> = (0u32..=60).filter(|rc| bind_succeeded(*rc)).collect();
        assert_eq!(successes, vec![0], "只有 rc=0 才算认证成功");
    }

    /// **P16 选项 (B)**：用构造的 `ldap3::LdapResult { rc: 49, .. }` 驱动生产用的判定函数，
    /// 断言 `false`；`rc = 0` 断言 `true`。
    ///
    /// 这条守的是判据的**函数体**；「生产有没有调用它」由 `tests/sso_ldap.rs` 守 ——
    /// 那里注入的 `RecordingLdap` 返回 `Ok(LdapResult { rc: 49, .. })`，
    /// 流到 [`authenticate`] 的判定行上，把该行改成恒成功就会红（D129）。
    ///
    /// 会红的情形：`bind_result` 改成 `true` / 改成 `result.success().is_ok() || true`。
    #[test]
    fn a_real_ldap_result_with_rc_49_is_a_failed_bind() {
        let result = |rc: u32| ldap3::LdapResult {
            rc,
            matched: String::new(),
            text: String::new(),
            refs: Vec::new(),
            ctrls: Vec::new(),
        };

        assert!(
            bind_result(&result(0)),
            "rc=0 的 LdapResult 必须判定为认证成功"
        );
        assert!(
            !bind_result(&result(49)),
            "rc=49（invalidCredentials）必须判定为认证失败 —— 否则任意密码都能登录"
        );
        assert!(
            !bind_result(&result(32)),
            "rc=32（noSuchObject）必须判定为认证失败"
        );
        // 明确解码一句：`LdapResult` 本身**不是**成功/失败的载体，
        // 外层 `Ok(...)` 与认证成败无关。
        assert!(
            result(49).success().is_err(),
            "前置：ldap3 的 success() 在 rc=49 时返回 Err（本判据的来源语义）"
        );
    }
}
