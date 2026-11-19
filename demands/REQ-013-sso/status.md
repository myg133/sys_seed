# 状态：REQ-013 SSO 入站（OIDC + LDAP）与强制 SSO

- **状态**: **已就绪**（rev 2，等待 QA 写用例与挑刺）
- **更新者**: myg133-ba-1
- **更新时间**: 2026-11-19 23:55

## 状态历史

| 时间 | 从 | 到 | 操作者 | 备注 |
|------|----|----|--------|------|
| 2026-11-17 23:45 | — | 草稿 | myg133-ba-1 | 从 REQ-001 拆分创建，只有 2 条占位 AC（「按需求描述构造前置状态」这类空话） |
| 2026-11-19 22:47 | 草稿 | 已就绪 | myg133-ba-1 | 用户选定下一棒做 SSO |
| 2026-11-19 23:55 | 已就绪 | 已就绪 | myg133-ba-1 | **rev 2：范围收窄** —— 用户裁定「本轮不做 SAML，只做 OIDC + LDAP」「拆两棒，本轮先做入站」。demand / acceptance / design 三份全部重写；12 条 AC 写实；出站 IdP 拆到 REQ-022 |

## 本轮范围（用户 2026-11-19 裁定）

| 做 | 不做 |
|---|---|
| OIDC / OAuth2 入站（授权码 + PKCE） | **SAML 2.0**（树里无 XML 库；XMLDSig 手写验签是 XSW 攻击重灾区） |
| LDAP / AD 入站（bind 认证） | **CAS**（2.0 断言是 XML） |
| 强制 SSO 策略 | **出站 IdP / 免登跳转 / 单点登出** → REQ-022 |
| 账号绑定 + JIT 建号 | SSO 登录页品牌定制（REQ-010 已供配置） |
| SSO 会话（与本地同构） | 审计**落库**（REQ-014 才做 sink） |
| SSO 审计事件**声明 + 发出** | 多 IdP 的**管理界面** |

## 为什么砍 SAML（技术事实，不是取舍偏好）

树里 `grep` 实测：**没有任何 XML 解析库**（`quick-xml` / `roxmltree` / `xmltree` / `samael` 全 0 命中）。

SAML 的断言校验需要：
1. XML 解析
2. **XMLDSig 签名验证** —— 含 **C14N 规范化**（同一文档可有多种等价序列化，签名只对其中一种有效）
3. **签名引用解析** —— 必须确保「验签的那个元素」就是「业务读取的那个元素」

第 3 点是**签名包装攻击（XSW）** 的根源。历史上 SAML 集成的严重漏洞几乎全在这里。
**手写这套的正确概率远低于不写的风险** —— 所以不做。

OIDC 已覆盖 Azure AD / Okta / Keycloak / Auth0 / Google Workspace；
LDAP 覆盖 AD / OpenLDAP。**这两条覆盖绝大多数企业身份源**。

## 写规格时实测确认的前置（不靠推断）

| # | 结论 | 依据 |
|---|---|---|
| 1 | OIDC **可行** | `hyper` + `hyper-util`（`platform-org/src/scim.rs` 的 `HttpScimGateway` 已在用）、`jsonwebtoken`（rust_crypto 后端）、`serde_json` 全在树里 |
| 2 | LDAP 密码校验**可行** | `ldap3` 0.11.5 已在树（REQ-008 引入）；`Ldap::simple_bind(bind_dn, bind_pw)` 源码 `ldap.rs:238` 确认存在 |
| 3 | LDAP 现有实现**只做 search 同步** | `platform-org/src/ldap.rs` 的 `Ldap3Source` 只有 `.search(...)`，**没有 bind 认证** → 本需求要新建 |
| 4 | 账号绑定表**已有** | `external_identity(tenant_id, provider, external_id, user_id)` + 唯一键 + `upsert_external_identity`（`platform-org/src/store.rs:635`） |
| 5 | 静态加密**已有** | `platform-iam/src/crypto.rs` 的 `encrypt` / `decrypt`（ChaCha20-Poly1305，密钥来自 `BASE_IAM_SECRET_KEY`） |
| 6 | **`client_secret` 不能进 config** | `platform_config.value` 是**明文 JSONB**，且 `GET /api/config/entries` 可读 → 必须走环境变量 |
| 7 | **审计 sink 不存在** | `platform-org/src/audit.rs` 明写「审计的落库与查询在 REQ-014…本期没有平台 sink 可调」，`platform-iam` 里 `grep audit` **0 命中** → AC-S-108 **只声明+发出，不验落库** |
| 8 | 登录限流**已有** | `platform_iam_login_rate` / `platform_iam_login_attempt` 表，LDAP 失败复用同一套 |
| 9 | 新增错误码会被卡住 | REQ-021 的 `i18n_titles.rs::titles_cover_every_code` 机械校验 `TITLES ⊇ ALL_CODES` → 18 个新码必须同时进 `TITLES` |

**第 7 条是本轮最关键的「主体不存在」排查**：如果 AC 写成「SSO 登录产生审计记录」，
验收时会发现**根本没有审计表** —— 那是 REQ-021 踩过的坑（AC 的前置物理不可达）。

## 最大风险（已写进 design §4.2，开工前必答）

**`hyper-util` 的 `build_http()` 不带 TLS**，而树里没有 TLS 客户端实现。
真实 IdP 都是 `https://`。

- 选 (A) 加 TLS 依赖 → **违反零新增依赖预算**（用户多轮明确要求）
- 选 (B) 本轮只支持 `http://` IdP + `SSO_OIDC_TLS_UNSUPPORTED` → 链路与安全约束可验，但真实云 IdP 接不了

**BA 倾向 (B) + 登记技术债**，因为本轮目标是把「链路通 + 6 条安全约束到位」做扎实。
Dev 若不同意，**必须在开工前提出，不得自行加依赖**。

## 阻塞项

无。依赖 REQ-007 / REQ-009 / REQ-010 均已合并 `develop`。

**不依赖** REQ-014（审计 sink，本轮只走接缝）与 REQ-022（出站 IdP）。

## 交付物

| 文件 | 内容 |
|---|---|
| [demand.md](demand.md) | 范围收窄说明 + 6 条安全约束 + 跨需求改动声明（D109） |
| [acceptance.md](acceptance.md) | **12 条 AC**（AC-S-020/021 重写 + AC-S-100~109 新增），逐条给错误码 |
| [design-summary.md](design-summary.md) | 现状盘点、数据模型、端点、时序、18 个错误码、12 个配置项、代码落点、开工前必答 |
