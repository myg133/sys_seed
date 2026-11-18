//! 同步的**字段级**模型与冲突策略。
//!
//! # 冲突策略默认 `manual`（D8）
//!
//! | 策略 | 行为 |
//! |------|------|
//! | `manual` | 进冲突队列，等人工裁决（**默认**） |
//! | `local_wins` | 底座为准，外部改动被忽略（但**记录**下来） |
//! | `remote_wins` | 外部为准，覆盖底座（危险：必须显式配置） |
//!
//! **绝不静默覆盖** —— 冲突队列记录到**字段**级
//! （`field` / `local_value` / `remote_value`），不是一个字段不同就人工处理整条记录。

use platform_core::RequestContext;
use platform_db::DbResult;
use platform_iam::store::{IamStore, UserProfileField, UserRecord};

use crate::ldap::RemoteUser;

/// 冲突处理策略。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ConflictStrategy {
    /// 进冲突队列，等人工裁决（**默认**）。
    #[default]
    Manual,
    /// 底座为准，外部改动被忽略（但记录下来）。
    LocalWins,
    /// 外部为准，覆盖底座（**危险**：必须显式配置）。
    RemoteWins,
}

impl ConflictStrategy {
    /// 稳定的字符串形式（配置与 API 都用它）。
    pub fn as_str(&self) -> &'static str {
        match self {
            ConflictStrategy::Manual => "manual",
            ConflictStrategy::LocalWins => "local_wins",
            ConflictStrategy::RemoteWins => "remote_wins",
        }
    }

    /// 从字符串解析（未知值一律拒绝 —— 不静默兜底成 `manual`）。
    pub fn parse(value: &str) -> Option<ConflictStrategy> {
        match value.trim() {
            "manual" => Some(ConflictStrategy::Manual),
            "local_wins" => Some(ConflictStrategy::LocalWins),
            "remote_wins" => Some(ConflictStrategy::RemoteWins),
            _ => None,
        }
    }
}

/// 可同步的**用户字段**（字段级冲突的粒度）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncField {
    /// 登录名。
    Username,
    /// 邮箱。
    Email,
    /// 显示名。
    DisplayName,
    /// 部门（对应组织节点；裁决要**搬成员**，不是改用户表）。
    Department,
}

impl SyncField {
    /// 用户表上可同步的字段（部门单独处理 —— 它落在成员关系上）。
    pub fn all() -> [SyncField; 3] {
        [
            SyncField::Username,
            SyncField::Email,
            SyncField::DisplayName,
        ]
    }

    /// 稳定的字段名（冲突队列里的 `field`）。
    pub fn as_str(&self) -> &'static str {
        match self {
            SyncField::Username => "username",
            SyncField::Email => "email",
            SyncField::DisplayName => "display_name",
            SyncField::Department => "department",
        }
    }

    /// 从字段名解析。
    pub fn parse(value: &str) -> Option<SyncField> {
        match value.trim() {
            "username" => Some(SyncField::Username),
            "email" => Some(SyncField::Email),
            "display_name" => Some(SyncField::DisplayName),
            "department" => Some(SyncField::Department),
            _ => None,
        }
    }

    /// 本地值（取自账号表）。
    pub fn local_value(&self, user: &UserRecord) -> String {
        match self {
            SyncField::Username => user.username.clone(),
            SyncField::Email => user.email.clone().unwrap_or_default(),
            SyncField::DisplayName => user.display_name.clone().unwrap_or_default(),
            SyncField::Department => String::new(),
        }
    }

    /// 远端值（取自目录条目）。
    pub fn remote_value(&self, remote: &RemoteUser) -> String {
        match self {
            SyncField::Username => remote.username.clone(),
            SyncField::Email => remote.email.clone().unwrap_or_default(),
            SyncField::DisplayName => remote.display_name.clone().unwrap_or_default(),
            SyncField::Department => remote.department.clone().unwrap_or_default(),
        }
    }

    /// 把一个值写到本地（**白名单**：只认已知的三个用户字段）。
    pub async fn apply(
        &self,
        iam: &IamStore,
        ctx: &RequestContext,
        user_id: &str,
        value: &str,
    ) -> DbResult<()> {
        let field = match self {
            SyncField::Username => UserProfileField::Username,
            SyncField::Email => UserProfileField::Email,
            SyncField::DisplayName => UserProfileField::DisplayName,
            SyncField::Department => {
                // 部门不是账号字段 —— 调用方（服务层）另行处理成员迁移。
                return Ok(());
            }
        };
        iam.update_user_profile_field(ctx, user_id, field, Some(value))
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_strategy_is_manual() {
        assert_eq!(ConflictStrategy::default(), ConflictStrategy::Manual);
    }

    #[test]
    fn strategies_round_trip_through_their_names() {
        for strategy in [
            ConflictStrategy::Manual,
            ConflictStrategy::LocalWins,
            ConflictStrategy::RemoteWins,
        ] {
            assert_eq!(ConflictStrategy::parse(strategy.as_str()), Some(strategy));
        }
    }

    #[test]
    fn an_unknown_strategy_is_rejected_instead_of_defaulting() {
        assert_eq!(ConflictStrategy::parse("whatever"), None);
        assert_eq!(ConflictStrategy::parse(""), None);
    }

    #[test]
    fn fields_round_trip_through_their_names() {
        for field in [
            SyncField::Username,
            SyncField::Email,
            SyncField::DisplayName,
            SyncField::Department,
        ] {
            assert_eq!(SyncField::parse(field.as_str()), Some(field));
        }
        assert_eq!(SyncField::parse("password"), None);
    }

    #[test]
    fn the_department_is_not_a_user_table_field() {
        // 部门落在成员关系上，不在账号表上。
        assert_eq!(SyncField::all().len(), 3);
        assert!(!SyncField::all().contains(&SyncField::Department));
    }
}
