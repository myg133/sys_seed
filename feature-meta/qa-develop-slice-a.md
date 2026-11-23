# develop 切片 A（安全核心）独立 QA

范围：`develop @ 164e32a`，crate = platform-iam / platform-rbac / platform-oauth / platform-org 的 `src/`（约 31.6k 行）。
方法：定向 grep 高危模式 + 真库（`base-postgres` 容器，`base_test`）SQL 核查 + 单 crate 测试。

## 结论

**不能按现状放心进下一个 Sprint —— 有 1 条 BLOCKER 级安全缺口，且已实测可利用。**

最严重的一条是 **F1 `platform-org` 完全无鉴权**：`manifest.json` 登记了 9 个 `org:*` 权限点，
但 `crates/platform-org/src/` 内**一个鉴权调用点都没有**（`grep require_permission` 零命中），
17 个 HTTP handler 全部只取 `auth: Authenticated`。我在**真实装配栈**上实测：只持 `app:registry:read`、
无任何 `org:*` 权限点的只读用户，`POST /api/org/org-node` 返回 **201 Created**，组织节点被真实写入。
组织树是 RBAC `OWN_ORG` / `OWN_ORG_AND_BELOW` 数据范围的**根数据**，改组织树即间接扩大数据可见范围
—— 这是一条「越权写 → 提权」的完整链。

根因是 **F2**：C-5 契约检查（「已登记权限点必须有鉴权调用点」）只挂在 `modules/sandbox` 一个地方，
四个核心 crate 从未被检查。不修 F2，同类缺口会复发。

第二档是 **F3/F4（已实测)**:MFA 的「一次性」在**并发**下不成立 —— 同一 TOTP 码 / 同一恢复码
12 路并发，**12 次全部成功**。仓库现有测试是串行的，所以全绿但漏掉了这个面。

正面结论也要说：**JWT 验签、租户隔离的数据库层、data scope 落 SQL、PKCE、OAuth 授权码交换、
OAuth 内省/撤销、LDAP 注入防护、RBAC 权限缓存失效**这些核心机制都做得扎实（见「已查确认无问题的项」）。
问题不在底座设计，而在**执行收口**：鉴权调用点漏了、一次性凭据的并发判定漏了。


## FINDING 清单

| # | 严重级 | 位置(file:line) | 问题 | 证据 |
|---|---|---|---|---|
| F1 | **BLOCKER** | `crates/platform-org/src/http.rs:281-313`（router）；17 个 handler `:365`–`:938` | 组织架构全部端点只认证不鉴权；manifest 登记 9 个权限点，代码 0 个调用点 | 实测：无任何 `org:*` 的只读用户 `POST /api/org/org-node` → **201 Created** |
| F2 | **MAJOR** | `crates/platform-contracts-macros/src/lib.rs:82`；`modules/sandbox/tests/contracts.rs:89` | C-5 契约检查只覆盖 `modules/sandbox`，四个核心 crate 从未被检查 | 全仓 `grep -rn platform_module_contract` 仅 1 处命中 |
| F3 | **MAJOR** | `crates/platform-iam/src/service.rs:2005-2012`；`store.rs:845-847` | MFA 时间步「读-校验-无条件写」TOCTOU，同一 TOTP 码并发可复用 | 实测 12 路并发同一 TOTP → **12/12 成功(204)** |
| F4 | **MAJOR** | `crates/platform-iam/src/service.rs:2024-2031`；`store.rs:892-904` | 恢复码消费丢弃 affected 行数，并发下同一恢复码可重复使用 | 实测 12 路并发同一恢复码 → **12/12 成功(204)** |
| F5 | **MAJOR** | `crates/platform-iam/src/totp.rs:7-8`；`store.rs:891` | 注释宣称「同一时间步不能用第二次」「一次性」，并发下不成立 | 与 F3/F4 实测结果直接冲突 |
| F6 | MINOR | `error.rs:431`(iam)/`:178`(rbac)/`:241`(oauth)/`:219`(org) | 500 响应体回显 `Internal{reason}` 原文，无统一脱敏 | iam 已示范脱敏(`http.rs:307-309`)，其余三处未做 |
| F7 | NIT | `crates/platform-iam/src/service.rs:137-152`；`http.rs:767-770` | 密码/令牌请求体派生 `Debug`，明文可被 `?req` 打进日志 | 当前无活跃打印，属潜在泄露面 |
| F8 | MINOR | `crates/platform-rbac/src/http.rs:774-788` | `TenantCtx` 缺上下文时静默兜底而非 401（纵深防御缺口） | 与 org `Authenticated`(`:141` 直接 401) 形态不一致 |

## 逐条详情

### F1 · BLOCKER · 组织架构端点无权限校验（任意已认证用户可改组织树）—— **已实测复现**

**规格要求**：`crates/platform-org/manifest.json` 登记了 9 个权限点，其中
`org:org-node:read`（查看组织架构）、`org:org-node:write`（编辑组织架构）、
`org:user-role` 类写权限、`org:scim-sync:run`（触发 SCIM 全量同步）等。
`crates/xtask/src/catalog.rs:56` 明确写：C-5 = 「已登记权限点必须有鉴权调用点」。

**实测代码**：`crates/platform-org/src/http.rs:281-313` 的 `router()` 把 13 条路由
直接 `with_state` 返回，**没有任何** `route_layer`；对比 `platform-rbac/src/http.rs:449-472`
逐组挂 `require_permission("rbac:role:write")`。

写操作 handler（`create_org_node`，`http.rs:383-411`）：
```rust
async fn create_org_node(
    State(state): State<Arc<OrgState>>,
    auth: Authenticated,     // ← 只验令牌
    axum::Json(body): axum::Json<CreateOrgNodeRequest>,
) -> Response {
    let ctx = authenticated_context(&auth);
    ...
    match state.service.create_node(&ctx, input).await { ... }
```
全 `src/` 范围内 `grep -rn "require_permission\|PermissionChecker\|has_permission\|permission"`
→ **No matches**。17 个 handler 全为 `auth: Authenticated`（`http.rs:365,385,414,435,460,482,509,576,612,632,680,715,742,814,856,900,938`）。

**为什么不可接受**：租户隔离（RLS + `TenantQuery`）只保证「不能跨租户」，
**不保证「同租户内不能越权」**。任何本租户内权限最低的账号（如只读员工）
都能 `POST /api/org/org-node` 增删节点、`/api/org/user/{id}/manager` 改他人汇报关系、
`/api/org/scim/full-sync` 触发全量同步。组织架构是 RBAC data scope 的
`OWN_ORG`/`OWN_ORG_AND_BELOW` 的**根数据** —— 改组织树 = 改所有人的数据可见范围，
即「越权改组织 → 间接扩大数据范围」的提权链。

**测试盲区**：`crates/platform-org/tests/` 全目录对 403/`insufficient` 零断言，
只有跨租户 404 断言（`org_real_db.rs:581`）；且全仓 `grep "/api/org"` 在 tests 下**零命中**
—— `/api/org` 没有任何 HTTP 层测试。

**实测证据（非推断）**：在 `platform-server/tests/app_real_stack.rs` 的**真实装配栈**上
临时挂入 `/api/org`，用**只持 `app:registry:read`、无任何 `org:*` 权限点**的只读用户令牌
（`world.read_token()`）调用 `POST /api/org/org-node`：

```
QA-RESULT-ORG: status=201 Created,
body={"id":"1e6b8363-...","parentId":null,"nodeType":"dept","code":"qa-dept","name":"QA 部门",...}
```

**201 Created —— 组织节点被真实创建**。对照：同一只读用户对 `/api/rbac/roles`（有权限层）
应是 403。这证明缺口不是理论上的：**无权限用户实际写入了组织树**。
（同一实验用非法 `nodeType` 时返回 422 而非 401/403，进一步说明请求已穿过鉴权到达业务逻辑。）
实验改动已从工作区移除，`git status` 干净。

**建议**：按 manifest 权限点逐组挂 `route_layer(require_permission("org:..."))`，
read/write 分开；并为每个端点补 403 用例。

### F2 · MAJOR · C-5 契约检查只覆盖 `modules/sandbox`，四个核心 crate 从未被检查

**规格要求**：`crates/xtask/src/catalog.rs:56`「C-5：已登记权限点必须有鉴权调用点」；
`crates/platform-contracts/src/lib.rs:29` 同样声明。

**实测**：C-5 由 `#[platform_module_contract]` 宏生成测试
（`crates/platform-contracts-macros/src/lib.rs:82`），
但全仓 `grep -rn "platform_module_contract"` 只有一处命中：
`modules/sandbox/tests/contracts.rs:89`。

**为什么不可接受**：F1 之所以能「登记 9 个权限点 / 0 个调用点」而不被任何 CI 拦住，
正是因为 C-5 从未对 `platform-org` 执行。这条是 F1 的**根因**，不修则同类缺口会复发。

**建议**：把契约宏挂到四个核心 crate 的 tests 上（或把 C-5 改成扫描整个 workspace）。

---

### F3 · MAJOR · MFA 时间步更新是 TOCTOU：同一 TOTP 码可并发复用（**已实测复现**）

**位置**：`crates/platform-iam/src/service.rs:2005-2012` + `crates/platform-iam/src/store.rs:838-851`

**实测代码**：
```rust
// service.rs:2005
let step = totp::verify_code(&raw, code, secret.last_used_step, now, &self.config.totp)?;
self.store.set_mfa_last_step(ctx, user_id, step).await.map_err(internal)?;
```
```rust
// store.rs:845 —— 无条件 UPDATE，没有 `AND last_used_step < ?`
"UPDATE {table} SET last_used_step = ? WHERE tenant_id = {{tenant}} AND user_id = ?"
```

**触发条件**：同一 TOTP 码在两个并发请求里同时到达（登录 + 敏感操作，或双开两个登录）。
两个请求都读到相同的旧 `last_used_step`，`verify_code` 都通过；两次 UPDATE 都成功。

**影响**：AC-S-015「同一时间步的验证码不能用第二次」在并发下不成立 —— 一次性保证退化为
「串行时成立」。攻击场景：攻击者截获一个 TOTP 码（30 秒窗口）后在窗口内并发重放。

**对比**：同文件的 refresh token 路径做对了 —— `find_refresh_by_hash_for_update_in`
用了 `FOR UPDATE`（`store.rs:728`），并在事务内判定 `used_at`。
恢复码路径也做了条件化（`store.rs:899` `AND used_at IS NULL`），说明团队知道这个模式。

**实测证据（非推断）**：临时并发测试（12 路 `tokio::spawn` + `Barrier` 对齐起跑线，
同一 TOTP 码同时 `POST /api/iam/mfa/verify`），两轮均得：

```
QA-RESULT-TOTP: statuses = [204,204,204,204,204,204,204,204,204,204,204,204]; successes=12
```

12 次全部 `204 No Content`（成功）。对照：仓库现有测试
`totp_codes_cannot_be_replayed_and_recovery_codes_are_single_use`
（`crates/platform-iam/tests/iam_real_db.rs:857`）是**串行**两次调用 → 绿。
即「串行一次性成立、并发一次性不成立」。实验文件已从工作区移除，`git status` 干净。

**建议**：改成条件更新并检查返回行数：
`UPDATE ... SET last_used_step = ? WHERE ... AND (last_used_step IS NULL OR last_used_step < ?)`，
`affected == 0` → `MfaInvalid`。

---

### F4 · MAJOR · 恢复码消费未检查受影响行数，并发下可重复使用（**已实测复现**）

**位置**：`crates/platform-iam/src/service.rs:2024-2031`

**实测代码**：
```rust
if candidate.code_hash == expected {
    self.store
        .mark_recovery_code_used(ctx, &candidate.id)
        .await
        .map_err(internal)?;      // ← 返回的 u64 被丢弃
    return Ok(());
}
```

**证据**：`mark_recovery_code_used` 返回 `DbResult<u64>`
（`crates/platform-iam/src/store.rs:892-904`），SQL 已正确条件化为
`AND used_at IS NULL`。但调用点用 `.map_err(internal)?` 把 `u64` 丢掉，
**没有**判 `== 0`。

**触发条件**：两个并发请求携带同一个恢复码。两者都从 `unused_recovery_codes` 读到该行，
两条 UPDATE 只有一个真正生效（另一个 affected=0），但**两个请求都返回 `Ok(())`**。

**影响**：一次性凭据在并发下可成功两次 —— 与 F3 同类，但 F3 是「读侧 TOCTOU」，
这条是「写侧结果被忽略」，修法只需判返回值，成本极低。

**实测证据（非推断）**：12 路并发携带**同一个**恢复码，三轮均得：

```
QA-RESULT: statuses = [204,204,204,204,204,204,204,204,204,204,204,204]; successes=12
```

12 次全部 204 成功 —— 一次性凭据被用 12 次。
注：低并发（2 路）时调度恰好串行化，只出现 1 次成功（可解释为「窗口窄」而非「不存在」）；
12 路 + barrier 后稳定 12/12 全通过。实验文件已移除，`git status` 干净。

**建议**：`if affected == 0 { return Err(IamError::MfaInvalid) }`。

---

### F5 · MAJOR · 注释声明的「一次性」保证只对串行成立（F3/F4 的注释面）

**位置**：`crates/platform-iam/src/totp.rs:7-8`、`crates/platform-iam/src/store.rs:891`

**注释原文**：
```
//! - **防重放**：记录最近一次成功的时间步，**同一时间步的验证码不能复用**。
```
```
/// 标记恢复码**已使用**（一次性）。
```

**实测落差**：注释是无条件表述（「不能复用」「一次性」），
但 F3 实测同一 TOTP 码 12 路并发 → 12 次全部成功；F4 实测同一恢复码 12 路并发 → 12 次全部成功。
注释描述的安全属性在并发下**不成立**，且注释未标注这一边界。

**为什么单列**：项目把「注释与代码不一致」列为硬规矩。这里的代码**意图**是对的
（`store.rs:899` 确实写了 `AND used_at IS NULL`），但**执行层**丢掉了结果判定，
注释读起来像已闭合的保证。读者（含后续维护者）据此会误判风险面。

**建议**：要么按 F3/F4 修好使注释成立，要么在注释里显式写明「串行一次性；并发保护由
条件更新 + affected==0 判定承担」——但修好是唯一正确选项。

---

### F6 · MINOR · 500 响应体回显内部错误原文（四 crate 同构）

**位置**：`crates/platform-iam/src/error.rs:431`、`crates/platform-rbac/src/error.rs:178`、
`crates/platform-oauth/src/error.rs:241`、`crates/platform-org/src/error.rs:219`

**实测代码**（四处一致）：
```rust
Self::Internal { reason } => format!("an internal error occurred: {reason}"),
```
`reason` 进 `ProblemDetails.detail` → 直接进 HTTP 响应体（`problem_details` 无脱敏层）。

**收紧后的实际风险（区分 confirmed / hypothesis）**：
- **confirmed**：`platform-org/src/error.rs:265-269` 的 `From<anyhow::Error>` 用
  `error.to_string()` 作 reason —— 若将来 anyhow 链上挂了 sqlx 错误，会把 SQL 片段带进响应体。
  当前生产路径**未**走该转换（`grep anyhow::` 在 org/src 只有 notify/audit），所以**当前不可达**。
- **confirmed**：`platform-iam/src/http.rs:307-309` 已示范了正确做法 —— 把 DbError 替换成
  固定串 `"the data access layer failed"`。org/rbac/oauth 没有等价脱敏。
- **hypothesis**：org 的 `From<DbError>`（`error.rs:273-296`）只透传稳定错误码
  （`error.code()` 如 `DB_ERROR`），不含 SQL —— 这条**没有**泄露。

**为什么不是 BLOCKER**：当前可达路径的 reason 多为稳定错误码或自造文案，未观察到 SQL/表名外泄。
但它是一条「一改就漏」的线：任何人在 `Internal { reason }` 里塞 `format!("{err}")` 就会漏。

**建议**：统一按 iam 的做法，在错误 → ProblemDetails 的边界把 `Internal` 的 detail
替换成固定文案，原文只进日志。

---

### F7 · NIT · 密码/令牌请求体派生 `Debug`（潜在日志泄露面）

**位置**：`crates/platform-iam/src/service.rs:137-152`、`crates/platform-iam/src/http.rs:767-770`

**实测**：`ResetPasswordRequest { token, new_password }`、
`ChangePasswordRequest { current_password, new_password }` 均 `#[derive(Debug, ...)]`，
字段是 `pub String` 明文。

**当前无活跃泄露**：`grep "{:?}\|{req:?}\|dbg!"` 在 iam/oauth/src 无命中，没有代码真的打印它们。

**为什么仍记**：一条 `tracing::debug!(?req)` 就会把明文密码写进日志。
建议手写 `Debug` 实现打码，或去掉 `Debug` 派生。

---

### F8 · MINOR · `TenantCtx` 提取器在上下文缺失时静默兜底而非拒绝

**位置**：`crates/platform-rbac/src/http.rs:774-788`

**实测代码**：
```rust
let ctx = parts.extensions.get::<RequestContext>().cloned()
    .unwrap_or_else(RequestContext::new);   // ← 缺上下文 → 空 ctx，不报错
Ok(TenantCtx(ctx))
```

**为什么当前不可利用**：`/api/rbac` 的所有路由都挂了 `authenticate` 中间件
（`http.rs:492-497`），中间件一定会写入带用户身份的 ctx；空 ctx 在
`require_permission::authorize` 里会因 `user_id().is_none()` 判 401（fail closed）。
所以这是**纵深防御缺口**而非可利用漏洞：一旦有人新增一条路由漏挂 `authenticate`，
该 handler 不会 401，而是拿着空 ctx 走进业务层（随后多半因 `TenantContextMissing` 报错，
但那已经是业务层判定，不是鉴权层）。

**对比**：`platform-org` 的 `Authenticated` 提取器在同样情况下**直接 401**
（`http.rs:141` `.ok_or_else(|| problem_response(&OrgError::Unauthenticated, ...))?`）
—— 这是更稳的形态。

**建议**：`TenantCtx` 改为缺 `user_id`/`tenant_id` 即 401，与 org 对齐。

## 已查确认无问题的项

- ✅ 已查：JWT 验签本体（`crates/platform-iam/src/jwt.rs:171-199`）——`leeway=0`、`required_spec_claims=["exp","sub","iss","aud"]`、`validate_exp/nbf/aud=true`、`iss`/`aud` 均设白名单、算法固定 ES256（`SIGNING_ALGORITHM` 常量，非从 token header 取）。不存在「只 base64 解 payload」的写法。
- ✅ 已查：`verify_access_token` 唯一实现（`crates/platform-iam/src/service.rs:1544-1547`）确实转调 `keys.verify`，无旁路。
- ✅ 已查：`TenantQuery::render`（`crates/platform-db/src/query.rs:251-287`）强制语句中**恰好一个** `{tenant}` 占位符，多/少均返回 `InvalidQuery`。漏租户在类型层面难以表达。
- ✅ 已查：四个 crate 的 `src/` 内无 `raw_sql` / `Pool<` / `acquire()` / 直连终端，全部经由 `TenantDb`/`TenantTx`。
- ✅ 已查：data scope 确实落到 SQL（`crates/platform-db/src/query.rs:356-386`）——`OrgNodes → col = ANY(?)` 数组绑定、`Owner → col = ?` 绑定、列名过 `safe_column`；不是内存过滤。
- ✅ 已查：PKCE 强制 S256（`crates/platform-oauth/src/pkce.rs:80-92`），真重算摘要而非原文比较。
- ✅ 已查：`constant_time_eq`（`crates/platform-oauth/src/client_auth.rs:155-164`）比较的是 SHA-256 摘要（定长），长度不一致分支不构成实际时序泄漏。
- ✅ 已查：授权码交换一次性（`crates/platform-oauth/src/service.rs:1118-1125`）——单条 DELETE + `deleted == 0` 判定；表无 `used_at` 列（D253）。
- ✅ 已查：`require_permission` 的 `authorize`（`crates/platform-core/src/extension/permission.rs:373-417`）fail-closed：无 user/tenant → 401；缺 checker → 500。
- ✅ 已查：真库 RLS 覆盖——`base_test` 中 org/rbac/oauth 的租户表均已 `rowsecurity=t` 且有 policy（`org_node`/`user_org`/`user_position`/`external_identity`/`sync_conflict`/`sync_source_state`、`platform_role*`/`platform_user_role`/`platform_permission_epoch`、`platform_oauth_{grant,code,device_code,token}`）。
- ✅ 已查：三张无 RLS 表均有正当理由——`platform_iam_oidc_state`/`platform_iam_sso_ticket`（256 位不可猜凭证，主键查，无权威租户来源）与 `platform_iam_login_rate`（全局 IP 限流，只存 IP 摘要，无租户维度）。

- ✅ 已查：`platform-oauth` 的独立验签 `keys.rs:89-137` 同样 `leeway=0`、强制 `exp`、逐字判 `iss`/`aud`、算法固定 ES256；`aud` 数组分支仅兼容外部格式（签发恒为单值 `String`），内省另用 `claims.client_id != client.client_id` 做强绑定。
- ✅ 已查：OAuth 授权码交换（`service.rs:1051-1125`）—— 跨租户定位后 `with_tenant(row.tenant_id)` 覆盖上下文、`client_id`/`redirect_uri` 逐字比对、PKCE 校验、单条 DELETE 保证一次性；`platform_oauth_code_by_hash` 为 `SECURITY DEFINER` + `SET search_path = public` + `LIMIT 1`，摘要 43 字符不可枚举。
- ✅ 已查：OAuth 内省（`service.rs:1573-1660`）—— 验签失败/过期/跨 client/缺 `tid` 一律 `active:false`，且按令牌自身 `tid` 覆盖 ctx 查状态；token 状态 + grant 状态双查，撤销即时生效。
- ✅ 已查：refresh token 轮换（`service.rs:1554-1630`）—— `FOR UPDATE` 行锁、`used_at` 重放检测排在 `revoked_at` 之前、family 作废后**提交**而非回滚。
- ✅ 已查：LDAP filter 注入防护（`crates/platform-iam/src/sso/ldap.rs:177-197`）—— `escape_filter_value` 覆盖 RFC 4515 全部五个字符 `\ * ( ) NUL`；`platform-org` 的 LDAP filter 来自配置、无用户输入插值。
- ✅ 已查：OIDC 回调租户绑定（`sso/oidc.rs:676-684`）—— 请求头租户与 state 租户不一致即拒；消费为租户作用域条件更新。
- ✅ 已查：RBAC 权限缓存失效（`crates/platform-rbac/src/checker.rs:115-147`）—— DB epoch 主失效 + TTL 兜底 + 每请求入口 `ensure_fresh`，多进程共享同一 epoch。
- ✅ 已查：密码存储（`crates/platform-iam/src/password.rs`）为 Argon2id + PHC 串，非明文；账号不存在走假哈希校验（`DummyHash`）防时序枚举。
- ✅ 已查：四个 crate 的 `src/` 内日志无令牌/密码/密钥打印（`grep tracing!/println! + password|token|secret` 无命中）。
- ✅ 已查：`platform_permission`（无 RLS）是代码派生的全局只读注册表，应用层只 `SELECT`（`store.rs:235-244`），无写路径。
- ✅ 已查：`platform-iam` 的 `tenant_context` 中间件用 `unwrap_or_default()` 传入空串，但 `RequestContext::with_tenant` 内 `normalize()` 把空串转 `None`（`context.rs:171-178`），不会被误判为「已认证租户」。
- ✅ 已查：测试基线全绿 —— `platform-org` 22、`platform-rbac` 83、`platform-iam` 180、`platform-oauth` 131 个用例（含 `--include-ignored` 真库用例）全部通过。

## 未验证项

以下项**未验证**，不得当作通过：

1. **启动时 RLS 自检的真实拒绝行为未实测**。读了 `main.rs:320-331` 的
   `verify_startup_isolation`（连角色绕过 RLS 即拒绝启动），但**没有**用一个超级用户
   连接实际启动进程验证它真的拒绝。当前库里策略存在是实测的（`pg_policies` 查询），
   但「配错角色会不会真的起不来」只有代码证据。
2. **生产角色的 RLS 实际生效性未端到端实测**。我用 `base`（可能是表属主/超级用户）
   查了 `pg_tables`/`pg_policies`，确认策略已建；但没有用**非超级用户的应用角色**
   跑一次「设 tenant_a 上下文去读 tenant_b 的行」的实测。仓库里有
   `crates/platform-app/tests/app_integration.rs:670` 这类测试，但我未逐条跑跨 crate 的隔离用例。
3. **F1 的完整影响面未枚举**。我实测了 `POST /api/org/org-node` 一条路径返回 201；
   推断其余 16 个 handler 同样无鉴权（代码层面一致：全部只取 `Authenticated`），
   但只实际发过 1 个请求。
4. **F3/F4 的攻击窗口宽度未量化**。实测 12 路并发 12/12 成功；但没有测「时间窗口内
   几次串行重放会失败」的边界，也未测真实网络延迟下的窗口大小。
5. **`platform-storage` / `platform-app` 等切片 B/C/D 的 crate 未看**，按要求排除。
6. **未做完整 workspace 编译/测试**（`cargo test --workspace`），只串行跑了四个目标 crate。
7. **OAuth 设备码流程（`service.rs:1229+`）的用户码枚举抗性未实测** —
   读了 `user_code_hash` 索引与 `poll_interval` 字段，但没验证「暴力猜 user_code」的限速行为。
