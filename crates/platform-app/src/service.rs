//! 业务编排：CRUD / 工作台 / 常用最近 / 导航 / scope 投影（REQ-016，design §4）。
//!
//! # 校验顺序（**钉死**，TC-016-014 / D280）
//!
//! 一次 `POST` 可能同时踩多个错误 → 顺序必须是**可判定**的，否则
//! 「实现顺序换一下，AC 红/绿翻面」：
//!
//! ```text
//! ① 必填 / 类型   （APP_INVALID_REQUEST）
//! ② client 可用   （APP_CLIENT_INVALID）
//! ③ URL scheme    （APP_LAUNCH_URL_INVALID / APP_ICON_URL_INVALID）
//! ④ 唯一性        （APP_DUPLICATED）
//! ```
//!
//! # scope 是**只读投影**（D266）
//!
//! 响应里的 `scopes` **直接取自** `platform_oauth_client.allowed_scopes` →
//! 应用表**不存** scope。改 client 的 `allowed_scopes` → 响应**立刻**跟着变
//! （AC-S-159 步骤 4）。本模块每次读应用都**现查** client 表，不做任何缓存。
//!
//! # URL 校验（D263 / AC-S-156 路径 3/4）
//!
//! 只接受**绝对 `https` URL**；`http` 仅在 `app.icon.allowHttp = true` 时放行
//! （仅本地开发）。**`javascript:` / `data:` 必须被拒** —— 那是安全判据：
//! `launch_url` 由租户管理员填，用户点击时是**整页跳转**（`kind="external"`）
//! → 允许 `javascript:` 等于给了租户管理员一个对**本租户所有用户**执行脚本的入口。

use std::collections::BTreeMap;
use std::sync::Arc;

use platform_core::RequestContext;
use platform_core::extension::audit::{AuditEvent, AuditSink};
use platform_core::extension::permission::PermissionChecker;
use platform_core::extension::manifest::{Manifest, MenuSpec};

use crate::config::{AppConfigSource, AppSettings};
use crate::error::{AppError, ServiceError};
use crate::events::{self, object_type};
use crate::model::{
    AppRow, AppView, CreateAppBody, NavigationItem, NavigationView, UpdateAppBody, WorkspaceGroup,
    WorkspaceView, kind, source, status, view_of, view_of_access,
};
use crate::store::{AppStore, AppUpdate, NewApp, UniqueColumn};
use crate::tokens::{TokenVerifier, VerifiedToken};

/// 应用 ID 的前缀（平台生成，便于人眼区分资源类型）。
pub const APP_ID_PREFIX: &str = "app-";

/// 应用接入的业务服务。
#[derive(Clone)]
pub struct AppService {
    /// 数据访问。
    store: Arc<AppStore>,
    /// 配置源（**每请求读**）。
    config: Arc<dyn AppConfigSource>,
    /// 审计 sink。
    audit: Arc<dyn AuditSink>,
    /// 已启用模块的 manifest（导航的 `source="module"` 那一半）。
    manifests: Arc<Vec<Manifest>>,
    /// 权限判定器（导航要按 `menus[].perm` 过滤）。
    checker: Arc<dyn PermissionChecker>,
    /// 登录令牌校验（装配层注入）。
    tokens: Arc<dyn TokenVerifier>,
}

impl std::fmt::Debug for AppService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppService").finish_non_exhaustive()
    }
}

impl AppService {
    /// 构造服务。
    pub fn new(
        store: Arc<AppStore>,
        config: Arc<dyn AppConfigSource>,
        audit: Arc<dyn AuditSink>,
        manifests: Arc<Vec<Manifest>>,
        checker: Arc<dyn PermissionChecker>,
        tokens: Arc<dyn TokenVerifier>,
    ) -> Self {
        Self {
            store,
            config,
            audit,
            manifests,
            checker,
            tokens,
        }
    }

    /// 验登录令牌 → 已认证主体。
    ///
    /// ⚠️ 失败**一律** `Unauthenticated`（**不区分**原因，避免探测）。
    pub fn authenticate(&self, token: &str) -> Result<VerifiedToken, ServiceError> {
        self.tokens
            .verify(token)
            .map_err(|_| ServiceError::Unauthenticated)
    }

    /// 当前的运行期配置。
    pub async fn settings(&self, ctx: &RequestContext) -> Result<AppSettings, ServiceError> {
        self.config
            .read(ctx)
            .await
            .map_err(|_| ServiceError::Internal {
                reason: "the app configuration could not be read".to_string(),
            })
    }

    // -----------------------------------------------------------------------
    // 注册表 CRUD（AC-S-156 / AC-S-158 / AC-S-159）
    // -----------------------------------------------------------------------

    /// 注册一个应用（AC-S-156 步骤 1）。
    pub async fn create_app(
        &self,
        ctx: &RequestContext,
        body: &CreateAppBody,
    ) -> Result<AppView, ServiceError> {
        // ① 必填 / 类型 ------------------------------------------------------
        let client_id = required_value(body.client_id.as_deref(), "the client id is required")?;
        let display_name = required_value(
            body.display_name.as_deref(),
            "the display name is required",
        )?;
        let category = match body.category.as_deref().map(str::trim) {
            Some(value) if !value.is_empty() => value.to_string(),
            _ => "general".to_string(),
        };

        // ② client 必须**存在且可用** ----------------------------------------
        self.ensure_client_usable(&client_id).await?;

        // ③ URL scheme -------------------------------------------------------
        let settings = self.settings(ctx).await?;
        let launch_url = validate_optional_url(
            body.launch_url.as_deref(),
            settings.allow_http,
            AppError::LaunchUrlInvalid,
        )?;
        let icon_url = validate_optional_url(
            body.icon_url.as_deref(),
            settings.allow_http,
            AppError::IconUrlInvalid,
        )?;

        // ④ 唯一性 -----------------------------------------------------------
        if self
            .store
            .count_conflicts(ctx, UniqueColumn::ClientId, &client_id, None)
            .await
            .map_err(|error| internal("the app uniqueness could not be checked", error))?
            > 0
        {
            return Err(AppError::Duplicated {
                field: UniqueColumn::ClientId.field(),
            }
            .into());
        }
        if self
            .store
            .count_conflicts(ctx, UniqueColumn::DisplayName, &display_name, None)
            .await
            .map_err(|error| internal("the app uniqueness could not be checked", error))?
            > 0
        {
            return Err(AppError::Duplicated {
                field: UniqueColumn::DisplayName.field(),
            }
            .into());
        }

        let row = self
            .store
            .insert_app(
                ctx,
                &NewApp {
                    id: format!("{APP_ID_PREFIX}{}", uuid::Uuid::new_v4()),
                    client_id: client_id.clone(),
                    display_name,
                    description: body.description.clone(),
                    category,
                    icon_url,
                    launch_url,
                    sort_order: body.sort_order.unwrap_or(0),
                    status: status::ACTIVE.to_string(),
                    created_by: ctx.user_id().unwrap_or_default().to_string(),
                },
            )
            .await
            .map_err(|error| duplicate_or_internal(error, ctx, "the app could not be created"))?;

        self.audit_event(ctx, events::code::REGISTERED, &row, None, Some(&row))
            .await;
        let scopes = self.scopes_for_one(&row).await?;
        Ok(view_of(&row, scopes))
    }

    /// 列出全部应用（**数组**，不是 `{ apps: [] }` —— design §3.1）。
    pub async fn list_apps(&self, ctx: &RequestContext) -> Result<Vec<AppView>, ServiceError> {
        let rows = self
            .store
            .list_apps(ctx)
            .await
            .map_err(|error| internal("the app list could not be read", error))?;
        let scopes = self.scopes_for(&rows).await?;
        Ok(rows
            .iter()
            .zip(scopes)
            .map(|(row, scopes)| view_of(row, scopes))
            .collect())
    }

    /// 读单个应用。
    pub async fn get_app(
        &self,
        ctx: &RequestContext,
        app_id: &str,
    ) -> Result<AppView, ServiceError> {
        let row = self.require_app(ctx, app_id).await?;
        let scopes = self.scopes_for_one(&row).await?;
        Ok(view_of(&row, scopes))
    }

    /// 修改应用（AC-S-156 步骤 5）。
    pub async fn update_app(
        &self,
        ctx: &RequestContext,
        app_id: &str,
        body: &UpdateAppBody,
    ) -> Result<AppView, ServiceError> {
        let before = self.require_app(ctx, app_id).await?;

        // ① 类型 / 取值 ------------------------------------------------------
        let display_name = match body.display_name.as_deref().map(str::trim) {
            Some("") => {
                return Err(AppError::InvalidRequest {
                    reason: "the display name must not be blank",
                }
                .into());
            }
            other => other.map(str::to_string),
        };
        let category = match body.category.as_deref().map(str::trim) {
            Some("") => {
                return Err(AppError::InvalidRequest {
                    reason: "the category must not be blank",
                }
                .into());
            }
            other => other.map(str::to_string),
        };
        let next_status = match body.status.as_deref() {
            None => None,
            Some(value) => Some(parse_status(value)?),
        };

        // ③ URL scheme -------------------------------------------------------
        let settings = self.settings(ctx).await?;
        let launch_url = validate_optional_url(
            body.launch_url.as_deref(),
            settings.allow_http,
            AppError::LaunchUrlInvalid,
        )?;
        let icon_url = validate_optional_url(
            body.icon_url.as_deref(),
            settings.allow_http,
            AppError::IconUrlInvalid,
        )?;

        // ④ 唯一性（**排除自身** —— 改回原值不算冲突）--------------------------
        if let Some(value) = display_name.as_deref()
            && self
                .store
                .count_conflicts(ctx, UniqueColumn::DisplayName, value, Some(app_id))
                .await
                .map_err(|error| internal("the app uniqueness could not be checked", error))?
                > 0
        {
            return Err(AppError::Duplicated {
                field: UniqueColumn::DisplayName.field(),
            }
            .into());
        }

        let update = AppUpdate {
            display_name,
            description: body.description.clone(),
            category,
            icon_url,
            launch_url,
            sort_order: body.sort_order,
            status: next_status,
        };
        if update.is_empty() {
            // 空 PATCH → 不发 UPDATE，直接回当前值（幂等）。
            let scopes = self.scopes_for_one(&before).await?;
            return Ok(view_of(&before, scopes));
        }

        let row = self
            .store
            .update_app(ctx, app_id, &update)
            .await
            .map_err(|error| duplicate_or_internal(error, ctx, "the app could not be updated"))?
            .ok_or(AppError::NotFound)?;

        self.audit_event(ctx, events::code::UPDATED, &row, Some(&before), Some(&row))
            .await;
        let scopes = self.scopes_for_one(&row).await?;
        Ok(view_of(&row, scopes))
    }

    /// 删除应用（AC-S-156 步骤 6）。
    pub async fn delete_app(&self, ctx: &RequestContext, app_id: &str) -> Result<(), ServiceError> {
        let before = self.require_app(ctx, app_id).await?;
        let affected = self
            .store
            .delete_app(ctx, app_id)
            .await
            .map_err(|error| internal("the app could not be deleted", error))?;
        if affected == 0 {
            return Err(AppError::NotFound.into());
        }
        self.audit_event(ctx, events::code::DELETED, &before, Some(&before), None)
            .await;
        Ok(())
    }

    /// 把 `launch_url` 置 **NULL**（专用端点，D283 / AC-S-162 步骤 6）。
    pub async fn clear_launch_url(
        &self,
        ctx: &RequestContext,
        app_id: &str,
    ) -> Result<(), ServiceError> {
        let before = self.require_app(ctx, app_id).await?;
        let affected = self
            .store
            .clear_launch_url(ctx, app_id)
            .await
            .map_err(|error| internal("the launch url could not be cleared", error))?;
        if affected == 0 {
            return Err(AppError::NotFound.into());
        }
        // 清空入口是**一条独立的业务动作** → 记 `app.registry.updated`
        // （不是 `deleted`：应用还在册，只是没入口了）。
        let after = AppRow {
            launch_url: None,
            ..before.clone()
        };
        self.audit_event(ctx, events::code::UPDATED, &after, Some(&before), Some(&after))
            .await;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // 工作台 / 常用 / 最近（AC-S-160 / AC-S-161）
    // -----------------------------------------------------------------------

    /// 工作台清单：`active` 过滤 + **确定性排序** + 分组。
    ///
    /// # 排序（**钉死**，D267 / S-18 / S-19）
    ///
    /// 1. **全局**按 `(category, sortOrder, displayName, id)` 升序；
    /// 2. 按 `app.workspace.maxApps` **截断**（取排序后的前 N）；
    /// 3. **再分组**（组按 `category` 字典序 — 已由第 1 步的全序保证）。
    ///
    /// → 截断发生在**分组前**：超限时被切掉的是**全局序的末尾**（不是「每组切一点」）。
    ///
    /// ⚠️ **不按 `launch_url` 过滤**（D279 / S-04）：工作台是「接入了哪些应用」的
    /// **总览**，含尚未配入口的。
    pub async fn workspace(&self, ctx: &RequestContext) -> Result<WorkspaceView, ServiceError> {
        let settings = self.settings(ctx).await?;
        let mut rows = self
            .store
            .list_active_apps(ctx)
            .await
            .map_err(|error| internal("the workspace apps could not be read", error))?;

        // 确定性排序：**不依赖数据库返回顺序**。
        rows.sort_by(|left, right| {
            left.category
                .cmp(&right.category)
                // sort_order 是 int4 → 数值序（**不是**字典序：「10」<「9」）。
                .then(left.sort_order.cmp(&right.sort_order))
                .then(left.display_name.cmp(&right.display_name))
                .then(left.id.cmp(&right.id))
        });
        rows.truncate(settings.workspace_limit());

        let scopes = self.scopes_for(&rows).await?;
        let views: Vec<(AppView, &str)> = rows
            .iter()
            .zip(scopes)
            .map(|(row, scopes)| (view_of(row, scopes), row.category.as_str()))
            .collect();

        // 分组：**BTreeMap** → 组按键**字典序**遍历（不用 HashMap，S-19）。
        let mut grouped: BTreeMap<&str, Vec<AppView>> = BTreeMap::new();
        for (view, category) in views {
            grouped.entry(category).or_default().push(view);
        }
        Ok(WorkspaceView {
            groups: grouped
                .into_iter()
                .map(|(category, apps)| WorkspaceGroup {
                    category: category.to_string(),
                    apps,
                })
                .collect(),
        })
    }

    /// 最近访问（AC-S-161 步骤 2）。
    pub async fn recent(&self, ctx: &RequestContext) -> Result<Vec<AppView>, ServiceError> {
        let settings = self.settings(ctx).await?;
        let user_id = ctx.user_id().unwrap_or_default().to_string();
        let rows = self
            .store
            .recent_apps(ctx, &user_id, settings.recent_limit())
            .await
            .map_err(|error| internal("the recent apps could not be read", error))?;
        let scopes = self.scopes_for_access(&rows).await?;
        Ok(rows
            .iter()
            .zip(scopes)
            .map(|(row, scopes)| view_of_access(row, scopes))
            .collect())
    }

    /// 常用应用（AC-S-161 步骤 3）。
    pub async fn frequent(&self, ctx: &RequestContext) -> Result<Vec<AppView>, ServiceError> {
        let settings = self.settings(ctx).await?;
        let user_id = ctx.user_id().unwrap_or_default().to_string();
        let rows = self
            .store
            .frequent_apps(ctx, &user_id, settings.recent_limit())
            .await
            .map_err(|error| internal("the frequent apps could not be read", error))?;
        let scopes = self.scopes_for_access(&rows).await?;
        Ok(rows
            .iter()
            .zip(scopes)
            .map(|(row, scopes)| view_of_access(row, scopes))
            .collect())
    }

    /// 记一次访问（AC-S-161 步骤 1/4）：UPSERT + **累加**。
    ///
    /// ⚠️ **不要求 `launch_url` 非空**（D279）：记录「用户打开过」这件事，
    /// 与有没有入口无关。
    pub async fn record_access(
        &self,
        ctx: &RequestContext,
        app_id: &str,
    ) -> Result<crate::model::AccessRecorded, ServiceError> {
        let app = self.require_app(ctx, app_id).await?;
        // 在册但 **hidden** 的应用**不可**记访问 —— 「不在册 / 不可见」的应用
        // 不该出现在最近 / 常用里（D268 的「只返回在册且 active」）。
        if app.status != status::ACTIVE {
            return Err(AppError::NotFound.into());
        }
        let row = self
            .store
            .record_access(ctx, &app.client_id)
            .await
            .map_err(|error| internal("the app access could not be recorded", error))?;
        Ok(crate::model::AccessRecorded {
            status: "recorded",
            access_count: row.access_count,
        })
    }

    // -----------------------------------------------------------------------
    // 导航下发（AC-S-162）
    // -----------------------------------------------------------------------

    /// 导航项：**两类来源的并集**（module + app），按权限点过滤。
    ///
    /// # 排序（**钉死**，D284）
    ///
    /// `module` 项**在前**、`app` 项**在后**；各组内按 `label` 升序
    /// （`label` 相同再按 `target` / `appId`，保证确定性）。
    ///
    /// # `module` 项：按 `menus[].perm` 过滤
    ///
    /// 调用者**没有**那个权限点 → **不下发**；`perm` 缺省（`None`）→
    /// **总是下发**（D269 / AC-S-162 步骤 3）。
    ///
    /// # `app` 项：`launch_url` 为空的**不下发**
    ///
    /// 没入口点了没意义（D269 / D279 / AC-S-162 步骤 6）。
    pub async fn navigation(&self, ctx: &RequestContext) -> Result<NavigationView, ServiceError> {
        let rows = self
            .store
            .list_active_apps(ctx)
            .await
            .map_err(|error| internal("the navigation apps could not be read", error))?;

        let mut items: Vec<NavigationItem> = Vec::new();

        // ---- module 来源（manifest 的 menus[]） ----------------------------
        for manifest in self.manifests.iter() {
            for menu in &manifest.menus {
                if !self.menu_is_visible(ctx, menu) {
                    continue;
                }
                items.push(module_item(menu));
            }
        }

        // ---- app 来源（platform_app） --------------------------------------
        for row in &rows {
            // 没入口 → 不下发（**唯一**按 launch_url 过滤的地方，D279）。
            let Some(target) = row.launch_url.as_deref().filter(|value| !value.is_empty()) else {
                continue;
            };
            items.push(NavigationItem {
                source: source::APP,
                app_id: Some(row.id.clone()),
                label: row.display_name.clone(),
                icon: row.icon_url.clone(),
                target: target.to_string(),
                kind: kind::EXTERNAL,
                required_permission: None,
            });
        }

        // 确定性排序：module 在前、app 在后；组内按 label 升序。
        items.sort_by(|left, right| {
            source_rank(left.source)
                .cmp(&source_rank(right.source))
                .then(left.label.cmp(&right.label))
                .then(left.target.cmp(&right.target))
                .then(left.app_id.cmp(&right.app_id))
        });

        Ok(NavigationView { items })
    }

    /// 一个 `menus[]` 项对当前调用者是否可见（`perm` 缺省 → 总是可见）。
    fn menu_is_visible(&self, ctx: &RequestContext, menu: &MenuSpec) -> bool {
        match menu.perm.as_deref() {
            None => true,
            Some(code) => self.checker.has(ctx, code),
        }
    }

    // -----------------------------------------------------------------------
    // 内部辅助
    // -----------------------------------------------------------------------

    /// 取一个应用；不存在 → `APP_NOT_FOUND`（**不区分**「不属于本租户」，避免探测）。
    async fn require_app(
        &self,
        ctx: &RequestContext,
        app_id: &str,
    ) -> Result<AppRow, ServiceError> {
        self.store
            .get_app(ctx, app_id)
            .await
            .map_err(|error| internal("the app could not be read", error))?
            .ok_or_else(|| AppError::NotFound.into())
    }

    /// client 必须**存在且可用**（`active` 且未过期）→ 否则 `APP_CLIENT_INVALID`。
    async fn ensure_client_usable(&self, client_id: &str) -> Result<(), ServiceError> {
        let Some(client) = self
            .store
            .get_client_view(client_id)
            .await
            .map_err(|error| internal("the oauth client could not be read", error))?
        else {
            return Err(AppError::ClientInvalid {
                reason: "the oauth client does not exist",
            }
            .into());
        };
        if client.status != "active" {
            return Err(AppError::ClientInvalid {
                reason: "the oauth client is not active",
            }
            .into());
        }
        if let Some(expires_at) = client.expires_at
            && expires_at <= time::OffsetDateTime::now_utc()
        {
            return Err(AppError::ClientInvalid {
                reason: "the oauth client has expired",
            }
            .into());
        }
        Ok(())
    }

    /// 一批应用的 `scopes`（**只读投影**，D266）：**现查** client 表，不缓存。
    ///
    /// ⚠️ client 查不到（被删 / 竞态）→ 空数组（不是 500）：应用行本身受
    /// `ON DELETE CASCADE` 保护，正常路径下不会出现；出现也说明「没有可声明的 scope」。
    async fn scopes_for(&self, rows: &[AppRow]) -> Result<Vec<Vec<String>>, ServiceError> {
        let client_ids: Vec<String> = rows.iter().map(|row| row.client_id.clone()).collect();
        let map = self
            .store
            .client_scopes(&client_ids)
            .await
            .map_err(|error| internal("the oauth client scopes could not be read", error))?;
        Ok(rows
            .iter()
            .map(|row| map.get(&row.client_id).cloned().unwrap_or_default())
            .collect())
    }

    /// 单个应用的 `scopes`（同 [`Self::scopes_for`]，只是省掉一层 `Vec`）。
    async fn scopes_for_one(&self, row: &AppRow) -> Result<Vec<String>, ServiceError> {
        let mut scopes = self.scopes_for(std::slice::from_ref(row)).await?;
        Ok(scopes.pop().unwrap_or_default())
    }

    /// 同 [`Self::scopes_for`]，输入是访问记录的 join 行。
    async fn scopes_for_access(
        &self,
        rows: &[crate::model::AppAccessRow],
    ) -> Result<Vec<Vec<String>>, ServiceError> {
        let client_ids: Vec<String> = rows.iter().map(|row| row.client_id.clone()).collect();
        let map = self
            .store
            .client_scopes(&client_ids)
            .await
            .map_err(|error| internal("the oauth client scopes could not be read", error))?;
        Ok(rows
            .iter()
            .map(|row| map.get(&row.client_id).cloned().unwrap_or_default())
            .collect())
    }

    /// 记一条审计事件。
    ///
    /// ⚠️ 写失败**不得**让业务失败（应用已落库，拒绝响应会让调用方以为没建成）——
    /// 与 `platform-oauth` 的 `audit_event` 同款口径。
    async fn audit_event(
        &self,
        ctx: &RequestContext,
        action: &str,
        row: &AppRow,
        before: Option<&AppRow>,
        after: Option<&AppRow>,
    ) {
        let event = AuditEvent {
            tenant_id: ctx.tenant_id().unwrap_or_default().to_string(),
            actor_id: ctx.user_id().map(str::to_string),
            action: action.to_string(),
            object_type: object_type::APP.to_string(),
            object_id: Some(row.id.clone()),
            before: before.map(audit_payload),
            after: after.map(audit_payload),
            ip: ctx.client_ip().map(str::to_string),
            user_agent: ctx.user_agent().map(str::to_string),
        };
        if let Err(error) = self.audit.record(ctx, &event).await {
            tracing::warn!(error = %error, action, "the app audit event could not be recorded");
        }
    }
}

/// 审计的 `before` / `after` payload（**不含** client 的任何敏感字段）。
fn audit_payload(row: &AppRow) -> serde_json::Value {
    serde_json::json!({
        "appId": row.id,
        "clientId": row.client_id,
        "displayName": row.display_name,
        "description": row.description,
        "category": row.category,
        "iconUrl": row.icon_url,
        "launchUrl": row.launch_url,
        "sortOrder": row.sort_order,
        "status": row.status,
    })
}

/// `source` 的排序权重：`module` = 0（**在前**），`app` = 1（D284）。
fn source_rank(source: &str) -> u8 {
    if source == source::MODULE { 0 } else { 1 }
}

/// 由一条 `menus[]` 声明产出 `source="module"` 的导航项（D284：只用 4 个字段）。
fn module_item(menu: &MenuSpec) -> NavigationItem {
    NavigationItem {
        source: source::MODULE,
        app_id: None,
        label: menu.label.clone(),
        icon: menu.icon.clone(),
        target: menu.route.clone(),
        kind: kind::INTERNAL,
        required_permission: menu.perm.clone(),
    }
}

/// 必填值：`None` / 空白 → `APP_INVALID_REQUEST`。
fn required_value(value: Option<&str>, reason: &'static str) -> Result<String, ServiceError> {
    match value.map(str::trim) {
        Some(trimmed) if !trimmed.is_empty() => Ok(trimmed.to_string()),
        _ => Err(AppError::InvalidRequest { reason }.into()),
    }
}

/// 状态取值：只认 `active` / `hidden`。
fn parse_status(value: &str) -> Result<String, ServiceError> {
    match value.trim() {
        status::ACTIVE => Ok(status::ACTIVE.to_string()),
        status::HIDDEN => Ok(status::HIDDEN.to_string()),
        _ => Err(AppError::InvalidRequest {
            reason: "the status must be either `active` or `hidden`",
        }
        .into()),
    }
}

/// 校验一个**可选** URL 字段（`None` / 空白 → `None`）。
///
/// 规则（D263 / AC-S-156 路径 3/4）：
///
/// | 取值 | 结果 |
/// |------|------|
/// | `None` / `""` / 全空白 | `Ok(None)`（没配） |
/// | `https://…` | `Ok(Some(…))` |
/// | `http://…` 且 `allow_http` | `Ok(Some(…))` |
/// | `http://…` 且 `!allow_http` | `Err(…)` |
/// | `javascript:` / `data:` / `ftp:` / `not-a-url` | `Err(…)` |
///
/// ⚠️ **不用正则**：正则能写出 `https?://` 的形状却写不出「绝对 URL」的语义，
/// 而且 `javascript:alert(1)` 在宽松的模式下很容易被放行 —— 这里按
/// **scheme 白名单 + 必须带 `://` 的 host** 判。
fn validate_optional_url(
    raw: Option<&str>,
    allow_http: bool,
    error: AppError,
) -> Result<Option<String>, ServiceError> {
    let Some(trimmed) = raw.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    let lowered = trimmed.to_ascii_lowercase();
    let allowed = if lowered.starts_with("https://") {
        true
    } else if lowered.starts_with("http://") {
        allow_http
    } else {
        false
    };
    if !allowed {
        return Err(error.into());
    }
    // 必须有 host（`https://` 后面不能立刻是 `/` 或空）。
    let rest = &trimmed[trimmed.find("://").map_or(0, |index| index + 3)..];
    let host = rest.split('/').next().unwrap_or_default();
    if host.is_empty() {
        return Err(error.into());
    }
    Ok(Some(trimmed.to_string()))
}

/// 数据访问错误 → 服务错误（**唯一性冲突**要识别成 `APP_DUPLICATED`）。
fn duplicate_or_internal(
    error: platform_db::DbError,
    _ctx: &RequestContext,
    summary: &str,
) -> ServiceError {
    if let Some(constraint) = error.foreign_key_constraint() {
        tracing::warn!(constraint, summary, "the app write hit a foreign key violation");
    }
    // 唯一索引冲突（`platform_app_tenant_client_idx` / `..._display_name_idx`）→ 409。
    if let platform_db::DbError::Database(sql_error) = &error
        && let Some(database_error) = sql_error.as_database_error()
        && database_error.is_unique_violation()
    {
        let constraint = database_error.constraint().unwrap_or_default();
        let field = if constraint.contains("display_name") {
            UniqueColumn::DisplayName.field()
        } else {
            UniqueColumn::ClientId.field()
        };
        return AppError::Duplicated { field }.into();
    }
    internal(summary, error)
}

/// 数据访问错误 → 平台内部错误（**不把内部细节漏给调用方**）。
fn internal<E: std::fmt::Display>(summary: &str, error: E) -> ServiceError {
    tracing::error!(error = %error, summary, "the app data access failed");
    ServiceError::Internal {
        reason: summary.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_value_is_treated_as_absent() {
        for raw in [None, Some(""), Some("   ")] {
            let result =
                validate_optional_url(raw, false, AppError::LaunchUrlInvalid).expect("应当通过");
            assert_eq!(result, None, "空值必须等价于「没配」：{raw:?}");
        }
    }

    #[test]
    fn an_https_url_is_accepted() {
        let value = validate_optional_url(
            Some("https://expense.example/app"),
            false,
            AppError::LaunchUrlInvalid,
        )
        .expect("https 必须通过");
        assert_eq!(value.as_deref(), Some("https://expense.example/app"));
    }

    #[test]
    fn an_http_url_is_rejected_by_default_and_accepted_when_allowed() {
        let rejected =
            validate_optional_url(Some("http://x.example/a"), false, AppError::LaunchUrlInvalid);
        assert!(rejected.is_err(), "allowHttp=false 时 http 必须被拒");
        assert_eq!(
            rejected.expect_err("必须报错").code(),
            AppError::LaunchUrlInvalid.code()
        );

        let accepted =
            validate_optional_url(Some("http://x.example/a"), true, AppError::LaunchUrlInvalid)
                .expect("allowHttp=true 时 http 必须通过");
        assert_eq!(accepted.as_deref(), Some("http://x.example/a"));
    }

    /// ⚠️ **安全判据**（AC-S-156 路径 3/4）：`javascript:` 与 `data:` 必须被拒。
    #[test]
    fn script_and_data_schemes_are_always_rejected() {
        for raw in [
            "javascript:alert(1)",
            "JavaScript:alert(1)",
            "data:text/html,<script>alert(1)</script>",
            "ftp://x.example/a",
            "not-a-url",
            "https://",
            "https:///path-without-host",
        ] {
            for allow_http in [false, true] {
                let result =
                    validate_optional_url(Some(raw), allow_http, AppError::LaunchUrlInvalid);
                assert!(
                    result.is_err(),
                    "`{raw}`（allowHttp={allow_http}）必须被拒 —— 那是安全判据"
                );
            }
        }
    }

    /// 图标 URL 报的是 `APP_ICON_URL_INVALID`（**不是** launch 那个码）。
    #[test]
    fn the_icon_url_error_carries_its_own_code() {
        let error = validate_optional_url(Some("javascript:alert(1)"), false, AppError::IconUrlInvalid)
            .expect_err("必须报错");
        assert_eq!(error.code(), AppError::IconUrlInvalid.code());
    }

    #[test]
    fn the_source_rank_puts_modules_first() {
        assert!(source_rank(source::MODULE) < source_rank(source::APP));
    }

    #[test]
    fn the_two_status_values_are_the_only_accepted_ones() {
        assert_eq!(parse_status("active").expect("应当通过"), "active");
        assert_eq!(parse_status("hidden").expect("应当通过"), "hidden");
        assert_eq!(
            parse_status("archived")
                .expect_err("必须报错")
                .code(),
            AppError::InvalidRequest {
                reason: "the status must be either `active` or `hidden`"
            }
            .code()
        );
    }

    /// `menus[]` → 导航项：`perm` 原样带出、`kind` 恒 `internal`（D284）。
    #[test]
    fn a_menu_maps_to_an_internal_navigation_item() {
        let menu = MenuSpec {
            label: "审计日志".to_string(),
            icon: Some("shield".to_string()),
            route: "/admin/audit".to_string(),
            perm: Some("audit:events:read".to_string()),
        };
        let item = module_item(&menu);
        assert_eq!(item.source, source::MODULE);
        assert_eq!(item.kind, kind::INTERNAL);
        assert_eq!(item.target, "/admin/audit");
        assert_eq!(item.label, "审计日志");
        assert_eq!(item.required_permission.as_deref(), Some("audit:events:read"));
        assert!(item.app_id.is_none());
    }
}
