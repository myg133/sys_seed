# SSO 配置与密钥保护、多 IdP 与租户隔离（AC-S-106、AC-S-109）

- **覆盖 AC**: AC-S-106（10 条：TC-013-071 ~ 080）、AC-S-109（9 条：TC-013-081 ~ 089）
- **前置基线**:
  - `BASE_SSO_OIDC_CLIENT_SECRET=SECRET-CANARY-7f3a9b1e-UNIQUE`
  - `tenant-a` OIDC + LDAP 全配；`tenant-b` 只配 OIDC（指向 `idp-b`，端口 9411）；
    `tenant-c` 未配任何 SSO
  - 三种令牌：平台管理员（`config:platform:read` + `config:entry:write`）、
    租户管理员（`config:entry:write`）、普通用户
- **AC-S-106 的四个步骤在本仓**不可照原样执行**（挑刺 S1/S2/S3），本文件把每步换成可执行等价式**：
  | AC 原文 | 实测问题 | 本文件的可执行形式 |
  |---|---|---|
  | 步骤 1「`GET /api/config/entries` 取**全量**配置」 | 该端点**必须**给 `keys` 查询参数，不给就返回 `{"entries":[]}`；且 `scope=platform` 的键在通用路由上直接 `403 INSUFFICIENT_PERMISSION` | 按**精确键名**读 11 个非敏感 SSO 键（TC-013-071） |
  | 步骤 2「`?scope=tenant`」 | 查询结构只有 `keys`（`crates/platform-config/src/http.rs:261-264`），`scope` 被静默忽略 | 用租户管理员令牌按**同一组键**再读一次（TC-013-072） |
  | 步骤 6「三张配置表 `SELECT` 全表文本扫描」 | 表名正确（`platform_config` / `platform_tenant_config` / `platform_space_config` 均存在），可执行 | 保留（TC-013-076） |
  | 步骤 7「往一个**测试用**配置键写入 canary」 | 未登记的键写入 → `404 KEY_NOT_REGISTERED`（D53）；没有「测试用键」 | 写入**已登记且无 `format` 约束**的键 `org` 模块 `ldapBaseDn`（TC-013-077） |

---

## AC-S-106：`client_secret` 三处扫描 0 命中

### TC-013-071: 平台管理员读 11 个非敏感 SSO 键 —— canary 0 命中

- **关联 AC**: AC-S-106
- **前置条件**: 平台管理员令牌；11 个 SSO 键均已登记
- **测试步骤**:
  1. `GET /api/config/entries?keys=sso.enforce,sso.jitProvisioning,sso.oidc.enabled,sso.oidc.issuer,sso.oidc.clientId,sso.oidc.scopes,sso.oidc.redirectUri,sso.ldap.enabled,sso.ldap.url,sso.ldap.baseDn,sso.ldap.userSearchFilter,sso.ldap.mailAttribute`
     （**12 个键**；`scope: tenant`，通用路由可读）
  2. 在响应体里搜 `SECRET-CANARY-7f3a9b1e-UNIQUE`
  3. 断言 `entries` 数组长度
- **预期结果**:
  1. 状态码 `200`
  2. `entries` 长度 `== 12`
  3. canary 命中数 `== 0`
  4. 每个条目的 `source` ∈ `{platform, tenant, space, default}`（`ConfigSource` 四值）
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 判定表

### TC-013-072: 租户管理员读同一组键 —— canary 0 命中，且看不到平台级写入以外的层

- **关联 AC**: AC-S-106
- **前置条件**: 租户管理员令牌（`x-tenant-id: tenant-a`）
- **测试步骤**:
  1. 用同一 `keys` 列表读一次
  2. 搜 canary
- **预期结果**:
  1. 状态码 `200`
  2. canary 命中数 `== 0`
  3. `source` 只能是 `tenant` 或 `default`（`tenant-a` 未在平台层覆盖这些键时）
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 判定表
- **备注**: 原 AC 的 `?scope=tenant` 参数**不存在**，会被静默忽略并返回空数组，
  用例照抄原样会「0 命中」但什么也没读到 —— 那是**假绿**。〔AC 修正 S2〕

### TC-013-073: 平台级路由读平台层 —— canary 0 命中

- **关联 AC**: AC-S-106
- **前置条件**: 平台管理员令牌
- **测试步骤**:
  1. `GET /api/config/platform-entries?keys=<12 个 SSO 键>`
  2. 搜 canary
- **预期结果**:
  1. 状态码 `200`，`entries` 长度 `== 12`
  2. canary 命中数 `== 0`
- **类型**: 安全
- **优先级**: P1
- **自动化**: 可自动化
- **设计方法**: 判定表
- **备注**: 若把 12 个键打到通用路由且其中有 `scope: platform` 的键，会得到
  `403 INSUFFICIENT_PERMISSION`（`crates/platform-config/src/error.rs:156,173`）——
  这不是权限问题而是「键的 scope 不允许走这条路由」，错误码语义误导。本用例绕开它。

### TC-013-074: 错误响应体里 canary 0 命中（含换 token 失败详情）

- **关联 AC**: AC-S-106
- **前置条件**: 有效 `state`；mock IdP 的 `/token` 返回
  `400 {"error":"invalid_client","error_description":"client_secret mismatch"}`
- **测试步骤**:
  1. 回调
  2. 把**完整**响应体（含 `detail` / `params`）序列化成字符串
  3. 搜 canary，以及 canary 的前 8 位 `SECRET-C`、后 8 位 `9b1e-UNIQ`（片段检查）
- **预期结果**:
  1. 状态码 `400`，`code == "SSO_OIDC_TOKEN_EXCHANGE_FAILED"`
  2. canary 全串命中数 `== 0`
  3. 三个片段各自命中数 `== 0`
  4. 响应体**不含** IdP 返回的 `error_description` 原文（含 `client_secret` 字样即算命中）
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 判定表

### TC-013-075: 成功 + 失败的 trace 级日志里 canary 0 命中

- **关联 AC**: AC-S-106
- **前置条件**: `BASE_LOG_LEVEL=trace`（**不设** `RUST_LOG`）；stdout 重定向到文件
- **测试步骤**:
  1. 跑一次成功 OIDC 登录
  2. 跑一次 `alg=none` 失败登录
  3. 在日志文件里搜 canary 全串与三个片段
- **预期结果**:
  1. 全串命中 `== 0`
  2. 片段命中 `== 0`
  3. 日志里**可以**出现 `client_id=base-portal`（非密钥）
  4. 阳性对照见 TC-013-078
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 判定表

### TC-013-076: `providers` 与 `sessions` 响应体 canary 0 命中

- **关联 AC**: AC-S-106
- **前置条件**: 已登录的 `bob`
- **测试步骤**:
  1. `GET /api/iam/sso/providers`（`x-tenant-id: tenant-a`）
  2. `GET /api/iam/sessions`（`x-tenant-id: tenant-a` + Bearer）
  3. 搜 canary
- **预期结果**:
  1. 两次均 `200`
  2. canary 命中数 `== 0`
  3. `providers` 响应不含 `issuer`、不含 `clientId` 的值（只暴露 `type/id/label/*Url`）
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 判定表

### TC-013-077: 三张配置表全表文本扫描 canary 0 命中

- **关联 AC**: AC-S-106
- **前置条件**: 管理员连接（绕过 RLS）
- **测试步骤**:
  1. `SELECT * FROM platform_config`
  2. `SELECT * FROM platform_tenant_config`
  3. `SELECT * FROM platform_space_config`
  4. 把三份结果整体 `serde_json::to_string` 后搜 canary
- **预期结果**:
  1. 三个查询均成功（表存在）
  2. canary 命中 `== 0`
  3. `SELECT count(*) FROM platform_tenant_config WHERE key LIKE 'sso.%' AND value::text LIKE '%SECRET%'` → `0`
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 判定表

### TC-013-078: 阳性对照（配置侧）—— 把 canary 写入已登记键，扫描器必须命中

- **关联 AC**: AC-S-106
- **前置条件**: 平台管理员令牌；`org` 模块的 `ldapBaseDn` 已登记为 `string` / `scope: platform`
  **且没有 `format` 约束**（`crates/platform-org/manifest.json` 的 `config` 段）
- **测试步骤**:
  1. `PUT /api/config/platform-entries`，body `{"entries":[{"key":"ldapBaseDn","value":"SECRET-CANARY-7f3a9b1e-UNIQUE"}]}`
  2. 用 **TC-013-071/072 的同一个读路径**读回该键
  3. 用 **TC-013-077 的同一个扫描器**扫 `platform_config`
  4. 清理（写回原值）
- **预期结果**:
  1. 步骤 1 状态码 `200`
  2. 步骤 2 读回的 `value` **逐字等于** canary
  3. 步骤 3 扫描器命中数 `>= 1`
  4. 步骤 4 后命中数回到 `0`
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 判定表
- **备注**: AC 原文要求「往一个**测试用**配置键里写入」——**没有这种键**：
  未登记的键写入返回 `404 KEY_NOT_REGISTERED`
  （`crates/platform-config/src/error.rs:172`，D53 的强制登记）。
  本用例改用「已登记 + 无 format」的既有键，这是唯一可执行的阳性对照。〔AC 修正 S3〕

### TC-013-079: 阳性对照（日志侧）—— 注入 canary 到日志，扫描器必须命中

- **关联 AC**: AC-S-106
- **前置条件**: 测试专用路由 `GET /__test/log-canary`（只在 `cfg(test)` 或测试 feature 下编译）
- **测试步骤**:
  1. 调用该路由（内部执行 `tracing::warn!("canary={}", "SECRET-CANARY-7f3a9b1e-UNIQUE")`）
  2. 用 TC-013-075 的同一个日志扫描器扫
- **预期结果**:
  1. 命中数 `>= 1`
  2. 该命中行的 `level` 为 `WARN`
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 判定表

### TC-013-080: 12 个键全部登记，且 `source` 能反映三层覆盖

- **关联 AC**: AC-S-106
- **前置条件**: 先在平台层给 `sso.oidc.clientId` 写 `platform-value`，
  再在 `tenant-a` 层写 `tenant-value`
- **测试步骤**:
  1. 平台管理员读 `sso.oidc.clientId`
  2. 租户管理员读同一键
  3. 读一个两层都没写的键（`sso.ldap.mailAttribute`）
- **预期结果**:
  1. 步骤 1 的 `source == "platform"`，`value == "platform-value"`
  2. 步骤 2 的 `source == "tenant"`，`value == "tenant-value"`
  3. 步骤 3 的 `source == "default"`，`value == "mail"`（manifest 默认值）
  4. 12 个键逐个 `ConfigRegistry::get(key).is_some()` 为真
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 判定表
- **备注**: `source` 是 `ResolvedConfig.source: ConfigSource`
  （`crates/platform-core/src/extension/config.rs:98-105`），四值
  `{platform, tenant, space, default}`。AC 只写「反映三层覆盖（平台 / 租户）」，
  漏了 `space` 与 `default` —— `scope: tenant` 意味着 space 层不可写（D53）。〔S40〕

---

## AC-S-109：多 IdP 并存与租户隔离

### TC-013-081: `providers` 按租户返回 2 / 1 / 0 个，且未配置返回 200 + 空数组

- **关联 AC**: AC-S-109
- **前置条件**: 三个租户按基线配置
- **测试步骤**:
  1. `GET /api/iam/sso/providers`，分别带 `x-tenant-id: tenant-a|tenant-b|tenant-c`
  2. 读 `providers` 数组长度与每项的 `type`
- **预期结果**:
  1. `tenant-a` → `200`，长度 `== 2`，`type` 集合 `== {oidc, ldap}`
  2. `tenant-b` → `200`，长度 `== 1`，`type == "oidc"`
  3. `tenant-c` → `200`，长度 `== 0`（**不是** `404`、**不是** `500`）
  4. 三次响应都含 `providers` 键（空时是 `[]`，不是 `null`、不是省略）
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 判定表

### TC-013-082: `tenant-a` 两条协议的登录各产生独立 family

- **关联 AC**: AC-S-109
- **前置条件**: `erin` 在目录里、`bob` 已绑定
- **测试步骤**:
  1. LDAP 登录（`erin`）
  2. OIDC 登录（`bob`）
  3. 查 `platform_iam_session` 中两条会话
- **预期结果**:
  1. 两次均 `200`
  2. 两条会话 `id` 不同、`family_id` 不同
  3. `auth_provider` 分别为 `"ldap"` 与 `"oidc"`
  4. 两条 `tenant_id` 都是 `"tenant-a"`
- **类型**: 功能
- **优先级**: P1
- **自动化**: 可自动化
- **设计方法**: 正交

### TC-013-083: `tenant-b` 的 `state` 到 `tenant-a` 回调 → 400 `SSO_OIDC_STATE_INVALID`

- **关联 AC**: AC-S-109
- **前置条件**: `tenant-b` 发起 authorize 拿到 `state_b`（`issuer` 指向 `idp-b`）
- **测试步骤**:
  1. 用 `state_b` + `x-tenant-id: tenant-a` 回调
  2. 读 `idp-b` 的 `token` 计数
- **预期结果**:
  1. 状态码 `400`，`code == "SSO_OIDC_STATE_INVALID"`
  2. `idp-b` 的 `token` 计数 `== 0`（未跨租户换 token）
  3. `tenant-a` 的 mock IdP `token` 计数 `== 0`
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 判定表

### TC-013-084: `tenant-c` 未配 OIDC → 400 `SSO_PROVIDER_NOT_CONFIGURED`

- **关联 AC**: AC-S-109
- **前置条件**: `tenant-c` 无任何 SSO 配置
- **测试步骤**:
  1. `GET /api/iam/sso/oidc/authorize`，`x-tenant-id: tenant-c`
- **预期结果**:
  1. 状态码 `400`，`code == "SSO_PROVIDER_NOT_CONFIGURED"`
  2. 响应**不含** `Location` 头
  3. `platform_iam_oidc_state` 新增 `0` 行
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 判定表

### TC-013-085: `tenant-c` 未配 LDAP → 与 TC-013-084 **同码同体**

- **关联 AC**: AC-S-109
- **前置条件**: 同上
- **测试步骤**:
  1. `POST /api/iam/sso/ldap/login`，`{"username":"x","password":"y"}`，`x-tenant-id: tenant-c`
- **预期结果**:
  1. 状态码 `400`，`code == "SSO_PROVIDER_NOT_CONFIGURED"`
  2. 响应体与 TC-013-084 的响应体 `code` 字段相同
  3. **不返回** `SSO_LDAP_UNREACHABLE`（未配置 ≠ 连不上）
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 判定表

### TC-013-086: `oidc.enabled=false` 与「未配置」**同一个码**（禁用即等于未配置）

- **关联 AC**: AC-S-109
- **前置条件**: `tenant-a` 的 `sso.oidc.enabled` 改为 `false`（`issuer` 等仍保留）
- **测试步骤**:
  1. `GET /api/iam/sso/oidc/authorize`
  2. 与 TC-013-084 的响应比对
- **预期结果**:
  1. 状态码 `400`，`code == "SSO_PROVIDER_NOT_CONFIGURED"`
  2. 两个响应的 `code` / `title` / `detail` **完全相同**
  3. **不出现**「已禁用」类字样（不暴露内部状态）
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 判定表
- **备注**: 这条与 AC-S-020「失败可区分、可排查」在**同一族端点**上给出相反要求：
  020 要求 discovery 的 503 与 502 分开（甚至告诉你能连但 JSON 非法），
  109 要求「禁用 / 未配置」合并。两套粒度不统一。〔S24〕

### TC-013-087: 改 `tenant-b` 的配置不影响 `tenant-a` 的登录

- **关联 AC**: AC-S-109
- **前置条件**: 基线
- **测试步骤**:
  1. `tenant-a` 走一次完整 OIDC 登录 → 期望 `200`
  2. 把 `tenant-b` 的 `sso.oidc.issuer` 改成 `http://127.0.0.1:9498/dead`（未监听）
  3. `tenant-a` 再走一次完整 OIDC 登录
- **预期结果**:
  1. 步骤 1 `200`
  2. 步骤 3 `200`（状态码与步骤 1 相同）
  3. `tenant-b` 此时登录会 `503 SSO_OIDC_DISCOVERY_FAILED`（对照，证明配置真的改到了）
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 正交

### TC-013-088: 两个租户指向不同 IdP 时 `sub` 不串

- **关联 AC**: AC-S-109
- **前置条件**: `idp-a` 与 `idp-b` 各自的 `sub` 空间不同：
  `idp-a` 给 `sub="same-sub-1"`，`idp-b` 也给 `sub="same-sub-1"`
- **测试步骤**:
  1. `tenant-a` 用 `sub="same-sub-1"` JIT 登录（`jitProvisioning=true`）
  2. `tenant-b` 用 `sub="same-sub-1"` JIT 登录
- **预期结果**:
  1. 两次均 `200`
  2. 两个 user 的 `id` 不同、`tenantId` 各自正确
  3. `external_identity` 两行：`(tenant-a, oidc, same-sub-1)` 与 `(tenant-b, oidc, same-sub-1)`
  4. 任一租户的 user 权限集不受另一租户影响（`me/permissions` 集合不相交时为 `0` 交叉）
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 正交

### TC-013-089: `providers` 不泄露配置细值（只暴露按钮所需字段）

- **关联 AC**: AC-S-109
- **前置条件**: `tenant-a` 配置完整
- **测试步骤**:
  1. `GET /api/iam/sso/providers`
  2. 取每个 provider 对象的键集合
- **预期结果**:
  1. `oidc` 项的键集合 `==` `{type, id, label, authorizeUrl}`
  2. `ldap` 项的键集合 `==` `{type, id, label, loginUrl}`
  3. 响应体**不含**键名 `issuer` / `clientId` / `url` / `baseDn` / `userSearchFilter`
  4. 响应体**不含** `SECRET-CANARY-7f3a9b1e-UNIQUE`
- **类型**: 安全
- **优先级**: P1
- **自动化**: 可自动化
- **设计方法**: 判定表
- **备注**: design §3.1 给了形状；本用例把它变成「键集合精确相等」，避免多塞字段。
