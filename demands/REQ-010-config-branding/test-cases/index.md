# REQ-010「配置、品牌与 Feature Flag」—— 测试用例总索引

- **需求编号**: REQ-010「配置、品牌与 Feature Flag」
- **父需求**: REQ-001（通用项目底座）｜**承接父 AC**: AC-013（配置三层覆盖）、AC-014（品牌，本期收窄，D52）、AC-015（Feature Flag 按租户生效）
- **验收基线**: [acceptance.md](../acceptance.md) —— **AC-S-049 ~ AC-S-063（15 条）**，本用例集**只**覆盖这一段
- **设计依据**: [design-summary.md](../design-summary.md)（**rev 2**，2026-11-18 23:15）
- **契约**: [api-conventions v2](../../../contracts/api-conventions/v2.md)（C3 / C6 / C9 / C10）、[extension-points v2](../../../contracts/extension-points/v2.md)（C-5）
- **格式基准**: [REQ-001 用例总索引](../../REQ-001-platform-foundation/test-cases/index.md)、[REQ-009 用例总索引](../../REQ-009-rbac/test-cases/index.md)
- **状态**: 待实现（用例本身**尚未执行**，不得据此判定需求通过）
- **版本**: **第 2 版（rev 2）** —— 对齐 BA 对 36 条问题的裁定（D55~D68）与重写的 acceptance / design

> ⚠️ **AC 条数**：`AC-S-049` ~ `AC-S-063` 是 **15 条**（049…063 连续）。acceptance.md 头部写的「17 条」是计数错误 —— 见 §8.9 N1。本用例集按 **15 条**统计。

---

## 1. 概览

| 项 | 值 |
|---|---|
| 用例总数 | **51**（TC-010-001 ~ TC-010-051，编号连续无跳号） |
| 覆盖 AC | **15 / 15**（AC-S-049 ~ AC-S-063，无遗漏） |
| 优先级分布 | P0 = **46**、P1 = **5**、P2 = 0 |
| 自动化分布 | 可自动化 = **49**、半自动 = **2**、手工 = 0 |
| 用例文件 | 4 个（另本索引 1 个） |
| 每条用例字段 | 标题 / 关联 AC / 前置条件 / 测试步骤 / 预期结果 / 类型 / 优先级 / 自动化 / 设计方法（**9 项全部齐备**） |

> **rev 2 的变化**：① 新增 **5 条**用例（TC-010-047 ~ 051：AC-S-063 读授权 4 条 + `SECURITY DEFINER` 写通道 1 条）；② 原有 **46 条全部按新 AC / 新错误码 / 新路由 / 新 body 形状修订过**；③ 新增 **AC-S-063** 的覆盖。
> **覆盖条数最少的 AC**：`AC-S-052`（2）、`AC-S-053`（2）、`AC-S-056`（2）、`AC-S-058`（2）、`AC-S-059`（2）。这 5 条各只描述一件事，但都配了「正常 + 反例/攻击路径」两路。
> **类型含「安全」的 13 条 AC**（050、051、052、053、054、056、057、058、059、060、061、062、063）**全部 ≥ 2 条**。

## 2. 文件清单

| 文件 | 能力域 | AC 范围 | 用例数 |
|---|---|---|---|
| [A-config-scope.md](A-config-scope.md) | 作用域、`source`（含 `default`）与类型校验 | AC-S-049 ~ AC-S-051 | 11（001 ~ 011） |
| [B-branding.md](B-branding.md) | 品牌公开接口（扁平 map）与前端接入 | AC-S-054、AC-S-055 | 8（012 ~ 019） |
| [C-feature-flags.md](C-feature-flags.md) | Feature Flag、派生键与确定性灰度 | AC-S-056 ~ AC-S-059 | 12（020 ~ 031） |
| [D-security-cache.md](D-security-cache.md) | 成对 epoch、写路由/DEFINER、C-5、三表隔离、读授权 | AC-S-052、AC-S-053、AC-S-060 ~ AC-S-063 | 20（032 ~ 046、047 ~ 051） |

---

## 3. 测试夹具（造数方式）

> **原则**：租户与空间在本仓**都没有实体表** —— `tenant_id` / `space_id` 都是字符串（已核实：全仓没有 `CREATE TABLE ... space`，也没有 `platform_tenant` 注册表）。夹具 id 直接写死为字符串，**不造影子表**。
> **`space_id` 在表里 NOT NULL**（design §2.0），空串不等于「没有空间」。
> **账号**由 REQ-007 的 IAM 提供（`platform_iam_user.id` 是 `TEXT`）；**角色 / 授权**由 REQ-009 的 RBAC 表提供。统一初始密码 `Passw0rd!`。
> **造数走管理员连接**（`base_test`），**业务访问走 `base_app`**（超管会绕过 RLS）。
> **可重置**：每条用例开始前把配置行与 flag 值恢复到 §3.5 / §3.6 的初始值（租户、空间、账号、角色固定不变）。**用例不得依赖执行顺序**。

### 3.1 租户

| ID | tenant_id | 用途 |
|---|---|---|
| T1 | `tenant-a` | 主测试租户（三层覆盖 / 写接口 / 越级写 / RLS / 读授权） |
| T2 | `tenant-b` | 跨租对照（同 `space_id` 字符串的跨租隔离、品牌按租户解析、缓存广播） |
| TX | `tenant-does-not-exist` | **不存在的租户 id**，品牌公开接口的探测对照 |

### 3.2 空间（**没有 space 表**，纯字符串；表列 `space_id NOT NULL`）

| ID | space_id | 说明 |
|---|---|---|
| S1 | `space-sales-1` | T1 的主空间；**T2 也使用同一字符串**（验证复合维度 `(tenant_id, space_id)`） |
| S2 | `space-legal` | T1 的第二个空间（局部性对照） |
| SX | `space-does-not-exist` | 不存在的空间 id（`X-Space-Id` 缺失/无效都不报错，契约 C3） |

### 3.3 账号与角色

| user_id | 账号 | 租户 | 绑定角色 | 用途 |
|---|---|---|---|---|
| `u-2001` | plain@example.com | T1 | `platform-user`（内置，**无** config 权限点） | **M**：普通用户；读/403 用例 |
| `u-2002` | cfgwriter@example.com | T1 | `cfg-entry-writer`（`config:entry:write` + `config:flag:write`） | **A**：租户级写成功、平台级写 403 |
| `u-2003` | cfgsuper@example.com | T1 | `cfg-platform-writer`（**4 个**：entry / flag / platform:write / **platform:read**） | **P**：平台级写 + 平台级读 |
| `u-2004` | cfgplatformonly@example.com | T1 | `cfg-platform-only`（**只有** `config:platform:write`） | **O**：权限正交性（写不蕴含读） |
| `u-2005` | t2plain@example.com | T2 | `cfg-entry-writer`（entry + flag） | **T2 的调用者** —— 读 T2 **必须**用它（用 T1 令牌 + `X-Tenant-Id: tenant-b` 会 404） |
| `u-2006` | cfgreader@example.com | T1 | `cfg-platform-reader`（**只有** `config:platform:read`） | **R**：读不蕴含写 |
| `u-3001` ~ `u-3200` | rollout-N@example.com | T1 | 无需角色 | 灰度统计的 **200 个不同用户** |
| `u-1001` | owner@example.com | T1 | `platform-admin`（注册表全集，动态得出，D31） | 造 RBAC 夹具 / 查库对照（不作被测调用者） |

**角色定义**（`platform_role` + `platform_role_permission`，`data_scope = SELF`）：

| 角色 code | 权限点 |
|---|---|
| `cfg-entry-writer` | `config:entry:write`、`config:flag:write` |
| `cfg-platform-writer` | `config:entry:write`、`config:flag:write`、`config:platform:write`、`config:platform:read` |
| `cfg-platform-only` | `config:platform:write` |
| `cfg-platform-reader` | `config:platform:read` |

### 3.4 配置键（注册表 = 各 manifest 的 `config` 段；**白名单枚举具体键**）

**品牌白名单（D66，11 个）**：`logo` / `favicon` / `title` / `loginBackground` / `footer` / `copyright` / `primaryColor` / `theme.radius` / `theme.mode` / `theme.density` / `theme.fontStack`。

| 键 | type | default | scope | 白名单 | 来源 manifest | 用途 |
|---|---|---|---|---|---|---|
| `logo` | string | `/branding/default.svg` | space | ✅ | `crates/platform-config/manifest.json`（新建） | 三层覆盖、越级写、品牌 |
| `title` | string | `示例平台` | space | ✅ | 同上 | `source=default`；`<title>`/`<h1>` |
| `primaryColor` | string | `#111111` | space | ✅ | 同上 | `source` 判别（值=默认、来源=platform）；AC-S-055 |
| `favicon` | string | `/favicon-default.svg` | space | ✅ | 同上 | AC-S-052/053（T1/T2 都无覆盖，能命中平台值） |
| `loginBackground` | string | `` | space | ✅ | 同上 | 白名单 |
| `footer` | string | `示例平台` | space | ✅ | 同上 | 白名单 |
| `copyright` | string | `© 2026` | space | ✅ | 同上 | 白名单 |
| `theme.radius` | number | `4` | space | ✅ | 同上 | number 类型 |
| `theme.mode` | string | `light` | space | ✅ | 同上 | 白名单 |
| `theme.density` | string | `comfortable` | space | ✅ | 同上 | 白名单 |
| `theme.fontStack` | array | `["system-ui"]` | space | ✅ | 同上 | array 类型 |
| `loginBanner` | string | `` | **space** | ❌（不在白名单） | 同上 | **D66 的判据**：登记 space 但非白名单 → 空间写 400 |
| `theme.tokens` | object | `{}` | tenant | ❌ | 同上 | object 类型（放 tenant 避免与 D66 混淆） |
| `pageSize` | number | `20` | **tenant** | ❌ | `modules/sandbox/manifest.json`（已有） | 类型校验（AC-S-051 指定它，避免越级顶包） |
| `beta` | boolean | `false` | tenant | ❌ | `crates/platform-config/manifest.json` | 灰度 flag 本体 |
| `newInvoice` | boolean | `false` | tenant | ❌ | 同上 | 租户隔离 flag 本体 |
| `cacheTtlSeconds` | number | `60` | **platform** | ❌ | `crates/platform-rbac/manifest.json`（已有） | 缓存 TTL；白名单外键 |
| `conflictStrategy` | string | `manual` | **platform** | ❌ | `crates/platform-org/manifest.json`（已有） | 越级写 400；读授权 403 |
| `scimBaseUrl` | string | `` | platform | ❌ | 同上 | 白名单外键；读授权 403 |
| `ldapBaseDn` | string | `` | platform | ❌ | 同上 | 白名单外键、读授权 |

**派生键（D57，不写进 manifest）**：对每个已登记 flag `F`，自动存在
`flags.<F>.rolloutPercent`（`type: number` / `scope: tenant` / `0..=100` / 默认 `100`）。
`flags.nosuch.rolloutPercent` 不是已声明键 → **400 `CONFIG_KEY_NOT_REGISTERED`**。

**未登记的键**：`notRegistered`。**非法键名**：`Theme.radius`、`theme-radius`。
**⚠️ 真实代码事实**：`ConfigSpec.scope` 的**缺省值是 `tenant`，不是 `platform`**（`code/crates/platform-core/src/extension/manifest.rs:213-215`）。design §3.2 已在 rev 2 改正 —— 品牌键**必须显式写** `"scope": "space"`。

### 3.5 已存配置值（三层，真实表）

**`platform_config`**（平台级；无 `tenant_id`、无 RLS）：

| key | value |
|---|---|
| `logo` | `/branding/platform-a.svg`（**PA**） |
| `favicon` | `/favicon-platform.svg`（**F**） |
| `primaryColor` | `#111111`（**显式**写入，与 manifest 默认同值） |
| `conflictStrategy` | `manual` |
| `scimBaseUrl` | `https://scim.example.com` |
| `cacheTtlSeconds` | `300`（TTL ≥ 300s） |
| `title` | **不写入** → 解析为 default `示例平台`，`source="default"` |

**`platform_tenant_config`**（租户级；`value` 从 `TEXT` 迁移到 `jsonb`）：

| tenant_id | key | value | 记号 |
|---|---|---|---|
| `tenant-a` | `logo` | `/branding/tenant-a.svg` | **PB** |
| `tenant-a` | `title` | `T1平台` | — |
| `tenant-a` | `newInvoice` | `true` | — |
| `tenant-a` | `pageSize` | `20` | 被用例改 |
| `tenant-b` | `logo` | `/branding/tenant-b.svg` | **PC** |

> **T2 没有 `newInvoice`**（AC-S-056：未配 → `enabled=false` + `source="default"`）。
> **T2 没有 `title`**（用于 TC-010-002 的 `source=default` 判别）。

**`platform_space_config`**（空间级；`(tenant_id, space_id, key)`，`space_id NOT NULL`）：

| tenant_id | space_id | key | value | 记号 |
|---|---|---|---|---|
| `tenant-a` | `space-sales-1` | `logo` | `/branding/space-s1.svg` | **PS** |
| `tenant-a` | `space-legal` | `logo` | `/branding/space-s2.svg` | **D** |
| `tenant-b` | `space-sales-1` | `logo` | `/branding/space-t2-s1.svg` | **E** |
| `tenant-a` | `space-sales-1` | `conflictStrategy` | `remote_wins` | **合成行**（管理员 SQL；见 §8.9 N5） |

> **A/B/PC/PS/D/E 六个值必须两两不同** —— 否则"命中层级"无法从值上判别。
> `conflictStrategy` 是 `scope: platform` 的键，**无法**通过 API 写进空间级（会 400）；这一行只能由管理员 SQL 直接插入，用来验证品牌接口**永不读取**空间级的非白名单键。

### 3.6 Feature Flag

| flag | 登记 | T1 | T2 | 用途 |
|---|---|---|---|---|
| `newInvoice` | `type: boolean`、`scope: tenant` | `true` | **无配置** | AC-S-056 租户隔离 + `source=default` |
| `beta` | `type: boolean`、`scope: tenant` | 无（`false`） | 无 | AC-S-057 / 058 灰度 |
| `newinvoce` | **未登记** | — | — | AC-S-059 拼错名 |
| `new-invoice` | **未登记**（kebab） | — | — | AC-S-059 不归一化 |
| `bad\nINJECTED` / 1KB 名 | **非法名** | — | — | AC-S-059 `FLAG_NAME_INVALID` |

- **灰度比例**：`flags.beta.rolloutPercent`（派生键，tenant，默认 100），由用例逐次写。
- **200 个灰度用户**：`u-3001` ~ `u-3200`（T1），`user_id` 直接作为哈希输入。

### 3.7 权限点（`platform_permission` + `capabilities/permissions.json`）

| 权限点 | 来源 manifest | 状态 |
|---|---|---|
| `config:entry:write` | `crates/platform-config/manifest.json`（**新建**） | 新增 |
| `config:flag:write` | 同上 | 新增 |
| `config:platform:write` | 同上 | 新增 |
| `config:platform:read` | 同上 | 新增（AC-S-063） |
| 既有 18 个（sandbox 5 / org 9 / rbac 4） | 各自 manifest | **已存在** |

- **不写死权限点总数**：`platform-admin` 的权限集合 = 注册表当前全集、动态得出（D31）。
- **三件套**（D67）：manifest 声明 + **权限点种子迁移** + 重生成 `capabilities/permissions.json`。少了种子迁移，启动对账 fail-fast，**进程起不来**（`code/crates/platform-server/src/main.rs:424-442`）。

### 3.8 表、授权、写通道与连接

| 表 | 关键列 / 主键 | RLS | 状态 |
|---|---|---|---|
| `platform_config` | `key`(PK) / `value jsonb` / `updated_at` | **无** | 新建 |
| `platform_tenant_config` | `(tenant_id, key)` PK / `value` | **有**（REQ-006） | 已存在；`value` 从 `TEXT` 迁移 |
| `platform_space_config` | `(tenant_id, space_id, key)` PK / `space_id NOT NULL` / `value jsonb` | **有**（新建） | 新建 |
| `platform_config_epoch` | `scope_key`(PK) / `epoch` / `updated_at` | 无 | 新建 |

- **平台级写通道**（D56）：迁移里 `REVOKE INSERT, UPDATE, DELETE ON platform_config FROM base_platform_app`（**保留 `SELECT`**）+ `platform_config_upsert(TEXT, JSONB)` 的 `SECURITY DEFINER` 函数（`GRANT EXECUTE` 给 `base_platform_app`）。
  ⚠️ `deploy/seed/02-app-role.sql:50,54-55` 的 `GRANT ALL TABLES` + `ALTER DEFAULT PRIVILEGES` 会让新表**默认可写** —— 不显式 `REVOKE` 就等着 AC-S-062 失败。
- **连接**：应用 `base_app`（`base_platform_app` 组成员，`app_role_url()` 构造）；造数用管理员连接。
- **租户上下文**：RLS 读 `current_setting('app.tenant_id', true)`（`code/crates/platform-db/src/rls.rs:141-150`）。
- **伪租户**：平台级表访问沿用 `"platform"` 伪租户 + `{tenant} IS NOT NULL`（design §2.3）；写入**不能**只靠这个条件，靠表级权限 + 函数。

### 3.9 日志捕获

AC-S-059 要求"告警日志内容可断言"。用 `tracing` 订阅器 + 内存 `BufWriter`，断言 **level = WARN** 与**结构化字段** `flag`（不是匹配整行文本）。

---

## 4. 命名与接口约定（rev 2 的实际取值）

| 约定项 | 取值 | 来源 |
|---|---|---|
| 平台 API 前缀 | `/api/config`、`/api/feature-flags` | 契约 C1 |
| 路由（**7 条**） | `GET /api/config/entries?keys=a,b`、`GET /api/config/branding`、`PUT /api/config/entries`、**`PUT /api/config/platform-entries`**、`GET /api/feature-flags`、`GET /api/feature-flags/{flag}`、`PUT /api/feature-flags/{flag}` | design §6（**路径照此表，不自创**） |
| 鉴权 | `entries` / `feature-flags` **需要令牌**；**平台级键读另需 `config:platform:read`**；`branding` **公开**；写接口见下 | design §6 + AC-S-063 |
| 写权限点 | `entries` → `config:entry:write`；`platform-entries` → `config:platform:write`；`feature-flags/{flag}` → `config:flag:write` | AC-S-060 + D59 |
| 配置读响应 | `{"entries":[{"key","value","source"}]}`，按请求顺序、重复键去重 | §7 A6（design 未重定义） |
| 品牌响应 | **扁平 map** `{"logo": "...", ...}`，无 `source`、无包装 | D61 |
| flag 响应 | `{"flag","enabled","source"}`；批量 `{"flags":[...]}` | 契约 C10 |
| 写 body | `{"scope":"tenant|space|platform","entries":[{"key","value"}, ...]}`；`scope` 必须与路由匹配 | BA 裁定 A2（rev 2） |
| `source` 取值 | `platform` / `tenant` / `space` / `default`（**四个**）；404 没有 `source` | C9 v2 §9.3 + AC-S-049 |
| 可写作用域 | `platform` / `tenant` / `space`（**三值**，`default` 不可写）；`ConfigSource` 与 `ConfigScope` 是两个枚举 | D60 |
| 写校验顺序 | 认证 401 → 权限点 403 → 键名格式 400 → 注册表 400 → 作用域 400 → 类型/范围 400 | D64 |
| 400 错误码（**6 个**） | `CONFIG_KEY_INVALID` / `CONFIG_KEY_NOT_REGISTERED`（写 400、读 404）/ `CONFIG_SCOPE_NOT_ALLOWED` / `CONFIG_TYPE_MISMATCH` / `CONFIG_VALUE_OUT_OF_RANGE` / `FLAG_NAME_INVALID` | D62 |
| 鉴权错误码 | 401 `UNAUTHENTICATED` / 403 `INSUFFICIENT_PERMISSION` / 404 `NOT_FOUND`（跨租头） | 契约 C6 + D26 + `problem.rs:33-39` |
| 上下文头 | `X-Tenant-Id`（与令牌不一致 → 404）、`X-Space-Id`（缺失 = 不限空间） | 契约 C3 |
| 键名 | camelCase，层级 `.` 分隔；分段 `^[a-z][a-z0-9]*$` | 契约 C9 + `config.rs:90-100` |
| flag 名 | `^[a-z][a-zA-Z0-9]{0,63}$`，**精确匹配不归一化**；哈希输入用登记名原文 | D65 |
| 派生键 | `flags.<F>.rolloutPercent`（number / tenant / 0..=100 / 默认 100） | D57 |
| 品牌白名单 | **枚举 11 个具体键**（无 `theme.*` 通配） | D66 |
| 空间级写 | `scope: space` **且** 在白名单内，否则 400 | D66 |
| epoch | 缓存存 `(platform_epoch, tenant_epoch)`，**成对比较** | D55 |
| 表名 | `platform_config` / `platform_tenant_config` / `platform_space_config` / `platform_config_epoch` | design §2.0 |
| 跑法 | `cargo test --workspace -- --test-threads=2`（**只能一个 `--`**）；`DATABASE_URL_TEST=postgres://base:base@127.0.0.1:5432/base_test`；库名必须恰好 `base_test`；`CARGO_TARGET_DIR=…/code/target` | design §10 |

---

## 5. AC → TC 覆盖矩阵（15 / 15）

| AC | 标题 | 覆盖 TC | 条数 | 外部依赖 | 最高优先级 |
|---|---|---|---|---|---|
| AC-S-049 | 三层覆盖与 `source` 如实反映命中层级 | TC-010-001、TC-010-002、TC-010-003、TC-010-004 | 4 | 真实库 + 真机 HTTP | P0 |
| AC-S-050 | 作用域越级写入被拒（含 D66 白名单门） | TC-010-005、TC-010-006、TC-010-007 | 3 | 真实库 + 真机 HTTP | P0 |
| AC-S-051 | 未声明的键不得写入，且按登记类型校验 | TC-010-008、TC-010-009、TC-010-010、TC-010-011 | 4 | 真实库 + 真机 HTTP | P0 |
| AC-S-052 | 配置变更在**下一个请求**生效 | TC-010-032、TC-010-033 | 2 | 真实库 + 两个独立实例 | P0 |
| AC-S-053 | 平台级变更使**所有租户**失效（含掩蔽反例） | TC-010-034、TC-010-035 | 2 | 真实库 + 两个独立实例 | P0 |
| AC-S-054 | 品牌公开接口白名单 fail closed（扁平 map） | TC-010-012、TC-010-013、TC-010-014、TC-010-015、TC-010-016 | 5 | 真实库 + 真机 HTTP（无令牌） | P0 |
| AC-S-055 | 前端无硬编码品牌（含同源与测试同步） | TC-010-017、TC-010-018、TC-010-019 | 3 | 静态扫描 + 浏览器 + 同源部署 | P0 |
| AC-S-056 | Feature Flag 按租户隔离（`source=default`） | TC-010-020、TC-010-031 | 2 | 真实库 + 真机 HTTP | P0 |
| AC-S-057 | 灰度确定性、单调性与边界（派生键） | TC-010-023、TC-010-024、TC-010-025、TC-010-026、TC-010-027、TC-010-028 | 6 | 真实库 + 单元层 + 两个进程 | P0 |
| AC-S-058 | 无用户身份时灰度 fail closed | TC-010-021、TC-010-022 | 2 | 真实库 + 单元层 | P0 |
| AC-S-059 | 未声明 flag 不 404；flag 名受约束 | TC-010-029、TC-010-030 | 2 | 真实库 + 日志捕获 | P0 |
| AC-S-060 | 配置与开关的写接口必须鉴权（独立路由 + 401/404） | TC-010-036、TC-010-037、TC-010-038、TC-010-039、**TC-010-040** | 5 | 真实库 + 真机 HTTP + 三档账号 | P0 |
| AC-S-061 | 4 个新权限点各有调用点；既有缺口不扩围 | TC-010-040、TC-010-041、TC-010-042 | 3 | 静态扫描 + 新的 C-5 入口 | P0 |
| AC-S-062 | 三表分离 + REVOKE 直接 DML + `jsonb` 可逆 | TC-010-043、TC-010-044、TC-010-045、TC-010-046、TC-010-051 | 5 | 真实库 + `base_app` + `sqlx migrate` | P0 |
| AC-S-063 | 平台级配置的**读**也要授权 | TC-010-047、TC-010-048、TC-010-049、TC-010-050 | 4 | 真实库 + 真机 HTTP + 四档账号 | P0 |

**合计 15 条 AC、51 条 TC**（TC-010-040 同时覆盖 AC-S-060 与 AC-S-061，矩阵内重复列出一次；按主 AC 去重后 51 条）。**无遗漏 AC、无悬空 TC。**
> TC-010-011 的第 2 步同时验证 AC-S-050 的作用域判定参与排序，但按主 AC 记为 AC-S-051，不重复计数；TC-010-050 的第 1/4 步对照 AC-S-054，但按主 AC 记为 AC-S-063。

---

## 6. 当前无法立即执行的 AC

### 6.1 判据本身只能在单元层/特定路径验证

| AC | 缺失项 | 影响 TC | 处理 |
|---|---|---|---|
| AC-S-049 步骤 1 | 「平台上下文（无租户）」在已认证 HTTP 路径不存在（租户由令牌推断） | TC-010-003 | 单元层断言 `source=platform`；HTTP 层断言 401（AC 已明写） |
| AC-S-052 / 053 | 真多实例不可用；且 AC-S-052 的 `logo` 被 AC-S-054 的租户覆盖挡住 | TC-010-032 ~ 035 | 用两个独立实例模拟跨进程；键改用 `favicon`（§8.9 N4） |
| AC-S-057 | 真多进程需要起两个进程 | TC-010-028 | 用两个独立进程（`std::process::Command` 或两次独立 `cargo test`，共享 `base_test`） |
| AC-S-058 | HTTP 层拿不到「无身份 → 200」 | TC-010-021 | 单元层判定 + HTTP 401（AC 已明写） |
| AC-S-063 混合批量 | AC 未定义「平台级键 + 租户级键同批」的行为 | TC-010-049 | 按 fail closed 整体 403 写（§8.9 N7） |

### 6.2 需要额外基础设施 / 前置修复

| AC | 缺失项 | 影响 TC | 处理 |
|---|---|---|---|
| AC-S-055 步骤 2~4 | 前端构建 + **同源部署**（服务端无 CORS） | TC-010-018, 019 | 把 `apps/shell/dist` 挂到 `platform-server` 静态目录（`BASE_STATIC_DIR`），同源访问 |
| AC-S-060 / 061 | 平台 crate 的 **C-5 入口**（不依赖 `ContractFixture`） | TC-010-040 ~ 042 | 需新增 `check_c5_for` 或等效入口（D58） |
| AC-S-062 步骤 2/3 | `REVOKE` + `SECURITY DEFINER` 函数 | TC-010-043, 051 | 迁移必须先做 `REVOKE` + 建函数，否则用例必失败 |
| AC-S-057 / 063 | 4 个权限点 + 种子迁移 + capabilities 重生成 | TC-010-036 ~ 051 | 三件套（D67），否则服务起不来 |
| AC-S-055 | `e2e/shell.spec.ts` 与 `App.svelte` 同步改 | TC-010-017, 018 | 交付清单已列（D/R18） |

### 6.3 可无外部依赖执行的部分

- **静态/单元入口**（不连库）：TC-010-003 的单元部分、TC-010-004 的键名断言、TC-010-017、TC-010-041、TC-010-042。
- **纯 DB 断言**：TC-010-043 ~ 046、TC-010-051 的第 1~3 步。
- **其余**需要真机/进程内 HTTP；TC-010-018 / 019 需要浏览器。

---

## 7. 已裁定约定（BA 2026-11-18 23:10 拍板，不再是待确认）

> rev 1 登记的 14 条「待确认约定」逐条裁定如下（原编号 A1~A14）。用例已按裁定改写。

| # | 事项 | BA 裁定 | 决策 / 依据 | 用例落点 |
|---|---|---|---|---|
| 1 | 租户/空间为纯字符串 | ✅ 确认；`space_id` 在表里 **NOT NULL** | design §2.0 | §3.1 / §3.2；全部 DB 用例 |
| 2 | 写 body 与响应形状 | 写 body 采纳推导：`{scope, entries:[{key,value}]}`；**平台级写走独立路由** | D59 | TC-010-005/006/036 等 |
| 3 | 400 错误码 | ✅ 采纳并扩到 **6 个** | D62 | 全部 400 断言 |
| 4 | flag 复合存储 | ❌ **改为派生键** `flags.<F>.rolloutPercent`；flag 本体仍是 boolean | D57 | TC-010-021/023/027/031 |
| 5 | 品牌接口扁平 map | ✅ 采纳为**正式形状** | D61 | TC-010-012 ~ 016 |
| 6 | 校验顺序 | ✅ 采纳，并**写进 AC**：认证 → 权限点 → 键名 → 注册表 → 作用域 → 类型 | D64 | TC-010-011 |
| 7 | 缓存 TTL = 300 | ✅ 采纳 | AC-S-052 | TC-010-032 |
| 8 | 未知租户回落平台默认 | ✅ 采纳（无租户注册表，200 回落） | design §6.1 | TC-010-014 |
| 9 | `theme.*` 按前缀匹配 | ❌ **改为枚举具体键**（11 个） | D66 | TC-010-013/017/018 |
| 10 | 未声明 flag 的 `source` | ✅ 定为 **`default`** | AC-S-056 / §5.3 | TC-010-020/029 |
| 11 | `<h1>底座壳</h1>` 算品牌值 | ✅ **算**；E2E 与 `App.svelte` 同步改 | AC-S-055 | TC-010-017/018 |
| 12 | 品牌接口 CORS | ❌ 不补 CORS；E2E 走**同源** | R17 | TC-010-018 |
| 13 | `entries` 是否键级授权 | ✅ **是**：平台级键需 `config:platform:read` → 新增 AC-S-063 | AC-S-063 | TC-010-047 ~ 050 |
| 14 | 其余 | 以 rev 1 登记为准 | — | — |

**rev 2 新增/改写的约定（design 已写死）**：

| # | 事项 | 取值 | 决策 |
|---|---|---|---|
| 15 | `source` 的枚举 | 独立 `ConfigSource`（4 值）；`ConfigScope` 保持 3 值 | D60 |
| 16 | 平台级写通道 | `REVOKE INSERT/UPDATE/DELETE` + `platform_config_upsert`（`SECURITY DEFINER`） | D56 |
| 17 | 新增权限点 | manifest + **种子迁移** + 重生成 capabilities（三件套） | D67 |
| 18 | 平台 crate 的 C-5 | 新增不依赖 `ContractFixture` 的扫描入口 | D58 |
| 19 | flag 名 | 精确匹配、不归一化；非法名 400 不进日志 | D65 |
| 20 | 平台级读 | 平台级键需 `config:platform:read`（新增 AC-S-063） | AC-S-063 |

---

## 8. 验收标准修订记录（36 条问题 → BA 全部接受）

> 本节保留 rev 1 问题清单的证据链与逐条裁定结果。**36 条全部成立、无一条被驳回**，其中 **7 条是 BA 自己的设计错误**（D68）。
> 证据里的 `acceptance.md:xx` 行号指 **rev 1** 的文本（已被重写）；**代码证据**（`code/...:行号`）仍然有效。
> 「落点」= 本用例集里承载该修订的用例。

### 8.1 硬伤（rev 1 的 12 条）

#### R-01（原 Q-01）: 平台表「租户写不进」与「运行时写得进」互斥
- **原问题**: AC-S-062 要求租户连接写不进 `platform_config`，AC-S-060 要求 P 运行时写得进；本仓只有一个 `base_app` 池、没有平台管理角色。
- **证据**: `code/crates/platform-server/src/main.rs:103-108`（只有一个 `pool`）；`code/deploy/seed/02-app-role.sql:20,34`（只有 `base_platform_app` / `base_app`）。
- **BA 裁定**: ✅ 接受，**D56** —— `REVOKE INSERT,UPDATE,DELETE` + `SECURITY DEFINER` 函数作为唯一写通道；数据库只保证「没有意外的写路径」。
- **改成什么**: AC-S-062 步骤 2 验证三种直接 DML 全拒、步骤 3 验证函数能写；design §2.1.1 给出函数 DDL。
- **落点**: TC-010-036 / 043 / 051。

#### R-02（原 Q-02）: `SELECT max(epoch)` 被更高的租户 epoch 掩蔽
- **原问题**: 两行各自自增，平台写入只让 `platform` +1，`max` 不变 → 该租户缓存不失效。
- **证据**: rev 1 design §7.1 的读路径；`epoch` 写路径各自 +1。
- **BA 裁定**: ✅ 接受，**D55** —— 缓存存 `(platform_epoch, tenant_epoch)`，**成对比较**，任一不等即重算。
- **改成什么**: AC-S-053 新增步骤 3（构造掩蔽顺序）；design §7.1 用反例说明并改正。
- **落点**: TC-010-035（反例门禁）。

#### R-03（原 Q-03）: `rolloutPercent` 无处安放
- **原问题**: flag 登记为 `type: boolean`、`ConfigSpec` 无 rollout 字段、另立键会被「未声明 → 400」拒。
- **证据**: `code/crates/platform-core/src/extension/manifest.rs:147-162`；rev 1 design §5.3；rev 1 AC-S-051 / 057。
- **BA 裁定**: ✅ 接受，**D57** —— 派生键 `flags.<F>.rolloutPercent`（number / tenant / 0..=100 / 默认 100）。
- **改成什么**: AC-S-057 前置改用派生键；design §5.3 写死判定顺序与派生规则。
- **落点**: TC-010-023 / 027 / 031。

#### R-04（原 Q-04）: AC-S-061 的验证手段不存在，且基线事实错误
- **原问题**: `ContractFixture` 要求 `Repository` + `PlatformModule`，平台 crate 没有；`platform-org` 9 个已登记权限点的调用点是 **0**。
- **证据**: `code/crates/platform-contracts/src/fixture.rs:31-67`；`code/crates/platform-org/src/http.rs:295-314`；`code/crates/platform-rbac/src/http.rs:419-420`。
- **BA 裁定**: ✅ 接受，**D58** —— 新增不依赖 fixture 的扫描入口；既有缺口登记为技术债、**不扩围**；「不得恶化」= 基线快照 + **差值 ≥ 0**。
- **改成什么**: AC-S-061 步骤 1/4 改写；验收「明确不做」新增「把 C-5 扩到平台 crate」。
- **落点**: TC-010-042（基线快照）。

#### R-05（原 Q-05）: 单路由 + 静态 `route_layer` 无法同时满足 060/061
- **原问题**: 把 `config:platform:write` 挂到共用路由 → A 连租户写都 403；改成 handler 判定 → 该权限点无 `require_permission` 调用点。
- **证据**: `code/crates/platform-core/src/extension/permission.rs:219-249`。
- **BA 裁定**: ✅ 接受，**D59** —— 平台级写拆成独立路由 `PUT /api/config/platform-entries`。
- **改成什么**: AC-S-060 步骤 4/5 明确走独立路由；design §6.0 说明原因。
- **落点**: TC-010-036 / 040。

#### R-06（原 Q-06）: 「五种 `source`」实为四种
- **原问题**: 步骤 1~5 只产出 platform/tenant/space/default 四个；第 5 步是 404，没有 `source`。
- **证据**: rev 1 AC-S-049；C9 v2 §9.3。
- **BA 裁定**: ✅ 接受 —— 改为「**4 个取值**」，并写明「404 不带 `source`」。
- **落点**: TC-010-001 / 002。

#### R-07（原 Q-07）: 「无租户上下文 / 无身份」在 HTTP 层不可达
- **原问题**: 读接口需要令牌，令牌必带租户；`require_permission` 缺 tenant 直接 401。
- **证据**: `code/crates/platform-rbac/src/http.rs:187-193`；`code/crates/platform-core/src/extension/permission.rs:319-323`；rev 1 design §6 与 §5.2 自相矛盾。
- **BA 裁定**: ✅ 接受 —— AC-S-049 步骤 1 改「单元层验证 + HTTP 断言 401」；AC-S-058 明写单元层 + HTTP 401；AC-S-056 步骤 3 指向 058。
- **落点**: TC-010-003 / 020 / 021。

#### R-08（原 Q-08）: AC-S-053 与 AC-S-049 的夹具互斥
- **原问题**: T1 的 `logo` 有租户覆盖 B，永远读不到平台 A。
- **BA 裁定**: ✅ 接受 —— AC-S-053 改用 **`favicon`**（T1 无覆盖）。
- **落点**: TC-010-034 / 035。

#### R-09（原 Q-09）: AC-S-054 步骤 3 缺 `X-Tenant-Id`
- **原问题**: 只带 `X-Space-Id` 时没有租户上下文，忽略空间与忽略租户无法区分。
- **BA 裁定**: ✅ 接受 —— 步骤 3 补 `X-Tenant-Id: T1`，并显式断言 `logo == PB ≠ PS`。
- **落点**: TC-010-015。

#### R-10（原 Q-10）: AC-S-054 步骤 1 只验键集合不验值
- **原问题**: 平台默认品牌同样"只含白名单键"，不做租户解析也能过。
- **BA 裁定**: ✅ 接受 —— 加值断言（`logo` 必须等于该租户的值），并新增 T2 对照。
- **落点**: TC-010-012 / 016。

#### R-11（原 Q-11）: 品牌接口响应形状未定义
- **BA 裁定**: ✅ 接受，**D61** —— 扁平 map，无 `source`、无包装。
- **落点**: TC-010-012 ~ 016。

#### R-12（原 Q-12）: `source` 与 `scope` 共用枚举
- **原问题**: 给 `ConfigScope` 加 `Default` 会让 manifest 能写 `"scope":"default"`，开出第 4 个可写层。
- **证据**: `code/crates/platform-core/src/config.rs:65-71,117-123`；`manifest.rs:157-158`。
- **BA 裁定**: ✅ 接受，**D60** —— 新增独立 `ConfigSource`，`ConfigScope` 保持三值。
- **落点**: A 域 §A.0；TC-010-001 / 002。

### 8.2 判据不成立 / 缺可判定判据（rev 1 的 23 条）

#### R-13（原 Q-13）: design 说 `scope` 默认 `platform`，代码是 `tenant`
- **证据**: `code/crates/platform-core/src/extension/manifest.rs:213-215`；`manifest_schema.rs:71`。
- **裁定**: ✅ 接受 —— design §3.2 已改正，并写明「品牌键必须显式写 `scope: space`」。
- **落点**: §3.4；TC-010-006。

#### R-14（原 Q-14）: AC-S-051 未指定写入作用域，类型校验被越级写顶包
- **裁定**: ✅ 接受 —— 改用 **`pageSize`（scope: tenant）**，全部写在租户级。
- **落点**: TC-010-009 / 010 / 011。

#### R-15（原 Q-15）: 校验顺序未定义
- **裁定**: ✅ 接受，**D64** —— 顺序写进 AC 与 design §3.3。
- **落点**: TC-010-011。

#### R-16（原 Q-16）: 400 的稳定错误码未定义
- **裁定**: ✅ 接受，**D62** —— 6 个码（`CONFIG_KEY_INVALID` / `CONFIG_KEY_NOT_REGISTERED` / `CONFIG_SCOPE_NOT_ALLOWED` / `CONFIG_TYPE_MISMATCH` / `CONFIG_VALUE_OUT_OF_RANGE` / `FLAG_NAME_INVALID`）。
- **落点**: 全部 400 断言；§4。

#### R-17（原 Q-17）: 「101 / -1 → 400」没说是写时还是读时
- **裁定**: ✅ 接受 —— AC-S-057 步骤 6 明写 **400 在 `PUT`**；读路径脏数据 → `enabled=false` + WARN。
- **落点**: TC-010-027。

#### R-18（原 Q-18）: 命中率区间无数值、集合"记录"不是判据、单调性单向
- **裁定**: ✅ 接受 —— **[70,130]** 写死；`S50' == S50`；补反向单调。
- **落点**: TC-010-024 / 025。

#### R-19（原 Q-19）: flag 的 `source` 在 design §5.1 与 C9 §9.3 之间矛盾
- **裁定**: ✅ 接受 —— AC-S-056 定为 `source="default"`（未配即默认值）；§5.3 同口径。
- **落点**: TC-010-020 / 029。
- **⚠️ 遗留**: design §5.1 表格**仍写 `source="platform"`**（§8.9 N8）。

#### R-20（原 Q-20）: flag 名规范化未定，同 flag 两种写法 bucket 不同
- **裁定**: ✅ 接受，**D65** —— `^[a-z][a-zA-Z0-9]{0,63}$`，精确匹配不归一化，哈希用登记名原文。
- **落点**: TC-010-030。

#### R-21（原 Q-21）: 「`source="tenant"`（若配在租户级）」是条件式预期
- **裁定**: ✅ 接受 —— 删掉"若"，前置写死配在租户级。
- **落点**: TC-010-020。

#### R-22（原 Q-22）: flag 名无字符集/长度边界 → 日志注入
- **裁定**: ✅ 接受 —— 非法名 **400 `FLAG_NAME_INVALID`**，**不进日志**；合法未声明名才 200 + WARN。
- **落点**: TC-010-030。

#### R-23（原 Q-23）: 白名单 `theme.*` 是通配
- **裁定**: ✅ 接受，**D66** —— 枚举 11 个具体键；并有「白名单 ⊆ `scope: space` 登记键」的不变式。
- **落点**: TC-010-013 / 017 / 018。

#### R-24（原 Q-24）: AC-S-060 缺未认证（401）与跨租头（404）两类攻击路径
- **裁定**: ✅ 接受 —— 新增步骤 6（401）与步骤 7（404）。
- **落点**: TC-010-037 / 038。

#### R-25（原 Q-25）: AC-S-055 扫描范围漏掉真实硬编码位置
- **裁定**: ✅ **部分接受** —— 扫描范围收到 `apps/shell/**`；`packages/theme` 的 `#1677ff` 允许作**兜底默认**，改为从**渲染结果**把关（步骤 4：渲染色 ≠ `#1677ff`）。
- **落点**: TC-010-017 / 018。

#### R-26（原 Q-26）: 未提「去硬编码会让现有测试变红」
- **裁定**: ✅ 接受 —— AC-S-055 要求同步更新 `e2e/shell.spec.ts` 与 `App.svelte`；不允许「留硬编码过 AC」或「改了让测试红」。
- **落点**: TC-010-017 / 018。

#### R-27（原 Q-27）: 没有 CORS，E2E 跨源跑不通
- **裁定**: ✅ 接受 —— **不补 CORS**；E2E 走 `platform-server` 静态目录的**同源**访问（R17）。
- **落点**: TC-010-018。

#### R-28（原 Q-28）: 「只授予平台管理角色」——该角色不存在；默认授权已开写权
- **证据**: `code/deploy/seed/02-app-role.sql:49-55`（`GRANT ALL TABLES` + `ALTER DEFAULT PRIVILEGES`）。
- **裁定**: ✅ 接受，**D56** —— 迁移里显式 `REVOKE INSERT/UPDATE/DELETE`；AC-S-062 步骤 2 覆盖**三种 DML**（不只 INSERT）。
- **落点**: TC-010-043 / 051。

#### R-29（原 Q-29）: 「down 可逆」在空表上是空断言
- **裁定**: ✅ 接受 —— AC-S-062 步骤 6：**先插 3 种 JSON 类型值**再 up/down/up；down **不得 `DROP TABLE`**。
- **落点**: TC-010-046。

#### R-30（原 Q-30）: 三张新表 DDL 未定义
- **裁定**: ✅ 接受 —— design §2.0 给出 DDL（PK 写死、`space_id NOT NULL`）。
- **落点**: §3.8；TC-010-044 / 046。

#### R-31（原 Q-31）: 契约基线写 v1，实际已 v2
- **裁定**: ✅ 接受，**D54** —— design §9 标"已完成"；Dev 必须 pin v2。
- **落点**: 全文引用。

#### R-32（原 Q-32）: `ConfigResolver` trait 位置给两处
- **裁定**: ✅ 接受，**D63** —— trait/`ConfigSource`/`ResolvedConfig`/`FlagDecision` 放 `extension/config.rs`；`src/config.rs` 只留 `AppConfig` + `InMemoryConfigResolver`；列出 4 处联动。
- **落点**: TC-010-042（间接）。

#### R-33（原 Q-33）: 只改 manifest 不加种子迁移 → 进程起不来
- **裁定**: ✅ 接受，**D67** —— 三件套（manifest + 种子迁移 + 重生成 capabilities）。
- **落点**: TC-010-036 / 042。

#### R-34（原 Q-34）: AC-S-050 与 design §4.1 对「非白名单 space 键」相反
- **裁定**: ✅ 接受，**D66** —— 空间级写 = `scope: space` **且**在白名单；否则 400。
- **落点**: TC-010-006。

#### R-35（原 Q-35）: 读接口缺键级授权
- **裁定**: ✅ 接受 —— **新增 AC-S-063**：平台级键读需 `config:platform:read`；租户级键不受限。
- **落点**: TC-010-047 ~ 050。

#### R-36（原 Q-36）: 批量读遇到未登记键的语义未定
- **裁定**: ✅ 接受 —— AC-S-049 步骤 6：**整请求 404** + `params.unknownKeys`。
- **落点**: TC-010-004。

---

## 8.9 rev 2 后新发现（未自行改验收标准，请 BA 复核）

> 以下 **14 条**是**重写后的文本/设计里**新暴露的不一致。它们都不是 rev 1 的旧问题，而是 rev 2 修订过程中留下的。

| # | 涉及 | 问题 | 证据 | 建议 |
|---|---|---|---|---|
| **N1** | acceptance 头部 | **AC 条数写错**：`AC-S-049` ~ `AC-S-063` 是 **15 条**，不是 17 条 | `acceptance.md:14`「17 条」；实际 `### AC-S-` 标题 15 个 | 改为 15；本用例集已按 15 统计 |
| **N2** | design §3.3 / acceptance 修订表 | 错误码个数前后不一致：design 正文写「新增 **5 个**」，表里列 **6 个**；acceptance 修订表也写「5 个」；**D62 写 6 个** | design-summary.md:262 vs :264-271；acceptance.md:30；decisions.md D62 | 统一为 **6 个** |
| **N3** | acceptance 修订表 | 把 AC-S-055 的修订归到 **D63**，但 D63 是 `ConfigResolver` 的位置 | acceptance.md:31 vs decisions.md:69 | 改为「无独立 D 号」或补一个 D69 |
| **N4** | AC-S-052 | 前置「平台级 `logo` = A 已被读过」在 HTTP 层不可达：AC-S-054 给 **T1 和 T2 都配了租户级 `logo`**，没有任何租户能读到平台值 | acceptance.md:111 与 :148-151 | 同 AC-S-053 的修法：换用 **`favicon`**（本用例集已按此写，TC-010-032/033） |
| **N5** | AC-S-054 前置 | 「另有 `conflictStrategy` 空间级值（白名单外）」**无法经 API 造出**：`conflictStrategy` 是 `scope: platform`，空间级写会 400 | acceptance.md:146；AC-S-050 步骤 2 | 注明「由管理员 SQL 直接插入的合成行」；本用例集已在 §3.5 标注 |
| **N6** | AC-S-063 / AC-S-061 | 读侧复现了写侧的互斥：平台级键**按 key** 授权，但 `GET /api/config/entries` **只有一条**路由；`config:platform:read` 要有 `require_permission` 调用点就只能挂静态层 → 要么所有读都要求它（违反 AC-S-063 步骤 2），要么无调用点（C-5 失败）。D59 只解决了写侧 | design-summary.md:388（单条读路由）；AC-S-063 步骤 2；AC-S-061 步骤 2；`permission.rs:219-249` | 二选一并写死：(a) 也拆一条 `GET /api/config/platform-entries`（与写对称）；(b) 在契约里给「请求内键级判定」一个 C-5 可扫描形态 |
| **N7** | AC-S-063 | 未定义「批量读同时含平台级与租户级键」的行为：整体 403 还是部分返回？两种都"合理"，前端行为完全不同 | acceptance.md:321-332（没有这一步） | 写死一种（建议整体 403 + `params.forbiddenKeys`）；TC-010-049 暂按 fail closed 写 |
| **N8** | design §5.1 | 仍写「无任何配置 → `source="platform"`」，与 AC-S-056（T2 未配 → `default`）及 §5.3（未登记 → `default`）矛盾 —— rev 1 的 Q-19 只改了一半 | design-summary.md:317 | 改为 `default`；或明确「flag 的 source 不走 C9 四值」并给出第五种 |
| **N9** | design §2.3 | 仍写「平台级配置的**写入控制必须来自表级权限（只授予平台管理角色）**+ 权限点」，与 §2.1.1（**没有**平台管理角色、必须 REVOKE + DEFINER）自相矛盾 | design-summary.md:190 vs :111-142 | 删掉 §2.3 里那句过时描述，改为「见 §2.1.1」 |
| **N10** | design §2.3 | 称「租户 id 是 UUID 形态，`platform` 不会与真实租户撞车」——真实夹具与 REQ-009 用的是 `tenant-a` / `tenant-b`，**不是 UUID**；若某租户 id 恰为 `platform`，会与 `scope_key='platform'` 及伪租户口径撞车。设计说「要有一条测试守住」，但**没有任何 AC 承载** | design-summary.md:191；`code/crates/platform-rbac/tests/common/mod.rs:19-21`（`tenant-a`/`tenant-b`） | 补一条 AC 或写明「租户 id 禁止取 `platform`」的约束；否则这条不变式无人守 |
| **N11** | AC-S-059 前置 | 「只登记了 `newInvoice`」与 AC-S-057 需要 `beta` **互斥**（同一类夹具冲突，rev 2 未修） | acceptance.md:236 与 :201-203 | 改为「登记了 `newInvoice` 与 `beta`」；本用例集 §3.6 已按两者都登记 |
| **N12** | AC-S-062 步骤 3 / design §2.1.1 | `platform_config_upsert` 的函数体**只 upsert 配置、不自增 epoch**；AC 只验「能写」。若 HTTP 写路径忘了在同一事务里 +1，缓存**永不失效** —— 该风险没有任何 AC 覆盖 | design-summary.md:130-139（函数 DDL）；AC-S-062 步骤 3 | 在 AC 里补「写成功后 `platform` epoch +1」；TC-010-051 第 4/5 步已按此写（派生断言） |
| **N13** | AC-S-050 / D66 | D66 要求「登记为 space 但不在白名单」的键空间写 400，但**没有任何 manifest 声明这样的键** —— 夹具必须自造一个（本用例集用 `loginBanner`） | acceptance.md:87-88；design §6.1 白名单 11 个全部是 space 且在白名单 | 在 `crates/platform-config/manifest.json` 里明确加一个非白名单的 space 键作为测试夹具（或说明由测试专用 manifest 提供） |
| **N14** | AC-S-060 / design §5.3 | **通过配置写路由可以改 flag 键**：flag 本体是普通 `config` 键（`type: boolean`），`PUT /api/config/entries` 只要求 `config:entry:write`。于是只有 entry 写权限的用户可以**绕过** `config:flag:write` 改开关（甚至改 `flags.<F>.rolloutPercent`） | design §5.3（flag 本体登记在 config 段）；design §6 路由表（entries 只挂 entry:write）；AC-S-060 只保护 `PUT /api/feature-flags/{flag}` | 在 `PUT /api/config/entries` 里对「flag 本体键 / 派生键」额外要求 `config:flag:write`，或直接禁止（400）；并补一条 AC |

---

## 9. 建议执行顺序

| 阶段 | 范围 | 说明 |
|---|---|---|
| **第一批**（无库、纯静态/单元） | TC-010-003（第 1 步）、TC-010-004（键名断言）、TC-010-017、TC-010-041、TC-010-042 | 042 依赖「平台 crate 的 C-5 入口」先落地（D58） |
| **第二批**（真实库 + 真机 HTTP，配置与品牌） | TC-010-001、002、005 ~ 016、020 ~ 022、029 ~ 031、036 ~ 039、047 ~ 050 | 需先做 §3 夹具 + 4 个权限点种子迁移（D67）；应用连接 `base_app` |
| **第三批**（缓存收敛 + 灰度统计） | TC-010-023 ~ 028、TC-010-032 ~ 035 | 两个独立实例/进程共享 `base_test`；TC-010-035 是 D55 的**缺陷门禁** |
| **第四批**（DB 权限 / RLS / 迁移 / DEFINER） | TC-010-043 ~ 046、TC-010-051 | 需先做 `REVOKE` + `platform_config_upsert`（D56）；046 需 down/up 各跑一次 |
| **第五批**（浏览器） | TC-010-018、019 | 需前端构建 + **同源部署**（R17） |

**建议的 CI 门禁**：第一批 + 第二批中不依赖起进程的断言 + 第四批。第三批的 TC-010-035 与第四批的 TC-010-043 / 051 是**回归价值最高**的三条（分别锁住 epoch 成对比较、平台表写侧、写路径失效信号）。

---

## 10. 自检记录（交付前，脚本化）

| 检查项 | 结果 |
|---|---|
| 15 条 AC 全覆盖，矩阵与各用例「关联 AC」逐条一致 | ✅（15/15；51 条 TC；矩阵逐条脚本比对通过） |
| 用例编号连续、无重复 | ✅ TC-010-001 ~ TC-010-051，无跳号 |
| 9 字段齐备率 100% | ✅ 51/51（标题 = `###` 标题行，其余 8 个字段逐条统计） |
| 所有夹具 id 在 §3 有定义 | ✅ 租户 T1/T2/TX、空间 S1/S2/SX、用户 2001~2006 / 3001~3200 / 1001、**20 个登记键 + 1 个派生键**、5 类 flag 名、4 张表 |
| 相对链接全部可达 | ✅ 逐个核验（见下） |
| 「验收标准问题清单/修订记录」非空且每条有证据 | ✅ 修订记录 36 条 + 新发现 14 条，均有证据 |
| 不发明 API（路径按 design §6） | ✅ 7 条路由逐条照抄；未定义的 body/读形状在 §4 登记 |
| 攻击向用例有对照 | ✅ 每类拒绝都配了 200 的对照组（TC-005/006/008/010/013/020/029/036/039/040/043/048/049/050） |
| 断言用精确错误码 | ✅ 401/403/404 用现有码；400 用 D62 的 6 个码 |

**链接核验清单**（自 `test-cases/` 出发）：`../acceptance.md`、`../design-summary.md`、`../demand.md`、`../status.md`、`../../../contracts/api-conventions/v2.md`、`../../../contracts/extension-points/v2.md`、`../../REQ-001-platform-foundation/test-cases/index.md`、`../../REQ-001-platform-foundation/decisions.md`、`../../REQ-009-rbac/test-cases/index.md`、同目录 `A-config-scope.md` / `B-branding.md` / `C-feature-flags.md` / `D-security-cache.md` —— 全部存在。
