# 需求：出站 IdP 与单点登出

- **需求编号**: REQ-022
- **父需求**: [REQ-001 通用项目底座](../REQ-001-platform-foundation/demand.md)
- **优先级**: P1
- **状态**: 草稿（等 REQ-013 完成后细化）
- **所属迭代**: Sprint 5
- **依赖**: REQ-013（入站 SSO）、REQ-007（认证）、REQ-009（RBAC）
- **更新时间**: 2026-11-19 23:58

## 需求描述

让底座**自己当身份提供方**：给第三方应用签发令牌，使「应用接入」（REQ-016）能成立。

这是 REQ-013 拆出来的第二棒。用户 2026-11-19 裁定「拆两棒，本轮先做入站」，
出站部分由本需求承接。

## 为什么单独立需求

| 理由 | 说明 |
|---|---|
| 工作量独立成块 | REQ-013 已有 12 条 AC；出站再加就翻倍，单棒做不扎实 |
| 服务不同的消费方 | REQ-013 服务「用户怎么进底座」；REQ-022 服务「第三方应用怎么拿令牌」 |
| 依赖方向不同 | REQ-013 只依赖 IAM；REQ-022 要定义 OAuth2 client 模型，是 REQ-016 的直接前置 |

## 功能要求（草稿，待 REQ-013 完成后细化）

### A. OAuth2 授权服务器

- **授权码 + PKCE**：第三方应用引导用户授权 → 拿 code → 换 token
- **Client Credentials**：应用对应用（无用户参与）
- **Device Code**：无浏览器设备（CLI / IoT）
- **令牌内省**（`POST /oauth/introspect`，RFC 7662）
- 复用已有的 JWKS（`/.well-known/jwks.json` 已在）、ES256 签发、`kid` 轮换

### B. 免登跳转

- 应用间携带令牌直接跳转，不重复登录

### C. 单点登出

- **Back-Channel Logout**（服务端通知）
- **Front-Channel Logout**（iframe）
- 全局登出 / 按应用登出 —— 复用已交付的 `family_id` 作废逻辑

### D. OAuth2 client 模型

- client 注册（`client_id` / `client_secret` 哈希存储 / 回调白名单 / 授权类型 / 有效期）
- **`client_secret` 只存摘要**（不是加密）—— 与 refresh token 同样的处理
- 与 REQ-016 的应用注册表的关系：**待定**（可能在 REQ-016 里定义，本需求只消费）

## 技术可行性（已初步确认）

| 能力 | 可行性 |
|---|---|
| 授权码 + PKCE | ✅ 纯 JSON + 已有 JWKS |
| Client Credentials | ✅ 同上 |
| Device Code | ✅ 同上（需轮询端点） |
| introspect | ✅ 同上 |
| 单点登出 | ✅ 已有 `family_id` |

**无技术障碍**，与 REQ-013 的 SAML 情况不同（那是硬缺 XML 库）。

## 待定项（细化时要定）

1. **OAuth2 client 数据模型归属** —— 本需求建表，还是 REQ-016 的应用注册表扩展？
2. **令牌类型** —— 只发 JWT access token，还是也要 opaque token + introspect？
3. **scope 与权限点的映射** —— OAuth2 的 `scope` 如何对应到底座的 `module:resource:action` 权限点
4. **用户授权同意的记录** —— 存哪、可撤销（REQ-016 提到「授权记录可审计可撤销」）

## 不在范围内

- 入站 SSO（REQ-013 已交付）
- SAML 2.0（技术不可行，见 REQ-013 的范围收窄说明）
- 应用市场 / 工作台界面（REQ-016）

## 关联验收标准

父需求 AC：**AC-006**（底座作为 IdP 向第三方应用签发令牌）—— 由 REQ-013 移入本需求承接。

本需求自有 AC：**AC-S-110 ~ AC-S-121**（12 条，待细化）

## 完成定义（草稿）

- 第三方应用能通过授权码 + PKCE 拿到可用令牌
- 能内省令牌有效性
- 能单点登出
- `client_secret` 只以摘要形式落库，任何读接口不回显
