# REQ-010「配置、品牌与 Feature Flag」—— 测试用例总索引

- **需求编号**: REQ-010「配置、品牌与 Feature Flag」
- **父需求**: REQ-001（通用项目底座）｜**承接父 AC**: AC-013（配置三层覆盖）、AC-014（品牌，本期收窄，见 D52）、AC-015（Feature Flag 按租户生效）
- **验收基线**: [acceptance.md](../acceptance.md) —— **AC-S-049 ~ AC-S-062（14 条）**，本用例集**只**覆盖这一段
- **设计依据**: [design-summary.md](../design-summary.md)（11 节）
- **契约**: [api-conventions v2](../../../contracts/api-conventions/v2.md)（C3 / C6 / C9 / C10）、[extension-points v2](../../../contracts/extension-points/v2.md)（C-5）
- **格式基准**: [REQ-001 用例总索引](../../REQ-001-platform-foundation/test-cases/index.md)、[REQ-009 用例总索引](../../REQ-009-rbac/test-cases/index.md)（§7「已裁定约定」/ §9「修订记录」结构）
- **状态**: 待实现（用例本身**尚未执行**，不得据此判定需求通过）
- **版本**: 第 1 版（2026-11-18，规格写实后首轮）

---

## 1. 概览

| 项 | 值 |
|---|---|
| 用例总数 | **46**（TC-010-001 ~ TC-010-046，编号连续无跳号） |
| 覆盖 AC | **14 / 14**（AC-S-049 ~ AC-S-062，无遗漏） |
| 优先级分布 | P0 = **40**、P1 = **6**、P2 = 0 |
| 自动化分布 | 可自动化 = **44**、半自动 = **2**、手工 = 0 |
| 用例文件 | 4 个（另本索引 1 个） |
| 每条用例字段 | 标题 / 关联 AC / 前置条件 / 测试步骤 / 预期结果 / 类型 / 优先级 / 自动化 / 设计方法（**9 项全部齐备**） |

> **覆盖条数最少的 AC**：`AC-S-052`（2）、`AC-S-053`（2）、`AC-S-056`（2）、`AC-S-058`（2）、`AC-S-059`（2）。
> 这 5 条各只描述**一件事**，但都配了「正常 + 反例/攻击路径」两路（052 = 时序 + 跨实例；053 = 多租户广播 + epoch 掩蔽；056 = 跨租隔离 + 空间级；058 = 无身份 + 三档对照；059 = 正常降级 + 输入健壮性）。
> **类型含「安全」的 11 条 AC**（050、051、052、053、054、056、057、058、060、061、062）**全部 ≥ 2 条**。
> 部分用例在 AC 之外补了攻击面或把「不可判定」变成「可判定」（正文标注「派生」或指向 §8 的对应问题项），例如未认证写 401、跨租头 404、空间表写侧 RLS、未声明 flag 名的日志注入。

## 2. 文件清单

| 文件 | 能力域 | AC 范围 | 用例数 |
|---|---|---|---|
| [A-config-scope.md](A-config-scope.md) | 作用域、`source` 与类型校验 | AC-S-049 ~ AC-S-051 | 11 |
| [B-branding.md](B-branding.md) | 品牌公开接口与前端接入 | AC-S-054、AC-S-055 | 8 |
| [C-feature-flags.md](C-feature-flags.md) | Feature Flag 与确定性灰度 | AC-S-056 ~ AC-S-059 | 12 |
| [D-security-cache.md](D-security-cache.md) | 缓存失效、写接口鉴权、C-5、三表与 RLS | AC-S-052、AC-S-053、AC-S-060 ~ AC-S-062 | 15 |

---

## 3. 测试夹具（造数方式）

> **原则**：租户与空间在本仓**都没有实体表** —— `tenant_id` / `space_id` 都是字符串（已核实：全仓没有 `CREATE TABLE ... space`，也没有 `platform_tenant` 注册表）。所以夹具 id 直接写死为字符串，**不造影子表**。
> **账号**由 REQ-007 的 IAM 提供（`platform_iam_user.id` 是 `TEXT`）；**角色 / 授权**由 REQ-009 的 RBAC 表提供（`platform_role` / `platform_role_permission` / `platform_user_role`）。统一初始密码 `Passw0rd!`。
> **造数走管理员连接**（`base_test`），**业务访问走 `base_app`**（超管会绕过 RLS，`code/crates/platform-server/src/isolation.rs`）。
> **可重置**：每条用例开始前把配置行与 flag 值恢复到 §3.5 / §3.6 的初始值（租户、空间、账号、角色固定不变）。**用例不得依赖执行顺序** —— 例如 TC-010-009 会改 `cacheTtlSeconds`，其收尾步骤必须恢复 300（AC-S-052 的 TTL 前提）。

### 3.1 租户

| ID | tenant_id | 用途 |
|---|---|---|
| T1 | `tenant-a` | 主测试租户（三层覆盖 / 写接口 / 越级写 / RLS） |
| T2 | `tenant-b` | 跨租对照（同 `space_id` 字符串的跨租隔离、平台级变更广播） |
| TX | `tenant-does-not-exist` | **不存在的租户 id**，品牌公开接口的探测对照 |

### 3.2 空间（**没有 space 表**，纯字符串）

| ID | space_id | 说明 |
|---|---|---|
| S1 | `space-sales-1` | T1 的主空间；**T2 也使用同一字符串**（验证复合维度 `(tenant_id, space_id)`） |
| S2 | `space-legal` | T1 的第二个空间（局部性对照） |
| SX | `space-does-not-exist` | 不存在的空间 id（`X-Space-Id` 缺失/无效都不报错，契约 C3） |

### 3.3 账号与角色

| user_id | 账号 | 租户 | 绑定角色 | 用途 |
|---|---|---|---|---|
| `u-2001` | plain@example.com | T1 | `platform-user`（内置，**无** config 权限点） | **M**：普通用户；读接口/403 用例 |
| `u-2002` | cfgwriter@example.com | T1 | `cfg-entry-writer`（`config:entry:write` + `config:flag:write`） | **A**：租户级写成功、平台级写 403 |
| `u-2003` | cfgsuper@example.com | T1 | `cfg-platform-writer`（三个权限点全有） | **P**：平台级写 / 品牌对照读 |
| `u-2004` | cfgplatformonly@example.com | T1 | `cfg-platform-only`（**只有** `config:platform:write`） | **O**：权限正交性（TC-010-039） |
| `u-2005` | t2plain@example.com | T2 | `cfg-entry-writer`（`config:entry:write` + `config:flag:write`） | **T2 的读/写调用者** —— 读 T2 的配置与开关**必须**用它（不能用 T1 的令牌：头与令牌不一致会 404） |
| `u-3001` ~ `u-3200` | rollout-N@example.com | T1 | 无需角色 | 灰度统计的 **200 个不同用户**（`N` = 3001..3200） |
| `u-1001` | owner@example.com | T1 | `platform-admin`（注册表全集，动态得出，D31） | 造 RBAC 夹具 / 查库对照（**不**作为被测调用者） |

**角色定义**（`platform_role` + `platform_role_permission`，`data_scope = SELF` 即可 —— 本需求不涉及数据范围）：

| 角色 code | 权限点 |
|---|---|
| `cfg-entry-writer` | `config:entry:write`、`config:flag:write` |
| `cfg-platform-writer` | `config:entry:write`、`config:flag:write`、`config:platform:write` |
| `cfg-platform-only` | `config:platform:write` |

### 3.4 配置键（注册表 = 各 manifest 的 `config` 段）

| 键 | type | default | scope | 品牌白名单 | 来源 manifest | 用途 |
|---|---|---|---|---|---|---|
| `logo` | string | `/branding/default.svg` | **space** | ✅ | `crates/platform-config/manifest.json`（新建） | 三层覆盖、越级写 |
| `title` | string | `示例平台` | space | ✅ | 同上 | `source=default`（三层都不配） |
| `primaryColor` | string | `#2f54eb` | space | ✅ | 同上 | `source` 的判别性（值=默认、来源=platform） |
| `favicon` | string | `/favicon-default.svg` | space | ✅ | 同上 | AC-S-052/053（T1 无覆盖，能命中平台值） |
| `loginBackground` | string | ``（空串） | space | ✅ | 同上 | 白名单 |
| `footer` | string | `示例平台` | space | ✅ | 同上 | 白名单 |
| `copyright` | string | `© 2026` | space | ✅ | 同上 | 白名单 |
| `theme.radius` | number | `4` | space | ✅（`theme.*`） | 同上 | number 类型 |
| `theme.fontStack` | array | `["system-ui"]` | space | ✅（`theme.*`） | 同上 | array 类型 |
| `theme.tokens` | object | `{}` | space | ✅（`theme.*`） | 同上 | object 类型 |
| `beta` | boolean | `false` | tenant | —（**不在**白名单） | 同上 | 灰度 flag |
| `newInvoice` | boolean | `false` | tenant | — | 同上 | 租户隔离 flag |
| `cacheTtlSeconds` | number | `60` | **platform** | — | `crates/platform-rbac/manifest.json`（**已有**） | 类型校验 / 缓存 TTL |
| `conflictStrategy` | string | `manual` | **platform** | — | `crates/platform-org/manifest.json`（**已有**） | 越级写 400 |
| `scimBaseUrl` | string | `` | platform | — | 同上（**已有**） | 白名单外键 |
| `ldapBaseDn` | string | `` | platform | — | 同上（**已有**） | 白名单外键 |
| `pageSize` | number | `20` | **tenant** | — | `modules/sandbox/manifest.json`（**已有**） | `scope: tenant` 的空间写 400 |

**未登记的键**（用例里用来打「未声明 → 400」）：`notRegistered`。
**非法键名**（打「键名格式 → 400」）：`Theme.radius`（大写开头）、`theme-radius`（连字符）。
**⚠️ 真实代码事实**：`ConfigSpec.scope` 的**缺省值是 `tenant`，不是 `platform`**（`code/crates/platform-core/src/extension/manifest.rs:213-215`，schema 同：`code/crates/platform-core/src/extension/manifest_schema.rs:71`）。design §3.2 写「现在只有一个值（默认 `platform`）」是**错的**（§8 Q-13）—— 所以品牌键**必须显式写** `"scope": "space"`，否则空间级写会被 400 拒掉。

### 3.5 已存配置值（三层，真实表）

**`platform_config`**（平台级；无 `tenant_id`、无 RLS）：

| key | value |
|---|---|
| `logo` | `/branding/platform-a.svg`（**A**） |
| `favicon` | `/favicon-platform.svg`（**F**） |
| `primaryColor` | `#2f54eb`（**显式**写入，与 manifest 默认同值） |
| `conflictStrategy` | `manual` |
| `scimBaseUrl` | `https://scim.example.com` |
| `cacheTtlSeconds` | `300`（TTL ≥ 300s，AC-S-052 的前置） |
| `title` | **不写入** → 解析为 default `示例平台`，`source="default"` |

**`platform_tenant_config`**（租户级；`value` 从 `TEXT` 迁移到 `jsonb`）：

| tenant_id | key | value | 记号 |
|---|---|---|---|
| `tenant-a` | `logo` | `/branding/tenant-a.svg` | **B** |
| `tenant-a` | `newInvoice` | `true` | — |
| `tenant-a` | `pageSize` | `20` | 初始值（被用例改） |
| `tenant-b` | `newInvoice` | `false` | — |
| `tenant-b` | `title` | `另一公司` | 跨租对照 |

**`platform_space_config`**（空间级；`(tenant_id, space_id, key)`）：

| tenant_id | space_id | key | value | 记号 |
|---|---|---|---|---|
| `tenant-a` | `space-sales-1` | `logo` | `/branding/space-s1.svg` | **C** |
| `tenant-a` | `space-legal` | `logo` | `/branding/space-s2.svg` | **D** |
| `tenant-b` | `space-sales-1` | `logo` | `/branding/space-t2-s1.svg` | **E** |

> **A/B/C/D/E 五个值必须两两不同** —— 否则"命中层级"无法从值上判别（TC-010-001 的判定基础）。

### 3.6 Feature Flag

| flag | 登记 | T1 存储 | T2 存储 | 用途 |
|---|---|---|---|---|
| `newInvoice` | `type: boolean`、`scope: tenant` | `true` | `false` | AC-S-056 租户隔离 |
| `beta` | `type: boolean`、`scope: tenant` | 无（`enabled=false`），`rolloutPercent` 由用例逐次设置 | 无 | AC-S-057 / 058 灰度 |
| `newinvoce` | **未登记** | — | — | AC-S-059 拼错名 |

- **flag 存储形状**（§7 A8，属硬伤 Q-03）：`{"enabled": <bool>, "rolloutPercent": <number|null>}`。写接口独立校验，**不套** config 的 JSON 类型校验（否则 `type: boolean` 会把 `rolloutPercent` 判成类型错）。
- **200 个灰度用户**：`u-3001` ~ `u-3200`（T1）。用户名与 `user_id` 都用 `u-300N` 形式，保证哈希输入可复现。

### 3.7 权限点（`platform_permission` + `capabilities/permissions.json`）

| 权限点 | 来源 manifest | 状态 |
|---|---|---|
| `config:entry:write` | `crates/platform-config/manifest.json`（**新建**） | 新增 |
| `config:flag:write` | 同上 | 新增 |
| `config:platform:write` | 同上 | 新增 |
| 既有 18 个（sandbox 5 / org 9 / rbac 4） | 各自 manifest | **已存在**（`code/capabilities/permissions.json`，`total = 18`） |

- **不写死权限点总数**：`platform-admin` 的权限集合 = 注册表当前全集、动态得出（D31）。对账一律按**集合**逐条比对。
- **⚠️ 只改 manifest 不够**：启动时 `reconcile_permissions` 要求「manifest 声明集合」与「`platform_permission` 表内容」**完全一致**，不一致**进程退出**（`code/crates/platform-server/src/main.rs:424-442`）。所以新增 3 个权限点必须同时加**权限点种子迁移**（照 `code/migrations/20261118000901_rbac_permission_seed.up.sql`）。AC-S-060 只写了"在 manifest 登记"（§8 Q-33）。

### 3.8 表、授权与连接

| 表 | 关键列 | RLS | 状态 |
|---|---|---|---|
| `platform_config` | `key`(PK) / `value jsonb` / `updated_at` | **无**（同 `platform_meta`） | 新建 |
| `platform_tenant_config` | `tenant_id` / `key` / `value` / `updated_at`，PK `(tenant_id,key)` | **有**（REQ-006 已建） | 已存在；`value` 从 `TEXT`（`code/migrations/20261118000200_tenant_isolation.up.sql:22`）迁移到 `jsonb` |
| `platform_space_config` | `tenant_id` / `space_id` / `key` / `value jsonb` / `updated_at`，PK `(tenant_id,space_id,key)` | **有**（新建） | 新建 |
| `platform_config_epoch` | `scope_key`(PK，`'platform'` 或 `'tenant:<id>'`) / `epoch` / `updated_at` | 无 | 新建（design §7.1） |

- **⚠️ 授权现状（已核实）**：`code/deploy/seed/02-app-role.sql:50` 把 `SELECT/INSERT/UPDATE/DELETE` 授予 `base_platform_app` 于 **public 所有表**，`:54-55` 的 `ALTER DEFAULT PRIVILEGES` 让**之后新建的表**自动获得同样权限 —— 新表 `platform_config` **默认就对租户连接可写**。这直接推翻 AC-S-062 步骤 2 的预期（§8 Q-28）。
- **连接**：应用 `base_app`（`base_platform_app` 组成员）；`base_app` 由 `app_role_url()` 构造（`code/crates/platform-rbac/tests/common/mod.rs:73-77`）。造数用管理员连接。
- **租户上下文**：RLS 读 `current_setting('app.tenant_id', true)`（`code/crates/platform-db/src/rls.rs:141-150`）。

### 3.9 日志捕获

AC-S-059 要求"告警日志内容可断言"。做法同 `platform-server` 的测试辅助：`tracing_subscriber::fmt().with_writer(BufWriter)` 把事件写进内存缓冲，断言 **level = WARN** 与**结构化字段** `flag`（不是匹配整行文本）。

---

## 4. 命名与接口约定（本用例集的实际取值）

| 约定项 | 取值 | 来源 |
|---|---|---|
| 平台 API 前缀 | `/api/config`、`/api/feature-flags` | 契约 C1 |
| 路由 | `GET /api/config/entries?keys=a,b`、`GET /api/config/branding`、`PUT /api/config/entries`、`GET /api/feature-flags`、`GET /api/feature-flags/{flag}`、`PUT /api/feature-flags/{flag}` | design §6（**路径照此表，不自创**） |
| 鉴权 | `entries` / `feature-flags` **需要令牌**；`branding` **公开**；写接口要权限点 | design §6 |
| 配置读响应 | `{"entries":[{"key","value","source"}]}`，按请求顺序、重复键去重 | §7 A6（design 未写死） |
| 品牌响应 | 扁平 `{"logo": "...", ...}`，无包装 | §7 A5（design 未写死） |
| flag 响应 | `{"flag","enabled","source"}`；批量 `{"flags":[...]}` | 契约 C10 |
| 写 body | entries：`{"scope","key","value"}`；flag：`{"enabled","rolloutPercent"?}` | §7 A2 / A4（design 未写死） |
| `source` 取值 | `platform` / `tenant` / `space` / `default`（**四个**） | C9 v2 §9.3 |
| 可写作用域 | `platform` / `tenant` / `space`（**三值**，`default` 不可写） | C9 v2 §9.4 |
| 400 错误码 | `CONFIG_KEY_NOT_REGISTERED` / `CONFIG_SCOPE_FORBIDDEN` / `CONFIG_TYPE_MISMATCH` / `CONFIG_VALUE_INVALID`；键名格式错用既有的 `INVALID_CONFIG_KEY` | §7 A3（**新增码，待 BA 确认**） |
| 鉴权错误码 | 401 `UNAUTHENTICATED` / 403 `INSUFFICIENT_PERMISSION` / 404 `NOT_FOUND` | 契约 C6 + D26 + `code/crates/platform-core/src/problem.rs:33-39` |
| 上下文头 | `X-Tenant-Id`（与令牌不一致 → 404）、`X-Space-Id`（缺失 = 不限空间） | 契约 C3 |
| 键名 | camelCase，层级 `.` 分隔；分段 `^[a-z][a-z0-9]*$` | 契约 C9 + `code/crates/platform-core/src/config.rs:90-100` |
| flag 名 | 以 manifest 登记的**原样字符串**为哈希输入 | C10 v2 §10.2（写法冲突见 §8 Q-20） |
| 表名 | `platform_config` / `platform_tenant_config` / `platform_space_config` / `platform_config_epoch` | design §2 / §7.1 |
| 跑法 | `cargo test --workspace -- --test-threads=2`（**只能有一个 `--`**）；集成测试 `DATABASE_URL_TEST=postgres://base:base@127.0.0.1:5432/base_test`，库名必须恰好 `base_test`；`export CARGO_TARGET_DIR=…/code/target` | design §10 + `code/crates/platform-rbac/tests/common/mod.rs:46-70` |

---

## 5. AC → TC 覆盖矩阵（14 / 14）

| AC | 标题 | 覆盖 TC | 条数 | 外部依赖 | 最高优先级 |
|---|---|---|---|---|---|
| AC-S-049 | 三层覆盖与 `source` 如实反映命中层级 | TC-010-001、TC-010-002、TC-010-003、TC-010-004 | 4 | 真实库 `base_test` + 真机 HTTP | P0 |
| AC-S-050 | 作用域越级写入被拒 | TC-010-005、TC-010-006、TC-010-007 | 3 | 真实库 + 真机 HTTP | P0 |
| AC-S-051 | 未声明的键不得写入，且按登记类型校验 | TC-010-008、TC-010-009、TC-010-010、TC-010-011 | 4 | 真实库 + 真机 HTTP | P0 |
| AC-S-052 | 配置变更在**下一个请求**生效 | TC-010-032、TC-010-033 | 2 | 真实库 + **两个独立实例** | P0 |
| AC-S-053 | 平台级变更使**所有租户**缓存失效 | TC-010-034、TC-010-035 | 2 | 真实库 + 两个独立实例 | P0 |
| AC-S-054 | 品牌公开接口白名单 fail closed | TC-010-012、TC-010-013、TC-010-014、TC-010-015、TC-010-016 | 5 | 真实库 + 真机 HTTP（**无令牌**） | P0 |
| AC-S-055 | 前端无硬编码品牌（父 AC-014 收窄版） | TC-010-017、TC-010-018、TC-010-019 | 3 | 静态扫描 + **浏览器（Playwright）+ 同源部署** | P0 |
| AC-S-056 | Feature Flag 按租户隔离 | TC-010-020、TC-010-031 | 2 | 真实库 + 真机 HTTP | P0 |
| AC-S-057 | 灰度确定性、单调性与边界 | TC-010-023、TC-010-024、TC-010-025、TC-010-026、TC-010-027、TC-010-028 | 6 | 真实库 + 单元层 + 两个实例 | P0 |
| AC-S-058 | 无用户身份时灰度 fail closed | TC-010-021、TC-010-022 | 2 | 真实库 + 单元层 | P0 |
| AC-S-059 | 未声明的 flag 不返回 404 | TC-010-029、TC-010-030 | 2 | 真实库 + 真机 HTTP + **日志捕获** | P0 |
| AC-S-060 | 配置与开关的写接口必须鉴权 | TC-010-036、TC-010-037、TC-010-038、TC-010-039、**TC-010-040** | 5 | 真实库 + 真机 HTTP + 三档账号 | P0 |
| AC-S-061 | 新登记权限点必须有鉴权调用点 | **TC-010-040**、TC-010-041、TC-010-042 | 3 | 静态扫描 + **新增平台 crate 的 C-5 入口** | P0 |
| AC-S-062 | 三表分离与 RLS 兜底，`jsonb` 可迁移 | TC-010-043、TC-010-044、TC-010-045、TC-010-046 | 4 | 真实库 + `base_app` + `sqlx migrate` | P0 |

**合计 14 条 AC、46 条 TC**（TC-010-040 同时覆盖 AC-S-060 与 AC-S-061，矩阵内重复列出一次；按主 AC 去重后 46 条）。**无遗漏 AC、无悬空 TC。**
> TC-010-011 的第 2 步同时验证 AC-S-050 的作用域判定参与排序，但「关联 AC」按主 AC 记为 AC-S-051，因此不重复计入 050 的条数。

---

## 6. 当前无法立即执行的 AC

> 前提：**所有**用例都要等实现落地才可能执行。下表标的是「除实现之外，还缺什么才能跑出判定结果」。

### 6.1 判据本身在 HTTP 层不可达（需 BA 裁定或改到单元层）

| AC | 缺失项 | 影响 TC | 处理 |
|---|---|---|---|
| AC-S-049 步骤 1 | 「无租户上下文」在已认证 HTTP 路径不存在（租户由令牌推断；`require_permission` 缺 `tenant_id` 直接 401） | TC-010-003 | 已改为**单元层**断言 `RequestContext::new()`；HTTP 侧断言 401（§8 Q-07） |
| AC-S-056 步骤 3 | 同上（读 flag 需要令牌） | TC-010-020 第 3 步 | 断言 **401** 并记录偏差（§8 Q-07） |
| AC-S-058 步骤 1 | 「不带令牌读开关 → `enabled=false`」与 design §6「需要令牌」冲突 | TC-010-021 | HTTP 断言 401，`enabled=false` 改在单元层验证（§8 Q-07） |

### 6.2 需要额外基础设施 / 前置修复

| AC | 缺失项 | 影响 TC | 处理 |
|---|---|---|---|
| AC-S-055 步骤 2/3 | 前端构建 + **同源部署或 CORS**（服务端当前**没有**任何 CORS 层，shell 的 API base 是 `:8080`、Playwright 预览是 `:4173`） | TC-010-018, 019 | 把 `apps/shell/dist` 挂到 `platform-server` 的静态目录、同源访问（§8 Q-27） |
| AC-S-060 / 061 | `crates/platform-config` **没有** C-5 入口（`ContractFixture` 只适用于 `modules/<name>`） | TC-010-040, 041, 042 | 需新增「对平台 crate 执行 C-5」的入口（独立函数或 `tests/contracts.rs`）；否则 AC-S-061 无法判定（§8 Q-04） |
| AC-S-060 步骤 4 / AC-S-061 | 「单路由 + 静态 `route_layer`」下两者互斥 | TC-010-040 | 需 BA/Dev 选形态（拆路由，或请求内判定 + 独立可扫描调用点）（§8 Q-05） |
| AC-S-062 步骤 2 | 需要「平台管理角色 / 第二连接池」的决策；且默认授权已把 INSERT 开给 `base_app` | TC-010-043 | 迁移里显式 `REVOKE`，或引入第二角色/池；否则该步必然失败（§8 Q-01 / Q-28） |
| AC-S-052 / 053（真多进程） | 本机单实例；用**两个独立实例**模拟跨进程 | TC-010-033, 035 | acceptance「已知不验收项」已列，本期按两实例判定 |
| AC-S-057（跨进程） | 同上 | TC-010-028 | 两实例模拟；真多实例属部署侧 |

### 6.3 可无外部依赖执行的部分

- **静态/单元入口**（不连库）：TC-010-003 的单元部分、TC-010-004 的键名断言、TC-010-017、TC-010-041、TC-010-042。
- **纯 DB 断言**：TC-010-043 ~ TC-010-046（`base_app` + 管理员连接 + `sqlx migrate`）。
- **其余**需要真机/进程内 HTTP（`/api/config/*` 由 `platform-server` 挂载）；TC-010-018 / 019 需要浏览器。

---

## 7. 假设与待 BA 确认的约定

> 以下条目在 acceptance.md / design-summary.md / 契约里**没有写死**。本用例集按最合理方式取值并在此登记；BA 裁定后按裁定改用例。**每条都对应 §8 的一个问题项**（有的话）。

| # | 事项 | 本用例集的取值 | 依据 / 对应问题 |
|---|---|---|---|
| A1 | 租户/空间的表示 | 纯字符串；T1=`tenant-a`、T2=`tenant-b`；**没有** space 表、**没有** tenant 表 | 真实代码（acceptance「明确不做」） |
| A2 | `PUT /api/config/entries` 的 body | `{"scope":"platform|tenant|space","key":"<key>","value":<JSON>}`；租户来自令牌、空间来自 `X-Space-Id` | design §6 只给路径、未给 body（本用例集取值为假设） |
| A3 | 400 的稳定错误码 | `CONFIG_KEY_NOT_REGISTERED` / `CONFIG_SCOPE_FORBIDDEN` / `CONFIG_TYPE_MISMATCH` / `CONFIG_VALUE_INVALID`；键名格式错沿用既有 `INVALID_CONFIG_KEY` | 现有 4 个码无 400（Q-16） |
| A4 | `PUT /api/feature-flags/{flag}` 的 body | `{"enabled": <bool>, "rolloutPercent": <int 0..=100>?}`；`rolloutPercent` 必须是整数 | design 未给 body（假设）；`rolloutPercent` 的存放问题是 Q-03，写/读时机是 Q-17 |
| A5 | 品牌接口响应形状 | 扁平对象 `{key: value}`；无 `source`；未配置的白名单键返回 manifest 默认 | design §6.1 只列约束（Q-11） |
| A6 | entries 批量读响应 | `{"entries":[...]}`，按请求顺序；重复键去重；含未登记键 → 整请求 **404** | C9 只给单条形状（Q-36） |
| A7 | 三张新表的 DDL | `platform_config(key PK, value jsonb, updated_at)`；`platform_space_config((tenant_id,space_id,key) PK, value jsonb, updated_at)`；`platform_config_epoch(scope_key PK, epoch BIGINT, updated_at)`；迁移建议名 `20261118001000_config_schema`（含 down） | design §2 只有列级描述（Q-30） |
| A8 | flag 的存储形状 | 复合对象 `{"enabled":bool,"rolloutPercent":number|null}`；flag 写接口独立校验，不套 config 的 JSON 类型校验 | `ConfigSpec` 无 rollout 字段（**硬伤** Q-03） |
| A9 | 写入判定顺序 | 认证(401) → 权限点(403) → 键名格式(400) → 注册表(400) → 作用域(400) → 类型(400) | 多错因输入需可区分（Q-15） |
| A10 | 缓存 TTL | 启动**前** seed 平台级 `cacheTtlSeconds = 300`；用例中途不改 TTL | AC-S-052「≥300s」 |
| A11 | 未知租户的品牌取值 | **200 + 平台默认品牌**（不报错）；空串头按缺失处理 | 本仓**没有租户注册表**（见 §3 原则），未知租户无从校验 |
| A12 | 空间级写的空间来源 | `X-Space-Id`；`scope=space` 但缺该头 → **400 `CONFIG_SCOPE_FORBIDDEN`** | 契约 C3「缺失=不限空间」是读语义，不是写语义 |
| A13 | `theme.*` 白名单 | 按**前缀**匹配（`theme.radius` / `theme.fontStack` / `theme.tokens` 都算白名单） | design §6.1 写 `theme.*`（Q-23） |
| A14 | 日志断言方式 | `tracing` 订阅器写内存缓冲，断言 level 与**结构化字段** `flag`（不匹配整行文本） | AC-S-059「日志内容可断言」 |

---

## 8. 验收标准问题清单

> 逐条核对 `acceptance.md` 的 14 条 AC 与 `design-summary.md` / 契约 / **真实代码**。**36 条**，其中 **12 条硬伤**（判据自相矛盾、逻辑上不可判定、或按字面实现必然失败）。每条都给了可定位的证据；BA 裁定后请回写 AC，并同步修改对应 TC。

| 严重度 | 条数 | 编号 |
|---|---|---|
| **硬伤**（不修就无法判定 / 按字面实现必然失败） | 12 | Q-01 ~ Q-05、Q-07、Q-09、Q-12、Q-25、Q-27、Q-28、Q-33 |
| **高**（判据不成立或攻击面缺口，需补判据/补用例） | 14 | Q-06、Q-08、Q-10、Q-11、Q-14 ~ Q-20、Q-22、Q-24、Q-29 |
| **中**（表述、契约基线、边界缺失） | 10 | Q-13、Q-21、Q-23、Q-26、Q-30 ~ Q-32、Q-34 ~ Q-36 |

### 8.1 硬伤（会让「通过」与「正确」脱钩）

#### Q-01: AC-S-062 步骤 2 与 AC-S-060 步骤 5 在「单连接池 + 无平台管理角色」下互斥
- **严重度**: 硬伤
- **AC**: AC-S-062 / AC-S-060
- **问题**: 062 要求「租户连接**写不进** `platform_config`」，060 步骤 5 要求「P 在运行时**写得进**平台级配置」。二者要同时成立，必须有**两条权限不同的连接**（或 `SECURITY DEFINER` / GUC 门控）。但本仓运行时**只有一个池**，且连接角色是 `base_app`；仓库里也**没有**「平台管理角色」。
- **证据**: `code/crates/platform-server/src/main.rs:103-108`（`AppState` 只有 `pool`）；`code/deploy/seed/02-app-role.sql:20,34`（只有 `base_platform_app` / `base_app`，无平台管理角色）；acceptance.md:197-208 与 :230-237 的预期互斥。
- **建议**: 明确平台级写的**通道**：(a) 第二角色 + 第二池（`base_platform_admin`，只授 `platform_config` 写权）；或 (b) `SECURITY DEFINER` 函数；或 (c) 会话 GUC 门控的 policy（需说明为何不算 design §2.1 所否的「单表 nullable 口子」）。同时把该角色写进 `deploy/seed`。

#### Q-02: design §7.1 的 `max(epoch)` 会漏失效 —— 平台写入可能被更高的租户 epoch 掩蔽
- **严重度**: 硬伤
- **AC**: AC-S-052 / AC-S-053
- **问题**: 读路径 `SELECT max(epoch) ... WHERE scope_key IN ('platform','tenant:<id>')`。两行**各自独立自增**：当 `tenant:<id>` 的 epoch 高于 `platform` 时，平台级写入只让 `platform` +1，`max` **不变** → 该租户的缓存被判为新鲜 → 继续返回旧值。AC-S-053 的「平台级变更让所有租户失效」在这种顺序下不成立。
- **证据**: design-summary.md:266（`SELECT max(epoch) ...`）；design-summary.md:264-265（写各自自增）。
- **建议**: 改成「两行分别比较（`platform_epoch` 与 `tenant_epoch` 都要匹配）」或「单一全局 epoch 行」或「平台写入时把所有租户行抬到 ≥ platform」。**不要**只改文档措辞 —— 这条只能用实现选择解决。用例 TC-010-035 已按反例构造（先让租户 epoch 更高）。

#### Q-03: AC-S-057 需要的 `rolloutPercent` 在注册表里无处安放（与 AC-S-051 / design §5.3 三者互斥）
- **严重度**: 硬伤
- **AC**: AC-S-057 / AC-S-051
- **问题**: ① design §5.3 说「Flag 键也登记在 manifest 的 `config` 段（`type: boolean`）」；② `ConfigSpec` 只有 `type/default/scope/label`，**没有** rollout 字段；③ AC-S-057 要求 `rolloutPercent` 可作为 flag 的配置被读写；④ AC-S-051 要求「未声明的键 → 400」。于是：把 `rolloutPercent` 存成独立键 → 因未声明被 400；存成 boolean 键的一部分 → 类型校验会把数字判成类型错；不校验 → 与 ② 矛盾。
- **证据**: design-summary.md:197-201（flag 登记为 boolean）；`code/crates/platform-core/src/extension/manifest.rs:147-162`（`ConfigSpec` 无 rollout）；acceptance.md:155-173（057 需要该字段）；acceptance.md:71-83（051 未声明 → 400）。
- **建议**: 二选一并写死：(a) `ConfigSpec` 增 `rollout` 可选字段（登记 `type: boolean` + `rollout: {min:0,max:100}`）；或 (b) flag 的登记类型改为 `object`，值为 `{"enabled","rolloutPercent"}`，并说明 flag 不走通用 `type` 校验。本用例集按 (b) 的**行为形态**写断言（§7 A8），存储形状可随裁定调整。

#### Q-04: AC-S-061 的验证手段不存在；且它假设的「`platform-org` 那 9 个点」实际是 0 个调用点
- **严重度**: 硬伤
- **AC**: AC-S-061
- **问题**: (a) `cargo test -p sandbox --test contracts` 只扫 `modules/sandbox`；`platform-contracts` 的 C-5 走 `ContractFixture`，该 trait 要求 `Repository` + `PlatformModule` —— **平台 crate 没有也不该有**。全仓只有 `modules/sandbox/tests/contracts.rs` 一个入口。所以「跑平台的契约测试」**不存在**。(b) AC 说「平台 crate 的鉴权现状（`platform-org` 那 **9 个点**）不得恶化」——`platform-org` 有 9 个**已登记权限点**，但 `require_permission` 调用点是 **0**（路由只有 `Authenticated` 提取器，没有任何权限判定）；`platform-rbac` 的 `rbac:permission:read` 也是 0。若把 C-5 扩到平台 crate，**当天就会失败**。
- **证据**: `code/crates/platform-contracts/src/fixture.rs:31-67`；`code/crates/platform-org/src/http.rs:295-314`（router 无鉴权层）；`code/crates/platform-rbac/src/http.rs:419-420`（注册表路由显式豁免，`rbac:permission:read` 无调用点）；`code/capabilities/permissions.json:39-85`（org 9 个）；acceptance.md:212-219。
- **建议**: (a) 新增一个**不依赖 ContractFixture** 的 `check_c5_for(manifest_path, src_dir)`，让 `crates/platform-config` 能跑；(b) 把 AC-S-061 第三句改成「新增的 3 个权限点必须有调用点；既有的 9+1 个缺口登记为技术债，本次不扩围」，否则「不得恶化」无法判定（基线是 0，无法再降）。用例 TC-010-042 已按基线快照写。

#### Q-05: AC-S-060 步骤 3/4 与 AC-S-061 在「单路由 + 静态 `route_layer`」下互斥
- **严重度**: 硬伤
- **AC**: AC-S-060 / AC-S-061
- **问题**: `require_permission(...)` 返回的是**静态路由层**（`PermissionLayer`），挂在路由上就对**所有**请求生效。若把 `config:platform:write` 挂到与租户写同一条 `PUT /api/config/entries` 上，A（只有 `config:entry:write`）连**租户级**写都会 403 —— 与 060 步骤 3 冲突；若只在 handler 里按 body 的 `scope` 判定，则 `config:platform:write` **没有** `require_permission` 调用点 —— 与 061 冲突（C-5 只认这个形态）。design §6 只给了**一条** PUT 路由。
- **证据**: `code/crates/platform-core/src/extension/permission.rs:219-249`（静态层 + `PermissionLayer`）；design-summary.md:236-241（路由表只有一条 `PUT /api/config/entries`）；acceptance.md:198-208（步骤 3/4）。
- **建议**: 三选一写死：(a) 拆两条路由（平台级单独前缀，如 `/api/config/platform-entries`）；(b) 保留单路由但在 handler 里判定，并给 C-5 一个「请求内权限判定」的可扫描形态（需要扩契约，扩扫描器）；(c) 单路由 + 两个 `route_layer`，但接受 A 无法写租户级（**不可接受**，与 060 冲突）。本用例集 TC-010-040 按 (a)/(b) 都能过的方式写（只要 `platform:write` 有真实可用的调用点）。

#### Q-06: AC-S-049 的「五种 `source` 取值」实际只有四种
- **严重度**: 高（计数错误：会让用例无法写全，Dev 会去找并不存在的第五种）
- **AC**: AC-S-049
- **问题**: 步骤 1~5 只产出 `platform` / `tenant` / `space` / `default` **四个** `source`；第 5 步是 **404**，没有 `source`。C9 v2 §9.3 也明确是四值。
- **证据**: acceptance.md:53（「五种 `source` 取值」）对照 :46-52（四个值 + 一个 404）；`BA/contracts/api-conventions/v2.md:53-58`（四个值）。
- **建议**: 改为「四种」，并把「404 没有 `source`」写成独立判据（TC-010-002 已按四种写）。

#### Q-07: 「无租户上下文」与「无令牌读开关」在 HTTP 层都不可达
- **严重度**: 硬伤
- **AC**: AC-S-049 / AC-S-056 / AC-S-058
- **问题**: 三条 AC 都假设能构造「无租户上下文」或「无身份」的 HTTP 请求并拿到 **200**。但：读配置/读开关都标注「需要令牌」；现有认证范式在**头缺失时由令牌推断租户**；`require_permission` 在缺 `tenant_id` 时直接 **401**。未认证请求只会 401，不会 200 + `enabled=false`。
- **证据**: design-summary.md:237-241（`GET /api/config/entries`、`GET /api/feature-flags/{flag}` 需要令牌）；design-summary.md:222（却又说「公开接口，没有令牌」——**自相矛盾**）；`code/crates/platform-rbac/src/http.rs:187-193`（缺头 → 令牌推断）；`code/crates/platform-core/src/extension/permission.rs:319-323`（缺 tenant → 401）；acceptance.md:42、:146-150、:177。
- **建议**: 明确三选一：(a) 这些步骤**只在单元层**（`RequestContext::new()`）验证，HTTP 层断言 401；(b) 让 flag 读接口变成**公开**接口（与 design §6 冲突，要改设计）；(c) 增加「平台级读取」的显式入口。本用例集按 (a) 写（TC-010-003 / 020 / 021）。

### 8.2 判据不成立 / 缺可判定判据

#### Q-08: AC-S-053 与 AC-S-049 的夹具互斥（T1 的 `logo` 已被租户级覆盖）
- **严重度**: 高
- **AC**: AC-S-053
- **问题**: 049 规定 T1 覆盖 `logo=B`；053 前置要求「T1、T2 各自读过**平台级** `logo=A`」。T1 永远命中 `tenant`，**读不到 A** —— 前置不可能同时成立。
- **证据**: acceptance.md:38（T1 覆盖 B）对照 :101（T1 读过平台级 A）。
- **建议**: 053 换一个 T1 **没有**租户覆盖的键（如 `favicon`），或删掉 049 对 T1 的 `logo` 覆盖。本用例集用 `favicon`（TC-010-034）。

#### Q-09: AC-S-054 步骤 3 缺 `X-Tenant-Id`，判据不成立
- **严重度**: 硬伤
- **AC**: AC-S-054
- **问题**: 步骤 3 只写「不带令牌，但带 `X-Space-Id: S1`」，**没带** `X-Tenant-Id`。这样一来根本没有租户上下文，「返回租户级/平台级品牌」与「返回平台默认」**无法区分**：一个完全忽略 `X-Space-Id` 的实现（哪怕它也忽略租户）照样通过。
- **证据**: acceptance.md:118-124。
- **建议**: 改为「`X-Tenant-Id: tenant-a` + `X-Space-Id: space-sales-1` → 返回 **B（租户值）**，并显式断言 **≠ C（空间值）**」。本用例集 TC-010-015 按此写。

#### Q-10: AC-S-054 步骤 1 只断言「只含白名单键」，不断言值
- **严重度**: 高
- **AC**: AC-S-054
- **问题**: 平台默认品牌同样"只含白名单键"。若不要求 `logo == B`，一个**完全不做租户解析**的实现也能通过 —— 而"登录页按租户显示 logo"正是本 AC 的目的。
- **证据**: acceptance.md:121。
- **建议**: 补值断言（`logo = T1 的租户值`、`title = T2 的租户值`）。本用例集 TC-010-012 / 016 已按值写。

#### Q-11: 品牌接口的响应形状未定义
- **严重度**: 高
- **AC**: AC-S-054
- **问题**: 「检查响应体里出现的**全部**键名」只有在**扁平对象**下才可判；若实现返回 C9 的条目数组 `[{key,value,source}]`，键名出现在 `key` 字段里，任何"逐条枚举"的断言都要重写。design §6.1 只列约束，没给形状。
- **证据**: design-summary.md:216-224；契约 C9 的示例是单条 `{key,value,source}`。
- **建议**: 在 design 里写死形状（建议扁平 map，因为它的用途是"直接渲染"）。本用例集按扁平 map 写（§7 A5）。

#### Q-12: `source` 与 `scope` 共用一个枚举 —— 加 `default` 会把 `default` 变成合法写入作用域
- **严重度**: 硬伤（实现陷阱）
- **AC**: AC-S-049
- **问题**: 真实代码里 `ConfigEntry.source` 的类型是 `ConfigScope`；而 `ConfigScope` 同时是 `ConfigSpec.scope` 和写入作用域的类型。若为了 `source="default"` 直接给 `ConfigScope` 加一个 `Default` 变体，manifest 就能写 `"scope": "default"`（serde `lowercase` 直接放行），写入接口也会接受「第 4 种作用域」—— 等于**新增一个可写层**。
- **证据**: `code/crates/platform-core/src/config.rs:65-71`（`ConfigScope` + `rename_all = "lowercase"`）、`:117-123`（`ConfigEntry.source: ConfigScope`）、`code/crates/platform-core/src/extension/manifest.rs:157-158`（`ConfigSpec.scope: ConfigScope`）。
- **建议**: 新增独立枚举 `ConfigSource { Platform, Tenant, Space, Default }` 专用于 `source`；`ConfigScope` 保持三值。**这是 AC-S-049 的前置条件，应写进 design。**

#### Q-13: design 说「`ConfigSpec.scope` 默认 `platform`」与真实代码不符（实为 `tenant`）
- **严重度**: 中（但会直接造成"空间写被 400"）
- **AC**: AC-S-050
- **问题**: design §3.2 写「`ConfigSpec.scope` 现在**只有一个值（默认 `platform`）**」。真实代码的缺省值是 `ConfigScope::Tenant`，schema 也是 `"default": "tenant"`。如果品牌键漏写 `"scope": "space"`，空间级写会被作用域规则拒成 400 —— 与 AC-S-050 步骤 4「成功」相反。
- **证据**: `code/crates/platform-core/src/extension/manifest.rs:213-215`；`code/crates/platform-core/src/extension/manifest_schema.rs:71`；design-summary.md:114。
- **建议**: 修正 design 措辞；并在 design 的 manifest 片段里对每个品牌键**显式**写 `"scope": "space"`。

#### Q-14: AC-S-051 未指定写入作用域 —— 类型校验会被「越级写」顶包
- **严重度**: 高
- **AC**: AC-S-051
- **问题**: `cacheTtlSeconds` 是 `scope: platform`。若把 `"60"` 写到**租户级**，会先因越级被 400 —— 即使类型校验完全没实现，用例也会"通过"。步骤 2/3/4 都没写 scope。
- **证据**: `code/crates/platform-rbac/manifest.json`（`cacheTtlSeconds.scope = "platform"`）；acceptance.md:73-81。
- **建议**: 明确「在**平台级**写」（需要 `config:platform:write`）。本用例集 TC-010-009 已按平台级写。

#### Q-15: 写入校验的判定顺序未定义（同一请求可得三种 400）
- **严重度**: 高
- **AC**: AC-S-050 / AC-S-051
- **问题**: 「未登记键 + 越级 + 类型错」可以同时成立。AC 只要求 400 + "错误信息说明"，没有顺序，于是"被拦住"可以来自任意一条规则 —— 无法证明具体规则生效。
- **证据**: acceptance.md:78-81、:64-68。
- **建议**: 写死顺序：**注册表 → 作用域 → 类型**（权限点判定在更外层）。本用例集 §7 A9 + TC-010-011 按此写。

#### Q-16: 400 的稳定错误码未定义
- **严重度**: 高
- **AC**: AC-S-050 / AC-S-051 / AC-S-057
- **问题**: 现有稳定码只有 4 个（`UNAUTHENTICATED` / `INSUFFICIENT_PERMISSION` / `NOT_FOUND` / `INTERNAL_ERROR`），**没有 400 码**。C6 明确「`code` 是稳定的机器可读错误码，前端据此做逻辑判断」—— 没有码就只能靠 `detail` 文本匹配，不可判定也不稳定。
- **证据**: `code/crates/platform-core/src/problem.rs:33-39`；契约 C6（`BA/contracts/api-conventions/v2.md` 沿用 v1 C6）。
- **建议**: 定 4 个码（§7 A3），并写进 C9 v2 或本需求的 API 说明。

#### Q-17: AC-S-057 步骤 5 的「400」没说是写时还是读时
- **严重度**: 高
- **AC**: AC-S-057
- **问题**: 若是**写时**拒绝，读路径永远看不到 101，这条只验了写校验；若是**读时**返回 400，则读的客户端（没有发这个值）收到 400，且与 C10「关闭返回 200 + `enabled=false`」的降级精神冲突。
- **证据**: acceptance.md:162、:170；`BA/contracts/api-conventions/v2.md:106-116`。
- **建议**: 明确「400 在 `PUT`；读路径对脏数据 fail closed 为 `enabled=false` + WARN」。本用例集 TC-010-027 按此写。

#### Q-18: 灰度的「合理区间」无数值、命中集合"记录"不是判据、单调性只写单向
- **严重度**: 高
- **AC**: AC-S-057
- **问题**: ① 「命中率落在合理区间（用例给出宽松上下界）」把判据甩给用例，但 AC 本身不可判定；② "命中集合被记录下来"只要求记录，不要求可复现；③ 单调性只写 `10→20`，没写反向（`60→50` 应为子集）与"改回原比例集合复原"。
- **证据**: acceptance.md:165-168。
- **建议**: 写死 n=200 / p=50 的接受区间（建议 [70,130]），要求两次运行集合完全一致，并补「60→50 为子集」「改回 50 集合复原」。本用例集 TC-010-024 / 025 按此写。

#### Q-19: flag 的 `source` 语义在 design §5.1 与 C9 v2 §9.3 之间自相矛盾
- **严重度**: 高
- **AC**: AC-S-056 / AC-S-059
- **问题**: design §5.1 说「**无任何配置** → `enabled=false`，`source="platform"`」；C9 v2 §9.3 说「谁都没配、用 manifest 默认值 → `source="default"`」。同一个状态（flag 有 manifest 默认 `false`、三层都没写）得到两种 `source`。未声明的 flag 更没定义。
- **证据**: design-summary.md:196；`BA/contracts/api-conventions/v2.md:53-58`。
- **建议**: 声明 flag 也走 C9 的四值（有默认 → `default`；未登记 → `platform` 或不返回 `source`，二选一写死）。本用例集只在"显式租户级"断言 `source="tenant"`，其余不断言（TC-010-029 第 4 条）。

#### Q-20: flag 名的规范化未定 —— 同一 flag 两种写法会得到不同 bucket
- **严重度**: 高
- **AC**: AC-S-057
- **问题**: 哈希输入是 `"<flag>:<user_id>"`。C10 的示例写 `new-invoice`（kebab）、AC-S-056 写 `newInvoice`（camelCase）、C9 要求键名 camelCase。若实例里登记的是 `newInvoice` 而客户端按 `new-invoice` 调，**bucket 完全不同**（且 API 还会把它当"未声明 flag"返回 false）。这不是理论问题：灰度比例会因此失效。
- **证据**: `BA/contracts/api-conventions/v2.md:113`（`"flag": "new-invoice"`）对照 acceptance.md:145（`newInvoice`）与 v2 §9.1（camelCase）。
- **建议**: 写死 flag 名规范（camelCase），并统一 C10 的示例；同时规定"未声明 flag"的判定必须按**登记名精确匹配**，不做大小写/连字符归一化（否则同名不同写法会互相串）。

#### Q-21: AC-S-056 步骤 1 的「`source="tenant"`（**若**配在租户级）」是条件式预期
- **严重度**: 中
- **AC**: AC-S-056
- **问题**: 「若」字让预期不可判定：执行者不知道 T1 的 flag 到底配在哪一层；若实现把 flag 配在平台级，`enabled=true` 也成立而 `source` 却是 `platform` —— 用例只能"看情况过"。
- **证据**: acceptance.md:148。
- **建议**: 删掉"若"，写死「T1 的 `newInvoice` 存在 `platform_tenant_config`（`source=tenant`）」。

#### Q-22: AC-S-059 未给 flag 名的字符集/长度边界
- **严重度**: 高（攻击面）
- **AC**: AC-S-059
- **问题**: 未声明 flag 一律 `200 + 告警日志`。若 flag 名含换行、超长（如 1KB）、或路径片段，服务端会把原文写进日志 —— 未认证/低权限用户可以**伪造日志行**或刷爆日志（告警风暴）。AC 没有任何边界。
- **证据**: acceptance.md:191；`BA/contracts/api-conventions/v2.md:154-160`（未声明 → 200 + WARN，含 flag 名）。
- **建议**: 规定 flag 名 `^[a-z][a-zA-Z0-9]{0,63}$`；**非法名 → 400**（与"合法但未声明 → 200"区分）；日志字段截断并结构化。本用例集 TC-010-030 按此写。

#### Q-23: 品牌白名单的 `theme.*` 是通配 —— 未来任何 `theme.xxx` 自动进公开接口
- **严重度**: 中
- **AC**: AC-S-054
- **问题**: 白名单按前缀放行。今天 `theme.*` 只有展示类；明天有人加一个 `theme.debugToken` / `theme.internalApiBase`，它会**自动**出现在无令牌的公开响应里。这与"白名单是显式枚举、fail closed"的初衷相反。
- **证据**: design-summary.md:222（`theme.*`）。
- **建议**: 改成枚举具体键（`theme.radius` / `theme.mode` / `theme.density` / `theme.fontStack`），或规定"前缀匹配 + 键名后缀白名单"。本用例集 §7 A13 按前缀写，但 TC-010-013 的集合断言会在新增键时失败（预期行为）。

#### Q-24: AC-S-060 漏掉两类攻击路径：未认证写（401）与跨租头写（404）
- **严重度**: 高（漏攻击面）
- **AC**: AC-S-060
- **问题**: 五步全是"已认证用户 + 正确租户头"，只验了"缺权限点 → 403"。没有验「没有令牌 → 401」与「令牌是 T1、头写 T2 → 404」。前者决定接口是否裸奔；后者决定能不能借请求头把写入导到别的租户。
- **证据**: acceptance.md:197-208。
- **建议**: 补两步。本用例集 TC-010-037 / 038 已补（派生）。

#### Q-25: AC-S-055 的扫描范围漏掉真实的硬编码位置（`packages/theme` 与 `app.css`）
- **严重度**: 硬伤（AC 的判据覆盖不到要改的代码）
- **AC**: AC-S-055
- **问题**: AC 只说查 `apps/shell/index.html` 与 `apps/shell/src/**`。但真正的品牌色硬编码有两处：`apps/shell/src/app.css:16`（在范围内）与 **`packages/theme/src/index.ts:21`（不在范围内）**。后者正是"主题令牌"的来源 —— 不改它，`defaultTheme()` 仍会给出 `#1677ff`，"无硬编码"不成立。
- **证据**: `code/apps/shell/index.html:6`；`code/apps/shell/src/app.css:16`；`code/packages/theme/src/index.ts:21`；acceptance.md:132。
- **建议**: 扫描范围加上 `packages/theme/src/**`，或明确"本期不动 theme 包"（则 AC 的措辞要收窄到"shell 不再引用硬编码值"）。

#### Q-26: AC-S-055 未提「去硬编码会让现有测试变红」
- **严重度**: 中
- **AC**: AC-S-055
- **问题**: `e2e/shell.spec.ts:5` 断言 heading 名叫 `底座壳`；`App.svelte:16` 的 `<h1>` 与 `:9` 的文案也是 `底座壳`；`packages/theme/test/theme.test.ts:8` 断言默认主色格式。AC 要求"硬编码值消失"，但没说这些测试要同步改 —— Dev 要么留硬编码（AC 不过），要么测试红（CI 不过）。
- **证据**: `code/e2e/shell.spec.ts:5`；`code/apps/shell/src/App.svelte:9,16`；`code/packages/theme/test/theme.test.ts:8`。
- **建议**: AC 补一句「同步更新受影响的 e2e/单测」，并明确 `<h1>` 的产品名算不算品牌值（本用例集按"算"处理：TC-010-017 要求 `底座壳` 命中数为 0）。

#### Q-27: AC-S-055 步骤 2/3 在当前开发形态下跑不通（没有 CORS，且不同源）
- **严重度**: 硬伤（环境前置不成立）
- **AC**: AC-S-055
- **问题**: shell 的 API base 写死 `http://127.0.0.1:8080`；Playwright 预览在 `:4173`；服务端**没有任何 CORS 层**（全仓 `grep CorsLayer` 0 命中）。浏览器会拦掉跨源 fetch → 品牌取不到 → 标题/主色永远不会变。AC 把这个当成"刷新页面看标题"。
- **证据**: `code/apps/shell/src/App.svelte:8`；`code/playwright.config.ts:9-13`；`code/crates/platform-server/src/main.rs`（无 `CorsLayer`）。
- **建议**: 二选一写死：(a) E2E 把 `apps/shell/dist` 挂到 `platform-server` 静态目录、**同源**访问；(b) 平台补 CORS（并限制 origin）。本用例集 TC-010-018 按 (a) 写。

#### Q-28: AC-S-062 的「表级权限只授予平台管理角色」——该角色不存在；且默认授权已把写权开给 `base_app`
- **严重度**: 硬伤
- **AC**: AC-S-062
- **问题**: ① 仓库里没有「平台管理角色」（只有 `base_platform_app` 组与 `base_app` 登录角色）。② `deploy/seed/02-app-role.sql` 对 public **所有表**授予 `SELECT/INSERT/UPDATE/DELETE`，并用 `ALTER DEFAULT PRIVILEGES` 让**之后新建的表**自动继承 —— 所以新建的 `platform_config` **默认就能被租户连接 INSERT/UPDATE/DELETE**。AC 的预期（被拒）与现状（成功）相反；而修好它又要和 AC-S-060 步骤 5（运行时写得进）对齐（Q-01）。③ AC 只写了 INSERT，UPDATE/DELETE 同样能改平台配置。
- **证据**: `code/deploy/seed/02-app-role.sql:49-55`；`code/crates/platform-server/src/main.rs:103-108`；acceptance.md:230-237。
- **建议**: 在迁移里显式 `REVOKE INSERT, UPDATE, DELETE ON platform_config FROM base_platform_app`（保留 SELECT），并定下平台级写的通道（Q-01）；AC 的判据同步覆盖 UPDATE/DELETE。

#### Q-29: AC-S-062 步骤 5 的「down 可逆」在空表上是空断言
- **严重度**: 高
- **AC**: AC-S-062
- **问题**: `platform_tenant_config` 当前**空表**，且**没有任何 Rust 代码读写它**（全仓 grep 0 命中）。空表上 `TEXT→jsonb→TEXT` 永远"可逆"，证明不了值语义不丢。另外既有 `20261118000200_tenant_isolation.down.sql` 直接 `DROP TABLE` —— 新迁移的 down 若照抄就会把历史表删掉。
- **证据**: acceptance.md:237；`code/migrations/20261118000200_tenant_isolation.down.sql:3-4`；`grep -rn platform_tenant_config code/crates code/modules`（0 命中）。
- **建议**: 判据改成「先插 3 种 JSON 类型值 → up/down/up → `jsonb_typeof` 与语义不变」，并规定新迁移的 down **不得** `DROP TABLE platform_tenant_config`。本用例集 TC-010-046 按此写。

#### Q-30: 三张新表的 DDL 未定义（主键 / `space_id` 是否 NOT NULL / `updated_at`）
- **严重度**: 中
- **AC**: AC-S-062
- **问题**: design §2 只有列级描述，没有 DDL；AC 只验"类型是 jsonb + RLS"。主键不写死会导致同一 `(tenant, space, key)` 出现多行（"最后一次写入"变成不确定），`space_id` 可空又会重新引入"单表 nullable"的语义混乱。
- **证据**: design-summary.md:49-53；acceptance.md:222-239。
- **建议**: 在 design 给出三张表的 DDL 片段（建议 `platform_space_config` 的 `space_id NOT NULL`、PK `(tenant_id, space_id, key)`；`platform_config` PK `key`）。

#### Q-31: 契约基线写的是 v1，但 `CURRENT` 已是 v2，且 v2 已包含本需求要"回写"的 4 处变更
- **严重度**: 中（文档一致性，会让 Dev pin 错版本）
- **AC**: 全（引用层）
- **问题**: acceptance.md 与 design-summary.md 都把契约指向 `api-conventions v1`；design §9 还把「C9 增 `default`、增作用域约束、增类型校验、C10 增确定性」列为**待本需求内完成**的回写。实际上 `CURRENT` 已是 **v2**，那 4 条**已经 ratified**。Dev 若按 v1 实现，就会漏掉 `source=default` 与作用域/类型约束。
- **证据**: `BA/contracts/api-conventions/CURRENT`（内容 `v2`）；`BA/contracts/api-conventions/v2.md:12-20`（4 条变更）；acceptance.md:7；design-summary.md:6、:302-312。
- **建议**: 把两处引用改成 v2，design §9 标"已完成"；`feature-meta/manifest.json` 的 pin 同步到 v2。

#### Q-32: `ConfigResolver` trait 的位置给了两处，且其中一处不会被能力索引收录
- **严重度**: 中
- **AC**: AC-S-061（间接）/ design §1
- **问题**: design §1 说能力接口放 `crates/platform-core/src/extension/config.rs`（与 `permission.rs` 同级），紧接着又说把 `src/config.rs` 里现有的 `ConfigResolver` 改造成 trait。同一类型两个目标文件，Dev 只能猜。另外能力索引的 `source` 是 `crates/platform-core/src/extension/` —— 放 `src/config.rs` 就不会被 `cargo xtask capabilities` 收录，`extension_point_count` / `platform-capability` 列表会与设计不符。
- **证据**: design-summary.md:26-41；`code/crates/platform-core/src/extension/mod.rs:37-50`（无 `config` 模块）；`code/crates/platform-core/src/lib.rs:12`（`ConfigResolver` 从 `crate::config` 导出）；`code/capabilities/extension-points.json` 的 `source`。
- **建议**: 写死「trait + `ConfigSource` 放 `extension/config.rs`；`src/config.rs` 只留 `AppConfig` + re-export」，并列出联动改动点（`extension/mod.rs`、`lib.rs`、`xtask/catalog.rs` 的摘要文案、重生成 `capabilities/`）。

#### Q-33: 「三个权限点在 manifest 登记」不够 —— 不同时加种子迁移，进程**起不来**
- **严重度**: 硬伤（会直接阻塞 AC-S-060 的全部用例）
- **AC**: AC-S-060
- **问题**: 启动时 `reconcile_permissions` 拿 manifest 声明集合与 `platform_permission` 表**双向对账**，不一致就**退出进程**（fail-fast）。只在 `crates/platform-config/manifest.json` 声明 3 个权限点、不写种子迁移，服务根本不会启动。
- **证据**: `code/crates/platform-server/src/main.rs:424-442`；`code/migrations/20261118000901_rbac_permission_seed.up.sql:7-13`（「忘了加 INSERT → 启动失败」）；`code/crates/platform-rbac/src/registry.rs:100-130`（会扫 `crates/*/manifest.json`）。
- **建议**: AC-S-060 补「manifest + 权限点种子迁移 + 重生成 `capabilities/permissions.json`」三件套；并把迁移文件名写进 design。

### 8.3 攻击面 / 边界补充

#### Q-34: AC-S-050 的「`scope` = 允许最深层」与 design §4.1 的「空间级只对展示类白名单键生效」给出相反结论
- **严重度**: 中
- **AC**: AC-S-050 / AC-S-054
- **问题**: 050 说「登记的 `scope: space` 就允许空间级写（步骤 4 成功）」；design §4.1 说「空间级覆盖**只对**展示类键（白名单）生效，其余**不得**生效」。对一个"登记为 `space` 但不在白名单"的键：按 050 应 200，按 §4.1 应不生效（写是 200 后被忽略，还是 400？也没说）。当前夹具里没有这种键，所以冲突是**潜伏**的。
- **证据**: acceptance.md:64-68；design-summary.md:149-158。
- **建议**: 写死「空间级写 = `scope: space` **且** 键在白名单，否则 **400**（fail closed）」，并把白名单变成注册表的一部分（配置键的一个字段），而不是散落在 design 正文里。

#### Q-35: 读接口缺「键级授权」——任何已认证用户都能读平台级运维配置
- **严重度**: 中（信息泄露面）
- **AC**: AC-S-049（缺口）
- **问题**: `GET /api/config/entries` 只要求令牌，不要求权限点。于是任何租户的普通用户都能读 `scimBaseUrl` / `ldapBaseDn` / `conflictStrategy` 这类**平台级基础设施配置**。AC-S-054 花了很大力气把 `scimBaseUrl` 挡在**公开**接口外，但同一个人登录后就能从 `entries` 读到它。
- **证据**: design-summary.md:237（只写"需要令牌"）；`code/crates/platform-org/manifest.json`（`scimBaseUrl` / `ldapBaseDn` / `conflictStrategy` 都是 `scope: platform`）。
- **建议**: 明确读接口的授权模型：要么所有已认证用户可读（则 AC-S-054 的白名单只是"匿名可见"的子集，需在文档里承认）；要么读平台级键要求 `config:platform:write`（或新增 `config:platform:read`）。本用例集 TC-010-013 的对照读用 P，避免把"普通用户可读"写成既定事实。

#### Q-36: 批量读遇到未登记键：整请求 404 还是跳过？
- **严重度**: 中
- **AC**: AC-S-049
- **问题**: AC-S-049 步骤 5 只给了**单键**读的 404 语义；design §6 的接口是**批量**（`?keys=a,b`）。`keys=logo,notRegistered` 时整请求 404 还是返回 `logo` + 忽略未知键？两种都"合理"，但前端行为完全不同（整页降级 vs 部分可用）。
- **证据**: acceptance.md:45、:52；design-summary.md:236。
- **建议**: 写死一种（建议**整请求 404** + `params.unknownKeys`，fail closed，且与单键语义一致）。本用例集 TC-010-004 第 4 步按此写。

---

## 9. 建议执行顺序

| 阶段 | 范围 | 说明 |
|---|---|---|
| **第一批**（无库、纯静态/单元） | TC-010-003（第 1 步）、TC-010-004（键名断言）、TC-010-017（静态扫描）、TC-010-041、TC-010-042 | 不连库；042 依赖「平台 crate 的 C-5 入口」先落地（Q-04） |
| **第二批**（真实库 + 真机 HTTP，配置与品牌） | TC-010-001、002、005 ~ 016、020 ~ 022、029 ~ 031、036 ~ 039 | 需先完成 §3 夹具 + 权限点种子迁移（Q-33）；应用连接用 `base_app` |
| **第三批**（缓存收敛 + 灰度统计） | TC-010-023 ~ 028、TC-010-032 ~ 035 | 两个独立实例共享 `base_test`；TC-010-035 是 design §7.1 的**缺陷门禁**，会先红 |
| **第四批**（DB 权限 / RLS / 迁移） | TC-010-043 ~ 046 | 需先做 Q-01 / Q-28 的 `REVOKE` 与平台写通道；046 需 down/up 各跑一次 |
| **第五批**（浏览器） | TC-010-018、019 | 需前端构建 + **同源部署**（或 CORS，Q-27） |

**建议的 CI 门禁**：第一批 + 第二批中不依赖起进程的断言 + 第四批。第三批的 TC-010-035 与第四批的 TC-010-043 是**回归价值最高的两条**（分别锁住 epoch 失效与平台表写侧），建议直接进门禁。浏览器两条留夜间/手动。

---

## 10. 自检记录（交付前）

| 检查项 | 结果 |
|---|---|
| 14 条 AC 全覆盖，矩阵与各用例「关联 AC」逐条一致 | ✅（14/14；46 条 TC；矩阵中的 TC 编号与文件内编号逐条核对） |
| 用例编号连续、无重复 | ✅ TC-010-001 ~ TC-010-046，无跳号 |
| 9 字段齐备率 100% | ✅ 46/46（脚本按 `- **关联 AC**` / `- **前置条件**` 等 9 个字段统计） |
| 所有夹具 id 在 §3 有定义 | ✅ 租户 T1/T2/TX、空间 S1/S2/SX、用户 u-2001~2005 / u-3001~3200 / u-1001、键 17 个、flag 3 个、表 4 张 |
| 相对链接全部可达 | ✅ 逐个核验（见下） |
| 「验收标准问题清单」非空且每条有证据 | ✅ 36 条（硬伤 12 / 高 14 / 中 10），均有文件:行号或原文对照 |
| 不发明 API（路径按 design §6） | ✅ 6 条路由逐条照抄；未定义的 body/形状在 §7 登记为假设 |
| 攻击向用例有对照 | ✅ 每类拒绝都配了 200 的对照组（TC-005/008/010/013/020/028/036/039/040/043） |
| 断言用精确错误码 | ✅ 401/403/404 用现有码；400 用 §7 A3 登记的 4 个码 |

**链接核验清单**（自 `test-cases/` 出发）：`../acceptance.md`、`../design-summary.md`、`../demand.md`、`../status.md`、`../../../contracts/api-conventions/v2.md`、`../../../contracts/extension-points/v2.md`、`../../REQ-001-platform-foundation/test-cases/index.md`、`../../REQ-001-platform-foundation/decisions.md`、`../../REQ-009-rbac/test-cases/index.md`、同目录 `A-config-scope.md` / `B-branding.md` / `C-feature-flags.md` / `D-security-cache.md` —— 全部存在。
