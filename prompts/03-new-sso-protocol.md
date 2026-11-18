# 任务：接入一种新的 SSO 协议

## 目标

在底座上接入一种新的单点登录协议（OIDC / SAML / CAS / 企业微信 …），
让平台能通过它完成登录，**且不改动平台核心**。

## 输入

- `<协议名>`：协议名称（如 `oidc` / `saml`）
- `<协议版本>`：协议版本（如 `2.0`）
- `<发现地址>`：元数据 / 发现文档地址（如 `https://idp.example/.well-known/openid-configuration`）
- `<回调路径>`：平台侧回调路径（形如 `/api/iam/sso/<协议名>/callback`）
- `<用户属性映射>`：外部属性 → 平台用户字段（如 `email → email`、
  `department → org`）

## 步骤

1. **先读** `AGENTS.md`、`capabilities/index.json` 与
   `BA/contracts/api-conventions/v1.md`，确认接口路径与错误码约定。
2. **确认边界**：平台的账号、令牌、会话能力在 `/api/iam` 分组下（**REQ-007** 落地），
   而 HTTP 层本身**本期尚未就绪**（见 `capabilities/index.json` 的
   `not_yet_provided.http_api_layer`）。因此本任务应当：
   - 新建一个**业务模块**（`cargo xtask create-module sso-<协议名>`），或
   - 在已有的 iam 模块里加一个**协议适配器**（如果 iam 模块已存在）。
   两种做法都**不得**改 `crates/platform-core/`。
3. **定义协议适配器**：在模块的 `backend/src/` 下加一个适配器类型，
   职责是「把外部协议的认证结果翻译成平台的登录态」。
   平台侧的登录态形状见 `@base/auth`（`AuthState`：`authenticated` / `userId` /
   `tenantId`）—— 用它，不要另造一个登录态模型。
4. **声明权限点**：SSO 登录 / 登出 / 绑定 / 解绑各自是独立权限点，
   在 `manifest.json` 的 `permissions` 里登记（`<模块名>:sso:<动作>`），
   并在 `backend/src/permission.rs` 里加同名常量（契约检查 C-2 会核对）。
5. **写外部请求**：如果需要调外部 IdP，用 `@base/sdk` 的 `createApiClient()`
   做 baseUrl 规范化与 URL 拼接，**不要**另造 HTTP 基类。
   本期 SDK 只做 URL 拼接、不发真实请求，真实调用随 REQ-011 落地。
6. **写契约测试**：在 `tests/contracts.rs` 的夹具里造两个租户的同名数据
   （C-3 会验证租户隔离），并让适配器的「外部身份 → 平台身份」映射可测。
7. **更新能力索引与文档**：`cargo xtask capabilities`，
   并更新 `modules/<模块名>/AGENTS.md`。
8. **自验**：
   ```bash
   cargo xtask check-modules
   cargo test -p <模块名> -- --test-threads=2
   cargo xtask capabilities --check
   ```

## 约束

- 不修改 `crates/platform-core/` 与 `crates/platform-contracts/`。
- 不自己实现密码学 / OAuth 协议细节：能用成熟库就用（但**新增依赖要先获批准**，
  本仓库当前零新增第三方依赖是硬基线）。
- 外部返回的用户标识**不要**直接当平台用户 ID：必须经过租户解析
  （租户从 `ctx` 拿，不从外部负载里直接取）。
- 错误只返回错误码 + 参数，不拼中文句子。
- 密钥 / client_secret 一律走配置项（`manifest.json` 的 `config` 段 +
  平台的配置分层），**不要**写进代码或 manifest。

## 验收

- [ ] 未修改 `crates/platform-core/` 与 `crates/platform-contracts/`
- [ ] 登录态复用 `@base/auth` 的 `AuthState`，没有第二套模型
- [ ] 权限点在 manifest 与代码常量里一一对应，C-2 无告警
- [ ] 密钥不在代码 / manifest 里，只在配置项里
- [ ] `cargo test -p <模块名> -- --test-threads=2` 全绿
- [ ] `modules/<模块名>/AGENTS.md` 已更新
