# 设计摘要：扩展点框架与契约测试（REQ-003）

- **需求编号**: REQ-003
- **父需求**: [REQ-001](../REQ-001-platform-foundation/demand.md)
- **架构决策**: [ADR-0001 D7](../../decisions/ADR-0001-底座技术架构基线.md)（依赖方向单向）
- **接口契约**: [api-conventions v1](../../contracts/api-conventions/v1.md)
- **性质**: 需求细化附件 —— 把扩展点签名与契约测试的判定方式写死，避免 Dev 自行发明
- **更新时间**: 2026-11-18 00:20

---

## 0. 一句话

平台定义扩展点 trait，业务模块实现 trait，平台按契约测试验证实现是否合规。
**业务模块只依赖 trait，不依赖平台内部模块。**

---

## 1. 扩展点放在哪个 crate

| crate | 内容 |
|---|---|
| `crates/platform-core/src/extension/` | 扩展点 trait 定义（本期首批 5 个） |
| `crates/platform-contracts/` | 契约测试套件 + 简化宏 |
| `crates/xtask/src/cmd/` | `create-module`（本期可只做骨架） |

依赖方向恒为：

```text
platform-core (定义 trait)
      ↑ trait
platform-contracts (依赖 platform-core)
      ↑ trait
业务模块 modules/<name>/backend  (依赖 platform-core + platform-contracts)
```

**平台代码永不反向依赖业务模块。** CI 有一条检查：platform-core 与 platform-contracts 的
依赖图中不得出现任何业务模块路径。

---

## 2. 首批 5 个扩展点（签名本期冻结）

> 冻结的意思：Sprint 2 及以后的业务模块按这份签名实现。后续要改必须走扩展点版本号。

> ### ⚠️ 扩展点版本注记（2026-11-18 BA 补，QA 提出）
>
> **本期起 5 个 trait 为 RPITIT 形态（`-> impl Future<Output = ..> + Send`），不可用于 trait object。**
>
> - 设计摘要原稿写的是 `#[async_trait]` 形态（dyn-compatible）。实现时发现：
>   Rust 1.98 上字面 `async fn` 写在公开 trait 上会触发 `async_fn_in_trait` lint
>   （`clippy -D warnings` 下失败）且无法保证 `Send`；而 `async-trait` 会引入第三方依赖。
>   故改用 RPITIT 展开写法。**实现方仍写 `async fn`，调用点写法不变。**
> - **代价**：`Box<dyn NotificationSender>` 不可用（实测 E0038: trait is not dyn compatible）。
>   异构汇聚（多渠道通知收集、多工作流处理器收集）**延后到 REQ-015 决策**（原同列的 REQ-017 已取消 —— D121），
>   届时要么加对象安全包装，要么引入 `async-trait`。
> - **给 Sprint 2 开发者的提醒**：不要按 `#[async_trait]` 的心智模型写代码
>   （例如不要假设能 `Box<dyn Trait>`、不要假设 trait object 可用）。
>
> 契约后果同步记入 [api-conventions v1](../../contracts/api-conventions/v1.md) 的扩展点章节（见下方 §2.0）。

### 2.1 PlatformModule —— 模块注册

```rust
pub trait PlatformModule: Send + Sync + 'static {
    /// 必须与 manifest.json 的 id 一致，否则契约测试失败
    fn id(&self) -> &'static str;
    /// semver
    fn version(&self) -> &'static str;
    /// 依赖的其他模块 id
    fn dependencies(&self) -> &[&'static str] { &[] }
    /// 启动时调用一次
    fn setup(&self, ctx: &ModuleContext) -> anyhow::Result<()> { Ok(()) }
    /// 是否启用（对应 manifest.json 的 enabled）
    fn enabled(&self) -> bool { true }
}
```

### 2.2 Repository —— 数据访问

```rust
#[async_trait]
pub trait Repository<T: Send + Sync + 'static>: Send + Sync {
    type Id: Send + Sync;

    async fn find(&self, ctx: &RequestContext, id: &Self::Id) -> anyhow::Result<Option<T>>;
    async fn list(&self, ctx: &RequestContext, q: &ListQuery) -> anyhow::Result<List<T>>;
    async fn insert(&self, ctx: &RequestContext, item: &T) -> anyhow::Result<T>;
    async fn update(&self, ctx: &RequestContext, item: &T) -> anyhow::Result<T>;
    async fn delete(&self, ctx: &RequestContext, id: &Self::Id) -> anyhow::Result<()>;
}
```

**关键约束**：`ctx: &RequestContext` 里已经带好租户与空间上下文。
实现方**不得**自己拼 `tenant_id` 过滤条件 —— 契约测试会验证这一点（§4.2）。

### 2.3 Auditable —— 审计埋点

```rust
pub trait Auditable: Send + Sync {
    /// 声明该模块会产生的审计事件
    fn audit_events(&self) -> &[AuditEventSpec];
}
```

平台据此自动埋点，业务模块**不手工写审计表**。

### 2.4 NotificationSender —— 通知投递

```rust
#[async_trait]
pub trait NotificationSender: Send + Sync {
    /// 通道标识，与 manifest.json 的 config 中声明的通道对应
    fn channel(&self) -> &str;
    async fn send(&self, ctx: &RequestContext, msg: &NotificationMessage)
        -> anyhow::Result<DeliveryResult>;
}
```

### 2.5 WorkflowHandler —— 工作流接入

```rust
#[async_trait]
pub trait WorkflowHandler: Send + Sync {
    /// 与流程定义 key 对应
    fn workflow_key(&self) -> &'static str;
    async fn on_transition(&self, ctx: &RequestContext, ev: &TransitionEvent)
        -> anyhow::Result<()>;
}
```

> 本期 REQ-003 只定义 trait 与骨架实现，通知的实际消费方在 REQ-015；工作流扩展点保留但引擎已取消（D121），无平台侧消费方。

---

## 3. 编译期注册

用 `inventory` crate（极小，无额外依赖负担）做编译期自动注册：

```rust
// crates/platform-core/src/extension/registry.rs
inventory::collect!(PlatformModule);

// 使用侧
pub fn discover() -> Vec<&'static dyn PlatformModule> {
    inventory::iter::<dyn PlatformModule>.into_iter().collect()
}
```

**为什么不用运行期动态插件**：ADR D5 已定 —— 运行期插件带来安全面、调试难、回滚难。
编译期注册有类型安全、性能可控、单测友好。

> `inventory` 是本期**唯一新增**依赖，约 3KB，允许在 REQ-003 引入。

---

## 4. 契约测试套件设计

`crates/platform-contracts` 提供。业务模块在自己的 `tests/contracts.rs` 里接入：

```rust
use platform_contracts::prelude::*;

#[platform_module_contract]
mod order {
    // 套件自动跑下列检查：
    //   C-1 manifest 与实现一致（id / version / enabled）
    //   C-2 权限点声明完整（代码里用的都在 manifest 里）
    //   C-3 租户隔离生效（多租户数据集下不串数据）
    //   C-4 事务边界符合约定
}
```

### 4.1 C-1 manifest 与实现一致

| 检查 | 判定 |
|---|---|
| `manifest.id` == `module.id()` | 不一致则失败 |
| `manifest.version` == `module.version()` | 不一致则失败 |
| `manifest.enabled` == `module.enabled()` | 不一致则失败 |
| `manifest.dependencies` == `module.dependencies()` | 不一致则失败 |

### 4.2 C-3 租户隔离生效（最重要）

**行为测试，不是静态检查。** 套件准备多租户数据集：

```text
tenant_a  user_a@example.com  Order#1  "同名订单"
tenant_b  user_b@example.com  Order#2  "同名订单"   ← 唯一区别是租户
```

然后：

1. 以 `tenant_a` 的 `RequestContext` 调 `repo.list()`
2. 断言结果**只含** tenant_a 的记录
3. 以 `tenant_a` 的 context 调 `repo.find(tenant_b 的 id)`
4. 断言返回 `None`（不是报错，也不是查到别人数据）

**这条能抓住绝大多数越权事故**：如果实现方漏了 `tenant_id` 过滤，测试直接红。

> 实现上，`RequestContext` 里的租户由平台注入；实现方若绕过 ctx 自己去拼 where，
> 数据访问层拿不到 ctx —— 所以契约测试**只能**通过 ctx 调，实现方想绕也绕不掉。
> 如果后续允许模块直接拿 pool，那这条测试必须同步升级为检查 SQL。

#### BA 澄清（2026-11-18，本期实现口径）

**平台本期没有 DB 访问层**（sqlx 刻意推到 REQ-006/007），因此 **C-3 本期不连数据库**。
它要验的是**契约语义**，由两条保证构成：

1. **编译期保证** —— `Repository` 的每个方法都必须传 `ctx: &RequestContext`，
   实现方没有任何别的途径拿到租户，漏传就编译不过
2. **行为验证** —— 套件提供双租户夹具，用 `ctx` 里指定的租户调 `list()` 断言只返回该租户数据；
   用同一 ctx 调 `find()` 查另一租户的 id，断言返回 `None`

**示例模块 `modules/sandbox/` 用内存存储实现 Repository**（不引入 sqlx），
数据按 `ctx.tenant_id()` 过滤，契约测试因此能真实跑通 C-3。

等 REQ-006 落地真实 DAO 层后，C-3 再叠加一层「真实库多租户数据集」验证。

### 4.3 C-2 权限点声明完整

遍历模块代码里用到的权限点常量，与 `manifest.json` 的 `permissions[].code` 比对：

- 代码里用了但 manifest 没声明 → **失败**（前端拿不到 `can()` 判定）
- manifest 声明了但代码没用到 → **警告**（不失败，可能是预留）

权限点编码遵守契约 C4：`module:resource:action`，全小写连字符。

> ### C-2 的口径边界（2026-11-18 BA 定，QA 提出后澄清）
>
> **C-2 只验「权限点声明完整性」，不验「服务端是否真的执行了鉴权」。**
>
> 原因：当前扫描基于源码中的权限点字面量，只能回答「声明齐不齐」，
> 回答不了「每个权限点是否真的在服务端被检查过」—— 后者需要运行时插桩或鉴权中间件，
> 属 **REQ-009 RBAC** 的职责（那里才有真正的鉴权执行点与越权测试）。
>
> **已知局限**（不阻塞，写入技术债）：一个只在 `permission.rs` 里定义、manifest 里也登记、
> 但服务端从不做鉴权判断的权限点，C-2 会全绿且无告警。
> 等 REQ-009 落地鉴权中间件后，应补一条「已登记权限点必须有对应鉴权调用点」的检查。

### 4.4 C-4 事务边界

约定：一次 `insert` / `update` / `delete` 是一个事务；跨表写操作必须显式开事务。
套件检查模块是否在写多张表时使用了平台的事务辅助（而不是裸连接）。

---

## 5. manifest.json schema（正式版）

放在 `crates/platform-core/src/extension/manifest_schema.rs`，同时导出 JSON Schema 到
`capabilities/schema/manifest.schema.json`（REQ-004 的能力索引收录）。

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "title": "Base Platform Module Manifest",
  "type": "object",
  "required": ["id", "name", "version", "enabled", "permissions", "menus", "dependencies"],
  "additionalProperties": false,
  "properties": {
    "id":     { "type": "string", "pattern": "^[a-z][a-z0-9]*(-[a-z][a-z0-9]*)*$" },
    "name":   { "type": "string", "pattern": "\\S" },
    "version":{ "type": "string", "pattern": "^\\d+\\.\\d+\\.\\d+$" },
    "enabled":{ "type": "boolean", "default": true },
    "permissions": {
      "type": "array",
      "items": {
        "type": "object",
        "required": ["code", "name", "type"],
        "additionalProperties": false,
        "properties": {
          "code": { "type": "string", "pattern": "^[a-z][a-z0-9-]*:[a-z][a-z0-9-]*:[a-z][a-z0-9-]*$" },
          "name": { "type": "string", "minLength": 1, "pattern": "\\S" },
          "type": { "enum": ["menu", "button", "field", "data"] }
        }
      }
    },
    "menus": {
      "type": "array",
      "items": {
        "type": "object",
        "required": ["label", "route"],
        "additionalProperties": false,
        "properties": {
          "label": { "type": "string" },
          "icon":  { "type": "string" },
          "route": { "type": "string", "pattern": "^/[a-z0-9/-]*$" },
          "perm":  { "type": "string" }
        }
      }
    },
    "dependencies": {
      "type": "array",
      "items": { "type": "string", "pattern": "^[a-z][a-z0-9]*(-[a-z][a-z0-9]*)*$" }
    },
    "config": {
      "type": "object",
      "additionalProperties": {
        "type": "object",
        "required": ["type", "default"],
        "properties": {
          "type":    { "enum": ["string", "number", "boolean", "array", "object"] },
          "default": {},
          "scope":   { "enum": ["platform", "tenant", "space"], "default": "tenant" },
          "label":   { "type": "string" }
        }
      }
    }
  }
}
```

> **2026-11-18 修正（QA 发现 schema 与平台实现判定不一致）**
>
> 上一版 schema 与实现有三处分歧（方向是「平台更严」，无安全洞，但 schema 是对外发布的，
> 第三方拿它校验会与平台判定不一致）。现已**把 schema 收紧到与实现完全一致**：
>
> | 字段 | 改动 | 原因 |
> |---|---|---|
> | `properties.id.pattern` | `^[a-z][a-z0-9]*(-[a-z0-9]+)*$` → `^[a-z][a-z0-9]*(-[a-z][a-z0-9]*)*$` | 原正则允许分段以数字开头（`a-1`、`sandbox-1demo`），实现不允许 |
> | `properties.dependencies.items` | 补上同款 pattern | 原 schema 完全没有约束，实现却按 id 规则校验 |
> | `properties.name` | 追加 `"pattern": "\\S"` | 只补 `minLength:1` 的话纯空白串仍与实现不一致 |
> | `properties.permissions.items.properties.name` | 补 `minLength:1` + `pattern:"\\S"` | 同上 |
>
> **判定一致性由测试保证**：`schema_and_implementation_agree_on_module_id_samples`
> 用**独立于平台实现的极简正则求值器**读 schema 的 pattern，与实现对同一批样本比对，
> 避免「用实现验实现」的循环论证。

---

## 6. 目录规范校验（`xtask check-modules`）

REQ-002 已放骨架，REQ-003 补齐规则。校验项：

| 校验 | 失败信息 |
|---|---|
| `modules/<name>/` 下必备文件齐全 | 列出缺失的文件 |
| `manifest.json` 符合 §5 的 JSON Schema | 给出字段路径与原因 |
| `manifest.id` 与目录名一致 | 说明应以哪个为准 |
| `backend/`、`frontend/`、`migrations/` 非空 | 指出空目录 |
| `AGENTS.md`、`README.md` 存在 | 列出缺失 |

---

## 7. 本期（REQ-003）交付边界

**做**：

- [ ] 5 个扩展点 trait 定义与文档注释
- [ ] `inventory` 编译期注册与 `discover()`
- [ ] `platform-contracts` 套件：C-1 ~ C-4 四项检查
- [ ] `#[platform_module_contract]` 宏（简化版）
- [ ] manifest JSON Schema + 解析器
- [ ] `xtask check-modules` 完整规则
- [ ] 一个示例模块 `modules/sandbox/`（**作为二次开发的标准范例**）

**不做**（留给后续 Sprint）：

- 通知通道的实际消费（REQ-015）
- ~~工作流的实际消费（REQ-017）~~ —— REQ-017 已取消（D121），`WorkflowHandler` 保留供业务方自接
- 微前端容器（ADR A4 明确不实现）
- `xtask create-module` 的完整实现（骨架放 REQ-002，功能完善放 REQ-004）

---

## 8. 内存约束提醒

本机 4.9G 可用内存。`platform-contracts` 的契约测试会起数据库连接，
**测试用 `base_test` 库，复用已起的 podman PG 容器，不要用 testcontainers**（每个测试环境起新容器 = +200-400M）。

```bash
DATABASE_URL_TEST=postgres://base:base@localhost:5432/base_test
cargo test -p platform-contracts -- --test-threads=2
```
