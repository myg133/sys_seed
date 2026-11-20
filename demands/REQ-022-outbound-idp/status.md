# 状态：REQ-022 出站 IdP 与单点登出

- **状态**: **规格已就绪**（BA 规格阶段完成；**待 QA 挑刺**）
- **更新者**: myg133-ba-1
- **更新时间**: 2026-11-20 23:58

## 状态历史

| 时间 | 从 | 到 | 操作者 | 备注 |
|------|----|----|--------|------|
| 2026-11-19 23:58 | — | 草稿 | myg133-ba-1 | 从 REQ-013 拆出。用户裁定「拆两棒，本轮先做入站」→ 出站部分独立成 REQ-022，承接父 AC-006 |
| 2026-11-20 22:00 | 草稿 | **规格进行中** | myg133-ba-1 | **开工前实测完成**（可行性 / 依赖 / 来源核实）+ **用户裁定范围**（全做，拆小任务，可并行） |
| 2026-11-20 23:58 | 规格进行中 | **规格已就绪** | myg133-ba-1 | **规格四件套定稿**（demand rev2 / acceptance 22 条写实 / decisions D196~D220 / design-summary）+ **契约升 v3** + **REQ-016 按 D109 改写** + 技术债 +2 |

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
- ⚠️ OAuth2 的具体形态（4 种 grant / introspect / 免登 / 单点登出）**全部是 BA 的设计**，
  用户原话里没有 → **范围类决定已回用户**（见 §2）。

**关于「拆两棒」范围裁定的原始回执**：REQ-013 的 `demand.md:42` 与 `status.md:28`
记载了 2026-11-19 的用户裁定，但该次 `ask_user` 的**问卷回执在持久化的会话日志里没有找到**
（`2026/11/19` 全部 messages/snapshots 均无 `questionnaire-response`）
→ 记为「**仓内文档记载，原始回执未找到**」。需求本身（原话 #3/#9）已逐字核实，不受影响。

## 2. 范围裁定（用户，2026-11-20 22:18）

> **「全做，但是拆成小任务，如果可以并行，可以让多个子 agent 一块来做。」**

- 即 草稿 **A + B + C + D 全做**（4 种 grant + introspect + 免登 + 单点登出 + client 模型）。
- **实现策略**：拆小任务 + **并行子 Agent**（文件集不相交）。
- ⚠️ 这与草稿里「REQ-013 已有 12 条 AC，出站再加就翻倍，单棒做不扎实」的拆分理由**相反** ——
  用户已知情并选择「一轮做完、分棒并行」。**不得再自行缩小范围。**
- **规模**：**22 条 AC**（本项目最大一轮；REQ-013 是 12 条 / REQ-014 是 13 条）。

## 3. AC 编号段（D23 全局唯一）

- 原预留 **AC-S-110~121**（12 条）**不够用**。
- 全仓已用最大编号 = **133**（REQ-014 占 AC-S-122~133）。
- → **本需求用 `AC-S-134 ~ AC-S-155`**（22 条，**134 起全空**，已核实）。

## 4. 开工前可行性实测（2026-11-20 22:10，零新增依赖）

| 需要的能力 | 树里现成的 | 结论 |
|---|---|---|
| ES256 签发 | `crates/platform-iam/src/jwt.rs:130` `JwtKeySet::sign` | ✅ 需新增 claims 类型 + 泛型签发方法（D109 #1） |
| JWKS 暴露 | `crates/platform-iam/src/service.rs:544` `IamService::jwks()` | ✅ 已有 |
| PKCE S256 | `crates/platform-iam/src/sso/oidc.rs:404` `code_challenge()` | ✅ **已实现，直接复用** |
| 随机串 | `crates/platform-iam/src/sso/oidc.rs:393` `random_token()` / `crypto.rs:39` `random_base64url()` | ✅ |
| 摘要 | `crates/platform-iam/src/crypto.rs:56` `digest_base64url()` | ✅ |
| 出站 HTTP | `crates/platform-iam/src/sso/oidc.rs:88` `OidcHttpClient`（hyper + rustls） | ✅ 但语义不同 → 本轮独立实现（D220 / TD-042） |
| 会话失效 | `crates/platform-iam/src/store.rs:754` `revoke_family` / `:765` `revoke_all_refresh_tokens` | ✅ 复用 |
| 权限点查询 | `crates/platform-core/src/extension/permission.rs:239` `PermissionProvider` | ✅ 用 trait，不引 rbac |
| 鉴权入口 | `crates/platform-core/src/extension/permission.rs:273` `require_permission` | ✅ C-5 唯一入口 |
| **新增第三方依赖** | — | **0 个** |

## 5. 本轮规格阶段的**关键决策**（详见 [decisions.md](decisions.md)）

| 决策 | 一句话 |
|---|---|
| **D196** | 契约升 **v3**：C1 加 `/api/oauth` 前缀；新增 **C14**（OAuth 协议端点用 RFC 错误体，不用 Problem Details） |
| **D197** | 新 crate **`platform-oauth`**；不引 rbac（用 `PermissionProvider` trait） |
| **D198** | 只发 **JWT**（ES256），`aud = client_id`；opaque 不做 |
| **D199** | client 表归 REQ-022；REQ-016 引用并扩展 |
| **D200** | secret 由平台生成、只存 SHA-256 摘要、**只显示一次** |
| **D201** | `scope` = **权限点码本身**；授予 = 请求 ∩ client 白名单 ∩ 用户权限 |
| **D202** | 同意记录用 **`platform_oauth_grant`**（一表三用） |
| **D203** | 授权端点 = **前端路由 + 后端 preview/confirm/deny**（平台会话是 Bearer，导航不带 header） |
| **D204** | 同意票据用 **ES256 签名**（零新表、不可伪造、5 分钟） |
| **D205~D207** | 授权码只存摘要 + 交换即删 + TTL 60s；**强制 PKCE S256**；client 认证 `client_secret_basic` |
| **D208** | 内省/撤销：只有 confidential client，且**只操作自己的令牌** |
| **D209** | ⚠️ **底座业务端点默认拒绝** OAuth 令牌（本轮不做 opt-in → TD-041） |
| **D210** | 撤销语义 = grant 级；底座侧立即生效，只验签的第三方最长延迟一个 TTL |
| **D211** | 单点登出：Back-Channel / Front-Channel；**通知失败不影响登出** |
| **D212~D217** | 免登 = `prompt=none` + 已有 grant；错误码 8 处登记；权限点三处齐；C-5 命名禁区；不做 refresh token |
| **D218~D220** | 前端边界（同意页/设备码页/我的授权归 REQ-022）；4 张表启用 RLS；出站 client 独立实现 |

## 6. 跨需求改动（已执行）

| # | 改了什么 | 状态 |
|---|---|---|
| 1 | `crates/platform-iam` 加 `JwtKeySet::sign_claims<T>` + `IamService::keys()` | **待 Dev 实现**（D109 #1，已在 REQ-022 demand 的依赖段声明） |
| 2 | `BA/contracts/api-conventions/` 升 **v3**（C1 + C14） | ✅ **本轮已完成**（`CURRENT` → v3；v2 保留一个 Sprint；INDEX 已更新） |
| 3 | `BA/demands/REQ-016-app-integration/demand.md` 改写（范围重叠拆清） | ✅ **本轮已完成**（rev 2） |
| 4 | `BA/demands/REQ-016-app-integration/acceptance.md` 的 AC-S-024 降级表述 | ✅ **本轮已完成**（rev 2，按 D210） |
| 5 | `BA/plan/roadmap.md` 同步 | ✅ 已更新 |
| 6 | `BA/plan/tech-debt.md` +2（TD-041 / TD-042） | ✅ 已更新（现 **42 条**） |

## 7. 下一步（下一棒照此做）

1. **QA 子 Agent 挑刺 + 写用例**（**必须先于 Dev**）：
   - 挑验收标准的毛病（重点：22 条 AC 的可判定性、两套错误标识的边界、
     免登语义、`prompt=none` 的四种结果、D209 的射程）；
   - 产出用例集 `BA/demands/REQ-022-outbound-idp/test-cases/`。
2. **BA 逐条裁决** → `acceptance.md` rev 2 + `decisions.md` rev 2。
3. **认领 REQ-022** → 建 `feature-REQ-022` worktree → **棒 1 ∥ 棒 2 并行**（子 Agent）：
   - 棒 1（后端）：`crates/platform-oauth/**` + `migrations/**` + `platform-iam` 的 2 个新增方法
     + `platform-server` 装配 + `platform-core/titles.rs` + `i18n_titles.rs`
   - 棒 2（前端）：`apps/shell/**` + `packages/i18n/**`
   - 棒 3：E2E 夹具 + 用例（`e2e/**`、`scripts/seed-e2e.sh`）
4. **Pre-merge 复验 → 修复轮 → 独立复核 → BA 终验 → 合并 develop + 回收 worktree + 复盘**

## 8. 阻塞项

**无。** REQ-013 / REQ-014 已交付（`develop` @ `310e4d3`），前置全部就绪。

## 9. 交付物

| 文件 | 状态 |
|---|---|
| [demand.md](demand.md) | ✅ **rev 2**（范围按用户裁定全做 + D109 跨需求改动声明） |
| [acceptance.md](acceptance.md) | ✅ **rev 1**（22 条写实，AC-S-134~155） |
| [decisions.md](decisions.md) | ✅ **rev 1**（D196~D220，25 条） |
| [design-summary.md](design-summary.md) | ✅ **rev 1**（5 张表 / 端点清单 / 时序 / file:line 落点 / 错误码 / 配置项 / 风险） |
| [status.md](status.md) | ✅ 本文件 |
| `test-cases/` | ⏳ **待 QA 产出** |
| `BA/contracts/api-conventions/v3.md` | ✅ 本轮已建 |
