//! 权限判定实现（[`PermissionChecker`]）：缓存 + **数据库 epoch 主动失效**。
//!
//! # 为什么要有 epoch
//!
//! 缓存 TTL 60 秒意味着「撤销权限后 60 秒内仍可用」—— 管理场景不可接受。
//! 所以：
//!
//! - 任何**角色 / 授权 / 绑定 / 继承 / 数据范围**的写操作，都在同一事务里
//!   `epoch = epoch + 1`；
//! - 判定前先读一次 epoch（一次极小的主键查询），与缓存里的 epoch 一致才用缓存。
//!
//! **失效信号来自共享的数据库** —— 多个进程实例各自持有缓存，但都会看到同一个 epoch，
//! 因此撤销在**下一个请求**即对所有实例生效（AC-S-047）。
//!
//! # 为什么 trait 的方法是同步的
//!
//! [`PermissionChecker`] 的方法全同步 → 可以 `Arc<dyn PermissionChecker>`（dyn 兼容）。
//! 代价是**读库必须发生在方法之外**：[`RbacChecker::ensure_fresh`] 是异步的，
//! 由认证中间件在请求进入时调用；同步方法只读**已经填好的缓存**。
//! 缓存里没有该用户 → **判定不通过**（fail closed）。

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use platform_core::RequestContext;
use platform_core::extension::expr::{CompiledCondition, EvalContext, compile};
use platform_core::extension::permission::{DataScope, PermissionChecker, ScopeFilter};

use crate::scope::{CombinedScope, ScopeContribution, ScopePolicy, ScopeResolver, combine_scopes};
use crate::service::RbacService;

/// 缓存的兜底 TTL（**不是**失效的主机制 —— 主机制是 epoch）。
pub const CACHE_TTL: Duration = Duration::from_secs(60);

/// 注册表缓存：`(载入时刻, 权限点 → 条件表达式)`。
type RegistryCache = (Instant, BTreeMap<String, Option<String>>);

/// 判定失败时的「永不匹配」条件（fail closed）。
///
/// 条件引用白名单外的属性时用它 —— **不是**「忽略条件放行」。
fn never_match() -> CompiledCondition {
    CompiledCondition {
        sql: "1 = 0".to_string(),
        values: Vec::new(),
    }
}

/// 编译权限点携带的**属性条件**。
///
/// **编译失败 → 永不匹配**（fail closed）—— 引用白名单外的属性、语法错、
/// 函数调用都会落到这里。**不是**「忽略条件放行」。
fn compile_condition(expression: &str, eval: &EvalContext<'_>) -> CompiledCondition {
    compile(expression, eval).unwrap_or_else(|_| never_match())
}

/// 一个用户在一个租户下的判定缓存。
#[derive(Debug, Clone)]
struct CacheEntry {
    /// 载入时的 epoch。
    epoch: i64,
    /// 载入时刻（兜底 TTL）。
    loaded_at: Instant,
    /// 展开后的有效权限点集合。
    permissions: BTreeSet<String>,
    /// 权限点 → 有效数据范围过滤条件。
    scopes: BTreeMap<String, ScopeFilter>,
    /// 单种 scope 的过滤条件（[`PermissionChecker::scope`] 用）。
    base_scopes: BTreeMap<DataScope, ScopeFilter>,
    /// 权限点 → 属性条件（编译后）。
    conditions: BTreeMap<String, CompiledCondition>,
}

/// 权限判定器。
///
/// **两个实例共享同一个数据库时，撤销仍然在下一个请求生效** ——
/// 失效信号来自 epoch，不是进程内的广播。
#[derive(Debug)]
pub struct RbacChecker {
    service: Arc<RbacService>,
    cache: Mutex<BTreeMap<(String, String), CacheEntry>>,
    registry: Mutex<Option<RegistryCache>>,
}

impl RbacChecker {
    /// 构造判定器。
    pub fn new(service: Arc<RbacService>) -> Self {
        Self {
            service,
            cache: Mutex::new(BTreeMap::new()),
            registry: Mutex::new(None),
        }
    }

    /// 底层服务。
    pub fn service(&self) -> &Arc<RbacService> {
        &self.service
    }

    /// 缓存条目里的权限点集合（调试与测试用）。
    pub fn cached_permissions(&self, tenant_id: &str, user_id: &str) -> Option<Vec<String>> {
        let cache = self.cache.lock().ok()?;
        let entry = cache.get(&(tenant_id.to_string(), user_id.to_string()))?;
        Some(entry.permissions.iter().cloned().collect())
    }

    /// **刷新**判定缓存（认证中间件在请求进入时调用）。
    ///
    /// - epoch 变了 → 重算（撤销立即生效）；
    /// - epoch 没变但超过 TTL → 重算（兜底）。
    pub async fn ensure_fresh(&self, ctx: &RequestContext) -> Result<(), crate::error::RbacError> {
        let (tenant_id, user_id) = match (ctx.tenant_id(), ctx.user_id()) {
            (Some(tenant), Some(user)) => (tenant.to_string(), user.to_string()),
            _ => return Ok(()),
        };

        let epoch = self
            .service
            .store()
            .current_epoch(ctx)
            .await
            .map_err(|error| crate::error::RbacError::Internal {
                reason: format!("the cache epoch could not be read: {error}"),
            })?;

        {
            let cache = self.cache.lock().ok();
            if let Some(entry) = cache
                .as_ref()
                .and_then(|cache| cache.get(&(tenant_id.clone(), user_id.clone())))
            {
                if entry.epoch == epoch && entry.loaded_at.elapsed() < CACHE_TTL {
                    return Ok(());
                }
            }
        }

        let entry = self.load(ctx, &tenant_id, &user_id, epoch).await?;
        let mut cache = self
            .cache
            .lock()
            .map_err(|_| crate::error::RbacError::Internal {
                reason: "the permission cache lock was poisoned".to_string(),
            })?;
        cache.insert((tenant_id, user_id), entry);
        Ok(())
    }

    /// 重算一个用户的判定结果。
    async fn load(
        &self,
        ctx: &RequestContext,
        _tenant_id: &str,
        user_id: &str,
        epoch: i64,
    ) -> Result<CacheEntry, crate::error::RbacError> {
        let registry = self.registry_map().await?;
        let graph = self.service.load_graph(ctx).await?;

        // 展开后的有效权限点集合。
        let permissions = crate::service::expand_user_permissions(&graph, user_id, &{
            registry.keys().cloned().collect::<Vec<_>>()
        });

        let resolver = ScopeResolver::new(self.service.store(), self.service.policy());
        let policy: &ScopePolicy = self.service.policy();

        // 单种 scope 的过滤条件。
        let mut base_scopes: BTreeMap<DataScope, ScopeFilter> = BTreeMap::new();
        for scope in [
            DataScope::All,
            DataScope::OwnOrg,
            DataScope::OwnOrgAndBelow,
            DataScope::OnlySelf,
        ] {
            let filter = resolver.resolve_one(ctx, scope).await?;
            base_scopes.insert(scope, filter);
        }

        // 每个权限点的有效范围（多角色取最宽松）。
        let mut scopes: BTreeMap<String, ScopeFilter> = BTreeMap::new();
        for permission in &permissions {
            let contributions = crate::service::contributions_for(&graph, user_id, permission, &{
                registry.keys().cloned().collect::<Vec<_>>()
            });
            let combined = combine_scopes(&contributions);
            let filter = resolve_combined(&resolver, ctx, &combined, &base_scopes).await?;
            scopes.insert(permission.clone(), filter);
        }

        // 权限点携带的属性条件（属性级校验）。
        let whitelist = policy.whitelist();
        let eval = EvalContext {
            whitelist: &whitelist,
            user_id: ctx.user_id(),
            tenant_id: ctx.tenant_id(),
        };
        let mut conditions: BTreeMap<String, CompiledCondition> = BTreeMap::new();
        for permission in &permissions {
            let Some(source) = registry.get(permission) else {
                continue;
            };
            let Some(expression) = source else {
                continue;
            };
            // 编译失败 → **永不匹配**（fail closed），不是「忽略条件放行」。
            let compiled = compile_condition(expression, &eval);
            conditions.insert(permission.clone(), compiled);
        }

        Ok(CacheEntry {
            epoch,
            loaded_at: Instant::now(),
            permissions,
            scopes,
            base_scopes,
            conditions,
        })
    }

    /// 权限点注册表（编码 → 条件表达式），带 60 秒兜底 TTL。
    async fn registry_map(
        &self,
    ) -> Result<BTreeMap<String, Option<String>>, crate::error::RbacError> {
        {
            let guard = self.registry.lock().ok();
            if let Some((loaded_at, map)) = guard.as_ref().and_then(|entry| entry.as_ref()) {
                if loaded_at.elapsed() < CACHE_TTL {
                    return Ok(map.clone());
                }
            }
        }

        let rows = self
            .service
            .store()
            .list_permissions()
            .await
            .map_err(|error| crate::error::RbacError::Internal {
                reason: format!("the permission registry could not be read: {error}"),
            })?;
        let mut map = BTreeMap::new();
        for row in rows {
            map.insert(row.code, row.condition);
        }

        let mut registry = self
            .registry
            .lock()
            .map_err(|_| crate::error::RbacError::Internal {
                reason: "the permission registry lock was poisoned".to_string(),
            })?;
        *registry = Some((Instant::now(), map.clone()));
        Ok(map)
    }
}

/// 把合并后的范围解析成过滤条件（能复用已算好的单 scope 过滤条件时就复用）。
async fn resolve_combined(
    resolver: &ScopeResolver<'_>,
    ctx: &RequestContext,
    combined: &CombinedScope,
    base_scopes: &BTreeMap<DataScope, ScopeFilter>,
) -> Result<ScopeFilter, crate::error::RbacError> {
    match combined {
        CombinedScope::All => Ok(ScopeFilter::All),
        CombinedScope::OnlySelf => Ok(base_scopes
            .get(&DataScope::OnlySelf)
            .cloned()
            .unwrap_or_else(fail_closed)),
        // 「取所有成员节点的并集」是**另一种范围** —— 不能复用默认（只按主部门）
        // 算出来的那一份，必须重新求值。
        CombinedScope::OwnOrg {
            use_all_memberships: true,
        } => Ok(resolver
            .resolve(ctx, combined)
            .await
            .unwrap_or_else(|_| fail_closed())),
        CombinedScope::OwnOrgAndBelow {
            use_all_memberships: true,
        } => Ok(resolver
            .resolve(ctx, combined)
            .await
            .unwrap_or_else(|_| fail_closed())),
        CombinedScope::OwnOrg {
            use_all_memberships: false,
        } => Ok(base_scopes
            .get(&DataScope::OwnOrg)
            .cloned()
            .unwrap_or_else(fail_closed)),
        CombinedScope::OwnOrgAndBelow {
            use_all_memberships: false,
        } => Ok(base_scopes
            .get(&DataScope::OwnOrgAndBelow)
            .cloned()
            .unwrap_or_else(fail_closed)),
        CombinedScope::Custom { .. } => Ok(resolver
            .resolve(ctx, combined)
            .await
            .unwrap_or_else(|_| fail_closed())),
    }
}

/// fail closed 的过滤条件：**一行都匹配不到**。
fn fail_closed() -> ScopeFilter {
    ScopeFilter::OrgNodes {
        column: "org_node_id".to_string(),
        nodes: Vec::new(),
    }
}

impl PermissionChecker for RbacChecker {
    fn has(&self, ctx: &RequestContext, permission: &str) -> bool {
        let (Some(tenant), Some(user)) = (ctx.tenant_id(), ctx.user_id()) else {
            return false;
        };
        let Ok(cache) = self.cache.lock() else {
            return false;
        };
        let Some(entry) = cache.get(&(tenant.to_string(), user.to_string())) else {
            // 缓存里没有这个用户 → **判定不通过**（fail closed）。
            return false;
        };
        entry.permissions.contains(permission)
    }

    fn scope(&self, ctx: &RequestContext, scope: DataScope) -> ScopeFilter {
        let (Some(tenant), Some(user)) = (ctx.tenant_id(), ctx.user_id()) else {
            return fail_closed();
        };
        let Ok(cache) = self.cache.lock() else {
            return fail_closed();
        };
        let Some(entry) = cache.get(&(tenant.to_string(), user.to_string())) else {
            return fail_closed();
        };
        entry
            .base_scopes
            .get(&scope)
            .cloned()
            .unwrap_or_else(fail_closed)
    }

    fn effective_scope(&self, ctx: &RequestContext, permission: &str) -> ScopeFilter {
        let (Some(tenant), Some(user)) = (ctx.tenant_id(), ctx.user_id()) else {
            return fail_closed();
        };
        let Ok(cache) = self.cache.lock() else {
            return fail_closed();
        };
        let Some(entry) = cache.get(&(tenant.to_string(), user.to_string())) else {
            return fail_closed();
        };
        entry
            .scopes
            .get(permission)
            .cloned()
            .unwrap_or_else(fail_closed)
    }

    /// 该权限点携带的**属性条件**；**没有条件**时返回 `None`。
    ///
    /// **非法条件返回「永不匹配」而不是 `None`**（BA 决策 D40）——
    /// `None` 只表示「无条件」；把非法条件塞进 `None` 会让调用方
    /// （`if let Some(c) = ... { 应用条件 }`）把它当成「无条件」而**静默放行**。
    fn condition(&self, ctx: &RequestContext, permission: &str) -> Option<CompiledCondition> {
        let (tenant, user) = (ctx.tenant_id()?, ctx.user_id()?);
        let cache = self.cache.lock().ok()?;
        let entry = cache.get(&(tenant.to_string(), user.to_string()))?;
        entry.conditions.get(permission).cloned()
    }
}

/// 一个角色的范围贡献（供 HTTP 层展示「用了自定义范围」）。
pub fn describe_contributions(contributions: &[ScopeContribution]) -> String {
    contributions
        .iter()
        .map(|item| item.scope.as_str())
        .collect::<Vec<_>>()
        .join(",")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scope::ScopePolicy;
    use platform_core::extension::expr::FieldWhitelist;

    /// 求值上下文（白名单来自平台的 `ScopePolicy`）。
    fn eval_context<'a>(whitelist: &'a FieldWhitelist) -> EvalContext<'a> {
        EvalContext {
            whitelist,
            user_id: Some("u-1"),
            tenant_id: Some("tenant-a"),
        }
    }

    /// 白名单内的属性 → 编译成**真实**条件（不是永不匹配）。
    #[test]
    fn a_condition_on_a_whitelisted_attribute_is_compiled() {
        let whitelist = ScopePolicy::default().whitelist();
        let eval = eval_context(&whitelist);
        let compiled = compile_condition("$record.amount <= 10000", &eval);
        assert_ne!(
            compiled.sql, "1 = 0",
            "白名单内的属性必须编译成真实条件：{}",
            compiled.sql
        );
        assert_eq!(compiled.placeholder_count(), compiled.values.len());
    }

    /// **白名单外的属性 → 永不匹配**（fail closed）—— 不是「忽略条件放行」。
    #[test]
    fn a_condition_outside_the_whitelist_never_matches() {
        let whitelist = ScopePolicy::default().whitelist();
        let eval = eval_context(&whitelist);
        let compiled = compile_condition("$record.secret_field == \"x\"", &eval);
        assert_eq!(
            compiled.sql, "1 = 0",
            "白名单外的属性必须落到永不匹配（fail closed）：{}",
            compiled.sql
        );
        assert!(compiled.values.is_empty(), "永不匹配的条件不绑定任何值");
    }

    /// 函数调用 / 多语句同样编译失败 → 永不匹配。
    #[test]
    fn a_forbidden_expression_never_matches() {
        let whitelist = ScopePolicy::default().whitelist();
        let eval = eval_context(&whitelist);
        for expression in [
            "now() > 0",
            "$record.amount <= 10000; DROP TABLE t",
            "$record.amount +",
        ] {
            let compiled = compile_condition(expression, &eval);
            assert_eq!(
                compiled.sql, "1 = 0",
                "禁止的表达式必须落到永不匹配（fail closed）：{expression} → {}",
                compiled.sql
            );
        }
    }
}
