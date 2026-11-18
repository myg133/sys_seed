//! 组织架构的领域实体。
//!
//! # 边界（REQ-008 D11）
//!
//! 本模块**只提供结构**，**不实现任何权限判定逻辑** ——
//! 「本部门及下级」「我的下属」这类判定属于 REQ-009。
//! 因此本文件里没有任何「能不能访问」「有没有权限」的表达。

use std::collections::BTreeMap;

/// 组织节点类型（D2）。
///
/// **岗位（position）不在这里** —— 岗位是挂在部门上的属性（[`UserPosition`]），
/// 不是组织层级的一层。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum NodeType {
    /// 公司（顶层）。
    Company,
    /// 部门。
    Dept,
    /// 小组。
    Team,
}

impl NodeType {
    /// 数据库里存的值。
    pub fn as_str(&self) -> &'static str {
        match self {
            NodeType::Company => "company",
            NodeType::Dept => "dept",
            NodeType::Team => "team",
        }
    }

    /// 从数据库的值解析（未知值一律拒绝 —— 不静默兜底成某个类型）。
    pub fn parse(value: &str) -> Option<NodeType> {
        match value {
            "company" => Some(NodeType::Company),
            "dept" => Some(NodeType::Dept),
            "team" => Some(NodeType::Team),
            _ => None,
        }
    }

    /// 全部节点类型（用于枚举校验与文档）。
    pub fn all() -> [NodeType; 3] {
        [NodeType::Company, NodeType::Dept, NodeType::Team]
    }
}

/// 组织节点。
///
/// 结构是**邻接表**（D1）：`parent_id` 是权威来源，
/// `path_labels` 只是**加速字段**（D5），与 `parent_id` 不一致时以 `parent_id` 为准。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrgNode {
    /// 归属租户。
    pub tenant_id: String,
    /// 节点 ID。
    pub id: String,
    /// 父节点 ID（根节点为 `None`）。
    pub parent_id: Option<String>,
    /// 节点类型。
    pub node_type: NodeType,
    /// 节点编码（租户内唯一；同步到第三方时作为稳定标识）。
    pub code: String,
    /// 节点名称。
    pub name: String,
    /// 祖先 ID 列表（从根到本节点，含本节点）—— **加速字段，不是权威来源**。
    pub path_labels: Vec<String>,
}

/// 组织节点的**原始行**（枚举在库里是字符串，实体里是枚举 —— 转换见 [`OrgNode::try_from_row`]）。
pub struct OrgNodeRow {
    /// 归属租户。
    pub tenant_id: String,
    /// 节点 ID。
    pub id: String,
    /// 父节点 ID。
    pub parent_id: Option<String>,
    /// 节点类型（库里的字符串）。
    pub node_type: String,
    /// 节点编码。
    pub code: String,
    /// 节点名称。
    pub name: String,
    /// 祖先链（库里的 JSON 文本）。
    pub path_labels: String,
}

platform_db::tenant_row!(OrgNodeRow {
    tenant_id: "tenant_id",
    id: "id",
    parent_id: "parent_id",
    node_type: "node_type",
    code: "code",
    name: "name",
    path_labels: "path_labels",
});

impl OrgNode {
    /// 从原始行转换（未知节点类型**拒绝**，不静默兜底）。
    pub fn try_from_row(row: OrgNodeRow) -> Option<OrgNode> {
        Some(OrgNode {
            tenant_id: row.tenant_id,
            id: row.id,
            parent_id: row.parent_id,
            node_type: NodeType::parse(&row.node_type)?,
            code: row.code,
            name: row.name,
            path_labels: crate::tree::decode(&row.path_labels),
        })
    }
}

/// 用户 ↔ 节点的成员关系（D3：多对多）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserOrg {
    /// 归属租户。
    pub tenant_id: String,
    /// 关系 ID。
    pub id: String,
    /// 用户 ID。
    pub user_id: String,
    /// 组织节点 ID。
    pub org_node_id: String,
    /// 在该节点上的角色（如「部门经理」；**不是**平台的权限角色）。
    pub role: Option<String>,
    /// 是否主部门（REQ-009 的「主部门判定」用这个字段）。
    pub is_primary: bool,
}

platform_db::tenant_row!(UserOrg {
    tenant_id: "tenant_id",
    id: "id",
    user_id: "user_id",
    org_node_id: "org_node_id",
    role: "role",
    is_primary: "is_primary",
});

/// 岗位（挂在部门上的属性，D2）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserPosition {
    /// 归属租户。
    pub tenant_id: String,
    /// 记录 ID。
    pub id: String,
    /// 用户 ID。
    pub user_id: String,
    /// 岗位编码。
    pub position_code: String,
    /// 岗位挂靠的组织节点（通常是部门）。
    pub org_node_id: Option<String>,
}

platform_db::tenant_row!(UserPosition {
    tenant_id: "tenant_id",
    id: "id",
    user_id: "user_id",
    position_code: "position_code",
    org_node_id: "org_node_id",
});

/// 外部身份映射（SSO / 目录 / SCIM 的本地锚点）。
///
/// 幂等键是 `(provider, external_id)`（D6）—— 同一个外部身份在本租户内只能映射一次。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalIdentity {
    /// 归属租户。
    pub tenant_id: String,
    /// 记录 ID。
    pub id: String,
    /// 来源标识（如 `corp-scim` / `corp-ad`）。
    pub provider: String,
    /// 外部系统里的 ID。
    pub external_id: String,
    /// 本地用户 ID。
    pub user_id: String,
    /// 上次同步时间。
    pub last_synced_at: Option<time::OffsetDateTime>,
}

platform_db::tenant_row!(ExternalIdentity {
    tenant_id: "tenant_id",
    id: "id",
    provider: "provider",
    external_id: "external_id",
    user_id: "user_id",
    last_synced_at: "last_synced_at",
});

/// 同步冲突的处理状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictStatus {
    /// 待人工裁决（默认策略 `manual`）。
    Pending,
    /// 已裁决。
    Resolved,
    /// 外部改动被忽略（策略 `local_wins`；记录下来但不覆盖本地）。
    Ignored,
}

impl ConflictStatus {
    /// 数据库里存的值。
    pub fn as_str(&self) -> &'static str {
        match self {
            ConflictStatus::Pending => "pending",
            ConflictStatus::Resolved => "resolved",
            ConflictStatus::Ignored => "ignored",
        }
    }

    /// 从数据库的值解析。
    pub fn parse(value: &str) -> Option<ConflictStatus> {
        match value {
            "pending" => Some(ConflictStatus::Pending),
            "resolved" => Some(ConflictStatus::Resolved),
            "ignored" => Some(ConflictStatus::Ignored),
            _ => None,
        }
    }
}

/// 人工裁决的结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictResolution {
    /// 保留本地值（外部值丢弃）。
    KeepLocal,
    /// 采用外部值（覆盖本地 —— **必须**是人工显式裁决的结果）。
    TakeRemote,
    /// 人工指定一个值。
    Manual,
}

impl ConflictResolution {
    /// 数据库里存的值。
    pub fn as_str(&self) -> &'static str {
        match self {
            ConflictResolution::KeepLocal => "local",
            ConflictResolution::TakeRemote => "remote",
            ConflictResolution::Manual => "manual",
        }
    }

    /// 从数据库的值解析。
    pub fn parse(value: &str) -> Option<ConflictResolution> {
        match value {
            "local" => Some(ConflictResolution::KeepLocal),
            "remote" => Some(ConflictResolution::TakeRemote),
            "manual" => Some(ConflictResolution::Manual),
            _ => None,
        }
    }
}

/// 同步冲突（**字段级**，D8）。
///
/// 冲突记录到**字段**级而不是整条记录 —— 否则一个字段不同就要人工处理整条记录。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncConflict {
    /// 归属租户。
    pub tenant_id: String,
    /// 冲突 ID。
    pub id: String,
    /// 来源标识（LDAP 源 / SCIM provider）。
    pub source: String,
    /// 外部 ID。
    pub external_id: String,
    /// 冲突字段（如 `email`）。
    pub field: String,
    /// 本地值。
    pub local_value: String,
    /// 外部值。
    pub remote_value: String,
    /// 处理状态。
    pub status: ConflictStatus,
    /// 裁决结果（未裁决时为 `None`）。
    pub resolution: Option<ConflictResolution>,
    /// 裁决人。
    pub resolved_by: Option<String>,
    /// 裁决时间。
    pub resolved_at: Option<time::OffsetDateTime>,
}

/// 同步冲突的**原始行**。
pub struct SyncConflictRow {
    /// 归属租户。
    pub tenant_id: String,
    /// 冲突 ID。
    pub id: String,
    /// 来源标识。
    pub source: String,
    /// 外部 ID。
    pub external_id: String,
    /// 冲突字段。
    pub field: String,
    /// 本地值。
    pub local_value: String,
    /// 外部值。
    pub remote_value: String,
    /// 状态（库里的字符串）。
    pub status: String,
    /// 裁决结果（库里的字符串）。
    pub resolution: Option<String>,
    /// 裁决人。
    pub resolved_by: Option<String>,
    /// 裁决时间。
    pub resolved_at: Option<time::OffsetDateTime>,
}

platform_db::tenant_row!(SyncConflictRow {
    tenant_id: "tenant_id",
    id: "id",
    source: "source",
    external_id: "external_id",
    field: "field",
    local_value: "local_value",
    remote_value: "remote_value",
    status: "status",
    resolution: "resolution",
    resolved_by: "resolved_by",
    resolved_at: "resolved_at",
});

impl SyncConflict {
    /// 从原始行转换（未知状态**拒绝**，不静默兜底）。
    pub fn try_from_row(row: SyncConflictRow) -> Option<SyncConflict> {
        Some(SyncConflict {
            tenant_id: row.tenant_id,
            id: row.id,
            source: row.source,
            external_id: row.external_id,
            field: row.field,
            local_value: row.local_value,
            remote_value: row.remote_value,
            status: ConflictStatus::parse(&row.status)?,
            resolution: row
                .resolution
                .as_deref()
                .and_then(ConflictResolution::parse),
            resolved_by: row.resolved_by,
            resolved_at: row.resolved_at,
        })
    }
}

/// 同步状态（可见性：上次同步时间、同步错误计数）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncSourceState {
    /// 归属租户。
    pub tenant_id: String,
    /// 来源标识。
    pub source: String,
    /// 上次同步时间。
    pub last_sync_at: Option<time::OffsetDateTime>,
    /// 上次同步的错误信息。
    pub last_error: Option<String>,
    /// 累计错误次数。
    pub error_count: i32,
}

platform_db::tenant_row!(SyncSourceState {
    tenant_id: "tenant_id",
    source: "source",
    last_sync_at: "last_sync_at",
    last_error: "last_error",
    error_count: "error_count",
});

/// 本地用户的**组织侧**属性（用于与外部系统做字段级比对）。
///
/// 这是**视图**，不是权威存储 —— 权威存储是 `platform_iam_user`（REQ-007 的账号表）。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LocalUserProfile {
    /// 用户 ID。
    pub user_id: String,
    /// 登录名。
    pub username: String,
    /// 邮箱。
    pub email: Option<String>,
    /// 显示名。
    pub display_name: Option<String>,
    /// 直属上级（**汇报关系是人的属性**，D3）。
    pub manager_id: Option<String>,
    /// 主部门（主成员关系的节点编码）。
    pub primary_department: Option<String>,
    /// 账号是否已禁用。
    pub disabled: bool,
}

/// 一次同步的产出（给上层汇总用）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SyncSummary {
    /// 拉取到的条目数。
    pub fetched: usize,
    /// 新建的用户数。
    pub created: usize,
    /// 更新过的用户数。
    pub updated: usize,
    /// 发现的冲突数（字段级）。
    pub conflicts: usize,
    /// 触发的离职禁用数。
    pub departures: usize,
    /// 忽略的条目数。
    pub skipped: usize,
}

impl SyncSummary {
    /// 汇总成可读的明细（用于通知与日志）。
    pub fn to_payload(&self) -> BTreeMap<String, String> {
        let mut payload = BTreeMap::new();
        payload.insert("fetched".to_string(), self.fetched.to_string());
        payload.insert("created".to_string(), self.created.to_string());
        payload.insert("updated".to_string(), self.updated.to_string());
        payload.insert("conflicts".to_string(), self.conflicts.to_string());
        payload.insert("departures".to_string(), self.departures.to_string());
        payload.insert("skipped".to_string(), self.skipped.to_string());
        payload
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_types_round_trip_through_the_database_representation() {
        for kind in NodeType::all() {
            assert_eq!(NodeType::parse(kind.as_str()), Some(kind));
        }
    }

    #[test]
    fn unknown_node_types_are_rejected_instead_of_defaulting() {
        assert_eq!(NodeType::parse("position"), None);
        assert_eq!(NodeType::parse("Position"), None);
        assert_eq!(NodeType::parse(""), None);
    }

    #[test]
    fn positions_are_not_an_organization_node_type() {
        // D2：岗位挂在部门上，不是组织层级的一种。
        assert!(
            NodeType::all()
                .iter()
                .all(|kind| kind.as_str() != "position")
        );
    }

    #[test]
    fn conflict_status_and_resolution_round_trip() {
        for status in [
            ConflictStatus::Pending,
            ConflictStatus::Resolved,
            ConflictStatus::Ignored,
        ] {
            assert_eq!(ConflictStatus::parse(status.as_str()), Some(status));
        }
        for resolution in [
            ConflictResolution::KeepLocal,
            ConflictResolution::TakeRemote,
            ConflictResolution::Manual,
        ] {
            assert_eq!(
                ConflictResolution::parse(resolution.as_str()),
                Some(resolution)
            );
        }
        assert_eq!(ConflictStatus::parse("done"), None);
        assert_eq!(ConflictResolution::parse("whatever"), None);
    }

    #[test]
    fn sync_summary_payload_exposes_every_counter() {
        let summary = SyncSummary {
            fetched: 3,
            created: 1,
            updated: 2,
            conflicts: 4,
            departures: 5,
            skipped: 6,
        };
        let payload = summary.to_payload();
        assert_eq!(payload.get("fetched").map(String::as_str), Some("3"));
        assert_eq!(payload.get("conflicts").map(String::as_str), Some("4"));
        assert_eq!(payload.get("departures").map(String::as_str), Some("5"));
    }
}
