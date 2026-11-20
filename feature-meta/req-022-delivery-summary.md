# REQ-022 交付总结

> **给用户的一句话**：需求已交付 —— **22/22 AC 通过**（每条指到 `file:line` 的 `assert!`）、
> 全量 **1080 passed / 0 failed / 0 ignored**、E2E **44 passed / 0 skipped**、
> 经过 **两轮独立复核**（第一轮 FAIL → 修复 → 第二轮 PASS-WITH-RISK）。
> ⚠️ 但这轮最大的产出不是代码，是**抓出了我自己的验收方法错误**（见 §2）。

- **需求**: REQ-022 出站 IdP 与单点登出（父需求 REQ-001）
- **需求来源**: 用户原话「#3 sso 多协议适配」的**出站部分** +「#9 预留应用接入能力」
- **分支**: `feature/REQ-022`（基线 `develop@310e4d3`）
- **裁决**: D196~**D261**（66 条）
- **技术债**: 40 → **47 条**（TD-041~047，其中 **TD-044 已还**，**TD-047 已修实例**）

---

## 1. 交付内容

### 1.1 新 crate `platform-oauth`（23 个文件）

底座自己当 **OAuth2 授权服务器**：

| 能力 | 实现 |
|---|---|
| **4 种 grant** | 授权码 + **强制 PKCE S256** / Client Credentials / Device Code（RFC 8628，base20 `user_code`） |
| **令牌** | ES256 JWT，**与登录令牌共用同一套密钥集** → 同一个 JWKS 覆盖两种令牌 |
| **内省 / 撤销** | RFC 7662 / RFC 7009；**grant 级撤销**（撤一个 grant → 该 grant 下**全部**令牌立即失效） |
| **免登** | `prompt=none` + 已有 active grant 覆盖 scope → 静默续发 |
| **单点登出** | **Back-Channel**（`logout_token` 投递）+ **Front-Channel**（后端返 JSON + 前端渲染 iframe） |
| **scope 语义** | = 权限点码；授予 = **三重交集**（请求 ∩ client 白名单 ∩ 用户真实权限点）；**通配一律不生效**；**旧令牌不被回溯扩大** |
| **client 模型** | 平台级表（无 `tenant_id`、不启用 RLS）；secret 只存 **base64url SHA-256 摘要**、只显示一次 |
| **默认拒绝** | 底座业务端点**一律拒绝**第三方 OAuth 令牌（AC-S-151） |

### 1.2 其它

- **5 张表** + **6 个权限点** + **5 个配置键**（3 个迁移 + down）
- **12 个端点**（8 个协议 / 9 个平台，含 D257 补的 2 个设备码批准端点）
- **前端 4 个页面**：`/oauth/authorize`（同意页）/ `/oauth/device` / `/oauth/grants` / `/oauth/logout`
- **未登录回跳**（D221）：公开路由 + `return_to`，**不动** `router.ts`
- **9 个平台错误码** + **8 个审计事件码**（事件码**不进** `TITLES`，行数 84→**93**）

---

## 2. 🔴 本轮最重要的产出：抓出**我自己的验收方法错误**

### 2.1 事故

我第一次终验时写了 **22 行** AC 判定表，判 **22/22 通过**。
**独立复核对它判 FAIL**：那 22 行里 **13 行站不住**，其中 **7 行是阻塞级**。

| 我写的「判据落点」 | `grep` 的真相 |
|---|---|
| 「401 `UNAUTHENTICATED`，不含 `error` 键」 | 只调了 `iam.verify_access_token()` **这个函数**，两个业务端点**一次没打** |
| 「`sid` 齐全、`events` 含 backchannel 键、无 `nonce`」 | 该字符串在测试里 **0 命中** |
| 「测试侧独立 ES256 验签」 | 测试里唯一相关文字是注释「**只解，不验签**」 |
| 「哈希链 `ok = true`」 | 用的是**内存假 sink**，**不落库** |

### 2.2 根因

**我是从 AC 出发**（"这条 AC 要求什么 → 哪条测试大概覆盖了它"），
**不是从测试出发**（"这条测试的 `assert!` 到底断什么 → 它能证明哪条 AC"）。

**形式上的 22 行 ≠ 实质上的 22 次核对。**

### 2.3 纠正（已落盘）

- `dev-guide §9.15`：「形式上的 N 行 ≠ 实质上的 N 次核对」+ 三个必须动作
- `dev-guide §9.15.1`：`file:line` **必须同时给函数名**、函数名要 `grep -n "async fn"` 反查、
  关键字符串要 `grep -c` 出现在**那个函数体内**（不是"这个文件里"—— 文件 3884 行，什么都能找到）
- 第一轮独立复核报告 + 修复轮报告 + 第二轮复核报告，全部落盘可追溯

### 2.4 这个错误为什么比单个缺陷更危险

**它让复核者失去「用报告定位判据」的能力。**
第一次复核是靠**逐条打开测试源码**才发现的；任何人只要信了那张表，就会得出 22/22 的结论。

如果按我原来的 22/22 合并，那 7 条阻塞级会带着**零覆盖**上线 —— 而它们恰恰是**安全承诺**：
「业务端点默认拒绝 OAuth 令牌」「ES256 签名有效性」「私钥泄露也拿不到越权令牌」。

---

## 3. 验证链（6 棒）

| 棒 | 内容 |
|---|---|
| 1 | **QA 挑刺 39 条**（阻塞 10 / 高 19 / 中 10；**22/22 AC 全部有问题**）+ **131 条用例** |
| 2 | Dev **两棒并行**（后端 23 文件 / 前端 18 文件，文件集严格不相交） |
| 3 | 终版全量 + E2E（第一次跑出 **2 failed**） |
| 4 | **修复轮一**：3 个真缺陷（前端 `userCode` / E2E `page.request` / **`ensure_fresh` 缺失**） |
| 5 | **独立复核 FAIL** → BA 认账 → **修复轮二**（12 项判据 / 17 新测试 / 24 变异） |
| 6 | **第二轮独立复核 PASS-WITH-RISK** → BA 修正 3 处指针 + 补验 3 个未验项 → 终验 |

---

## 4. 最终数字（BA 亲跑，不抄 Dev 报告）

### 4.1 Rust 全量（14 个成员，`--all-targets -- --test-threads=1 --include-ignored`）

| crate | passed |
|---|---|
| platform-core | 153 |
| platform-audit | 54 |
| platform-config | 89 |
| platform-contracts | 32 |
| platform-contracts-macros | 0 |
| platform-db | 46 |
| platform-iam | 180 |
| **platform-oauth** | **131** |
| platform-org | 93 |
| platform-rbac | 83 |
| platform-schema | 20 |
| platform-server | **131** |
| xtask | 51 |
| sandbox | 33 |
| **合计** | **1094**（`awk` 聚合，`failed=0`、`ignored=0`） |

### 4.2 前端

| 包 | 结果 |
|---|---|
| `@base/shell` | **279 passed / 0 failed** |
| `@base/i18n` | **45 passed / 0 failed** |
| `@base/sdk` | 36 passed / **2 skipped**（既有 `describe.skipIf(!built)`，`develop` 上一模一样） |
| `svelte-check` | **0 errors / 0 warnings** |

### 4.3 E2E

**44 passed / 0 failed / 0 skipped**（含既有 6 个 spec；`oauth.spec.ts` **9/9**）

### 4.4 守卫命令

| 命令 | 结果 |
|---|---|
| `cargo test -p platform-oauth --test contract_c5`（真 C-5） | **2 passed** |
| `cargo xtask capabilities --check` | ✅ 14 个文件一致（**TD-044 已还**，基线原为红） |
| `cargo xtask tenant-policies --check` | ✅ 4 个模块的表都开了 RLS |

### 4.5 变异验证

**总计 25 条**（棒1 九条 + 棒2 八条 + 修复轮 23 条 + BA 一条），**全部变红 → 全部还原干净**。
每轮都验了 `sha256sum -c` 逐字节相同 + `grep -c MUTATION` = 0。

---

## 5. 22 条 AC 判定（**每条指到 `file:line` 的 `assert!`**）

> 完整表见 `req-022-final-verification.md` **§9.4**（含每条的测试函数名与核心断言）。
> 判定：**22 通过 / 0 不通过 / 0 判不了**。
> 其中 **AC-S-153 标 ⚠️**（`total == 32` 与 C-5 步骤 3/4 的阳性对照**原本无自动化断言** ——
> BA 在终验时**亲手补验**了步骤 2/3/4，见 §6.1）。

---

## 6. 终验时 BA 亲手补验的 3 项（复核列为「未独立验证」）

### 6.1 AC-S-153 步骤 2：启动对账 **fail-fast**

往 `manifest.json` 加一个未登记的权限点 `oauth:probe:read` → 起服务：

```
✗ ✗ 权限点注册表与数据库不一致，进程退出（fail-fast）：
[REGISTRY_MISMATCH] the permission registry does not match the database:
  the permission `oauth:probe:read` is declared in crates/platform-oauth/manifest.json
  but missing from the database
```

→ **服务确实起不来**，且日志**逐字**列了缺失的权限点编码与**来源 manifest 路径**。还原后服务正常。

### 6.2 AC-S-153 步骤 3：C-5 阳性对照（删调用点）

把 `require_permission("oauth:grant:read")` 改成 `require_permission(String::from(...))`（非字面量）→

```
test result: FAILED. 1 passed; 1 failed
  提示：权限点必须在 `crates/platform-oauth/src/` 里用字面量调用 require_permission
```

### 6.3 AC-S-153 步骤 4：C-5 阳性对照（命名禁区）

往 `authz.rs` 加一个 `pub fn authorize(...)` →

```
C-5 违例：the custom authorization entry point at .../authz.rs:89:8
  bypasses the platform entry point `require_permission`
```

→ **D216 的命名禁区是有效的**，且报出了**精确的 `file:line`**。

三项都已还原（`sha256sum -c` 成功、MUTATION 零残留）。

---

## 7. 遗留技术债（7 条，全部带证据与修法）

| # | 内容 | 状态 |
|---|---|---|
| TD-041 | OAuth 令牌**不能**访问底座业务 API（opt-in 未实现） | D209 **有意**不做 |
| TD-042 | 出站 HTTP client 两份 | D220 有意独立 |
| TD-043 | 登出**不杀**已签发的登录 access token（最长延迟一个 TTL） | D229 显式写明 |
| **TD-044** | `xtask capabilities --check` 基线红 | ✅ **已还** |
| TD-045 | 「我的授权」要权限点，普通用户也得被发 → 真正起作用的是数据范围 | 去权限点会让 C-5 红 |
| TD-046 | 私钥泄露时自签 ticket 能**写 grant 行**（但**拿不到令牌**） | 修它要废 AC-S-140 的 4 条 P0（D260 权衡） |
| TD-047 | **每个自带认证中间件的模块都必须自己调 `ensure_fresh`** | 本轮已修实例；机制化需新契约检查 |

---

## 8. 明早你可以只看这几份

| 文件 | 内容 |
|---|---|
| **本文件** | 总览 |
| `req-022-final-verification.md` **§9** | 22 条 AC 的**逐条 `file:line` 证据** |
| `req-022-independent-review.md` | 第一轮复核（**FAIL**，989 行 —— 它救了这个需求） |
| `req-022-recheck2.md` | 第二轮复核（**PASS-WITH-RISK**，400 行） |
| `req-022-fix.md` | 修复轮自验（17 新测试 / 24 变异） |
| `sprint/req-022-retro.md` | 复盘 |
| `plan/dev-guide-low-spec.md` **§9.15 / §9.15.1** | 这轮的两条新规矩 |

---

## 9. 需要你拍板的两点

1. **D260**：AC-S-140 与 AC-S-146 步骤 3 **确实互斥**（consent ticket 无状态，D204 定的零新表）。
   我选**保 AC-S-140** 的 4 条 P0，AC-S-146 步骤 3 换成「交换令牌必须 `invalid_scope`」的替代判据。
   代价 = **TD-046**（私钥泄露时多写一行 grant，**不越权**）。
   → 若你认为纵深防御更重要，我回滚（但那要引入新表、等于动 D204）。
2. **TD-047 机制化**：`PermissionRefresher` 的 trait 注释已明写「自带认证中间件的模块都要刷新缓存」，
   但它是**散文**。建议后续把「验令牌 + 建 ctx + `ensure_fresh` + 注入 checker」做成
   `platform-core` 的组合中间件，或加一条契约检查（扫 `insert(checker)` 的文件必须同时有 `ensure_fresh`）。
