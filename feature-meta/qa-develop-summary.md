# develop 全量独立 QA 汇总（四片）

> 基线：`develop` @ `164e32a`（REQ-019 合并后）
> 方式：4 个**独立** verifier 子 agent 并行分片复查，覆盖全部生产代码（约 7.9 万行）
> BA 角色：**逐条亲验高危项**，不直接采信子 agent 结论

## 分片与报告

| 片 | 范围 | 行数 | 报告 |
|---|---|---|---|
| A | iam / rbac / oauth / org | ~31.6k | `qa-develop-slice-a.md`（335 行） |
| B | db / storage / audit / notify | ~18.4k | `qa-develop-slice-b.md`（435 行） |
| C | observability / core / xtask | ~16.8k | `qa-develop-slice-c.md`（203 行） |
| D | server / app / config / schema / contracts | ~16.6k | `qa-develop-slice-d.md` |

## 结论

**不能直接进 Sprint 7。** 底座设计扎实（多处正面确认，见下），但有 **2 条 BLOCKER** 必须修。

---

## BLOCKER（必须修，我亲验过）

### B1 · `platform-org` **完全没有鉴权**

**证据**（我亲验）：
```console
$ grep -o "org:[a-z-]*:[a-z-]*" crates/platform-org/manifest.json | sort -u
org:org-node:read  org:org-node:write  org:scim-sync:run
org:sync-conflict:read  org:sync-conflict:resolve
org:user-org:read  org:user-org:write
org:user-position:read  org:user-position:write
（9 个权限点）

$ grep -rc "require_permission" crates/platform-org/src/
0 处
```

17 个 handler 只取 `Authenticated`（只验「你是谁」，不验「你能不能」）。

**QA-A 实测可利用**：只持 `app:registry:read`、无任何 `org:*` 权限的只读用户，
`POST /api/org/org-node` → **201 Created**，组织节点真的被写入。

**为什么严重**：组织树是 `OWN_ORG` 数据范围的根 → 越权写 ≈ 提权链。

**根因**（B2）：C-5 契约检查（「已登记权限点必须有鉴权调用点」）由
`#[platform_module_contract]` 宏生成，而该宏**只挂在 `modules/sandbox` 一处**
（我亲验：`grep -rn "platform_module_contract"` 仅 1 个实际挂载点）。
→ 四个核心 crate（iam/rbac/oauth/org）**从未被 C-5 扫过**。

### B2 · C-5 契约检查只覆盖 sandbox

见 B1 的根因。这不是「org 一个 crate 的疏忽」，而是**检查器覆盖面**的缺陷 ——
修了 org 而不修 C-5，下次新 crate 还会犯。

---

## MAJOR

### M1 · `platform_audit_archive_move` **无 advisory lock**（QA-B F4，我亲验）

```console
$ awk 'NR>=608 && NR<=700' migrations/20261120000200_audit.up.sql | grep -c "advisory"
0
```
而 `platform_audit_append` 有（`migrations/20261120000200_audit.up.sql:457`
`PERFORM pg_advisory_xact_lock(hashtext('audit:' || p_tenant_id))`）。

两函数都读改写 `tip_hash` → 并发归档与写入会互相覆盖。
QA-B 用**仓库自己的** `ChainVerifier` 实测：链链接全部正确，但 `tip_ok=false`
→ 校验器报 `ok=false` + error 级告警（**误报断链**），3/3 复现。

### M2 · `platform_audit_append` 缺租户守卫（QA-B F1，我亲验）

`head_hash`（`:537`）/ `tip_hash`（`:571`）等三个 SECURITY DEFINER 函数都有：
```sql
IF p_tenant_id IS DISTINCT FROM current_setting('app.tenant_id', true) THEN
    RAISE EXCEPTION '... does not match the request context' USING ERRCODE = 'insufficient_privilege';
END IF;
```
`platform_audit_append`（`:416`）**没有**这一段。

QA-B 实测：上下文租户为 `tenant-VICTIM` 时，成功写入 `tenant-ATTACKER` 的链。

**影响面（我核对）**：`sink.rs` 始终传上下文租户 → **HTTP 路径不可利用**。
但这是**纵深防御缺口**：任何持有 `base_platform_app` 角色的代码都能跨租户伪造审计。

### M3 · `claim_batch` / `claim_jobs` 的 `LIMIT` 会溢出（QA-B F2/F3，**我实测修正了幅度**）

SQL 形态：`WHERE id IN (SELECT ... ORDER BY ... LIMIT ? FOR UPDATE SKIP LOCKED)`
→ 子查询在 semi-join 里被重扫，`LIMIT` 不能严格约束外层 UPDATE。

**我用仓库真实代码实测**（临时探针，已删）：
```
claim_batch(batch=10) 在突发 500 条队列上 → 认领 16 条（超出 60%）
claim_batch(batch=2)  在 6 条队列上        → 认领  3 条（超出 50%）
```

⚠️ **修正 QA-B 的幅度**：QA-B 报「LIMIT 10 认领全部 500 / 3000 条」，
我的实测是超 60%/50%。**方向对，幅度被夸大** → 定级 MAJOR 而非 BLOCKER。
（QA-B 自己在报告里多次记录「结果自相矛盾」「之前数是 grep 数错」，这部分不可靠。）

**修复已验证有效**（QA-B 做的）：改成 `= ANY(ARRAY(...))` → 严格 10 条。

### M4 · MFA 一次性凭据在**并发**下不成立（QA-A F3/F4）

- TOTP：`verify_code` 读 `last_used_step` 校验后**无条件** UPDATE → TOCTOU
- 恢复码：`mark_recovery_code_used` 的返回值被**丢弃**，`?` 只传播错误

QA-A 实测：同一 TOTP 码 / 同一恢复码各 **12 路并发 → 12 次全部成功**。

现有测试是**串行**的（先第一次、再第二次）→ 全绿但完全测不到并发面。

### M5 · `/api/observability/rate-limit/status` 无任何认证（QA-C，我亲验）

`main.rs:818` 直接 nest 裸 `router()`，无 `authenticate`。实测匿名 200，
可读全局限流配额（`requests` / `windowSeconds`）→ 攻击者可精确卡配额。

设计文档只写「本身也受限流保护」，**完全没提认证** → 这是设计遗漏。

### M6 · TD-076 装配无判据（QA-D，变异实证）

把 `/api/notify` 整条 `.nest()` 删掉 → `platform-server` **136 个测试全绿**。
把 `trace_id` 与 `rate_limit` 顺序对调 → `middleware_assembly` 11 个用例全绿。

⚠️ 这**推翻了我上一轮的说法**：我在 D346 / TD-075 里写过「顺序错了会被判据抓到」，
实际上只有 `client_info` 在不在最外层一条有判据，trace ↔ rate_limit 的相对顺序**没有**。

### M7 · `SET LOCAL` 语义无测试覆盖（QA-B F7，变异实证）

把 `set_config(..., true)` 变异成 `false`（提交后残留租户 → 串租户）→ **38 个测试全绿**。
这是租户隔离最关键的机制，却没有判据兜住。

### M8 · `platform_notify_delivery` / `platform_storage_job` 无清理路径（QA-B F6/F9）

同项目的 `platform_rate_limit` 有 `prune`，但这两张表每入队一条就加一行、无删除路径。

### M9 · `audit verify` 全量载入内存（QA-B F5）

`verify.rs:234-242` 把整条在线链读进 `Vec`，无 LIMIT。实测 5 万条约 29MB 载荷，
且是用户可反复调用的 GET。

---

## MINOR

| # | 内容 | 来源 |
|---|---|---|
| m1 | `prune` 注释承诺「传 0 → 60 兜底」，代码是 `.max(1) * 2`（传 0 保留 2 秒） | C，我亲验 `mod.rs:278` |
| m2 | `main.rs:794` 顺序注释漏 `deprecation` 一整层 | D，我亲验 |
| m3 | `service.rs:192` 审计 before 值读库失败时静默返回空 map、不打日志 | D，我亲验 |
| m4 | `ResetPasswordRequest` 等派生 `Debug` 含明文密码（无活跃 debug 打印） | A |
| m5 | `deprecation.rs` 内部注释自相矛盾（一处说用 Rfc7231，另两处说没有该 API） | C |
| m6 | 设计文档承诺的 `used` / `key` 字段未实现（非 AC 违规） | C |

---

## 正面确认（QA 查过没问题的项，摘录）

这批很重要 —— 说明**设计**是扎实的，问题在执行收口：

- **JWT 验签**：leeway=0、强制 exp/nbf/aud/iss、算法固定 ES256、无「解 payload 当验过」
- **租户隔离**：`TenantQuery::render` **强制恰好一个** `{tenant}` 占位符（结构性保证）
- **data scope 真落 SQL**：`= ANY(?)` 数组绑定 + `safe_column`，不是内存过滤
- **OAuth**：PKCE 强制 S256 且真算摘要；授权码一次性 DELETE 且绑定项逐字比对；内省/撤销链路完整
- **LDAP**：`escape_filter_value` 完整覆盖 RFC 4515 五个特殊字符
- **权限缓存**：epoch 主动失效 + TTL 兜底，无安全问题
- **RLS 实测生效**：`tenant-a` 见 1 行、`tenant-b` 见 0 行（读、写两侧）
- **storage 预签名**：先验签再解析、token 不能跨 key 重放
- **无 TODO/unimplemented**：生产代码零未完成标记
- **104 个 `#[ignore]` 全部有理由**，102 个是「需要真库」，全量跑带 `--include-ignored`

## 对 REQ-019 的专项验证（QA-C）

我上一轮的限流修复（D346 顶层验令牌）**经独立验证可信**：

1. `client_info` 确实只填 IP/UA → 限流执行时租户必为 `None` → 必然走令牌解析
2. `TokenTenantResolverImpl` 调 `verify_access_token`，做 JWK 验签 + exp/nbf/aud/iss 校验
3. `main.rs:807` 确实传了 `sso_iam`
4. **变异验证**：解析分支改恒 `None` → 组 A 变红（`middleware_assembly.rs:595`），还原复绿

三个 xtask 守卫（check-deps / capabilities / openapi）也经变异攻击验证有效。

---

## 一个方法论问题（必须记）

四片 QA **并行在同一个工作区**做变异实验 → `git status` 抖动：
- `openapi.json` 被写入过 `/api/fake/mutation-endpoint`
- `Cargo.lock` 短暂出现异常条目
- `iam_real_db.rs` 出现「QA 临时实验」用例

每片都报告自己已还原，最终 `git status` 干净。但**并行变异会互相污染**，
且会让「git status 干净」这个验证手段失效。

→ **下次派多片 QA 必须各用独立 worktree，或串行。**

## 结论与处置顺序

1. **B1 + B2**（org 鉴权 + C-5 扩覆盖）—— 修缺口 + 修检查器
2. **M1 + M2**（审计锁 + 租户守卫）—— 数据完整性
3. **M4**（MFA 并发）—— 判 `affected == 0` 即可
4. **M3**（claim 限额）—— 改 `= ANY(ARRAY(...))`
5. **M5**（端点认证）、**M6**（装配判据）、**M7**（SET LOCAL 判据）
6. **M8 + M9 + MINOR**
