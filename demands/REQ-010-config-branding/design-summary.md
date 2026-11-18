# 设计摘要：配置、品牌与 Feature Flag（REQ-010）

- **需求编号**: REQ-010
- **父需求**: [REQ-001](../REQ-001-platform-foundation/demand.md)
- **依赖**: [REQ-006 多租户与隔离](../REQ-006-tenant/design-summary.md)、[REQ-009 RBAC](../REQ-009-rbac/design-summary.md)
- **契约**: [api-conventions v1（C9 配置键与响应、C10 Feature Flag、C3 上下文请求头、C6 错误体）](../../contracts/api-conventions/v1.md)、[extension-points v2（C-5 鉴权调用点）](../../contracts/extension-points/v2.md)
- **性质**: 需求细化附件 —— **本迭代第一次让「配置」成为可运行时变更的数据**，不做成编译期常量
- **更新时间**: 2026-11-18 22:20

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

> ⚠️ **与既有代码的冲突要处理**：`crates/platform-core/src/config.rs` 里**已经有一个**
> `ConfigResolver`（REQ-002 的骨架，纯内存 `Vec<ConfigEntry>`，无 DB、无缓存）。
> 本需求把它**改造成 trait**，骨架实现保留为 `InMemoryConfigResolver`（测试与非 DB 场景用）。
> 这是一次**重命名 + 迁移**，不是新增同名类型 —— Dev 要检查所有引用点。

**依赖方向**：`platform-core`（定义 trait）← `platform-config`（实现）← `platform-server`（装配）。

---

## 2. 三张表，**不做单表 nullable**

| 表 | 作用域 | `tenant_id` | `space_id` | RLS |
|---|---|---|---|---|
| `platform_config` | 平台级 | **无** | 无 | **无**（同 `platform_meta` / `platform_permission`） |
| `platform_tenant_config` | 租户级 | **有** | 无 | **有**（REQ-006 已建） |
| `platform_space_config` | 空间级 | **有** | **有** | **有**（新建） |

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

拆成三张表后：平台级表**没有 tenant_id 列**，租户连接**连写都写不进去**（表级权限 + 无 RLS 但单独授权），
`WITH CHECK` 只剩「只能写自己租户」这一条语义，不会误开。

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

`ConfigSpec.scope` 现在只有一个值（默认 `platform`）。本需求把它解释为「**允许覆盖到的最深一层**」：

| 登记的 `scope` | 允许写入 | 说明 |
|---|---|---|
| `platform` | 只能平台级 | 租户/空间**不得**覆盖（安全相关配置常属此类，如 `conflictStrategy`） |
| `tenant` | 平台级 + 租户级 | 空间**不得**覆盖 |
| `space` | 平台级 + 租户级 + 空间级 | 三层都可 |

往**比登记更深**的层写 → **400**。
这条把「配置项的作用域」从文档约定变成**强制约束** —— 否则任意租户都能覆盖
本该平台独有的配置（例如把 `conflictStrategy` 改成 `remote_wins` 从而绕过人工裁决）。

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

### 5.3 Flag 的声明

Flag 键也登记在 manifest 的 `config` 段（`type: boolean`）。
未登记的 flag 名 → 判定为 `enabled = false` + **记录一条告警**（便于发现拼错的 flag 名），
**不返回 404**（契约 C10 的理由同样适用：客户端要能优雅降级）。

---

## 6. 接口

| 路由 | 鉴权 | 说明 |
|---|---|---|
| `GET /api/config/entries?keys=a,b` | **需要令牌** | 批量读配置（带 `source`） |
| `GET /api/config/branding` | **公开（无令牌）** | 登录页也要用 → 只返回**白名单键** |
| `PUT /api/config/entries` | 权限点 `config:entry:write` | 写配置（带作用域判定） |
| `GET /api/feature-flags` | 需要令牌 | 批量读本租户的开关 |
| `GET /api/feature-flags/{flag}` | 需要令牌 | 单个开关 |
| `PUT /api/feature-flags/{flag}` | 权限点 `config:flag:write` | 改开关（含灰度比例） |

### 6.1 品牌公开接口的**白名单**（安全边界）

`GET /api/config/branding` **不需要令牌**（登录页在认证前就要显示 logo），因此：

| 约束 | 判据 |
|---|---|
| 只返回**白名单键** —— `logo` / `favicon` / `title` / `loginBackground` / `footer` / `copyright` / `primaryColor` / `theme.*` | 请求任意非白名单键（例如 `conflictStrategy`、`scimBaseUrl`）→ **不返回该键**（不是报错，是缺失） |
| 租户来源：`X-Tenant-Id` 请求头；**缺失时**返回**平台默认品牌** | 不是报错 —— 登录页在不知道租户时也得能渲染 |
| **不得**因为 `X-Space-Id` 而返回空间级品牌 | 未认证请求不带可信的空间身份（见 §4.1）；此接口**忽略**空间级覆盖 |

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
- 读路径：`SELECT max(epoch) FROM platform_config_epoch WHERE scope_key IN ('platform', 'tenant:<id>')`
  —— **一条查询**，`scope_key` 是主键，代价与单行查询相当

**为什么不用两行分别读**：两条查询 = 两次往返，且平台级与租户级之间可能不一致。
`max()` 一条查询天然给出「任一变更都失效」。

> ⚠️ 与 REQ-009 的 epoch 表**形状不同**（那张是 `tenant_id` PK）。这是**故意的**：
> 权限注册表不在运行期变，配置会。两张表各自的形状对应各自的失效维度。
> 不合并 —— 合并会让两套语义纠缠（见 D48）。

---

## 8. 本期交付边界

**做**

- [ ] `crates/platform-config`（平台 crate）：三层解析、类型校验、作用域约束、品牌聚合、Feature Flag 判定
- [ ] 三张表 + `platform_tenant_config.value` 的 `TEXT → jsonb` 迁移（含 down）
- [ ] `platform_config_epoch` + 进程内缓存 + 主动失效
- [ ] 配置键注册表来自 manifest（`crates/platform-config/manifest.json` 新建，声明品牌与 flag 键）
- [ ] `GET /api/config/entries` / `GET /api/config/branding`（**公开 + 白名单**）/ `PUT /api/config/entries`
- [ ] Feature Flag：确定性灰度（`sha2`）、单调性、边界、无身份 fail closed
- [ ] 权限点 `config:entry:write` / `config:flag:write` / `config:platform:write`（C-5 要求有鉴权调用点）
- [ ] 前端 shell 最小接入：`index.html` 的 `<title>` 与 favicon / 主色**从接口读**，去掉硬编码

**不做**

- 主题令牌的**完整体系**（深浅色、组件密度、字体栈的组件级应用）→ REQ-011
- 配置的管理界面（表单/列表）→ REQ-012 的元数据驱动渲染
- 空间（space）实体与管理：本仓**没有 space 表**，本期只按 `X-Space-Id` 字串维度存配置
- Feature Flag 的 A/B 分流统计、灰度自动推进
- 配置的版本历史与回滚
- 多语言文案（属 REQ-021）

---

## 9. 需要回写的契约（本需求内完成）

`api-conventions` 升 **v2**，`breaking: no`（纯新增）：

| 变更 | 理由 |
|---|---|
| C9 的 `source` 增加 `default` 取值 | 必须能区分「显式配了」与「用 manifest 默认值」（§4） |
| C9 增加「作用域约束」条：登记的 `scope` 是**允许覆盖到的最深层** | 把文档约定变成强制约束（§3.2） |
| C9 增加「配置值类型校验」条 | 未声明的键不得写入（§3.1） |
| C10 增加「灰度必须确定性 + 无身份 fail closed」 | 否则多实例下行为不一致（§5.2） |

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
| R4 | **平台级配置写入开给租户** → 跨租户影响 | 三张表分离 + `config:platform:write` 独立权限点（§2.1 / §6.2） |
| R5 | **配置缓存不失效** → 改完看不到 | DB epoch（`scope_key` 含 `'platform'`）+ 主动失效；生效上界 = 下一个请求 |
| R6 | **未声明的配置键被写入** → 前端拿到无类型无默认的值 | 写入校验注册表；未登记 → 400 |
| R7 | **作用域越级写** → 租户覆盖平台独有配置 | 按登记的 `scope` 强制限制可写层级（§3.2） |
| R8 | `TEXT → jsonb` 迁移出错 / 不可逆 | 写 down 脚本；迁移前后各跑一次完整套件；**空表迁移也要有 down** |
| R9 | 与既有 `ConfigResolver`（REQ-002 骨架）重名冲突 | 改造为 trait + 保留 `InMemoryConfigResolver`；Dev 要 grep 全部引用点 |
| R10 | 每请求多一次 epoch 查询的代价 | 主键查询，量级 ~0.1ms；与 REQ-009 的同款取舍，实测记录写进报告 |
