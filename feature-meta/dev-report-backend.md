# REQ-012 后端部分 —— Dev 自验报告

- **需求**: REQ-012「元数据驱动渲染」（**后端部分**：AC-S-076 / 077 / 078 / 079 / 080 / 081）
- **分支 / worktree**: `feature/REQ-012` @ `/home/myg133/Projects/system_seed/feature-REQ-012`
- **基线**: `737b384`（前端 Dev 的提交）
- **提交**: `e6e092f` `[Dev] REQ-012 后端：元数据链路与字段级权限 (关联: REQ-012)`
- **实施人**: Dev（后端棒）
- **前端部分**: 另一个 Dev（`feature-meta/dev-report-frontend.md`），本报告不覆盖

> 结论先说：**AC-S-076/077/078/079/080/081 全部实现并真机验证通过**。
> 过程中在**真机**上抓到一个单测测不出的缺陷（模块路由缺认证层 → 401），已修并复验。
> 另有 **4 处 BA/QA 产物与既有契约/实现冲突**，我按「契约优先」处理并逐条列出（§5）。

---

## 1. 交付内容

| # | 内容 | 位置 |
|---|---|---|
| 1 | manifest 的 `schema` 段（形状 + **阶段 1** 聚合校验） | `crates/platform-core/src/extension/schema.rs`（新）、`manifest.rs` |
| 2 | manifest 的 JSON Schema 补 `schema` 定义 | `crates/platform-core/src/extension/manifest_schema.rs` |
| 3 | 元数据接口 `GET /api/schema/<module>/<entity>` + **阶段 2** 全局校验 | `crates/platform-schema/`（新 crate） |
| 4 | `SchemaProjector`（唯一序列化入口，白名单语义） | `crates/platform-core/src/extension/schema.rs` |
| 5 | 契约 **C-6** + 负向用例 | `crates/platform-contracts/src/schema_scan.rs`（新）、`checks.rs`、`tests/c6_schema_projection.rs`（新）、`platform-contracts-macros` |
| 6 | sandbox 挂进服务（D103）+ `demoItem` 完整 CRUD + **认证层** | `crates/platform-server/src/main.rs`、`modules/sandbox/**` |
| 7 | `sandbox:demo:secret` 权限点三件套（D27/D67） | `modules/sandbox/manifest.json`、`migrations/20261119000100_*.up.sql`（新） |
| 8 | 建表迁移（tenant_id；RLS 由平台按 manifest 生成） | `modules/sandbox/migrations/20261119000100_sandbox_demo_item.up.sql`（新） |
| 9 | 能力索引 + `AGENTS.md`「已有能力」段（**同改**） | `crates/xtask/{catalog,capabilities}.rs`、`AGENTS.md`、`capabilities/**` |
| 10 | 依赖方向规则的**装配层例外** | `crates/xtask/src/deps.rs` |

**没有改动**：`packages/`、`apps/shell/`、`e2e/`（另一个 Dev 的）；`scope-item`（REQ-009 验收资产）。

---

## 2. 关键设计落点（可核对的判据）

### 2.1 两阶段校验

| 阶段 | 何时 | 在哪 | 覆盖 |
|---|---|---|---|
| 阶段 1（单文件，**聚合**） | `Manifest::validate()` | `EntitySchema::validate` | 实体键格式 / `fields` 非空 / 字段 `key`/`label`/`type` / 同实体 key 不重复 / `api` 前缀 |
| 阶段 2（跨文件） | 全部 manifest 加载后 | `SchemaRegistry::from_manifest_root` | 字段 `permission` 已登记、`reference.module/entity` 存在 |

- **聚合**保留：`issues` 是 `Vec`，单条违规 → 1 条、两条违规 → 2 条（§4.3 有真机输出）。
- 阶段 1 内部顺序按 design §2.4.3 写死；`type` 的错误信息带**允许集合**。
- 阶段 2 失败 → `Failure::Runtime` → **退出码 1**，逐条 `manifest 路径: 字段: 原因`。

### 2.2 字段类型枚举

唯一来源是 Rust 的 `FieldType::ALL`（5 个）；`capabilities/schema/manifest.schema.json` 与
`schema/entity.schema.json` 都由 `cargo xtask capabilities` 从它生成 —— 手改会被覆盖、`--check` 会红。

### 2.3 白名单语义（fail closed）

`SchemaProjector::project` **重建对象**：只保留「元数据声明过 + 当前用户有权」的键。
`sandbox_demo_item` 表故意多两列不写进元数据（`internal_note` / `created_at`），真机确认这两列
**从不出现**（带全部权限也不出现）—— 这是「加字段忘写元数据时少显示而不是多泄露」的活证据。

### 2.4 元数据与数据**共用同一判定**

`EntitySchema::visible_fields` 与 `SchemaProjector::project` 都调
`field_is_visible(field, ctx, checker)` —— 两处各写一套 = 「元数据说能看到、数据却剔掉」的分叉。

### 2.5 C-6（判据 = 「至少一处」）

`SchemaProjector::project` 必须出现在源码里（剥注释 + 抹字符串后扫描；注释/字符串不算）。
负向用例 5 条：合规通过 / 无调用时失败并点名模块+实体+字段 / **只有一处也通过**（把局限固化成会红的测试）/
注释与字符串不算 / 未声明字段权限不误报。

---

## 3. 改动清单（36 个文件）

**新增**

- `crates/platform-core/src/extension/schema.rs`
- `crates/platform-schema/{Cargo.toml,src/lib.rs,src/registry.rs,src/http.rs,tests/metadata_api.rs}`
- `crates/platform-contracts/src/schema_scan.rs`、`crates/platform-contracts/tests/c6_schema_projection.rs`
- `modules/sandbox/backend/src/{demo_item.rs,demo_item_http.rs}`、`modules/sandbox/tests/demo_items.rs`
- `modules/sandbox/migrations/20261119000100_sandbox_demo_item.{up,down}.sql`
- `migrations/20261119000100_sandbox_demo_secret_permission.{up,down}.sql`
- `capabilities/schema/entity.schema.json`（生成）

**修改**

- `crates/platform-core/src/extension/{manifest.rs,manifest_schema.rs,mod.rs}`、`crates/platform-core/src/problem.rs`（新增 2 个错误码常量）
- `crates/platform-contracts/src/{checks.rs,lib.rs,authz_scan.rs}`、`crates/platform-contracts-macros/src/lib.rs`
- `crates/platform-server/{Cargo.toml,src/main.rs}`
- `modules/sandbox/{Cargo.toml,manifest.json}`、`modules/sandbox/backend/src/{http.rs,mod.rs}`
- `crates/xtask/src/{catalog.rs,capabilities.rs,deps.rs}`、`AGENTS.md`、`Cargo.{toml,lock}`、`capabilities/**`

---

## 4. 验证（跑过的命令与观察到的结果）

### 4.1 静态与单测

| 命令 | 结果 |
|---|---|
| `cargo clippy --workspace --all-targets -- -D warnings` | **0 告警** |
| `cargo test -p platform-core --lib` | **96 passed** |
| `cargo test -p platform-schema` | **11 + 9 passed** |
| `cargo test -p platform-contracts --test c6_schema_projection` | **5 passed** |
| `cargo test -p sandbox --test contracts` | **6 passed**（含 `contract_c6_schema_projection_is_used`） |
| `cargo test -p sandbox --test demo_items -- --include-ignored` | **7 passed**（真库） |
| `cargo test -p xtask --all-targets` | **51 passed** |
| `cargo xtask capabilities --check` | **退出 0**（14 个文件与源码一致） |
| `./scripts/test-integration.sh` | **全绿**：33 个 `test result: ok`、无 FAILED、`容器数量比对通过：2 → 2` |

依赖预算：`Cargo.lock` 包数 295 → **296**，新增的唯一条目是**本仓库的新 crate**
`platform-schema`（路径依赖）。**零新增第三方包**（`git diff Cargo.lock` 只有这一条 `[[package]]`
与各 crate 的依赖列表联动）。

### 4.2 真机验证（8094；8093 被另一个 session 的 e2e 服务占用，未去杀别人的进程）

启动参数：`SERVER_PORT=8094` + `DATABASE_URL=postgres://base_app:…/base_test` + `BASE_MANIFEST_ROOT=.`
+ PKCS#8 EC 私钥 + base64url `BASE_IAM_SECRET_KEY`。启动日志确认：

```
INFO 租户隔离前提自检通过：连接角色不绕过 RLS role=base_app
INFO 元数据读取已启用：/api/schema/<module>/<entity>
INFO 业务模块已挂载：/api/sandbox/*（sandbox）
```

| 用例 | 观察 |
|---|---|
| 不带令牌 `GET /api/schema/sandbox/demoItem` | **401** `UNAUTHENTICATED`，`instance` = 原始路径，body = RFC 9457（7 键，键名 `trace_id`） |
| `GET /api/schema/notAModule/demoItem` | **404** `SCHEMA_MODULE_NOT_FOUND` |
| `GET /api/schema/sandbox/notAnEntity` | **404** `SCHEMA_ENTITY_NOT_FOUND` |
| 授予 `sandbox:demo:secret` **之前** | **200**，`fields` = `[id,title,amount,paid,dueAt,ownerId]`（6 个），全文无 `"secret"`、无 `内部备注`、无 `null` |
| 走 `POST /api/rbac/roles/e2e-role-write/permissions` 授予 `sandbox:demo:secret` | 204；**同一个令牌的下一个请求**元数据即 7 个字段（`…,ownerId,secret`） |
| `POST /api/sandbox/demo-items`（有 read+write+secret） | **201**，响应含 `secret:"真机内部备注"`，且 `internalNote`/`createdAt` **不在** |
| `GET /api/sandbox/demo-items`（有 secret） | 信封键 = `{items,nextCursor}`；每条 7 键，`demo-1.secret = "内部-A"`（= 库里的值） |
| `DELETE …/permissions/sandbox:demo:secret` 后的下一个请求 | 元数据回到 6 个字段；列表 raw **`"secret"` 完全不出现**（无 `null` / `[REDACTED]` / `***`），其余字段仍在 |
| `DELETE /api/sandbox/demo-items/{id}` ×2 | 先 **204**，再 **404** |

### 4.3 进程级 fail-fast（AC-S-076 步骤 2~7，`BASE_MANIFEST_ROOT=/tmp/mroot`）

| 场景 | 结果 |
|---|---|
| 合法 manifest | `serve` 正常监听（`timeout 40` → 124 = 被杀，说明起来了）；`tenant-policies --check` 退出 0 |
| `type: "currency"` | `serve` **退出 1**，点名 `schema.demoItem.fields[4].type` + `` `currency` `` + 允许集合；**issues 恰好 1 条** |
| `fields: []` | 退出 1，`schema.demoItem.fields: must not be empty` |
| `type` 非法 **且** key 重复 | 退出 1，**两条都报**（`fields[2].type` 与 `fields[3].key`）= 聚合校验 |
| `permission: "sandbox:demo:nope"` | 退出 1，`…fields[6].permission: the permission point \`sandbox:demo:nope\` is not declared in any manifest` |
| `reference: {org, notAnEntity}` | 退出 1，`…fields[5].reference.entity: the reference points to \`org/notAnEntity\`, but module \`org\` declares no entity \`notAnEntity\`` |

> 路径 B（`base tenant-policies --check`）只覆盖**阶段 1**：它只解析 manifest、不建元数据注册表。
> 阶段 2 的两条要靠 `base serve`（路径 A，需真库）。

### 4.4 变异验证（3 条必做，全做）

| # | 变异 | 观察 |
|---|---|---|
| 1 | `SchemaProjector::project` 直接 `return raw` | `demo_items`：**4 failed / 3 passed**（无 secret 键、创建/更新投影、信封、未声明字段四条全红） |
| 2 | 元数据接口不过滤权限（下发全部 fields） | `metadata_api`：**3 failed**（无权限剔除、授予后立即生效、无权限用户仍可读三条） |
| 3 | `check_c6_for` 永远 `Ok(vec![])` | `c6_schema_projection`：**3 failed**（无调用失败、注释/字符串、单处调用三条） |

每条变异后都从备份恢复并复跑至全绿（计数见 §4.1）。

---

## 5. 偏差与裁定请求（★ 需要 BA/Planner 看一眼）

### 5.1 ★ 元数据响应**包含 `api`**（与 TC-012-014 / TC-012-023 的「4 键集合」冲突）

- design §3.2 的示例没有 `api`；TC-012-014 断言 `Object.keys(body).sort() == ["entity","fields","label","module"]`。
- 但 D102 加 `api` 子段的目的就是「让前端知道去哪取数」，而元数据接口是**唯一**通道。
- **决定性证据**：前端 Dev 在 `737b384` 提交的 `packages/schema/src/index.ts` 里写死
  `interface EntityMeta { module; entity; label; api: ApiEndpoints; fields }` —— `api` 是**必填**字段。
  不返回 `api`，AC-S-085/086 的闭环直接断。
- 实现：响应 = `{module, entity, label, api, fields}`。
- **请求**：把 TC-012-014 / TC-012-023 的顶层键集合改成 5 个键（含 `api`）。

### 5.2 `trace_id` 不是 `traceId`（TC-012-019）

- AC/TC 写 `traceId`；`api-conventions v1` C6 的样例与既有 401/403/404（iam/org/rbac/config）
  **全是 `trace_id`**（`platform-core/src/problem.rs` 的字段名就是 `trace_id`）。
- 我按**契约**实现，没有为单个用例改全平台错误体形状。
- **请求**：把 TC-012-019 的键名改成 `trace_id`。

### 5.3 TC-012-019 的「`"title"` 出现次数 == 0」**必然失败**

- RFC 9457 的错误体**本身**就有 `title` 键（本仓 401 的 `title = "未认证"`）。
  与 QA #4（「全文不出现 `b`」）是同一类错误：把信封自身的键当成业务字段。
- 我的用例改为断言「不出现 `demoItem` / 示例条目 / `fields`」，并保留「错误体键集合精确 = 7 键」。
- **请求**：修掉这条不可执行断言。

### 5.4 `ownerId` 的 `reference` 指向 `sandbox/demoItem`（不是 `org/user`）

- 夹具 §3.1 写 `{ "module": "org", "entity": "user" }`，但 **`crates/platform-org/manifest.json`
  没有 `schema` 段**，所以阶段 2 会判「org 没有实体 user」→ **进程起不来**，
  与 AC-S-076 步骤 2（合法 manifest 应启动成功）直接冲突。
- `crates/platform-org/` **不在本次 Dev 的文件范围**，且给 org 加实体意味着要同时编造它的
  `api` 地址（= 声明一个不存在的接口）。
- 处理：`ownerId.reference = { module: "sandbox", entity: "demoItem" }`（自引用，语义勉强但**可解析**）；
  TC-012-009（改成 `org/notAnEntity`）因此仍按预期失败。
- **请求**二选一：(a) 接受自引用并改夹具；(b) 下一棒给 `crates/platform-org` 补一个真实的
  `schema.user`（含真实 `api` 路径），我再把 reference 改回去。

### 5.5 C-5 / C-2 现在**排除** `type: "field"` 的权限点

- 字段级权限点的消费方是 `SchemaProjector`（元数据里动态引用），**不会**有
  `require_permission("字面量")` 调用点，也不会以字面量出现在源码里。
- 不排除的话，C-5 会对每个**合法**字段级权限点报「没有鉴权调用点」→ 误报 → 检查会被关掉。
- 排除后由 **C-6** 守它（声明了字段权限就必须有投影调用点）。C-5 其余判据未放松。

### 5.6 `platform-server` 可以依赖业务模块（依赖方向规则的**显式例外**）

- `crates/xtask/src/deps.rs` 原来禁止**一切**平台 crate 依赖 `modules/`，与 D103「把 sandbox 挂进服务」冲突。
- 处理：新增 `ASSEMBLY_CRATES = ["platform-server"]`（装配层 = 可执行交付物），在检查集合里排除它；
  **库 crate（core/db/contracts/…）一条没放松**。
- 守卫测试 `only_the_assembly_layer_may_depend_on_business_modules`：`platform-server → sandbox` 不报，
  `platform-core → sandbox` 仍报 1 条（只测前者的话，把规则删掉也能过）。

### 5.7 ★ 真机抓到、单测抓不到的缺陷（已修）

- **现象**：`GET /api/sandbox/demo-items` 真机 **401**（`authentication is required`），连既有
  `/api/sandbox/scope-item` 也是 401。
- **根因**：`platform-server` 只 `nest` 了模块 router，**没有任何认证中间件** —— 业务模块不依赖
  IAM/RBAC，`require_permission` 拿不到 `RequestContext.user_id` → 401。单测（`http_authz.rs`）
  是**手动**把 ctx + checker 塞进 extensions 的，所以一直是绿的（REQ-011 的「单测绿 ≠ 能用」在此复现）。
- **修法**：装配层新增 `ModuleAuthState` + `authenticate_module`（验令牌 → 建租户/用户上下文 →
  `ensure_fresh` 刷新判定缓存 → 注入 checker），套在 `/api/sandbox` 外层。
- **修后**：真机 401 → 200，两态切换即时生效（§4.2）。

### 5.8 其它小偏差

| # | 偏差 | 说明 |
|---|---|---|
| a | 记录里**没有** `createdAt` 输出 | TC-012-025 期望键集合含 `createdAt`，但它不在元数据里 → 白名单必然剔除（正是 TC-012-028 要的语义）。两条用例互相矛盾，我按白名单语义走 |
| b | 没有 `GET /api/sandbox/demo-items/{id}` 详情接口 | AC-S-080 钉死的是列表接口；TC-012-030（详情同样剔除）本轮**未实现**，列为后续（C-6 判据也抓不到，design §4.4 已承认该局限） |
| c | 真机验证用 **8094** | 8093 被另一个 session 的 e2e 服务占用（`BASE_STATIC_DIR=apps/shell/dist`） |
| d | `capabilities/sdk/schema.json` 有变化 | 前端 Dev 的 `packages/schema/src/index.ts` 已提交但索引未重生成；我跑生成器时一并收敛（`--check` 因此为绿） |
| e | `capabilities/api.json` 仍是占位 | 本轮没做机器可读的接口清单（AC 未要求）；`http_api_layer` 的 not_yet 条目**未改**（那段不在我的授权范围内） |
| f | `sandbox` 新增 `uuid` / `time` 依赖 | 两个都已在依赖树里（零新包）；用于主键与 `created_at` |

---

## 6. 阻塞 / 剩余风险

- **无阻塞**：六条后端 AC 的自动化 + 真机验证都通过。
- 风险 1：**C-6 判据弱**（「至少一处」）——「3 个 handler 只有 1 个调用」能过。
  已按 design §4.4 保留，TC-012-033 会在判据被改强时变红；主路径由 AC-S-080 的列表用例兜住。
- 风险 2：§5.4 的 `reference` 自引用是**夹具妥协**，不是好示例。若 BA 补 `org` 的 schema 段，改动很小。
- 风险 3：阶段 2 只在 `base serve` 执行（`tenant-policies --check` 只覆盖阶段 1）。
  只跑 `tenant-policies` 而从不 `serve` 的部署发现不了「字段权限点未登记」—— 但那种部署也没有可访问的前端。
- 风险 4：`demo_items` 用例按**租户隔离**播种（每个用例自己的 `tenant-demo-*`），
  已在 `--test-threads=2`（CI 口径）下跑过：7 passed，互不影响。
