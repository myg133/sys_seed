//! SCIM 2.0 **出站**同步（底座 → 第三方）。
//!
//! # 为什么自己实现，不引 SCIM 库（D6）
//!
//! SCIM 2.0 的 User / Group 资源 schema 不复杂，核心就是 JSON + HTTP。
//! Rust 生态的 SCIM 库成熟度一般，且会引入不确定的依赖。
//! 这里用**树里已有的** HTTP 栈（`hyper` / `hyper-util` / `http`）实现，
//! **不引 `reqwest`**，依赖增量为 0。
//!
//! # 幂等（D6）
//!
//! - 幂等键：`(provider, external_id)`，记在 `external_identity` 表里；
//! - 推送用 **PUT 语义**（幂等）：同一个 id 推两次，目标端只有一条记录。
//!
//! # 推送必须**异步**（D6 / R3）
//!
//! 不能在用户请求里同步调第三方 —— 第三方挂了不能拖垮本地。
//! 本地操作只把推送任务**入队**（[`ScimOutbox`]）就返回；
//! 后台 worker 再慢慢推。失败进重试（指数退避），超过上限记进同步状态。
//!
//! # HTTPS 的边界
//!
//! 本期**只支持 `http://`**：树里目前没有任何 TLS 栈
//! （无 `rustls` / `native-tls`），引入 TLS 会带来 30~50 个 crate，
//! 且 `rustls` 的默认后端会拉进 `ring` / `aws-lc-rs` —— 超出本期依赖预算。
//! 配置项 [`ScimProviderConfig::tls_mode`] 把口子留好：
//! 填 `platform` 时会明确报「本期未实现」，而不是悄悄降级成明文。

use std::collections::{BTreeMap, VecDeque};
use std::sync::Mutex;
use std::time::Duration;

use http_body_util::BodyExt;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// User 资源的 schema URI。
pub const SCIM_USER_SCHEMA: &str = "urn:ietf:params:scim:schemas:core:2.0:User";
/// Group 资源的 schema URI。
pub const SCIM_GROUP_SCHEMA: &str = "urn:ietf:params:scim:schemas:core:2.0:Group";
/// SCIM 的内容类型。
pub const SCIM_CONTENT_TYPE: &str = "application/scim+json";

/// 推送失败的最大重试次数。
pub const MAX_PUSH_ATTEMPTS: u32 = 3;

/// SCIM 出站的错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScimError {
    /// 目标不可达 / 返回了错误状态码。
    Transport(String),
    /// 目标返回的响应体不符合 SCIM。
    Protocol(String),
    /// 配置不合法。
    Config(String),
    /// 本期不实现的能力（如 HTTPS）。
    NotImplemented(String),
}

impl std::fmt::Display for ScimError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Transport(reason) => write!(f, "the scim target is unreachable: {reason}"),
            Self::Protocol(reason) => write!(f, "the scim response is invalid: {reason}"),
            Self::Config(reason) => write!(f, "the scim configuration is invalid: {reason}"),
            Self::NotImplemented(feature) => write!(f, "{feature} is not implemented"),
        }
    }
}

impl std::error::Error for ScimError {}

/// TLS 模式。
///
/// 本期只有 [`TlsMode::Disabled`]（明文 `http://`）可用。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TlsMode {
    /// 明文 HTTP。
    #[default]
    Disabled,
    /// 走平台 TLS 栈 —— **本期未实现**（树里没有 TLS 依赖）。
    Platform,
}

/// 邮箱字段的取值形态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EmailShape {
    /// 多值数组（SCIM 标准：`emails: [{value, primary}]`）。
    #[default]
    Multi,
    /// 单值（`"workEmail": "a@b.c"`）。
    Single,
}

/// SCIM 出站的用户字段映射。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ScimUserFields {
    /// 用户名字段名（默认 `userName`）。
    #[serde(default = "default_username_field")]
    pub username: String,
    /// 邮箱字段名（默认 `emails`）。
    #[serde(default = "default_email_field")]
    pub email: String,
    /// 显示名字段名（默认 `displayName`）。
    #[serde(default = "default_display_name_field")]
    pub display_name: String,
    /// 邮箱字段的形态（默认多值数组）。
    #[serde(default)]
    pub email_shape: EmailShape,
}

fn default_username_field() -> String {
    "userName".to_string()
}

fn default_email_field() -> String {
    "emails".to_string()
}

fn default_display_name_field() -> String {
    "displayName".to_string()
}

impl Default for ScimUserFields {
    fn default() -> Self {
        Self {
            username: default_username_field(),
            email: default_email_field(),
            display_name: default_display_name_field(),
            email_shape: EmailShape::default(),
        }
    }
}

/// SCIM 出站的 Group 字段映射。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ScimGroupFields {
    /// 显示名字段名（默认 `displayName`）。
    #[serde(default = "default_display_name_field")]
    pub display_name: String,
}

impl Default for ScimGroupFields {
    fn default() -> Self {
        Self {
            display_name: default_display_name_field(),
        }
    }
}

/// 一个 SCIM provider 的配置（**外置**，每个 provider 一份）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ScimProviderConfig {
    /// provider 标识（写进 `external_identity.provider`）。
    pub provider: String,
    /// 目标地址（如 `http://scim.internal/scim/v2`）。
    pub base_url: String,
    /// 认证头（整行，如 `Authorization: Bearer xxx`）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_header: Option<String>,
    /// TLS 模式（本期只支持 `disabled`）。
    #[serde(default)]
    pub tls_mode: TlsMode,
    /// 用户字段映射。
    #[serde(default)]
    pub user_fields: ScimUserFields,
    /// Group 字段映射。
    #[serde(default)]
    pub group_fields: ScimGroupFields,
}

impl ScimProviderConfig {
    /// 造一个最小配置。
    pub fn new(provider: impl Into<String>, base_url: impl Into<String>) -> Self {
        Self {
            provider: provider.into(),
            base_url: base_url.into(),
            auth_header: None,
            tls_mode: TlsMode::default(),
            user_fields: ScimUserFields::default(),
            group_fields: ScimGroupFields::default(),
        }
    }

    /// 从 JSON 解析配置。
    pub fn from_json(raw: &str) -> Result<ScimProviderConfig, ScimError> {
        serde_json::from_str(raw).map_err(|err| ScimError::Config(err.to_string()))
    }

    /// 校验配置（本期唯一的硬约束：只支持明文 HTTP）。
    pub fn validate(&self) -> Result<(), ScimError> {
        if self.base_url.starts_with("https://") {
            return match self.tls_mode {
                TlsMode::Platform => Err(ScimError::NotImplemented(
                    "https scim push (the platform tls stack is not in this release)".to_string(),
                )),
                TlsMode::Disabled => Err(ScimError::Config(
                    "the scim base url uses https but the tls mode is disabled".to_string(),
                )),
            };
        }
        if !self.base_url.starts_with("http://") {
            return Err(ScimError::Config(
                "the scim base url must start with http:// (https is not implemented yet)"
                    .to_string(),
            ));
        }
        Ok(())
    }

    /// 按映射规则构造 User 的请求体。
    ///
    /// 不同 IdP 的用户名 / 邮箱 / 显示名字段名与位置都不同 ——
    /// 映射配置就是为此存在的（AC-S-013）。
    pub fn user_payload(&self, draft: &ScimUserDraft) -> Value {
        let mut body = Map::new();
        body.insert(
            "schemas".to_string(),
            Value::Array(vec![Value::String(SCIM_USER_SCHEMA.to_string())]),
        );
        body.insert("id".to_string(), Value::String(draft.id.clone()));
        body.insert("externalId".to_string(), Value::String(draft.id.clone()));
        body.insert(
            self.user_fields.username.clone(),
            Value::String(draft.username.clone()),
        );
        if let Some(display_name) = &draft.display_name {
            body.insert(
                self.user_fields.display_name.clone(),
                Value::String(display_name.clone()),
            );
        }
        if let Some(email) = &draft.email {
            match self.user_fields.email_shape {
                EmailShape::Multi => {
                    body.insert(
                        self.user_fields.email.clone(),
                        Value::Array(vec![
                            Map::from_iter([
                                ("value".to_string(), Value::String(email.clone())),
                                ("primary".to_string(), Value::Bool(true)),
                                ("type".to_string(), Value::String("work".to_string())),
                            ])
                            .into(),
                        ]),
                    );
                }
                EmailShape::Single => {
                    body.insert(self.user_fields.email.clone(), Value::String(email.clone()));
                }
            }
        }
        body.insert("active".to_string(), Value::Bool(draft.active));
        if !draft.group_ids.is_empty() {
            body.insert(
                "groups".to_string(),
                Value::Array(
                    draft
                        .group_ids
                        .iter()
                        .map(|id| {
                            Map::from_iter([("value".to_string(), Value::String(id.clone()))])
                                .into()
                        })
                        .collect(),
                ),
            );
        }
        Value::Object(body)
    }

    /// 按映射规则构造 Group 的请求体。
    pub fn group_payload(&self, draft: &ScimGroupDraft) -> Value {
        let mut body = Map::new();
        body.insert(
            "schemas".to_string(),
            Value::Array(vec![Value::String(SCIM_GROUP_SCHEMA.to_string())]),
        );
        body.insert("id".to_string(), Value::String(draft.id.clone()));
        body.insert("externalId".to_string(), Value::String(draft.id.clone()));
        body.insert(
            self.group_fields.display_name.clone(),
            Value::String(draft.display_name.clone()),
        );
        body.insert(
            "members".to_string(),
            Value::Array(
                draft
                    .member_ids
                    .iter()
                    .map(|id| {
                        Map::from_iter([("value".to_string(), Value::String(id.clone()))]).into()
                    })
                    .collect(),
            ),
        );
        Value::Object(body)
    }
}

/// 待推送的 User（本地 → SCIM 的中间形态）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScimUserDraft {
    /// 本地用户 ID（作为 SCIM 的 `id` / `externalId`）。
    pub id: String,
    /// 用户名。
    pub username: String,
    /// 邮箱。
    pub email: Option<String>,
    /// 显示名。
    pub display_name: Option<String>,
    /// 是否启用（离职 → `active: false`）。
    pub active: bool,
    /// 所属 Group（组织节点）的 SCIM ID。
    pub group_ids: Vec<String>,
}

/// 待推送的 Group（组织节点 → SCIM）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScimGroupDraft {
    /// 节点 ID（作为 SCIM 的 `id` / `externalId`）。
    pub id: String,
    /// 显示名。
    pub display_name: String,
    /// 成员（本地用户 ID）。
    pub member_ids: Vec<String>,
}

/// SCIM 目标端返回的一条 User。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ScimUser {
    /// 目标端 ID。
    pub id: String,
    /// 用户名。
    pub user_name: String,
    /// 显示名。
    pub display_name: Option<String>,
    /// 邮箱。
    pub email: Option<String>,
    /// 是否启用。
    pub active: bool,
}

impl ScimUser {
    /// 解析目标端的 User（**容错**：先按标准字段名取，取不到再按映射字段名取）。
    pub fn from_json(value: &Value, config: &ScimProviderConfig) -> ScimUser {
        let user_name = value
            .get(&config.user_fields.username)
            .and_then(Value::as_str)
            .or_else(|| value.get("userName").and_then(Value::as_str))
            .unwrap_or_default()
            .to_string();
        let display_name = value
            .get(&config.user_fields.display_name)
            .and_then(Value::as_str)
            .or_else(|| value.get("displayName").and_then(Value::as_str))
            .map(str::to_string);
        let email = match value.get(&config.user_fields.email) {
            Some(Value::Array(items)) => items
                .first()
                .and_then(|item| item.get("value"))
                .and_then(Value::as_str)
                .map(str::to_string),
            Some(Value::String(single)) => Some(single.clone()),
            _ => None,
        };
        ScimUser {
            id: value
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            user_name,
            display_name,
            email,
            active: value.get("active").and_then(Value::as_bool).unwrap_or(true),
        }
    }
}

/// SCIM 目标端返回的一条 Group。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ScimGroup {
    /// 目标端 ID。
    pub id: String,
    /// 显示名。
    pub display_name: String,
    /// 成员 ID。
    pub member_ids: Vec<String>,
}

/// 一次 SCIM 写操作的响应。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ScimResponse {
    /// 目标端资源 ID。
    pub id: String,
    /// HTTP 状态码。
    pub status: u16,
}

/// SCIM 出站的网关（**可对象化** —— 这样服务层能持有 `Arc<dyn ScimGateway>`）。
///
/// 生产实现是 [`HttpScimGateway`]；测试用内存实现。
pub trait ScimGateway: Send + Sync {
    /// PUT 一个 User（**幂等**）。
    fn put_user<'a>(
        &'a self,
        config: &'a ScimProviderConfig,
        draft: &'a ScimUserDraft,
    ) -> ScimFuture<'a, Result<ScimResponse, ScimError>>;

    /// PUT 一个 Group（**幂等**）。
    fn put_group<'a>(
        &'a self,
        config: &'a ScimProviderConfig,
        draft: &'a ScimGroupDraft,
    ) -> ScimFuture<'a, Result<ScimResponse, ScimError>>;

    /// 查询 Users（`filter` 用 SCIM 的过滤表达式，如 `userName eq "alice"`）。
    fn list_users<'a>(
        &'a self,
        config: &'a ScimProviderConfig,
        filter: &str,
    ) -> ScimFuture<'a, Result<Vec<ScimUser>, ScimError>>;

    /// 查询 Groups。
    fn list_groups<'a>(
        &'a self,
        config: &'a ScimProviderConfig,
        filter: &str,
    ) -> ScimFuture<'a, Result<Vec<ScimGroup>, ScimError>>;

    /// 局部更新用户的 `active`（离职时用）。
    fn patch_user_active<'a>(
        &'a self,
        config: &'a ScimProviderConfig,
        external_id: &str,
        active: bool,
    ) -> ScimFuture<'a, Result<ScimResponse, ScimError>>;
}

/// 装箱的 future（让网关 trait 保持**可对象化**）。
pub type ScimFuture<'a, T> = std::pin::Pin<Box<dyn std::future::Future<Output = T> + Send + 'a>>;

/// 基于 `hyper` 的 SCIM 网关（**明文 HTTP**）。
#[derive(Debug, Clone)]
pub struct HttpScimGateway {
    client: hyper_util::client::legacy::Client<
        hyper_util::client::legacy::connect::HttpConnector,
        http_body_util::Full<bytes::Bytes>,
    >,
}

impl Default for HttpScimGateway {
    fn default() -> Self {
        Self::new()
    }
}

impl HttpScimGateway {
    /// 造一个网关。
    pub fn new() -> Self {
        let client =
            hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new())
                .build_http();
        Self { client }
    }

    /// 发一个请求。
    async fn send(
        &self,
        config: &ScimProviderConfig,
        method: http::Method,
        path: &str,
        body: Option<String>,
    ) -> Result<(u16, String), ScimError> {
        config.validate()?;
        let url = format!("{}{}", config.base_url.trim_end_matches('/'), path);
        let mut builder = http::Request::builder()
            .method(method)
            .uri(&url)
            .header(http::header::CONTENT_TYPE, SCIM_CONTENT_TYPE);
        if let Some(auth) = &config.auth_header {
            if let Some((name, value)) = auth.split_once(':') {
                builder = builder.header(name.trim(), value.trim());
            }
        }
        // 请求体类型固定为 `Full<Bytes>`（GET 时是空体）——
        // `Option<Full<_>>` 不实现 `Body`，hyper 的 client 收不了。
        let payload = body.unwrap_or_default();
        let request = builder
            .body(http_body_util::Full::new(bytes::Bytes::from(payload)))
            .map_err(|err| ScimError::Transport(err.to_string()))?;

        let response = self
            .client
            .request(request)
            .await
            .map_err(|err| ScimError::Transport(err.to_string()))?;
        let status = response.status().as_u16();
        let collected = response
            .into_body()
            .collect()
            .await
            .map_err(|err| ScimError::Transport(err.to_string()))?;
        let text = String::from_utf8_lossy(&collected.to_bytes()).to_string();
        Ok((status, text))
    }

    /// 发一个写请求并解析响应。
    async fn write(
        &self,
        config: &ScimProviderConfig,
        method: http::Method,
        path: &str,
        body: Value,
    ) -> Result<ScimResponse, ScimError> {
        let (status, text) = self
            .send(config, method, path, Some(body.to_string()))
            .await?;
        if !(200..300).contains(&status) {
            return Err(ScimError::Transport(format!(
                "the scim target answered with status {status}: {text}"
            )));
        }
        let id = serde_json::from_str::<Value>(&text)
            .ok()
            .and_then(|value| value.get("id").and_then(Value::as_str).map(str::to_string))
            .unwrap_or_default();
        Ok(ScimResponse { id, status })
    }
}

impl ScimGateway for HttpScimGateway {
    fn put_user<'a>(
        &'a self,
        config: &'a ScimProviderConfig,
        draft: &'a ScimUserDraft,
    ) -> ScimFuture<'a, Result<ScimResponse, ScimError>> {
        let payload = config.user_payload(draft);
        let path = format!("/Users/{}", encode_path_segment(&draft.id));
        Box::pin(async move { self.write(config, http::Method::PUT, &path, payload).await })
    }

    fn put_group<'a>(
        &'a self,
        config: &'a ScimProviderConfig,
        draft: &'a ScimGroupDraft,
    ) -> ScimFuture<'a, Result<ScimResponse, ScimError>> {
        let payload = config.group_payload(draft);
        let path = format!("/Groups/{}", encode_path_segment(&draft.id));
        Box::pin(async move { self.write(config, http::Method::PUT, &path, payload).await })
    }

    fn list_users<'a>(
        &'a self,
        config: &'a ScimProviderConfig,
        filter: &str,
    ) -> ScimFuture<'a, Result<Vec<ScimUser>, ScimError>> {
        let path = format!("/Users?filter={}", encode_query(filter));
        Box::pin(async move {
            let (status, text) = self.send(config, http::Method::GET, &path, None).await?;
            if !(200..300).contains(&status) {
                return Err(ScimError::Transport(format!(
                    "the scim target answered with status {status}: {text}"
                )));
            }
            let value: Value =
                serde_json::from_str(&text).map_err(|err| ScimError::Protocol(err.to_string()))?;
            let resources = value
                .get("Resources")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            Ok(resources
                .iter()
                .map(|item| ScimUser::from_json(item, config))
                .collect())
        })
    }

    fn list_groups<'a>(
        &'a self,
        config: &'a ScimProviderConfig,
        filter: &str,
    ) -> ScimFuture<'a, Result<Vec<ScimGroup>, ScimError>> {
        let path = format!("/Groups?filter={}", encode_query(filter));
        Box::pin(async move {
            let (status, text) = self.send(config, http::Method::GET, &path, None).await?;
            if !(200..300).contains(&status) {
                return Err(ScimError::Transport(format!(
                    "the scim target answered with status {status}: {text}"
                )));
            }
            let value: Value =
                serde_json::from_str(&text).map_err(|err| ScimError::Protocol(err.to_string()))?;
            let resources = value
                .get("Resources")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            Ok(resources
                .iter()
                .map(|item| ScimGroup {
                    id: item
                        .get("id")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    display_name: item
                        .get(&config.group_fields.display_name)
                        .and_then(Value::as_str)
                        .or_else(|| item.get("displayName").and_then(Value::as_str))
                        .unwrap_or_default()
                        .to_string(),
                    member_ids: item
                        .get("members")
                        .and_then(Value::as_array)
                        .map(|members| {
                            members
                                .iter()
                                .filter_map(|member| member.get("value").and_then(Value::as_str))
                                .map(str::to_string)
                                .collect()
                        })
                        .unwrap_or_default(),
                })
                .collect())
        })
    }

    fn patch_user_active<'a>(
        &'a self,
        config: &'a ScimProviderConfig,
        external_id: &str,
        active: bool,
    ) -> ScimFuture<'a, Result<ScimResponse, ScimError>> {
        let path = format!("/Users/{}", encode_path_segment(external_id));
        let body = serde_json::json!({
            "schemas": ["urn:ietf:params:scim:api:messages:2.0:PatchOp"],
            "Operations": [{ "op": "replace", "path": "active", "value": active }],
        });
        Box::pin(async move { self.write(config, http::Method::PATCH, &path, body).await })
    }
}

/// URL 路径段编码。
fn encode_path_segment(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// 查询串编码。
fn encode_query(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            b' ' => out.push_str("%20"),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// 一个待推送的任务。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScimJob {
    /// PUT 一个 User。
    PutUser(ScimUserDraft),
    /// PUT 一个 Group。
    PutGroup(ScimGroupDraft),
    /// 局部更新用户的 `active`。
    PatchActive {
        /// 目标端用户 ID。
        external_id: String,
        /// 是否启用。
        active: bool,
    },
}

/// 入队的任务（含租户、目标 provider 与重试状态）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScimJobEntry {
    /// 归属租户。
    pub tenant_id: String,
    /// 目标 provider 配置。
    pub config: ScimProviderConfig,
    /// 任务内容。
    pub job: ScimJob,
    /// 已尝试次数。
    pub attempt: u32,
    /// 下次可重试的时间（`None` = 立即可试）。
    pub ready_at: Option<time::OffsetDateTime>,
}

/// 一个任务的处理结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScimJobOutcome {
    /// 任务内容。
    pub job: ScimJob,
    /// 是否成功。
    pub ok: bool,
    /// 失败原因（成功时为空）。
    pub error: Option<String>,
    /// 已尝试次数。
    pub attempt: u32,
    /// 是否还会重试。
    pub will_retry: bool,
}

/// 指数退避的重试间隔。
pub fn backoff_delay(attempt: u32) -> Duration {
    let exponent = attempt.saturating_sub(1).min(6);
    Duration::from_millis(100u64 << exponent)
}

/// SCIM 推送的**出站队列**。
///
/// 本地写操作只 `enqueue`（**立即返回**），后台 worker 再推 ——
/// 第三方挂了不会拖垮本地请求（R3）。
#[derive(Debug, Default)]
pub struct ScimOutbox {
    queue: Mutex<VecDeque<ScimJobEntry>>,
}

impl ScimOutbox {
    /// 造一个空队列。
    pub fn new() -> Self {
        Self::default()
    }

    /// 入队一个任务（**立即返回** —— 不等第三方）。
    pub fn enqueue(&self, entry: ScimJobEntry) {
        let mut queue = self.queue.lock().unwrap_or_else(|err| err.into_inner());
        queue.push_back(entry);
    }

    /// 队列里待处理的任务数。
    pub fn pending(&self) -> usize {
        let queue = self.queue.lock().unwrap_or_else(|err| err.into_inner());
        queue.len()
    }

    /// 处理队列里的**全部就绪任务**，返回每个任务的结果。
    ///
    /// 失败的任务按指数退避**重新入队**（不超过 [`MAX_PUSH_ATTEMPTS`] 次）。
    pub async fn drain_once(
        &self,
        gateway: &dyn ScimGateway,
        now: time::OffsetDateTime,
    ) -> Vec<ScimJobOutcome> {
        let mut outcomes = Vec::new();
        while let Some(entry) = self.take_ready(now) {
            let outcome = self.run(gateway, entry, now).await;
            outcomes.push(outcome);
        }
        outcomes
    }

    /// 取出一个就绪的任务（跳过还没到重试时间的）。
    fn take_ready(&self, now: time::OffsetDateTime) -> Option<ScimJobEntry> {
        let mut queue = self.queue.lock().unwrap_or_else(|err| err.into_inner());
        let index = queue.iter().position(|entry| match entry.ready_at {
            None => true,
            Some(ready_at) => ready_at <= now,
        })?;
        queue.remove(index)
    }

    /// 执行一个任务（失败则按退避重新入队）。
    async fn run(
        &self,
        gateway: &dyn ScimGateway,
        mut entry: ScimJobEntry,
        now: time::OffsetDateTime,
    ) -> ScimJobOutcome {
        entry.attempt += 1;
        let attempt = entry.attempt;
        let result = match &entry.job {
            ScimJob::PutUser(draft) => gateway.put_user(&entry.config, draft).await,
            ScimJob::PutGroup(draft) => gateway.put_group(&entry.config, draft).await,
            ScimJob::PatchActive {
                external_id,
                active,
            } => {
                gateway
                    .patch_user_active(&entry.config, external_id, *active)
                    .await
            }
        };
        match result {
            Ok(_) => ScimJobOutcome {
                job: entry.job.clone(),
                ok: true,
                error: None,
                attempt,
                will_retry: false,
            },
            Err(error) => {
                let will_retry = attempt < MAX_PUSH_ATTEMPTS;
                let job = entry.job.clone();
                if will_retry {
                    entry.ready_at = Some(now + backoff_delay(attempt));
                    self.enqueue(entry);
                }
                ScimJobOutcome {
                    job,
                    ok: false,
                    error: Some(error.to_string()),
                    attempt,
                    will_retry,
                }
            }
        }
    }
}

/// 判断一条 User 是否命中 SCIM 过滤器。
///
/// 只支持最常见的一种形式：`<attr> eq "<value>"`（`attr` 取 `id` / `userName`）。
/// 空过滤器表示「全部」。
pub fn matches_filter(user: &ScimUser, filter: &str) -> bool {
    let filter = filter.trim();
    if filter.is_empty() {
        return true;
    }
    let Some((attribute, value)) = filter.split_once(" eq ") else {
        return true;
    };
    let value = value.trim().trim_matches('"');
    match attribute.trim() {
        "id" => user.id == value,
        "userName" => user.user_name == value,
        _ => true,
    }
}

/// 内存 SCIM 网关（**内置 mock**，用于测试与本地演练）。
///
/// 它按 SCIM 的 **PUT 语义**实现：同一个 id 推两次只有一条记录 ——
/// 这正是幂等性要验的东西。
#[derive(Debug, Default)]
pub struct InMemoryScimGateway {
    users: Mutex<BTreeMap<String, Value>>,
    groups: Mutex<BTreeMap<String, Value>>,
    /// 每次调用记一笔（用来断言「推了几次」）。
    calls: Mutex<Vec<String>>,
    /// 模拟失败（前 N 次调用失败）。
    failures: Mutex<u32>,
}

impl InMemoryScimGateway {
    /// 造一个空的 mock。
    pub fn new() -> Self {
        Self::default()
    }

    /// 让接下来 N 次调用失败（用于测重试与「推送失败不阻塞本地」）。
    pub fn fail_next(&self, count: u32) {
        let mut failures = self.failures.lock().unwrap_or_else(|err| err.into_inner());
        *failures = count;
    }

    /// 当前的 User 条数。
    pub fn user_count(&self) -> usize {
        self.users
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .len()
    }

    /// 当前的 Group 条数。
    pub fn group_count(&self) -> usize {
        self.groups
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .len()
    }

    /// 读一条 User（按 id）。
    pub fn user(&self, id: &str) -> Option<Value> {
        self.users
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .get(id)
            .cloned()
    }

    /// 读一条 Group（按 id）。
    pub fn group(&self, id: &str) -> Option<Value> {
        self.groups
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .get(id)
            .cloned()
    }

    /// 已发生的调用（`put:Users/<id>` 这样的标记）。
    pub fn calls(&self) -> Vec<String> {
        self.calls
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .clone()
    }

    /// 记一次调用，并在需要时失败。
    fn record(&self, marker: &str) -> Result<(), ScimError> {
        let mut calls = self.calls.lock().unwrap_or_else(|err| err.into_inner());
        calls.push(marker.to_string());
        let mut failures = self.failures.lock().unwrap_or_else(|err| err.into_inner());
        if *failures > 0 {
            *failures -= 1;
            return Err(ScimError::Transport(
                "the mock scim target is down".to_string(),
            ));
        }
        Ok(())
    }
}

impl ScimGateway for InMemoryScimGateway {
    fn put_user<'a>(
        &'a self,
        config: &'a ScimProviderConfig,
        draft: &'a ScimUserDraft,
    ) -> ScimFuture<'a, Result<ScimResponse, ScimError>> {
        let marker = format!("put:Users/{}", draft.id);
        let result = self.record(&marker).map(|()| {
            let payload = config.user_payload(draft);
            let mut users = self.users.lock().unwrap_or_else(|err| err.into_inner());
            users.insert(draft.id.clone(), payload);
            ScimResponse {
                id: draft.id.clone(),
                status: 200,
            }
        });
        Box::pin(async move { result })
    }

    fn put_group<'a>(
        &'a self,
        config: &'a ScimProviderConfig,
        draft: &'a ScimGroupDraft,
    ) -> ScimFuture<'a, Result<ScimResponse, ScimError>> {
        let marker = format!("put:Groups/{}", draft.id);
        let result = self.record(&marker).map(|()| {
            let payload = config.group_payload(draft);
            let mut groups = self.groups.lock().unwrap_or_else(|err| err.into_inner());
            groups.insert(draft.id.clone(), payload);
            ScimResponse {
                id: draft.id.clone(),
                status: 200,
            }
        });
        Box::pin(async move { result })
    }

    fn list_users<'a>(
        &'a self,
        config: &'a ScimProviderConfig,
        filter: &str,
    ) -> ScimFuture<'a, Result<Vec<ScimUser>, ScimError>> {
        let result = self.record("list:Users").map(|()| {
            let users = self.users.lock().unwrap_or_else(|err| err.into_inner());
            users
                .values()
                .map(|value| ScimUser::from_json(value, config))
                .filter(|user| matches_filter(user, filter))
                .collect()
        });
        Box::pin(async move { result })
    }

    fn list_groups<'a>(
        &'a self,
        config: &'a ScimProviderConfig,
        _filter: &str,
    ) -> ScimFuture<'a, Result<Vec<ScimGroup>, ScimError>> {
        let result = self.record("list:Groups").map(|()| {
            let groups = self.groups.lock().unwrap_or_else(|err| err.into_inner());
            groups
                .values()
                .map(|value| ScimGroup {
                    id: value
                        .get("id")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    display_name: value
                        .get(&config.group_fields.display_name)
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    member_ids: value
                        .get("members")
                        .and_then(Value::as_array)
                        .map(|members| {
                            members
                                .iter()
                                .filter_map(|member| member.get("value").and_then(Value::as_str))
                                .map(str::to_string)
                                .collect()
                        })
                        .unwrap_or_default(),
                })
                .collect()
        });
        Box::pin(async move { result })
    }

    fn patch_user_active<'a>(
        &'a self,
        _config: &'a ScimProviderConfig,
        external_id: &str,
        active: bool,
    ) -> ScimFuture<'a, Result<ScimResponse, ScimError>> {
        let result = self
            .record(&format!("patch:Users/{external_id}"))
            .map(|()| {
                let mut users = self.users.lock().unwrap_or_else(|err| err.into_inner());
                if let Some(payload) = users.get_mut(external_id) {
                    if let Some(object) = payload.as_object_mut() {
                        object.insert("active".to_string(), Value::Bool(active));
                    }
                }
                ScimResponse {
                    id: external_id.to_string(),
                    status: 200,
                }
            });
        Box::pin(async move { result })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draft(id: &str) -> ScimUserDraft {
        ScimUserDraft {
            id: id.to_string(),
            username: format!("{id}-name"),
            email: Some(format!("{id}@example.com")),
            display_name: Some(format!("{id} display")),
            active: true,
            group_ids: vec!["dept-1".to_string()],
        }
    }

    #[test]
    fn the_default_mapping_uses_the_standard_scim_attribute_names() {
        let config = ScimProviderConfig::new("p", "http://scim.example/v2");
        let payload = config.user_payload(&draft("u1"));
        assert_eq!(payload["userName"], Value::String("u1-name".to_string()));
        assert_eq!(
            payload["displayName"],
            Value::String("u1 display".to_string())
        );
        assert_eq!(
            payload["emails"][0]["value"],
            Value::String("u1@example.com".to_string())
        );
        assert_eq!(payload["emails"][0]["primary"], Value::Bool(true));
        assert_eq!(payload["active"], Value::Bool(true));
        assert_eq!(
            payload["groups"][0]["value"],
            Value::String("dept-1".to_string())
        );
        assert_eq!(
            payload["schemas"][0],
            Value::String(SCIM_USER_SCHEMA.to_string())
        );
    }

    /// AC-S-013：用户名 / 邮箱 / 显示名来自**不同属性**时，映射必须生效。
    #[test]
    fn a_custom_field_mapping_is_applied_to_the_payload() {
        let config = ScimProviderConfig::from_json(
            r#"{
              "provider": "corp-scim",
              "baseUrl": "http://scim.example/v2",
              "userFields": {
                "username": "login",
                "email": "workEmail",
                "displayName": "cn",
                "emailShape": "single"
              }
            }"#,
        )
        .expect("配置应能解析");
        let payload = config.user_payload(&draft("u2"));
        assert_eq!(payload["login"], Value::String("u2-name".to_string()));
        assert_eq!(
            payload["workEmail"],
            Value::String("u2@example.com".to_string())
        );
        assert_eq!(payload["cn"], Value::String("u2 display".to_string()));
        assert!(payload.get("userName").is_none());
        assert!(payload.get("emails").is_none());
    }

    #[test]
    fn https_is_rejected_with_a_clear_error_instead_of_a_silent_downgrade() {
        let config = ScimProviderConfig::new("p", "https://scim.example/v2");
        let error = config.validate().expect_err("https 应被拒绝");
        assert!(matches!(error, ScimError::Config(_)));

        let mut platform = config.clone();
        platform.tls_mode = TlsMode::Platform;
        let error = platform.validate().expect_err("platform tls 应被拒绝");
        assert!(matches!(error, ScimError::NotImplemented(_)));
    }

    /// AC-S-012：连续两次全量同步**不产生重复条目**（PUT 语义）。
    #[tokio::test]
    async fn pushing_the_same_user_twice_does_not_duplicate_it() {
        let gateway = InMemoryScimGateway::new();
        let config = ScimProviderConfig::new("p", "http://scim.example/v2");
        let payload = draft("u3");

        gateway
            .put_user(&config, &payload)
            .await
            .expect("第一次应成功");
        gateway
            .put_user(&config, &payload)
            .await
            .expect("第二次应成功");

        assert_eq!(gateway.user_count(), 1);
        assert_eq!(gateway.calls().len(), 2);
    }

    #[tokio::test]
    async fn groups_are_pushed_with_their_members() {
        let gateway = InMemoryScimGateway::new();
        let config = ScimProviderConfig::new("p", "http://scim.example/v2");
        let group = ScimGroupDraft {
            id: "dept-1".to_string(),
            display_name: "Engineering".to_string(),
            member_ids: vec!["u1".to_string(), "u2".to_string()],
        };
        gateway.put_group(&config, &group).await.expect("应成功");
        let stored = gateway.group("dept-1").expect("Group 应存在");
        assert_eq!(
            stored["displayName"],
            Value::String("Engineering".to_string())
        );
        assert_eq!(stored["members"].as_array().map(Vec::len), Some(2));
    }

    #[tokio::test]
    async fn a_failed_push_is_retried_with_backoff_and_then_gives_up() {
        let gateway = InMemoryScimGateway::new();
        let outbox = ScimOutbox::new();
        let config = ScimProviderConfig::new("p", "http://scim.example/v2");
        let now = time::OffsetDateTime::now_utc();

        gateway.fail_next(1);
        outbox.enqueue(ScimJobEntry {
            tenant_id: "tenant-a".to_string(),
            config: config.clone(),
            job: ScimJob::PutUser(draft("u4")),
            attempt: 0,
            ready_at: None,
        });

        let outcomes = outbox.drain_once(&gateway, now).await;
        assert_eq!(outcomes.len(), 1);
        assert!(!outcomes[0].ok);
        assert!(outcomes[0].will_retry);
        assert_eq!(outbox.pending(), 1, "失败的任务应留在队列里等重试");

        // 退避未到之前不会再取它。
        let outcomes = outbox.drain_once(&gateway, now).await;
        assert!(outcomes.is_empty(), "退避时间内不应重试");
        assert_eq!(outbox.pending(), 1);

        // 退避到期后重试成功。
        let later = now + backoff_delay(1);
        let outcomes = outbox.drain_once(&gateway, later).await;
        assert_eq!(outcomes.len(), 1);
        assert!(outcomes[0].ok);
        assert_eq!(outbox.pending(), 0);
    }

    #[tokio::test]
    async fn a_push_is_not_retried_forever() {
        let gateway = InMemoryScimGateway::new();
        let outbox = ScimOutbox::new();
        let config = ScimProviderConfig::new("p", "http://scim.example/v2");
        gateway.fail_next(u32::MAX);

        outbox.enqueue(ScimJobEntry {
            tenant_id: "tenant-a".to_string(),
            config,
            job: ScimJob::PutUser(draft("u5")),
            attempt: 0,
            ready_at: None,
        });

        let mut now = time::OffsetDateTime::now_utc();
        let mut attempts = 0;
        for _ in 0..MAX_PUSH_ATTEMPTS {
            let outcomes = outbox.drain_once(&gateway, now).await;
            attempts += outcomes.len();
            now += backoff_delay(MAX_PUSH_ATTEMPTS);
        }
        assert_eq!(attempts, MAX_PUSH_ATTEMPTS as usize);
        assert_eq!(outbox.pending(), 0, "超过重试上限后任务应被丢弃");
    }

    #[test]
    fn the_backoff_grows_exponentially() {
        assert_eq!(backoff_delay(1), Duration::from_millis(100));
        assert_eq!(backoff_delay(2), Duration::from_millis(200));
        assert_eq!(backoff_delay(3), Duration::from_millis(400));
    }

    #[test]
    fn path_segments_are_url_encoded() {
        assert_eq!(encode_path_segment("a b/c"), "a%20b%2Fc");
        assert_eq!(encode_path_segment("simple-id_1.0"), "simple-id_1.0");
    }

    #[test]
    fn query_filters_are_url_encoded() {
        assert_eq!(
            encode_query("userName eq \"alice\""),
            "userName%20eq%20%22alice%22"
        );
    }

    #[test]
    fn responses_are_parsed_with_the_configured_field_names() {
        let config = ScimProviderConfig::from_json(
            r#"{
              "provider": "p",
              "baseUrl": "http://scim.example/v2",
              "userFields": { "username": "login", "email": "workEmail", "displayName": "cn" }
            }"#,
        )
        .expect("配置应能解析");
        let value = serde_json::json!({
            "id": "u1",
            "login": "alice",
            "workEmail": "alice@example.com",
            "cn": "Alice",
            "active": false
        });
        let user = ScimUser::from_json(&value, &config);
        assert_eq!(user.id, "u1");
        assert_eq!(user.user_name, "alice");
        assert_eq!(user.email.as_deref(), Some("alice@example.com"));
        assert_eq!(user.display_name.as_deref(), Some("Alice"));
        assert!(!user.active);
    }
}
