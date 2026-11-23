//! 审计链校验（REQ-014 AC-S-022 / AC-S-124 / AC-S-125）。
//!
//! # 两段式校验（D146）
//!
//! | 段 | 判据 | reason |
//! |----|------|--------|
//! | 逐条独立重算 | 用**本条自己的 `prev_hash` 列** + 本条内容重算 `hash`，与**存储值**比 | `hash_mismatch` |
//! | 接续校验 | 本条的 `prev_hash` 与**同租户上一条的存储 `hash`** 比 | `prev_hash_mismatch` |
//!
//! 两个 reason **都有真实触发路径**（没有死值）：
//!
//! - 改 `after` → 第 2 条 `hash_mismatch`；
//! - 改**存储的 `hash`** → 第 2 条 `hash_mismatch` **且** 第 3 条 `prev_hash_mismatch`
//!   （第 3 条自己没被改 → 它**不是** `hash_mismatch`，断掉的是**接续关系**）。
//!
//! # 复算口径（D141 / D151）
//!
//! - `before` / `after` **直接用列里存的那段文本** —— **不**解析回 JSON 再序列化
//!   （那是「验证生产代码的拷贝」，且会因序列化差异假红）；
//! - `len(f)` 是**字节长度**；
//! - `prev_hash` 以 **64 字节文本**参与拼接（不是 32 字节二进制）；
//! - `created_at` 是 **UTC、RFC3339、微秒 6 位**（`format_timestamp_micros_utc`）。
//!
//! # 链基准（D159）与**链尖端**（D171）
//!
//! 在线表的**第一条**的 `prev_hash` 要与**链基准**（`head_hash`）比对，而不是与
//! 「上一条」比对 —— 归档后上一条已经搬走了。基准存在 `platform_audit_chain_state` 里：
//! genesis 或上次归档那段的尾 `hash`。
//!
//! 🔑 **只有基准时末尾截断检不出来**（D171）：链内**相邻**断裂仍能检出，但
//! 「尾巴被砍了」查不出来 —— 因为链尖不落库。
//! → 链尖落在同表的 `tip_hash` 列（**写入时**更新）：校验时**在线表最后一条**的
//! `hash` 必须 == `tip_hash`，否则「尾巴被砍了」→ 记一条 `hash_mismatch`。

use platform_core::RequestContext;
use platform_core::extension::audit::{AuditError, format_timestamp_micros_utc};
use platform_db::{TenantDb, TenantQuery};
use sha2::{Digest, Sha256};
use time::OffsetDateTime;

/// 链断裂的原因（**恰好两个**，D146）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrokenReason {
    /// 逐条独立重算与存储值不符。
    HashMismatch,
    /// 与同租户上一条的接续关系断了。
    PrevHashMismatch,
}

impl BrokenReason {
    /// 稳定的名字（响应体与日志都用它）。
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::HashMismatch => "hash_mismatch",
            Self::PrevHashMismatch => "prev_hash_mismatch",
        }
    }
}

/// 一条断裂记录。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrokenEntry {
    /// 断裂记录的 `id`（= 表列 `id`，告警日志里的 `event_id`，D164）。
    ///
    /// ⚠️ **末尾截断**（D171）且在线表**已被搬空**时**没有**对应的行 → 此时是**空串**
    /// （表示「在线段整段不见了」，而不是「某一行坏了」）。
    pub id: String,
    /// 断裂原因。
    pub reason: BrokenReason,
    /// 期望值（重算出的哈希 / 期望的 `prev_hash` / 链尖）。
    pub expected: String,
    /// 实际值（存储的哈希 / 实际的 `prev_hash` / 在线表的实际尾）。
    pub actual: String,
}

/// 链校验结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifyReport {
    /// 链是否完好。
    pub ok: bool,
    /// 检查了多少条。
    pub checked: usize,
    /// 断裂清单（**恰好**列出断裂的那些条）。
    pub broken: Vec<BrokenEntry>,
}

/// 参与哈希的字段（**固定顺序**，D131）。
///
/// 字段顺序**不可换** —— 换了就与数据库算出来的哈希对不上。
#[derive(Debug, Clone, Copy)]
pub struct HashInput<'a> {
    /// 上一条的哈希（genesis 是 64 个 `0`）。
    pub prev_hash: &'a str,
    /// 租户。
    pub tenant_id: &'a str,
    /// 操作人（无则空串）。
    pub actor_id: &'a str,
    /// 动作名。
    pub action: &'a str,
    /// 对象类型。
    pub object_type: &'a str,
    /// 对象 ID（无则空串）。
    pub object_id: &'a str,
    /// 变更前（**列里存的那段文本**；`NULL` 用空串）。
    pub before: &'a str,
    /// 变更后（**列里存的那段文本**；`NULL` 用空串）。
    pub after: &'a str,
    /// 来源 IP。
    pub ip: &'a str,
    /// 设备。
    pub user_agent: &'a str,
    /// 规范时间戳文本（UTC、微秒 6 位）。
    pub created_at: &'a str,
}

/// 按 D131 的规则算哈希：`sha256(prev_hash + 长度前缀拼接的固定 10 字段)`。
///
/// ⚠️ `len(f)` 是**字节长度**；`prev_hash` 以 **64 字节文本**参与拼接。
pub fn compute_hash(input: &HashInput<'_>) -> String {
    let mut buffer: Vec<u8> = Vec::with_capacity(512);
    buffer.extend_from_slice(input.prev_hash.as_bytes());
    for field in [
        input.tenant_id,
        input.actor_id,
        input.action,
        input.object_type,
        input.object_id,
        input.before,
        input.after,
        input.ip,
        input.user_agent,
        input.created_at,
    ] {
        buffer.extend_from_slice(field.len().to_string().as_bytes());
        buffer.push(b':');
        buffer.extend_from_slice(field.as_bytes());
    }
    let digest = Sha256::digest(&buffer);
    digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>()
}

/// `platform_audit_head_hash` 的返回值（链基准）。
struct HeadHash {
    value: String,
}

platform_db::tenant_row!(HeadHash {
    value: "platform_audit_head_hash",
});

/// `platform_audit_tip_hash` 的返回值（链尖端，D171）。
///
/// ⚠️ **没有链状态记录时是 `None`**（不是 genesis）——
/// 「该租户从没走过平台写入通道」与「链尖恰好是 genesis」是**两件事**：
/// 前者**无法**做末尾截断判定（没有基准可比），后者**能**。
struct TipHash {
    value: Option<String>,
}

platform_db::tenant_row!(TipHash {
    value: "platform_audit_tip_hash",
});

/// 链上的一条记录（校验视角）。
#[derive(Debug, Clone, PartialEq, Eq)]
struct ChainRow {
    id: String,
    tenant_id: String,
    actor_id: Option<String>,
    action: String,
    object_type: String,
    object_id: Option<String>,
    before: Option<String>,
    after: Option<String>,
    ip: String,
    user_agent: String,
    created_at: OffsetDateTime,
    prev_hash: String,
    hash: String,
}

platform_db::tenant_row!(ChainRow {
    id: "id",
    tenant_id: "tenant_id",
    actor_id: "actor_id",
    action: "action",
    object_type: "object_type",
    object_id: "object_id",
    before: "before",
    after: "after",
    ip: "ip",
    user_agent: "user_agent",
    created_at: "created_at",
    prev_hash: "prev_hash",
    hash: "hash",
});

/// 校验时**每批**读取的条数（TD-084）。
///
/// 1000 条的载荷约 0.6MB（按 5 万条 29MB 折算）—— 远低于「整条链进内存」的量级，
/// 又足够大让往返次数可忽略。
///
/// ⚠️ 不要调得太大：这个常量的全部意义就是**限制峰值内存**。
const VERIFY_BATCH_SIZE: i64 = 1000;

/// 链校验服务。
#[derive(Debug, Clone)]
pub struct ChainVerifier {
    db: TenantDb,
    /// 每批读取条数（默认 [`VERIFY_BATCH_SIZE`]）。
    ///
    /// ⚠️ 可调**只**为了测试能构造「多批」场景：
    /// 默认 1000 意味着要用 1000+ 条真实审计记录才能跨批，
    /// 那会让测试跑很久。生产不要改它。
    batch_size: i64,
}

impl ChainVerifier {
    /// 用已有的租户作用域数据入口构造。
    pub fn new(db: TenantDb) -> Self {
        Self {
            db,
            batch_size: VERIFY_BATCH_SIZE,
        }
    }

    /// **仅供测试**：改每批条数，用来构造「多批」场景。
    ///
    /// ⚠️ 生产不要调用 —— 批量大小的意义就是限制峰值内存，
    /// 由 [`VERIFY_BATCH_SIZE`] 统一决定。
    #[doc(hidden)]
    pub fn with_batch_size(mut self, batch_size: i64) -> Self {
        self.batch_size = batch_size.max(1);
        self
    }

    /// 校验当前租户的审计链。
    pub async fn verify(&self, ctx: &RequestContext) -> Result<VerifyReport, AuditError> {
        let mut tx = self.db.begin(ctx).await.map_err(unavailable)?;

        // 链基准（genesis 或上次归档的尾 hash）。
        let baseline: HeadHash = tx
            .fetch_one(&TenantQuery::new(
                "SELECT platform_audit_head_hash({tenant})",
            ))
            .await
            .map_err(unavailable)?;

        // 链尖端（D171）—— 没有链状态记录时是 `None`（无法做末尾截断判定）。
        let tip: TipHash = tx
            .fetch_one(&TenantQuery::new(
                "SELECT platform_audit_tip_hash({tenant})",
            ))
            .await
            .map_err(unavailable)?;

        // -- 按**批次**流式校验（TD-084）--------------------------------------
        //
        // # ⚠️ 为什么不一次 `fetch_all` 全部
        //
        // 原实现把整条在线链读进一个 `Vec` 再逐条比对。5 万条约 **29MB** 载荷，
        // 而 `GET /api/audit/verify` 是**用户可反复调用**的接口、无并发上限
        // → 几个并发请求就能把内存打满。
        //
        // → 改成 **keyset 分批**：每批 `VERIFY_BATCH_SIZE` 条，批间用
        // `(created_at, id) > (上一批最后一条)` 推进，`previous_hash` 跨批保持。
        //
        // # ⚠️ 为什么仍在**同一个事务**里
        //
        // 分批读必须看到**一致的快照** —— 否则「批 2 读到批 1 之后新插入的行」
        // 会让链校验出现假的 `prev_hash_mismatch`。
        // 单事务（默认 `READ COMMITTED` 下同一条语句的快照）保证批间一致；
        // 且整个校验期间持有事务，与 `append` 的 advisory lock 不冲突
        //（锁是 `xact` 级的，本函数只读）。
        let mut broken: Vec<BrokenEntry> = Vec::new();
        let mut previous_hash = baseline.value.clone();
        let mut checked: usize = 0;
        // 最后一条 (id, hash) —— 用于末尾截断判定（替代原来 `rows.last()`）。
        let mut last_seen: Option<(String, String)> = None;
        // keyset 游标：(created_at, id)。
        let mut cursor: Option<(time::OffsetDateTime, String)> = None;

        loop {
            let (query, rows): (platform_db::TenantQuery, Vec<ChainRow>) = match &cursor {
                None => {
                    let query = platform_db::TenantQuery::new(
                        "SELECT id, tenant_id, actor_id, action, object_type, object_id, \"before\", \
                         \"after\", ip, user_agent, created_at, prev_hash, hash \
                         FROM platform_audit_log WHERE tenant_id = {tenant} \
                         ORDER BY created_at ASC, id ASC LIMIT ?",
                    )
                    .bind(self.batch_size);
                    let rows = tx.fetch_all(&query).await.map_err(unavailable)?;
                    (query, rows)
                }
                Some((created_at, id)) => {
                    let query = platform_db::TenantQuery::new(
                        "SELECT id, tenant_id, actor_id, action, object_type, object_id, \"before\", \
                         \"after\", ip, user_agent, created_at, prev_hash, hash \
                         FROM platform_audit_log WHERE tenant_id = {tenant} \
                           AND (created_at, id) > (?, ?) \
                         ORDER BY created_at ASC, id ASC LIMIT ?",
                    )
                    .bind(*created_at)
                    .bind(id.clone())
                    .bind(self.batch_size);
                    let rows = tx.fetch_all(&query).await.map_err(unavailable)?;
                    (query, rows)
                }
            };
            // `query` 只是为了让 borrow 活得够久（`fetch_all` 已消费完它）。
            let _ = &query;

            if rows.is_empty() {
                break;
            }

            for row in &rows {
                let created_at = format_timestamp_micros_utc(&row.created_at);
                let recomputed = compute_hash(&HashInput {
                    prev_hash: &row.prev_hash,
                    tenant_id: &row.tenant_id,
                    actor_id: row.actor_id.as_deref().unwrap_or(""),
                    action: &row.action,
                    object_type: &row.object_type,
                    object_id: row.object_id.as_deref().unwrap_or(""),
                    before: row.before.as_deref().unwrap_or(""),
                    after: row.after.as_deref().unwrap_or(""),
                    ip: &row.ip,
                    user_agent: &row.user_agent,
                    created_at: &created_at,
                });
                if recomputed != row.hash {
                    broken.push(BrokenEntry {
                        id: row.id.clone(),
                        reason: BrokenReason::HashMismatch,
                        expected: recomputed,
                        actual: row.hash.clone(),
                    });
                }
                if row.prev_hash != previous_hash {
                    broken.push(BrokenEntry {
                        id: row.id.clone(),
                        reason: BrokenReason::PrevHashMismatch,
                        expected: previous_hash.clone(),
                        actual: row.prev_hash.clone(),
                    });
                }
                previous_hash = row.hash.clone();
            }

            checked += rows.len();
            let last = rows.last().expect("上面已判非空");
            cursor = Some((last.created_at, last.id.clone()));
            last_seen = Some((last.id.clone(), last.hash.clone()));
            // ⚠️ `rows` 在此作用域结束时释放 → 内存占用被限制在一批之内。
            if (rows.len() as i64) < self.batch_size {
                break;
            }
        }

        tx.commit().await.map_err(unavailable)?;

        // -- 末尾截断检测（D171）----------------------------------------------
        //
        // 在线表**最后一条**的 `hash` 必须 == 链尖（`tip_hash`）。
        // 否则 → 「尾巴被砍了」（末尾若干条被删）→ 记一条 `hash_mismatch`。
        //
        // ⚠️ **在线表为空**时，链的「最后一条」= 归档段的尾（`head_hash`）——
        //     全量归档会把 `tip_hash` 重算成 `head_hash`，两者相等 → 不是截断。
        //
        // ⚠️ **没有链状态记录**（`tip` 为 `None`）→ **跳过**判定：
        //     「该租户从没走过平台写入通道」（比如全是管理员直连造的夹具）时
        //     没有链尖可比 → 硬判会把「没有基准」误报成「被截断」。
        if let Some(expected_tip) = tip.value.as_deref() {
            let (last_id, actual_tail) = match &last_seen {
                Some((id, hash)) => (id.clone(), hash.clone()),
                None => (String::new(), baseline.value.clone()),
            };
            if actual_tail != expected_tip {
                broken.push(BrokenEntry {
                    id: last_id,
                    reason: BrokenReason::HashMismatch,
                    expected: expected_tip.to_string(),
                    actual: actual_tail,
                });
            }
        }

        let report = VerifyReport {
            ok: broken.is_empty(),
            checked,
            broken,
        };

        if !report.ok {
            // 告警：**error 级别** + 结构化字段 `tenant_id` / `event_id` / `reason`（D164）。
            for entry in &report.broken {
                tracing::error!(
                    tenant_id = ctx.tenant_id().unwrap_or_default(),
                    event_id = %entry.id,
                    reason = entry.reason.as_str(),
                    expected = %entry.expected,
                    actual = %entry.actual,
                    "the audit chain integrity check found a broken entry"
                );
            }
        }

        Ok(report)
    }
}

fn unavailable(error: impl std::fmt::Display) -> AuditError {
    AuditError::SinkUnavailable {
        reason: format!("the audit chain could not be verified: {error}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 拼接形态的守卫：`len(f)` 是**字节长度**，`prev_hash` 以 **64 字节文本**参与。
    #[test]
    fn the_hash_input_is_length_prefixed() {
        let input = HashInput {
            prev_hash: &"0".repeat(64),
            tenant_id: "tenant-a",
            actor_id: "",
            action: "demo.item.update",
            object_type: "demo_item",
            object_id: "obj-1",
            before: "",
            after: "",
            ip: "",
            user_agent: "",
            created_at: "2026-11-20T01:02:03.000000Z",
        };
        let mut expected: Vec<u8> = Vec::new();
        expected.extend_from_slice("0".repeat(64).as_bytes());
        for field in [
            "tenant-a",
            "",
            "demo.item.update",
            "demo_item",
            "obj-1",
            "",
            "",
            "",
            "",
            "2026-11-20T01:02:03.000000Z",
        ] {
            expected.extend_from_slice(field.len().to_string().as_bytes());
            expected.push(b':');
            expected.extend_from_slice(field.as_bytes());
        }
        let digest = Sha256::digest(&expected);
        let expected_hash: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
        assert_eq!(compute_hash(&input), expected_hash);
    }

    #[test]
    fn the_hash_is_lowercase_hex_of_64_chars() {
        let input = HashInput {
            prev_hash: &"0".repeat(64),
            tenant_id: "tenant-a",
            actor_id: "u-1",
            action: "a",
            object_type: "b",
            object_id: "c",
            before: "",
            after: "",
            ip: "",
            user_agent: "",
            created_at: "2026-11-20T01:02:03.000000Z",
        };
        let hash = compute_hash(&input);
        assert_eq!(hash.len(), 64);
        assert!(
            hash.chars()
                .all(|ch| ch.is_ascii_hexdigit() && !ch.is_ascii_uppercase())
        );
    }

    #[test]
    fn the_reason_names_are_the_two_stable_ones() {
        assert_eq!(BrokenReason::HashMismatch.as_str(), "hash_mismatch");
        assert_eq!(
            BrokenReason::PrevHashMismatch.as_str(),
            "prev_hash_mismatch"
        );
    }
}
