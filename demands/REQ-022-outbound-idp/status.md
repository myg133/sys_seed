# 状态：REQ-022 出站 IdP 与单点登出

- **状态**: **规格进行中**（可行性已实测；范围已由用户裁定；AC 尚未写实）
- **更新者**: myg133-ba-1
- **更新时间**: 2026-11-20 22:30

## 状态历史

| 时间 | 从 | 到 | 操作者 | 备注 |
|------|----|----|--------|------|
| 2026-11-19 23:58 | — | 草稿 | myg133-ba-1 | 从 REQ-013 拆出。用户裁定「拆两棒，本轮先做入站」→ 出站部分独立成 REQ-022，承接父 AC-006 |
| 2026-11-20 22:00 | 草稿 | **规格进行中** | myg133-ba-1 | **开工前实测完成**（可行性 / 依赖 / 来源核实）+ **用户裁定范围**（全做，拆小任务，可并行） |

---

## 1. 需求来源核实（D122）

**REQ-022 的需求来源是「用户原话」，不是 BA 自补。** 已从会话快照**逐字核实**：

> 3. sso 多协议适配
> 9. 预留应用接入能力
>
> —— 用户 2026-11-17 的 9 项需求清单（首个会话快照 `g000000000000--ctx_43539355-*.jsonl`）

- **原话 #3「sso 多协议适配」** → 拆成两棒：**入站**（REQ-013，已交付）+
  **出站**（本需求，底座自己当 IdP）。
- **原话 #9「预留应用接入能力」** → REQ-016「应用接入与应用注册表」（依赖本需求）。
- ⚠️ **OAuth2 的具体形态（授权码 / PKCE / Client Credentials / Device Code / introspect /
  免登 / 单点登出）全部是 BA 的设计**，用户原话里没有这些细节 →
  **范围类决定必须回用户**（见 §3，用户已裁定）。

**关于「拆两棒」范围裁定的原始回执**：REQ-013 的
`demand.md:42` 与 `status.md:28` 记载了 2026-11-19 的用户裁定
（「本轮不做 SAML，只做 OIDC + LDAP」「拆两棒，本轮先做入站」），
但该次 `ask_user` 的**问卷回执在持久化的会话日志里没有找到**
（`2026/11/19` 全部 `messages.jsonl` 与 `snapshots/*.jsonl` 均无
`questionnaire-response` 记录）→ 记为「**仓内文档记载，原始回执未找到**」。
需求本身（原话 #3/#9）已逐字核实，不受此影响。

## 2. 开工前可行性实测（2026-11-20 22:10，零新增依赖）

> ⚠️ 草稿里「无技术障碍」是 2026-11-19 写的，**在 REQ-013 落地之前**。
> 下表是**在 `develop` @ `310e4d3` 上的实测**，每条都有 `file:line`。

| 需要的能力 | 树里现成的 | 结论 |
|---|---|---|
| ES256 签发 | `crates/platform-iam/src/jwt.rs:130` `JwtKeys::sign` | ✅ 可复用（**需新增 claims 类型**，不改 `AccessClaims` → 不破坏 REQ-007/013 的登录令牌） |
| JWKS 暴露 | `crates/platform-iam/src/service.rs:544` `IamService::jwks()`；`/.well-known/jwks.json` 已在 | ✅ 已有 |
| PKCE S256 | `sha2` 在 `Cargo.lock` | ✅ |
| `client_secret` 摘要 | `crates/platform-iam/src/crypto.rs:59` `digest_base64url`（SHA-256） | ✅ 高熵口径吻合（见 §4.2） |
| 授权码 / device code 随机值 | `crates/platform-iam/src/crypto.rs:44` `random_base64url` | ✅ |
| 会话失效（登出） | `crates/platform-iam/src/store.rs:151` `family_id` | ✅ 可复用 |
| 权限点注册表（scope 映射的底座） | `crates/platform-core/src/extension/permission.rs:273` `require_permission` + 各 crate `manifest.json` + `platform_permission` 表 | ✅ 已有（D27 三处齐） |
| Back-Channel Logout 的出站 HTTP | `crates/platform-iam/src/sso/oidc.rs:73-74` `hyper_rustls` + `hyper_util`（REQ-013 引入） | ✅ 可复用（**需抽一个通用出站 client**；OIDC 那个带 JWKS 缓存，不能直接当通用 client 用） |
| **新增第三方依赖** | — | **0 个** |

依赖核查（`Cargo.lock`）：`argon2` `sha2` `base64` `jsonwebtoken` `time` `uuid` `serde_json`
`hyper` `rustls` **全部已在**。

## 3. 范围裁定（用户，2026-11-20 22:18）

> **「全做，但是拆成小任务，如果可以并行，可以让多个子 agent 一块来做。」**

- 即 **草稿 A + B + C + D 全做**（4 种 grant + introspect + 免登 + 单点登出 + client 模型）。
- **实现策略**：拆小任务 + **并行子 Agent**（文件集不相交）。
- ⚠️ 这与草稿里「REQ-013 已有 12 条 AC，出站再加就翻倍，单棒做不扎实」的拆分理由**相反** ——
  用户已知情并选择「一轮做完、分棒并行」。**不得再自行缩小范围。**
- **规模**：约 **22 条 AC**（本项目最大一轮；REQ-013 是 12 条 / REQ-014 是 13 条）。

### 3.1 AC 编号段（D23 全局唯一）

- REQ-022 原预留 **AC-S-110~121**（12 条）**不够用**。
- 全仓已用最大编号 = **133**（REQ-014 占 AC-S-122~133）。
- → **本需求改用 `AC-S-134 ~ AC-S-155`**（22 条，**134 起全空**，已核实）。

### 3.2 实现分棒（并行，文件集不相交）

| 棒 | 范围 | 文件集 | 并行性 |
|---|---|---|---|
| **棒 1** | 后端核心：`platform-oauth` crate（表 + 迁移 + client 模型 + 4 种 grant + introspect + revoke + 元数据 + 登出 + 审计埋点） | `crates/platform-oauth/**`、`migrations/**`、`crates/platform-core/**` | 与棒 2 **并行** |
| **棒 2** | 前端：同意页 + 设备码输入页 + 授权管理 | `apps/shell/**`、`packages/i18n/**` | 与棒 1 **并行**（规格驱动） |
| **棒 3** | E2E 夹具 + 用例 | `e2e/**`、`scripts/seed-e2e.sh` | 棒 1、2 之后 |
| **棒 4** | 独立复核 / 修复 | — | 最后 |

## 4. 四个待定项的**初裁**（BA 拍板，待规格阶段写成决策）

### 4.1 OAuth2 client 数据模型归属 → **REQ-022 建表**，REQ-016 引用

- REQ-022 建 **`platform_oauth_client`**（协议级：`client_id` / `client_secret` 摘要 /
  回调白名单 / 允许的 grant / 允许的 scope / 有效期）。
- REQ-016 的「应用注册表」是**业务级**对象（类型 / 图标 / 描述 / 分类）→
  **引用** REQ-022 的 client 表并扩展，**不再自带一份 `secret` / 回调 / 有效期**。
- ⚠️ **REQ-016 的草稿现在写的是「应用注册表：app_id / secret、类型、回调地址、令牌有效期」——
  与本表直接重叠** → 必须按 **D109**（跨需求改动显式声明）改写 REQ-016 的 demand.md。
- 依赖方向：**REQ-016 → REQ-022**（单向，符合 ADR-0001）。

### 4.2 令牌类型 → **只发 JWT access token**（opaque 不做）

- 复用 `JwtKeys::sign` + 已有 JWKS（第三方应用可验签）→ **零新增依赖**、零新增状态。
- `introspect`（RFC 7662）**仍然要做**，但它校验的是 JWT 令牌（验签 + `exp` + 查 grant 状态）。
- ⚠️ **撤销语义必须写清**：撤销粒度 = **grant 级**（撤销某 client 对某 user 的全部授权）→
  - **底座自己的 API**：每次校验都查 grant 状态 → **立即生效**；
  - **只验签 JWKS 的第三方调用方**：**最长延迟一个 access token TTL**。
  → 这条必须同时写进 AC、设计文档与**技术债**，并**提前告知 REQ-016**：
  REQ-016 的 **AC-S-024「用户撤销后该应用的令牌立即失效」** 在「只验签」场景下需降级表述。
- `client_secret` 由**平台随机生成**（高熵）→ 按 `crypto.rs` 的既有三档口径走 **SHA-256 摘要**
  （与 refresh token 同档）；**不允许用户自选 secret**（自选 = 低熵 → 需 Argon2id）→ 钉死。

### 4.3 scope ↔ 权限点映射 → **scope 就是权限点码本身**

- `scope` = 空格分隔的**权限点码**（如 `scope=rbac:role:write`），
  取值必须 ∈ `client.allowed_scopes` ∩ **用户自身的有效权限**。
- **零映射表**；授权语义与底座 RBAC **完全同构** → 同意页展示的就是实际权限。
- RFC 6749 §3.3 的 `scope-token` 语法（`1*( %x21 / %x23-5B / %x5D-7E )`）
  **允许** `:`（0x3A ∈ 0x23-0x5B）→ 合规。
- ⚠️ **通配不得隐式生效**：`rbac:role:*` 必须是**注册过的权限点**，否则 400（不静默放大授权）。
- ⚠️ **越权防线**：申请到的 scope **永远不得超过用户本人**的权限（否则应用拿到比用户更大的权）。
- **令牌生效范围默认拒绝**：OAuth 令牌**只能**访问**显式声明接受 OAuth 令牌**的端点
  （白名单），**不得**让所有端点都接受第三方令牌 → fail closed。

### 4.4 用户授权同意记录 → 新表 **`platform_oauth_grant`**

- 一张表同时承担：① 同意记录 ② 撤销依据 ③ 登出定位（哪些 client 拿过该用户的令牌）。
- 字段：`tenant_id` / `client_id` / `user_id` / `scopes` / `status` / `revoked_at`；
  **UNIQUE (tenant_id, client_id, user_id)** → 再次授权 = 更新 scope（幂等）。
- 「可审计」→ 由 REQ-014 的审计 sink 满足（本需求埋点）。

### 4.5 同意页归属 → **REQ-022 含一个最小同意页**

- 理由：路线图要求每个子需求「**可独立构建、独立测试、独立认领**」→
  没有同意页，授权码流程用户无法完成 → **REQ-022 不可独立交付**。
- **REQ-016 的「用户授权同意页」改为「应用级权限声明 + 授权管理界面」**
  （管理已注册应用、查看 / 撤销授权）；**同意动作本身在 REQ-022**。
- ⚠️ 这是对 REQ-016 范围的改动 → 按 **D109** 显式声明 + 改写 REQ-016 的 demand.md。

## 5. 其他范围决定（初裁）

| 项 | 决定 | 理由 |
|---|---|---|
| **refresh token** | **本轮不做** | 草稿 A 组未列；引入它就要做 rotation / reuse detection 一整套；scope 收窄后重新授权成本低 |
| **授权码存储** | 只存 **SHA-256 摘要**（不存明文），**交换后即删**（删除即消费 → 天然一次性） | 与 `crypto.rs` 的高熵口径一致 |
| **device code** | `device_code` 与 `user_code` 都存摘要；`user_code` 走**独立短码**（人可输入） | — |
| **端点前缀** | `/api/oauth/**`（平台统一），另有 `/.well-known/oauth-authorization-server`（RFC 8414）供客户端自动发现 | 路径可自定义（客户端从元数据读）→ 不违反 OAuth 惯例 |
| **免登跳转** | 实现为「授权码 + **已有 grant** + 静默续发（不重复显示同意页）」 | 真正的应用间跳转归 REQ-016 的工作台导航 |
| **单点登出** | Back-Channel（向 client 的 `backchannel_logout_uri` POST `logout_token`）+ Front-Channel（返回含 iframe 的 HTML）+ 按应用登出（撤销 grant） | 依赖 `platform_oauth_grant` + `family_id` |
| **错误码** | 新增错误码必须同步 **8 处**（D125），且 `title` 必须 ASCII（D113） | REQ-013 的教训 |

## 6. 下一步（下一棒照此做）

1. **BA 写规格**（BA 本人不派子 Agent，因为这是「拍板」）：
   - `demand.md` rev 2（范围按用户裁定全做 + **D109 跨需求改动声明** + 不在范围内）
   - `acceptance.md` 写实，**AC-S-134 ~ AC-S-155**（22 条，全部可机械判定）
   - `decisions.md`（把 §4 / §5 的初裁写成正式决策 + 实现钉死）
   - `design-summary.md`（数据模型 6 张表 + 端点清单 + 时序 + **代码落点 file:line** + 错误码表 + 配置项）
2. **QA 挑刺 + 写用例**（子 Agent，**必须先于 Dev**）：挑刺 + 用例集
3. **BA 逐条裁决** → AC 写实定稿
4. **认领 REQ-022** → 建 `feature-REQ-022` worktree → **棒 1 ∥ 棒 2 并行**（子 Agent）
5. **Pre-merge 复验 → 修复轮 → 独立复核 → BA 终验 → 合并 develop + 回收 worktree + 复盘**

## 7. 阻塞项

**无。** REQ-013 已交付（`develop` @ `310e4d3`），REQ-022 的前置全部就绪。

## 8. 交付物

| 文件 | 状态 |
|---|---|
| [demand.md](demand.md) | 草稿 → **待写 rev 2**（范围已裁定） |
| [acceptance.md](acceptance.md) | 骨架 → **待写实**（AC-S-134~155，编号段已改） |
| decisions.md | **待建** |
| design-summary.md | **待建** |
| [status.md](status.md) | 本文件（可行性 + 来源 + 范围裁定的落盘） |
