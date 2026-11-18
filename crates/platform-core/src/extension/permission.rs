//! 权限判定面：数据范围语义 + 鉴权入口。
//!
//! # 定位（design-summary §1）
//!
//! 业务模块**只**依赖 `platform-core` + `platform-db`（AGENTS.md 硬规则 2），
//! 所以鉴权入口 [`require_permission`] **住在 `platform-core`**，不住在 `platform-rbac`。
//!
//! | | 谁实现 | 谁调用 | 能力索引里的分类 |
//! |---|--------|--------|------------------|
//! | 5 个扩展点 | 业务模块 | 平台 | `kind: "extension-point"` |
//! | [`PermissionChecker`] | 平台（`platform-rbac`） | 业务模块 | `kind: "platform-capability"` |
//!
//! [`PermissionChecker`] **不是**第 6 个扩展点 —— 方向相反：
//! 扩展点是「业务 ↔ 平台的接缝」，必须双向都有角色；
//! `PermissionChecker` 只是平台向业务提供的一个能力接口。
//!
//! # 鉴权入口怎么拿到判定实现
//!
//! **不靠泛型状态，靠请求扩展**：
//! `platform-server` 把 `Arc<dyn PermissionChecker>` 放进请求扩展，
//! [`require_permission`] 返回的 layer 从扩展里取出来用。
//! trait 的方法**全是同步的** → dyn 兼容。

use std::convert::Infallible;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use axum::extract::Request;
use axum::response::{IntoResponse, Response};
use tower::{Layer, Service};

use crate::context::RequestContext;
use crate::extension::expr::CompiledCondition;
use crate::problem::{CODE_INTERNAL_ERROR, ProblemDetails, problem_response};

/// 数据范围语义（五种）。
///
/// 语义与求值口径见 design-summary §4；**求值**在 `platform-rbac`，
/// 这里只定死「有哪些语义」与「语义名怎么写」（落库与 API 都用它）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DataScope {
    /// 全部（无过滤）。
    All,
    /// 本部门（用户**主部门**节点）。
    OwnOrg,
    /// 本部门及下级（主部门节点的子树）。
    OwnOrgAndBelow,
    /// 仅本人（`created_by = 当前用户`）。
    OnlySelf,
    /// 自定义过滤条件（受限表达式 DSL）。
    Custom,
}

impl DataScope {
    /// 全部五种语义（顺序固定，便于逐条比对）。
    pub const ALL: [DataScope; 5] = [
        DataScope::All,
        DataScope::OwnOrg,
        DataScope::OwnOrgAndBelow,
        DataScope::OnlySelf,
        DataScope::Custom,
    ];

    /// 落库 / API 用的名字。
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::All => "ALL",
            Self::OwnOrg => "OWN_ORG",
            Self::OwnOrgAndBelow => "OWN_ORG_AND_BELOW",
            Self::OnlySelf => "SELF",
            Self::Custom => "CUSTOM",
        }
    }

    /// 从落库 / API 的名字解析。
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "ALL" => Some(Self::All),
            "OWN_ORG" => Some(Self::OwnOrg),
            "OWN_ORG_AND_BELOW" => Some(Self::OwnOrgAndBelow),
            "SELF" => Some(Self::OnlySelf),
            "CUSTOM" => Some(Self::Custom),
            _ => None,
        }
    }

    /// 是否是「自定义范围」——需要在响应里标明。
    pub fn is_custom(&self) -> bool {
        matches!(self, Self::Custom)
    }
}

/// 可执行的数据范围过滤条件（**已参数化**）。
///
/// 渲染由 `platform-db` 的 `TenantQuery::with_scope` 负责 ——
/// **业务模块构造不出「跳过范围」的东西**，因为这个枚举里没有那个选项。
///
/// # 多角色的可见范围取**并集**
///
/// 与权限点「任一满足即通过」一致：每个角色给一份授权（权限 + 一份可见范围过滤条件），
/// 多角色的过滤条件取**并集** —— [`Self::Custom`] 的多个条件之间是 **OR**，
/// [`Self::CustomOr`] 表达「自定义条件 **OR** 包含型范围」。
#[derive(Debug, Clone, PartialEq)]
pub enum ScopeFilter {
    /// 全部：不追加任何条件。
    All,
    /// 限定在一组组织节点内（`org_node_id = ANY($n)`）。
    OrgNodes {
        /// 组织节点列名（必须是安全 SQL 标识符）。
        column: String,
        /// 允许看到的节点 ID 集合。
        nodes: Vec<String>,
    },
    /// 仅本人（`created_by = $n`）。
    Owner {
        /// 创建人列名（必须是安全 SQL 标识符）。
        column: String,
        /// 当前用户 ID。
        user_id: String,
    },
    /// 自定义条件（多个条件取**并集** —— 与「多角色取最宽松」一致）。
    Custom {
        /// 已编译的条件（SQL 片段里的字面值一律走绑定）。
        conditions: Vec<CompiledCondition>,
    },
    /// 自定义条件 **OR** 一个包含型范围（**并集**语义）。
    ///
    /// 多角色的可见范围取**并集** —— 与权限点「任一满足即通过」一致：
    /// `CUSTOM` 条件 **OR** `OWN_ORG` / `OWN_ORG_AND_BELOW` / `SELF` 的范围。
    ///
    /// 渲染成**一个**括号包起来的 OR 表达式；两边的值**一律走参数绑定**
    /// （包含型范围的节点集合仍是**数组绑定**，不展开成字符串拼接）。
    CustomOr {
        /// 已编译的自定义条件（彼此 **OR**）。
        conditions: Vec<CompiledCondition>,
        /// 另一侧的范围（包含型：`OrgNodes` / `Owner`）。
        scope: Box<ScopeFilter>,
    },
}

impl ScopeFilter {
    /// 是否不加任何条件。
    pub fn is_all(&self) -> bool {
        matches!(self, Self::All)
    }

    /// 条件个数（0 = 不限范围）。
    pub fn condition_count(&self) -> usize {
        match self {
            Self::All => 0,
            Self::OrgNodes { nodes, .. } => usize::from(!nodes.is_empty()),
            Self::Owner { .. } => 1,
            Self::Custom { conditions } => conditions.len(),
            Self::CustomOr { conditions, scope } => conditions.len() + scope.condition_count(),
        }
    }
}

/// 权限判定能力（**平台实现，业务模块调用**）。
///
/// 方法**全是同步的**（无 RPITIT）→ 可以 `Arc<dyn PermissionChecker>`。
pub trait PermissionChecker: Send + Sync + 'static {
    /// 当前用户在 `ctx` 指定的租户下是否拥有该权限点。
    ///
    /// **只回答「有没有」**，不回答「能对哪条记录做」——
    /// 后者是数据范围（[`Self::effective_scope`]）与属性条件（[`Self::condition`]）的事。
    fn has(&self, ctx: &RequestContext, permission: &str) -> bool;

    /// 数据范围：把一种 scope 解析成可执行的过滤条件。
    fn scope(&self, ctx: &RequestContext, scope: DataScope) -> ScopeFilter;

    /// 某个权限点的**有效数据范围**（多角色取最宽松；权限点级 scope 覆盖角色级）。
    ///
    /// 没有该权限点时返回 [`ScopeFilter::All`] —— 调用方**必须**先用
    /// [`Self::has`] 判过权限点，否则「没权限」会被误当成「范围不限」。
    fn effective_scope(&self, ctx: &RequestContext, permission: &str) -> ScopeFilter;

    /// 某个权限点携带的**属性条件**（属性级校验 / ABAC）；**没有条件**时返回 `None`。
    ///
    /// 返回值语义（**必须**严格区分，BA 决策 D40）：
    ///
    /// | 返回值 | 含义 | 调用方动作 |
    /// |---|---|---|
    /// | `None` | 这个权限点**根本没有**条件 | 直接放行 |
    /// | `Some(永不匹配的条件)` | 条件**非法**（引用白名单外属性 / 编译失败） | **判不通过**（fail closed） |
    ///
    /// ⚠️ **非法条件不得返回 `None`** —— 那会让调用方把「无条件」与「条件非法」混为一谈：
    /// 要么误拒全部无条件权限点，要么把非法条件**静默放行**。两者都是错的。
    /// 非法条件必须编译成**永不匹配**的条件（如 `1 = 0`），
    /// 调用方应用它之后**一行都看不到**。
    fn condition(&self, ctx: &RequestContext, permission: &str) -> Option<CompiledCondition>;
}

/// 权限判定的**异步刷新**接缝（平台能力，不是扩展点）。
///
/// [`PermissionChecker`] 的方法必须同步（dyn 兼容），所以读库发生在**方法之外**：
/// 认证中间件在请求进入时调用一次 [`Self::ensure_fresh`]，同步方法只读已填好的缓存。
///
/// # 为什么放在 `platform-core`
///
/// 任何**自带认证中间件**的平台模块（REQ-010 的 `platform-config` 就是第一个）
/// 都要在 `require_permission` 之前刷新判定缓存；没有这个接缝，它只能去依赖
/// `platform-rbac` 的具体实现（平台模块之间不该这样耦合）。
///
/// 返回 `Pin<Box<dyn Future>>` 而不是 RPITIT —— 这样它可以
/// `Arc<dyn PermissionRefresher>`（trait object），与 [`PermissionChecker`] 保持同样的
/// 装配方式。
pub trait PermissionRefresher: Send + Sync + 'static {
    /// 用共享数据库的 epoch 判断缓存是否新鲜；不一致则重算。
    ///
    /// 失败时返回**英文**原因（由调用方转成 `INTERNAL_ERROR`；**不得放行**）。
    fn ensure_fresh<'a>(
        &'a self,
        ctx: &'a RequestContext,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>>;
}

/// 鉴权入口：**平台唯一的权限判定入口**。
///
/// ```ignore
/// router.route("/api/order/invoice/approve", post(approve))
///     .route_layer(require_permission("order:invoice:approve"));
/// ```
///
/// # 三种拒绝语义（契约 C5）
///
/// | 情形 | HTTP | 错误码 |
/// |------|------|--------|
/// | 无令牌 / 未认证 | **401** | `UNAUTHENTICATED` |
/// | 已认证但缺该权限点 | **403** | `INSUFFICIENT_PERMISSION` |
/// | 有权限点但记录不在数据范围内 | **404** | `NOT_FOUND`（由数据层过滤后判定） |
///
/// # 为什么入口在 `platform-core`
///
/// 业务模块的运行期依赖只有 `platform-core` + `platform-db`（AGENTS.md 硬规则 2）。
/// 鉴权入口住在 `platform-rbac` 的话，业务模块就得依赖平台内部模块 → 违反规则 2。
///
/// # 返回类型
///
/// 返回 [`PermissionLayer`] —— 一个具名类型，契约检查 C-5 按它定位入口的唯一性。
pub fn require_permission(code: impl Into<String>) -> PermissionLayer {
    PermissionLayer { code: code.into() }
}

/// 鉴权层。
///
/// **全仓库只有这一个鉴权入口** —— 契约检查 C-5 扫的就是它，
/// 出现第二个入口 → 检查失败（扫描会被绕过）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionLayer {
    /// 要求的权限点编码。
    code: String,
}

impl PermissionLayer {
    /// 要求的权限点编码。
    pub fn permission(&self) -> &str {
        &self.code
    }
}

impl<S> Layer<S> for PermissionLayer {
    type Service = PermissionService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        PermissionService {
            inner,
            code: self.code.clone(),
        }
    }
}

/// 鉴权层的服务。
#[derive(Debug, Clone)]
pub struct PermissionService<S> {
    inner: S,
    code: String,
}

impl<S> Service<Request> for PermissionService<S>
where
    S: Service<Request, Error = Infallible> + Clone + Send + Sync + 'static,
    S::Response: IntoResponse,
    S::Future: Send + 'static,
{
    type Response = Response;
    type Error = Infallible;
    type Future = Pin<Box<dyn Future<Output = Result<Response, Infallible>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, request: Request) -> Self::Future {
        // 与 axum 自己的 `from_fn` 同一套做法：把「已就绪」的那份内层取出来用。
        let not_ready_inner = self.inner.clone();
        let ready_inner = std::mem::replace(&mut self.inner, not_ready_inner);
        let code = self.code.clone();

        Box::pin(async move {
            let mut service = PermissionService {
                inner: ready_inner,
                code,
            };
            // **请求路径**（嵌套路由会剥掉前缀 → 用 `OriginalUri` 扩展取原始的）。
            let instance = request
                .extensions()
                .get::<axum::extract::OriginalUri>()
                .map(|uri| uri.0.path().to_string())
                .unwrap_or_else(|| request.uri().path().to_string());
            let ctx = request
                .extensions()
                .get::<RequestContext>()
                .cloned()
                .unwrap_or_else(RequestContext::new);

            if let Some(response) = authorize(&ctx, &instance, &service.code, &request) {
                return Ok(response);
            }

            match service.inner.call(request).await {
                Ok(response) => Ok(response.into_response()),
                Err(never) => match never {},
            }
        })
    }
}

/// 鉴权判定：401 / 403 / 放行。
///
/// 返回 `Some(响应)` = 拒绝（`None` = 放行）。
/// 用 `Option` 而不是 `Result` —— 拒绝用的响应体很大，
/// 放在 `Err` 里会触发 `clippy::result_large_err`。
fn authorize(
    ctx: &RequestContext,
    instance: &str,
    code: &str,
    request: &Request,
) -> Option<Response> {
    // 未认证 → 401。**先判认证、再判权限**：顺序反了会把「没登录」说成「没权限」。
    if ctx.user_id().is_none() || ctx.tenant_id().is_none() {
        return Some(problem_response(&ProblemDetails::unauthenticated(
            ctx, instance,
        )));
    }

    // 判定实现缺失 → **fail closed**（平台装配错了，绝不能放行）。
    let Some(checker) = request
        .extensions()
        .get::<Arc<dyn PermissionChecker>>()
        .cloned()
    else {
        return Some(problem_response(&ProblemDetails::from_context(
            CODE_INTERNAL_ERROR,
            "服务内部错误",
            500,
            "the permission checker is not installed on this service",
            ctx,
            instance,
            None,
        )));
    };

    if !checker.has(ctx, code) {
        return Some(problem_response(&ProblemDetails::insufficient_permission(
            ctx, instance, code,
        )));
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scope_names_round_trip() {
        for scope in DataScope::ALL {
            assert_eq!(DataScope::parse(scope.as_str()), Some(scope));
        }
        assert_eq!(DataScope::parse("NOPE"), None);
    }

    #[test]
    fn the_scope_names_are_the_ones_the_spec_fixes() {
        let names: Vec<&str> = DataScope::ALL.iter().map(DataScope::as_str).collect();
        assert_eq!(
            names,
            vec!["ALL", "OWN_ORG", "OWN_ORG_AND_BELOW", "SELF", "CUSTOM"]
        );
    }

    #[test]
    fn only_custom_is_flagged_as_custom() {
        for scope in DataScope::ALL {
            assert_eq!(scope.is_custom(), scope == DataScope::Custom);
        }
    }

    #[test]
    fn the_all_filter_has_no_condition() {
        assert!(ScopeFilter::All.is_all());
        assert_eq!(ScopeFilter::All.condition_count(), 0);
    }
}
