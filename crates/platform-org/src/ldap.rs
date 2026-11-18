//! LDAP / AD **入站**同步（第三方 → 底座）。
//!
//! # 属性映射是**外置配置**（D7）
//!
//! AD 与 OpenLDAP 的属性差异很大，所以「哪个属性映射到哪个字段」**不写死在代码里**，
//! 而是每个源一份 JSON 配置（[`LdapSourceConfig`]）。
//!
//! # 离职检测用**位运算表达式**，不是布尔比较（D7 / R2）
//!
//! AD 的 `userAccountControl` 是一组**位标志**：
//!
//! | 位 | 含义 |
//! |----|------|
//! | 2 | `ACCOUNTDISABLE`（账号已禁用） |
//! | 16 | `HOMEDIR_REQUIRED` |
//! | 32 | `LOCKOUT` |
//! | 512 | `NORMAL_ACCOUNT` |
//!
//! 「禁用」是**第 2 位为 1**，而不是「值等于某个常量」——
//! `546`（512+32+2，禁用且锁定）与 `660`（512+128+16+4，正常账号）都说明了这一点。
//! 写成 `value == 514` 这种布尔比较，漏掉任何一个组合就会漏判离职。
//!
//! # 明文 LDAP 与 HTTPS 的边界
//!
//! 本期 **LDAP 只支持明文 `ldap://`**：`ldap3` 的 TLS 特性会拉进
//! `native-tls`（需要 OpenSSL 开发包）或 `rustls`（带 `ring` / `aws-lc-rs`）——
//! 两者都超出本期依赖预算。`ldaps://` / StartTLS 留后续需求。

use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;

use serde::{Deserialize, Serialize};

/// 装箱的 future（让 trait 保持**可对象化**）。
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// LDAP 同步的错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LdapError {
    /// 连接失败。
    Connection(String),
    /// 绑定失败（凭据错误或权限不足）。
    Bind(String),
    /// 查询失败。
    Query(String),
    /// 配置不合法。
    Config(String),
    /// 目录条目缺少必填属性。
    MissingAttribute {
        /// 属性名。
        attribute: &'static str,
        /// 条目 DN。
        dn: String,
    },
}

impl std::fmt::Display for LdapError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Connection(reason) => write!(f, "ldap connection failed: {reason}"),
            Self::Bind(reason) => write!(f, "ldap bind failed: {reason}"),
            Self::Query(reason) => write!(f, "ldap query failed: {reason}"),
            Self::Config(reason) => write!(f, "ldap configuration is invalid: {reason}"),
            Self::MissingAttribute { attribute, dn } => {
                write!(f, "the ldap entry {dn} has no `{attribute}` attribute")
            }
        }
    }
}

impl std::error::Error for LdapError {}

/// 一条 LDAP 条目（属性名**统一小写** —— LDAP 的属性名大小写不敏感）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LdapEntry {
    /// 条目的 DN。
    pub dn: String,
    /// 属性表（键为小写属性名）。
    pub attributes: BTreeMap<String, Vec<String>>,
}

impl LdapEntry {
    /// 造一条条目（属性名自动转小写）。
    pub fn new(dn: impl Into<String>, attributes: &[(&str, Vec<&str>)]) -> Self {
        let mut map = BTreeMap::new();
        for (name, values) in attributes {
            let values: Vec<String> = values.iter().map(|value| (*value).to_string()).collect();
            map.insert(name.to_ascii_lowercase(), values);
        }
        Self {
            dn: dn.into(),
            attributes: map,
        }
    }

    /// 读属性的第一个值。
    pub fn first(&self, attribute: &str) -> Option<&str> {
        self.attributes
            .get(&attribute.to_ascii_lowercase())
            .and_then(|values| values.first())
            .map(String::as_str)
    }

    /// 读属性的整数值（十进制或 `0x` 十六进制）。
    pub fn first_int(&self, attribute: &str) -> Option<i64> {
        let raw = self.first(attribute)?.trim();
        if let Some(hex) = raw.strip_prefix("0x").or_else(|| raw.strip_prefix("0X")) {
            return i64::from_str_radix(hex, 16).ok();
        }
        raw.parse::<i64>().ok()
    }
}

/// 属性取值的**表达式**。
///
/// 只有位运算与直取两种 —— 这两类覆盖了 AD / OpenLDAP 的实际差异。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum AttrExpression {
    /// 位与：`value & mask != 0` → `"true"`；否则 `"false"`。
    ///
    /// AD 的 `userAccountControl` 判定就靠它。
    #[serde(rename = "bit_and")]
    BitAnd {
        /// 属性名。
        attr: String,
        /// 掩码。
        mask: i64,
    },
    /// 位非：`value & mask == 0` → `"true"`；否则 `"false"`。
    #[serde(rename = "bit_not")]
    BitNot {
        /// 属性名。
        attr: String,
        /// 掩码。
        mask: i64,
    },
}

/// 一个属性的取值方式：直取属性值，或按表达式求值。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AttrSpec {
    /// 直取该属性的第一个值。
    Direct(String),
    /// 按表达式求值。
    Expression(AttrExpression),
}

/// LDAP 源的映射配置（**外置**，每个源一份）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LdapSourceConfig {
    /// 源标识（写进 `external_identity.provider` 与 `sync_conflict.source`）。
    pub source: String,
    /// 检索根（Base DN）。
    pub base_dn: String,
    /// 检索过滤器。
    pub filter: String,
    /// 字段 → 属性取值方式的映射。
    pub attributes: BTreeMap<String, AttrSpec>,
    /// 增量同步时用的「修改时间」属性名（默认 `modifyTimestamp`）。
    #[serde(default = "default_modify_attribute")]
    pub modify_attribute: String,
}

fn default_modify_attribute() -> String {
    "modifyTimestamp".to_string()
}

/// 映射后的远端用户（字段级比对的输入）。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RemoteUser {
    /// 外部 ID（AD 的 `objectGUID`、OpenLDAP 的 `entryUUID`）。
    pub external_id: String,
    /// 登录名。
    pub username: String,
    /// 邮箱。
    pub email: Option<String>,
    /// 显示名。
    pub display_name: Option<String>,
    /// 部门（对应组织节点编码）。
    pub department: Option<String>,
    /// 是否已离职（禁用 / 锁定）。
    pub disabled: bool,
}

impl LdapSourceConfig {
    /// 从 JSON 解析配置。
    pub fn from_json(raw: &str) -> Result<LdapSourceConfig, LdapError> {
        serde_json::from_str(raw).map_err(|err| LdapError::Config(err.to_string()))
    }

    /// 需要的属性清单（去重后返回）。
    pub fn required_attributes(&self) -> Vec<String> {
        let mut names: Vec<String> = Vec::new();
        for spec in self.attributes.values() {
            let name = match spec {
                AttrSpec::Direct(name) => name.clone(),
                AttrSpec::Expression(AttrExpression::BitAnd { attr, .. })
                | AttrSpec::Expression(AttrExpression::BitNot { attr, .. }) => attr.clone(),
            };
            if !names.contains(&name) {
                names.push(name);
            }
        }
        names.sort();
        names
    }

    /// 把一条目录条目映射成远端用户。
    pub fn map_entry(&self, entry: &LdapEntry) -> Result<RemoteUser, LdapError> {
        let external_id =
            self.value_of("external_id", entry)
                .ok_or_else(|| LdapError::MissingAttribute {
                    attribute: "external_id",
                    dn: entry.dn.clone(),
                })?;
        let username =
            self.value_of("username", entry)
                .ok_or_else(|| LdapError::MissingAttribute {
                    attribute: "username",
                    dn: entry.dn.clone(),
                })?;

        let disabled = match self.value_of("disabled", entry) {
            Some(raw) => parse_bool(&raw).unwrap_or(false),
            None => false,
        };

        Ok(RemoteUser {
            external_id,
            username,
            email: self.value_of("email", entry),
            display_name: self.value_of("display_name", entry),
            department: self.value_of("department", entry),
            disabled,
        })
    }

    /// 按映射配置取一个字段的值。
    fn value_of(&self, field: &str, entry: &LdapEntry) -> Option<String> {
        let spec = self.attributes.get(field)?;
        match spec {
            AttrSpec::Direct(name) => entry.first(name).map(str::to_string),
            AttrSpec::Expression(AttrExpression::BitAnd { attr, mask }) => {
                let value = entry.first_int(attr)?;
                Some(if value & mask != 0 { "true" } else { "false" }.to_string())
            }
            AttrSpec::Expression(AttrExpression::BitNot { attr, mask }) => {
                let value = entry.first_int(attr)?;
                Some(if value & mask == 0 { "true" } else { "false" }.to_string())
            }
        }
    }

    /// 增量同步的过滤器：基线过滤器 ∧ 「修改时间晚于上次同步点」。
    pub fn incremental_filter(&self, since: Option<time::OffsetDateTime>) -> String {
        match since {
            Some(moment) => format!(
                "(&{base}({attr}>={stamp}))",
                base = self.filter,
                attr = self.modify_attribute,
                stamp = format_ldap_timestamp(moment)
            ),
            None => self.filter.clone(),
        }
    }
}

/// 把布尔值文本解析成布尔（认 `true` / `false` / `1` / `0`）。
fn parse_bool(raw: &str) -> Option<bool> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" => Some(true),
        "false" | "0" | "no" => Some(false),
        _ => None,
    }
}

/// 把时间点格式化成 LDAP 的 GeneralizedTime（UTC）。
///
/// 格式：`YYYYMMDDHHMMSS.0Z` —— AD 的 `modifyTimestamp` 就是这个形状。
pub fn format_ldap_timestamp(moment: time::OffsetDateTime) -> String {
    let utc = moment.to_offset(time::UtcOffset::UTC);
    format!(
        "{:04}{:02}{:02}{:02}{:02}{:02}.0Z",
        utc.year(),
        u8::from(utc.month()),
        utc.day(),
        utc.hour(),
        utc.minute(),
        utc.second()
    )
}

/// LDAP 源的抽象（可测的接缝）。
///
/// 生产实现是 [`Ldap3Source`]；测试用 [`InMemoryLdapSource`]。
pub trait LdapSource: Send + Sync {
    /// 检索条目。
    ///
    /// - `base_dn`：检索根；
    /// - `filter`：检索过滤器；
    /// - `attributes`：需要的属性；
    /// - `since`：增量同步点（`None` 表示全量）。
    fn search(
        &self,
        base_dn: &str,
        filter: &str,
        attributes: &[String],
        since: Option<time::OffsetDateTime>,
    ) -> BoxFuture<'_, Result<Vec<LdapEntry>, LdapError>>;
}

/// 内存 LDAP 源（**内置 mock**，用于测试与本地演练）。
///
/// 它按 `filter` 做最朴素的匹配：包含 `modifyTimestamp>=` 时按时间戳过滤，
/// 否则返回全部条目。
#[derive(Debug, Clone, Default)]
pub struct InMemoryLdapSource {
    entries: Vec<LdapEntry>,
}

impl InMemoryLdapSource {
    /// 造一个内存源。
    pub fn new(entries: Vec<LdapEntry>) -> Self {
        Self { entries }
    }

    /// 追加一条条目。
    pub fn push(&mut self, entry: LdapEntry) {
        self.entries.push(entry);
    }

    /// 条目数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 从过滤器里抽出 `modifyTimestamp>=` 的时间戳。
    fn since_of(filter: &str) -> Option<time::OffsetDateTime> {
        let marker = "modifyTimestamp>=";
        let index = filter.find(marker)?;
        let rest = &filter[index + marker.len()..];
        let end = rest.find([')', ' ']).unwrap_or(rest.len());
        let stamp = &rest[..end];
        parse_ldap_timestamp(stamp)
    }
}

impl LdapSource for InMemoryLdapSource {
    fn search(
        &self,
        _base_dn: &str,
        filter: &str,
        _attributes: &[String],
        since: Option<time::OffsetDateTime>,
    ) -> BoxFuture<'_, Result<Vec<LdapEntry>, LdapError>> {
        let since = since.or_else(|| InMemoryLdapSource::since_of(filter));
        let entries = self.entries.clone();
        Box::pin(async move {
            let Some(since) = since else {
                return Ok(entries);
            };
            let mut kept = Vec::new();
            for entry in entries {
                let modified = entry
                    .first("modifytimestamp")
                    .and_then(parse_ldap_timestamp);
                match modified {
                    // 没有时间戳的条目：**证明不了它旧**，保守保留 ——
                    // 增量同步漏条目比多处理条目危险得多。
                    None => kept.push(entry),
                    Some(moment) if moment >= since => kept.push(entry),
                    Some(_) => continue,
                }
            }
            Ok(kept)
        })
    }
}

/// 解析 LDAP GeneralizedTime（`YYYYMMDDHHMMSS.0Z`）。
pub fn parse_ldap_timestamp(raw: &str) -> Option<time::OffsetDateTime> {
    let raw = raw.trim();
    if raw.len() < 15 {
        return None;
    }
    let year: i32 = raw[0..4].parse().ok()?;
    let month: u8 = raw[4..6].parse().ok()?;
    let day: u8 = raw[6..8].parse().ok()?;
    let hour: u8 = raw[8..10].parse().ok()?;
    let minute: u8 = raw[10..12].parse().ok()?;
    let second: u8 = raw[12..14].parse().ok()?;
    let date =
        time::Date::from_calendar_date(year, time::Month::try_from(month).ok()?, day).ok()?;
    let time_of_day = time::Time::from_hms(hour, minute, second).ok()?;
    Some(time::OffsetDateTime::new_utc(date, time_of_day))
}

/// AD / OpenLDAP 的真实连接（`ldap3`）。
///
/// ⚠️ 本期只支持**明文** `ldap://`（见模块文档的 TLS 边界说明）。
#[derive(Debug, Clone)]
pub struct Ldap3Source {
    /// 连接地址（如 `ldap://127.0.0.1:389`）。
    pub url: String,
    /// 绑定 DN（匿名绑定时为 `None`）。
    pub bind_dn: Option<String>,
    /// 绑定口令。
    pub bind_password: Option<String>,
}

impl Ldap3Source {
    /// 造一个连接配置。
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            bind_dn: None,
            bind_password: None,
        }
    }

    /// 设置绑定凭据。
    pub fn with_credentials(mut self, dn: impl Into<String>, password: impl Into<String>) -> Self {
        self.bind_dn = Some(dn.into());
        self.bind_password = Some(password.into());
        self
    }

    /// 连接并绑定，返回可用的 `Ldap` 客户端。
    async fn connect(&self) -> Result<ldap3::Ldap, LdapError> {
        let (conn, ldap) = ldap3::LdapConnAsync::new(&self.url)
            .await
            .map_err(|err| LdapError::Connection(err.to_string()))?;
        // `drive!` 把连接的后台读取循环放进独立任务。
        ldap3::drive!(conn);
        let mut ldap = ldap;
        let bound = match (&self.bind_dn, &self.bind_password) {
            (Some(dn), Some(password)) => ldap.simple_bind(dn, password).await,
            _ => ldap.simple_bind("", "").await,
        }
        .map_err(|err| LdapError::Bind(err.to_string()))?;
        // `success()` 校验 LDAP 的结果码（0 = 成功）。
        bound
            .success()
            .map_err(|err| LdapError::Bind(err.to_string()))?;
        Ok(ldap)
    }
}

impl LdapSource for Ldap3Source {
    fn search(
        &self,
        base_dn: &str,
        filter: &str,
        attributes: &[String],
        _since: Option<time::OffsetDateTime>,
    ) -> BoxFuture<'_, Result<Vec<LdapEntry>, LdapError>> {
        let url = self.url.clone();
        let bind_dn = self.bind_dn.clone();
        let bind_password = self.bind_password.clone();
        let base_dn = base_dn.to_string();
        let filter = filter.to_string();
        let attributes = attributes.to_vec();
        Box::pin(async move {
            let source = Ldap3Source {
                url,
                bind_dn,
                bind_password,
            };
            let mut ldap = source.connect().await?;
            let requested: Vec<&str> = attributes.iter().map(String::as_str).collect();
            let (entries, _result) = ldap
                .search(&base_dn, ldap3::Scope::Subtree, &filter, &requested)
                .await
                .map_err(|err| LdapError::Query(err.to_string()))?
                .success()
                .map_err(|err| LdapError::Query(err.to_string()))?;
            let entries = entries
                .into_iter()
                .map(ldap3::SearchEntry::construct)
                .map(search_entry_to_ldap_entry)
                .collect();
            let _ = ldap.unbind().await;
            Ok(entries)
        })
    }
}

/// `ldap3::SearchEntry` → [`LdapEntry`]（属性名转小写 —— LDAP 的属性名大小写不敏感）。
pub fn search_entry_to_ldap_entry(entry: ldap3::SearchEntry) -> LdapEntry {
    let mut attributes = BTreeMap::new();
    for (name, values) in entry.attrs {
        attributes.insert(name.to_ascii_lowercase(), values);
    }
    LdapEntry {
        dn: entry.dn,
        attributes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const AD_CONFIG: &str = r#"{
      "source": "corp-ad",
      "baseDn": "DC=corp,DC=example,DC=com",
      "filter": "(objectClass=user)",
      "attributes": {
        "external_id": "objectGUID",
        "username": "sAMAccountName",
        "email": "mail",
        "display_name": "displayName",
        "department": "department",
        "disabled": { "op": "bit_and", "attr": "userAccountControl", "mask": 2 }
      }
    }"#;

    fn ad_entry(uac: &str) -> LdapEntry {
        LdapEntry::new(
            "CN=alice,DC=corp,DC=example,DC=com",
            &[
                ("objectGUID", vec!["guid-alice"]),
                ("sAMAccountName", vec!["alice"]),
                ("mail", vec!["alice@example.com"]),
                ("displayName", vec!["Alice Example"]),
                ("department", vec!["eng"]),
                ("userAccountControl", vec![uac]),
            ],
        )
    }

    #[test]
    fn the_mapping_config_is_external_and_parseable() {
        let config = LdapSourceConfig::from_json(AD_CONFIG).expect("配置应能解析");
        assert_eq!(config.source, "corp-ad");
        assert_eq!(config.base_dn, "DC=corp,DC=example,DC=com");
        assert_eq!(config.filter, "(objectClass=user)");
        assert!(
            config
                .required_attributes()
                .contains(&"userAccountControl".to_string())
        );
    }

    /// AC-S-016：AD 的离职判定是**位运算**，不是布尔比较。
    #[test]
    fn the_ad_disabled_flag_is_decided_by_bitwise_operations() {
        let config = LdapSourceConfig::from_json(AD_CONFIG).expect("配置应能解析");

        // 512 = NORMAL_ACCOUNT（启用）
        assert!(
            !config
                .map_entry(&ad_entry("512"))
                .expect("应能映射")
                .disabled
        );
        // 514 = NORMAL_ACCOUNT | ACCOUNTDISABLE（禁用）
        assert!(
            config
                .map_entry(&ad_entry("514"))
                .expect("应能映射")
                .disabled
        );
        // 546 = 512 | 32(LOCKOUT) | 2(ACCOUNTDISABLE)（禁用且锁定）
        assert!(
            config
                .map_entry(&ad_entry("546"))
                .expect("应能映射")
                .disabled
        );
        // 4098 = 4096 | 2（禁用）
        assert!(
            config
                .map_entry(&ad_entry("4098"))
                .expect("应能映射")
                .disabled
        );
        // 660 = 512 | 128 | 16 | 4（正常账号，**不是** 512 —— 布尔比较会误判）
        assert!(
            !config
                .map_entry(&ad_entry("660"))
                .expect("应能映射")
                .disabled
        );
        // 402653184 = 512 | 4194304(DONT_EXPIRE_PASSWORD)（启用）
        assert!(
            !config
                .map_entry(&ad_entry("402653184"))
                .expect("应能映射")
                .disabled
        );
        // 2 = 只有 ACCOUNTDISABLE 位
        assert!(config.map_entry(&ad_entry("2")).expect("应能映射").disabled);
    }

    #[test]
    fn the_mapping_produces_the_expected_remote_user() {
        let config = LdapSourceConfig::from_json(AD_CONFIG).expect("配置应能解析");
        let remote = config.map_entry(&ad_entry("514")).expect("应能映射");
        assert_eq!(remote.external_id, "guid-alice");
        assert_eq!(remote.username, "alice");
        assert_eq!(remote.email.as_deref(), Some("alice@example.com"));
        assert_eq!(remote.display_name.as_deref(), Some("Alice Example"));
        assert_eq!(remote.department.as_deref(), Some("eng"));
        assert!(remote.disabled);
    }

    #[test]
    fn a_missing_mandatory_attribute_is_reported() {
        let config = LdapSourceConfig::from_json(AD_CONFIG).expect("配置应能解析");
        let entry = LdapEntry::new("CN=x", &[("sAMAccountName", vec!["x"])]);
        let error = config.map_entry(&entry).expect_err("缺 external_id 应报错");
        assert!(matches!(error, LdapError::MissingAttribute { .. }));
    }

    #[test]
    fn the_incremental_filter_carries_the_modify_timestamp() {
        let config = LdapSourceConfig::from_json(AD_CONFIG).expect("配置应能解析");
        let moment = time::OffsetDateTime::new_utc(
            time::Date::from_calendar_date(2026, time::Month::November, 18).expect("日期应合法"),
            time::Time::from_hms(3, 4, 5).expect("时间应合法"),
        );
        let filter = config.incremental_filter(Some(moment));
        assert!(
            filter.contains("modifyTimestamp>=20261118030405.0Z"),
            "{filter}"
        );
        assert!(filter.starts_with("(&(objectClass=user)"), "{filter}");
        assert_eq!(config.incremental_filter(None), "(objectClass=user)");
    }

    #[test]
    fn ldap_timestamps_round_trip() {
        let moment = time::OffsetDateTime::new_utc(
            time::Date::from_calendar_date(2026, time::Month::November, 18).expect("日期应合法"),
            time::Time::from_hms(3, 4, 5).expect("时间应合法"),
        );
        let stamp = format_ldap_timestamp(moment);
        assert_eq!(stamp, "20261118030405.0Z");
        assert_eq!(parse_ldap_timestamp(&stamp), Some(moment));
    }

    #[tokio::test]
    async fn the_in_memory_source_filters_by_modify_timestamp() {
        let source = InMemoryLdapSource::new(vec![
            LdapEntry::new(
                "CN=old",
                &[
                    ("modifyTimestamp", vec!["20260101000000.0Z"]),
                    ("x", vec!["1"]),
                ],
            ),
            LdapEntry::new(
                "CN=new",
                &[
                    ("modifyTimestamp", vec!["20261118000000.0Z"]),
                    ("x", vec!["2"]),
                ],
            ),
        ]);
        assert_eq!(source.len(), 2);

        let all = source
            .search("dc=x", "(objectClass=user)", &[], None)
            .await
            .expect("检索应成功");
        assert_eq!(all.len(), 2);

        let since = parse_ldap_timestamp("20261101000000.0Z").expect("时间戳应能解析");
        let recent = source
            .search("dc=x", "(objectClass=user)", &[], Some(since))
            .await
            .expect("检索应成功");
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].dn, "CN=new");
    }

    #[tokio::test]
    async fn the_in_memory_source_reads_the_since_point_from_the_filter() {
        let source = InMemoryLdapSource::new(vec![LdapEntry::new(
            "CN=new",
            &[("modifyTimestamp", vec!["20261118000000.0Z"])],
        )]);
        let filter = "(&(objectClass=user)(modifyTimestamp>=20261101000000.0Z))";
        let entries = source
            .search("dc=x", filter, &[], None)
            .await
            .expect("检索应成功");
        assert_eq!(entries.len(), 1);
    }

    #[test]
    fn bit_not_expresses_the_inverted_flag() {
        let config = LdapSourceConfig::from_json(
            r#"{
              "source": "s",
              "baseDn": "dc=x",
              "filter": "(objectClass=user)",
              "attributes": {
                "external_id": "objectGUID",
                "username": "uid",
                "disabled": { "op": "bit_not", "attr": "userAccountControl", "mask": 2 }
              }
            }"#,
        )
        .expect("配置应能解析");
        let enabled = LdapEntry::new(
            "CN=a",
            &[
                ("objectGUID", vec!["g"]),
                ("uid", vec!["a"]),
                ("userAccountControl", vec!["512"]),
            ],
        );
        let disabled = LdapEntry::new(
            "CN=b",
            &[
                ("objectGUID", vec!["g"]),
                ("uid", vec!["b"]),
                ("userAccountControl", vec!["514"]),
            ],
        );
        // `bit_not` 的语义是「被掩码的位**没有**置上」——
        // 所以用它表达 `disabled` 时，读数是**反的**（禁用位没置上 → 取值为 true）。
        // 判定「账号是否禁用」必须用 `bit_and`（见上面的 AD 用例）。
        assert!(config.map_entry(&enabled).expect("应能映射").disabled);
        assert!(!config.map_entry(&disabled).expect("应能映射").disabled);
    }

    #[test]
    fn hexadecimal_attribute_values_are_understood() {
        let entry = LdapEntry::new("CN=a", &[("userAccountControl", vec!["0x202"])]);
        assert_eq!(entry.first_int("userAccountControl"), Some(0x202));
    }
}
