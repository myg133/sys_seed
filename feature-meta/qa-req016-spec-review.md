# REQ-016「应用接入与应用注册表」—— QA 挑刺报告（开工前）

- **审查对象**: `BA/demands/REQ-016-app-integration/` 四份文档 ——
  `demand.md`(rev 3, 93 行) / `acceptance.md`(rev 3, **11 条 AC**) /
  `decisions.md`(rev 1, D262~D275) / `design-summary.md`(rev 1)
  —— 按 **2026-11-21 09:42 之后**的版本（BA 自查修正 commit `0c88234` 后）审
- **代码基线**: worktree `feature-REQ-016` @ `feature/REQ-016` @ `5aba742`
  （`develop@aa5b305` + 1 个 BA 派单提交）；**只读核实，未改任何文件**
- **环境**: Arch Linux / podman `base-postgres` / 本机无 `psql`
- **方法**: 11 条 AC 逐条 + design 逐节 + 契约 C1/C4/C6/C7/C9/C13 + 代码实测
  （脚本计数 / `grep` / 打开结构体与 handler 源码）。**未跑 `cargo build/test`**
  （需求尚未实现）。§3 列出全部实测命令与输出。
- **分级**: 级别（阻塞/高/中/低）与 dev-guide §9.12 的**档位**（主体不存在 /
  物理不可达 / 不可判定 / 口径不一致 / 表述）独立标注。
  **阻塞 = 落在前三档，或「按错的规格实现 + 按错的规格验收」会自洽闭环**。

---

## 1. 总览

| 级别 | 数量 |
|---|---|
| **阻塞** | **9** |
| **高** | **16** |
| **中** | **9** |
| **低** | **5** |
| **合计** | **39** |

**按档位（dev-guide §9.12，一条只记最严重的一档）**：

| 档位 | 数量 | 编号 |
|---|---|---|
| 主体不存在 | **4** | S-02、S-03、S-09、S-24 |
| 物理不可达 | **5** | S-01、S-06、S-07、S-21、S-23 |
| 不可判定 | **7** | S-04、S-10、S-18、S-19、S-30、S-33、S-39 |
| 口径不一致 | **18** | S-05、S-08、S-12、S-13、S-14、S-15、S-16、S-17、S-20、S-22、S-25、S-26、S-27、S-28、S-31、S-34、S-35、S-36 |
| 表述 | **5** | S-11、S-29、S-32、S-37、S-38 |
| **合计** | **39** | |

**11 条 AC 全部有问题（11/11）**，其中 **7 条含阻塞级**：

| AC | 问题编号 | 阻塞 | 结论 |
|---|---|---|---|
| AC-S-024 | S-10 | 0 | 「exp 距撤销时刻 ≤ TTL」是**恒真断言**，零敏感度 |
| AC-S-156 | S-05、S-11、S-34 | **1** | 步骤 8~11 前置不闭合；换 5 种 URL 校验的是同一个 client |
| AC-S-157 | S-23、S-27 | 0 | 步骤 5 的 INSERT 缺必填/FK 列 → 会因 FK 失败而非 RLS |
| AC-S-158 | S-06 | **1** | 删的是**全仓共享夹具** `client-A` → 打掉 024/156/159/162 |
| AC-S-159 | S-26 | 0 | 未写调用账号/权限、appId 来源 |
| AC-S-160 | S-01、S-04、S-07、S-18、S-19 | **3** | 配置作用域错 → 步骤 4/5 永不生效；seed 缺 `launch_url` → 0 组 |
| AC-S-161 | S-01、S-04、S-07、S-28 | **3** | 同上 + `POST /access` 也要求 `launch_url` 非空 |
| AC-S-162 | S-08、S-09、S-24、S-33 | **2** | 「置空 launch_url」在 `Option<String>` 下表达不出来；`labelKey` 无来源 |
| AC-S-163 | S-03、S-09、S-21、S-22 | **2** | 现有壳层里**运行时加路由不生效、也没有页面可渲染** |
| AC-S-164 | S-12、S-39 | 0 | 事件码两段式违反已交付三段式；`object_type` 口径冲突 |
| AC-S-165 | S-02、S-15、S-30 | **1** | design §6 的 `config` 段是**非法 schema** → manifest 解析失败 |

---

## 2. 逐条挑刺

### 阻塞（9）

#### S-01: 3 个 `app.*` 配置键**没写 `scope`** → 落到 `tenant` 作用域 → 平台级读写端点直接拒绝 → AC-S-160 步骤 4/5 与 AC-S-161 步骤 6「配置生效」**永远达不到**

- **档位**: 物理不可达 ｜ **级别**: 阻塞 ｜ **位置**: `design-summary.md:256-264`、`decisions.md:357-366`、`acceptance.md:220-221`、`:246`
- **问题**: design §6 的 manifest 示例与 D274 都给键名+默认值，但**都没写 `"scope": "platform"`**。
- **证据**（全部实测）：
  - `crates/platform-core/src/extension/manifest.rs:189-207` 的 `ConfigSpec` 有 `scope: ConfigScope`
    （`:196-197`），`fn default_scope()` 在 `:264-266` 返回 **`ConfigScope::Tenant`**。
  - `crates/platform-config/src/service.rs:280` 平台级**写**端点：`if spec.scope != ConfigScope::Platform`
    → `Err(ScopeNotAllowed{reason:"the platform endpoint only serves platform-scoped keys"})`。
  - `crates/platform-config/src/service.rs:238-246` 平台级**读**也只取 `scope == Platform` 的键
    （generic 路由则对 platform 键整请求 403）。
  - 对照已交付范式：`crates/platform-oauth/manifest.json` 的 5 个键**每条都带 `"scope": "platform"`**；
    `crates/platform-oauth/src/config.rs:100-105` 明写「在 `platform-server` 里用
    `ConfigService::read_platform_entries` 读那 5 个键」。
  - 配置快照是 **epoch 成对比较 + 60s TTL**（`crates/platform-config/src/resolver.rs:46-47`、`:100-145`）。
- **照字面实现的后果**: platform-app 按 oauth 范式注入 `AppConfigSource`，assembly 层用
  `read_platform_entries(["app.recentCount", …])` → 返回 `ScopeNotAllowed` 或空 →
  只能降级默认值 → **AC-S-160 步骤 5（maxApps=2）和 AC-S-161 步骤 6（recentCount=1）永红**，
  而且失败现象是「配置改了不生效」，最像代码 bug。
- **建议**: manifest 三个键**必须**写 `"scope": "platform"`；把这条写进 D274 与 design §6 示例。
  （其余配置项统一：`label` 也补上，否则配置界面空标题 —— 见 S-38。）

#### S-02: design §6 的 `config` 段是**非法 manifest schema**（值是裸数字/布尔）→ `Manifest::parse_checked` 直接失败 → 服务起不来、`capabilities` 生成失败、权限点对账失败

- **档位**: 主体不存在 ｜ **级别**: 阻塞 ｜ **位置**: `design-summary.md:256-264`
- **问题**:
  ```json
  "config": {
    "app.recentCount": 6,
    "app.workspace.maxApps": 60,
    "app.icon.allowHttp": false
  }
  ```
- **证据**:
  - `manifest.rs:189-207`：`ConfigSpec { #[serde(rename="type")] kind: ConfigType, default: Value, scope, label?, format? }`
    —— `type` 与 `default` **无 serde 默认**，均为必填。
  - `manifest.rs:247-248`：`pub config: Option<BTreeMap<String, ConfigSpec>>`；`:229` 有 `deny_unknown_fields`。
  - `Manifest::parse_checked` 的调用点：`crates/xtask/src/capabilities.rs:241`、
    `crates/platform-rbac/src/registry.rs:130`（**启动对账**）。
- **照字面实现的后果**: Dev 照抄 design §6 → `cargo xtask capabilities` 直接报
  「invalid type: integer, expected struct ConfigSpec」→ **AC-S-165 步骤 1a 第一秒就红**；
  更糟：启动对账也会因同一 manifest 解析失败而挂，日志却写「权限点注册表对账失败」→ 误导排查。
- **建议**: 改成 oauth 的真实形状（每条 `{"type":"number","default":6,"scope":"platform","label":"…"}`），
  并在 design 里注明「照抄 `crates/platform-oauth/manifest.json` 的 `config` 段形状」。

#### S-03: AC-S-163 的「运行时路由」在现有壳层**没有落点**：`routes` 是 `const`、`createRouter` 捕获数组、`RouteDefinition` 无 `component`、`App.svelte` 用硬编码 `if/else` 渲染

- **档位**: 主体不存在 ｜ **级别**: 阻塞 ｜ **位置**: `acceptance.md:285-301`、`decisions.md:270-297`、`design-summary.md:194-216`、`:296-300`
- **证据**（全部实测）：
  - `apps/shell/src/App.svelte:68` `const routes = createRoutes(...)`；`:81` `const router = createRouter({ routes, ... })`。
  - `apps/shell/src/lib/router.ts:101` `readonly routes: readonly RouteDefinition[]`；
    `:156` `const { routes, environment, getState, onResolve } = options;` —— 闭包**捕获数组引用**；
    `export interface Router`（`:113`）**没有** `addRoute` / `setRoutes`。
  - `apps/shell/src/lib/router.ts:41-62` `RouteDefinition` 只有 `path/title/permission?/requiresAuth?/load?`
    —— **没有组件字段**。
  - `apps/shell/src/App.svelte:221-266` 页面渲染是**逐路径硬编码**：
    `{:else if resolution.path === '/login'}` … `{:else if resolution.path === AUDIT_ROUTE_PATH}` … `{:else}<Home/>`。
- **照字面实现的后果**: design §4.4 的
  `routes = [...createRoutes(...), ...routesFromNavigation(nav)]`
  ① 因为 `routes` 被 `createRouter` 捕获，**重赋值对路由器零影响**（按钮点了还是走旧表）；
  ② 即使生效，新路径也会落到 `{:else}<Home/>`，**不可能「落到对应页面」**；
  ③ 要真生效就必须改 `router.ts`（加 `addRoute`/改 `routes` 为 getter）—— **违反 D270「router.ts 零改动」**。
  → AC-S-163 步骤 4/5 在「不改底座代码」的约束下**自相矛盾**。
- **建议**: 二选一并写进 D270：(a) 允许改 `router.ts`（加 `addRoute(route)`，并给出既有用例不受影响的证明）；
  或 (b) 保留「不改 router.ts」但把验收降级为「`routesFromNavigation` 纯函数单测 + 工作台页面渲染外部链接」，
  **删除**步骤 4「落到对应页面」。

#### S-04: AC-S-160 / AC-S-161 的播种数据**没给 `launch_url`**，而 design §4.2/§4.3 要求「`launch_url` 非空才可见/可记访问」→ 照字面实现得到 **0 组 / 0 条**

- **档位**: 不可判定 ｜ **级别**: 阻塞 ｜ **位置**: `acceptance.md:210-215`、`:237-238`；`design-summary.md:174`、`:184`、`:369-370`(R6)；`decisions.md:162-176`(D267)
- **证据**:
  - design §4.2 步骤 1：「取 status='active' 的应用（RLS 限定 + **`launch_url` 非空**）」；
  - design §4.3 步骤 1：「校验 app 存在…且 active 且 **`launch_url` 非空**」；
  - design §12 R6：「`launch_url` 为空的应用…**不显示**（没入口点了没意义）→ AC-S-160/162 一致」；
  - 但 AC-S-160 前置（`:210-215`）只给了 category / sortOrder / status，**一个字没提 `launch_url`**；
  - 而 D267（`decisions.md:174`）只写「只含 `status='active'` 的应用」，**没有** `launch_url` 过滤
    → **D267 与 design §4.2/R6 本身互相矛盾**。
- **照字面实现的后果**: Dev 按 design 实现过滤 + 按 AC 播种（`launch_url` 默认 NULL）
  → `groups == []`，`recent == []` → AC-S-160 步骤 3、AC-S-161 步骤 2/3/5/6 全红，
  且**看起来像排序逻辑错了**。
- **建议**: ① D267 与 design §4.2 统一（推荐：工作台**不**按 `launch_url` 过滤，只有导航过滤）；
  ② 若保留过滤，AC-S-160/161 的播种必须逐条写 `launch_url`。

#### S-05: AC-S-156 步骤 8~11 的**前置不闭合**：需要 `client-B` 与多个**未被占用**的 client / 唯一 `display_name`，否则先撞 409 `APP_DUPLICATED` 而不是目标错误码

- **档位**: 口径不一致 ｜ **级别**: 阻塞 ｜ **位置**: `acceptance.md:118-125`、`:134-139`；`decisions.md:133-134`(D265 两个 UNIQUE)
- **证据**: D265 声明 `UNIQUE (tenant_id, client_id)` 与 `UNIQUE (tenant_id, display_name)`
  （`decisions.md:133-134`）；`scripts/seed-e2e.sh:414-430` 只有 `client-A` / `client-B` / `public-client`。
- **问题**:
  - 步骤 8 第二条「换个 clientId 但同一个 displayName」—— AC 前置（`:106-110`）**只说有 `client-A`**，
    没写 `client-B` 可用；
  - 步骤 9/10/11 一共 **5 + 5 + 3 = 13 次** `POST /api/apps`，**没写每次用哪个 `clientId` / `displayName`**。
    若沿用 `client-A` → 第 2 次就 409 `APP_DUPLICATED`；若沿用同名 → 409。
  - 步骤 9 期望「5 次全部 400 `APP_LAUNCH_URL_INVALID`」与步骤 8 期望「409」在**同一份夹具**上冲突
    → 必须规定**校验顺序**（client 是否可用 → URL 合法性 → 唯一性），否则两条判据互斥。
- **照字面实现的后果**: 一条测试里 13 次 POST 复用同一夹具，断言码取决于实现内部校验顺序 →
  「实现顺序换一下，AC 红/绿翻面」的**不可判定**状态。
- **建议**: AC 前置补 `client-B`~`client-F`（或写明「步骤 9~11 每次先新建一个专用 client」），
  并把校验顺序写死成一条（建议 `APP_INVALID_REQUEST` → `APP_CLIENT_INVALID` →
  `APP_LAUNCH_URL_INVALID`/`APP_ICON_URL_INVALID` → `APP_DUPLICATED`）。

#### S-06: AC-S-158 路径 3 **删除的是全仓共享夹具 `client-A`** → 级联删掉 AC-S-024/156/159/162 的全部数据，且 `seed-e2e.sh` 里带 `backchannel_logout_uri` 的 `client-A` 永久消失

- **档位**: 物理不可达 ｜ **级别**: 阻塞 ｜ **位置**: `acceptance.md:179-186`；`decisions.md:106-110`
- **证据**:
  - `scripts/seed-e2e.sh:414-430` 建 `client-A`(`confidential` + back-channel URI)、`client-B`、`public-client`；
    `:558-562` 的强断言段会校验 `client-A` 的 `backchannel_logout_uri` /
    `frontchannel_logout_uri IS NOT NULL` / `length(client_secret_hash)=43`。
  - `client-A` 被 AC-S-024（授权/内省）、AC-S-156/159（绑定）、AC-S-162（`launch_url` 应用）复用。
  - D264（`decisions.md:110`）自己写明「其它租户引用**同一个** client 的应用行**也**被级联删」。
- **照字面实现的后果**: `DELETE /api/oauth/clients/client-A` 之后：
  ① AC-S-156/159/162 的 app 行被级联删；
  ② AC-S-024 的 grant / 令牌路径失效（client 没了）；
  ③ 再跑 `seed-e2e.sh` 的强断言段 → `client-A` 行数为 0 → **seed 自检红**。
  → 只要 AC-S-158 先跑（编号 158 < 159/162），**后面全红**；这是「一条 AC 打掉整个套件」。
- **建议**: 路径 3 改为「用 API **新建**一个一次性 client（如 `client-cascade-probe`）→ 建应用 →
  删 client → 断言行没了」，**不得**动 `client-A`；并在 AC 里明写「本路径不改动任何既有夹具」。

#### S-07: AC-S-160 步骤 5 / AC-S-161 步骤 6 的「把配置改成 N」**没有执行主体与合法通道**

- **档位**: 物理不可达 ｜ **级别**: 阻塞 ｜ **位置**: `acceptance.md:220-221`、`:246`
- **证据**:
  - 合法写通道：`PUT /api/config/entries` 需 `config:entry:write`、`PUT /api/config/platform-entries`
    需 `config:platform:write`（`crates/platform-config/src/http.rs:105-111`）。
  - AC 的账号池：`u-perm-app`(app:registry:*) / `u-perm-audit`(audit:events:read) /
    `u-perm-read`(rbac:role:read+sandbox:demo:read+oauth:grant:*)（`scripts/seed-e2e.sh:146-149`）
    —— **没有一个持 config 写权限**。
  - 若测试改用 `base` 直连 `UPDATE platform_config` → **不 bump epoch**
    （`migrations/20261118001000_config_schema.up.sql:120-135` 只有 `platform_config_upsert` 自增；
    `crates/platform-config/src/resolver.rs:46-47` TTL **60s**）→ 最多 60 秒内读到旧值 → **偶发红**。
- **照字面实现的后果**: Dev 二选一都会踩：走 API → 403（没人有权限）；直连 SQL → 偶发红。
- **建议**: 写成「以 `base` 执行 `SELECT platform_config_upsert('app.workspace.maxApps','2'::jsonb)`
  （**同事务自增 epoch**）→ 立刻 `GET`」；并给 `u-perm-app` 补 `config:platform:write` 作为备选通道。

#### S-08: AC-S-162 步骤 5「把应用的 `launch_url` 置空（`NULL`）」在 `Option<String>` 的 PATCH 语义下**表达不出来**

- **档位**: 口径不一致 ｜ **级别**: 阻塞 ｜ **位置**: `acceptance.md:270`、`:280`；`design-summary.md:123`(`PATCH` 请求体「可选字段的任意子集」)
- **证据**: design §3.1 只写「上述可选字段的任意子集」；已交付的 PATCH 范式
  `crates/platform-oauth/src/http.rs:610-627`（`update_client`）**根本不支持清空**，只支持 status 枚举。
  在 serde 里 `Option<String>` 对 `null` 与「缺键」**同义**（都反序列化成 `None`）→
  写 `{"launchUrl": null}` 与「不改这个字段」**不可区分** → 无法表达「置空」。
- **照字面实现的后果**: Dev 用 `Option<String>` → 发 `null` 不生效 → `launch_url` 仍有值 →
  AC-S-162 步骤 5「该应用不出现在 items 里」必红；且 D269「为空不出现在导航」这条规则**永远测不到**。
- **建议**: AC 里钉死清空手段，三选一：`Option<Option<String>>` + `null`（并给出实现方式）、
  专用端点 `POST /api/apps/{id}/clear-launch-url`、或 PATCH 里加 `clearLaunchUrl: true`。

#### S-09: `module` 来源导航项的 **`labelKey` / `category` / `sortOrder` 在 `MenuSpec` 里根本不存在** —— D269 的 `"labelKey": "nav.audit"` 无源

- **档位**: 主体不存在 ｜ **级别**: 阻塞 ｜ **位置**: `decisions.md:234-242`(D269 字段表)、`:222-231`(示例)；`design-summary.md:130-146`；`acceptance.md:299`
- **证据**: `crates/platform-core/src/extension/manifest.rs:120-132` 的 `MenuSpec` **只有 4 个字段**：
  `label`(中文文案) / `icon: Option<String>` / `route` / `perm: Option<String>`；
  `deny_unknown_fields`（`:117`）。D269 字段表却写 `key` = `menus[].label`、
  `labelKey` = 「翻译键（`nav.*`）」、`category` = `"platform"`、`sortOrder` = `0`
  —— **后三个字段 manifest 里没有**，`key` 用中文文案当键。
- **照字面实现的后果**:
  ① `labelKey` 只能靠后端**从中文 label 猜**（"审计日志" → `nav.audit`？）→ 不可实现；
  ② AC-S-163 步骤 4「页面标题是该导航项的 `labelKey`」直接落空；
  ③ `key` 用中文 → 前端列表 key 随文案变、无法 i18n；
  ④ 要真加字段 = 改 `manifest.rs` + `manifest_schema.rs`（**跨需求改 schema**，D109 要显式声明）。
- **建议**: 二选一并写进 D269：(a) `manifest.json` 的 `menus[]` 增加 `labelKey` / `category` / `sortOrder`
  （需在 demand 的依赖段按 D109 声明改动，并同步 `schema/manifest.schema.json` 与 `capabilities`）；
  或 (b) 后端按固定映射表（`module id` → `nav.*`）生成，并把映射表当交付物写明。

---

### 高（16）

#### S-10: AC-S-024 步骤 3 的判据**恒真**（零敏感度）

- **档位**: 不可判定 ｜ **级别**: 高 ｜ **位置**: `acceptance.md:91-96`
- **证据**: 令牌在**签发时**就定了 `exp = issued_at + TTL`（`crates/platform-oauth/src/service.rs:1145` 等
  用 `settings.access_token_ttl_seconds` 算 `exp`）。撤销发生在签发之后 → `exp - 撤销时刻` **必然 < TTL**。
  → 「`exp` 距撤销时刻**不超过** `access_token_expires_in` 秒」对**任何**实现都成立，
  即使撤销把 `exp` 改短、或令牌根本没被撤销。
- **建议**: 改成可失败的判据：`exp == 签发时刻 + access_token_expires_in`（**逐字节**比对
  令牌 payload 的 `iat` / `exp` 与元数据），并补一条 `exp > 撤销时刻`（否则「撤销即把 exp 归零」
  这种错实现也能过）。另：本条与 REQ-022 AC-S-143/145 重复，建议明确降级为「回归 smoke」，
  不要在 REQ-016 重新定义判据。

#### S-11: AC-S-156 步骤 9/10 的 scheme 判据**缺 https 阳性对照**，且用 `app.icon.allowHttp` 管 `launch_url`（命名与语义错位）

- **档位**: 表述 ｜ **级别**: 高 ｜ **位置**: `acceptance.md:120-123`、`:135-138`；`decisions.md:363`(D274)
- **证据**: 配置键名是 `app.icon.allowHttp`（「**图标** URL 是否允许 http」）；
  AC 却用它判定 `launch_url` 的 `"http://x.example/a"` 必须 400。
- **问题**: ① 5/5 全是**拒绝**，没有一条应通过的取值 → 一个「全部拒绝」的校验器
  （甚至 `validate()` 直接 `return Err`）都能 5/5 绿 —— 这正是 dev-guide §9.13 的「无区分力夹具」；
  ② 键名与用途不符，Dev 会问「`launch_url` 到底受哪个开关控制」，两种实现（复用/新加键）都能自洽。
- **建议**: ① 每个路径补 **2 条阳性对照**（`"https://x.example/a"` → 201；`allowHttp=true` 时
  `"http://x.example/a"` → 201）；② 把键改名为 `app.url.allowHttp`（或新增 `app.launchUrl.allowHttp`），
  并在 AC 里明写「该开关同时约束 `launch_url` 与 `icon_url`」。

#### S-12: AC-S-164 的事件码是 **两段式**，与它自己引用的「三段式」规范及已交付的 `oauth.client.registered` 冲突；`object_type == "app"` 与 `oauth_client` 口径冲突

- **档位**: 口径不一致 ｜ **级别**: 高 ｜ **位置**: `acceptance.md:313-317`
- **证据**: `acceptance.md:313` 写「D215 同款命名规范：**三段式** `app.*`」，但列出的码是
  `app.registered` / `app.updated` / `app.deleted`（**只有两段**）。已交付实现在
  `crates/platform-oauth/src/events.rs:25-42`：`oauth.client.registered` 等，
  且 `:57-59` `object_type::CLIENT = "oauth_client"`；该模块文档（`:19-23`）明写
  「命名法 `<域>.<对象>.<动作>`（D163）」。
- **建议**: 改为 `app.registry.registered` / `app.registry.updated` / `app.registry.deleted`，
  `object_type` 定为 `platform_app`（对齐 `oauth_client`）；同时要求 `events.rs` 式的
  `pub mod code` + `ALL_CODES: [&str; 3]` + 单测完备性（照 `events.rs` 范式）。

#### S-13: `platform_app_access.client_id` **没有外键** → client 删除后访问记录成孤儿行、永不清理；D268 的理由自相矛盾

- **档位**: 口径不一致 ｜ **级别**: 高 ｜ **位置**: `decisions.md:180-198`(D268)、`design-summary.md:71-83`
- **证据**: D268 的表只声明 `PK (tenant_id, user_id, client_id)`，**没有任何 FK**；
  理由写「因为 `platform_app` 行会被级联删除（D264），用 client_id **更稳定**」。
  但 `platform_oauth_client` 删除时（AC-S-158 路径 3 / AC-S-135 步骤 7）这些行**也没人删** →
  表里留下指向不存在 client 的行，只能靠 join `platform_app` 过滤（design `:198`）→ **物理垃圾 + 表无限增长**。
- **建议**: 二选一并写清：① `client_id` 加 `REFERENCES platform_oauth_client(client_id) ON DELETE CASCADE`
  （与 D264 一致，「跟着 client 走」）；② 不加 FK 但明确「孤儿行保留、按 join 过滤」并记 TD + 清理任务。

#### S-14: 契约 C1 给 REQ-016 的前缀是 **`/api/app`**，而全部规格用 **`/api/apps`**

- **档位**: 口径不一致 ｜ **级别**: 高 ｜ **位置**: `BA/contracts/api-conventions/v3.md:45`（`| /api/app | 应用注册表、工作台 | REQ-016 |`）、`:54`（「未分配前缀的分组在新增前须先改本契约升版本」）；`design-summary.md:101-116`
- **证据**: 实测既有 nest 全为单数：`crates/platform-server/src/main.rs:339`(`/api/iam`)、
  `:359`(`/api/org`)、`:368`(`/api/rbac`)、`:411`(`/api/audit`)、`:457`(`/api/schema`)。
  （附带发现：C1 写 `/api/meta` 管 REQ-012，代码是 `/api/schema` → **C1 本身已漂移**。）
- **建议**: 明确选一个：改成 `/api/app`（与 C1 + 既有风格一致，10 个端点全改）**或** 在 C1 里把
  `/api/app` 改成 `/api/apps` 并升 v4（跨契约改动，D109 声明）。不要留两套写法。

#### S-15: capabilities 的**组成算术错**：固定是 **7** 个 + sdk **7** 个 = 14，不是「8 + 6」

- **档位**: 口径不一致 ｜ **级别**: 高 ｜ **位置**: `acceptance.md:335-337`、`decisions.md:390-408`(D275)
- **证据**（实跑）:
  - `find capabilities -type f | wc -l` → **14** ✓（总数对）
  - `ls capabilities/sdk` → **7 个**（`auth hooks i18n schema sdk theme ui`），
    `ls packages/` 也是 7 个（`crates/xtask/src/capabilities.rs:461-500` 按 `packages/*/package.json` 生成）。
  - `crates/xtask/src/capabilities.rs:56-72` 的 `generate` 固定插入 **7 个**：
    `permissions` / `extension-points` / `schema/{manifest,metadata,entity}`(3) / `api` / `index`
    → 4 + 3 = 7。（AC 枚举里写的「8 个」数不出第 8 个。）
- **校正后的正文**: 「固定 **7** 个 + `sdk/<pkg>.json` **7** 个 = **14**」。
  **总数 14 是对的**，但两个加数都错 —— 这正是 D275 要防的那类错，且它已经混进了 AC 正文。

#### S-16: `decisions.md:423` 仍写「`platform_app` 的 **11 列**」；`design-summary.md:64` 对 D265 的**缺陷描述是错的**

- **档位**: 口径不一致 ｜ **级别**: 高
- **证据**:
  - `decisions.md:131`「**共 12 列**（本表是唯一权威）」；`:116-129` 表里**逐行数确实是 12 行**，
    **且含 `launch_url`（`:125`）**。
  - `decisions.md:423`（D275 的「同理」清单）写「`platform_app` 的 **11 列**」→ 与 131 冲突。
  - `design-summary.md:64` 写「（D265 的表里我写了 11 列，**漏了 `launch_url`** —— 以本表为准）」
    → **不成立**：D265 的 `:125` 就是 `launch_url`。
  - `acceptance.md:131` 已是「**12** 列」✓。
- **建议**: 删掉 `:423` 的「11 列」与 `design:64` 的错误解释；只保留「以 D265 的 12 列为唯一权威」。

#### S-17: D274 标题写「**5 个**配置项」，表里只有 **3** 行

- **档位**: 口径不一致 ｜ **级别**: 高 ｜ **位置**: `decisions.md:357-368`
- **证据**: 表行 `app.recentCount` / `app.workspace.maxApps` / `app.icon.allowHttp` = **3**；
  `design-summary.md:286` 也写「3 个配置键」。
- **建议**: 标题改 3；若原本打算 5 个，需补出另外 2 个键并说明用途（否则 `config` 段与迁移不一致）。

#### S-18: AC-S-160 步骤 5 的 `maxApps` 截断语义**未定义**，且判据「不超过 2」容许 0/1

- **档位**: 不可判定 ｜ **级别**: 高 ｜ **位置**: `acceptance.md:230`；`design-summary.md:172-178`
- **证据**: design §4.2 只写「应用 maxApps 上限（超了截断，**按排序后的前 N**）」，但**截断发生在分组前还是后**、
  「排序」是全序（category/sortOrder/displayName）还是组内序、**跨组截断会切掉哪个组** —— 全没定义。
  AC 判据只写「总数**不超过** 2」→ 返回 `groups: []`（0 个）也满足。
- **建议**: 钉死为「按 `(category, sortOrder, displayName)` 全局排序后取前 N，**再分组**」，
  判据改成**恰好等于**期望的 `[["general","C"],["finance","B"]]` 之类，并写死 N=2 时是哪两条。

#### S-19: AC-S-160 步骤 2「3 次响应逐字节相同」只证明**稳定**，不证明**正确**；用 `HashMap` 分组会偶发红

- **档位**: 不可判定 ｜ **级别**: 高 ｜ **位置**: `acceptance.md:224`
- **证据**: Rust `std::collections::HashMap` 每次 `new()` 的 `RandomState` 不同
  （同进程内不同实例的迭代序**不保证**一致）→ 若分组用 `HashMap<String, Vec<..>>` 并直接序列化，
  3 次之间顺序可能变 → **偶发红**；反过来，一个**稳定但错序**的实现（比如永远按 `id` 排）也能 3/3 相同。
- **建议**: 「3 次逐字节相同」保留为辅助断言；**主判据**必须是完整的期望数组逐字比对
  （`groups[0].category == "finance"`、`groups[0].apps[*].displayName == ["B","A"]` …）。

#### S-20: D262 的「`client.name` vs `app.display_name` 可以不同」+ 外部整页跳转 = **钓鱼面**，全套 AC 无一条约束

- **档位**: 口径不一致 ｜ **级别**: 高 ｜ **位置**: `decisions.md:61-68`、`:244-266`(D269)；`acceptance.md:258-281`
- **证据**: D262 明确「允许两者不同，同意页永远用 client 的 name」；D269 让 `app` 项 `kind="external"`、
  点击**整页跳转**到 `launch_url`；而 `launch_url` 与 `display_name` **都由租户管理员自填**（D269 `:264`）。
- **问题**: 用户在工作台看到的是「报销系统」（`display_name`）+ 任意 `https://` 域，
  点击**不经过同意页**、页面上**看不到 `client.name`**。租户管理员可把 `display_name` 写成
  「公司统一登录」「IT 服务台」而 `launch_url` 指向自己的站点 → 对**本租户所有用户**的定向钓鱼。
  AC-S-162 只断言「target 逐字等于 launch_url」，**没有**任何「名字必须与 client.name 一致/需并显」的判据。
- **建议**: 至少选一条：① 工作台/导航里 `app` 项**同时**下发 `clientName`（来自 client 表）并要求前端并显；
  ② 或禁止 `display_name` 与 `client.name` 偏离（`APP_DISPLAY_NAME_MISMATCH`）；
  ③ 或在外链上加「将离开本平台 → 目标域名」的中间确认页（新增 1 条 AC）。

#### S-21: AC-S-163 步骤 1 的「用 `git diff --stat` 对比本需求**前后**」**没有基线 ref** → CI 不可执行

- **档位**: 物理不可达 ｜ **级别**: 高 ｜ **位置**: `acceptance.md:289`、`:295`
- **建议**: 改为可机械执行的断言：
  `git diff --stat aa5b305..HEAD -- apps/shell/src/lib/router.ts` **输出为空**（并写死基线 commit），
  或更稳：对 `router.ts` 做**内容哈希白名单**（把基线的 SHA-256 写进 AC）。

#### S-22: AC-S-163 步骤 4 的「**页面标题**是该导航项的 `labelKey`」歧义，且该用例**无区分力**（`/admin/audit` 路由早已存在）

- **档位**: 口径不一致 ｜ **级别**: 高 ｜ **位置**: `acceptance.md:299`；`apps/shell/src/lib/routes.ts:143-160`
- **证据**: ①「标题 = labelKey」是指 `document.title` 等于裸键 `nav.audit`，还是等于翻译后的「审计日志」？
  现有约定是 `RouteDefinition.title` 存**翻译键**、由渲染层 `t()`（`routes.ts:87-89` 注释）；
  ② `routes.ts:143` 已有 `AUDIT_ROUTE_PATH`(`/admin/audit`)+`title:'nav.audit'`，若 E2E 点的正是它，
  **新机制有没有生效都一样绿**。
- **建议**: 判据写成「`document.title == t("nav.audit")`（中文环境为『审计日志』）」，
  并让 E2E 使用**一个只在导航里出现的** internal 项（如为测试新增 `sandbox`/`org` 的页面），
  以证明路由**来自导航**而不是硬编码。

#### S-23: AC-S-157 步骤 5 的 `INSERT` 缺必填列（`client_id` NOT NULL + FK、`user_id`）→ 会因 **FK/NOT NULL 失败**而非 `WITH CHECK` 拒绝 → 判据按错的理由变绿

- **档位**: 物理不可达 ｜ **级别**: 高 ｜ **位置**: `acceptance.md:159-160`、`:166`
- **证据**: `platform_app` 的 `client_id TEXT NOT NULL` + FK（`design-summary.md:53`、`decisions.md:120`）；
  `platform_app_access` 的 PK 含 `user_id`（`decisions.md:190`）。AC 只写「INSERT 一行 `tenant_id='tenant-b'`」。
- **建议**: 把两条 INSERT 的**完整列清单**写进 AC（含一个已存在的 `client_id`、`user_id='u-perm-app'`…），
  并把断言写成「SQLSTATE **42501** 或 `row-level security` 字样」——**排除** 23503(外键)/23502(非空)。

#### S-24: 导航上线后会**立即下发死链**：`org` / `sandbox` 的 `menus[].route` 在前端路由表里不存在

- **档位**: 主体不存在 ｜ **级别**: 高 ｜ **位置**: `acceptance.md:258-281`（只断言 target 以 `/` 开头）；`crates/platform-org/manifest.json`、`modules/sandbox/manifest.json`
- **证据**（实跑）: 带 `perm` 的菜单实测 **4 条** —— `audit /admin/audit`、`oauth /oauth/grants`、
  `org /org`、`sandbox /sandbox`。而 `apps/shell/src/lib/routes.ts:88-172` 的路由表只有
  `/`、`/login`、sso callback、oauth×4、`/403`、`/schema/...`、`/admin/roles`、`/admin/audit`、`/oauth/grants`
  —— **`/org` 与 `/sandbox` 不存在**，`apps/shell/src/pages/` 也没有 `Org.svelte` / `Sandbox.svelte`。
- **照字面实现的后果**: AC-S-162 一上线，有 `org:org-node:read` 的用户导航里就多两个**点了落到 `<Home/>`** 的项
  （`App.svelte:266` 的 `{:else}`）—— AC 全绿而用户体验坏。
- **建议**: 补一条判据「每个下发的 `module` 项，其 `target` 必须存在于**当前前端路由表**」，
  并把断言落在 `routes.ts` 的 path 集合上（这条也顺便挡住将来新增死链）。

#### S-25: `PATCH` 并发丢更新 + `UNIQUE (tenant_id, display_name)` 改名撞车的返回码 —— 两条都无 AC 覆盖

- **档位**: 口径不一致 ｜ **级别**: 高 ｜ **位置**: `acceptance.md:104-140`（全文无并发/撞车用例）
- **证据**: D265 声明 `UNIQUE (tenant_id, display_name)`（`decisions.md:134`），但 AC-S-156 步骤 8 只测
  **INSERT** 的重复，**没测 PATCH 改成已有名**；也没有 `updated_at` / 版本号列（`decisions.md:116-129`）。
- **照字面实现的后果**: ① 两人同时 PATCH → 后写覆盖先写（last-write-wins），无任何提示；
  ② PATCH 撞名 → Dev 可能返回 500（唯一约束异常直接冒泡）→ 没判据抓不到。
- **建议**: AC-S-156 补路径 6：`PATCH` 把 `displayName` 改成另一个应用的名字 → **409 `APP_DUPLICATED`**；
  并发丢更新记 TD（或加 `If-Match`/`version`）。

---

### 中（9）

#### S-26: AC-S-159 前置**没写用哪个账号**调 `GET /api/apps/{appId}`，也没写 `appId` 从哪来

- **位置**: `acceptance.md:193-199` ｜ **档位**: 口径不一致 ｜ **级别**: 中
- **证据**: `GET /api/apps/{appId}` 需 `app:registry:read`（`design-summary.md:107`），而 AC-S-159 前置只写
  「`client-A` 的 `allowed_scopes` = …」。
- **建议**: 补「以 `u-perm-app` 调；`appId` 来自 AC-S-156 步骤 1 或 seed 的固定应用」。

#### S-27: AC-S-157 步骤 1/2「以 `tenant-a` 上下文查」**没写手段**，也没写期望行数

- **位置**: `acceptance.md:155-165` ｜ **档位**: 口径不一致 ｜ **级别**: 中
- **问题**: 走 HTTP 还是 `set_config`+SQL？「只看到自己租户的行」是「≥1 且全属本租户」还是「恰好 N 行」？
- **建议**: 写死「以 `base_app` 单事务 `set_config('app.tenant_id','tenant-a')` 后 `SELECT tenant_id, count(*)`
  → `('tenant-a', N)`，N 由夹具给定」。

#### S-28: AC-S-161 步骤 4「行数**仍为 3**」依赖 seed 恰好播 3 行，且没排除别的用例残留

- **位置**: `acceptance.md:251` ｜ **档位**: 不可判定 ｜ **级别**: 中
- **建议**: 断言写成「按 `(tenant_id,user_id)` 分组，`count(*) == 3` 且 `client_id` 集合 `== {A,B,C}`」，
  并在前置里明确「seed 只播 A/B/C 三行访问记录」。

#### S-29: `GET /api/apps` 返回**裸数组**，违反契约 C7 的游标分页（有先例，但需显式声明偏离）

- **位置**: `design-summary.md:124`（「**数组**，不是 `{apps: []}`」）；`BA/contracts/api-conventions/v1.md` 的 C7
- **证据**: C7 要求「游标分页（`cursor`+`limit`）、响应含 `next_cursor`、默认 `limit=20`」；
  先例：`crates/platform-oauth/src/http.rs:562-570` 的 `list_clients` 也返回裸数组（REQ-022 已偏离）。
- **建议**: 在 design 里写一句「注册表类小列表不适用 C7，属**已声明偏离**（与 `list_clients` 同款）」，
  否则审计/合规时会被当成违约。

#### S-30: AC-S-165 的「`total == 35`」「恰好 14 个文件」**没有 `file:line` 的 `assert!` 落点**

- **位置**: `acceptance.md:334-342`；`design-summary.md:303-312`(测试落点表)
- **证据**: `cargo xtask capabilities` 的 `--check` 只**比对产物与磁盘一致性**
  （`crates/xtask/src/capabilities.rs:91-130` 的 `check`），**不断言** `total` 数值；
  design §9 的测试落点表里**没有** capabilities 的数值断言。
- **建议**: 明确新增 `crates/xtask/tests/capabilities_numbers.rs`（断言 `total==35`、
  `modules` 含 `platform-app`、`manifest=="crates/platform-app/manifest.json"`、文件数 14），
  并在 AC 里给出 `file:line` + 函数名（dev-guide §9.15.1）。

#### S-31: `launch_url` / `icon_url` 的**空串与长度上限**未定义；`icon_url` 可指任意域（追踪像素/隐私）

- **位置**: `acceptance.md:120-123`、`:138`
- **问题**: `""`（空串）算合法还是 400？`icon_url = "https://tracker.example/pixel.gif"` 会让
  **每个打开工作台的用户的 IP/UA** 泄露给第三方；`launch_url` 同理可以指向任意 https 域（见 S-20）。
- **建议**: 写明「空串等价 NULL，合法」；`icon_url` 记 TD（REQ-018 的文件上传或域名白名单）；
  `launch_url` 的域名策略至少记 TD。

#### S-32: 应用数量**配额**未定义（一个租户能注册 10000 个应用？）

- **位置**: 全文无配额
- **建议**: 记 TD，并在 `design` 的风险表里加一行（`maxApps` 是**展示**上限，不是**注册**上限）。

#### S-33: D269 明写「`perm` 为 `null` 时**总是下发**」，但 AC-S-162 **没有对应用例**

- **位置**: `decisions.md:253`；`acceptance.md:265-280`
- **证据**: `platform-app` 自己的 manifest 里「工作台」项就是 `perm` 缺省（`design-summary.md:248-250`）。
- **建议**: 补一条判据：不经任何权限点的账号（`u-perm-none`）也能在 `items` 里看到该 `perm` 缺省项。

#### S-34: 审计字段判据「每条**含** `ip` / `user_agent`」**无敏感度**（列是 `NOT NULL DEFAULT ''`）

- **位置**: `acceptance.md:315-316`；`migrations/20261120000200_audit.up.sql`（`ip TEXT NOT NULL DEFAULT ''`）
- **建议**: 改成「`ip` **逐字等于** 请求方发来的 `X-Forwarded-For` 首段」，
  或至少「`ip != ''`」—— 否则「含这个字段」永远成立。

---

### 低（5）

#### S-35: 「不复制敏感字段」的清单口径不统一（D262 说 **5 类**，AC-S-156 步骤 7 列了 **6 个列名**）

- **位置**: `decisions.md:49-51`（secret / 回调地址 / 授权类型 / scope 白名单 / 有效期 = 5 类）；
  `acceptance.md:132-133`（6 个列名 + `allowed_grant_types`）；`design-summary.md:69`
- **实测**: `platform_oauth_client` **确为 13 列**（`migrations/20261120000500_oauth_schema.up.sql:43-57`）✓，
  其中「敏感」的是 6 个：`client_secret_hash` / `redirect_uris` / `allowed_grant_types` /
  `allowed_scopes` / `backchannel_logout_uri` / `frontchannel_logout_uri`；
  **不敏感**的是 `client_type` / `name` / `status` / `expires_at` / `created_at` / `created_by` / `client_id`。
- **建议**: 统一为「**6 个列**」并逐字列出（AC 那 6 个是对的，改 D262 的「5 类」）。

#### S-36: `design-summary.md:267-269` 用 `deny_unknown_fields` 论证「不要写 `"perm": null`」——**论证不成立**

- **证据**: `perm` 是 `MenuSpec` 的**已知字段**（`manifest.rs:128-132`），`deny_unknown_fields` 不会因此报错；
  `"perm": null` 与省略该键**等价**。结论（建议省略）仍然正确，但理由错误。
- **建议**: 改成「`perm` 是可选字段，省略即可；写 `null` 也能解析，但少一种写法少一处风险」。

#### S-37: `platform_app` 缺 `updated_at` / `updated_by`

- **位置**: `decisions.md:116-129`（12 列里只有 `created_at` / `created_by`）
- **问题**: PATCH 之后没有任何「最后修改人/时间」；AC-S-164 的 `app.updated` 审计有记录，但表自身查不出。
- **建议**: 记为已知取舍（D265 写明「PATCH 不落表时间戳，只进审计」）或补 2 列。

#### S-38: 3 个 `app.*` 配置键**缺 `label`**

- **证据**: `crates/platform-oauth/manifest.json` 的 5 个键每条都有 `label`（「访问令牌有效期（秒）」等）；
  `ConfigSpec.label` 是 `Option<String>`（`manifest.rs:198-200`）→ 不写就是空 → 配置界面无标题。
- **建议**: 三个键各补 `label`。

#### S-39: AC-S-165 步骤 3/4 的**变异测试**是「改源码 → 跑测试 → 还原」，需按 §9.4 处理还原后时间戳

- **证据**: dev-guide §9.4（「还原文件后必须 `touch`」，否则 cargo 不重编 → 假绿）。
- **建议**: 在 AC 里写明「还原后 `touch` 该文件，并重跑一次确认回到绿」。

---

## 3. 实测证据（跑了什么、输出是什么）

> 全部只读。worktree `/home/myg133/Projects/system_seed/feature-REQ-016`
> （`feature/REQ-016` @ `5aba742`，`git status` 干净）。

| # | 命令 | 输出（摘要） | 用途 |
|---|---|---|---|
| 1 | `find capabilities -type f \| wc -l` | **14** | AC-S-165「恰好 14」总数 ✓ |
| 2 | `ls capabilities/sdk` | **7** 个（auth hooks i18n schema sdk theme ui） | 证明「6 个前端包」错 → S-15 |
| 3 | `ls packages/` 逐个查 `package.json` | **7** | 同 S-15 |
| 4 | 读 `crates/xtask/src/capabilities.rs:56-72` | 固定插入 **7** 个（permissions / extension-points / schema×3 / api / index） | S-15 |
| 5 | `python3` 解析 `crates/platform-core/src/titles.rs`（去注释后数字符串字面量对） | 186 字面量 → **93 条** | TITLES 基线 **93** ✓ |
| 6 | `grep -n "BASELINE_ROWS_BEFORE_REQ_013\|REQ_0*_NEW_CODES" crates/platform-server/tests/i18n_titles.rs` | `58 + 21 + 5 + 9` = **93** | 交叉验证 93 ✓ |
| 7 | 读 `acceptance.md:46-56` 错误码表 | **6** 个码，逐个有 AC 触发点 | `93+6=99` ✓，**无死码** ✓（BA 自查修正成立） |
| 8 | `python3` 读 `capabilities/permissions.json` | `total=32`、`modules=6` | `32+3=35` ✓ |
| 9 | 读 `capabilities/permissions.json` 的 `modules[3]` | `"manifest": "crates/platform-oauth/manifest.json"` | `modules[].manifest` 格式与 AC 一致 ✓；生成器 `capabilities.rs:230-275` 扫 `modules/`+`crates/` |
| 10 | 读 `crates/platform-core/src/extension/manifest.rs:100-135` | `MenuSpec{label, icon?, route, perm?}`，`deny_unknown_fields` | `perm: Option<String>` ✓（R3 成立）；**无 labelKey/category/sortOrder** → S-09 |
| 11 | `grep -rn "menus" crates/*/src/*.rs \| grep -v manifest.rs` | 仅测试夹具 `"menus": []` | 「无 API 下发」✓ |
| 12 | 读 `crates/platform-{audit,oauth,org}/manifest.json` + `modules/sandbox/manifest.json` | **4 条带 perm 的菜单**：`/admin/audit`、`/oauth/grants`、`/org`、`/sandbox` | AC-S-162 前置成立 ✓；但 `/org` `/sandbox` 无路由 → S-24 |
| 13 | `cat apps/shell/src/lib/nav.ts` | `:24`「本期为占位数据，后续由应用注册表下发」；`NAV_ITEMS` 指向 `/workbench` `/api/iam` … | 占位 ✓；`/workbench` 无路由 → 见 S-03 |
| 14 | `cat apps/shell/src/lib/router.ts` | `:101` `readonly routes`；`:113` Router **无 addRoute**；`resolve()` 未命中返回 `allowed:true` | D269「kind 必须」的理由 ✓（静默放行，`route:undefined`）；**S-03 的根因** |
| 15 | `grep -n "path:" apps/shell/src/lib/routes.ts` | 12 条路由；**无 `/workbench`**（`:88` 是 `/` + `title:'nav.workbench'`）；**无 `/org` `/sandbox`** | S-03 / S-24 / S-22 |
| 16 | `grep -n "const routes\|const router" apps/shell/src/App.svelte` | `:68` `const routes = createRoutes(...)`；`:81` `const router = createRouter({routes…})` | S-03 |
| 17 | 读 `App.svelte:221-266` | 逐路径 `if/else` 渲染，`{:else}<Home/>` | S-03 / S-24 |
| 18 | 读 `manifest.rs:189-207`、`:247-248`、`:264-266` | `ConfigSpec{type,default,scope?,label?,format?}`；`default_scope() = Tenant` | **S-01 / S-02** |
| 19 | 读 `crates/platform-config/src/service.rs:238-246`、`:277-290` | 平台级读写**只接受 `scope == Platform`** | S-01 |
| 20 | 读 `migrations/20261118001000_config_schema.up.sql:120-135` + `resolver.rs:46-47` | epoch 只在 `platform_config_upsert` 里自增；TTL 60s | S-07 |
| 21 | 读 `crates/platform-config/src/http.rs:96-125` | 写端点分别需 `config:entry:write` / `config:platform:write` | S-07 |
| 22 | `grep -n "u-perm-*" scripts/seed-e2e.sh` | 有 `u-perm-read` / `u-perm-audit`；**无 `u-perm-app`**；无 config 写权限账号 | S-07 / AC-S-156 前置需新增 ✓ |
| 23 | `grep -n "platform_oauth_client" scripts/seed-e2e.sh`（`:414-430`、`:558-562`） | 只建 `client-A` / `client-B` / `public-client`，且强断言 `client-A` 的 logout URI | **S-05 / S-06** |
| 24 | 读 `migrations/20261120000500_oauth_schema.up.sql:43-57` | `platform_oauth_client` **13 列**、无 `tenant_id`、无 RLS | D262 的实测依据 ✓；S-35 |
| 25 | 读 `crates/platform-oauth/src/http.rs:245-263`、`:610-647` | `GET/PATCH/DELETE /clients/{id}` 存在；PATCH **只支持 status**；DELETE → 204 | AC-S-158 路径 2/3 可执行 ✓；S-08 |
| 26 | 读 `crates/platform-db/src/rls.rs:1-40` | 生成 `ENABLE ROW LEVEL SECURITY`（**不是 `FORCE`**）+ `<table>_tenant_isolation` | **D264 的 CASCADE 不会被 RLS 挡住**（owner 绕过）→ 结论：**CASCADE 不破坏 AC-S-135 步骤 7** ✓ |
| 27 | 读 `deploy/seed/02-app-role.sql:99-106` | `ALTER DEFAULT PRIVILEGES … GRANT SELECT,INSERT,UPDATE,DELETE ON TABLES TO base_platform_app` | 新表自动授权 ✅（不必额外写 GRANT，design §7 不列 GRANT 是**对的**） |
| 28 | 读 `crates/platform-server/src/main.rs:1127-1155` + `platform-rbac/src/registry.rs:100-140` | `reconcile_permissions` 失败即 `Failure::Runtime`；`ManifestPermission.source` 逐条带 `crates/…/manifest.json` | **AC-S-165 步骤 2 成立** ✓（日志含编码+源路径） |
| 29 | 读 `crates/platform-contracts/src/authz_scan.rs:36-52` | 8 个别名含 `authorize`；`non_literal_calls` 单列 | **AC-S-165 步骤 3/4 成立** ✓ |
| 30 | `cat crates/platform-oauth/src/events.rs` | `oauth.client.registered`（三段式）；`object_type::CLIENT = "oauth_client"` | **S-12** |
| 31 | 读 `crates/platform-audit/src/verify.rs:78-85`、`:211` | `VerifyReport{ok,checked,broken}`；`verify(&self, ctx)` | AC-S-164「`ok == true`」字段存在 ✓（签名是 `&RequestContext`） |
| 32 | 读 `migrations/20261120000200_audit.up.sql` | `platform_audit_log` 13 列，`ip/user_agent NOT NULL DEFAULT ''` | AC-S-164 字段集 ✓；**S-34** |
| 33 | 读 `BA/contracts/api-conventions/v3.md:45`、`:54` | `/api/app` ← REQ-016；「新增前缀须先升版本」 | **S-14** |
| 34 | `grep -n "nest(" crates/platform-server/src/main.rs` | `/api/iam` `/api/org` `/api/rbac` `/api/audit` `/api/schema` `/api/sandbox`（全单数） | S-14 |
| 35 | 读 `crates/xtask/src/cli.rs:113-128` | `tenant-policies --check` 存在，非零退出 | AC-S-157 步骤 6 ✓ |

### 3.1 BA 要求重点核的 6 个数 —— 核对结果

| 数 | BA 的声称 | 我的实测 | 结论 |
|---|---|---|---|
| `TITLES` 当前 | **93** | 186 字面量 / 2 = **93**；测试常量 `58+21+5+9=93` | ✅ **对** |
| 本轮 +6 → **99** | 6 个 `APP_*` 码 | 6 个码，**逐个有 AC 触发**（156 步6/路2/路3/路4/路5、158 路1/2） | ✅ **对**（且无死码） |
| `permissions.json` 当前 | **32** | 32 | ✅ **对** |
| 本轮 +3 → **35** | 3 个 `app:*` | manifest 设计里 3 个 | ✅ **对** |
| `capabilities/` 文件数 | **14**（「8 + 6」） | **14** ✓，但组成是 **7 固定 + 7 sdk** | ⚠️ **总数对、加数错**（S-15） |
| `platform_app` 列数 | **12** | D265 表 **12 行**（含 `launch_url`）；AC `:131` 12；design `:64` 12；**`decisions.md:423` 仍写 11** | ⚠️ **一处残留**（S-16） |

> **结论**：BA 这轮两次自查修正（15→14、101→99）**都是对的**；但
> ①「14 = 8+6」的**分解算错**（7+7），②`decisions.md:423` 还留着「11 列」，
> ③design `:64` 对 D265 的缺陷描述是**虚构的**（D265 从来没漏 `launch_url`）。
> 三类都是 D275 自己点名的病 —— **同一类错还在文档里**。

---

## 4. 11 条 AC 逐条评估（可原样执行吗？）

| AC | 可原样执行 | 阻塞点 | 一句话 |
|---|---|---|---|
| **AC-S-024** | ❌ | — | 步骤 3 判据恒真（S-10）；且与 REQ-022 重复 |
| **AC-S-156** | ❌ | **S-05** | 13 次 POST 的夹具与**校验顺序**没定 → 断言码取决于实现顺序 |
| **AC-S-157** | ⚠️ 需补 | — | 步骤 5 的 INSERT 缺列，会因 FK 失败（S-23）；上下文手段未写（S-27） |
| **AC-S-158** | ❌ | **S-06** | 删共享夹具 `client-A` → 一条 AC 打掉 4 条 AC + seed 自检 |
| **AC-S-159** | ⚠️ 需补 | — | 没写调用账号与 `appId` 来源（S-26） |
| **AC-S-160** | ❌ | **S-01/S-04/S-07** | 配置改了不生效 + seed 缺 `launch_url` → 0 组；`maxApps` 语义未定 |
| **AC-S-161** | ❌ | **S-01/S-04/S-07** | 同上；`POST /access` 也要求 `launch_url` 非空 |
| **AC-S-162** | ❌ | **S-08/S-09** | 「置空 `launch_url`」无法表达；`labelKey/category/sortOrder` 无来源 |
| **AC-S-163** | ❌ | **S-03/S-09** | 现有壳层里加路由**不生效也无页面**；`router.ts 零改动` 与「运行时扩展」互斥 |
| **AC-S-164** | ⚠️ 需补 | — | 事件码段数与已交付约定冲突（S-12）；`ip` 判据无敏感度（S-34） |
| **AC-S-165** | ❌ | **S-02** | design §6 的 `config` 段非法 → 步骤 1a 第一步就红 |

**能原样跑通 0 条；11/11 都要改。**

---

## 5. 我认为**最危险**的一条

### S-01（配置键 `scope` 缺失）

理由不是「它最严重」，而是**它最不可见**：

1. **每一步看起来都对**：manifest 能解析（`scope` 有默认值）、迁移能写库（`platform_config_upsert` 不问 scope）、
   代码能编译 —— 只有**运行时读配置的路径**在静默过滤（`service.rs:280`）。
2. **失败现象会被误判成业务 bug**：AC-S-160 步骤 5 表现为「改了 `maxApps` 没生效」→
   Dev 去查分页/排序逻辑，BA 判定「maxApps 没实现」→ **一整轮返工**，而真因是一行 `"scope": "platform"`。
3. **它同时挡住两条 AC**（160/161），且这两条是本需求「工作台」的全部验收。
4. 与它同源的 S-02（manifest `config` 段非法）**反而是好事** —— 它会**大声失败**；
   S-01 会**静默成功**，这正是 dev-guide §9.13 说的「按错的规格实现 + 按错的规格验收，自洽闭环」。

**次危险两条**：**S-03**（运行时路由在这套壳层里不成立 —— 设计层死路，Dev 无论怎么写都违反 D270 的某一条）
与 **S-09**（`labelKey` 无来源 —— D269 的字段表凭空多出 3 个 manifest 里不存在的字段）。

---

## 6. 建议的处置顺序（给 BA）

1. **先改 9 条阻塞**（S-01~S-09）—— 这 9 条不改，Dev 写一天就撞墙。
2. **再改 16 条高**，其中 S-12 / S-14 / S-16 / S-17 是**纯口径**，可批量改。
3. 中/低 可选；S-31 / S-32（配额、URL 策略）**记 TD** 即可。
4. 改完后请 `verifier` 按 §9.15.1 逐条反查 `file:line` + 函数名 —— 本报告已给出全部行号。
