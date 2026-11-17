//! 平台事务助手：C-4「事务边界」的声明通道。
//!
//! # 定位
//!
//! 本期平台**没有 DB 访问层**（sqlx 刻意推到 REQ-006/007），因此这里不提供连接、
//! 不提供 `begin()` / `commit()`，只承担**声明**职责：让模块能说清楚
//! 「我的写操作涉及哪几张表、是不是经由平台事务助手执行」。
//! 契约检查 C-4 据此判定「跨表写必须显式开事务」。
//!
//! # 与 5 个扩展点的关系
//!
//! [`TransactionalWrite`] **不是**首批 5 个扩展点之一（那 5 个是业务模块实现的接缝），
//! 而是平台为 C-4 提供的能力声明面。REQ-006/007 落地 DB 访问层后，
//! 本模块会补上真正的事务助手实现，trait 签名保持不变。

/// 写边界声明。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriteBoundary {
    /// 写操作涉及的表。
    pub tables: Vec<String>,
    /// 写操作是否经由平台事务助手执行。
    pub uses_platform_transaction: bool,
}

impl WriteBoundary {
    /// 构造写边界声明。
    pub fn new(
        tables: impl IntoIterator<Item = impl Into<String>>,
        uses_platform_transaction: bool,
    ) -> Self {
        Self {
            tables: tables.into_iter().map(Into::into).collect(),
            uses_platform_transaction,
        }
    }

    /// 是否跨表写（跨表写必须显式开事务）。
    pub fn is_cross_table(&self) -> bool {
        self.tables.len() > 1
    }
}

/// 写边界声明扩展点。
pub trait TransactionalWrite: Send + Sync {
    /// 本模块的写边界声明。
    fn write_boundary(&self) -> WriteBoundary;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cross_table_is_detected_by_table_count() {
        let single = WriteBoundary::new(["demo_item"], false);
        assert!(!single.is_cross_table());

        let cross = WriteBoundary::new(["demo_item", "demo_log"], true);
        assert!(cross.is_cross_table());
        assert!(cross.uses_platform_transaction);
    }

    #[test]
    fn write_boundary_is_readable_through_the_trait() {
        struct Demo;
        impl TransactionalWrite for Demo {
            fn write_boundary(&self) -> WriteBoundary {
                WriteBoundary::new(["demo_item"], false)
            }
        }

        assert_eq!(Demo.write_boundary().tables, vec!["demo_item".to_string()]);
    }
}
