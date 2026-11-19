# REQ-013「SSO 入站（OIDC + LDAP）与强制 SSO」—— 测试用例总索引

- **需求编号**: REQ-013
- **父需求**: REQ-001（通用项目底座）｜**承接父 AC**: AC-005（⚠️ 本轮只交付 OIDC + LDAP，SAML / CAS 不交付）
- **验收基线**: [acceptance.md](../acceptance.md) rev 2 —— **12 条 AC**
  （AC-S-020 / AC-S-021 重写 + AC-S-100 ~ AC-S-109 新增）
- **设计依据**: [design-summary.md](../design-summary.md) rev 2
- **需求原文**: [demand.md](../demand.md)
- **代码基线**: `code/` @ `develop` @ `59486d0`（REQ-021 合并点）—— **本需求尚未实现**
- **状态**: 待实现（用例本身**尚未执行**，不得据此判定需求通过）
- **版本**: 第 1 版（rev 1）—— QA 逐条核对 acceptance rev 2 与 `code` 源码

> ⚠️ **本索引 §6 与配套的挑刺报告 `BA/feature-meta/qa-req013-spec-review.md` 是同一轮产出**：
> 用例按**可机械判定**写（已对 AC 的不可执行处逐条标注 `〔AC 修正〕`），
> 最危险的三条见 §1.1，全部 51 条的逐条证据见报告。

---

## 1. 概览

| 项 | 值 |
|---|---|
| 用例总数 | **129**（TC-013-001 ~ TC-013-129，编号连续、无跳号、无重复 —— 已用脚本核对；129 为 D123 修订后追加，见该用例「编号说明」） |
| 覆盖 AC | **12 / 12**（无遗漏、无多余） |
| 用例文件 | 7 个（另本索引 1 个） |
| 类型分布 | 功能 **51** / 安全 **71** / 边界 **7** |
| 优先级分布 | P0 **102** / P1 **25** / P2 **2** |
| 自动化分布 | 可自动化 **124** / 半自动 **5**（需注入计数器 / 真实容器 / TLS 根证书）/ 手工 **0** |
| 守卫用例（断言 AC **未定义**项，BA 钉死前会红） | **12 条**：TC-013-007、008、009、019、035、045、058、068、069、110、119、129 |
| 对 AC 做了可执行化修正的用例 | **16 条**（见 §7，逐条给挑刺编号） |
| 每条用例字段 | 关联 AC / 前置条件 / 测试步骤 / 预期结果 / 类型 / 优先级 / 自动化 / 设计方法 |

### 1.1 本轮最危险的三个结论（先看这三条）

1. **AC-S-106 的「三处扫描」有三步根本跑不起来**（挑刺 S1/S2/S3）。
   `GET /api/config/entries` **必须**带 `keys` 参数，不带就返回 `{"entries": []}`；
   `?scope=tenant` 这个参数**不存在**（只有 `keys`，`crates/platform-config/src/http.rs:261-264`）；
   「往一个测试用配置键写入」也不成立 —— 未登记键写入直接 `404`（D53 的强制登记）。
   照原文写测试，会得到「0 命中 + 什么也没读到」的**完美假绿**。

2. **LDAP 的两个关键断言没有可观测面**（挑刺 S6/S7）。
   `ldap3` 0.11.5 **没有** in-memory 服务端；`platform-org` 的 `InMemoryLdapSource`
   **只实现 `search`、不实现 bind**（`crates/platform-org/src/ldap.rs:300-315`）。
   AC-S-104 的「0 条 bind 请求」和「bind 只带 DN 与密码」今天**无法观测**，
   design §8 的文件落点里也没有这个接缝。

3. **AC-S-108 的三分之一建立在不存在的东西上**（挑刺 S8）。
   `platform_core` 只有 `Auditable` + `AuditEventSpec`（字段只有 code/action/object_type/description），
   **没有** `AuditRecorder`，**没有** payload 概念；`platform-org` 的
   `RecordingAuditRecorder` 绑定的是 `OrgAuditEvent`（`crates/platform-org/src/audit.rs:64-118`）。
   AC 要求「用 `RecordingAuditRecorder` 注入」并「对 payload 做敏感值扫描」——
   两件事在现有接缝上都做不到，必须先在 `platform-iam` 里建一套。

---

## 2. 文件清单与 AC 映射

| 文件 | 能力域 | AC 范围 | 用例数 | 编号段 |
|---|---|---|---|---|
| [oidc-flow.md](oidc-flow.md) | OIDC 授权码流程端到端（含 `https` D123 验证点） | AC-S-100 | **13** | 001 ~ 012、129 |
| [oidc-security.md](oidc-security.md) | `state`/`nonce`/PKCE + `id_token` 验签 | AC-S-101、AC-S-102 | **26** | 013 ~ 038 |
| [ldap.md](ldap.md) | LDAP bind 成功/失败/不可达/空密码 + 与本地共存 | AC-S-104、AC-S-105 | **22** | 039 ~ 060 |
| [binding.md](binding.md) | 账号绑定与 JIT 建号 | AC-S-103 | **10** | 061 ~ 070 |
| [config-secret.md](config-secret.md) | `client_secret` 三处扫描 + 多 IdP 与租户隔离 | AC-S-106、AC-S-109 | **19** | 071 ~ 089 |
| [errors-enforce.md](errors-enforce.md) | 失败可诊断 + 强制 SSO | AC-S-020、AC-S-021 | **21** | 090 ~ 110 |
| [session-audit.md](session-audit.md) | 会话同构 + 审计事件声明与发出 | AC-S-107、AC-S-108 | **18** | 111 ~ 128 |
| **合计** | — | **12 条 AC** | **129** | **001 ~ 129** |

---

## 3. 追溯矩阵（AC → TC，含每条 AC 的用例数与「无悬空 TC」核对）

| AC | 主题 | 用例编号 | 用例数 | 其中 P0 | 悬空 TC |
|---|---|---|---|---|---|
| AC-S-020 | 协议失败可诊断（可区分错误码） | TC-013-090 ~ 100 | **11** | 9 | 0 |
| AC-S-021 | 强制 SSO（不可用于枚举） | TC-013-101 ~ 110 | **10** | 10 | 0 |
| AC-S-100 | OIDC 授权码流程端到端（含 `https`） | TC-013-001 ~ 012、129 | **13** | 7 | 0 |
| AC-S-101 | `state`/`nonce`/PKCE 防 CSRF 与重放 | TC-013-013 ~ 024 | **12** | 8 | 0 |
| AC-S-102 | `id_token` 验签与算法白名单 | TC-013-025 ~ 038 | **14** | 11 | 0 |
| AC-S-103 | 账号绑定与 JIT 建号 | TC-013-061 ~ 070 | **10** | 10 | 0 |
| AC-S-104 | LDAP bind（成功/失败/不可达/空密码） | TC-013-039 ~ 052 | **14** | 11 | 0 |
| AC-S-105 | LDAP 与本地账号共存 | TC-013-053 ~ 060 | **8** | 5 | 0 |
| AC-S-106 | `client_secret` 三处扫描 0 命中 | TC-013-071 ~ 080 | **10** | 9 | 0 |
| AC-S-107 | SSO 会话与本地会话同构 | TC-013-111 ~ 119 | **9** | 8 | 0 |
| AC-S-108 | SSO 审计事件（声明 + 发出，不落库） | TC-013-120 ~ 128 | **9** | 7 | 0 |
| AC-S-109 | 多 IdP 并存与租户隔离 | TC-013-081 ~ 089 | **9** | 7 | 0 |
| **合计** | — | **TC-013-001 ~ 129** | **129** | **102** | **0** |

**核对方式（可复跑）**：
1. `grep -c '^### TC-013-' *.md` → 各文件计数与上表一致（10+19+21+22+13+26+18 = 129）。
2. `grep -h '^### TC-013-' *.md | sed -E 's/^### (TC-013-[0-9]+).*/\1/' | sort` 与
   `seq -f "TC-013-%03g" 1 129` **逐行相同**（无跳号、无重复、无越界）。
3. 每条用例的 `- **关联 AC**` 只有一个值，且该值 ∈ 上表 12 个 AC（**无悬空 TC**：
   没有任何 TC 关联到本需求之外的 AC，也没有任何 AC 没有 TC）。

---

## 4. 夹具约定

> **原则 1**：造数走管理员连接（`base_test`），业务访问走 `base_app`；
> 绝不写 `base_dev`（acceptance 全局断言 5）。
> **原则 2**：夹具走独立幂等播种脚本（D101），不依赖其它测试的副作用。
> **原则 3**：E2E 的 `skip` 只允许「被测系统未就绪」；403/500 必须 `fail`（D108）。

### 4.1 租户与用户

| 标识 | 配置 | 用途 |
|---|---|---|
| `tenant-a`（T-A） | OIDC + LDAP 全配；`sso.enforce` 按用例切换；`sso.jitProvisioning` 按用例切换 | 主租户 |
| `tenant-b`（T-B） | 只配 OIDC，指向 `idp-b`（9411）；`sso.enforce=false` | 租户隔离 / 强制 SSO 不串 |
| `tenant-c`（T-C） | 未配任何 SSO | 「未配置」分支（AC-S-109） |

| 账号 | 租户 | 附件 | 用途 |
|---|---|---|---|
| `alice` | tenant-a | 本地密码 `Al1ce-correct` | 强制 SSO 的 3 条枚举用例 |
| `alice` | tenant-b | 本地密码 `Al1ce-tb`（**同名不同人**） | 证明 `sso.enforce` 是租户级（TC-013-106） |
| `bob` | tenant-a | 绑定 `sub=idp-bob-001`、`email=bob@example.com` | OIDC 主账号 |
| `carol` | tenant-a | IdP 侧 `sub=idp-carol-001`，底座**无**绑定 | JIT |
| `dave` | tenant-a | 绑定 `sub=idp-dave-001`，`status='disabled'` | 禁用不得绕过 |
| `grace` | tenant-a | 本地账号 | 会话同构对照 |
| `frank` | tenant-a | 本地 + 目录同 `mail` | LDAP 与本地共存 |
| `erin` | 仅目录 | `uid=erin,ou=people,dc=example,dc=com`，`Er1n-pass` | LDAP bind |
| `nobody` / `ghost` | — | 不存在 | 枚举 / 不可区分 |

### 4.2 外部服务（全部进程内/本机，无外网）

| 夹具 | 地址 | 必须暴露的观测面 |
|---|---|---|
| mock IdP `idp-a` | `http://127.0.0.1:9410/idp-a` | `/.well-known/openid-configuration`、`/authorize`、`/token`、`/jwks`；**每端点计数器**；`/__idp/override`（自定义 `iss/aud/exp/nonce/alg/kid/x5c/jku/omit_nonce`）；`/__stats` |
| mock IdP `idp-b` | `http://127.0.0.1:9411/idp-b` | 同上（独立 `sub` 空间） |
| mock IdP `idp-a-tls` | `https://127.0.0.1:9413/idp-a` | 自签证书（SAN 含 `127.0.0.1`）+ 测试根 `fixtures/tls/test-root.pem`；用于 D123 验证点（TC-013-129） |
| 故障 IdP | `:9420` 返回 500 / `:9421` 非法 JSON / `:9422` 缺 `token_endpoint` | 只用于 AC-S-020 |
| **mock 目录服务** | `ldap://127.0.0.1:9389` | **`__stats = {connect, bind, search}` + `last_filter`** ⚠️ 必须新建（S6） |
| 攻击者 JWKS | `http://127.0.0.1:9499/jwks` | 请求计数器（AC-S-102 的 `jku`/`x5c` 用例） |

> ⚠️ **两个夹具是新造的，design 里没有**：
> - **mock 目录服务**：`ldap3` 0.11.5 无服务端实现；`InMemoryLdapSource` 只有 `search`。
>   「0 条 bind」只能靠「真实 OpenLDAP + 记录 BIND op 的代理」或自写最小 LDAP stub。
> - **可注入计数的密码校验器**：TC-013-105 用它取代 AC 的「耗时 1.5×」判据。

### 4.3 关键配置值

| 键 | 值 | 说明 |
|---|---|---|
| `sso.oidc.issuer` | `http://127.0.0.1:9410/idp-a` | `http` 与 `https` **都要支持**（D123：`https_or_http()`）；https 夹具见 §4.2 |
| `sso.oidc.clientId` | `base-portal` | 非敏感，可读 |
| `sso.oidc.redirectUri` | `http://127.0.0.1:8080/api/iam/sso/oidc/callback` | 与 IdP 侧逐字一致 |
| `sso.oidc.scopes` | `openid email profile` | 空格分隔 |
| `sso.ldap.userSearchFilter` | `(uid={username})` | 必须含 `{username}` |
| `BASE_SSO_OIDC_CLIENT_SECRET` | `SECRET-CANARY-7f3a9b1e-UNIQUE` | canary，用于三处扫描 |
| `state` TTL | 300 秒（默认） | **无配置键**，过期用例靠 `UPDATE expires_at` 构造 |

---

## 5. 执行层次与命令

| 层 | 内容 | 命令（BA 待确认） |
|---|---|---|
| `rust-unit` | `TITLES` 完备性、`state` 熵、filter 转义、`LdapResult.rc` 判定 | `cargo test -p platform-iam --all-targets`、`cargo test -p platform-server --test i18n_titles` |
| `rust-integration` | 6 类失败码、OIDC 全链路（进程内 mock IdP）、LDAP 全链路 | `cargo test -p platform-iam --all-targets`（需 `DATABASE_URL_TEST` 指向 `base_test`） |
| `rust-integration`（真库） | AC-S-100 / 107 / 109 的端到端 | `cargo test -p platform-server --test sso_real_db` |
| `frontend-unit` | `/api/iam/sso/providers` → 企业登录按钮渲染 | `pnpm --filter @base/shell test` |
| `e2e` | 登录页出现企业登录入口（mock IdP 在 E2E 里起） | `pnpm playwright test e2e/sso.spec.ts` |

> **低配机约束**（design §10）：`cargo test` **逐 crate** 跑，`--workspace` 必失败；
> 必须带 `--all-targets`；`.cargo/config.toml` 固定 `jobs = 3`。
> 本轮**未跑任何 cargo 命令**（QA 只做只读核实）。

---

## 6. 本需求的假绿清单（每条都有对应用例）

| 陷阱 | 为什么在本需求特别危险 | 守它的用例 |
|---|---|---|
| 只断言「0 命中」不给阳性对照 | 扫描器坏了与真的干净无法区分（REQ-021 教训） | TC-013-078、079（两条阳性对照：配置侧 + 日志侧） |
| `GET /api/config/entries` 不带 `keys` | 返回空数组 → 「0 命中」但什么也没读（**最危险的一条**） | TC-013-071（断言 `entries` 长度 `== 12`） |
| 「耗时同量级」证明没跑 Argon2id | 三次请求走**同一条早退路径**，跑与不跑都同量级 | TC-013-105（改成计数器 + 对照组） |
| 只断言「认证失败」不断言 bind 次数 | 空密码被转成匿名 bind 后**服务端仍然成功** | TC-013-042（`bind/connect/search` 三项全 0） |
| 只断言「两次响应都可区分」 | 两个分支给同一个错误码时，「可区分」无从判定 | TC-013-027（`EXPIRED != INVALID` 逐字比较） |
| 断言「未知 `kid` 登录成功」 | 「每次请求都刷 JWKS」也成功 | TC-013-034（`__stats.jwks == 1`） |
| 用同一个 `currency`/租户测隔离 | 配置没生效时结果也一样 | TC-013-087（改 T-B 后 T-A 仍 200，且 T-B 变 503） |
| 把 `skip` 当通过 | D108 的教训 | 无 `skip`；403/500 一律 `fail` |
| 断言 payload「含 X」 | 下界断言对「多塞了敏感字段」无感 | TC-013-128（白名单键集合，多一个键即红） |
| 用 `deviceLabel` UA 猜来源 | 自由文本当判定依据必然出错 | TC-013-112（`authProvider` 枚举值） |

---

## 7. 用例对 AC 的「可执行化修正」清单（16 条用例 / 15 行）

| 用例 | AC 原文不可执行处 | 修正为 | 挑刺编号 |
|---|---|---|---|
| TC-013-006 | 未给 `/api/iam/refresh` 的 `x-tenant-id` | 显式带头（缺头会 `401 TENANT_CONTEXT_MISSING`） | S12 |
| TC-013-016 | 未定「回调无租户头」时如何取租户 | 双分支都断言 `400 STATE_INVALID` | S12 |
| TC-013-017 | 「用 OIDC 的 `state` 打 LDAP 接口」无意义 | DB 侧篡改 `provider` 列 | S4 |
| TC-013-020 | 「回调时换 `code_verifier`」客户端做不到 | DB 侧替换 `code_verifier` | S5 |
| TC-013-039 | bind 次数 1 还是 2 未定 | 断言值与实现约定写死 | S32 |
| TC-013-042 | 只要求「0 条 bind」 | 追加 `search`/`connect` 也是 0 | 补充 |
| TC-013-051 | 「10 次后第 11 次 → 429」与实测不符 | 按 `ip_limit=20` 写第 21 次 | S23 |
| TC-013-058 | `mail` 冲突与绑定的判定顺序未定 | 标注守卫 + 要求钉死顺序 | S21 |
| TC-013-067 | 一步塞两个互斥前置 | 只取「开 JIT 建独立 user」 | S39 |
| TC-013-069 | OIDC 侧「同 email 既有账号」无规则 | 标注守卫，要求钉死 | S33 |
| TC-013-071/072 | 全量配置 / `?scope=tenant` 不可执行 | 精确 12 键 + 分路由两次读 | S1/S2 |
| TC-013-078 | 「测试用配置键」不存在 | 用已登记无格式的 `ldapBaseDn` | S3 |
| TC-013-105 | 耗时判据假绿 | 计数器 + 对照组 | S15 |
| TC-013-110 | 无「enforce 读配置失败」用例 | fail-closed 守卫用例 | S16 |
| TC-013-129 | 「自签证书 + 测试根」与 `with_webpki_roots()` 冲突 | 断言「必须提供信任测试根的途径」，否则守卫红 | S13 / S14 |

---

## 8. 依赖 BA 先钉死的前提（否则对应用例只能写成「守卫」）

**这 9 项不钉死，对应断言无法机械判定。**

| # | 待钉死项 | 影响用例 | 建议取值 |
|---|---|---|---|
| P1 | **回调不带 `x-tenant-id` 时的租户来源** | 013-016、013-115 之后全部回调类 | 「有头必须相等；无头采纳 `state.tenant_id`」 |
| P2 | **`oids_state` 的 TTL 是否可配** | 013-014、013-023 | 加 `sso.oidc.stateTtlSeconds`（或明确只在测试里改 DB） |
| P3 | **LDAP 的 `provider` / `external_id` 取值** | 013-054、013-059、013-088 | `provider='ldap'`、`external_id=<DN>`（DN 稳定、`uid` 可改） |
| P4 | **LDAP 匹配顺序**（`external_id` 优先还是 `mail` 优先） | 013-058、013-059 | `external_id` 优先（绑定优先于属性），冲突时 `409` |
| P5 | **JIT 的 `username` 生成规则与冲突处理** | 013-062、013-069 | `email` 本地部；冲突 → `409`，不自动加序号 |
| P6 | **JIT 建号时 `password_hash`（NOT NULL）写什么** | 013-062 | 写入一个永不匹配的固定 PHC 串（不是空串） |
| P7 | **`state` 过期判定与「已消费」谁优先** | 013-013、013-014 | 先判 `expires_at` 再判 `consumed_at`（过期码更可诊断） |
| P8 | **审计事件声明的读取面** | 013-120、013-124 | `IamModule` 实现 `Auditable`（与 org 一致）+ `pub const SSO_AUDIT_EVENTS` |
| P9 | **`ssoProviders` 在错误体里的位置** | 013-101 ~ 013-104 | `ProblemDetails.params.ssoProviders`（契约 C6 的既有扩展位） |

---

## 9. 未覆盖/不覆盖的说明（避免「看起来全绿」）

| 项 | 为什么不覆盖 |
|---|---|
| SAML 2.0 / CAS | 本轮**不交付**（demand 范围收窄，用户 2026-11-19 裁定）；父 AC-005 已显式标注 |
| 出站 IdP、免登跳转、单点登出 | 移到 REQ-022 |
| 审计**落库与查询** | REQ-014 才提供 sink；本轮只有 AC-S-108 的「声明 + 发出」，且 TC-013-127 **反向断言**没有审计表 |
| `https://` IdP 的**证书信任配置** | D123 已批准 TLS，`https` 正向用例为 TC-013-129；但「自签证书 + 测试根」如何被信任（CA 配置键 / 测试专用根）**规格里没有**，故该用例是守卫 |
| 多 IdP 的管理界面 | 本轮只支持配置层声明（demand「不在范围内」） |
| 前端「用企业账号登录」按钮的交互细节 | 只有一条 E2E 冒烟（`e2e/sso.spec.ts`），不覆盖视觉/品牌（REQ-010 的范围） |
