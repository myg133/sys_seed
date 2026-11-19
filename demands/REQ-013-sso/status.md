# 状态：REQ-013 SSO 入站（OIDC + LDAP）与强制 SSO

- **状态**: **已就绪**（**rev 3** —— QA 挑刺 51 条全部裁定完毕，可派 Dev）
- **需求来源**: ✅ **用户原话第 3 项** —— 用户 2026-11-17 原始需求列举：
  「3. sso 多协议适配」。（D122 要求每轮披露来源）
  > 与 REQ-017（工作流）不同：**这条是用户自己提的，不是 BA 补的**。
  > 出站 IdP 部分对应用户原话第 9 项「预留应用接入能力」→ 拆到 REQ-022。
- **更新者**: myg133-ba-1
- **更新时间**: 2026-11-19 23:55

## 状态历史

| 时间 | 从 | 到 | 操作者 | 备注 |
|------|----|----|--------|------|
| 2026-11-17 23:45 | — | 草稿 | myg133-ba-1 | 从 REQ-001 拆分创建，只有 2 条占位 AC（「按需求描述构造前置状态」这类空话） |
| 2026-11-19 22:47 | 草稿 | 已就绪 | myg133-ba-1 | 用户选定下一棒做 SSO |
| 2026-11-19 23:49 | 已就绪 | 已就绪 | myg133-ba-1 | **D123**：用户批准加 TLS 依赖支持 https |
| 2026-11-20 00:05 | 已就绪 | 已就绪 | QA | **QA 交付**：129 条用例（12/12 AC 覆盖）+ **51 条挑刺**（阻塞 14 / 高 13 / 中 16 / 低 8） |
| 2026-11-20 00:35 | 已就绪 | 已就绪 | myg133-ba-1 | **rev 3：51 条全部接受**（D124 + D125），重写 acceptance 与 design 的关键章节 |
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

## rev 3：QA 挑刺的结果（51 条，全部接受）

**12 条 AC 没有一条能原样实现。** QA 逐 AC、逐 design 章节核对，每条带 `file:line` 证据。
**我抽验了最关键的 3 条，全部成立**：

| # | 问题 | 我的规格错在哪 | 后果 |
|---|---|---|---|
| 1 | `ldap3::Ldap::simple_bind` 返回 `Ok(LdapResult)`，**密码错误时外层仍是 `Ok`**，必须再 `.success()` | design 写「可直接校验密码」 | 照字面实现 → **任意密码都能登录** |
| 2 | `GET /api/config/entries` **必须带 `keys`**，不给返回空数组；`?scope=tenant` 参数**根本不存在** | AC-S-106 三步全建在错误假设上 | **三重假绿**：0 命中 + 什么也没读到 |
| 3 | 既有 `upsert_external_identity` 是 `ON CONFLICT … DO UPDATE SET user_id`（**改指**） | 我让 iam「照抄」org 的实现 | 绑定可被**静默改到另一个用户** = 账号接管 |

**根本教训与 REQ-021 同源**：我写规格时**读的是自己上一版的设计，没去源码验返回值 / 参数 / 语义**。
「我记得这个 API 是这样」不是证据。

**rev 3 的 4 项决策**（D124）：

| # | 决策 | 为什么 |
|---|---|---|
| 1 | **回调改为 ticket 交接**：`callback` → 302 到前端 `/sso/callback?ticket=…`，前端 `POST /sso/exchange` 换 `LoginResponse` | 原设计「回调返回 JSON」在真实浏览器**物理不可用**（顶层导航渲染成文本，SPA 拿不到 refreshToken），而 AC 用脚本客户端测**恰好掩盖**了它。不选 cookie 是因为会破坏 D99（会话存 `sessionStorage`） |
| 2 | **https 测试用 `with_custom_root(测试根)` + dev-only 服务端 TLS** | `with_webpki_roots()` 不信任自签根 → AC 的 https 断言**必然失败且伪装成网络故障** |
| 3 | **空密码改 `400`** | 请求不合法 ≠ 凭据错误；`401` 会触发客户端「凭据失效跳登录页」 |
| 4 | **拆 `TOKEN_ENDPOINT_UNAVAILABLE`(502)** 与 `TOKEN_EXCHANGE_FAILED`(400) | 合成一个 400 会让前端提示「登录已失效」，运维分不清是我们还是 IdP 的问题 |

**rev 3 新增 3 个必须新建的接缝**（rev 2 漏了，AC 会因此不可验或运行期炸）：

| 接缝 | 漏了会怎样 |
|---|---|
| `SsoLdapClient` trait（可计数） | 「空密码 0 条 bind」**没有观测面**，只能手工看日志 |
| `SsoAuditEvent` + `SsoAuditRecorder`（iam 自建） | 照搬 org 的 recorder 会**成环**，且 `AuditEventSpec` **没有 payload** |
| `SessionView.authProvider` + **两条 SELECT** | 加 DB 列但没改 SELECT → `tenant_row!` 按列名 `try_get` → `GET /sessions` **运行期 500**（不是编译错） |

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
| 9 | 新增错误码会被卡住 | REQ-021 的完备性测试机械校验 `TITLES ⊇ ALL_CODES`。**但「改两处」是错的**（D125）：实际要改 **8 处**。本轮 **21 个**新码 → `TITLES.len()` 期望 **79**（基线 58 = HTTP 码 52 + 事件码 6） |

**第 7 条是本轮最关键的「主体不存在」排查**：如果 AC 写成「SSO 登录产生审计记录」，
验收时会发现**根本没有审计表** —— 那是 REQ-021 踩过的坑（AC 的前置物理不可达）。

## 依赖预算破例（D123 —— 本项目唯一一次）

**实测事实**：`Cargo.lock` 里 `rustls` / `native-tls` / `openssl` / `ring` **全 0 命中**，
`hyper-util` 的 `build_http()` 不带 TLS。而**真实企业 IdP（Azure AD / Okta / Keycloak）全部是 `https://`**
→ 只支持 `http` 的 OIDC **在真实环境基本不可用**。

**已上报用户，用户 2026-11-19 裁定「批准加 TLS 依赖，支持 https」。**

| 项 | 结论 |
|---|---|
| 破例范围 | **仅 TLS 连接器**：`rustls` + `ring` + `hyper-rustls`（优先方案） |
| 不得顺带引入 | `reqwest`（HTTP 栈继续用 `hyper` + `hyper-util`，只换 connector） |
| 必须实测报告 | `Cargo.lock` 包数增量（基线 **296**）、首次编译耗时、低配机（`jobs = 3`）能否编过 |
| https 与 http | **两者都要能用**（`https_or_http()`）：mock IdP 用 http，真实 IdP 用 https |
| 验证点 | AC-S-100 步骤 8：把 mock IdP 换成 `https://` 自签证书，必须同样登录成功 |
| 不要做 | 删掉 `SSO_OIDC_TLS_UNSUPPORTED` 这个错误码（已不需要） |

**这与既有的「避开 ring / aws-lc-rs」（`jsonwebtoken` 选 `rust_crypto`）不矛盾**：
那条是为 JWT 验签性能；TLS 没有成熟纯 Rust 等价物。**D123 是唯一破例点。**

## 阻塞项

无。依赖 REQ-007 / REQ-009 / REQ-010 均已合并 `develop`。

**不依赖** REQ-014（审计 sink，本轮只走接缝）与 REQ-022（出站 IdP）。

## 交付物

| 文件 | 内容 |
|---|---|
| [demand.md](demand.md) | 范围收窄说明 + 6 条安全约束 + 跨需求改动声明（D109） |
| [acceptance.md](acceptance.md) | **12 条 AC**（AC-S-020/021 重写 + AC-S-100~109 新增），逐条给错误码 |
| [design-summary.md](design-summary.md) | 现状盘点、数据模型、端点、时序、**21 个**错误码、12 个配置项、代码落点、开工前必答 |
