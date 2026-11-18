# 设计摘要：配置、品牌与 Feature Flag（REQ-010）

- **需求编号**: REQ-010
- **父需求**: [REQ-001](../REQ-001-platform-foundation/demand.md)
- **依赖**: [REQ-006 多租户与隔离](../REQ-006-tenant/design-summary.md)、[REQ-009 RBAC](../REQ-009-rbac/design-summary.md)
- **契约**: [api-conventions **v2**（C9 配置键与响应、C10 Feature Flag、C3 上下文请求头、C6 错误体）](../../contracts/api-conventions/v2.md)、[extension-points v2（C-5 鉴权调用点）](../../contracts/extension-points/v2.md)
- **性质**: 需求细化附件 —— **本迭代第一次让「配置」成为可运行时变更的数据**，不做成编译期常量
- **更新时间**: 2026-11-18 23:15（**rev 2**：经 QA 用例评审挑出 36 条问题后修订，见决策 D55~D68）

---

## 0. 一句话

把「底座的视觉与行为」从**代码里的常量**变成**按作用域分层的数据**：
平台默认 → 租户覆盖 → 空间覆盖，改完**下一个请求**就生效；
功能开关按租户与灰度比例**确定性地**判定，而不是每次请求掷骰子。

---

## 1. 放在哪：`crates/platform-config`（平台 crate）

与 `platform-org` / `platform-rbac` 同级。理由同前两个：
- 配置接口要挂在 `platform-server` 上 → 平台 crate 才能被 server 依赖（`check-deps` 禁止 server 依赖业务模块）
- 业务模块**通过能力接口**读配置，不直接依赖 `platform-config` 的内部

业务模块侧只需要一个能力接口，放在 **`crates/platform-core/src/extension/config.rs`**
（与 `permission.rs` 同级同分类：`kind = "platform-capability"`，**不是扩展点**，见 §6.3）：

```rust
pub trait ConfigResolver: Send + Sync + 'static {
    /// 解析一个配置键在当前上下文（租户 / 空间）下的有效值与其来源作用域
    fn resolve(&self, ctx: &RequestContext, key: &ConfigKey) -> Option<ResolvedConfig>;
    /// 解析一个功能开关（含灰度比例）
    fn flag(&self, ctx: &RequestContext, flag: &str) -> FlagDecision;
}
```

> ⚠️ **与既有代码的冲突要处理（D63）**：`crates/platform-core/src/config.rs` 里**已经有一个**
> `ConfigResolver`（REQ-002 的骨架，纯内存 `Vec<ConfigEntry>`，无 DB、无缓存）。
>
> **位置写死**（初版给了两处，Dev 只能猜）：
>
> | 内容 | 放哪 |
> |---|---|
> | `ConfigResolver` trait、`ConfigSource`、`ResolvedConfig`、`FlagDecision` | **`platform-core/src/extension/config.rs`（新建）** —— 能力索引只扫这个目录，放别处不会被收录 |
> | `AppConfig`（进程级配置） | 留在 `platform-core/src/config.rs` |
> | `InMemoryConfigResolver`（骨架实现） | 留在 `platform-core/src/config.rs` |
>
> 联动改动点（**逐项**）：`extension/mod.rs` 加 `pub mod config;` + re-export、
> `lib.rs` 的导出调整、`xtask/src/catalog.rs` 的摘要文案、重生成 `capabilities/`。
> 这是一次**迁移**，不是新增同名类型 —— Dev 要 grep 全部引用点。

**依赖方向**：`platform-core`（定义 trait）← `platform-config`（实现）← `platform-server`（装配）。

---

## 2. 三张表，**不做单表 nullable**

| 表 | 作用域 | `tenant_id` | `space_id` | RLS | 主键 |
|---|---|---|---|---|---|
| `platform_config` | 平台级 | **无** | 无 | **无**（同 `platform_meta`） | `key` |
| `platform_tenant_config` | 租户级 | **有** | 无 | **有**（REQ-006 已建） | `(tenant_id, key)` |
| `platform_space_config` | 空间级 | **有** | **有（NOT NULL）** | **有**（新建） | `(tenant_id, space_id, key)` |

**主键必须写死**：不写死会导致同一 `(tenant, space, key)` 出现多行 ——
「最后一次写入生效」变成不确定行为。`space_id` **不允许可空**：
可空会重新引入「单表 nullable」那种语义混乱（「没有 space」与「space 为空串」两义）。

### 2.0 DDL（照此实现）

```sql
-- 平台级：无 tenant_id、无 RLS
CREATE TABLE IF NOT EXISTS platform_config (
    key        TEXT   PRIMARY KEY,
    value      JSONB  NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- 租户级：已存在，本需求把 value 从 TEXT 改成 JSONB
--   ALTER TABLE platform_tenant_config ALTER COLUMN value TYPE JSONB USING value::jsonb;
--   ⚠️ down 迁移**不得** DROP TABLE（既有 20261118000200 的 down 就是 DROP，
--      照抄会把 REQ-006 的历史表删掉）

-- 空间级：tenant_id + space_id 都 NOT NULL
CREATE TABLE IF NOT EXISTS platform_space_config (
    tenant_id  TEXT   NOT NULL,
    space_id   TEXT   NOT NULL,
    key        TEXT   NOT NULL,
    value      JSONB  NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (tenant_id, space_id, key)
);
```

### 2.1 为什么不做「一张表 + 两个可空列」

看起来更简洁，但 **RLS 会开一个致命口子**：

```sql
-- ❌ 危险写法
CREATE POLICY cfg ON platform_config
    USING  (tenant_id IS NULL OR tenant_id = current_setting('app.tenant_id', true))
    WITH CHECK (tenant_id IS NULL OR tenant_id = current_setting('app.tenant_id', true));
```

`WITH CHECK` 同样放行了 `tenant_id IS NULL` → **租户 A 可以 INSERT 一行平台级配置**，
影响所有租户。这是「写得进去」的写侧越权，比读侧更严重。

拆成三张表后：平台级表**没有 tenant_id 列**，`WITH CHECK` 只剩「只能写自己租户」这一条语义，不会误开。

### 2.1.1 平台级表的**写通道**：REVOKE 直接 DML + `SECURITY DEFINER` 函数（D56）

初版这里写「租户连接连写都写不进去（表级权限）」——**这是空话**，因为：

| 事实 | 后果 |
|---|---|
| 本仓运行时**只有一个连接池**，角色是 `base_app` | 没有「第二个角色/池」可用 |
| `deploy/seed/02-app-role.sql` 对 public **所有表**授予 `SELECT/INSERT/UPDATE/DELETE` | 新表**默认就有写权** |
| 同一脚本用了 `ALTER DEFAULT PRIVILEGES` | **之后新建的表自动继承**，`platform_config` 一建就可写 |
| 仓库里**没有**「平台管理角色」 | 「只授予平台管理角色」无处落地 |

**正确做法（两条一起才成立）**：

```sql
-- 1) 迁移里显式收回直接 DML（必须，因为 ALTER DEFAULT PRIVILEGES 会自动授予）
REVOKE INSERT, UPDATE, DELETE ON platform_config FROM base_platform_app;
-- 保留 SELECT：平台级配置对所有租户可见（读授权在应用层按 AC-S-063 把关）

-- 2) 唯一的写通道：SECURITY DEFINER 函数（属主是有写权的角色）
CREATE OR REPLACE FUNCTION platform_config_upsert(p_key TEXT, p_value JSONB)
RETURNS VOID
LANGUAGE sql
SECURITY DEFINER
SET search_path = public
AS $$
    INSERT INTO platform_config (key, value, updated_at)
    VALUES (p_key, p_value, now())
    ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value, updated_at = now();
$$;
REVOKE ALL ON FUNCTION platform_config_upsert(TEXT, JSONB) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION platform_config_upsert(TEXT, JSONB) TO base_platform_app;
```

**这条设计的实际保证是什么（别说过头）**：

| 保证 | 强度 |
|---|---|
| 租户作用域的 SQL **不可能**「顺手」写进平台级行（直接 DML 被拒） | ✅ 数据库层强制 |
| 平台级写入**只有一条显式路径**（函数），代码审查只需看这一处 | ✅ 结构强制 |
| 「谁能写平台级配置」的业务判断 | ❌ **在应用层**（`config:platform:write` 权限点）。函数不做权限判断 |

**不要说「数据库保证了租户不能改平台配置」** —— 应用若被攻破，仍可调这个函数。
数据库保证的是「没有意外的写路径」，不是「没有写路径」。

### 2.2 值列用 `jsonb`，不用 `TEXT`

`platform_tenant_config.value` 现在是 `TEXT`（REQ-006 建的）。
配置值有类型（string / number / boolean / array / object，见 `ConfigType`），
存 `TEXT` 就要在应用层双重编码（JSON 字符串塞进 TEXT），类型校验也落在应用层。

**本需求把三张表的值列统一成 `jsonb`**，并给 `platform_tenant_config` 写迁移把 `TEXT` → `jsonb`。
收益：类型由数据库保证、可以直接按键查询/索引、不用 double-encode。
代价：一次列类型迁移（表当前无生产数据，`USING value::jsonb` 即可，**但要写 down 脚本**）。

### 2.3 平台级表的访问路径：沿用 REQ-009 的「伪租户」模式

`platform-db` **只暴露租户作用域的入口**（`TenantDb` 的每个方法都要 `ctx`，
且 `TenantQuery` 强制语句里出现且只出现一次 `{tenant}`）。**没有「平台级访问」入口。**

REQ-009 读平台级表 `platform_permission` 的既有做法（`crates/platform-rbac/src/store.rs`）：

```rust
pub const PLATFORM_SCOPE_TENANT: &str = "platform";

let ctx = RequestContext::new().with_tenant(PLATFORM_SCOPE_TENANT);
let query = TenantQuery::new(format!(
    "SELECT ... FROM {} WHERE {{tenant}} IS NOT NULL ORDER BY ...", self.db.table(TABLE)
));
```

即：用 `"platform"` 这个**伪租户**满足 `TenantQuery` 的硬约束，
SQL 条件写成 `{tenant} IS NOT NULL`（恒真）。对**没有 RLS、没有租户列**的表是安全的 ——
`SET LOCAL app.tenant_id = 'platform'` 对它们不起作用。

**REQ-010 必须沿用这个模式，不得绕过 `platform-db` 直连连接池**（AGENTS.md 硬规则 4）。
两条注意事项：

| 注意 | 说明 |
|---|---|
| **写入不能只靠 `{tenant} IS NOT NULL`** | 对 INSERT 而言这个条件毫无约束力。平台级配置的**写入控制必须来自表级权限**（只授予平台管理角色）+ **权限点**（`config:platform:write`），**不是**靠 SQL 条件 |
| 伪租户值 `"platform"` **不能与真实租户 id 撞车** | 租户 id 是 UUID 形态；`"platform"` 不是合法 UUID，不会冲突。**要有一条测试守住**（若哪天允许自选租户 id，这条就得改） |

---

## 3. 配置键的注册表：来自 manifest（复用既有机制，不另建）

契约 C9 已定：「配置键与默认值在业务模块 `manifest.json` 的 `config` 段声明，平台据此自动生成配置界面」。

所以配置键的**唯一来源**是各 manifest 的 `config` 段：

| 来源 | 例子 |
|---|---|
| `crates/platform-config/manifest.json`（**新建**） | 品牌键 `logo` / `favicon` / `title` / `primaryColor` / `theme.radius` …、功能开关声明 |
| `crates/platform-rbac/manifest.json`（已有） | `cacheTtlSeconds` |
| `crates/platform-org/manifest.json`（已有） | `scimBaseUrl` / `ldapBaseDn` / `conflictStrategy` |
| `modules/sandbox/manifest.json`（已有） | 模块自己的配置 |

**写入未声明的键 → 拒绝**（不是静默接受）。理由：未声明的键既没有类型信息也没有默认值，
一旦允许写入，前端拿到的值无法校验、配置界面也生成不出来。

### 3.1 类型校验：按登记的 `type` 校验值

| 登记的 `type` | 接受的值 |
|---|---|
| `string` | JSON string |
| `number` | JSON number（**不含**布尔、不含数字字符串） |
| `boolean` | JSON `true` / `false` |
| `array` | JSON array |
| `object` | JSON object |

类型不符 → **400**，错误信息给出**期望类型与实际类型**。

### 3.2 作用域约束：登记的作用域决定**允许写到哪一层**

> ⚠️ 初版这里写「`ConfigSpec.scope` 现在只有一个值（默认 `platform`）」——**与代码不符**。
> 真实缺省值是 **`ConfigScope::Tenant`**（`manifest.rs` 的 `default_scope()` 与
> `manifest_schema.rs` 的 `"default": "tenant"`）。
> 后果：品牌键若漏写 `"scope": "space"`，空间级写会被判成越级 → 400。
> **所以品牌键必须显式写 `"scope": "space"`。**

本需求把 `ConfigSpec.scope` 解释为「**允许覆盖到的最深一层**」：

| 登记的 `scope` | 允许写入 | 说明 |
|---|---|---|
| `platform` | 只能平台级 | 租户/空间**不得**覆盖（安全相关配置常属此类，如 `conflictStrategy`） |
| `tenant` | 平台级 + 租户级 | 空间**不得**覆盖 |
| `space` | 平台级 + 租户级 + 空间级（**且键在白名单内**） | 三层都可 |

往**比登记更深**的层写 → **400**（`CONFIG_SCOPE_NOT_ALLOWED`）。
这条把「配置项的作用域」从文档约定变成**强制约束** —— 否则任意租户都能覆盖
本该平台独有的配置（例如把 `conflictStrategy` 改成 `remote_wins` 从而绕过人工裁决）。

**空间级的两道门（D66）**：`scope: space` **且** 在品牌白名单内。
只满足前者 → 写 **400**（fail closed），不是「写进去之后被忽略」。

### 3.3 校验顺序与错误码（D62 / D64）

**顺序写死**，否则「被拦住」可能来自任意一条规则，无法证明具体规则生效：

```
1. 认证（无令牌 → 401）
2. 权限点（缺 → 403）
3. 键名格式（不符合 ConfigKey → 400 CONFIG_KEY_INVALID）
4. 注册表（未登记 → 400 CONFIG_KEY_NOT_REGISTERED）
5. 作用域（越级 → 400 CONFIG_SCOPE_NOT_ALLOWED）
6. 类型（不符 → 400 CONFIG_TYPE_MISMATCH）
   └ 范围（如 rolloutPercent 越界 → 400 CONFIG_VALUE_OUT_OF_RANGE）
```

现有稳定错误码只有 4 个（`UNAUTHENTICATED` / `INSUFFICIENT_PERMISSION` / `NOT_FOUND` /
`INTERNAL_ERROR`），**没有 400 码**。契约 C6 要求「`code` 是稳定的机器可读错误码，
前端据此做逻辑判断」—— 所以本需求新增 **5 个**：

| `code` | HTTP | 何时 |
|---|---|---|
| `CONFIG_KEY_INVALID` | 400 | 键名不符合 camelCase 分段规则 |
| `CONFIG_KEY_NOT_REGISTERED` | 400 / 404 | 写入时 → 400；读取时 → **404** |
| `CONFIG_SCOPE_NOT_ALLOWED` | 400 | 往比登记更深的层写，或空间级写不在白名单 |
| `CONFIG_TYPE_MISMATCH` | 400 | JSON 类型与登记类型不符 |
| `CONFIG_VALUE_OUT_OF_RANGE` | 400 | 数值越界（`rolloutPercent` 不在 `0..=100`） |
| `FLAG_NAME_INVALID` | 400 | flag 名不符合 `^[a-z][a-zA-Z0-9]{0,63}$` |

**同一请求同时违反多条 → 返回顺序最靠前的那条的码**（AC-S-051 步骤 5 专门验这个）。

---

## 4. 三层覆盖与 `source`

契约 C9 已定优先级：**空间 > 租户 > 平台**，响应带 `source` 暴露命中的层级。

| 情形 | 响应 |
|---|---|
| 空间级有值 | `{ "key": "logo", "value": "...", "source": "space" }` |
| 空间级无、租户级有 | `source: "tenant"` |
| 只有平台默认 | `source: "platform"` |
| 都没配 | 用 manifest 登记的 `default`，`source: "default"` |
| manifest 里都没登记 | **404** |

**`source: "default"` 是新增取值**（契约 C9 只列了三个作用域）。
理由：必须有办法区分「平台显式配了这个值」与「谁都没配、用的是 manifest 默认值」——
否则管理员在界面上看不出哪些值是「继承来的」。**这条要回写契约**（升 v2，见 §9）。

### 4.1 空间级配置的**安全边界**（本需求最需要小心的地方）

`X-Space-Id` 是**完全没有校验的请求头**（契约 C3：缺失时视为「不限空间」，不返回错误）。
本仓**没有任何 space 实体表** —— 没有 space 注册表、没有成员关系、无法验证调用者是否属于该空间。

因此两条硬约束：

| 约束 | 理由 |
|---|---|
| **空间级覆盖只对「展示类」键生效** —— 即登记的 `scope = space` 且键名在白名单内（logo / favicon / title / colors / theme.* / footer / copyright） | 未校验的头部**不得**能改变行为（安全/权限/开关类配置）。能把别人空间的品牌换成别的，是低危；能把别人的 `conflictStrategy` 换掉，是高危 |
| **Feature Flag 不支持空间级** | 同上：开关改变行为。未校验的头 + 行为变更 = 越权 |
| 空间级行的读取仍受 **RLS** 约束（`tenant_id` 必须匹配） | 跨租户的 space_id 拿不到他租数据（数据库层兜底） |

> 这是一条**fail closed 的边界**：白名单是**显式枚举**的，不在名单内的键**不会**因为
> 「登记成 space 作用域」就自动获得空间级覆盖能力。

---

## 5. Feature Flag

### 5.1 三档判定

| 配置 | 判定 |
|---|---|
| 无任何配置 | `enabled = false`，`source = "platform"`（**未声明的功能默认关闭**，fail closed） |
| 平台级 `true` / `false`，且无灰度 | 直接取该值 |
| 有灰度比例 `rolloutPercent` | 见 §5.2 |

响应按契约 C10：**`200 + enabled=false`**（不返回 404），带 `source`。

### 5.2 灰度必须是**确定性的** —— 本需求最容易写错的地方

```
bucket = sha256("<flag>:" + "<user_id>") 的前 4 字节按大端解释为 u32
enabled = bucket % 100 < rolloutPercent
```

**为什么不能用随机数**：每次请求掷骰子会让同一个用户在两次请求里得到不同结果 ——
界面一会儿有一会儿没有，同一事务里前后不一致。**必须按用户稳定。**

**为什么用 `sha2`**：它在依赖树里（REQ-007 引入），**不新增依赖**。
⚠️ **不得**用 `std::collections::hash_map::RandomState` —— 它每个进程随机播种，
同一用户在**两个实例**上会得到不同结果（多实例部署下表现为「一半请求命中」）。

**单调性**：`rolloutPercent` 从 10 调到 20 时，原本命中的用户**必须仍然命中**。
这一点由「哈希 user_id 得到固定 bucket，再与阈值比较」自然满足；若实现成「每次重新分配」
就会破坏它 —— 调用方会看到用户来回横跳。

**边界**：`0` → 无人命中；`100` → 全部命中；超出 `0..=100` → **400**。

**无用户身份时**（公开接口，没有令牌）：灰度比例判定**不成立** →
返回 `enabled = false`（fail closed），**不是**「按比例随机给」。

### 5.3 Flag 与灰度比例的声明（D57）

初版这里说「Flag 键也登记在 manifest 的 `config` 段（`type: boolean`）」——
**那样 `rolloutPercent` 无处安放**：

- `ConfigSpec` 只有 `type` / `default` / `scope` / `label`，**没有 rollout 字段**
- 把 `rolloutPercent` 另立一个键 → 因「未声明的键 → 400」被拒
- 塞进 boolean 键 → 类型校验会把数字判成类型错

**解决：派生键**。为每个登记的 flag `F`，平台**自动**认为存在一个配置键：

```
flags.<F>.rolloutPercent    type: number    scope: tenant    范围 0..=100    默认 100
```

| 特点 | 说明 |
|---|---|
| **不需要改 manifest schema** | 派生键由「已登记的 flag」推导，不写在 manifest 里 |
| **不违反「未声明 → 400」** | `flags.beta.rolloutPercent` 是**已声明**的（由 `beta` 派生）；`flags.nosuch.rolloutPercent` 才是未声明 |
| **`scope: tenant`** | 平台与租户都能设；空间不能（D51） |
| 默认 `100` | 只要 `enabled=true` 就全量生效；灰度要显式调低 |

Flag 本体仍登记在 `config` 段（`type: boolean`），语义是「总开关」：

| 判定顺序 | 规则 |
|---|---|
| 1 | flag 本体 `enabled=false` → **不看比例**，直接 `false` |
| 2 | `enabled=true` 且比例为 100 → `true` |
| 3 | `enabled=true` 且比例 < 100 → 按 §5.2 的哈希判定 |

未登记的 flag 名 → `enabled=false` + `source="default"` + **一条结构化 WARN**，**不返回 404**。

**Flag 名的规范化（D65）**：名字必须匹配 `^[a-z][a-zA-Z0-9]{0,63}$`，
**匹配是精确的，不做归一化**（`new-invoice` 与 `newInvoice` 是两个不同的 flag）。
哈希输入用**登记名原文**：`sha256("<登记的 flag 名>:<user_id>")`。

---

## 6. 接口

| 路由 | 鉴权 | 说明 |
|---|---|---|
| `GET /api/config/entries?keys=a,b` | **需要令牌**；平台级键另需 `config:platform:read` | 批量读配置（带 `source`）；含未登记键 → **整请求 404** |
| `GET /api/config/branding` | **公开（无令牌）** | 登录页也要用 → 只返回**白名单键**，扁平 map |
| `PUT /api/config/entries` | 权限点 `config:entry:write` | 写**租户级 / 空间级**配置 |
| `PUT /api/config/platform-entries` | 权限点 **`config:platform:write`** | 写**平台级**配置（**独立路由**，D59） |
| `GET /api/feature-flags` | 需要令牌 | 批量读本租户的开关 |
| `GET /api/feature-flags/{flag}` | 需要令牌 | 单个开关 |
| `PUT /api/feature-flags/{flag}` | 权限点 `config:flag:write` | 改开关（含灰度比例） |

### 6.0 为什么平台级写要**单独一条路由**（D59）

`require_permission(...)` 返回的是**静态路由层**，挂在路由上就对**该路由的所有请求**生效。
若把 `config:platform:write` 也挂到 `PUT /api/config/entries`：

- 只持有 `config:entry:write` 的用户 A **连租户级写都会被 403**（与 AC-S-060 步骤 3 冲突）
- 若改成「在 handler 里按 body 的 scope 判定」，则 `config:platform:write` **没有**
  `require_permission` 调用点 → **C-5 失败**（它只认这个形态）

拆成两条路由后两者同时成立，且权限点仍然可被 C-5 静态扫描到。

### 6.0.1 `source` 与 `scope` 必须是**两个枚举**（D60）

真实代码里 `ConfigEntry.source` 用的是 `ConfigScope`，而 `ConfigScope` 同时是
`ConfigSpec.scope` 与写入作用域的类型（`crates/platform-core/src/config.rs`）。
若为了 `source="default"` 直接给 `ConfigScope` 加 `Default` 变体：

- manifest 就能写 `"scope": "default"`（serde `lowercase` 直接放行）
- 写入接口会接受「第 4 种作用域」→ **等于新增一个可写层**

**所以新增独立枚举 `ConfigSource { Platform, Tenant, Space, Default }`**，
只用于 `source`；`ConfigScope` 保持**三值**。

### 6.1 品牌公开接口的**白名单**（安全边界）

`GET /api/config/branding` **不需要令牌**（登录页在认证前就要显示 logo），因此：

| 约束 | 判据 |
|---|---|
| 只返回**白名单键** —— 见下（**枚举具体键，不用通配**） | 请求任意非白名单键（`conflictStrategy`、`scimBaseUrl`）→ **不返回该键**（不是报错，是缺失） |
| 租户来源：`X-Tenant-Id` 请求头；**缺失时**返回**平台默认品牌** | 不是报错 —— 登录页在不知道租户时也得能渲染 |
| **不得**因为 `X-Space-Id` 而返回空间级品牌 | 未认证请求不带可信的空间身份（§4.1）；此接口**忽略**空间级覆盖 |

**响应形状 = 扁平 map**（D61）：

```json
{ "logo": "/branding/t1.svg", "title": "示例平台", "primaryColor": "#0055aa", "theme.radius": 6 }
```

不是 `[{key,value,source}]` —— 这个接口的用途是**直接渲染**，不是配置查询。
没有 `source` 字段（渲染方不需要它）。

**白名单（D66）**：定义在 `platform-config` 的**代码常量**里，**枚举具体键**：

```
logo / favicon / title / loginBackground / footer / copyright
primaryColor / theme.radius / theme.mode / theme.density / theme.fontStack
```

初版写的是 `theme.*` **前缀通配** —— 那意味着将来任何人加一个
`theme.debugToken` / `theme.internalApiBase`，它会**自动**出现在无令牌的公开响应里，
与「显式枚举、fail closed」的初衷相反。

**不变式（要有测试守着）**：白名单里的每个键都必须在某个 manifest 里登记为 `scope: space`。
反之不必（登记为 `space` 但不在白名单的键 → 空间级写 **400**，见 AC-S-050）。

### 6.2 写接口的鉴权（自举防护）

`PUT /api/config/entries` 与 `PUT /api/feature-flags/{flag}` **必须**要求权限点，
否则任何登录用户都能改全租户的品牌与开关。
管理权限点在 `crates/platform-config/manifest.json` 登记，`platform-admin` 因「注册表全集动态得出」（D31）自动拥有。

**平台级配置的写入权限**：只有持有 `config:platform:write` 的角色能写平台级
（普通租户管理员只能写自己租户的）。这是**跨租户影响**，必须单独一个权限点。

### 6.3 `ConfigResolver` 不是扩展点

与 REQ-009 的 `PermissionChecker` 同理：**平台实现、业务调用**，方向与 5 个扩展点相反。
放 `platform-core/src/extension/config.rs`，能力索引里 `kind = "platform-capability"`，
`extension_point_count` **仍是 5**，`extension-points` 契约**不为它升版**。

---

## 7. 缓存与失效

| 数据 | 缓存位置 | 失效方式 |
|---|---|---|
| 解析后的整套配置（按 租户+空间 维度） | 进程内 | **DB epoch** |
| Feature Flag 判定结果 | 进程内 | 同上 |

### 7.1 epoch 用 `scope_key`，一条查询同时覆盖平台级与租户级

REQ-009 的 `platform_permission_epoch` 主键只有 `tenant_id`，**装不下「平台级变更」**。
那对权限点没问题（权限点注册表由迁移写入，运行期不变），但**平台级配置是运行期会变的** ——
管理员改了平台 logo，**所有租户的缓存都得失效**。

```sql
CREATE TABLE platform_config_epoch (
    scope_key  TEXT PRIMARY KEY,   -- 'platform' 或 'tenant:<tenant_id>'
    epoch      BIGINT NOT NULL DEFAULT 0,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
```

- 写租户级配置 → `scope_key = 'tenant:<id>'` 自增
- 写平台级配置 → `scope_key = 'platform'` 自增
- 读路径：`SELECT scope_key, epoch FROM platform_config_epoch
  WHERE scope_key IN ('platform', 'tenant:<id>')` —— 一条查询、两行

#### ❌ 初版写的 `SELECT max(epoch)` 是**错的**（QA 折出的硬伤）

```
tenant:<T1> 的 epoch = 7   （T1 先改过一次租户级配置）
platform     的 epoch = 3
管理员改平台级配置 → platform 变成 4
max(7, 4) = 7  → 与缓存里的 7 相同 → 判「新鲜」→ 继续返回旧值
```

**平台级变更被更高的租户 epoch 掩蔽了**。只要某个租户先改过自己的配置，
之后**所有**平台级变更对它都失效不掉。

#### ✅ 正确做法：缓存里存**一对** epoch，逐一比较

```
缓存项 = { config, platform_epoch, tenant_epoch }
新鲜 = (缓存.platform_epoch == 库里.platform) && (缓存.tenant_epoch == 库里.tenant:<id>)
```

任一不等 → 重算并写回。**不能取 max、不能取单值、不能只比一个。**

**代价**：仍是一条主键查询（两行），与单行查询同量级。

> ⚠️ 与 REQ-009 的 epoch 表**形状不同**（那张是 `tenant_id` PK）。这是**故意的**：
> 权限注册表不在运行期变，配置会。两张表各自的形状对应各自的失效维度（D48）。
> 但**失效判据的形态**必须按本节来 —— 单表 `max` 是错的。

---

## 8. 本期交付边界

**做**

- [ ] `crates/platform-config`（平台 crate）：三层解析、类型校验、作用域约束、品牌聚合、Feature Flag 判定
- [ ] 三张表 + `platform_tenant_config.value` 的 `TEXT → jsonb` 迁移（含 down，**down 不得 DROP TABLE**）
- [ ] **`REVOKE INSERT/UPDATE/DELETE ON platform_config`** + `platform_config_upsert` 的 `SECURITY DEFINER` 函数（§2.1.1）
- [ ] `platform_config_epoch` + 进程内缓存 + **成对比较**的失效判据（§7.1）
- [ ] 配置键注册表来自 manifest（`crates/platform-config/manifest.json` 新建，声明品牌键与 flag 键，
      **品牌键显式写 `scope: space`**）
- [ ] `ConfigResolver` trait + `ConfigSource` + `ResolvedConfig` + `FlagDecision`
      （放 `platform-core/src/extension/config.rs`，§1 的迁移表）
- [ ] `GET /api/config/entries`（平台级键需 `config:platform:read`）
- [ ] `GET /api/config/branding`（**公开 + 白名单 + 扁平 map**）
- [ ] `PUT /api/config/entries`（租户级/空间级）
- [ ] **`PUT /api/config/platform-entries`**（平台级，**独立路由** + `config:platform:write`）
- [ ] Feature Flag：确定性灰度（`sha2`）、单调性、边界、无身份 fail closed、派生键 `flags.<F>.rolloutPercent`
- [ ] **4 个权限点**：`config:entry:write` / `config:flag:write` / `config:platform:write` /
      **`config:platform:read`**
- [ ] **权限点种子迁移 + 重生成 `capabilities/permissions.json`**
      （只改 manifest 不写种子迁移 → 启动对账 fail-fast，**进程起不来**，§风险 R11）
- [ ] **新增一个不依赖 `ContractFixture` 的 C-5 扫描入口**，让 `crates/platform-config` 能跑 C-5（§6.3）
- [ ] 前端 shell 最小接入：`index.html` 的 `<title>`、`App.svelte` 的 `<h1>`、`app.css` 的主色
      **从接口读**；**同步更新 `e2e/shell.spec.ts`**；E2E 走**同源**（server 静态目录，本仓无 CORS）

**不做**

- 主题令牌的**完整体系**（深浅色、组件密度、字体栈的组件级应用）→ REQ-011
- 配置的管理界面（表单/列表）→ REQ-012 的元数据驱动渲染
- 空间（space）实体与管理：本仓**没有 space 表**，本期只按 `X-Space-Id` 字串维度存配置
- Feature Flag 的 A/B 分流统计、灰度自动推进
- 配置的版本历史与回滚
- 多语言文案（属 REQ-021）
- **把 C-5 扩到平台 crate**（`platform-org` 9 个 / `rbac:permission:read` 的缺口登记为技术债）

---

## 9. 契约：`api-conventions` **已升 v2**（本需求已完成）

`BA/contracts/api-conventions/v2.md` 已 ratified，`CURRENT` 指向 v2。
本需求**已经是 v2 的第一个消费方** —— 初版的 §9 把下面 4 条列为「待本需求内完成」，
**实际已完成**（D54）：

| 变更 | 状态 |
|---|---|
| C9 的 `source` 增加 `default` 取值 | ✅ 已写入 v2 §9.3 |
| C9 增加「作用域约束」：登记的 `scope` = 允许覆盖的最深层 | ✅ 已写入 v2 §9.4 |
| C9 增加「配置值类型校验」+ `jsonb` 存储 | ✅ 已写入 v2 §9.5 / §9.6 |
| C10 增加「灰度确定性 + 无身份 fail closed + 未声明 flag 不 404」 | ✅ 已写入 v2 §10.2~§10.4 |

**Dev 必须 pin `api-conventions: v2`**（`feature-meta/manifest.json`），
并跑 `scripts/sync-contracts.sh --sync` 生成 `docs/contracts/api-conventions/v2.md` 快照。

`extension-points` 仍是 **v2**（本需求不改那 5 个 trait；`ConfigResolver` 是
`platform-capability`，不是扩展点 —— 与 `PermissionChecker` 同理）。

---

## 10. 资源约束

- 本机 7.7G 内存 / ~4.8G 可用；`Cargo.lock` 当前 **294 包**（第三方 registry **284**）
- **不新增第三方依赖**：确定性哈希用已在树里的 `sha2`；JSON 用 `serde_json`；**不引** figment / config-rs / launchdarkly
- 日常命令 `cargo check`；测试 `cargo test -- --test-threads=2`
- **测试只用 `base_test`**；**`base_dev` 绝不写**
- ⚠️ 应用连接必须用普通角色 `base_app`（超管会被 `require_isolation` 拒绝启动）
- ⚠️ `cargo test` **只能有一个 `--`**
- 复用 target：`export CARGO_TARGET_DIR=/home/myg133/Projects/system_seed/code/target`

---

## 11. 风险

| # | 风险 | 应对 |
|---|------|------|
| R1 | **未校验的 `X-Space-Id` 改变行为** | 空间级只对展示类白名单键生效；Feature Flag 不支持空间级（§4.1） |
| R2 | **品牌公开接口泄露他租配置** | 白名单键显式枚举，不在名单内的键**不返回**；空间级覆盖被忽略 |
| R3 | **灰度不确定** → 同一用户横跳 / 多实例不一致 | `sha256(flag:user) % 100`；**禁用 `RandomState`**；单调性与边界有专门用例 |
| R4 | **平台级配置写入开给租户** → 跨租户影响 | 三张表分离 + **REVOKE 直接 DML + `SECURITY DEFINER` 函数**（§2.1.1）+ `config:platform:write` 独立权限点与**独立路由**（§6.0） |
| R5 | **配置缓存不失效** → 改完看不到 | `platform_config_epoch` **成对比较** `(platform, tenant)`；生效上界 = 下一个请求 |
| R6 | **未声明的配置键被写入** → 前端拿到无类型无默认的值 | 写入校验注册表；未登记 → 400 |
| R7 | **作用域越级写** → 租户覆盖平台独有配置 | 按登记的 `scope` 强制限制可写层级（§3.2） |
| R8 | `TEXT → jsonb` 迁移出错 / 不可逆 | down 脚本 + **先插 3 种 JSON 类型值再验 up/down/up**；**down 不得 `DROP TABLE`** |
| R9 | 与既有 `ConfigResolver`（REQ-002 骨架）重名冲突 | 迁移表写死位置（§1）；Dev 要 grep 全部引用点 |
| R10 | 每请求多一次 epoch 查询的代价 | 主键查询（两行），量级 ~0.1ms；实测记录写进报告 |
| **R11** | **只改 manifest 不写权限点种子迁移 → 启动对账 fail-fast，进程起不来** | 交付清单里把「manifest + 种子迁移 + 重生成 capabilities」列为三件套（§8）；`platform_permission` 行由**迁移**写入（同 REQ-009） |
| **R12** | **`ALTER DEFAULT PRIVILEGES` 让新建的 `platform_config` 自动继承写权** | 迁移里**显式** `REVOKE INSERT, UPDATE, DELETE`（§2.1.1）。忘了这步，AC-S-062 步骤 2 会**直接失败**（租户写得进去） |
| **R13** | **`SELECT max(epoch)` 被掩蔽** → 平台级变更失效不掉（初版的设计错误） | 成对比较两个 epoch（§7.1）；AC-S-053 步骤 3 专门构造「租户 epoch 更高」的顺序 |
| **R14** | **`source` 与 `scope` 共用 `ConfigScope`** → 加 `default` 会开出第 4 个可写层 | 新增独立 `ConfigSource`（§6.0.1） |
| **R15** | **单路由 + 静态 `route_layer`** → 平台级权限点要么挡住租户级写、要么无调用点（C-5 失败） | 拆两条路由（§6.0） |
| **R16** | **C-5 在平台 crate 上不可运行**（`ContractFixture` 要求 `Repository`+`PlatformModule`） | 新增不依赖 fixture 的扫描入口（§6.3）；平台既有缺口登记为技术债，**不扩围** |
| **R17** | **开发形态跨源**（shell 在 `:4173`、API 在 `:8080`、全仓无 CORS） | E2E 走 `platform-server` 静态目录的**同源**访问 |
| **R18** | **改硬编码会让既有 e2e/单测变红** | 交付清单要求同步更新 `e2e/shell.spec.ts` 与 `App.svelte`；不允许「留硬编码过 AC」 |
| **R19** | **品牌白名单用前缀通配** → 未来任何 `theme.xxx` 自动进公开接口 | 白名单**枚举具体键**（§6.1），并有「白名单 ⊆ `scope: space` 登记键」的不变式测试 |
