//! 组织架构的业务服务。
//!
//! # 边界（REQ-008 D11）
//!
//! 本服务**只提供结构**，**不实现任何权限判定逻辑** ——
//! 「本部门及下级」「我的下属」是 REQ-009 的判定，本服务只把**判定需要的数据**
//! 组织好（见 [`OrgService::subtree`] / [`OrgService::members_of_subtree`] /
//! `user.manager_id`）。
//!
//! # 事务边界（契约 C4）
//!
//! 跨表写一律开一个事务：移动节点（`org_node` + 子孙的 `path_labels`）、
//! 加成员（`user_org`）、删节点（`user_org` + `org_node`）、裁决冲突
//! （`platform_iam_user` + `sync_conflict`）。
//!
//! # 离职禁用的顺序（D9 / R5）
//!
//! 1. 禁用账号（`status = disabled` + `departed_at`）；
//! 2. **撤销该用户的全部会话与 refresh family** —— 这一步最容易漏：
//!    只禁用了账号，旧 refresh token 还能换新 access token；
//! 3. 解绑全部组织成员关系与岗位；
//! 4. 审计事件；
//! 5. 通知（直属上级 / 相关方）；
//! 6. 出站 SCIM 推送 `active: false`（**入队**，不阻塞）。

use std::collections::BTreeMap;
use std::sync::Arc;

use platform_core::RequestContext;
use platform_iam::store::IamStore;
use uuid::Uuid;

use crate::audit::{AuditRecorder, OrgAuditEvent};
use crate::entity::{
    ConflictResolution, ConflictStatus, ExternalIdentity, NodeType, OrgNode, SyncConflict,
    SyncSourceState, SyncSummary, UserOrg, UserPosition,
};
use crate::error::OrgError;
use crate::ldap::{LdapError, LdapSource, LdapSourceConfig, RemoteUser};
use crate::notify::{Notifier, OrgEvent, OrgEventKind};
use crate::scim::{
    ScimGateway, ScimGroupDraft, ScimJob, ScimJobEntry, ScimJobOutcome, ScimOutbox,
    ScimProviderConfig, ScimUserDraft,
};
use crate::store::{OrgStore, parent_links, stored_paths};
use crate::sync::{ConflictStrategy, SyncField};
use crate::tree;

/// SCIM 全量对账一次最多拉多少个账号（防止把内存吃满）。
pub const USER_LIST_LIMIT: i64 = 10_000;

/// 一条待记录的字段级冲突（内部用）。
#[derive(Debug, Clone, Copy)]
struct ConflictDraft<'a> {
    /// 来源标识。
    source: &'a str,
    /// 外部 ID。
    external_id: &'a str,
    /// 冲突字段。
    field: &'a str,
    /// 本地值。
    local_value: &'a str,
    /// 外部值。
    remote_value: &'a str,
    /// 处理状态。
    status: ConflictStatus,
}

/// 新建组织节点的入参。
#[derive(Debug, Clone)]
pub struct NewNode {
    /// 节点 ID（不给就生成）。
    pub id: Option<String>,
    /// 父节点 ID（`None` = 根节点）。
    pub parent_id: Option<String>,
    /// 节点类型。
    pub node_type: NodeType,
    /// 节点编码（租户内唯一）。
    pub code: String,
    /// 节点名称。
    pub name: String,
}

/// 新增成员关系的入参。
#[derive(Debug, Clone)]
pub struct NewMembership {
    /// 关系 ID（不给就生成）。
    pub id: Option<String>,
    /// 用户 ID。
    pub user_id: String,
    /// 组织节点 ID。
    pub org_node_id: String,
    /// 在该节点上的角色（如「部门经理」；**不是**平台权限角色）。
    pub role: Option<String>,
    /// 是否主部门。
    pub is_primary: bool,
}

/// 新增岗位的入参。
#[derive(Debug, Clone)]
pub struct NewPosition {
    /// 记录 ID（不给就生成）。
    pub id: Option<String>,
    /// 用户 ID。
    pub user_id: String,
    /// 岗位编码。
    pub position_code: String,
    /// 岗位挂靠的组织节点。
    pub org_node_id: Option<String>,
}

/// 离职禁用的结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DepartureOutcome {
    /// 被禁用的用户 ID。
    pub user_id: String,
    /// 撤销的会话数。
    pub sessions_revoked: u64,
    /// 撤销的 refresh token 数。
    pub refresh_tokens_revoked: u64,
    /// 解绑的成员关系数。
    pub memberships_removed: u64,
    /// 解绑的岗位数。
    pub positions_removed: u64,
    /// 是否排了一次 SCIM 出站推送（`active: false`）。
    pub scim_push_queued: bool,
}

/// 组织架构的服务。
#[derive(Clone)]
pub struct OrgService {
    store: OrgStore,
    iam: IamStore,
    notifier: Arc<dyn Notifier>,
    audit: Arc<dyn AuditRecorder>,
    outbox: Arc<ScimOutbox>,
}

impl std::fmt::Debug for OrgService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OrgService")
            .field("store", &self.store)
            .finish_non_exhaustive()
    }
}

impl OrgService {
    /// 构造服务。
    pub fn new(
        store: OrgStore,
        iam: IamStore,
        notifier: Arc<dyn Notifier>,
        audit: Arc<dyn AuditRecorder>,
    ) -> Self {
        Self {
            store,
            iam,
            notifier,
            audit,
            outbox: Arc::new(ScimOutbox::new()),
        }
    }

    /// 用自己的出站队列构造服务（测试与自定义 worker 用）。
    pub fn with_outbox(
        store: OrgStore,
        iam: IamStore,
        notifier: Arc<dyn Notifier>,
        audit: Arc<dyn AuditRecorder>,
        outbox: Arc<ScimOutbox>,
    ) -> Self {
        Self {
            store,
            iam,
            notifier,
            audit,
            outbox,
        }
    }

    /// 出站队列（后台 worker 用）。
    pub fn outbox(&self) -> &Arc<ScimOutbox> {
        &self.outbox
    }

    /// 数据访问层（读多写少的调用方可用）。
    pub fn store(&self) -> &OrgStore {
        &self.store
    }

    // -----------------------------------------------------------------------
    // 组织树
    // -----------------------------------------------------------------------

    /// 新建组织节点。
    pub async fn create_node(
        &self,
        ctx: &RequestContext,
        input: NewNode,
    ) -> Result<OrgNode, OrgError> {
        let code = input.code.trim().to_string();
        if code.is_empty() {
            return Err(OrgError::InvalidArgument {
                field: "code",
                reason: "must not be empty".to_string(),
            });
        }
        let name = input.name.trim().to_string();
        if name.is_empty() {
            return Err(OrgError::InvalidArgument {
                field: "name",
                reason: "must not be empty".to_string(),
            });
        }

        let mut tx = self.store.begin(ctx).await?;
        if OrgStore::find_node_by_code_in(&mut tx, &code)
            .await?
            .is_some()
        {
            return Err(OrgError::OrgCodeDuplicated { code });
        }
        let parent_path = match &input.parent_id {
            None => Vec::new(),
            Some(parent_id) => {
                let parent = OrgStore::find_node_in(&mut tx, parent_id).await?.ok_or(
                    OrgError::NotFound {
                        resource: "org_node",
                    },
                )?;
                parent.path_labels
            }
        };

        let id = input.id.unwrap_or_else(|| Uuid::new_v4().to_string());
        let node = OrgNode {
            tenant_id: ctx.tenant_id().unwrap_or_default().to_string(),
            id: id.clone(),
            parent_id: input.parent_id.clone(),
            node_type: input.node_type,
            code,
            name,
            path_labels: tree::child_path(&parent_path, &id),
        };
        OrgStore::insert_node_in(&mut tx, &node).await?;
        tx.commit().await?;

        self.audit
            .record(
                ctx,
                &OrgAuditEvent::new("org.node.create", "create", "org_node", &node.id)
                    .with_detail(format!("created the node {}", node.code)),
            )
            .await?;
        Ok(node)
    }

    /// 查节点。
    pub async fn get_node(&self, ctx: &RequestContext, id: &str) -> Result<OrgNode, OrgError> {
        self.store
            .find_node(ctx, id)
            .await?
            .ok_or(OrgError::NotFound {
                resource: "org_node",
            })
    }

    /// 列出节点（可按父节点过滤）。
    pub async fn list_nodes(
        &self,
        ctx: &RequestContext,
        parent_id: Option<&str>,
    ) -> Result<Vec<OrgNode>, OrgError> {
        Ok(self.store.list_nodes(ctx, parent_id).await?)
    }

    /// 查某个节点的**子树**（递归 CTE，D1）。
    ///
    /// 移动节点后**立即**反映新结构 —— 子树查询走的是 `parent_id`，
    /// 不依赖 `path_labels` 这个加速字段。
    pub async fn subtree(
        &self,
        ctx: &RequestContext,
        root_id: &str,
    ) -> Result<Vec<OrgNode>, OrgError> {
        let nodes = self.store.subtree(ctx, root_id).await?;
        if nodes.is_empty() {
            return Err(OrgError::NotFound {
                resource: "org_node",
            });
        }
        Ok(nodes)
    }

    /// 查某个子树的**全部成员**（REQ-009 的「本部门及下级」判定所需）。
    pub async fn members_of_subtree(
        &self,
        ctx: &RequestContext,
        root_id: &str,
    ) -> Result<Vec<UserOrg>, OrgError> {
        let nodes = self.subtree(ctx, root_id).await?;
        let ids: Vec<String> = nodes.into_iter().map(|node| node.id).collect();
        Ok(self.store.members_of_nodes(ctx, &ids).await?)
    }

    /// 重命名节点。
    pub async fn rename_node(
        &self,
        ctx: &RequestContext,
        id: &str,
        name: &str,
    ) -> Result<OrgNode, OrgError> {
        let name = name.trim().to_string();
        if name.is_empty() {
            return Err(OrgError::InvalidArgument {
                field: "name",
                reason: "must not be empty".to_string(),
            });
        }
        let node = self.get_node(ctx, id).await?;
        self.store
            .update_node(ctx, id, node.parent_id.as_deref(), &name, &node.path_labels)
            .await?;
        self.audit
            .record(
                ctx,
                &OrgAuditEvent::new("org.node.rename", "rename", "org_node", id)
                    .with_detail(format!("renamed the node to {name}")),
            )
            .await?;
        self.notify_node_members(ctx, OrgEventKind::NodeRenamed, id, &[("name", &name)])
            .await?;
        self.get_node(ctx, id).await
    }

    /// 移动节点（换父节点）。
    ///
    /// - **子树查询立即反映新结构**（权威是 `parent_id`）；
    /// - `path_labels` 与 `parent_id` 保持一致（不一致时以 `parent_id` 为准）；
    /// - **成员关系不受影响** —— 成员关系挂在节点上，节点搬家成员跟着走。
    pub async fn move_node(
        &self,
        ctx: &RequestContext,
        id: &str,
        new_parent_id: Option<&str>,
    ) -> Result<OrgNode, OrgError> {
        let mut tx = self.store.begin(ctx).await?;
        let node = OrgStore::find_node_in(&mut tx, id)
            .await?
            .ok_or(OrgError::NotFound {
                resource: "org_node",
            })?;

        let parent_path = match new_parent_id {
            None => Vec::new(),
            Some(parent_id) => {
                if parent_id == id {
                    return Err(OrgError::OrgCycle {
                        reason: "a node cannot be moved under itself",
                    });
                }
                let parent = OrgStore::find_node_in(&mut tx, parent_id).await?.ok_or(
                    OrgError::NotFound {
                        resource: "org_node",
                    },
                )?;
                parent.path_labels
            }
        };

        // 环检测：新父节点不能是本节点的子孙。
        let subtree = OrgStore::subtree_in(&mut tx, id).await?;
        if let Some(parent_id) = new_parent_id {
            if subtree.iter().any(|child| child.id == parent_id) {
                return Err(OrgError::OrgCycle {
                    reason: "the target parent is a descendant of the node",
                });
            }
        }

        let new_path = tree::child_path(&parent_path, id);
        OrgStore::update_node_in(&mut tx, id, new_parent_id, &node.name, &new_path).await?;

        // 重算子孙的祖先链（权威是 parent_id，所以逐个按新结构重算）。
        let mut links = parent_links(&subtree);
        links.insert(id.to_string(), new_parent_id.map(str::to_string));
        for child in subtree.iter().filter(|child| child.id != id) {
            let path = subtree_path(child, &links, &new_path);
            OrgStore::update_node_path_in(&mut tx, &child.id, &path).await?;
        }
        tx.commit().await?;

        self.audit
            .record(
                ctx,
                &OrgAuditEvent::new("org.node.move", "move", "org_node", id).with_detail(format!(
                    "moved the node under {}",
                    new_parent_id.unwrap_or("the root")
                )),
            )
            .await?;
        self.notify_node_members(ctx, OrgEventKind::NodeMoved, id, &[])
            .await?;
        self.get_node(ctx, id).await
    }

    /// 删除节点。
    ///
    /// **成员必须先处理**（R4）：要么把成员迁到别的节点（`move_members_to`），
    /// 要么调用方明确知道自己在删什么 —— 两种情况都不允许留下悬空成员关系。
    ///
    /// 有子节点时**拒绝**删除（先删子节点）。
    pub async fn delete_node(
        &self,
        ctx: &RequestContext,
        id: &str,
        move_members_to: Option<&str>,
    ) -> Result<(), OrgError> {
        let mut tx = self.store.begin(ctx).await?;
        let node = OrgStore::find_node_in(&mut tx, id)
            .await?
            .ok_or(OrgError::NotFound {
                resource: "org_node",
            })?;

        let children = OrgStore::count_children_in(&mut tx, id).await?;
        if children > 0 {
            return Err(OrgError::OrgNodeNotEmpty {
                children: children as usize,
            });
        }

        let members = OrgStore::list_user_org_in(&mut tx, None, Some(id)).await?;
        if !members.is_empty() && move_members_to.is_none() {
            return Err(OrgError::OrgNodeHasMembers {
                members: members.len(),
            });
        }
        if let Some(target) = move_members_to {
            if target == id {
                return Err(OrgError::InvalidArgument {
                    field: "moveMembersTo",
                    reason: "must not be the node being deleted".to_string(),
                });
            }
            if OrgStore::find_node_in(&mut tx, target).await?.is_none() {
                return Err(OrgError::NotFound {
                    resource: "org_node",
                });
            }
            // 把成员迁到目标节点（一个事务里完成，不留悬空成员）。
            for member in &members {
                let mut moved = member.clone();
                moved.org_node_id = target.to_string();
                moved.is_primary = false;
                let table = format!("{}.{}", tx.schema(), crate::store::USER_ORG_TABLE);
                let query = platform_db::TenantQuery::new(format!(
                    "UPDATE {table} SET org_node_id = ?, is_primary = FALSE \
                     WHERE tenant_id = {{tenant}} AND id = ?"
                ))
                .bind(target.to_string())
                .bind(member.id.clone());
                tx.execute(&query).await?;
            }
        } else {
            for member in &members {
                OrgStore::delete_user_org_in(&mut tx, &member.id).await?;
            }
        }

        OrgStore::delete_node_in(&mut tx, id).await?;
        tx.commit().await?;

        self.audit
            .record(
                ctx,
                &OrgAuditEvent::new("org.node.delete", "delete", "org_node", id)
                    .with_detail(format!("deleted the node {}", node.code)),
            )
            .await?;

        // 通知：被处理的成员（迁走的或被删掉的）都该知道。
        let recipients: Vec<String> = members
            .iter()
            .map(|member| member.user_id.clone())
            .collect();
        self.notify(
            ctx,
            OrgEvent::new(OrgEventKind::NodeDeleted, recipients)
                .with_payload("orgNodeId", id)
                .with_payload("orgNodeName", &node.name),
        )
        .await?;
        Ok(())
    }

    /// 校验 `path_labels` 与 `parent_id` 是否一致（D5）。
    ///
    /// 返回不一致的节点 ID（空 = 一致）。
    pub async fn verify_path_labels(&self, ctx: &RequestContext) -> Result<Vec<String>, OrgError> {
        let nodes = self.store.all_nodes(ctx).await?;
        let links = parent_links(&nodes);
        let stored = stored_paths(&nodes);
        Ok(tree::inconsistent_nodes(&links, &stored))
    }

    /// 按 `parent_id` **重建**全部节点的 `path_labels`（权威是 `parent_id`，D5）。
    ///
    /// 返回被修正的节点数。
    pub async fn rebuild_path_labels(&self, ctx: &RequestContext) -> Result<usize, OrgError> {
        let nodes = self.store.all_nodes(ctx).await?;
        let links = parent_links(&nodes);
        let mut fixed = 0usize;
        let mut tx = self.store.begin(ctx).await?;
        for node in &nodes {
            let expected = tree::rebuild_path(&node.id, &links);
            if expected != node.path_labels {
                OrgStore::update_node_path_in(&mut tx, &node.id, &expected).await?;
                fixed += 1;
            }
        }
        tx.commit().await?;
        Ok(fixed)
    }

    // -----------------------------------------------------------------------
    // 成员关系
    // -----------------------------------------------------------------------

    /// 把用户加入组织节点。
    pub async fn add_member(
        &self,
        ctx: &RequestContext,
        input: NewMembership,
    ) -> Result<UserOrg, OrgError> {
        if input.user_id.trim().is_empty() {
            return Err(OrgError::InvalidArgument {
                field: "userId",
                reason: "must not be empty".to_string(),
            });
        }
        let mut tx = self.store.begin(ctx).await?;
        let node = OrgStore::find_node_in(&mut tx, &input.org_node_id)
            .await?
            .ok_or(OrgError::NotFound {
                resource: "org_node",
            })?;

        let id = input.id.unwrap_or_else(|| Uuid::new_v4().to_string());
        if input.is_primary {
            // 一个用户只有一个主部门。
            OrgStore::clear_primary_membership_in(&mut tx, &input.user_id, &id).await?;
        }
        let item = UserOrg {
            tenant_id: ctx.tenant_id().unwrap_or_default().to_string(),
            id,
            user_id: input.user_id.clone(),
            org_node_id: input.org_node_id.clone(),
            role: input.role.clone(),
            is_primary: input.is_primary,
        };
        OrgStore::insert_user_org_in(&mut tx, &item).await?;
        tx.commit().await?;

        self.audit
            .record(
                ctx,
                &OrgAuditEvent::new("org.membership.add", "create", "user_org", &item.id)
                    .with_detail(format!("added the user to {}", node.code)),
            )
            .await?;

        let mut recipients = vec![item.user_id.clone()];
        recipients.extend(self.members_of_node(ctx, &node.id).await?);
        self.notify(
            ctx,
            OrgEvent::new(OrgEventKind::MemberAdded, recipients)
                .with_payload("userId", &item.user_id)
                .with_payload("orgNodeId", &node.id)
                .with_payload("orgNodeName", &node.name),
        )
        .await?;
        Ok(item)
    }

    /// 把用户移出组织节点。
    pub async fn remove_member(
        &self,
        ctx: &RequestContext,
        membership_id: &str,
    ) -> Result<(), OrgError> {
        let membership =
            self.store
                .find_user_org(ctx, membership_id)
                .await?
                .ok_or(OrgError::NotFound {
                    resource: "user_org",
                })?;
        let node = self.get_node(ctx, &membership.org_node_id).await.ok();

        self.store.delete_user_org(ctx, membership_id).await?;
        self.audit
            .record(
                ctx,
                &OrgAuditEvent::new("org.membership.remove", "delete", "user_org", membership_id),
            )
            .await?;

        let mut recipients = vec![membership.user_id.clone()];
        if let Some(node) = &node {
            recipients.extend(self.members_of_node(ctx, &node.id).await?);
        }
        self.notify(
            ctx,
            OrgEvent::new(OrgEventKind::MemberRemoved, recipients)
                .with_payload("userId", &membership.user_id)
                .with_payload("orgNodeId", &membership.org_node_id),
        )
        .await?;
        Ok(())
    }

    /// 列出成员关系（可按用户 / 节点过滤）。
    pub async fn list_members(
        &self,
        ctx: &RequestContext,
        user_id: Option<&str>,
        org_node_id: Option<&str>,
    ) -> Result<Vec<UserOrg>, OrgError> {
        Ok(self.store.list_user_org(ctx, user_id, org_node_id).await?)
    }

    /// 某个节点的成员（用户 ID）。
    async fn members_of_node(
        &self,
        ctx: &RequestContext,
        node_id: &str,
    ) -> Result<Vec<String>, OrgError> {
        let members = self.store.list_user_org(ctx, None, Some(node_id)).await?;
        Ok(members.into_iter().map(|item| item.user_id).collect())
    }

    // -----------------------------------------------------------------------
    // 岗位与汇报关系
    // -----------------------------------------------------------------------

    /// 给用户加岗位（岗位挂在部门上，D2）。
    pub async fn add_position(
        &self,
        ctx: &RequestContext,
        input: NewPosition,
    ) -> Result<UserPosition, OrgError> {
        let code = input.position_code.trim().to_string();
        if code.is_empty() {
            return Err(OrgError::InvalidArgument {
                field: "positionCode",
                reason: "must not be empty".to_string(),
            });
        }
        if let Some(node_id) = &input.org_node_id {
            self.get_node(ctx, node_id).await?;
        }
        let item = UserPosition {
            tenant_id: ctx.tenant_id().unwrap_or_default().to_string(),
            id: input.id.unwrap_or_else(|| Uuid::new_v4().to_string()),
            user_id: input.user_id.clone(),
            position_code: code,
            org_node_id: input.org_node_id.clone(),
        };
        self.store.insert_user_position(ctx, &item).await?;
        Ok(item)
    }

    /// 列出岗位（可按用户过滤）。
    pub async fn list_positions(
        &self,
        ctx: &RequestContext,
        user_id: Option<&str>,
    ) -> Result<Vec<UserPosition>, OrgError> {
        Ok(self.store.list_user_position(ctx, user_id).await?)
    }

    /// 设置用户的直属上级（**汇报关系是人的属性**，D3）。
    ///
    /// 这是 REQ-009「我的下属」判定所需的结构 —— **本模块不做判定**。
    pub async fn set_manager(
        &self,
        ctx: &RequestContext,
        user_id: &str,
        manager_id: Option<&str>,
    ) -> Result<(), OrgError> {
        if let Some(manager_id) = manager_id {
            if manager_id == user_id {
                return Err(OrgError::InvalidArgument {
                    field: "managerId",
                    reason: "a user cannot be their own manager".to_string(),
                });
            }
            if self.iam.find_user_by_id(ctx, manager_id).await?.is_none() {
                return Err(OrgError::NotFound { resource: "user" });
            }
        }
        self.iam.set_user_manager(ctx, user_id, manager_id).await?;
        Ok(())
    }

    /// 查用户的直属上级。
    pub async fn manager_of(
        &self,
        ctx: &RequestContext,
        user_id: &str,
    ) -> Result<Option<String>, OrgError> {
        let user = self
            .iam
            .find_user_by_id(ctx, user_id)
            .await?
            .ok_or(OrgError::NotFound { resource: "user" })?;
        Ok(user.manager_id)
    }

    // -----------------------------------------------------------------------
    // 离职自动禁用（D9）
    // -----------------------------------------------------------------------

    /// 离职自动禁用：禁用账号 + **撤销全部会话** + 解绑成员关系 + 审计 + 通知。
    ///
    /// 第 2 步最容易漏：只禁用了账号，旧 refresh token 还能换新 access token
    /// （access TTL 15 分钟，refresh 30 天）—— 必须显式撤销。
    pub async fn disable_departed_user(
        &self,
        ctx: &RequestContext,
        user_id: &str,
        cause: &str,
    ) -> Result<DepartureOutcome, OrgError> {
        let user = self
            .iam
            .find_user_by_id(ctx, user_id)
            .await?
            .ok_or(OrgError::NotFound { resource: "user" })?;

        // 1) 禁用账号。
        self.iam.disable_user(ctx, user_id).await?;

        // 2) **撤销全部会话与 refresh family**（D9 第 2 条）。
        let refresh_tokens_revoked = self.iam.revoke_all_refresh_tokens(ctx, user_id).await?;
        let sessions_revoked = self.iam.revoke_all_sessions(ctx, user_id).await?;

        // 3) 解绑组织成员关系与岗位。
        let memberships_removed = self.store.delete_user_org_by_user(ctx, user_id).await?;
        let positions_removed = self
            .store
            .delete_user_position_by_user(ctx, user_id)
            .await?;

        // 4) 审计事件。
        self.audit
            .record(
                ctx,
                &OrgAuditEvent::new("org.user.departed", "disable", "user", user_id)
                    .with_detail(format!(
                        "disabled the account (cause: {cause}); revoked {sessions_revoked} sessions \
                         and {refresh_tokens_revoked} refresh tokens"
                    )),
            )
            .await?;

        // 5) 通知（该用户 + 其直属上级）。
        let mut recipients = vec![user_id.to_string()];
        if let Some(manager_id) = &user.manager_id {
            recipients.push(manager_id.clone());
        }
        self.notify(
            ctx,
            OrgEvent::new(OrgEventKind::UserDeparted, recipients)
                .with_payload("userId", user_id)
                .with_payload("cause", cause),
        )
        .await?;

        // 6) 出站 SCIM：`active: false`（**入队**，不阻塞本地操作）。
        let scim_push_queued = self.queue_scim_user_active(ctx, user_id, false).await;

        Ok(DepartureOutcome {
            user_id: user_id.to_string(),
            sessions_revoked,
            refresh_tokens_revoked,
            memberships_removed,
            positions_removed,
            scim_push_queued,
        })
    }

    // -----------------------------------------------------------------------
    // 入站同步（LDAP / AD）
    // -----------------------------------------------------------------------

    /// 入站同步：拉取目录条目 → 与本地比对 → 产出冲突 / 更新 / 离职。
    ///
    /// 冲突策略默认 [`ConflictStrategy::Manual`]（D8）：**绝不静默覆盖**。
    pub async fn sync_ldap(
        &self,
        ctx: &RequestContext,
        config: &LdapSourceConfig,
        source: &dyn LdapSource,
        strategy: ConflictStrategy,
    ) -> Result<SyncSummary, OrgError> {
        let state = self.store.get_sync_state(ctx, &config.source).await?;
        let since = state.as_ref().and_then(|item| item.last_sync_at);
        let filter = config.incremental_filter(since);
        let attributes = config.required_attributes();

        let entries = source
            .search(&config.base_dn, &filter, &attributes, since)
            .await
            .map_err(|error: LdapError| OrgError::Internal {
                reason: error.to_string(),
            })?;

        let mut summary = SyncSummary {
            fetched: entries.len(),
            ..SyncSummary::default()
        };

        for entry in entries {
            let remote = match config.map_entry(&entry) {
                Ok(remote) => remote,
                Err(_) => {
                    summary.skipped += 1;
                    continue;
                }
            };
            let outcome = self
                .apply_remote_user(ctx, config, &remote, strategy)
                .await?;
            summary.created += outcome.created;
            summary.updated += outcome.updated;
            summary.conflicts += outcome.conflicts;
            summary.departures += outcome.departures;
            summary.skipped += outcome.skipped;
        }

        // 同步状态可见性（上次同步时间、错误计数）。
        let now = time::OffsetDateTime::now_utc();
        self.store
            .upsert_sync_state(
                ctx,
                &SyncSourceState {
                    tenant_id: ctx.tenant_id().unwrap_or_default().to_string(),
                    source: config.source.clone(),
                    last_sync_at: Some(now),
                    last_error: None,
                    error_count: 0,
                },
            )
            .await?;
        Ok(summary)
    }

    /// 把一条远端用户应用到本地。
    async fn apply_remote_user(
        &self,
        ctx: &RequestContext,
        config: &LdapSourceConfig,
        remote: &RemoteUser,
        strategy: ConflictStrategy,
    ) -> Result<SyncSummary, OrgError> {
        let mut summary = SyncSummary::default();

        // 本地用户：先按外部身份映射，再按登录名。
        let identity = self
            .store
            .find_external_identity(ctx, &config.source, &remote.external_id)
            .await?;
        let user_id = match &identity {
            Some(item) => Some(item.user_id.clone()),
            None => self
                .iam
                .find_user_by_username(ctx, &remote.username)
                .await?
                .map(|user| user.id),
        };

        let Some(user_id) = user_id else {
            // 本地没有对应账号：**不自动开号**（开号策略涉及密码 / SSO 绑定 / 初始状态，
            // 属于账号生命周期的决策，不该由同步顺带决定）。
            summary.skipped += 1;
            return Ok(summary);
        };

        let user = self
            .iam
            .find_user_by_id(ctx, &user_id)
            .await?
            .ok_or(OrgError::NotFound { resource: "user" })?;

        // 逐字段比对（**字段级**冲突）。
        let mut changed = false;
        for field in SyncField::all() {
            let local_value = field.local_value(&user);
            let remote_value = field.remote_value(remote);
            if local_value == remote_value {
                continue;
            }
            match strategy {
                ConflictStrategy::Manual => {
                    self.record_conflict(
                        ctx,
                        ConflictDraft {
                            source: &config.source,
                            external_id: &remote.external_id,
                            field: field.as_str(),
                            local_value: &local_value,
                            remote_value: &remote_value,
                            status: ConflictStatus::Pending,
                        },
                    )
                    .await?;
                    summary.conflicts += 1;
                }
                ConflictStrategy::LocalWins => {
                    // 保留本地值，但**记录**下来（设计摘要 §4.3）。
                    self.record_conflict(
                        ctx,
                        ConflictDraft {
                            source: &config.source,
                            external_id: &remote.external_id,
                            field: field.as_str(),
                            local_value: &local_value,
                            remote_value: &remote_value,
                            status: ConflictStatus::Ignored,
                        },
                    )
                    .await?;
                    summary.conflicts += 1;
                }
                ConflictStrategy::RemoteWins => {
                    // 显式配置才允许覆盖（默认策略不是它）。
                    field.apply(&self.iam, ctx, &user_id, &remote_value).await?;
                    changed = true;
                }
            }
        }

        // 部门：远端说在哪，本地主部门在哪 —— 同样是字段级冲突。
        if let Some(department) = &remote.department {
            let local_department = self.primary_department_code(ctx, &user_id).await?;
            if local_department.as_deref() != Some(department.as_str()) {
                match strategy {
                    ConflictStrategy::RemoteWins => {
                        self.move_primary_department(ctx, &user_id, department)
                            .await?;
                        changed = true;
                    }
                    _ => {
                        self.record_conflict(
                            ctx,
                            ConflictDraft {
                                source: &config.source,
                                external_id: &remote.external_id,
                                field: SyncField::Department.as_str(),
                                local_value: local_department.as_deref().unwrap_or_default(),
                                remote_value: department,
                                status: if strategy == ConflictStrategy::Manual {
                                    ConflictStatus::Pending
                                } else {
                                    ConflictStatus::Ignored
                                },
                            },
                        )
                        .await?;
                        summary.conflicts += 1;
                    }
                }
            }
        }

        // 离职检测（AD 的位运算判定已经在映射层做完了）。
        if remote.disabled && user.status != platform_iam::store::STATUS_DISABLED {
            self.disable_departed_user(ctx, &user_id, "directory-disabled")
                .await?;
            summary.departures += 1;
        }

        // 外部身份映射（幂等键 `(provider, external_id)`）。
        self.store
            .upsert_external_identity(
                ctx,
                &ExternalIdentity {
                    tenant_id: ctx.tenant_id().unwrap_or_default().to_string(),
                    id: identity
                        .as_ref()
                        .map(|item| item.id.clone())
                        .unwrap_or_else(|| Uuid::new_v4().to_string()),
                    provider: config.source.clone(),
                    external_id: remote.external_id.clone(),
                    user_id: user_id.clone(),
                    last_synced_at: Some(time::OffsetDateTime::now_utc()),
                },
            )
            .await?;

        if changed {
            summary.updated += 1;
        }
        Ok(summary)
    }

    /// 记录一条字段级冲突。
    async fn record_conflict(
        &self,
        ctx: &RequestContext,
        draft: ConflictDraft<'_>,
    ) -> Result<(), OrgError> {
        self.store
            .upsert_conflict(
                ctx,
                &SyncConflict {
                    tenant_id: ctx.tenant_id().unwrap_or_default().to_string(),
                    id: Uuid::new_v4().to_string(),
                    source: draft.source.to_string(),
                    external_id: draft.external_id.to_string(),
                    field: draft.field.to_string(),
                    local_value: draft.local_value.to_string(),
                    remote_value: draft.remote_value.to_string(),
                    status: draft.status,
                    resolution: None,
                    resolved_by: None,
                    resolved_at: None,
                },
            )
            .await?;
        self.audit
            .record(
                ctx,
                &OrgAuditEvent::new("org.sync.conflict", "detect", "sync_conflict", draft.field)
                    .with_detail(format!(
                        "the field `{}` differs between the local value and the remote value",
                        draft.field
                    )),
            )
            .await?;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // 冲突裁决
    // -----------------------------------------------------------------------

    /// 列出冲突。
    pub async fn list_conflicts(
        &self,
        ctx: &RequestContext,
        status: Option<ConflictStatus>,
    ) -> Result<Vec<SyncConflict>, OrgError> {
        Ok(self.store.list_conflicts(ctx, status).await?)
    }

    /// 人工裁决一条冲突。
    ///
    /// - `KeepLocal`：保留本地值（外部值丢弃）；
    /// - `TakeRemote`：采用外部值（**必须**是人工显式裁决的结果）；
    /// - `Manual`：采用调用方给的值。
    pub async fn resolve_conflict(
        &self,
        ctx: &RequestContext,
        conflict_id: &str,
        resolution: ConflictResolution,
        value: Option<&str>,
        resolved_by: &str,
    ) -> Result<SyncConflict, OrgError> {
        let conflict =
            self.store
                .find_conflict(ctx, conflict_id)
                .await?
                .ok_or(OrgError::NotFound {
                    resource: "sync_conflict",
                })?;
        if conflict.status != ConflictStatus::Pending {
            return Err(OrgError::Conflict {
                reason: "the sync conflict has already been adjudicated",
            });
        }

        let identity = self
            .store
            .find_external_identity(ctx, &conflict.source, &conflict.external_id)
            .await?;
        let Some(identity) = identity else {
            return Err(OrgError::Conflict {
                reason: "the external identity of the conflict is unknown",
            });
        };

        let chosen = match resolution {
            ConflictResolution::KeepLocal => None,
            ConflictResolution::TakeRemote => Some(conflict.remote_value.clone()),
            ConflictResolution::Manual => {
                Some(value.map(str::to_string).ok_or(OrgError::InvalidArgument {
                    field: "value",
                    reason: "a manual resolution requires a value".to_string(),
                })?)
            }
        };

        if let Some(chosen) = &chosen {
            if conflict.field == SyncField::Department.as_str() {
                self.move_primary_department(ctx, &identity.user_id, chosen)
                    .await?;
            } else {
                let field = SyncField::parse(&conflict.field).ok_or(OrgError::InvalidArgument {
                    field: "field",
                    reason: format!("`{}` is not a synchronizable field", conflict.field),
                })?;
                field
                    .apply(&self.iam, ctx, &identity.user_id, chosen)
                    .await?;
            }
        }

        self.store
            .resolve_conflict(
                ctx,
                conflict_id,
                resolution,
                resolved_by,
                time::OffsetDateTime::now_utc(),
            )
            .await?;
        self.audit
            .record(
                ctx,
                &OrgAuditEvent::new(
                    "org.sync.conflict.resolved",
                    "resolve",
                    "sync_conflict",
                    conflict_id,
                )
                .with_detail(format!("resolved with {}", resolution.as_str())),
            )
            .await?;
        self.store
            .find_conflict(ctx, conflict_id)
            .await?
            .ok_or(OrgError::NotFound {
                resource: "sync_conflict",
            })
    }

    // -----------------------------------------------------------------------
    // 出站同步（SCIM 2.0）
    // -----------------------------------------------------------------------

    /// SCIM **全量对账**：把本地用户与组织节点排进**出站队列**就返回。
    ///
    /// 推送由后台 worker 执行 —— 本地请求**不等第三方**（D6 / R3）。
    pub async fn full_scim_sync(
        &self,
        ctx: &RequestContext,
        config: &ScimProviderConfig,
    ) -> Result<SyncSummary, OrgError> {
        config
            .validate()
            .map_err(|error| OrgError::InvalidArgument {
                field: "baseUrl",
                reason: error.to_string(),
            })?;

        let mut summary = SyncSummary::default();
        let users = self.iam.list_users(ctx, USER_LIST_LIMIT).await?;
        let nodes = self.store.list_nodes(ctx, None).await?;

        // Group（组织节点）。
        for node in &nodes {
            let members = self.store.list_user_org(ctx, None, Some(&node.id)).await?;
            let member_ids: Vec<String> = members.into_iter().map(|item| item.user_id).collect();
            self.outbox.enqueue(ScimJobEntry {
                tenant_id: ctx.tenant_id().unwrap_or_default().to_string(),
                config: config.clone(),
                job: ScimJob::PutGroup(ScimGroupDraft {
                    id: node.id.clone(),
                    display_name: node.name.clone(),
                    member_ids,
                }),
                attempt: 0,
                ready_at: None,
            });
            summary.fetched += 1;
        }

        // User。
        for user in &users {
            let memberships = self.store.list_user_org(ctx, Some(&user.id), None).await?;
            let group_ids: Vec<String> = memberships
                .into_iter()
                .map(|item| item.org_node_id)
                .collect();
            self.outbox.enqueue(ScimJobEntry {
                tenant_id: ctx.tenant_id().unwrap_or_default().to_string(),
                config: config.clone(),
                job: ScimJob::PutUser(ScimUserDraft {
                    id: user.id.clone(),
                    username: user.username.clone(),
                    email: user.email.clone(),
                    display_name: user.display_name.clone(),
                    active: user.status == platform_iam::store::STATUS_ACTIVE,
                    group_ids,
                }),
                attempt: 0,
                ready_at: None,
            });
            summary.fetched += 1;
        }

        summary.updated = summary.fetched;
        Ok(summary)
    }

    /// 处理出站队列里的就绪任务（后台 worker / 测试调用）。
    pub async fn drain_outbox(&self, gateway: &dyn ScimGateway) -> Vec<ScimJobOutcome> {
        self.outbox
            .drain_once(gateway, time::OffsetDateTime::now_utc())
            .await
    }

    /// 排一次 SCIM 出站推送（`active` 变更）。
    ///
    /// 失败**不影响**本地操作（推送是异步的）。
    async fn queue_scim_user_active(
        &self,
        ctx: &RequestContext,
        user_id: &str,
        active: bool,
    ) -> bool {
        let Some(provider) = crate::config::scim_provider_from_env() else {
            return false;
        };
        if provider.validate().is_err() {
            return false;
        }
        self.outbox.enqueue(ScimJobEntry {
            tenant_id: ctx.tenant_id().unwrap_or_default().to_string(),
            config: provider,
            job: ScimJob::PatchActive {
                external_id: user_id.to_string(),
                active,
            },
            attempt: 0,
            ready_at: None,
        });
        true
    }

    // -----------------------------------------------------------------------
    // 同步状态
    // -----------------------------------------------------------------------

    /// 读同步状态（上次同步时间、错误计数）。
    pub async fn sync_state(
        &self,
        ctx: &RequestContext,
        source: &str,
    ) -> Result<Option<SyncSourceState>, OrgError> {
        Ok(self.store.get_sync_state(ctx, source).await?)
    }

    // -----------------------------------------------------------------------
    // 内部工具
    // -----------------------------------------------------------------------

    /// 某用户主部门所在节点的**编码**（与目录里的 `department` 比对用）。
    async fn primary_department_code(
        &self,
        ctx: &RequestContext,
        user_id: &str,
    ) -> Result<Option<String>, OrgError> {
        let memberships = self.store.list_user_org(ctx, Some(user_id), None).await?;
        let primary = memberships
            .iter()
            .find(|item| item.is_primary)
            .or_else(|| memberships.first())
            .cloned();
        let Some(primary) = primary else {
            return Ok(None);
        };
        let node = self.get_node(ctx, &primary.org_node_id).await.ok();
        Ok(node.map(|node| node.code))
    }

    /// 把用户的主部门迁到某个编码的节点上（远端胜出 / 人工裁决用）。
    async fn move_primary_department(
        &self,
        ctx: &RequestContext,
        user_id: &str,
        department_code: &str,
    ) -> Result<(), OrgError> {
        let node = self
            .store
            .find_node_by_code(ctx, department_code)
            .await?
            .ok_or(OrgError::NotFound {
                resource: "org_node",
            })?;
        let memberships = self.store.list_user_org(ctx, Some(user_id), None).await?;
        let existing = memberships
            .into_iter()
            .find(|item| item.org_node_id == node.id);
        match existing {
            Some(item) => {
                self.store
                    .clear_primary_membership(ctx, user_id, &item.id)
                    .await?;
            }
            None => {
                self.add_member(
                    ctx,
                    NewMembership {
                        id: None,
                        user_id: user_id.to_string(),
                        org_node_id: node.id.clone(),
                        role: None,
                        is_primary: true,
                    },
                )
                .await?;
            }
        }
        Ok(())
    }

    /// 通知某个节点（含子树）下的全部成员。
    async fn notify_node_members(
        &self,
        ctx: &RequestContext,
        kind: OrgEventKind,
        node_id: &str,
        payload: &[(&str, &str)],
    ) -> Result<(), OrgError> {
        let subtree = self.store.subtree(ctx, node_id).await?;
        let ids: Vec<String> = subtree.into_iter().map(|node| node.id).collect();
        let members = self.store.members_of_nodes(ctx, &ids).await?;
        let mut recipients: Vec<String> = Vec::new();
        for member in members {
            if !recipients.contains(&member.user_id) {
                recipients.push(member.user_id);
            }
        }
        let mut event = OrgEvent::new(kind, recipients).with_payload("orgNodeId", node_id);
        for (key, value) in payload {
            event = event.with_payload(*key, *value);
        }
        self.notify(ctx, event).await
    }

    /// 投递一条通知（**失败不影响**业务操作 —— 没有渠道不该让组织操作失败）。
    async fn notify(&self, ctx: &RequestContext, event: OrgEvent) -> Result<(), OrgError> {
        let message = event.to_message();
        if let Err(error) = self.notifier.notify(ctx, &message).await {
            tracing::warn!(
                event_code = event.kind.code(),
                error = %error,
                "组织变更通知投递失败（业务操作本身已成功）"
            );
        }
        Ok(())
    }
}

/// 子孙节点的祖先链 = 新基链 + 子树内的相对链。
///
/// `base_path` 是被移动节点的**新**祖先链（末尾是它自己）；
/// `links` 是子树内的父子关系（被移动节点指向新父）。
fn subtree_path(
    node: &OrgNode,
    links: &BTreeMap<String, Option<String>>,
    base_path: &[String],
) -> Vec<String> {
    let mut path = base_path.to_vec();
    let mut current = node.id.clone();
    let mut guard = 0usize;
    loop {
        guard += 1;
        if guard >= tree::MAX_DEPTH {
            break;
        }
        let Some(parent_id) = links.get(&current).cloned().flatten() else {
            break;
        };
        // 走到被移动节点就停（它的链已经在 base_path 里了）。
        if parent_id == *base_path.last().unwrap_or(&String::new()) {
            break;
        }
        path.push(parent_id.clone());
        current = parent_id;
    }
    path.push(node.id.clone());
    path
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_descendant_path_is_the_new_base_plus_the_relative_chain() {
        // base = [root, moved]；子树内 moved → dept → team
        let mut links = BTreeMap::new();
        links.insert("moved".to_string(), Some("root".to_string()));
        links.insert("dept".to_string(), Some("moved".to_string()));
        links.insert("team".to_string(), Some("dept".to_string()));
        let base = vec!["root".to_string(), "moved".to_string()];

        let node = OrgNode {
            tenant_id: "t".to_string(),
            id: "team".to_string(),
            parent_id: Some("dept".to_string()),
            node_type: NodeType::Team,
            code: "t".to_string(),
            name: "t".to_string(),
            path_labels: Vec::new(),
        };
        assert_eq!(
            subtree_path(&node, &links, &base),
            vec![
                "root".to_string(),
                "moved".to_string(),
                "dept".to_string(),
                "team".to_string()
            ]
        );
    }
}
