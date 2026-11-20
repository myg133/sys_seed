# scope 三重交集与通配（AC-S-140）

- **覆盖 AC**: AC-S-140（8 条：TC-022-039 ~ 046）
- **前置基线**:
  - `client-A`：`allowed_scopes = ['rbac:role:read','rbac:role:write']`（**只这两个**）
  - `u-perm-read@example.com`（`tenant-a`）：**只**持 `rbac:role:read`（**没有** write）
  - `u-perm-write@example.com`（`tenant-a`）：持 `rbac:role:read` + `rbac:role:write`
- **实现事实（用例按此写）**:
  - **用户的有效权限点是「已展开通配」的集合**（`platform-core/src/extension/permission.rs:239-247`；
    `platform-rbac/src/service.rs:1013-1038` → `platform-rbac/src/entity.rs:232-241` 的 `expand()`）
    → **集合里永远只有注册表里的具体码，永不出现通配字面量**
  - `platform_permission` 表里**没有任何**含 `*` 的码（**实测**：`SELECT code … WHERE code LIKE '%*%'` → 空）
  - 权限点注册表：`crates/platform-rbac/manifest.json` → `rbac:role:read` / `rbac:role:write` /
    `rbac:user-role:write` / `rbac:permission:read`（**没有** `rbac:role:*`）

---

## AC-S-140: scope 是三重交集；通配不隐式生效；不得越过用户本人

### TC-022-039: 请求 scope = 用户有效权限点 ∩ client 白名单 → 授予成功且**恰好**等于

- **关联 AC**: AC-S-140
- **前置条件**: `u-perm-read@example.com` 已登录；`client-A` 的 `allowed_scopes` 含 `rbac:role:read`
- **测试步骤**:
  1. preview `scope=rbac:role:read` → confirm → 拿 `code`
  2. `POST /api/oauth/token` 换令牌
  3. 读响应体的 `scope`
- **预期结果**:
  1. 步骤 2：HTTP **200**
  2. 步骤 3 的 `scope` **逐字等于** `rbac:role:read`（**恰好 1 个**）
  3. 步骤 3 的 `scope` **不含** `rbac:role:write`
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 基本路径
- **备注**: AC 路径 1 的原文。

### TC-022-040: 请求的 scope **超出用户本人** → 400 `invalid_scope`（协议话）

- **关联 AC**: AC-S-140
- **前置条件**: `u-perm-read@example.com`（**没有** `rbac:role:write`）；`client-A` 的白名单**含** write
- **测试步骤**:
  1. preview `scope=rbac:role:read rbac:role:write` → 走到令牌交换
  2. 读响应体
- **预期结果**:
  1. HTTP **400**
  2. 响应体**逐字等于** `{"error":"invalid_scope"}`（**只含** `error` 键；**不含** `OAUTH_*` 平台码）
  3. **没有任何令牌被签发**（`platform_oauth_token` 里**没有**本次的 `jti` 行）
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 异常路径
- **备注**: D201 §2：「交集为空 → `invalid_scope`（400）」。
  第 3 条钉住「失败时不留半成品令牌」。

### TC-022-041: 请求的 scope **超出 client 白名单** → 400 `invalid_scope`

- **关联 AC**: AC-S-140
- **前置条件**: `u-perm-write@example.com`（**持有** `sandbox:demo:read`）；`client-A` 的白名单**不含**它
- **测试步骤**:
  1. preview `scope=sandbox:demo:read` → 走到令牌交换
  2. 读响应体
- **预期结果**:
  1. HTTP **400**
  2. 响应体**逐字等于** `{"error":"invalid_scope"}`
  3. `error_description`（若存在）**不含**任何 CJK 字符（契约 C14 §14.2）
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 异常路径
- **备注**: AC 路径 3 的原文。**用户有、client 没有** → 也拒 → 这一条与 TC-022-040 合起来
  才证明是**交集**而不是「只看某一维」。

### TC-022-042: `rbac:role:*` → 400 `invalid_scope`，且它**确实不是**注册过的权限点

- **关联 AC**: AC-S-140
- **前置条件**: `u-perm-write@example.com`（持 `rbac:role:read` + `rbac:role:write`）
- **测试步骤**:
  1. 以 `base` 直连查 `SELECT count(*) FROM platform_permission WHERE code = 'rbac:role:*'`
  2. `GET /api/rbac/me/permissions`（该用户）→ 读集合
  3. preview `scope=rbac:role:*` → 走到令牌交换 → 读响应
- **预期结果**:
  1. 步骤 1 的计数 **== 0**（`rbac:role:*` **不是**注册过的权限点）
  2. 步骤 2 的集合里**没有任何**含 `*` 的码（用户有效权限点是展开后的）
  3. 步骤 3：HTTP **400**，响应体**逐字等于** `{"error":"invalid_scope"}`
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 否定断言 + 集合精确匹配
- **备注**: 〔S-14〕〔AC 修正〕 —— AC 原文要求「二者必须能区分开（不是注册过的权限点 vs 通配被禁用）」，
  但它给的方法（用 `rbac:role:read` 走路径 1）**在两种实现下都成立** → 无区分力。
  本用例把 AC 想要的**前提**（两条事实）断言成可判的形式。

### TC-022-043: **把 `rbac:role:*` 登记进注册表** → 仍然 400（证明「令牌 scope 层面通配一律不生效」）

- **关联 AC**: AC-S-140
- **前置条件**: `u-perm-write@example.com`；管理员可直连 `platform_permission`
- **测试步骤**:
  1. 以 `base` 直连 `INSERT INTO platform_permission (code, name, kind, condition, source)
     VALUES ('rbac:role:*','QA 临时通配','button',NULL,'crates/platform-rbac/manifest.json')`
     （**仅本用例夹具**；用完在步骤 4 删除）
  2. 查 `SELECT count(*) FROM platform_permission WHERE code = 'rbac:role:*'` → **== 1**
  3. preview `scope=rbac:role:*` → 走到令牌交换 → 读响应
  4. 以 `base` 直连 `DELETE FROM platform_permission WHERE code = 'rbac:role:*'`
- **预期结果**:
  1. 步骤 2 的计数 **== 1**（前提成立）
  2. 步骤 3：HTTP **400**，响应体**逐字等于** `{"error":"invalid_scope"}`
     → **证明拒绝的理由不是「它没注册」**（它注册了照样被拒）
  3. 步骤 4 删除成功；删除后 `platform_permission` 的行数回到执行前
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 反向变异（把「不是注册过的」这个前提改掉，看判据是否还成立）
- **备注**: 〔S-14〕**这是唯一能把「两个原因」真正区分开的用例**：
  D201 §3 说「通配**只有**在它本身是注册过的权限点时才有效」（角色层面可以，令牌 scope 层面不行）→
  注册了却仍被拒 → 令牌层面**一律**拒绝通配。
  ⚠️ 本用例**直接改注册表** → 必须用 `base`（超管）连接、**必须**在结束时清理，
  且**不得**影响其它用例（步骤 4 的清理是硬要求）。

### TC-022-044: **后来被授予 write** 不得回溯扩大**已签发**的令牌 scope

- **关联 AC**: AC-S-140
- **前置条件**: `u-perm-read@example.com`（**只**有 `rbac:role:read`）
- **测试步骤**:
  1. 请求 `scope=rbac:role:read` → 交换**成功** → 令牌 A（记下 `jti`）
  2. 以 `base` 直连给该用户**追加** `rbac:role:write`
     （`platform_role_permission` 插一行 + bump `platform_permission_epoch`）
  3. **不再交换**，直接内省令牌 A → 读 `scope`
  4. 另发一枚令牌 B（请求 `scope=rbac:role:read rbac:role:write`）→ 读令牌 B 的 `scope`
- **预期结果**:
  1. 步骤 3：令牌 A 的 `scope` **逐字等于** `rbac:role:read`，
     **不含** `rbac:role:write`（**旧令牌不被回溯扩大**）
  2. 步骤 4：令牌 B 的 `scope` **包含** `rbac:role:read` 与 `rbac:role:write`
     （**新令牌拿到新权限**）→ 证明第 1 条不是「权限没生效」
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 状态迁移（授权变化前后）
- **备注**: 〔S-15〕〔AC 修正〕 —— AC 原文「路径 2 的令牌不含 write」是**假绿**：
  路径 2 本身就 **400**、**根本没有令牌** → 断言对象不存在。
  本用例改成「**先成功签发 → 再授权 → 旧令牌不变**」，这才是 D201 想说的意思。
  第 2 步的 `platform_permission_epoch` bump **必须**做，否则 RBAC 的权限缓存不失效
  （`scripts/seed-e2e.sh:211-216` 有同样的纪律）。

### TC-022-045: `scope` 的**空格分隔**与多值形态

- **关联 AC**: AC-S-140
- **前置条件**: `u-perm-write@example.com`；`client-A` 白名单含 read + write
- **测试步骤**:
  1. 请求 `scope=rbac:role:read rbac:role:write`（**空格**分隔）→ 交换 → 读 `scope`
  2. 请求 `scope=rbac:role:read,rbac:role:write`（**逗号**分隔）→ 交换 → 读响应
- **预期结果**:
  1. 步骤 1：HTTP **200**，`scope` **逐字等于** `rbac:role:read rbac:role:write`（**空格**分隔，顺序与请求一致）
  2. 步骤 2：HTTP **400**（逗号**不是**合法分隔符；按 RFC 6749 §3.3，`,` 也不是合法的 scope 字符）
- **类型**: 边界
- **优先级**: P1
- **自动化**: 可自动化
- **设计方法**: 等价类（分隔符）
- **备注**: D201 §1：「`scope` 参数的每个 token = 一个权限点码，**空格分隔**」。
  RFC 6749 §3.3 的 scope-token 字符集 `%x21 / %x23-5B / %x5D-7E` **不含**空格与 `,`。
  第 2 条钉住「不能用逗号」——实现者很容易顺手支持逗号。

### TC-022-046: 非法 scope 字符 → 400（不静默截断）

- **关联 AC**: AC-S-140
- **前置条件**: `u-perm-write@example.com`
- **测试步骤**:
  1. 请求 `scope=rbac:role:read,extra`（含非法字符 `,`）
  2. 请求 `scope=`（空）
  3. 请求 `scope=rbac:role:read%20rbac:role:write`（URL 编码的空格，正常）
- **预期结果**:
  1. 步骤 1：HTTP **400** + `{"error":"invalid_scope"}`
  2. 步骤 2：HTTP **400** + `{"error":"invalid_scope"}`（空 scope 不是「不要 scope」）
  3. 步骤 3：HTTP **200**（编码后的空格就是分隔符）
- **类型**: 边界
- **优先级**: P1
- **自动化**: 可自动化
- **设计方法**: 边界值（空 / 非法字符）
- **备注**: D201 §4 的字符集依据；「静默截断」是这类实现最常见的坑
  → 第 1 条必须**整体 400**，不是「丢掉非法部分继续」。
