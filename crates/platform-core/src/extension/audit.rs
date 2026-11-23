//! 通用审计事件与审计落点（REQ-014）。
//!
//! # 定位（D135）
//!
//! `platform-org` 的 `AuditRecorder` 绑死 `OrgAuditEvent`、`platform-iam` 的
//! `SsoAuditRecorder` 绑死 `SsoAuditEvent` —— 两者**签名一律不改**（D135）。
//! 本模块提供**第三样东西**：一个**通用**的、带 payload 的事件类型与一个
//! **可对象化**的 sink trait，让各业务模块把自己的事件翻译成通用事件后
//! 交给平台 sink。
//!
//! ```text
//! platform-core/extension/audit.rs   （本模块，零额外依赖）
//!        ↑ trait
//! platform-iam / platform-org / platform-rbac / platform-config
//! ```
//!
//! 通用类型必须住在**最底层**：`platform-org → platform-iam` 已是既成依赖方向，
//! `platform-iam → platform-org` 会成环（D135）。
//!
//! # 三样能力
//!
//! | 能力 | 位置 | 为什么在 core |
//! |------|------|------------|
//! | 通用事件 + sink trait | 本模块 | 业务模块只依赖 core → 不成环（D150） |
//! | JSON 规范化（写入侧） | [`canonical_json`] | 业务方自己落审计时也需要同一套形态 |
//! | 敏感键扫描器 | [`scan_sensitive_keys`] | 必须**可单独调用**（AC-S-133 步骤 4 要喂字符串） |
//!
//! # 敏感键「两侧都做」（D156）
//!
//! - **DB 侧**（权威，真正的「写入前」）：迁移里的 `platform_audit_sensitive_keys`；
//! - **Rust 侧**（本模块）：供测试 / 工具 / 提前拦截用。
//!
//! 两边的**键集合必须逐项相等** → 由 `platform-audit` 的一致性测试守住
//! （从迁移 SQL 里抽出 DB 侧的数组，与 [`SENSITIVE_KEYS`] 比对）。
//!
//! # 哈希链的分工
//!
//! - **写入侧**的哈希由数据库里的 `SECURITY DEFINER` 函数计算（并发下才不会分叉，
//!   见 REQ-014 decisions D161）→ 本模块**不**实现写入侧哈希；
//! - **校验侧**的哈希由 `platform-audit/src/verify.rs` 重算 —— 它**读回列里存的那段
//!   文本**直接参与哈希（D141），**不**把 `before` / `after` 解析回 JSON 再序列化。

use std::collections::BTreeSet;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, OnceLock};

use serde_json::Value;
use time::OffsetDateTime;

use crate::context::RequestContext;
use crate::problem::{
    CODE_AUDIT_CHAIN_BROKEN, CODE_AUDIT_EVENT_NOT_FOUND, CODE_AUDIT_INVALID_FILTER,
    CODE_AUDIT_SENSITIVE_PAYLOAD_REJECTED, CODE_AUDIT_SINK_UNAVAILABLE, CODE_INVALID_REQUEST,
    ProblemDetails,
};

/// 装箱的 future（让 sink trait 保持**可对象化**）。
///
/// 与 `platform-org/src/audit.rs` 的同名类型别名同款 —— 平台里**不引** `futures`
/// （依赖预算：默认零新增，dev-guide §9.10）。
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// 链的**genesis**：64 个字符 `0`（REQ-014 design §4.4）。
///
/// ⚠️ **恰好 64 个** —— 32 个 `0`（MD5 长度）在实现里很容易顺手写错。
pub const GENESIS_HASH: &str = "0000000000000000000000000000000000000000000000000000000000000000";

/// 敏感键的**基线集合**（**恰好 14 个**，REQ-014 D156）。
///
/// 匹配规则（D156 + **D172 的「驼峰拆词」**）：
///
/// - **归一化**：见 [`normalize_sensitive_key`]（小写 / 数字与随后的大写字母之间插 `_`，
///   缩写边界也插 `_`，全转小写，`-` 视为 `_`）；
/// - **匹配**：归一化后与基线键**精确相等** —— **不是子串**
///   （否则 `secretary` / `tokenizer` 会被误伤，业务方会绕开防护）；
/// - **递归**：检查 JSON 的**每一层**。
///
/// ⚠️ 这里是 DB 侧 `platform_audit_sensitive_keys` 的**对照基准** ——
/// 两边不一致由 `platform-audit` 的一致性测试判红。
pub const SENSITIVE_KEYS: [&str; 14] = [
    "password",
    "passwd",
    "pwd",
    "secret",
    "client_secret",
    "token",
    "access_token",
    "refresh_token",
    "id_token",
    "authorization",
    "credential",
    "api_key",
    "private_key",
    "cookie",
];

/// 通用审计事件（**带 payload**）。
///
/// 字段与契约 C12 的固定字段**逐字对应**（`before` / `after` 的类型是实现选择）。
#[derive(Debug, Clone, PartialEq)]
pub struct AuditEvent {
    /// 租户；链的边界（分链，D130）。
    pub tenant_id: String,
    /// 操作人；未认证（如登录失败）为 `None`。
    pub actor_id: Option<String>,
    /// 动作名（事件码，命名法 `<域>.<对象>.<动作>`，D163）。
    pub action: String,
    /// 对象类型。
    pub object_type: String,
    /// 对象 ID。
    pub object_id: Option<String>,
    /// 变更前（**任意 JSON**；`None` = 无变更）。
    pub before: Option<Value>,
    /// 变更后（**任意 JSON**；`None` = 无变更）。
    pub after: Option<Value>,
    /// 来源 IP；取不到时落**空串**（不是 `NULL`）。
    pub ip: Option<String>,
    /// 设备（User-Agent）；取不到时落**空串**。
    pub user_agent: Option<String>,
}

impl AuditEvent {
    /// 造一个最小事件（无 actor / 无前后值 / 无 IP / 无 UA）。
    pub fn new(
        tenant_id: impl Into<String>,
        action: impl Into<String>,
        object_type: impl Into<String>,
    ) -> Self {
        Self {
            tenant_id: tenant_id.into(),
            actor_id: None,
            action: action.into(),
            object_type: object_type.into(),
            object_id: None,
            before: None,
            after: None,
            ip: None,
            user_agent: None,
        }
    }

    /// 绑定操作人。
    pub fn with_actor(mut self, actor_id: impl Into<String>) -> Self {
        self.actor_id = Some(actor_id.into());
        self
    }

    /// 绑定对象 ID。
    pub fn with_object_id(mut self, object_id: impl Into<String>) -> Self {
        self.object_id = Some(object_id.into());
        self
    }

    /// 绑定变更前的值。
    pub fn with_before(mut self, before: Value) -> Self {
        self.before = Some(before);
        self
    }

    /// 绑定变更后的值。
    pub fn with_after(mut self, after: Value) -> Self {
        self.after = Some(after);
        self
    }

    /// 绑定来源 IP。
    pub fn with_ip(mut self, ip: impl Into<String>) -> Self {
        self.ip = Some(ip.into());
        self
    }

    /// 绑定设备。
    pub fn with_user_agent(mut self, user_agent: impl Into<String>) -> Self {
        self.user_agent = Some(user_agent.into());
        self
    }
}

/// 审计落点（可对象化）。
///
/// 业务模块**不写审计表 SQL** —— 它们把事件交给平台 sink（REQ-014 D150）。
///
/// ```ignore
/// let sink: Arc<dyn AuditSink> = app.audit_sink();
/// sink.record(&ctx, &event).await?;
/// ```
pub trait AuditSink: Send + Sync {
    /// 记录一个事件。
    ///
    /// 失败**不得**被静默吞掉：审计写不进去时调用方要能据此拒绝请求
    /// （「宁可不登录，不可无审计地登录」，D157）。
    fn record<'a>(
        &'a self,
        ctx: &'a RequestContext,
        event: &'a AuditEvent,
    ) -> BoxFuture<'a, Result<(), AuditError>>;
}

/// 审计 sink 的**可选**后设槽（装配层注入一次）。
///
/// # 为什么是「后设槽」而不是构造参数
///
/// 与 `platform-iam` 的 `PermissionProviderSlot` 同一套模式：
/// 加进各模块的 `new` 签名会**破既有调用点**（属已合并需求的产物）→
/// 后设槽既不改签名，也让「未装配」成为可表达状态。
///
/// # 为什么放在 `platform-core`
///
/// 四个业务模块（iam / rbac / config / org）都要用它 →
/// 放在 core = **一份实现**，各模块只依赖 core → **不成环**（D150）。
#[derive(Clone, Default)]
pub struct AuditSinkSlot(OnceLock<Arc<dyn AuditSink>>);

impl AuditSinkSlot {
    /// 注入实现；**重复注入返回 `false`**（不覆盖已装好的实现）。
    pub fn set(&self, sink: Arc<dyn AuditSink>) -> bool {
        self.0.set(sink).is_ok()
    }

    /// 读实现；**未注入 → `None`**。
    pub fn get(&self) -> Option<&Arc<dyn AuditSink>> {
        self.0.get()
    }
}

impl std::fmt::Debug for AuditSinkSlot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuditSinkSlot")
            .field("installed", &self.0.get().is_some())
            .finish()
    }
}

/// 审计链上的错误（**稳定错误码**，契约 C6）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuditError {
    /// `before` / `after` 含敏感键 → **写入被硬拒绝**（D134）。
    ///
    /// ⚠️ 只带**键名**，**不带值** —— 回显值等于再泄漏一次（D134 / AC-S-133 步骤 4）。
    SensitivePayloadRejected {
        /// 命中的敏感键（**归一化后**的键名）。
        keys: Vec<String>,
    },
    /// 链校验发现断裂（D146 的两个 reason 之一）。
    ChainBroken {
        /// 断裂条数。
        broken: usize,
    },
    /// 单条审计记录不存在。
    EventNotFound,
    /// 过滤 / 排序参数非法（D153）。
    ///
    /// ⚠️ **只用于审计专属的筛选/排序参数**（`filter[...]` / `sort` / `page`）。
    /// 分页参数（`limit` / `cursor`）非法请用 [`Self::InvalidPagination`]。
    InvalidFilter {
        /// 面向开发者的英文原因。
        reason: String,
    },
    /// **分页参数**非法（`limit` 越界 / `cursor` 格式错，REQ-019 D341 / AC-S-034）。
    ///
    /// # 为什么与 [`Self::InvalidFilter`] 分开
    ///
    /// AC-S-034 要求「分页契约**跨 crate 统一**」：同一个错误场景
    /// （`limit=0` / `limit=1000` / 非法 `cursor`）在**任何**分页端点上
    /// 都必须返回同一个 `code`。`platform-observability` 的新端点用的是
    /// 平台通用的 `INVALID_REQUEST`，所以 audit 侧的分页错误也必须用它。
    ///
    /// ⚠️ `AUDIT_INVALID_FILTER` **不废弃** —— 它继续服务审计专属的
    /// 筛选/排序参数（那些错误码是 audit 语义，不需要跨 crate 统一）。
    InvalidPagination {
        /// 面向开发者的英文原因。
        reason: String,
    },
    /// 审计写入失败 → 调用方应当**拒绝**这次操作（D157）。
    SinkUnavailable {
        /// 面向开发者的英文原因。
        reason: String,
    },
    /// 事件声明的租户与请求上下文的租户**不一致**。
    ///
    /// 租户的**唯一来源**是请求上下文（契约 C3）—— 两边不一致说明调用方写错了，
    /// 静默取其中一个等于把「写错租户」变成不可见的事故。
    TenantMismatch {
        /// 请求上下文里的租户。
        context: String,
        /// 事件里声明的租户。
        event: String,
    },
}

impl AuditError {
    /// 稳定的机器可读错误码（契约 C6）。
    pub fn code(&self) -> &'static str {
        match self {
            Self::SensitivePayloadRejected { .. } => CODE_AUDIT_SENSITIVE_PAYLOAD_REJECTED,
            Self::ChainBroken { .. } => CODE_AUDIT_CHAIN_BROKEN,
            Self::EventNotFound => CODE_AUDIT_EVENT_NOT_FOUND,
            Self::InvalidFilter { .. } => CODE_AUDIT_INVALID_FILTER,
            // ⚠️ **REQ-019 D341 / AC-S-034**：分页参数非法改用**平台通用**的
            // `INVALID_REQUEST`，与 `GET /api/observability/rate-limit/status` 逐字一致。
            // 这是「分页契约跨 crate 统一」的实质 —— 调用方用一套逻辑处理即可。
            Self::InvalidPagination { .. } => CODE_INVALID_REQUEST,
            Self::SinkUnavailable { .. } => CODE_AUDIT_SINK_UNAVAILABLE,
            Self::TenantMismatch { .. } => CODE_AUDIT_INVALID_FILTER,
        }
    }

    /// HTTP 状态码。
    pub fn status(&self) -> u16 {
        match self {
            Self::SensitivePayloadRejected { .. } => 400,
            Self::ChainBroken { .. } => 409,
            Self::EventNotFound => 404,
            Self::InvalidFilter { .. } | Self::TenantMismatch { .. } => 400,
            Self::InvalidPagination { .. } => 400,
            Self::SinkUnavailable { .. } => 503,
        }
    }

    /// 面向开发者的**英文 ASCII** 描述（契约 C6：不得在 `detail` 里拼中文）。
    pub fn detail(&self) -> String {
        match self {
            Self::SensitivePayloadRejected { keys } => format!(
                "the audit payload contains sensitive keys: {}",
                keys.join(", ")
            ),
            Self::ChainBroken { broken } => {
                format!("the audit chain integrity check found {broken} broken entries")
            }
            Self::EventNotFound => "the audit event was not found".to_string(),
            Self::InvalidFilter { reason } => format!("the audit query is invalid: {reason}"),
            Self::InvalidPagination { reason } => {
                format!("the pagination parameter is invalid: {reason}")
            }
            Self::SinkUnavailable { reason } => {
                format!("the audit sink is unavailable: {reason}")
            }
            Self::TenantMismatch { context, event } => format!(
                "the audit event tenant `{event}` does not match the request context tenant `{context}`"
            ),
        }
    }

    /// 转成统一错误体（RFC 9457）。
    pub fn problem_details(&self, ctx: &RequestContext, instance: &str) -> ProblemDetails {
        ProblemDetails::from_context(
            self.code(),
            crate::titles::title_of_code(self.code()),
            self.status(),
            self.detail(),
            ctx,
            instance,
            None,
        )
    }
}

impl std::fmt::Display for AuditError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.detail())
    }
}

impl std::error::Error for AuditError {}

/// 把 JSON 值规范化成**写入侧**的规范文本（REQ-014 D142 §2）。
///
/// 规则（**逐字节**钉死，差一个字节哈希就永远对不上）：
///
/// - 对象的键按 **UTF-8 字节序升序**排序，**递归**到每一层；
/// - **无任何多余空白**（紧凑）；
/// - 非 ASCII 字符**不转义**（按 UTF-8 原样输出）；
/// - 控制字符用 JSON 标准短转义（`\n` `\t` `\r` `\b` `\f`），其余用 `\u00XX`；
/// - 字符串里的 `"` 与 `\` 必须转义。
///
/// # 为什么**不**依赖 `serde_json` 的 `Map` 迭代序
///
/// `serde_json` 默认的 `Map` 是 `BTreeMap`（键有序 ✓），但任一 crate 打开
/// `preserve_order` feature 后就变成 `IndexMap`（插入序 ❌）→
/// 同一份 JSON 两次规范化产出不同文本 → 哈希不可复现。
/// 所以这里**自己**收集键并排序，不看 `Map` 的迭代序。
pub fn canonical_json(value: &Value) -> String {
    let mut out = String::new();
    write_canonical(value, &mut out);
    out
}

/// 把 JSON 值规范化成规范文本；`None` → `None`（列里存 `NULL`，D142 §3）。
pub fn canonical_json_opt(value: Option<&Value>) -> Option<String> {
    value.map(canonical_json)
}

fn write_canonical(value: &Value, out: &mut String) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(flag) => out.push_str(if *flag { "true" } else { "false" }),
        Value::Number(number) => out.push_str(&number.to_string()),
        Value::String(text) => write_json_string(text, out),
        Value::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_canonical(item, out);
            }
            out.push(']');
        }
        Value::Object(map) => {
            // 自己收集键并按 **UTF-8 字节序**排序（不依赖 Map 的迭代序）。
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort_by(|left, right| left.as_bytes().cmp(right.as_bytes()));
            out.push('{');
            for (index, key) in keys.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_json_string(key, out);
                out.push(':');
                write_canonical(&map[*key], out);
            }
            out.push('}');
        }
    }
}

/// 把一段文本写成 JSON 字符串字面量（含引号）。
fn write_json_string(value: &str, out: &mut String) {
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            other if (other as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", other as u32));
            }
            other => out.push(other),
        }
    }
    out.push('"');
}

/// 扫描一段 JSON 文本里的**敏感键**（D156）。
///
/// 返回**归一化后**的命中键集合（`BTreeSet` → 去重 + 稳定序）。
///
/// - **可单独调用**：喂一段字符串即可（AC-S-133 步骤 4 的阳性对照要求）；
/// - **递归**：检查**每一层**（含数组里的对象）；
/// - **完整键名匹配**：归一化后与基线键**精确相等**（`secretary` / `tokenizer` **不**命中）；
/// - 文本**不是合法 JSON** → 返回空集合（没有可命中的键）。
pub fn scan_sensitive_keys(payload: &str) -> BTreeSet<String> {
    let Ok(value) = serde_json::from_str::<Value>(payload) else {
        return BTreeSet::new();
    };
    let mut hits = BTreeSet::new();
    collect_sensitive_keys(&value, &mut hits);
    hits
}

/// 键名归一化（D156 + **D172 的「驼峰拆词」**）。
///
/// # 规则（**按序**，两侧必须逐字一致）
///
/// 1. **缩写边界**：`X Y z`（大写 + 大写 + 小写）→ `X _ Y z`
///    （`APIKey` → `API_Key`）；
/// 2. **驼峰边界**：小写 / 数字与**随后**的大写字母之间插入 `_`
///    （`apiKey` → `api_Key`、`key2Value` → `key2_Value`）；
/// 3. 全部转**小写**；
/// 4. `-` 视为 `_`。
///
/// | 输入 | 归一化 | 命中？ |
/// |------|--------|--------|
/// | `apiKey` / `APIKey` / `clientSecret` | `api_key` / `api_key` / `client_secret` | ✅ |
/// | `api_key` / `api-key` | `api_key` | ✅ |
/// | `secretary` / `tokenizer` | **不变** | ❌（**非子串匹配**这条纪律不能破） |
///
/// # 为什么加「驼峰拆词」（D172）
///
/// D156 原本的归一化只做「转小写 + `-`→`_`」→ `apiKey` 变成 `apikey` ≠ `api_key`
/// → **不命中**。而 `apiKey` / `clientSecret` 是**极常见**的写法 →
/// 只挡 `api_key` 等于给了一个绕过口。
///
/// ⚠️ **拆词只影响「键名长什么样」，不影响「匹配是不是子串」** ——
/// `secretary` / `tokenizer` 里没有大写字母 → 归一化后**原样** → 仍然**不命中**。
///
/// # 与 DB 侧的一致性
///
/// DB 侧用 `platform_audit_normalize_key`（迁移里）实现**同一套规则** ——
/// 两侧的**归一化结果**必须逐字相同（由 `platform-audit` 的一致性测试判红）。
/// ⚠️ 两侧都**只认 ASCII** 的大小写与数字（`is_ascii_upper` / `ascii()` 区间判断）——
/// 用正则的 `[A-Z]` 会受**排序规则**影响，中文 / 土耳其语等环境下两侧会漂移。
pub fn normalize_sensitive_key(key: &str) -> String {
    let chars: Vec<char> = key.chars().collect();

    // -- 规则 1：缩写边界（`XYz` → `X_Yz`） ---------------------------------
    //
    // ⚠️ 命中后**消费 3 个字符**（`X` `Y` `z`）—— 与 DB 侧的正则
    //    `([A-Z])([A-Z][a-z])` 的「非重叠、左到右」语义一致。
    let mut split: Vec<char> = Vec::with_capacity(chars.len() + 4);
    let mut index = 0usize;
    while index < chars.len() {
        let current = chars[index];
        let is_acronym_boundary = index >= 2
            && index + 1 < chars.len()
            && is_ascii_upper(current)
            && is_ascii_upper(chars[index - 1])
            && is_ascii_lower(chars[index + 1]);
        if is_acronym_boundary {
            split.push('_');
            split.push(current);
            split.push(chars[index + 1]);
            index += 2;
            continue;
        }
        split.push(current);
        index += 1;
    }

    // -- 规则 2：驼峰边界（小写 / 数字与随后的大写字母之间插 `_`） -----------
    let mut out = String::with_capacity(split.len() + 4);
    for (index, ch) in split.iter().enumerate() {
        if index > 0
            && is_ascii_upper(*ch)
            && (is_ascii_lower(split[index - 1]) || split[index - 1].is_ascii_digit())
        {
            out.push('_');
        }
        out.push(*ch);
    }

    // -- 规则 3 + 4：转小写；`-` 视为 `_` ------------------------------------
    out.to_lowercase().replace('-', "_")
}

/// ASCII 大写字母。
fn is_ascii_upper(ch: char) -> bool {
    ch.is_ascii_uppercase()
}

/// ASCII 小写字母。
fn is_ascii_lower(ch: char) -> bool {
    ch.is_ascii_lowercase()
}

fn collect_sensitive_keys(value: &Value, hits: &mut BTreeSet<String>) {
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                let normalized = normalize_sensitive_key(key);
                if SENSITIVE_KEYS.contains(&normalized.as_str()) {
                    hits.insert(normalized);
                }
                collect_sensitive_keys(child, hits);
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_sensitive_keys(item, hits);
            }
        }
        _ => {}
    }
}

/// 把时刻格式化成**规范时间戳文本**：`YYYY-MM-DDTHH:MM:SS.ffffffZ`（D142 §1 / D144）。
///
/// ⚠️ **必须手工拼装**：
///
/// - `time` 的 `Rfc3339` 在微秒为 0 时**整个省略小数部分** → 违反「恰好 6 位」；
/// - 根 `Cargo.toml` 只给 `time` 开了 `formatting` → `format_description!` 宏与
///   `format_description::parse()` **都不可用**（D144）；
/// - **必须先转 UTC** —— 直接格式化 `OffsetDateTime` 会打印**本地时**的钟点再贴个 `Z`
///   （差几小时，链永远「校验失败」，D151）。
///
/// DB 侧的对应写法是
/// `to_char(created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US"Z"')`，
/// 两者必须**逐字节相同**。
pub fn format_timestamp_micros_utc(moment: &OffsetDateTime) -> String {
    let utc = moment.to_offset(time::UtcOffset::UTC);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:06}Z",
        utc.year(),
        utc.month() as u8,
        utc.day(),
        utc.hour(),
        utc.minute(),
        utc.second(),
        u32::from(utc.millisecond()) * 1000 + utc.microsecond() % 1000,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_sensitive_baseline_has_exactly_fourteen_keys() {
        assert_eq!(SENSITIVE_KEYS.len(), 14, "D156 把基线钉死为 14 个");
        let mut sorted = SENSITIVE_KEYS.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), 14, "基线键不得重复");
    }

    #[test]
    fn the_genesis_hash_is_exactly_sixty_four_zeros() {
        assert_eq!(GENESIS_HASH.len(), 64);
        assert!(GENESIS_HASH.chars().all(|ch| ch == '0'));
    }

    /// 阳性对照（dev-guide §9.2）：扫「0 命中」之前先证明扫描器会命中。
    #[test]
    fn the_scanner_hits_a_nested_sensitive_key() {
        let hits = scan_sensitive_keys(r#"{"a":{"b":{"api_key":"x"}}}"#);
        assert!(
            hits.contains("api_key"),
            "扫描器必须能命中嵌套层里的敏感键，否则「0 命中」不可信：{hits:?}"
        );
    }

    /// 反例：按**完整键名**匹配，`secretary` / `tokenizer` **不是**敏感键。
    #[test]
    fn the_scanner_does_not_match_substrings() {
        let hits = scan_sensitive_keys(r#"{"secretary":"x","tokenizer":"y"}"#);
        assert!(hits.is_empty(), "子串不得被当成敏感键：{hits:?}");
    }

    /// D172：**驼峰写法必须命中**（`apiKey` / `APIKey` / `clientSecret`）。
    #[test]
    fn the_scanner_hits_camel_case_keys() {
        for (payload, expected) in [
            (r#"{"apiKey":"x"}"#, "api_key"),
            (r#"{"APIKey":"x"}"#, "api_key"),
            (r#"{"clientSecret":"x"}"#, "client_secret"),
            (r#"{"accessToken":"x"}"#, "access_token"),
            (r#"{"refresh_token":"x"}"#, "refresh_token"),
        ] {
            let hits = scan_sensitive_keys(payload);
            assert!(
                hits.contains(expected),
                "`{payload}` 必须命中 `{expected}`（D172）：{hits:?}"
            );
        }
    }

    /// D172：**非子串匹配这条纪律不能破** —— 拆词后 `secretary` / `tokenizer` 仍不命中。
    #[test]
    fn the_camel_case_rule_does_not_break_the_exact_match_rule() {
        for payload in [
            r#"{"secretary":"x"}"#,
            r#"{"tokenizer":"y"}"#,
            r#"{"apiKey":"x"}"#,
        ] {
            // `apiKey` 命中，但 `secretary` / `tokenizer` **不得**命中。
            let hits = scan_sensitive_keys(payload);
            if payload.contains("secretary") || payload.contains("tokenizer") {
                assert!(hits.is_empty(), "`{payload}` 不得命中：{hits:?}");
            }
        }
    }

    /// D172：归一化规则的**逐个**断言（Rust 侧；DB 侧由一致性测试对齐）。
    #[test]
    fn the_normalization_follows_the_pinned_rules() {
        let cases = [
            ("apiKey", "api_key"),
            ("APIKey", "api_key"),
            ("clientSecret", "client_secret"),
            ("api_key", "api_key"),
            ("api-key", "api_key"),
            ("secretary", "secretary"),
            ("tokenizer", "tokenizer"),
            ("password", "password"),
            ("Password", "password"),
            ("PASSWORD", "password"),
            ("access-token", "access_token"),
            ("API_KEY", "api_key"),
            ("idToken", "id_token"),
            ("privateKey", "private_key"),
        ];
        for (input, expected) in cases {
            assert_eq!(
                normalize_sensitive_key(input),
                expected,
                "归一化 `{input}` 必须是 `{expected}`（D172）"
            );
        }
    }

    #[test]
    fn the_scanner_normalizes_case_and_dashes() {
        assert!(scan_sensitive_keys(r#"{"Password":"x"}"#).contains("password"));
        assert!(scan_sensitive_keys(r#"{"PASSWORD":"x"}"#).contains("password"));
        assert!(scan_sensitive_keys(r#"{"api-key":"x"}"#).contains("api_key"));
        assert!(
            scan_sensitive_keys(r#"{"access-token":"x"}"#).contains("access_token"),
            "连字符与下划线等价（D156）"
        );
    }

    #[test]
    fn the_scanner_recurses_into_arrays() {
        let hits = scan_sensitive_keys(r#"[{"password":"x"}]"#);
        assert_eq!(hits.len(), 1);
        assert!(hits.contains("password"));
    }

    #[test]
    fn every_baseline_key_is_detected() {
        for key in SENSITIVE_KEYS {
            let payload = format!(r#"{{"{key}":"x"}}"#);
            let hits = scan_sensitive_keys(&payload);
            assert_eq!(hits.len(), 1, "基线键 `{key}` 必须恰好命中它自己：{hits:?}");
        }
    }

    #[test]
    fn invalid_json_yields_no_hits() {
        assert!(scan_sensitive_keys("not json at all").is_empty());
    }

    /// 规范化：递归键排序 + 紧凑 + 非 ASCII 不转义（D142 §2）。
    #[test]
    fn canonical_json_sorts_keys_recursively() {
        let value = json!({"z": {"b": 1, "a": 2}, "a": [3, 1], "m": 0});
        assert_eq!(
            canonical_json(&value),
            r#"{"a":[3,1],"m":0,"z":{"a":2,"b":1}}"#
        );
    }

    #[test]
    fn canonical_json_is_stable_regardless_of_input_order() {
        let left = canonical_json(&json!({"b": 1, "a": 2}));
        let right = canonical_json(&json!({"a": 2, "b": 1}));
        assert_eq!(left, right);
    }

    #[test]
    fn canonical_json_keeps_non_ascii_unescaped() {
        assert_eq!(
            canonical_json(&json!({"名字": "中文"})),
            r#"{"名字":"中文"}"#
        );
    }

    #[test]
    fn canonical_json_escapes_control_characters() {
        assert_eq!(canonical_json(&json!({"a": "x\ny"})), r#"{"a":"x\ny"}"#);
        assert_eq!(
            canonical_json(&json!({"a": "x\u{1}y"})),
            "{\"a\":\"x\\u0001y\"}"
        );
    }

    /// 时间戳文本：**恰好 6 位**微秒 + 字面量 `Z`（D142 §1）。
    #[test]
    fn the_timestamp_text_has_exactly_six_microsecond_digits() {
        let moment = OffsetDateTime::from_unix_timestamp(1_700_000_000).expect("合法时刻");
        let text = format_timestamp_micros_utc(&moment);
        assert_eq!(text.len(), "2023-11-14T22:13:20.000000Z".len());
        assert!(text.ends_with('Z'));
        assert_eq!(&text[19..], ".000000Z", "微秒必须是 6 位：{text}");
    }

    /// 边界：微秒**不为 0** 时也必须是 6 位（`time` 的 `Rfc3339` 在这里会漏掉小数部分）。
    #[test]
    fn the_timestamp_text_keeps_six_digits_when_microseconds_are_not_zero() {
        let moment = OffsetDateTime::from_unix_timestamp(1_700_000_000)
            .expect("合法时刻")
            .replace_microsecond(456_789)
            .expect("合法微秒");
        assert_eq!(
            format_timestamp_micros_utc(&moment),
            "2023-11-14T22:13:20.456789Z"
        );
    }

    #[test]
    fn the_audit_error_codes_are_stable() {
        assert_eq!(
            AuditError::SensitivePayloadRejected {
                keys: vec!["password".to_string()]
            }
            .code(),
            "AUDIT_SENSITIVE_PAYLOAD_REJECTED"
        );
        assert_eq!(
            AuditError::ChainBroken { broken: 1 }.code(),
            "AUDIT_CHAIN_BROKEN"
        );
        assert_eq!(AuditError::EventNotFound.code(), "AUDIT_EVENT_NOT_FOUND");
        assert_eq!(
            AuditError::InvalidFilter {
                reason: "x".to_string()
            }
            .code(),
            "AUDIT_INVALID_FILTER"
        );
        assert_eq!(
            AuditError::SinkUnavailable {
                reason: "x".to_string()
            }
            .code(),
            "AUDIT_SINK_UNAVAILABLE"
        );
    }

    /// 契约 C6：`detail` 必须是英文 ASCII（不得拼中文句子）。
    #[test]
    fn every_audit_error_detail_is_english_ascii() {
        let errors = [
            AuditError::SensitivePayloadRejected {
                keys: vec!["password".to_string()],
            },
            AuditError::ChainBroken { broken: 2 },
            AuditError::EventNotFound,
            AuditError::InvalidFilter {
                reason: "limit must be a positive integer".to_string(),
            },
            AuditError::SinkUnavailable {
                reason: "the database is unreachable".to_string(),
            },
            AuditError::TenantMismatch {
                context: "tenant-a".to_string(),
                event: "tenant-b".to_string(),
            },
        ];
        for error in errors {
            let detail = error.detail();
            assert!(detail.is_ascii(), "detail 必须是 ASCII：{detail}");
            assert!(!detail.is_empty());
        }
    }

    /// 拒绝时**不得**回显敏感值（AC-S-133 步骤 4）。
    #[test]
    fn the_rejection_detail_names_keys_but_not_values() {
        let error = AuditError::SensitivePayloadRejected {
            keys: vec!["password".to_string()],
        };
        assert!(error.detail().contains("password"));
        assert!(!error.detail().contains("p@ssw0rd"));
    }

    #[test]
    fn the_event_builder_fills_the_optional_fields() {
        let event = AuditEvent::new("tenant-a", "demo.item.update", "demo_item")
            .with_actor("u-1")
            .with_object_id("obj-1")
            .with_before(json!({"name": "old"}))
            .with_after(json!({"name": "new"}))
            .with_ip("203.0.113.9")
            .with_user_agent("audit-test-agent");
        assert_eq!(event.tenant_id, "tenant-a");
        assert_eq!(event.actor_id.as_deref(), Some("u-1"));
        assert_eq!(event.object_id.as_deref(), Some("obj-1"));
        assert_eq!(event.ip.as_deref(), Some("203.0.113.9"));
        assert_eq!(event.user_agent.as_deref(), Some("audit-test-agent"));
    }
}
