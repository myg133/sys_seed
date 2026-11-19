# REQ-013「SSO 入站（OIDC + LDAP）与强制 SSO」—— QA 挑刺报告

- **审查对象**: `BA/demands/REQ-013-sso/` 的四份文档 —— `demand.md` / `acceptance.md` / `design-summary.md` / `status.md`
- **审查版本**: 2026-11-19 **23:56** 的修订版（含 **D123**：批准引入 TLS 依赖支持 `https`）
- **代码基线**: `code/` @ `develop` @ `59486d0`（REQ-021 合并点）—— **只读核实，未改动、未编译**
- **审查者**: QA（myg133）
- **方法**: 逐 AC、逐 design 章节核对；每条结论都到 `code` 树里找**文件:行号**，或给出可复跑的命令。
  未跑 `cargo build` / `cargo test`（低配机 8 线程 / 7.7G，任务约束禁跑）。
- **配套用例集**: [test-cases/index.md](../demands/REQ-013-sso/test-cases/index.md)（**129 条**，12/12 AC 覆盖）

> **版本说明**：本报告针对 **23:56 修订版**。修订前（23:43 版）的两条结论已随之变化，此处如实记录：
> - 原「`SSO_OIDC_TLS_UNSUPPORTED` 条件性存在 → `no_orphan_titles` 两难」**已由 D123 解决**（该码被删除）；
>   但「18 个码」的残留表述仍在（见 S11），且 D123 自身引出了两条新问题（S13、S14）。
> - 全局断言 1 的「零新增依赖」已按 D123 改写为「TLS 是唯一破例」，这条**已修好**。

---

## 汇总

| 级别 | 数量 |
|---|---|
| **阻塞**（主体不存在 / 不可判定 / 物理不可达） | **14** |
| **高**（口径不一致 / 安全缺口 / 不可执行） | **13** |
| **中**（错误码设计 / 依赖缺失 / 表述） | **16** |
| **低**（措辞 / 格式 / 冗余） | **8** |
| **合计** | **51** |

**12 条 AC 里，12 条都还有问题**（无一条可原样实现并验收）。按 AC 分布：

| AC | 问题 | 其中阻塞 | 结论 |
|---|---|---|---|
| AC-S-020 | S10、S11、S24 | 2 | 「6 个码」错 + 与 109 口径冲突 |
| AC-S-021 | S9、S15、S16、S38、S39 | 2 | 前置不可达 + 耗时判据假绿 |
| AC-S-100 | S12、S13、S14、S27、S38、S48、S51 | 3 | 回调交接不可用 + https 验证点不可达 |
| AC-S-101 | S4、S5、S26、S31、S41 | 2 | 两个步骤物理不可执行 |
| AC-S-102 | S17、S34、S45 | 0 | 缺 `jku`/`x5c` 用例 |
| AC-S-103 | S19、S20、S33、S39 | 0 | 并发幂等无机制 |
| AC-S-104 | S6、S7、S18、S23、S32、S46 | 2 | 两个断言无观测面 |
| AC-S-105 | S21、S32 | 0 | 匹配顺序未定 → 409 不可达 |
| AC-S-106 | S1、S2、S3、S40 | 3 | 前三步跑不起来 |
| AC-S-107 | S22、S25、S46、S50 | 0 | 来源字段在 API 层不存在 |
| AC-S-108 | S8、S35、S37 | 1 | 接缝主体不存在 |
| AC-S-109 | S24、S40 | 0 | 粒度冲突 |

---

## 阻塞（14）

### S1: AC-S-106 步骤 1「`GET /api/config/entries` 取全量配置」**取不到全量**

- **级别**: 阻塞
- **位置**: acceptance.md AC-S-106 步骤 1、预期 1
- **问题**: 该端点**必须**给 `keys`（逗号分隔）；不给时 `split_keys(None)` 返回空列表 → 响应 `{"entries":[]}`。
  另外只要 `keys` 里含**任何** `scope: platform` 的键，整请求 `403`，而 `org` 模块的 3 个键正是 platform scope。
  所以「平台管理员令牌取全量」两种打法都拿不到数据：要么空，要么 403。
- **证据**:
  - `crates/platform-config/src/http.rs:261-264` —— `struct KeysQuery { keys: Option<String> }`
  - `crates/platform-config/src/http.rs:266-271` —— `split_keys(None) => Vec::new()`
  - `crates/platform-config/src/service.rs:135,540-552` —— 空 keys → 空 entries
  - `crates/platform-config/src/service.rs:143-157` —— platform scope 键 → `PlatformKeyOnGenericRoute`
  - `crates/platform-config/src/error.rs:156,173` —— 该错误 → `403` + `INSUFFICIENT_PERMISSION`
  - `crates/platform-org/manifest.json` `config` 段 —— `"scope": "platform"`
- **建议**: 改成「按**精确键名**读 12 个 SSO 键」并断言 `entries` 长度 `== 12`；「全量导出」是**不存在的能力**，
  要做得单独立项。→ TC-013-071 / 073。

### S2: AC-S-106 步骤 2 的 `?scope=tenant` 参数**不存在**

- **级别**: 阻塞
- **位置**: acceptance.md AC-S-106 步骤 2
- **问题**: 查询结构只有 `keys`，多传的 `scope` 被忽略 → 等价于「不带 keys」→ 返回空数组。
  测试会「0 命中并通过」，实际什么都没检查。
- **证据**: `crates/platform-config/src/http.rs:261-264`；`:274-288`（只读 `Query<KeysQuery>`）
- **建议**: 改成「租户管理员令牌 + 同一组键」，断言 `source ∈ {tenant, default}`。→ TC-013-072。

### S3: AC-S-106 步骤 7 的阳性对照要求「往**测试用**配置键写入」，但**没有这种键**

- **级别**: 阻塞
- **位置**: acceptance.md AC-S-106 步骤 7
- **问题**: 配置键**强制登记**（D53），未登记键写入 `404`。所以「测试用键」必须先登进 manifest ——
  等于为测试改生产契约。
- **证据**: `crates/platform-config/src/error.rs:172`（`UnknownKeys => 404`）；
  `crates/platform-config/src/service.rs:137-141`；D53（`BA/demands/REQ-001-platform-foundation/decisions.md:59`）
- **建议**: 用**已登记且无 `format`** 的既有键做阳性对照（`org` 模块 `ldapBaseDn`，string/platform/无 format），
  写完清理。→ TC-013-078。

### S4: AC-S-101 步骤 4「用 OIDC 的 `state` 去打 **LDAP** 登录接口」**物理无意义**

- **级别**: 阻塞
- **位置**: acceptance.md AC-S-101 步骤 4、预期 4
- **问题**: `POST /api/iam/sso/ldap/login` 的 body 只有 `{username, password}`，**不读 `state`**。
  把 `state` 放进 `username` 只会得到一次 bind 失败（`401 SSO_LDAP_BIND_FAILED`），
  不可能得到 `400 SSO_OIDC_STATE_INVALID`。
- **证据**: `design-summary.md:113-118`（端点表，LDAP body 定义）；`design-summary.md:255-268`（LDAP 流程，全程无 state）
- **建议**: 用 DB 侧把 `oidc_state.provider` 改成别的值再回调，验「`state` 绑定 provider」。→ TC-013-017。

### S5: AC-S-101 步骤 6「回调时把 `code_verifier` 换成随机的另一个值」**客户端做不到**

- **级别**: 阻塞
- **位置**: acceptance.md AC-S-101 步骤 6
- **问题**: `code_verifier` 是服务端生成并存 `platform_iam_oidc_state.code_verifier`（**明文列**），
  回调请求里**不带** verifier。测试无法「在回调时换掉它」。
- **证据**: `design-summary.md:47-60`（`code_verifier TEXT NOT NULL`）；`:68-74`（「明文存」的理由）
- **建议**: 改成 DB 侧 `UPDATE … SET code_verifier=…` 后回调；或让 mock 的 `/token` 做真实 PKCE 校验。→ TC-013-020。

### S6: AC-S-104 前置的「`ldap3` 的 in-memory 替身」**不存在**

- **级别**: 阻塞
- **位置**: acceptance.md AC-S-104 前置第 1 条；design-summary.md:418（§9 第 3 项）
- **问题**: `ldap3` 0.11.5 **没有服务端/内存实现**；`platform-org` 的 `InMemoryLdapSource`
  只是「检索结果的内存替身」，它的 trait **只有 `search`、没有 bind**。
  design 说「已有 `InMemoryLdapSource` 先例」，这个先例证明不了「bind 可替身」。
- **证据**:
  - `~/.cargo/registry/src/*/ldap3-0.11.5/src/` —— `conn/ldap/protocol/result/search/sync/…`，无 server
  - `crates/platform-org/src/ldap.rs:300-315` —— `trait LdapSource { fn search(...) }`
  - `crates/platform-org/src/ldap.rs:321` —— `InMemoryLdapSource`
- **建议**: 明确夹具（真实 OpenLDAP 容器 **或** 自写最小 LDAP stub），并写进 design §8。→ test-cases/index.md §4.2。

### S7: AC-S-104 步骤 8「mock LDAP 侧 0 条 bind」**没有观测面**

- **级别**: 阻塞
- **位置**: acceptance.md AC-S-104 步骤 8、预期 4/8
- **问题**: 「0 条 bind」是证明「拦截在发出前」的唯一判据，但**没有任何接缝**能让测试读到 bind 次数：
  `Ldap3Source` 是具体实现、直接持有 `ldap3::Ldap`，`LdapSource` trait 不含 bind。
  design §9 提了一句「替身要记录 bind 调用」，§8 的落点表里**没有**这个文件/trait。
- **证据**: `crates/platform-org/src/ldap.rs:449-496`（`connect()` 直接建连接，无注入点）；
  `design-summary.md:357-371`（后端落点表，无 LDAP client trait）
- **建议**: 加 `SsoLdapClient` trait（`connect`/`search`/`simple_bind`），生产走 `ldap3`、测试记计数，
  并写进 §8。→ TC-013-042（connect/bind/search 三项全 0）。

### S8: AC-S-108 的 `RecordingAuditRecorder` 注入与「payload 扫描」**主体都不存在**

- **级别**: 阻塞
- **位置**: acceptance.md AC-S-108 前置、步骤 2/3/5、预期 1/2/3/5
- **问题**: 三件事同时不成立：
  1. `platform_core` **没有** `AuditRecorder` trait（只有 `Auditable` + `AuditEventSpec`）；
  2. `AuditEventSpec` 字段是 `code/action/object_type/description`，**没有 payload**；
  3. `platform-org` 的 `RecordingAuditRecorder` 绑死 `OrgAuditEvent`，`platform-iam` 不能用
     （且 iam→org 会成环，design `:102-108` 自己写了这条）。
- **证据**:
  - `crates/platform-core/src/extension/auditable.rs:8-45`
  - `grep -rn AuditRecorder crates/platform-core/src` → 0 命中
  - `crates/platform-org/src/audit.rs:26-58`（`OrgAuditEvent`，只有 `detail: String`）、`:64-72`、`:89`
  - `grep -rin audit crates/platform-iam/src` → 0 命中
  - `design-summary.md:102-108`（iam 不能依赖 org）
- **建议**: 写死「`platform-iam` 自建 `SsoAuditEvent`（结构化 payload）+ `SsoRecordingAuditRecorder`」，
  并给 payload 的**白名单键集合**。→ TC-013-120/121/128。

### S9: AC-S-021 步骤 4「对 `tenant-b` 用 `alice` + 正确密码 → 200」**前置不可达**

- **级别**: 阻塞
- **位置**: acceptance.md AC-S-021 前置、步骤 4
- **问题**: 前置只写「`tenant-b` 未配 SSO」，而 `alice` 是 **`tenant-a` 的账号**。
  用户名查找带租户作用域，`tenant-b` 没有同名账号时是 `401 INVALID_CREDENTIALS`。
- **证据**: `crates/platform-iam/src/service.rs:388-392`；`migrations/20261118000400_iam_auth.up.sql:44-45`（`UNIQUE (tenant_id, username)`）
- **建议**: 前置补「`tenant-b` 有自己的 `alice`（不同密码）」。→ TC-013-106。

### S10: AC-S-020 预期 7 说「上述 **6 个**错误码」，实际只有 **5 个**

- **级别**: 阻塞
- **位置**: acceptance.md:59
- **问题**: 步骤 1~6 的不同码只有 5 个（步骤 5、6 共用 `SSO_LDAP_BIND_FAILED`，是**故意的**）。
  校验清单多一项会让「全部出现在 `ALL_CODES`」这条退化成不可判定。
- **证据**: acceptance.md:53-58（六条预期逐条点名 code）vs `:59`
- **建议**: 改成「上述 **5 个**错误码」。→ TC-013-097。

### S11: 新增错误码数量**五个口径**并存（9 / 13 / 17 / 18 / 18）

- **级别**: 阻塞
- **位置**: demand.md:149；acceptance.md:375；design-summary.md:295 与 :321 与 :368 与 :383；status.md:57、:94
- **问题**: 同一件事五个数字：

  | 出处 | 原文 | 数 |
  |---|---|---|
  | demand.md:149 | 「新增错误码（**约 9 个**）」 | 9 |
  | acceptance.md:375 | 「共 **13 个**新错误码」 | 13 |
  | design-summary.md:295 | 「## 6. 错误码清单（**13 个**新增）」 | 13 |
  | design-summary.md:301-319 | 表体 17 行；`:321`「表里实际 **17 个**码」 | 17 |
  | design-summary.md:368 | 「**18 个**新变体」 | 18 |
  | design-summary.md:383 | 「**18 个**新码的 ASCII title」 | 18 |
  | status.md:57 | 「**18 个**新码必须同时进 `TITLES`」 | 18 |
  | status.md:94 | 「…**18 个**错误码…」 | 18 |

  D123 删掉第 18 个码后，**只改了表体和一条脚注**，标题（13）、§8 两处（18）、status 两处（18）、
  demand（9）、acceptance 自检（13）全部**没同步**。这是 D109 意义上的「改动没改全」。
- **证据**（可复跑）:
  ```
  grep -oE '\b(SSO_[A-Z_]+|ALG_NOT_ALLOWED|NONCE_MISMATCH|PKCE_MISMATCH|TOKEN_INVALID|TOKEN_EXPIRED)\b' \
    BA/demands/REQ-013-sso/acceptance.md | sort -u | wc -l     → 17
  grep -c '^| [0-9]* | `SSO' BA/demands/REQ-013-sso/design-summary.md  → 17
  grep -c '^    ("' code/crates/platform-core/src/titles.rs            → 56（基线）
  ```
- **建议**: 全文统一成 **17**，并同步 6 处；`TITLES.len()` 的期望值是 **56 + 17 = 73**。

### S12: AC-S-100 步骤 3 的回调**不带租户头**，但 design 要求「`state.tenant_id` 必须等于请求头租户」

- **级别**: 阻塞
- **位置**: acceptance.md AC-S-100 步骤 3；design-summary.md:77-79
- **问题**: 真实流程里浏览器从 IdP 跳回 `/callback` **不可能带** `x-tenant-id`，AC 步骤 3 也没带；
  但 design §2.1 写「`state.tenant_id` 必须等于请求头租户」。两条规则不相容。
  另外 `/api/iam` 的 `tenant_context` 在缺头时把租户归一化为 `None`，
  于是回调里 `ctx.tenant_id()` 是 `None`，任何 `ctx.tenant_id().ok_or(...)` 都会 `401`。
- **证据**:
  - `crates/platform-iam/src/http.rs:400`（`/api/iam` 统一挂 `tenant_context`）
  - `crates/platform-iam/src/http.rs:245-253`（缺头 → `with_tenant("")`）
  - `crates/platform-core/src/context.rs:101-108`（`normalize("") => None`）
  - `crates/platform-iam/src/service.rs:670`（`ok_or(TenantContextMissing)`）
  - `design-summary.md:77-79`；acceptance.md:100
- **建议**: 写死「有头必须相等（不等 → `STATE_INVALID`）；无头采纳 `state.tenant_id` 并据此构造 `RequestContext`」，
  并补「回调必须把租户写进 session（否则 RLS 写入失败）」。→ TC-013-016、TC-013-006。

### S13: AC-S-100 新增的 https 验证点用「**自签证书 + 测试根**」，但设计只配了 `webpki` 根

- **级别**: 阻塞
- **位置**: acceptance.md:108-110（预期 8）；design-summary.md:236
- **问题**: AC 要求「mock IdP 换成 `https://`（自签证书 + 测试根证书）重跑必须成功」。
  但 design 给的构造是
  `HttpsConnectorBuilder::new().with_webpki_roots().https_or_http()…` ——
  **`webpki_roots` 不信任任何自签根**，而 §7 的 12 个配置键里**没有** CA / 根证书项，
  design 也没提「测试专用 `with_custom_root()`」。
  按现有规格实现，这条**必然失败**（且会以 `503 SSO_OIDC_DISCOVERY_FAILED` 的形式失败，
  让人误以为是网络问题）。
- **证据**: acceptance.md:108-110；design-summary.md:236（`with_webpki_roots()`）；
  design-summary.md:331-344（12 个配置键，无 CA 项）
- **建议**: 二选一并写进 design：① 加 `sso.oidc.caCertPem`（或 `sso.oidc.tlsRootCertPem`）配置键；
  ② 明确「测试期用 `with_custom_root(test_root)` 的测试构造，生产用 webpki」。
  → TC-013-129（守卫用例）。

### S14: `https` mock IdP 需要**服务端** TLS，超出 D123 的破例范围（「仅 TLS 连接器」）

- **级别**: 阻塞
- **位置**: acceptance.md:108-110；status.md:72
- **问题**: AC 要求「mock IdP 换成 https」。design §9 定的 mock IdP 是「**进程内 axum**」——
  要让 axum 提供 https，需要 `axum-server` / `tokio-rustls` 服务端配置这类依赖。
  而 D123 的破例范围写死为「**仅 TLS 连接器**：`rustls` + `ring` + `hyper-rustls`」。
  所以「AC 要求的验收手段」需要一份**没被批准的依赖增量**，且必然影响
  「`Cargo.lock` 包数增量必须实测报告」这个基线的可比性（dev-dependencies 也会进锁文件）。
- **证据**: status.md:72（破例范围）；design-summary.md:417（「mock IdP 用进程内 axum」）；
  acceptance.md:30（「包数增量必须实测报告」）
- **建议**: 明确 mock IdP 的 TLS 是否在破例内；若不在，改用「外部 nginx/traefik 终止 TLS」
  （不进 Rust 依赖）或「只在 CI 里用 `openssl s_server` 反代」。→ TC-013-129 前置。

---

## 高（13）

### S15: AC-S-021 步骤 3 的「耗时同量级（1.5×）」是**假绿断言**

- **级别**: 高
- **位置**: acceptance.md AC-S-021 步骤 5、预期 3
- **问题**: 三次请求在正确实现下走**同一条早退路径**，即使每次都跑一遍 Argon2id，三个耗时也同量级。
  该断言对「有没有跑 Argon2id」**没有区分力**；要能判定必须有「跑了」的基线。
- **证据**: `crates/platform-iam/src/service.rs:376-430`（login 前置只有限流 + 查账号）；
  `:394-422`（「账号不存在」本身也跑 `dummy_hash.verify`，耗时天然接近）
- **建议**: 改成「Argon2id 调用计数 `== 0`」+ 对照组（`enforce=false` 时 `== 3`）。→ TC-013-105。

### S16: 六条安全约束**没有一条**覆盖「`sso.enforce` 读不出来时怎么办」

- **级别**: 高
- **位置**: demand.md §安全约束 §2；acceptance.md AC-S-021
- **问题**: `sso.enforce` 来自配置三层（DB + 缓存）。读失败时若当成 `false`，强制 SSO **静默失效**。
  AC 没有任何一条守这个分支，也没写 fail-open / fail-closed。
- **证据**: `crates/platform-config/src/service.rs:159`（`resolver.refresh(ctx)` 可失败）；
  `design-summary.md:441-442`（AC-S-021 落点只有 `login` 开头 + `sso/config.rs`）
- **建议**: §2 追加「读取失败 → **fail-closed**（拒绝本地登录）」，并补 AC 步骤。→ TC-013-110。

### S17: design 明写「不从 token 自带的 `x5c`/`jku` 取公钥」，**12 条 AC 无一守它**

- **级别**: 高
- **位置**: design-summary.md:192-193；acceptance.md AC-S-102
- **问题**: 「从 token 自身取公钥 = 不验签」。design 列为要点，AC-S-102 却只测 `alg`、`iss/aud/exp`、
  篡改 payload、未知 `kid` —— Dev 只要在验签前读了 `header.x5c`，**12 条 AC 全部仍可通过**。
- **证据**: design-summary.md:192-193；acceptance.md:145-163（八步，无 `x5c`/`jku`）
- **建议**: 补两步：`jku` 指向攻击者 JWKS（断言攻击者端点 **0 次请求**）、`x5c` 带攻击者公钥（断言 401）。
  → TC-013-032/033。

### S18: demand 说「`simple_bind` 可直接校验密码」，但它**返回 `Ok` 即失败**，必须查 `rc`

- **级别**: 高
- **位置**: demand.md:27；design-summary.md:263-264
- **问题**: `ldap3::Ldap::simple_bind` 的签名是 `-> Result<LdapResult>`，**密码错误时外层仍是 `Ok`**
  （`rc=49`），必须再 `.success()` 才变 `Err`。design 只写「成功 → 继续；失败 → 401」，
  没写这个必须的二次判定。照字面实现 `simple_bind(..).await?` 并当成功 → **任意密码都能登录**。
  这是本需求唯一的「致命实现陷阱」。
- **证据**:
  - `~/.cargo/registry/src/*/ldap3-0.11.5/src/ldap.rs:238-259`
  - 同 crate `src/result.rs:281-287`（`rc == 0` 才 `Ok`）
  - 树里正确用法先例：`crates/platform-org/src/ldap.rs:481-495`（`.map_err…?` 之后再 `bound.success()…?`）
- **建议**: design §5 步骤 5 改写为「`simple_bind` → `LdapResult` → **必须** `.success()`；
  非 0 rc 一律归一为 `SSO_LDAP_BIND_FAILED`」。→ TC-013-040。

### S19: 复用的 `upsert_external_identity` 语义是「**改指**」，不是「只增」

- **级别**: 高
- **位置**: design-summary.md:98-110（§2.3）；demand.md 安全约束 §5
- **问题**: 既有 upsert 是 `ON CONFLICT … DO UPDATE SET user_id = EXCLUDED.user_id`。
  design 让 iam「照抄最小 upsert」= 把**改指**语义带进认证绑定路径：
  同一 `(provider, external_id)` 再写一次会**静默把绑定改到另一个本地用户**。
  §5「不得绕过禁用与租户隔离」**没有**「绑定行不可改指」这一条，AC 也没有断言守它。
- **证据**: `crates/platform-org/src/store.rs:647-667`，关键在 `:655-656`
  `ON CONFLICT (tenant_id, provider, external_id) DO UPDATE SET user_id = EXCLUDED.user_id, …`
- **建议**: 认证路径改用 `DO NOTHING` + 读回比对，不一致 → `409`；§5 补一条。→ TC-013-070。

### S20: AC-S-103 步骤 6 的并发幂等（3 并发 → 1 user / 1 绑定）**没有任何实现机制**

- **级别**: 高
- **位置**: acceptance.md AC-S-103 步骤 6、预期 6
- **问题**: design 没给锁、没给 user 插入的 `ON CONFLICT` 策略、没给「唯一冲突后重查」。
  能兜住的只有 `(tenant_id, email)` 部分唯一索引 —— 且只在「JIT 按 email 建号」时生效：
  - 并发第二个请求撞唯一索引 → 若没处理就是 `500`；
  - 若 JIT 按 `sub` 建号（不看 email）→ 两个 user 都能建出来，绑定 upsert 把 `user_id` 改到后者（叠加 S19）。
- **证据**:
  - `migrations/20261118000400_iam_auth.up.sql:48-50`（`UNIQUE (tenant_id, email) WHERE email IS NOT NULL`）
  - `crates/platform-org/src/store.rs:655-656`（绑定冲突走 `DO UPDATE`，不报错）
  - `design-summary.md:357-371`（落点表无锁/事务边界说明）
- **建议**: 写死「一个事务里 `INSERT … ON CONFLICT (tenant_id,email) DO NOTHING` → 重查 → 绑定 `DO NOTHING` + 读回比对」；
  并明确并发下允许 `409`。→ TC-013-065（判据含「5xx 数量 == 0」）。

### S21: AC-S-105 步骤 6 的 `409 SSO_LDAP_IDENTITY_CONFLICT` 在「已绑定」路径上**不可达**

- **级别**: 高
- **位置**: acceptance.md AC-S-105 步骤 6；design-summary.md:265
- **问题**: design §5 步骤 6 只写「从条目取映射字段（如 `mail`）→ 匹配/建本地 user」，
  **没说 `external_id` 与 `mail` 谁优先**：
  - 「`external_id` 命中即放行」→ 改 `mail` 后仍 `200`，`409` 永不出现；
  - 「`mail` 优先」→ 才可能 `409`。
  两种实现的验收结果相反，AC 只写了后者。
- **证据**: design-summary.md:255-268（时序 1~8，无匹配顺序说明）；`:98-110`（绑定表复用）
- **建议**: 写死「先查 `(provider, external_id)` 绑定；命中即用；未命中再按 `mail` 匹配；
  匹配到**他人已绑定**账号 → `409`」。→ TC-013-058/059。

### S22: AC-S-107 要求的会话来源字段在 **API 层不存在**，design 只加了数据库列

- **级别**: 高
- **位置**: acceptance.md AC-S-107 步骤 2、预期 2；design-summary.md:82-96
- **问题**: design §2.2 只给了 `ADD COLUMN auth_provider`，但要求是「会话记录里能看出它来自 SSO」——
  那是**响应体**的事。当前 `SessionView` 只有 6 个字段、**没有** `authProvider`；
  design §8 的落点表也没提 `SessionView` 与 `store.rs` 的 SELECT。
  另外 `SessionRecord` 由 `tenant_row!` **按列名** `try_get`：加 struct 字段却没加进 SELECT
  会在**运行期**报「列不存在」（不是编译错），`GET /api/iam/sessions` 直接 500。
- **证据**:
  - `crates/platform-iam/src/service.rs:147-159`（`SessionView` 六字段）
  - `crates/platform-iam/src/store.rs:582-596`、`:599-612`（两条 SELECT 逐列列举）
  - `crates/platform-db/src/lib.rs:100-112`（`tenant_row!` 逐字段 `try_get`）
  - `design-summary.md:82-96`、`:357-371`
- **建议**: §8 补两处落点（`SessionView` + 两条 SELECT），AC 写死字段名 `authProvider` 与取值枚举。
  → TC-013-112/115。

### S23: AC-S-104 步骤 7「连续 10 次错误 → 第 11 次 429」与**既有实现**冲突

- **级别**: 高
- **位置**: acceptance.md AC-S-104 步骤 7、预期 7
- **问题**: 三处不符：
  1. IP 限流阈值 **20 次 / 300 秒** → 第 11 次只有 `401`；
  2. 账号锁定阈值 **5 次** → 若 LDAP 失败计入锁定，第 6 次就是 `ACCOUNT_LOCKED`；
  3. 限流的 `ip` 取自 `x-forwarded-for`，**该头为空时直接跳过限流** → 不设这个头**永远不会** 429。
- **证据**:
  - `crates/platform-iam/src/config.rs:167-170`
  - `crates/platform-iam/src/service.rs:544-547`（`if ip.is_empty() { return Ok(()) }`）
  - `crates/platform-iam/src/service.rs:554-558`（`failures >= ip_limit`）
- **建议**: 改成「同一 `x-forwarded-for` 发满 `ip_limit` 次后第 `ip_limit+1` 次 → 429」，
  并写死「LDAP 失败是否计入账号锁定」。→ TC-013-051。

### S24: AC-S-020 要「可区分可排查」，AC-S-109 要「禁用 = 未配置合并」——**同一族端点两套粒度**

- **级别**: 高
- **位置**: acceptance.md AC-S-020 预期 1~4 vs AC-S-109 预期 5~6
- **问题**: 020 要求把「连不上（503）」与「JSON 非法（502）」分开、`detail` 还要带 issuer 主机；
  109 又要求「禁用」与「未配置」必须同码同 title。同一个租户的 SSO 配置状态，
  一部分细节被故意暴露、另一部分被故意合并，**没有统一判据**，实现者只能凭感觉。
- **证据**: acceptance.md:53-58 vs `:349-352`；design-summary.md:304（「未配或已禁用」）
- **建议**: 给统一判据（例：「未认证调用者可见的一律合并；细节只进日志」），并按同一标准重裁 020。→ TC-013-086。

### S25: AC-S-107 步骤 2 自相矛盾：「字段集合一致」与「能区分来源」落在同一字段上

- **级别**: 高
- **位置**: acceptance.md AC-S-107 预期 1 / 2
- **问题**: 加了 `authProvider` 后，两条会话在**该字段值上必然不同**，于是「字段集合一致」
  在「键集合」意义上成立、在「记录一致」意义上不成立。规格没区分这两种读法。
- **证据**: acceptance.md:289-291；`crates/platform-iam/src/service.rs:147-159`
- **建议**: 拆成「**键集合**必须相同（同一视图类型）；`authProvider` 是唯一允许不同的字段」。
  → TC-013-112/115。

### S26: AC-S-101 步骤 2 要「等过 `state` TTL（测试环境可配）」，但 **12 个配置键里没有 TTL**

- **级别**: 高
- **位置**: acceptance.md AC-S-101 步骤 2；design-summary.md:331-344
- **问题**: 「测试环境可配」暗示有配置项，但 §7 的 12 个键里没有。于是只能「真等 5 分钟」或「改 DB」，
  而后者没写。
- **证据**: design-summary.md:331-344（逐项核对无 TTL）；`:56-60`（TTL 只在表注释里）
- **建议**: 加 `sso.oidc.stateTtlSeconds`（number / tenant / 默认 300），或明确「常量 + 测试改 DB」。
  → TC-013-014/023。

### S27: 回调成功返回 **JSON `LoginResponse`**，在真实浏览器流程里**无法被接收**

- **级别**: 高
- **位置**: design-summary.md:150；AC-S-100 步骤 3
- **问题**: `/oidc/callback` 是**顶层浏览器导航**的落点。返回 `200 + JSON` 时浏览器会把 JSON 渲染成文本，
  SPA 拿不到 `refreshToken`。设计里既没有「回调 → 前端的一次性交接」（302 到 `/sso/done?ticket=…`），
  也没有前端回调路由。AC 用脚本客户端验证，**恰好掩盖了这条**。
- **证据**:
  - design-summary.md:150（「callback 与 ldap/login 成功 → 与 `POST /api/iam/login` 完全相同的 `LoginResponse`」）
  - `apps/shell/src/lib/routes.ts:1-40`（无 `sso/callback` 页）
  - design-summary.md:390-395（前端落点只有 `Login.svelte` / `SsoButtons.svelte` / `sso-api.ts`）
- **建议**: 二选一并写进 design：① 回调 302 到前端页 + 一次性 `ticket`；② 回调设 `HttpOnly` cookie 后 302 回首页。
  → 用例只覆盖脚本侧（TC-013-003），index §9 标为未覆盖。

---

## 中（16）

### S28: `SSO_OIDC_TOKEN_EXCHANGE_FAILED` 用**一个 400** 同时表示「code 伪造」与「IdP 5xx / 超时」

- **级别**: 中
- **位置**: design-summary.md:309
- **问题**: 换 token 失败有两类原因：① 我们发错了（code 伪造/已用）→ `400`；② IdP 挂了/超时 → 应 `502/504`。
  合成一个 `400` 会让前端提示「你的登录已失效」，运维也分不清是我们还是 IdP 的问题 ——
  正好违背 AC-S-020 的「可排查」。
- **证据**: design-summary.md:309；对照 discovery 分了 503/502 两个码（`:305-306`）
- **建议**: 拆 `…_TOKEN_EXCHANGE_FAILED`（400）与 `…_TOKEN_ENDPOINT_UNAVAILABLE`（502/504）。

### S29: `SSO_LDAP_EMPTY_PASSWORD` 用 **401**（请求畸形却报认证失败）

- **级别**: 中
- **位置**: design-summary.md:317
- **问题**: 空密码是请求前置条件不满足，不是凭据错误。`401` 会触发客户端「凭据失效 → 跳登录页」的通用处理，
  且与 `SSO_LDAP_BIND_FAILED` 共享同一语义频道。
- **证据**: `crates/platform-iam/src/error.rs:194-214`（401 的既有语义集合）
- **建议**: 改 `400`（保留独立码）；若坚持 401，须在 AC 里写理由。

### S30: 17 个码里**没有**「SSO 配置非法」这一类

- **级别**: 中
- **位置**: design-summary.md:348-351；§6
- **问题**: `validate()` 会拒绝 `ldaps://`、相对 `redirectUri`、filter 缺 `{username}`、空 `issuer`，
  但没有对应码，只能复用 `SSO_PROVIDER_NOT_CONFIGURED`（语义是「没配」，不是「配错」）。
- **证据**: `crates/platform-org/src/ldap.rs:432-447`（`ldaps://` 直接 `Err(Config(…))`）；
  design-summary.md:348-351；`:304`
- **建议**: 加 `SSO_CONFIG_INVALID`（400，`detail` 指明键），或明确「配置非法 = 未配置」并写进 §6。

### S31: 没有「回调缺参数」的错误码（缺 `code` / 缺 `state` / 重复参数）

- **级别**: 中
- **位置**: design-summary.md:301-319；AC-S-101
- **问题**: 17 个码里没有参数校验类。缺 `state` 只能借 `STATE_INVALID`、缺 `code` 只能借
  `TOKEN_EXCHANGE_FAILED` —— 语义上「没给参数」与「给的无效」不同，AC 也没写这两格。
- **证据**: design-summary.md:301-319（逐行核对）
- **建议**: 复用 `INVALID_ARGUMENT` 或新增 `SSO_CALLBACK_PARAM_MISSING`，并补 AC 步骤。→ TC-013-008/009。

### S32: LDAP 的 `provider` / `external_id` **取值未定义**，bind 次数也不确定

- **级别**: 中
- **位置**: design-summary.md:50-66（表）、`:255-268`（时序）
- **问题**: 两个进**唯一键**的列没定义：`provider` 是 `'ldap'` 还是 `'ldap-<摘要>'`；
  `external_id` 是 DN 还是 `uid`。另外步骤 3 写「用**服务账号** bind（**若要** search）」——
  「若要」让 bind 次数是 1 或 2，AC-S-104 的「bind 只带 DN 与密码」也因此无法计数。
- **证据**: design-summary.md:50-66；`:261`；`migrations/20261118000700_org_schema.up.sql:85`
- **建议**: 写死 `provider='ldap'`、`external_id=<DN>`，并明确是否用服务账号。→ TC-013-039/054。

### S33: JIT 建号的 `username` 生成规则与 `password_hash`（NOT NULL）都没定

- **级别**: 中
- **位置**: demand.md:80-88；design-summary.md:98-110
- **问题**: `platform_iam_user` 有 `username NOT NULL` + `UNIQUE (tenant_id, username)`，
  JIT **必须**造 username；`password_hash NOT NULL` 也**必须**给值。
  规则未定就无法判定「同 email 既有账号」时该 `409` 还是接管（后者是账号接管路径）。
- **证据**: `migrations/20261118000400_iam_auth.up.sql:27-45`
- **建议**: 写死「`username = email 本地部`；冲突 → `409`；`password_hash` = 永不匹配的固定 PHC 串」。
  → TC-013-062/069。

### S34: 算法白名单「**可由配置收窄**」，但 §7 没有这个键

- **级别**: 中
- **位置**: design-summary.md:195-201
- **问题**: 「可由配置收窄」暗示有配置项，实际没有 → 要么硬编码（「可配置」是空话），
  要么自造未登记键（写入 `404`）。
- **证据**: design-summary.md:195-201 vs `:331-344`
- **建议**: 删掉「可由配置收窄」，或加 `sso.oidc.allowedAlgs` 并限定只能取白名单子集。

### S35: AC-S-108 的注入会改 `IamService::new` 签名，**破 4 处调用点**（含 `platform-rbac` 测试），D109 未声明

- **级别**: 中
- **位置**: acceptance.md AC-S-108 前置；demand.md:145-155（跨需求改动表）
- **问题**: `IamService` 无 recorder 字段。改签名会破 4 处调用，其中 **2 处在 `platform-rbac/tests`**
  （已合并需求的产物），按 D109 必须在 demand 依赖段声明 —— 表里**没有**这一行。
  （更合理的是照 `PermissionProviderSlot` 的「后设」模式，就不破调用点。）
- **证据**:
  - `crates/platform-iam/src/service.rs:245-255`（`new(store, config)`）、`:257-262`（既有后设模式）
  - 调用点：`crates/platform-iam/tests/iam_real_db.rs:194`、
    `crates/platform-rbac/tests/me_permissions.rs:205`、`:590`、`crates/platform-server/src/main.rs:510`
  - demand.md:145-155（无此行）
- **建议**: design 写死「后设槽 `with_audit_recorder(…)`」；或补声明并接受破调用点。

### S36: 「新增错误码必须同时改**两处**」不准确 —— 实际至少 **8 个编辑点**

- **级别**: 中
- **位置**: demand.md:149；acceptance.md:31；design-summary.md:297-299
- **问题**: 完备性靠**四个**测试，新增变体要动：① code 常量 ② enum 变体 ③ `code()` match
  ④ `detail()` match ⑤ `ALL_CODES` ⑥ `TITLES` ⑦ `error.rs` 测试的 `variants` 数组
  ⑧ `error.rs` 测试的 `errors` 数组。漏 ⑦ 红在「清单里有码从未产出」，漏 ⑧ 红在「detail 非 ASCII」。
- **证据**: `crates/platform-iam/src/error.rs:32-89,363-455`；`crates/platform-core/src/titles.rs:45-122`；
  `crates/platform-server/tests/i18n_titles.rs:76,135`
- **建议**: 文档改成「8 个编辑点」并列清单。

### S37: 新表 `platform_iam_oidc_state` 的 DDL **没有 GRANT**

- **级别**: 中
- **位置**: design-summary.md:49-66
- **问题**: 本仓 IAM 表在迁移里显式 `GRANT … TO base_platform_app`；新表不加 GRANT，
  应用角色 `INSERT` 会 `permission denied for table`。design 明确「不启用 RLS」——那就更需要 GRANT。
- **证据**: `migrations/20261118000400_iam_auth.up.sql:236-245`；
  `migrations/20261118000500_iam_login_rate.up.sql:38-44`（无租户维度表也 GRANT）
- **建议**: 建表 SQL 后补 GRANT 块（带 `IF EXISTS (SELECT 1 FROM pg_roles …)` 守卫）。

### S38: 四处「不可机械」的表述

- **级别**: 中
- **位置**: acceptance.md:86（代码位置断言）、`:106`（「高熵」）、`:80`（`ssoProviders` 位置）、`:220-221`（复用限流表）
- **问题**: ① 「代码位置断言」不是测试能做的事；② 「高熵」无长度/字符集/重复率判据；
  ③ `ssoProviders` 在 `params`、顶层还是 `extensions` 未定；④ 「复用限流」没说是否计入账号锁定。
- **证据**: acceptance.md:80,86,106,220-221
- **建议**: ① 改成「DB 无该用户 → 403 而非 401」+「Argon2id 计数 0」；② 改成
  「`^[A-Za-z0-9_-]{43,}$` 且 20 次不重复」；③ 写死 `params.ssoProviders`；④ 写死是否计入
  `account_failure_limit`。→ TC-013-024/101/105/109。

### S39: AC-S-103 步骤 7 把**两个互斥前置**塞进一个步骤

- **级别**: 中
- **位置**: acceptance.md AC-S-103 步骤 7、预期 7
- **问题**: 「`tenant-b` 未开 JIT → 403；**开启时**建独立 user」—— 一个步骤两种配置两种结果，
  没有「先关再开」的动作序列，一条用例无法同时满足。
- **证据**: acceptance.md:181,191-192
- **建议**: 拆成 7a / 7b。→ TC-013-066 / 067。

### S40: `source` 的取值集合与 `scope: tenant` 的语义（D53）没交代

- **级别**: 中
- **位置**: acceptance.md AC-S-106 预期 4
- **问题**: 写「`source` 反映**三层**覆盖」，但 `ConfigSource` 是**四值**（含 `space`/`default`）；
  D53 把 `scope` 定义为「允许覆盖到的最深层」——`scope: tenant` 意味着**平台可写、space 不可写**。
  AC 既没写 `default`，也没说 space 为何不可写；实现者可能把 `sso.*` 登记成 `space`
  （做出「每空间一个 IdP」这种未被承认的形态）。
- **证据**: `crates/platform-core/src/extension/config.rs:50-59`；`crates/platform-core/src/config.rs:100-110`；
  D53（decisions.md:59）
- **建议**: 改成「`source ∈ {platform, tenant, default}`，**不得**为 `space`」，并补 D53 的解释。

### S41: 未认证者可**枚举租户**及其 SSO 配置状态（AC 主动要求，§5 未覆盖）

- **级别**: 中
- **位置**: acceptance.md AC-S-109 预期 1/4/6；demand.md 安全约束 §5
- **问题**: 三个未认证端点构成租户探测面：`providers` 的 `[]`/`[oidc]`/`[oidc,ldap]`、
  `authorize` 的 `302` vs `400`、discovery 的 `503` vs `502`。
  这些差异都是 AC **明文要求**的，但 §5 只提「不绕过禁用与租户隔离」，
  没有承认「租户存在性本身可枚举」。
- **证据**: acceptance.md:344-352；demand.md:133-137
- **建议**: §5 补一句「租户/provider 存在性对未认证调用者可见（登录页需要），本轮接受；
  **不得**进一步泄露 issuer/clientId」——并用 TC-013-089 守住「不泄露配置值」。

### S42: design §10 仍写「不会引入新的编译单元」，与 D123 **直接矛盾**

- **级别**: 中
- **位置**: design-summary.md:431-433
- **问题**: D123 批准 `rustls` + `ring` + `hyper-rustls`，这三者**都是新包**，`ring` 还要编 C/汇编。
  但 §10 仍写「全是 `Cargo.lock` 里已有的包，**不会引入新的编译单元**」，
  「预计首次 `cargo test -p platform-iam` 需要几分钟」也随之失真 ——
  而 D123 恰恰要求「首次 `cargo build` 耗时 + 低配机能否编过」必须实测报告（`:239-240`）。
  运维/排期会按 §10 的乐观估计做计划。
- **证据**: design-summary.md:431-433 vs `:224-249`（D123 段）；
  `code/Cargo.lock` 中 `rustls`/`ring`/`hyper-rustls` 全部 0 命中（`grep -c` 都为 0）
- **建议**: §10 改成「新增 TLS 栈（约 20~30 个包，含 `ring` 的 C 构建），
  首次编译时间**不可沿用旧估计**，以实测数为准」。

### S43: TLS 栈已进树，但 LDAP 仍只支持明文，且 §7 的注释指向已被改写的「风险」段

- **级别**: 中
- **位置**: design-summary.md:341（`sso.ldap.url` 行）、`:348-351`
- **问题**: `sso.ldap.url` 的说明仍写「`ldap://host:389`（本轮只支持明文，见 §4.2 风险）」——
  而 §4.2 现在已经是「D123：支持 https」的决策段，**不再有「风险」**。
  另外 `ldap3` 本身就有 `tls-rustls` feature：既然 TLS 栈已获批进树，
  「LDAP 明文」的技术理由（树里没有 TLS）已经不成立，需要重新说明或改为支持 `ldaps://`。
- **证据**: design-summary.md:341；`:220-249`（§4.2 已改写）；
  `~/.cargo/registry/src/*/ldap3-0.11.5/Cargo.toml:152-160`（`tls` / `tls-rustls` feature）；
  `crates/platform-org/src/ldap.rs:432-447`（`ldaps://` 被拒）
- **建议**: §7 该行去掉「见 §4.2 风险」，并明确「本轮 LDAP 是否支持 `ldaps`」；
  若不支持，给出**新**理由（而不是已被推翻的「没有 TLS 栈」）。

---

## 低（8）

### S44: design §8 让 `platform-iam`「加 `base64` / `sha2`」，但两者**已是它的依赖**

- **级别**: 低
- **位置**: design-summary.md:386
- **问题**: 该行列了 6 个依赖，其中 `base64`、`sha2` 已在 `platform-iam`，`http` 是 workspace 依赖。
  照抄会变成无意义改动，也让「依赖变更」清单失准。
- **证据**: `crates/platform-iam/Cargo.toml:44`（sha2）、`:48`（base64）；`Cargo.toml:106-108`
- **建议**: 改成「**新增**：`ldap3` / `hyper` / `hyper-util` / `http` / TLS 三件套；`base64`/`sha2` 已有」。

### S45: AC-S-102 预期 9 说「全部 **5 类**失败」，按步骤数是 **6~7 类**

- **级别**: 低
- **位置**: acceptance.md:163
- **问题**: 8 个步骤的失败类：`iss`、`aud`、`exp`、`alg=none`、`HS256`、`RS512`、签名篡改 ——
  至少 6 类（把三种 `alg` 合成一类则 4 类）。「5 类」与任何分法都对不上。
- **证据**: acceptance.md:145-163
- **建议**: 改成具体枚举，或删掉计数改成「每个被引用的 code 都在 `TITLES` 里」。

### S46: `DELETE /sessions/{id}` 与 `logout-all` 的返回码没钉死

- **级别**: 低
- **位置**: acceptance.md AC-S-107 预期 3/4
- **问题**: 只写「返回成功」/「作废全部 family」。既有实现是 `204 No Content`，
  不钉死会让「200 vs 204」变成实现自选。
- **证据**: `crates/platform-iam/src/http.rs:485-493`（`StatusCode::NO_CONTENT`）
- **建议**: 写「`204` 且响应体长度 0」。→ TC-013-113。

### S47: `BASE_LOG_LEVEL=trace` 会被 `RUST_LOG` 覆盖

- **级别**: 低
- **位置**: acceptance.md AC-S-106 步骤 4
- **问题**: 生产入口 `log_filter` **优先读 `RUST_LOG`**；环境里残留 `RUST_LOG=info` 时
  `BASE_LOG_LEVEL=trace` 不生效 →「日志 0 命中」变成因为**没打 trace**。
- **证据**: `crates/platform-server/src/main.rs:1061-1062`（注释明写）、`:247`、`:1096`
- **建议**: 补「测试时 `unset RUST_LOG`」，或加一条「日志里至少有 SSO 模块的 `tracing` 行」。

### S48: `tokenType` / `expiresIn` 的类型与取值没钉死

- **级别**: 低
- **位置**: acceptance.md AC-S-100 预期 3
- **问题**: 只写「字段集合完全一致」，值层面全空：`expiresIn` 数字还是字符串、`tokenType` 是否恒 `Bearer`、
  `permissions` 空时是 `[]` 还是省略。若只在键名层面「同构」，AC-S-107 的同构就是空的。
- **证据**: `crates/platform-iam/src/service.rs:163-184`（`token_type: "Bearer"`、`expires_in: u64`、
  `permissions: Vec<String>` 恒为数组）
- **建议**: 写死「`tokenType=="Bearer"`；`expiresIn` 正整数；`permissions` 恒为数组」。

### S49: demand 的「零新增依赖」与 §8 的「加 6 个依赖」读起来冲突（D123 后更需澄清）

- **级别**: 低
- **位置**: demand.md:187-188；design-summary.md:386
- **问题**: D123 已把 TLS 列为破例，但 §8 仍把 TLS 三件套与「已在 `Cargo.lock` 里」混在一行；
  读者要自己数锁文件才能判断「哪些是真新增」。
- **证据**: `grep -c '^\[\[package\]\]' code/Cargo.lock` → `296`（基线）；
  `grep -c 'name = "rustls"' code/Cargo.lock` → `0`
- **建议**: §8 拆成两行：「已存在（只加引用）」与「**D123 新增（进锁文件）**」，后者注明增量实测。

### S50: AC-S-109 步骤 2「产生的是**不同**会话（各自独立 family）」没给判定字段

- **级别**: 低
- **位置**: acceptance.md:346
- **问题**: 「不同会话」可由 `id` / `family_id` / `created_at` 任一判定；若实现复用 family，只比 `id` 仍通过。
- **证据**: `crates/platform-iam/src/service.rs:576-596`（每次 `issue_session` 生成新 `family_id`）
- **建议**: 写死「两次登录的 `family_id` 不相等，且 `platform_iam_session` 有两行」。→ TC-013-082。

### S51: AC-S-100 预期结果**编号错乱**（1,2,3,**8**,4,5,6），且新增项没有对应步骤

- **级别**: 低
- **位置**: acceptance.md:104-113
- **问题**: D123 修订在预期 3 与预期 4 之间插了一条 https 期望，编号写成 **8**，于是列表变成
  `1、2、3、8、4、5、6`。同时**测试步骤仍然只有 6 步**，没有任何一步对应「把 mock IdP 换成 https」。
  按编号读会漏掉第 8 条；按顺序读会以为它是最后一条。
- **证据**: acceptance.md:98-103（6 个步骤）vs `:105-113`（预期 1、2、3、8、4、5、6）
- **建议**: 编号改为 4（后续顺延为 5~7），并在步骤列表里插入「步骤 7：把 mock IdP 换成 https 重跑」。
  → TC-013-129 已按「独立用例」承载这条期望。

---

## 附 A：任务指定的 7 项实测核实结论

| # | 待核实项 | 结论 | 证据 |
|---|---|---|---|
| 1 | `ldap3::Ldap::simple_bind` 是否真能校验密码（design 说 `ldap.rs:238`） | **部分成立** —— 函数在 0.11.5 的 `ldap.rs:238` 确实存在，但**不能单独校验密码**：错误凭据返回 `Ok(LdapResult{rc:49})`，必须再 `.success()`。demand 写「可直接校验密码」是**不完整的事实**。 | `~/.cargo/registry/src/*/ldap3-0.11.5/src/ldap.rs:238-259`；`src/result.rs:281-287`；先例 `crates/platform-org/src/ldap.rs:481-495` |
| 2 | `external_identity` 唯一约束与 `upsert_external_identity` 签名 | **成立，但语义有坑** —— 表 6 列、`UNIQUE (tenant_id, provider, external_id)`；签名为 `(&self, ctx: &RequestContext, item: &ExternalIdentity) -> DbResult<()>`（`platform-org/src/store.rs:635`）。**但** upsert 是 `DO UPDATE SET user_id = EXCLUDED.user_id`（改指，见 S19）。 | `migrations/20261118000700_org_schema.up.sql:77-90`；`crates/platform-org/src/store.rs:635-667` |
| 3 | `platform_iam_session` 现有列；加 `auth_provider` 会不会破坏既有测试 | **不破坏，但有运行期陷阱** —— 现有 10 列；加列（`DEFAULT 'local'`）不破坏既有 INSERT / 测试（`iam_real_db.rs:1084-1087` 只断言 3 个字段存在）。**但** `SessionRecord` 由 `tenant_row!` 按列名 `try_get`：加 struct 字段却不改两条 SELECT → **运行期** `ColumnNotFound` → 500。 | `migrations/20261118000400_iam_auth.up.sql:68-81`；`crates/platform-iam/src/store.rs:115-136,582-612`；`crates/platform-db/src/lib.rs:100-112`；`crates/platform-iam/tests/iam_real_db.rs:1084-1087` |
| 4 | `IamError::ALL_CODES` 与 `titles.rs::TITLES`，确认「必须同时改两处」 | **不成立** —— 至少 **8 个编辑点**（见 S36）。漏 `variants` 红在「清单里有码从未产出」，漏 `errors` 红在「detail 非 ASCII」。 | `crates/platform-iam/src/error.rs:32-89,363-455`；`crates/platform-core/src/titles.rs:45-122`；`crates/platform-server/tests/i18n_titles.rs:76,135` |
| 5 | `AuditRecorder` / `AuditEventSpec` 真实签名（design 说照搬 org） | **不成立（照搬不了）** —— 核心只有 `Auditable { fn audit_events(&self) -> &[AuditEventSpec] }`；`AuditRecorder` 是 **org 私有**、参数写死 `OrgAuditEvent`（只有 `detail: String`，**无 payload**）。iam 必须自建。 | `crates/platform-core/src/extension/auditable.rs:8-45`；`crates/platform-org/src/audit.rs:26-58,64-72,89`；`grep -i audit crates/platform-iam/src` → 0 |
| 6 | `HttpScimGateway` 是否真用 `hyper_util` 且**不带 TLS** | **成立** —— `Client<HttpConnector, Full<Bytes>>` + `.build_http()`；`https://` 在 `validate()` 里被显式拒绝。所以「真实 IdP 全是 https」的确是真风险，D123 的破例有事实依据。 | `crates/platform-org/src/scim.rs:452-472`；`:197-210`；`:73-83` |
| 7 | `platform-iam` 是否真的没有依赖 `ldap3` | **成立** —— `platform-iam/Cargo.toml` 无 `ldap3`；只有 `platform-org/Cargo.toml:32`。`Cargo.lock` 已有 `ldap3 0.11.5`（workspace 依赖 `default-features = false`）。「不新增包」成立，但**要新增 `[dependencies]` 引用**。 | `crates/platform-iam/Cargo.toml`；`crates/platform-org/Cargo.toml:32`；`Cargo.toml:101`；`Cargo.lock:1177-1180` |

## 附 B：已核实为「成立、不需要改」的项（避免过度挑刺）

| 项 | 核实结果 |
|---|---|
| `Cargo.lock` 包数 296（基线） | **成立**（`grep -c '^\[\[package\]\]' Cargo.lock` → 296） |
| 全局断言 1 已按 D123 改写成「TLS 是唯一破例 + 增量实测报告」 | **成立**（acceptance.md:30；demand.md:187-188）—— 修订有效 |
| `SSO_OIDC_TLS_UNSUPPORTED` 已按 D123 删除 | **成立**（design-summary.md:322；status.md:77）—— 原「条件性错误码」问题已解决 |
| `external_identity` 唯一键 | **成立**（`20261118000700_org_schema.up.sql:85`） |
| `upsert_external_identity` 在 `platform-org/src/store.rs:635` | **成立**（行号精确） |
| `crypto::encrypt` / `random_base64url` 已交付 | **成立**（`crates/platform-iam/src/crypto.rs:39,76,93`） |
| 限流两张表已交付 | **成立**（`platform_iam_login_rate` 无租户维度、`platform_iam_login_attempt` 有 RLS） |
| `ACCOUNT_DISABLED` 是 403（AC-S-103 预期 5 正确） | **成立**（`crates/platform-iam/src/error.rs:205`） |
| `refresh` 在 family 作废后返回 `TOKEN_REVOKED`（AC-S-107 预期 3 正确） | **成立**（`crates/platform-iam/src/service.rs:706-710`） |
| `TITLES` 完备性 + 无孤儿测试存在 | **成立**（`i18n_titles.rs:76`、`:135`，含 `>= 45` 退化守卫） |
| `platform-iam` 无 audit 实现（status 第 7 条） | **成立** |
| LDAP 现有实现只做 search、无 bind 认证（status 第 3 条） | **成立** |
| 12 个 SSO 配置键都是合法 `ConfigKey` | **成立**（按 `.` 分段、每段 lowerCamelCase，12 个全过） |
| 三张配置表名正确 | **成立**（`20261118001000_config_schema.up.sql:36,56`；`20261118000200_tenant_isolation.up.sql:19`） |
| `apps/shell/src/pages/Login.svelte` 存在 | **成立** |

## 附 C：总体结论

**不建议按当前 rev 2（23:56 版）直接开工。** 三条理由：

1. **AC-S-106 是头号任务，三步跑不起来**（S1/S2/S3）。
   这条 AC 的全部价值在「0 命中是真的」，而三条读路径里两条拿不到数据、一条无法构造。
   照原文实现并「验收通过」，得到**三重假绿** —— REQ-021 的教训换了个位置重演。

2. **三条断言没有观测面**（S6/S7/S8）：LDAP 的「0 条 bind」、审计的「payload 扫描」、
   以及 https 的「自签证书 + 测试根」（S13）—— 都需要**尚不存在**的接缝，
   design §8 的落点表里也没有它们。不补接缝，这些只能「手工看日志」，不可复跑。

3. **两条设计事实会直接导致失效**：`simple_bind` 漏 `rc` → 任意密码通过（S18）；
   `external_identity` 的 `DO UPDATE` 被照抄进认证路径 → 绑定可被静默改指（S19）。
   两者都不是「实现细节」，而是**规格里写错/写少**。

**建议路径**：
1. 出 `acceptance.md` rev 3：先修 14 条阻塞，重点是 S1/S2/S3/S9/S12/S13/S14，并同步 S11 的数字（统一 17）。
2. 出 `design-summary.md` rev 3：补三处落点（`SsoLdapClient` trait、`SsoAuditEvent` payload、
   `SessionView.authProvider`），修 §6/§8 的 18→17，重写 §10 的编译成本估计（S42），
   明确 LDAP 是否支持 `ldaps`（S43）。
3. 再开工。

> **本轮 129 条用例可直接复用到 rev 3**：期望值是按**源码事实**与**可执行形式**写的，
> 不依赖错误表述。rev 3 落地后只需去掉 §7 的 16 条「可执行化修正」标注，
> 并把 12 条「守卫用例」的取值钉死。
