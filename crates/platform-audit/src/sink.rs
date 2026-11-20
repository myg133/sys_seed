//! 审计写入通道（REQ-014）。
//!
//! # 写入**必须**走数据库里的 `SECURITY DEFINER` 函数
//!
//! 应用层「先查最后一条 → 再插入」在并发下有两个请求读到**同一条**最后一条 →
//! 两条新记录拿到**相同的 `prev_hash`** → 链分叉 → 校验必然失败。
//! 所以 `prev_hash` 与 `hash` **由数据库算**（`platform_audit_append`），
//! 应用侧**不**自己算（REQ-014 decisions D161）。
//!
//! ```text
//! PgAuditSink ──► platform_audit_append(...)   （SECURITY DEFINER）
//!                     ├─ 敏感键硬拒绝（D134 / D156）
//!                     ├─ pg_advisory_xact_lock + 新语句重读（D161）
//!                     ├─ clock_timestamp()（D143）
//!                     ├─ 规范化事件体 → sha256（D131 / D142）
//!                     └─ 写 platform_audit_log + 更新链基准（D159）
//! ```
//!
//! # 租户的**唯一来源**是请求上下文（契约 C3）
//!
//! [`AuditEvent`] 里也带 `tenant_id`，但那只是**声明** —— sink 以**上下文**为准，
//! 两者不一致时**报错**（[`AuditError::TenantMismatch`]），
//! 绝不静默取其中一个（静默取一个 = 把「写错租户」变成看不见的事故）。
//!
//! # IP / UA 的取值顺序
//!
//! 事件里显式给了就用事件给的；否则回落到**请求上下文**的
//! `client_ip` / `user_agent`（D140：上下文是稳定取值来源；D149：中间件填）。
//! 两者都没有 → 落**空串**（不是 `NULL`，AC-S-122 要求不得缺列）。

use platform_core::RequestContext;
use platform_core::extension::audit::{
    AuditError, AuditEvent, AuditSink, BoxFuture, canonical_json, scan_sensitive_keys,
};
use platform_db::{TenantDb, TenantQuery};

/// `platform_audit_append` 的返回值（新记录的 `id`）。
struct AppendedId {
    id: String,
}

platform_db::tenant_row!(AppendedId {
    id: "platform_audit_append",
});

/// 审计落点的 PostgreSQL 实现。
#[derive(Debug, Clone)]
pub struct PgAuditSink {
    db: TenantDb,
}

impl PgAuditSink {
    /// 用已有的租户作用域数据入口构造。
    pub fn new(db: TenantDb) -> Self {
        Self { db }
    }

    /// 记录一个事件，返回新记录的 `id`。
    ///
    /// # 失败语义
    ///
    /// - 敏感键 → [`AuditError::SensitivePayloadRejected`]（**硬拒绝**，不落库）；
    /// - 数据库不可用 / 函数报错 → [`AuditError::SinkUnavailable`]
    ///   （调用方据此**拒绝**这次操作 —— 「宁可不登录，不可无审计地登录」，D157）。
    pub async fn record_event(
        &self,
        ctx: &RequestContext,
        event: &AuditEvent,
    ) -> Result<String, AuditError> {
        let tenant = ctx.tenant_id().ok_or_else(|| AuditError::SinkUnavailable {
            reason: "the request context has no tenant".to_string(),
        })?;
        if event.tenant_id != tenant {
            return Err(AuditError::TenantMismatch {
                context: tenant.to_string(),
                event: event.tenant_id.clone(),
            });
        }

        // 写入侧规范化（键排序 + 紧凑 + 非 ASCII 不转义，D142）。
        let before = event.before.as_ref().map(canonical_json);
        let after = event.after.as_ref().map(canonical_json);

        // Rust 侧的**提前拦截**（DB 侧仍会再查一次 —— 那边才是权威）。
        let mut sensitive = scan_sensitive_keys(before.as_deref().unwrap_or(""));
        sensitive.extend(scan_sensitive_keys(after.as_deref().unwrap_or("")));
        if !sensitive.is_empty() {
            return Err(AuditError::SensitivePayloadRejected {
                keys: sensitive.into_iter().collect(),
            });
        }

        // IP / UA 的**稳定取值来源**是请求上下文（D140 / D149）：
        // 事件里显式给了就用事件给的，否则回落到上下文的值。
        let ip = event
            .ip
            .clone()
            .or_else(|| ctx.client_ip().map(|value| value.to_string()))
            .unwrap_or_default();
        let user_agent = event
            .user_agent
            .clone()
            .or_else(|| ctx.user_agent().map(|value| value.to_string()))
            .unwrap_or_default();

        let mut tx = self
            .db
            .begin(ctx)
            .await
            .map_err(|error| AuditError::SinkUnavailable {
                reason: format!("the audit transaction could not be started: {error}"),
            })?;

        let query =
            TenantQuery::new("SELECT platform_audit_append({tenant}, ?, ?, ?, ?, ?, ?, ?, ?)")
                .bind(event.actor_id.clone())
                .bind(event.action.clone())
                .bind(event.object_type.clone())
                .bind(event.object_id.clone())
                .bind(before)
                .bind(after)
                .bind(ip)
                .bind(user_agent);

        let appended: AppendedId =
            tx.fetch_one(&query)
                .await
                .map_err(|error| AuditError::SinkUnavailable {
                    reason: format!("the audit event could not be appended: {error}"),
                })?;

        tx.commit()
            .await
            .map_err(|error| AuditError::SinkUnavailable {
                reason: format!("the audit transaction could not be committed: {error}"),
            })?;

        Ok(appended.id)
    }
}

impl AuditSink for PgAuditSink {
    fn record<'a>(
        &'a self,
        ctx: &'a RequestContext,
        event: &'a AuditEvent,
    ) -> BoxFuture<'a, Result<(), AuditError>> {
        Box::pin(async move { self.record_event(ctx, event).await.map(|_| ()) })
    }
}
