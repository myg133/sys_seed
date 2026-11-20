# 需求：出站 IdP 与单点登出

- **需求编号**: REQ-022
- **父需求**: [REQ-001 通用项目底座](../REQ-001-platform-foundation/demand.md)
- **优先级**: P1
- **状态**: 已就绪（rev 2 —— 范围已由用户裁定，AC 已写实）
- **所属迭代**: Sprint 5
- **需求来源**: **用户原话** —— 「3. sso 多协议适配」+「9. 预留应用接入能力」
  （首个会话快照 `g000000000000--ctx_43539355-*.jsonl`；与 REQ-013 同源，
  REQ-013 承接「入站」，本需求承接「出站」）
- **更新时间**: 2026-11-20 23:40

## 需求描述

让底座**自己当身份提供方（IdP）**：给第三方应用签发令牌，使「应用接入」（REQ-016）能成立。

本需求是 REQ-001「通用项目底座」的拆分子项，负责其中的一个能力域。

**业务方写业务代码时不需要自建 OAuth2 服务器** —— 平台提供完整授权服务器，
第三方应用只需要按标准协议对接；平台自己作为资源服务器的部分**默认拒绝**（见 §5）。

## 依赖

以下前置需求需先完成（依赖关系见 [roadmap.md](../../plan/roadmap.md)）：

- REQ-007（认证与账号体系）—— ✅ 已完成
- REQ-009（RBAC 权限点）—— ✅ 已完成
- REQ-013（入站 SSO）—— ✅ 已完成

### ⚠️ 跨需求改动声明（D109）

> 本需求**会改动已合并的交付物**，全部在此显式声明（不偷偷改别人的东西）：

| # | 被改的交付物 | 属于 | 改什么 | 为什么必须改 |
|---|---|---|---|---|
| 1 | `crates/platform-iam/src/jwt.rs` + `service.rs` | REQ-007 | `JwtKeySet` 新增泛型方法 `sign_claims<T: Serialize>`；`IamService` 新增 `keys()` getter | 现有 `sign()` **特化到 `AccessClaims`**（`jwt.rs:130`）→ 签不了 OAuth 令牌。**不改 `AccessClaims` 本身**，REQ-007/013 的登录令牌行为不变 |
| 2 | `BA/contracts/api-conventions/` | 契约 | 升 **v3**，C1 的前缀表增加 `/api/oauth` | C1 原文：「未分配前缀的分组在新增前须先改本契约升版本」→ 不改契约就用 `/api/oauth` = 违反契约 |
| 3 | `BA/demands/REQ-016-app-integration/demand.md` | REQ-016 | 改写「应用注册表」与「同意页」的归属 | REQ-016 草稿写的「app_id / secret、回调地址、令牌有效期」与本需求的 client 表**直接重叠**；「用户授权同意页」也重复 |
| 4 | `BA/demands/REQ-016-app-integration/acceptance.md` | REQ-016 | AC-S-024 降级表述 | 「撤销后令牌**立即**失效」在「只验签 JWKS」的第三方侧做不到（最长延迟一个 access token TTL）→ 见 [decisions.md](decisions.md) D210 |

## 功能要求

### A. OAuth2 授权服务器

- **授权码 + PKCE（S256 强制）**：第三方应用引导用户授权 → 拿 code → 换 token
- **Client Credentials**：应用对应用（无用户参与）
- **Device Code**（RFC 8628）：无浏览器设备（CLI / IoT）
- **令牌内省**（RFC 7662）与**令牌撤销**（RFC 7009）
- **授权服务器元数据**（RFC 8414）：`/.well-known/oauth-authorization-server`
- 复用已有的 JWKS（`/.well-known/jwks.json` 已在）、ES256 签发、`kid` 轮换

### B. 免登跳转

- 已有授权时**静默续发**（不重复显示同意页）；没有授权时返回 `login_required` / `consent_required`

### C. 单点登出

- **Back-Channel Logout**：服务端向 client 的 `backchannel_logout_uri` POST `logout_token`
- **Front-Channel Logout**：返回含 iframe 的 HTML
- **按应用登出**：撤销该 client 的 grant + 作废该 client 拿到的令牌
- **全局登出**：撤销该用户的全部 grant + 作废全部会话 + 通知所有已授权 client

### D. OAuth2 client 模型

- client 注册（`client_id` / `client_secret` 摘要 / 回调白名单 / 授权类型白名单 / scope 白名单 / 有效期）
- **`client_secret` 只存摘要**（SHA-256）—— 与 refresh token 同样的处理
- 平台**生成** secret（高熵），**不允许用户自选**；**只在创建时显示一次**
- 与 REQ-016 的应用注册表的关系：**本需求建协议级 client 表**，REQ-016 **引用并扩展**（见 D199）

### E. 同意与授权管理

- **最小同意页**（前端）：展示应用名 + 请求的权限点清单 + 允许 / 拒绝
- **同意记录落库**：授权可查、可撤销、可审计
- **「我的授权」页**（前端）：用户查看自己已授权的应用并撤销

## 不在范围内

- **入站 SSO**（REQ-013 已交付）
- **SAML 2.0**（本项目不交付，D124）
- **应用市场 / 工作台界面**（REQ-016）
- **refresh token**（见 D217）
- **opaque token**（见 D198）
- **底座业务端点接受第三方 OAuth 令牌**（见 D209 —— 本轮**不做**）
- **DCR（动态客户端注册，RFC 7591）** —— client 由管理员显式注册
- **JWT 私有密钥的在线轮换编排** —— 沿用 REQ-007 已有的 `kid` 双密钥宽限期机制

## 关联验收标准

父需求 AC：**AC-006**（底座作为 IdP 向第三方应用签发令牌）—— 由 REQ-013 移入本需求承接。

本需求自有 AC：**AC-S-134 ~ AC-S-155**（**22 条**）

> ⚠️ 编号段说明：草稿预留的 AC-S-110~121（12 条）不够用；
> 全仓已用最大编号 = 133（REQ-014 占 AC-S-122~133）→ 改用 **134 起**（已核实 134 起全空）。

详见 [acceptance.md](acceptance.md)。

## 完成定义

- 第三方应用能通过授权码 + PKCE 拿到可用令牌（AC-S-137 / 138 / 139）
- 能用 Client Credentials 与 Device Code 拿令牌（AC-S-141 / 142）
- 能内省与撤销令牌（AC-S-143 / 144）
- 令牌格式可被第三方验签（AC-S-145）
- 同意页可用、同意记录可查可撤销（AC-S-146 / 147）
- 免登与单点登出可用（AC-S-148 / 149 / 150）
- 底座业务端点默认拒绝 OAuth 令牌（AC-S-151）
- 错误码、权限点、审计埋点三处登记齐全（AC-S-152 / 153 / 154）
- 跨租户不可见（AC-S-155）
