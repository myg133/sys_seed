# 设计摘要：认证与账号体系（REQ-007）

- **需求编号**: REQ-007
- **父需求**: [REQ-001](../REQ-001-platform-foundation/demand.md)
- **依赖**: [REQ-006 多租户与隔离](../REQ-006-tenant/design-summary.md)（会话与令牌必须从第一行就带租户维度）
- **接口契约**: [api-conventions v1](../../contracts/api-conventions/v1.md)（C6 错误体 / C8 幂等键 / C13 命名与编码）
- **性质**: 需求细化附件 —— **本项目第一个安全敏感模块**，架构决策在此定死
- **更新时间**: 2026-11-18 11:40

---

## 0. 一句话

账号体系**必须自持**（ADR-0001 A3）：底座是身份的唯一权威源，
SSO 只是「另一种登录方式」，不是「身份来源」。

---

## 1. 身份模型：自持账号 + SSO 挂载

```
本地账号模式（默认）：用户名/邮箱 + 密码，底座是唯一身份源
SSO 挂载模式：外部 IdP 登录 → 自动创建/关联本地账号 → 之后权限完全本地管
混合模式（企业常见）：账号在底座，组织架构来自 LDAP/AD
```

**本地账号的稳定主键是底座一切权限模型的地基** —— RBAC、数据范围、审批流、审计
全部挂在它上面。所以**不能**把外部 IdP 的 user id 当主键。

外部 IdP 登录后，底座建一条 `user` 记录 + 一条 `external_identity` 映射
（`provider` + `external_id` → `user_id`）。

---

## 2. 令牌设计：**混合式**（推荐）

| 令牌 | 形态 | TTL | 存储 | 撤销 |
|---|---|---|---|---|
| **access token** | JWT（自包含，含 `kid`） | **15 分钟** | 不存（客户端持有） | 靠短 TTL 兜底 |
| **refresh token** | **不透明随机串** | **30 天** | DB 存 **hash** | **可立即撤销** |
| 令牌家族（family） | 一组 refresh 的归属 | — | DB | 检测到重放 → **整个 family 作废** |

**为什么混合**：

- 全 JWT → **无法撤销**。管理员强制下线、用户改密码后踢出，都做不到。
  「设备会话管理」「登出全端失效」这两条需求直接落空。
- 全不透明 → 每次请求都要查库。高频接口上这是不可接受的延迟，
  而且 access token 和 refresh token 的生命周期本来就不同。

**混合式兼顾**：access token 短 TTL 换性能，refresh token 可撤销换控制力。

### 2.1 refresh token 轮换与重放检测

```
客户端持 refresh_1 → 刷新 → 服务端作废 refresh_1，签发 refresh_2（记入 family F）

若 refresh_1 被再次使用 → 判定为泄露 → **整个 family F 立即全部作废**
```

这是 OAuth 2.0 Security BCP 的标准做法。**轮换必须真的作废旧 token** ——
只签发新的不废旧，等于没有轮换。

### 2.2 JWT 的密钥管理

- 签名密钥**不入库不入 Git**，从环境变量 / Secret 注入
- **JWKS 端点**：`GET /.well-known/jwks.json` 公开公钥（ADR-0001 A3 要求）
- 密钥带 `kid`，支持**轮换**：新旧密钥同时有效一小段时间，客户端按 `kid` 选
- kid 轮换策略：当前密钥 + 上一把密钥（宽限期内双有效）

> ⚠️ **JWKS 是公开的** —— 公钥可以公开，**私钥绝不可以**。

---

## 3. 密码：Argon2id

**决定**：用 `argon2` crate（Argon2id），参数按目标机器调。

| 项 | 决定 | 理由 |
|---|---|---|
| 算法 | **Argon2id** | 当前推荐（OWASP / NIST SP 800-63）。比 bcrypt 抗 GPU 更强，bcrypt 72 字节截断是已知缺陷 |
| 参数 | **可配置**，默认值按目标机器定 | 低配开发机跑不动生产参数；生产值应在配置里给足 |
| 依赖 | `argon2`（会拉 blake2 / base64ct 等约 10 个 crate） | **安全模块不省依赖** —— 自制密码哈希绝不可接受 |
| 存储格式 | PHC 标准串（`$argon2id$v=19$m=...,t=...,p=...$salt$hash`） | 算法自描述，将来换算法/调参数不用改表结构 |
| 校验 | **恒定时间比较**（库自带） | 防时序侧信道 |

**依赖取舍说明**：Sprint 2 的其余部分坚持「不引重量级依赖」，但认证模块**例外** ——
这里省依赖等于自造密码学。正确做法是**引成熟库 + 记入依赖预算**。

### 3.1 密码策略（可配置）

| 项 | 默认 |
|---|---|
| 最小长度 | **12** |
| 复杂度 | 不强制「大写+小写+数字+符号」组合，但**禁止**出现在泄露密码库里 |
| 有效期 | 180 天（可关） |
| 历史不重复 | 最近 5 次 |
| 首次登录 | 强制改密 |

**泄露密码库检查**：接入 HIBP Pwned Passwords 的 k-anonymity 模式
（只发 SHA-1 前 5 位，服务端返回后 5 位匹配）——**不要把完整密码哈希发出去**。
本期可只做本地小词库 + 长度策略，泄露库接入放后续需求。

---

## 4. MFA

| 方式 | 本期 | 说明 |
|---|---|---|
| **TOTP** | ✅ 实现 | `totp-rs`（HMAC-SHA1 + base32），标准做法 |
| **WebAuthn / FIDO2** | ⛔ 只留接口 | 依赖重（`webauthn-rs` 一大票），本期不实现，**但接口要留** |
| 短信验证码 | ⛔ 不做 | 需要短信服务商；本期用 TOTP 覆盖 |

**TOTP 实现要点**：
- secret 用 **base32**，存 DB 时**加密或至少哈希**（`totp-rs` 提供 hashed secret）
- 时间窗口容差 **±1 个 30 秒步**
- **恢复码**：绑定时生成 10 个一次性恢复码，存 hash；每个只能用一次
- 防重放：记录最近一次成功的时间步，同一时间步的验证码**不能复用**

---

## 5. 接口设计（遵循契约 C6）

| 方法 | 路径 | 说明 |
|---|---|---|
| POST | `/api/iam/login` | `{ username, password, mfa_code? }` → `{ access_token, refresh_token, expires_in, user, permissions }` |
| POST | `/api/iam/refresh` | `{ refresh_token }` → 新的 access + **轮换后的** refresh |
| POST | `/api/iam/logout` | 撤销当前 session |
| POST | `/api/iam/logout-all` | 撤销该用户全部 session |
| GET | `/api/iam/sessions` | 设备会话列表（设备 / IP / 最近活跃） |
| DELETE | `/api/iam/sessions/{id}` | 踢下线 |
| POST | `/api/iam/password/forgot` | 发送重置链接（响应**不泄露邮箱是否存在**） |
| POST | `/api/iam/password/reset` | `{ token, new_password }` |
| POST | `/api/iam/mfa/enroll` | 返回 secret + otpauth URI |
| POST | `/api/iam/mfa/verify` | 校验并绑定 |
| GET | `/.well-known/jwks.json` | 公钥集 |

### 5.1 登录失败的统一错误 —— 防止账号枚举

**账号不存在** 与 **密码错误** 必须返回**完全相同**的错误码与文案：

```json
{ "code": "INVALID_CREDENTIALS", "detail": "invalid username or password" }
```

**并且响应时间要接近** —— 账号不存在时若直接返回，会快得多 → 可做时序枚举。
做法：账号不存在时也**执行一次哈希校验**（对固定的假 hash），让耗时相当。

### 5.2 密码重置也要防枚举

`/api/iam/password/forgot` 无论邮箱是否存在，都返回**同样的成功响应**。

### 5.3 错误码（契约 C6）

| code | 含义 |
|---|---|
| `INVALID_CREDENTIALS` | 用户名或密码错误（**不区分**） |
| `MFA_REQUIRED` | 需要 MFA 验证码 |
| `MFA_INVALID` | MFA 验证码无效 |
| `ACCOUNT_LOCKED` | 账号被锁 |
| `ACCOUNT_DISABLED` | 账号已禁用 |
| `TOKEN_EXPIRED` | 令牌已过期 |
| `TOKEN_REVOKED` | 令牌已撤销 |
| `TOKEN_REUSE_DETECTED` | 检测到 refresh token 重放 → **整个 family 作废** |
| `PASSWORD_POLICY_VIOLATION` | 不满足密码策略 |
| `PASSWORD_REUSED` | 与历史密码重复 |

**面向用户的中文文案由前端 i18n 渲染**（契约 C6 已定，REQ-005 刚踩过这个坑 —— 后端 `detail` **必须英文**）。

---

## 6. 登录限流与账号锁定

| 项 | 策略 |
|---|---|
| 按 IP 限流 | Redis 滑动窗口，如 5 分钟 20 次 |
| 按账号限流 | Redis，失败 N 次（如 5 次）后**锁定** M 分钟（如 15 分钟） |
| 锁定存储 | Redis（快）+ DB（持久，便于审计与解封） |
| 计数器 | 成功登录后**清零** |

**限流与配额分开**（REQ-006 设计摘要已定）：限流防瞬时打爆（Redis 滑动窗口），
配额防长期超量（DB 累计计数）。**别混用**。

---

## 7. 设备会话

`user_session` 表记录：`user_id` / `tenant_id` / `device_label` / `ip` / `user_agent` /
`created_at` / `last_used_at` / `refresh_family_id` / `revoked_at`。

- **每个 session 一个 refresh family** → 踢下线 = 作废该 family
- 「登出全端失效」= 作废该用户全部 family
- 「改密码后踢出其他设备」= 作废除当前以外的全部 family

**租户维度**：会话必须带 `tenant_id`。用户在 A 租户的会话**不能**用于 B 租户 ——
这是 REQ-006 隔离原则在认证模块的延伸。

---

## 8. 依赖预算（安全模块例外）

| crate | 用途 | 代价 |
|---|---|---|
| `argon2` | 密码哈希 | ~10 crate |
| `jsonwebtoken` | JWT 签名/验签 | ~15-20 crate |
| `totp-rs` | TOTP | ~8 crate |

**这是 Sprint 2 唯一允许引入重量级依赖的地方** —— 这里的省依赖等于自造密码学。
**正确做法是引成熟库 + 记入依赖预算**，而不是自制。

本机内存够（4.6G 可用），但**要重新实测**冷编译峰值。

---

## 9. 本期交付边界

**做**

- [ ] 本地账号：注册（策略可配）、登录、登出、登出全端
- [ ] 密码：Argon2id + PHC 格式 + 可配置策略 + 泄露库检查（本期只做本地小词库）
- [ ] 密码找回与重置（防枚举）
- [ ] access/refresh 混合令牌 + **refresh 轮换 + 重放检测 + family 作废**
- [ ] JWT：签名密钥注入、`kid`、**JWKS 端点**、密钥轮换宽限期
- [ ] TOTP MFA：绑定、校验、**防重放**、恢复码
- [ ] WebAuthn **只留接口**
- [ ] 登录限流 + 账号锁定（Redis）+ 审计
- [ ] 设备会话：列表、踢下线、全端失效
- [ ] **全部会话与令牌带 tenant_id**
- [ ] 错误码统一（防账号枚举）

**不做**

- WebAuthn / FIDO2 实现
- 短信验证码
- 完整泄露密码库（HIBP k-anonymity）接入
- 企业目录同步（LDAP/AD 属 REQ-008）
- SSO 入站协议（SAML/OIDC/CAS 属 REQ-013）

---

## 10. 风险

| # | 风险 | 应对 |
|---|------|------|
| R1 | **refresh token 轮换没真的作废旧的** → 等于没轮换 | 契约测试断言：旧 refresh 立即失效；**重放检测**有专门测试 |
| R2 | **登录响应泄露账号是否存在**（错误码不同或耗时不同） | 统一错误码 + 账号不存在时也跑一次假哈希校验；测试断言两种情况响应体与耗时接近 |
| R3 | **JWT 密钥进 Git 或进 DB** | 只从环境注入；启动时校验「不是默认密钥」；私钥不入 JWKS |
| R4 | **JWT 无法撤销** 导致改密码后旧 access token 仍能用 | access TTL 15 分钟 + 敏感操作可查 session 状态；如需立即失效，走 session 检查 |
| R5 | TOTP 验证码被重放 | 记录最后成功的时间步，同一步不能复用 |
| R6 | 依赖树膨胀导致编译变慢 | 引成熟库是必须的，但要**实测**峰值，必要时调 `jobs` |
| R7 | 忘记给会话加 `tenant_id` → 跨租户会话 | 契约测试 + `platform-db` 的租户作用域入口强制 |
