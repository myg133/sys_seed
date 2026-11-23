# REQ-020 规格独立挑刺

> 独立 QA · 未参与规格编写 · 2026-11-23
> 基线：`code` @ `develop` `1be47f5`（`modules/leave` 尚不存在，已实测确认）
> 方法：静态阅读 + grep + 实测命令；**未跑** `cargo test` / E2E / 真库（本机低配，且消费方代码不存在）

## 结论

**VERDICT: FAIL**

**不能开工。** 不是措辞问题，是三条结构性矛盾：

1. **「消费方零改动」与「消费方跑得起来」在当前平台上互斥。** 业务模块要被服务加载，
   必须在 `crates/platform-server/src/main.rs` 里加一条硬编码 match 臂
   （`main.rs:582-586` 对未接线模块只 `warn!`「no HTTP mount point」）；
   另外根 `Cargo.toml:21` 的 workspace members 是**显式清单**，不加就编不进工作区。
   前一处是 `crates/**` → AC-S-186 判红；后一处**不在任何 AC 的测量范围内**。
2. **AC-S-190 的 BASE 与 AC-S-186 的 BASE 被规格钉成同一个 commit**（`test-cases/04-upgrade.md:66`），
   而 AC-S-186 的 BASE 是「实现开始前」（`acceptance.md:31`）
   → `git diff --name-only BASE -- modules/leave/` 在实现后必然列出全部新增文件
   → **AC-S-190 步骤 1「空输出」必然失败**，与实现质量无关。
3. **升级演示的事实前提写错了。** 规格说 v1 是「裸数组」（`design-summary.md:95`），
   实际是 `{"permissions":[...]}` 对象（`crates/platform-rbac/src/http.rs:428-431`），
   且该文件 422-425 行明写「为什么是**对象**而不是裸数组 …… 改形状是破坏性变更」。照规格实施，TC-020-026 的 `is_array()` 必红。

另加一条判据级硬伤：**AC-S-189⑤（通知必须落 `platform_notify_delivery`）在当前平台上不可达** ——
`NotificationSender` 的现有实现（InApp/Smtp/Webhook）**都不写投递表**，唯一写表入口是
`platform-notify` 的 `NotifyService::enqueue`，而 AC-S-193 明令业务不得依赖它。

**最该先改的一条**：把「平台改动」的口径从「`crates/**` 零改动」改成**可测量集合**
（`crates/**` + 根 `Cargo.toml` + `capabilities/**` + `apps/shell/**` + `packages/**`），
并把「消费方装配」明确列为**平台缺口实测项**（允许改、必须登记、必须归因），
否则这份规格要么判自己红，要么靠漏测变绿。

## FINDING 清单

| # | 严重级 | 位置 | 问题 | 是否阻塞实现 |
|---|---|---|---|---|
| B1 | BLOCKER | `crates/platform-server/src/main.rs:553-586`；`Cargo.toml:21,49`；`crates/platform-server/Cargo.toml:65` | 业务模块须在 main.rs 加硬编码装配臂 + 根 Cargo.toml 加 member，二者与「平台零改动」互斥；后者不被任何 AC 计量 | **是** |
| B2 | BLOCKER | `acceptance.md:31,141-147`；`test-cases/04-upgrade.md:62-68` | AC-S-190 与 AC-S-186 共用「实现前」BASE → `modules/leave/` 的 diff 必然非空，「零改动」永远判不成立 | **是** |
| B3 | BLOCKER | `design-summary.md:93-99`；`04-upgrade.md:13-16,37-42` | v1 响应形态写成「裸数组」，实际是对象；`platform-rbac/src/http.rs:422-431` 明写「改形状是破坏性变更」 | **是** |
| B4 | BLOCKER | `acceptance.md:125,210-216`；`test-cases/03-approval.md:96-107` | 「通知必须落 platform_notify_delivery」与「不得依赖 platform-notify」不可兼得；无任何 `NotificationSender` 实现写该表 | **是** |
| B5 | BLOCKER | `acceptance.md:53-75`；`demand.md:131` | 500 行预算结构上不可达：脚手架骨架 backend 405 + frontend 45 = **450 行**，剩 50 行装不下 DB 仓储替换 + 三态机 + 审计 + 通知 | **是** |
| B6 | BLOCKER | `acceptance.md:38-48`；`design-summary.md:124-136` | 「平台改动 = `crates/**`」漏掉真实必需改动（`capabilities/permissions.json`、根 `Cargo.toml`、`apps/shell/**`、`packages/**`）；且 AC-S-186 自带的 `capabilities --check` 会**强制**产生口径外改动 | **是** |
| M1 | MAJOR | `acceptance.md:36,43` | 把 `check-deps` 的退出码 0 解释为「新模块依赖方向合规」，实际只校验**反向**依赖（`xtask/src/deps.rs:217-224`） | 否 |
| M2 | MAJOR | `acceptance.md:96`；`test-cases/02-reuse.md:60-71` | ④ 的 `NewDelivery\|notify` 与 AC-S-193 冲突、且对 `NotificationSender` 假红（`notify` 大小写敏感，不匹配 `Notification`）；AC 与 TC-020-014 的模式不一致 | 否 |
| M3 | MAJOR | `acceptance.md:93`；`test-cases/02-reuse.md:50-56` | `INSERT INTO.*audit` 无区分力：平台自身写入走 `platform_audit_append`（`sink.rs:114-115`），多行 SQL 直接绕过；且全仓唯一命中在测试夹具里，无阳性对照 | 否 |
| M4 | MAJOR | `acceptance.md:102-104`；`test-cases/02-reuse.md:22-25` | 指定的阳性对照（sandbox）对 `sqlx::query` 的命中就是 **0**（实测）→ 按其自身 §9.2 规则「结论作废」；有效对照在 `platform-db`（10 命中） | 否 |
| M5 | MAJOR | `acceptance.md:121-127`；`test-cases/03-approval.md:82-107` | AC 说「每次流转 1 条审计 / 每次状态变更 1 条通知」，TC 只断 approve 的 1 条通知、却把 created 算进 3 条审计 → 数量口径不一致 | 否 |
| M6 | MAJOR | `design-summary.md:98-99`；`04-upgrade.md:37` | 「以 `docs/contracts/api-conventions/v2.md` 为准」无依据：该文只改 C9/C10（配置、Flag），全仓无任何 HTTP 版本协商约定 → TC-020-028「按约定请求」不可执行 | 否 |
| M7 | MAJOR | `acceptance.md:185`；`design-summary.md:68-71`；`apps/shell/src/lib/routes.ts:41-47`；`SchemaTable.svelte:65-67,131-148` | 「shell 已装配模块页面」不存在；唯一数据页路由是 `/schema/:moduleId/:entityKey`，动作列只有编辑/删除，无审批 → AC-S-192 步骤 3 必须改 `apps/shell/**`（不被任何 AC 计量） | 否 |
| M8 | MAJOR | `design-summary.md:132`；`docs/contracts/README.md:3,34` | 平台改动清单第 3 项要求改 `docs/contracts/`，但该目录是快照，「不要手改」且 CI 校验一致；正本在 BA `demand` 分支，不在本仓 diff 里 | 否 |
| M9 | MAJOR | `acceptance.md:164-178`；`design-summary.md:157`；`test-cases/05-metrics-e2e.md:70-72` | AC-S-191 违反自己的判据：`测试通过数` 一行只有输出模式没有取数命令；E2E `skipped` 的取数方式未定义（`playwright.config.ts:22` 只有 `list` reporter） | 否 |
| M10 | MAJOR | `acceptance.md:193`；`test-cases/05-metrics-e2e.md:69-72`；`e2e/audit.spec.ts:157` | 「`skipped == 0`」未定义范围：现有 suite 存在条件 skip，全量跑与只跑本 spec 会得出不同数 | 否 |
| N1 | MINOR | `acceptance.md:109,112`；`design-summary.md:32-39` | 「三态」实为 4 个取值；状态机图里的 `pending → draft` 无端点可达（不可验） | 否 |
| N2 | MINOR | `acceptance.md:120`；`test-cases/03-approval.md:64-67` | AC 步骤 6 只要求「已 approved 再 submit」，TC 还测 decide → AC 窄于 TC | 否 |
| N3 | MINOR | `acceptance.md:126`；`test-cases/03-approval.md:70-72`；`docs/contracts/api-conventions/v1.md:109` | 「409 或 422 + 明确错误码」未给期望 `code` 字符串，而契约要求稳定大写码 → 不可机械断言 | 否 |
| N4 | MINOR | `design-summary.md:81-87` | `can()` 实际在 `packages/auth/src/index.ts:87`，不在 `packages/sdk`；且关键前提是占位符「（实现时填实测输出）」 | 否 |
| N5 | MINOR | `demand.md:17`；`acceptance.md:17,160,234` | `AC-S-028` 全仓不存在（悬空引用） | 否 |
| N6 | MINOR | `decisions.md:54,67` | TD-085 未登记进 `BA/plan/tech-debt.md`（当前最大 TD-084） | 否 |
| N7 | MINOR | `test-cases/index.md:72` vs `:45-55` | 自称「15/37 是静态」，同页统计为 9+6+4=19 → 自相矛盾 | 否 |
| N8 | MINOR | `acceptance.md:32-37` | 未纳入 `cargo xtask check-modules`（模块目录规范的正规检查，`cli.rs:16-17`） | 否 |
| N9 | MINOR | `crates/xtask/src/catalog.rs:79-81`；`capabilities/index.json:29` | 能力索引文案「勿自建流程引擎」与 D347 相反；修正它要改 `crates/**`（违反 AC-S-186），不改则平台文档与需求结论矛盾 | 否 |

---

## 逐条详情

### B1 · BLOCKER · 「消费方零改动」与「消费方真能跑」互斥（装配点被硬编码在 `main.rs`）

**原文**
- `acceptance.md:45`：「「平台改动」= `crates/**` 下的任何文件」；`acceptance.md:47`：「**不允许**：为了让消费方跑起来而改 `crates/**`」。
- `design-summary.md:70-71`：「路由注册走 shell 的既有机制（不新增嵌套层级）」。

**实测**
```
crates/platform-server/src/main.rs:549   let enabled_modules = platform_registry::discover_enabled()...
main.rs:555            "sandbox" => { ... sandbox::mount(...) ... }
main.rs:581            "org" => {}
main.rs:582            other => {
main.rs:585                "模块已启用但没有 HTTP 装配点：its routes are not served on this service"
```
- `discover_enabled()` 只决定「谁注册了」，**挂不挂路由由一条硬编码 match 决定**；
  未加臂的模块 = 启动时一行 warn + 全部 404。
- 认证层也是装配层加的（`main.rs:562-572` `authenticate_module`，注释「认证层由**装配层**加（模块不依赖 IAM / RBAC）」）
  → `/api/leave/*` 要能拿到 `ctx`，同样必须加这段。
- 另有两条**编译期**前置：根 `Cargo.toml:21` 的 members 是显式列表（只有 `"modules/sandbox"`），
  根 `Cargo.toml:49` 与 `crates/platform-server/Cargo.toml:65` 各自要加一条依赖。
  不加 member，`leave` crate 根本不在工作区里。

**为什么不可接受**
- 这是「父 AC-029 只在业务目录写代码」这一条**唯一的硬骨头**，规格却把它当成不存在：
  要么改 `crates/platform-server/{Cargo.toml,src/main.rs}`（AC-S-186 直接判红），
  要么模块永远 404（AC-S-189 HTTP 用例、AC-S-192 E2E 无从谈起）。
- 讽刺的是规格自己写了「若必须改平台 → 这本身是本需求的结论」(acceptance.md:47-48)，
  但**预期结果仍写「0 个文件」**——结论项与判定项写在同一处，判定必然红。
- `design-summary.md` 的「平台改动清单」§3 里**没有**这一项，说明 BA 未实测到装配点。

**建议**
1. 把「业务模块装配」单列为**已知平台缺口**（实测证据如上），在 D351 里增第三类
   **C 类改动：为让新模块可挂载所必需的装配改动**，要求逐项登记 + 归因；
2. 或者把 AC-S-186 的预期改成「A 类 = 0，B/C 类逐项登记且与 diff 相等」
   （现在的「0 个文件」与「C 类不可避免」二选一）；
3. 根 `Cargo.toml`、`crates/platform-server/Cargo.toml`、`main.rs` 三处必须进清单。

---

### B2 · BLOCKER · AC-S-190 与 AC-S-186 共用「实现前」BASE → 「零改动」必然失败

**原文**
- `acceptance.md:31`（AC-S-186 前置）：「基线提交为 `develop` 上的某个 commit（下文记作 `BASE`）」；
- `acceptance.md:141-147`（AC-S-190）：「`git diff --name-only BASE -- modules/leave/` … **空输出**」；
- `test-cases/04-upgrade.md:66`：「⚠️ `BASE` 与 TC-020-001 用**同一个** commit（实现开始前的 develop）」；
- `test-cases/01-boundary-and-size.md:17`：「`BASE` 必须是**实现开始前的** develop commit」。

**实测**
`modules/leave` 现在不存在（`ls modules/` 只有 `README.md`、`sandbox`）。
以「实现前」为 BASE，实现后 `git diff --name-only BASE -- modules/leave/` 的输出是
**该模块的全部新增文件**（`A modules/leave/...`），不可能为空。

**为什么不可接受**
这条 AC 与实现质量完全无关地恒红，属于「判据自毁」。反过来，若有人为了让这条通过，
把 BASE 挪到「实现完成后」，那 AC-S-186 的「实现过程零改动」又失去意义 —— **一个 BASE 同时承担两件事，逻辑上不可分**。

**建议**
拆成两个基线并写进 AC：
- `BASE_IMPL`（实现开始前）：只用于 AC-S-186（`crates/`、`modules/` 非 leave 部分）；
- `BASE_UPGRADE`（leave 完成、升级开始前的那个 commit）：只用于 AC-S-190 步骤 1。
同时要求在交付摘要里写死两个 sha，并**要求提交顺序**（leave 先提交，升级后提交），否则 `BASE_UPGRADE` 不存在。

---

### B3 · BLOCKER · 升级演示的事实前提写错：v1 是对象，不是裸数组

**原文**
- `design-summary.md:95`：`| **v1**（现状） | GET /api/rbac/me/permissions | ["rbac:role:read", "leave:request:write"]（裸数组） |`
- `design-summary.md:96`：v2 = `{"permissions":[...], "roles":[...], "revision":3}`（对象）
- `test-cases/04-upgrade.md:14`：「响应体仍是 **v1 形态**（裸数组）」；判据 `is_array()`

**实测**
```rust
// crates/platform-rbac/src/http.rs:420-431
/// 当前用户的有效权限点的响应体。
///
/// # 为什么是**对象**而不是裸数组
///
/// 后续要加 `dataScope` ... 或 `registryVersion` 时
/// 不必改响应形状 —— 改形状是破坏性变更。
pub struct MyPermissionsResponse { pub permissions: Vec<String> }
```
```rust
// crates/platform-rbac/tests/me_permissions.rs:326-348（已 ratified 的 REQ-011 用例）
/// TC-011-001：200，响应体是**对象**（不是裸数组），恰好一个键 `permissions`。
assert!(body.is_object(), "响应体必须是对象（不是裸数组）：{body}");
assert_eq!(keys, vec!["permissions"], "响应体必须恰好一个键：{body}");
```
前端也是按对象解析的：`packages/auth/src/index.ts:235-238` `PermissionsResponse { permissions?: string[] }`，`:327` 直接取 `response.permissions`。

**为什么不可接受**
1. 照 TC-020-026 实施 → 现有 `tc_011_001` 会红（或 v1 被改成数组 → 真·破坏性变更，与「升级不破坏业务」相反）。
2. 更根本：`http.rs:424-425` 明确写「改形状是破坏性变更」，而 REQ-020 的整个 v2 设计就是**改形状**。
   规格没有回答：**v2 加 `roles`/`revision` 是「纯新增」还是「破坏性」？**
   若是纯新增，那「语义真变」(acceptance.md:139) 不成立 → 验不出东西；
   若是破坏性，那必须在契约层记 breaking，而 `docs/contracts/api-conventions/v2.md:5` 自己写的是 `breaking: **no**`。
3. 结果：这条 AC 的「语义真变 vs 向后兼容」自我矛盾，无论怎么实现都会被另一条判据打脸。

**建议**
重选一个 v2 语义（例如列表接口的分页信封从 `nextCursor` 改成 `cursor+hasMore`，或状态码语义变化），
并在 AC 里写清「v1 保留什么行为、v2 改了什么行为、消费方在哪一行代码上**本可以**被打断但没被打断」。
单纯加字段的 v2 没有区分力。

---

### B4 · BLOCKER · 「通知必须落 `platform_notify_delivery`」在当前平台不可达

**原文**
- `acceptance.md:125`：「步骤 5：**每次状态变更恰好一条通知**投递给申请人」；
- `test-cases/03-approval.md:106-107`：「⚠️ 必须查**真实的 `platform_notify_delivery` 表**，不能用内存假 sink」；
- `acceptance.md:213-215`（AC-S-193）：「**不得**出现 `platform-server` / `platform-iam` / `platform-notify` 等内部 crate …
  若业务模块需要通知能力，应通过 `platform_core::extension::NotificationSender` trait 注入」。

**实测**
- `crates/platform-core/src/extension/notification.rs:81` 只有 `NotificationSender`（send/channel），**没有任何入队/delivery 相关方法**。
- 全仓 `impl NotificationSender` 的生产实现共 4 个，全是「通道发送器」：
  `channels/smtp.rs:111`、`channels/webhook.rs:154`、`channels/in_app.rs:67`、`channels/unconfigured.rs:41`。
  其中 InApp 的实现把消息存进**内存 `Vec`**（`in_app.rs:55-62` `delivered()`），**不写表**。
- `platform_notify_delivery` 的唯一写入入口是 `platform-notify/src/service.rs:191 pub async fn enqueue(...)`
  （`store.rs:114 NewDelivery`）；全仓 `.enqueue(` 的调用**只出现在 `platform-notify/tests/`**。
- 且该 trait 是 RPITIT、不可对象化：`platform-org/src/notify.rs:5-9`「`Box<dyn NotificationSender>` 编译不过」。
- `platform-org` 的「真实」notifier 是 `LoggingNotifier`（只打日志），也不是落库适配器。

**为什么不可接受**
三条路全堵死：
| 做法 | 后果 |
|---|---|
| 依赖 `platform-notify` 调 `enqueue` | AC-S-193 红（依赖内部 crate） |
| 在平台里新增「enqueue 适配器」 | 改 `crates/**` → AC-S-186 红，且清单里没登记 |
| 测试里用 InApp/内存 sender 断言「1 条」 | 正是 §9.15 的「假 sink」→ TC-020-024 红 |

规格把「通知复用」写成既成事实（demand.md:71「通知 | `platform_notify` 的 `NewDelivery` + `tick()`」），
但**业务侧拿不到 `NewDelivery`**（它在 forbidden crate 里）。

**建议**
二选一并写进 D 记录：
1. 平台新增 `NotificationSender` 的落库适配器（明确列为 C 类平台改动，登记 + 归因）；
2. 降低 AC：只断言「业务调用了一次 `NotificationSender`（可用 RecordingSender 断言调用），
   **并另有一条平台侧用例**证明「sender → 投递表」链路存在」——把「假 sink」风险留在平台侧而不是业务侧。

---

### B5 · BLOCKER · 500 行预算结构上不可达（骨架已占 450 行）

**实测（脚手架模板逐文件行数，来自 `crates/xtask/src/scaffold.rs`）**

| 生成文件 | 行数 | 出处 |
|---|---|---|
| `backend/src/mod.rs` | 56 | `scaffold.rs:356` |
| `backend/src/entity.rs` | 16 | `:414` |
| `backend/src/permission.rs` | 20 | `:432` |
| `backend/src/http.rs` | 55 | `:454` |
| `backend/src/repository.rs`（**内存**存储） | 193 | `:511` |
| `backend/src/service.rs` | 65 | `:706` |
| **backend/src 小计** | **405** | |
| `frontend/routes.ts` | 26 | `:773` |
| `frontend/pages/*Page.svelte` | 19 | `:801` |
| **frontend 小计** | **45** | |
| **①+② 合计（尚未写一行业务逻辑）** | **450** | 预算 500 |

**还缺什么（都在 50 行里）**
- 仓储必须从**内存**换成真 DB：`scaffold.rs:511` 模板首行写着「内存存储，**不引入 sqlx**」；
  同类真库实现见 `modules/sandbox/backend/src/tenant_repository.rs`（**207 行**）→ 净增约 +14。
- 三态状态机 + `submit`/`decide` 两个端点：`modules/sandbox/backend/src/demo_item_http.rs` 是 **269 行 / 4 个 CRUD 端点**（约 67 行/handler）；
  `demo_item.rs` 239 行。**一个实体的 entity+HTTP 就已经 508 行**（不含状态机、不含审计通知）。
- 审计埋点（每次流转 1 条 `AuditEvent`，含 object_id/before/after）、通知埋点、审批按钮 UI —— 全都没算。

**结论**：即使业务逻辑写得极简，**backend 单体 ≥ 550 行**是保守估计，加上能点的审批 UI 约 650-700。
`demand.md:131`「业务代码 < 500 行」在「用 `cargo xtask create-module` 起手 + 真库」的路径上不可达。

**为什么这是规格问题而不是实现问题**
AC-S-187 把「< 500」定成**完成定义**（demand.md:131），却没有任何一处说明
「脚手架生成代码算不算」。`find ... -name '*.rs'` 机械地把它算进去（含 `mod.rs`），
于是这条 AC 实际度量的是**脚手架体积**，不是「复用是否省代码」。
`status.md:46` 把它列为「风险 1」并要求「如实判不成立」—— 但那样的话需求**永远无法完成**，
规格应该在写之前就把预算改成可达成值，或改成「净业务增量」口径。

**建议**
- 口径改为「**相对脚手架骨架的净增量**」：`wc -l` 实得值 − 450，阈值另定（例如净增 < 150）；
- 或把阈值改成「≤ 700 且逐项归因」，并把「骨架 450 行」写进口径说明；
- 无论哪种，都必须**先实测**再定阈值（本报告的 405/45/207/269/508 可直接引用）。

---

### B6 · BLOCKER · 「平台改动 = `crates/**`」漏掉真实改动面，且规格自带的检查会强制产生口径外改动

**原文**
- `acceptance.md:45`：「「平台改动」= `crates/**` 下的任何文件」；
- `acceptance.md:36-37`：步骤 4/5 `check-deps` / `capabilities --check` 期望退出码 0；
- `design-summary.md:135-136`：「输出必须**逐项**出现在本表里；多出任何一项 → AC-S-186 **不成立**」。

**实测**
1. `crates/xtask/src/capabilities.rs:226-245` 的 `permissions_json()` **会扫描 `modules/*/manifest.json`**；
   `capabilities/permissions.json` 里现有 8 处 `sandbox` 条目即证据。
   → 一旦新增 `modules/leave/manifest.json`（带 `leave:request:*` 权限点），生成结果改变，
   **`cargo xtask capabilities --check` 必然失败**（`main.rs:272` `E_CAPABILITIES_OUTDATED`）；
   → 必须跑 `cargo xtask capabilities` 重新生成，写出 **`capabilities/permissions.json`**（仓库根目录）。
   这个文件既不在 `crates/**`、也不在 `modules/**` → **AC-S-186 的两个步骤都看不见它**。
2. 根 `Cargo.toml:21`（members）与 `:49`（workspace 依赖）同样在口径外。
3. `apps/shell/src/**`（M7 说明必须改）与 `packages/auth|sdk/**`（design-summary §2.4 明确允许改）
   也都在口径外 —— 也就是说**任意数量的前端逻辑都可以搬进 `packages/**` 而不触发任何判据**。
4. 反向：口径内唯一「必须改」的 `crates/platform-server/**`（B1）又必然让 AC 判红。

**为什么不可接受**
「零改动」这条判据的可测量集合 ≠ 实际需要改动的集合：
- 漏测的部分（`capabilities/**`、根 `Cargo.toml`、`apps/`、`packages/`）给了**无上限的逃生通道**；
- 被测的部分（`crates/**`）又包含**不可避免**的改动。
结果是这条 AC 无论怎么实施都既不真也不假 —— 这是最坏的一种判据。

**建议**
把「平台改动」定义成**显式白名单式的文件集合**（更严谨）或**全仓 diff 减 `modules/leave/`**（更简单）：
```bash
git diff --name-only BASE_IMPL | grep -v '^modules/leave/'
```
落在这个集合里的每个文件都必须出现在「平台改动清单」并归因；
这样 `capabilities/permissions.json`、根 `Cargo.toml`、`apps/shell`、`packages/auth` 全部自动纳入。

---

### M1 · MAJOR · 把 `check-deps` 退出码解释成「新模块依赖方向合规」

- `acceptance.md:43`：步骤 4/5 退出码 0 ⇒「新模块的依赖方向与能力索引都合规」。
- 实测 `crates/xtask/src/deps.rs:217-224`：只做 `for platform in graph.platform_crates() { reverse_dependencies(platform) }`，
  `main.rs:250` 打印的是「平台 crate 未反向依赖业务模块」。
  **它不校验业务模块是否依赖了平台内部 crate**（那是 AC-S-193 的 grep 在管）。
- 影响：容易让复核者以为「check-deps 绿了 ⇒ 依赖面干净」。建议在 AC 里把这句改成「平台未反向依赖业务（仅此一项）」。

### M2 · MAJOR · AC-S-188④ 的 grep 模式与 AC-S-193 冲突、且对正确实现假红

- `acceptance.md:96`：`grep -rn "NewDelivery\|notify" … ` 期望 **> 0**。
- 实测：`NewDelivery` 定义在 `platform-notify/src/store.rs:114` —— **正是 AC-S-193 禁止依赖的 crate**；
  而 `notify` 是**大小写敏感**子串，**不匹配** `NotificationSender`：
  ```console
  $ printf 'use platform_core::extension::notification::NotificationSender;\n' | grep -c "notify"
  0
  ```
- 同时 TC-020-014（`02-reuse.md:65`）用的是 `NotificationSender` —— **AC 与 TC 判据不同**。
- 影响：按 AC 实现（走 trait）→ ④ 假红；按 ④ 的字面（用 `NewDelivery`）→ AC-S-193 红。
  建议改成 `grep -rn "NotificationSender\|NotificationMessage"`。

### M3 · MAJOR · `INSERT INTO.*audit` 无区分力且无阳性对照

- `acceptance.md:93`、`02-reuse.md:51`：期望 0。
- 实测平台自己的审计写入**不匹配该模式**：`crates/platform-audit/src/sink.rs:114-115`
  走的是 `TenantQuery::new("SELECT platform_audit_append({tenant}, …)")` —— 审计表由 SQL 函数写入。
- 全仓 `grep -rni "INSERT INTO.*audit"` **只有 1 处命中**，而且在测试夹具里（`platform-server/tests/oauth_real_stack.rs:764`）。
- 绕过方式（真实可行）：`TenantQuery::new("INSERT INTO platform_audit_log\n  …")` ——
  不触发 `sqlx::query`（① 绿）、多行不匹配点号（③ 绿）。**哈希链就这么被旁路了而判据全绿**。
- 建议：改判「业务不得出现审计表名 / 审计函数名」：`grep -rn "platform_audit_log\|platform_audit_append"`，
  阳性对照取 `crates/platform-audit/src/sink.rs`（必 > 0）。这个模式才是真正有区分力的。

### M4 · MAJOR · AC-S-188 指定的阳性对照本身失效

- `acceptance.md:102-104` 要求「对 `modules/sandbox/backend/src/` 跑同样的 grep，若也是 0 则结论不可信」。
- 实测：`grep -rn "sqlx::query\|sqlx::query_as" modules/sandbox/backend/src/` = **0**
  （sandbox 全部走 `TenantQuery`，40 处）。
  → 按规格自己的规则，「sqlx::query == 0」这条**永远处于「结论不可信」状态**。
- 有效对照存在：`crates/platform-db/src/` 命中 10 处。
- 建议：把每条 ==0 断言的阳性对照**逐条指定**（不要笼统说「对 sandbox 跑」）：
  | 断言 | 阳性对照 | 实测 |
  |---|---|---|
  | `sqlx::query == 0` | `crates/platform-db/src/` | 10 ✅ |
  | `require_permission > 0` | `crates/platform-org/src/http.rs` | 10 ✅ |
  | `AuditEvent > 0` | `crates/platform-audit/src/` | 13 ✅ |
  | `jwt|argon2|sha256 == 0` | `crates/platform-iam/src/` | 159 ✅ |

### M5 · MAJOR · 审计/通知的「恰好几条」口径不一致

- `acceptance.md:123`：「**每次流转**恰好一条审计记录」；`acceptance.md:125`：「**每次状态变更**恰好一条通知」。
- `test-cases/03-approval.md:86`：「建单 → 提交 → 审批通过（**3 次状态变更**）」；`:88` 期望「**恰好 3 条**」。
  → 建单（`draft`）不是「流转」，但被算成一条审计。
- `test-cases/03-approval.md:103`：通知只断言「审批通过后，申请人 A 收到恰好 1 条」，
  而 AC 要求**每次**状态变更都发（submit 时给申请人自己发一条？）。
- 影响：同一实现，按 AC 是「2 条审计 + 3 条通知」，按 TC 是「3 条审计 + 1 条通知」，**两边都能判红**。
- 建议：在 AC 里给出**计数表**（事件 × 审计数 × 通知数 × 收件人），并明确「创建算不算流转」。

### M6 · MAJOR · 「以 `docs/contracts/api-conventions/v2.md` 为准」没有依据

- `design-summary.md:98-99`：「用 `Accept` 版本协商还是独立路径前缀 …… `docs/contracts/api-conventions/v2.md` 已有约定 → **以它为准**」；
- `test-cases/04-upgrade.md:37`：「按 `docs/contracts/api-conventions/v2.md` 约定的方式请求 v2」。

**实测**
```console
$ grep -rn "Accept\|application/json; version\|/v2/\|版本协商" docs/contracts/   → 无输出
$ grep -n "^## C" docs/contracts/api-conventions/v2.md
38:## C9 配置键名与配置响应（v2 修订）
135:## C10 Feature Flag 响应（v2 修订）
```
该文件的「v2」是**契约文档快照版本**（C9/C10 两处修订，`v2.md:5` 自述 `breaking: no`），
**完全不含 HTTP 端点版本协商规则**。TC-020-028 的步骤因此不可执行（照什么「约定」请求？）。

- 建议：先由 BA 在契约里**新增**版本协商条款（`Accept: application/json; version=2` vs 路径前缀，
  二选一并写死），再让 AC 引用；否则删掉「以它为准」这句，改为「本需求**定义**协商方式并写入契约」。

### M7 · MAJOR · AC-S-192 的前置条件（shell 已装配模块页面）不存在，且审批动作无处承载

- `acceptance.md:185`：「前置条件: 前端 shell 已装配 `modules/leave` 的页面」。
- 实测：
  - `apps/shell/src/lib/routes.ts:47`：「`/schema/:moduleId/:entityKey` 是**唯一**的数据页面路由：新增实体只改后端 manifest，`apps/shell/src/**` 一行不改。」
  - `grep -rn "modules/" apps/shell/src` **无命中** —— 连 `modules/sandbox/frontend` 都没有被任何应用引用（`DemoPage.svelte` 是孤儿）。
  - 通配页的动作列只有编辑/删除：`SchemaTable.svelte:65-67`（`canEdit`/`canDelete`/`hasActions`）、`:131-148`（两个按钮）。
  - manifest schema 的 `api` 只有 4 个键：`crates/platform-core/src/extension/schema.rs:138-141`（list/create/update/delete）——
    **没有任何「行内动作 / 审批」的概念**。
- 影响：
  1. 「装配」这件事**没有既有机制**，design-summary §1.5「路由注册走 shell 的既有机制」是错的；
  2. 「点审批通过」必须写自定义页面 **且** 在 `apps/shell/src/**` 加一条硬编码路由（`App.svelte` 是逐路径渲染的），
     而 `apps/shell/**` 不在 AC-S-186 的任何测量范围（B6）；
  3. 若退化成「用通配表单把 `status` 字段直接改成 approved」，则绕过状态机（`decide` 端点 / 409 校验），
     AC-S-189 的安全判据就被 UI 旁路了 —— 规格没有禁止这种实现。
- 建议：把「模块页面装配」列为**已知平台缺口**（要么允许改 shell 并登记，要么要求本需求扩展 manifest 的 actions 能力——那也是平台改动）。

### M8 · MAJOR · 平台改动清单第 3 项与「契约是快照」的规则冲突

- `design-summary.md:132`：`| 3 | docs/contracts/api-conventions/ | 若契约文档需同步版本说明 | 契约是唯一权威，必须一致 |`
- 实测 `docs/contracts/README.md:3`「这里的文件是快照，不是正本」、`:34`「**不要手改本目录下的文件** —— 下次同步会被覆盖，而且 CI 会因快照与正本不一致而失败」，正本在 `BA/contracts/`（demand 分支）。
- 影响：按清单去改 `docs/contracts/` 会**破坏 CI**；真正的改动发生在 BA 分支（另一个 worktree），
  **既不在 `git diff BASE -- crates/` 里，也不在任何 AC 的检查范围**。
- 建议：清单里改成「契约正本（BA `demand` 分支）+ `scripts/sync-contracts.sh --sync`」，并明确这条不计入「平台改动」。

### M9 · MAJOR · AC-S-191 违反自己的判据（有一个数字没有取数命令）

- `acceptance.md:178`：「报告里若出现一个**没有取数命令**的数字 → 该条判不成立」。
- 但 `acceptance.md:175` 自己的表格里，`测试通过数` 一行写的是「全量输出的 `test result: ok. N passed`」——
  这是**输出模式**，不是可执行命令；`design-summary.md:157` 同样写「逐 crate 输出的 …」，也没有命令。
- 且「全量」= 多少条命令、要不要 `--include-ignored`（`platform-rbac/tests/me_permissions.rs:326-329` 的用例是 `#[ignore]`），
  规格没说 → 同一 commit 可以跑出多个 N（例如 1496 vs 1496+ignored）。
- 同理 E2E `skipped`：`playwright.config.ts:22` 只有 `reporter: [['list']]`，没有机器可读报告，
  `test-cases/05-metrics-e2e.md:72`「读 E2E 报告的 skipped 数」**没有取数方式**。
- 建议：给出确切命令，例如
  `cargo test --workspace --all-features 2>&1 | grep -oP 'test result: ok\. \K\d+(?= passed)' | paste -sd+ | bc`
  与 `npx playwright test --reporter=json | jq '[.. | .skipped? // empty] | ...'`，并把 `--include-ignored` 写死。

### M10 · MAJOR · 「`skipped == 0`」未定义范围，同一份代码可报两个数

- `acceptance.md:193`、`test-cases/05-metrics-e2e.md:72`：要求 E2E 报告 `skipped == 0`。
- 实测现有 suite 里就有条件 skip：`e2e/audit.spec.ts:157,186,225,254,298`（`test.skip(!(await auditApiReady(request)), …)`）、
  `e2e/schema-driven.spec.ts:137`。
- 影响：跑全量 → skipped 含其他 spec 的条件跳；只跑本模块 spec → 数不同。
  「换个执行范围就能从红变绿」，不满足 AC-S-191 要求的「同一 commit 结果一致」。
- 建议：把范围写死成「`npx playwright test e2e/leave*.spec.ts --reporter=json`，该文件的 skipped == 0」，
  或明确「全量 suite 的 skipped 数必须与 BASE_IMPL 上相同」（相对判据）。

---

### MINOR / NIT（合并说明）

- **N1** `acceptance.md:109`「三态状态机」vs `:112` 列出 **4** 个取值（draft/pending/approved/rejected）；
  `design-summary.md:38` 的非法流转 `pending → draft` **没有端点可达**（只有 submit/decide），不可验。
- **N2** `acceptance.md:120` 只写「对已 approved 再 submit」，`03-approval.md:64-67` 还测「再 decide」；AC 窄于 TC，应补进 AC。
- **N3** `acceptance.md:126`「409 或 422 … 明确错误码」：未给期望 `code` 字符串；而 `docs/contracts/api-conventions/v1.md:109` 规定
  「`code` 是**稳定的机器可读错误码**（大写 + 下划线）」。建议写死 `LEAVE_INVALID_TRANSITION`（或等价）。
- **N4** `design-summary.md:82` 说 `can()` 由 `packages/sdk` 调用 —— 实际在 `packages/auth/src/index.ts:87`（`sdk` 无此函数）；
  且 `:84-87` 的取证是占位符「（实现时填实测输出）」，即**核心前提未实测**，而 `status.md:3` 已标「已就绪、阻塞项无」。
- **N5** `AC-S-028` 全仓不存在（`demand.md:17`、`acceptance.md:17,160,234` 引用它）→ 悬空引用，应改为 D 编号或删掉。
- **N6** `decisions.md:54,67` 声称「记入技术债（TD-085）」，但 `BA/plan/tech-debt.md` 当前最大是 TD-084，**TD-085 未登记**。
- **N7** `test-cases/index.md:72`「静态用例占多数（**15/37** 是 git/grep/wc）」与同页 `:49-51` 的统计（9+6+4=**19**）矛盾。
- **N8** AC-S-186 未纳入 `cargo xtask check-modules`（`crates/xtask/src/cli.rs:16-17`，模块目录规范的正规检查，对应 AC-S-001）。
- **N9** `crates/xtask/src/catalog.rs:79-81` 与 `capabilities/index.json:29` 的能力文案是
  「WorkflowHandler + TransitionEvent 提供工作流接入接缝，**勿自建流程引擎**」——与 D347（业务自建）相反。
  修正文案 = 改 `crates/**`（违反 AC-S-186）+ 重新生成 `capabilities/**`（口径外）；不修正则平台文档与本需求结论矛盾。
- **N10** `demand.md:99` 的代码块写 `Deprecation::new(2027-06-30)`，**不是合法 Rust**
  （真实实现是 `time::macros::datetime!(2027-06-30 00:00:00 UTC)`，`platform-observability/src/http.rs:391-394`）。
  引用实测证据时把不可编译的伪代码当成输出，容易被照抄。

---

## 已查确认无问题的项

| # | 项 | 实测证据 |
|---|---|---|
| ✅1 | **编号无冲突**：`AC-S-186~193`、`D347~D351`、`TC-020-001~037` 在 `BA/` 全目录下无第二处使用；TC 段连续无跳号，分布表相加 = 37 | `grep -rn "AC-S-18[6-9]\|AC-S-19[0-3]" BA/`（除 REQ-020 外 0 命中）；`test-cases/index.md:13-23` |
| ✅2 | **D347 的事实前提成立**：`WorkflowHandler::on_transition` 平台侧零调用点，只有能力索引字符串与 re-export | `grep -rn "on_transition\|WorkflowHandler"` → 仅 `xtask/src/capabilities.rs:50`、`xtask/src/catalog.rs:79-80`、`platform-core/src/lib.rs:22`、`extension/mod.rs:80` |
| ✅3 | **弃用头机制真实存在且格式正确**（不是文档自述） | `platform-observability/src/http.rs:388-396`（`default_deprecations()` 只登记 `/api/audit/events`）；`deprecation.rs:43-52`（手写 IMF-fixdate 及原因）；`:173-187`（反解测试） |
| ✅4 | **弃用中间件在生产装配路径上，且按完整路径精确匹配**（不会因 `nest` 匹配不到） | `http.rs:505-507` 挂载 → `platform-server/src/main.rs:855` 调用；`deprecation.rs:137-143`（`request.uri().path()` 精确 `==`） |
| ✅5 | **升级目标端点存在**：`GET /api/rbac/me/permissions` 已实现（本轮只需加 v2，不需从零造端点） | `platform-rbac/src/http.rs:493`（路由）、`:759-778`（handler） |
| ✅6 | **「业务不写裸 SQL 也能读写」成立**：`TenantQuery` + `platform_db::tenant_row!` 足以完成 INSERT/UPDATE，sandbox 实测 0 处 `sqlx::query` | `modules/sandbox/backend/src/tenant_repository.rs:40-45`（宏）、`:18`（TenantDb/TenantQuery）；`grep -c "sqlx::query"` = 0 |
| ✅7 | **`require_permission` 是唯一鉴权入口**，design-summary 的用法与平台一致 | `platform-core/src/extension/permission.rs:273-281`（注释「全仓库只有这一个鉴权入口」） |
| ✅8 | **审计确实是真落库 + 哈希链**（业务不得自建审计表的判据方向正确） | `platform-audit/src/sink.rs:7-16,114-129`（`platform_audit_append`，SECURITY DEFINER） |
| ✅9 | **`platform_notify_delivery` 表真实存在** | `platform-notify/tests/notify_worker_real_db.rs:39,48,58`（直接对该表增删查） |
| ✅10 | **xtask 子命令都存在**，AC 里的命令不会被「命令不存在」卡住 | `xtask/src/cli.rs:17`（check-modules）、`:19`（check-deps）、`:21`（capabilities）、`:25`（tenant-policies） |
| ✅11 | **AC-S-187 三条命令语法正确**（目录存在后 `xargs wc -l \| tail -1` 会给总计；文件数少，不会触发 xargs 分批） | 本地以 `modules/sandbox` 同样式命令验证：输出 `2223 总计` |
| ✅12 | **多数阳性对照实测可用**：`require_permission`(platform-org)=10、`AuditEvent`(platform-audit)=13、`jwt\|argon2\|sha256`(platform-iam)=159 | 见 M4 表格；两条失效的已单列 |
| ✅13 | **E2E 基建齐备且支持多账号/租户切换**（AC-S-192 的「切到审批人账号」执行层可行） | `playwright.config.ts:16-40`；`e2e/` 下 8 个 spec；`e2e/shell-auth.spec.ts:25,36-44`（`fillLogin(page, email, tenant)`） |
| ✅14 | **§9.2 / §9.15 引用有效**（不是编造的规矩编号） | `BA/plan/dev-guide-low-spec.md:529`（§9.2 静态扫描必须带阳性对照）、`:817`（§9.15）、`:872`（9.15.1）、`:900`（9.15.2） |
| ✅15 | **前置依赖状态属实**：`develop` 最新提交 = `1be47f5`，与 `demand.md:60` 一致；`modules/leave` 确实不存在（`ls modules/` 只有 `README.md`、`sandbox`） | `git log --oneline -3`、`ls modules/` |
| ✅16 | **「依赖未来产物」这件事本身有交代**：AC-S-187/188/192/193 都在前置条件里写了「已完成 / 已定稿」 | `acceptance.md:55,81,185,202` |
| ✅17 | **`design-summary.md` 的「平台改动清单」确实被 AC 引用**（引用链没断） | `acceptance.md:40-41,153-155` → `design-summary.md:124-136`；`test-cases/01:26-31`、`04:84-92` |
| ✅18 | **仓库里没有第二套同名模块/编号的遗留物**（`leave` 未被占用，`AC-S-028` 是唯一例外见 N5） | `grep -rln "modules/leave" BA/demands/` 全部 8 个文件均在 REQ-020 目录内；`ls modules/` |

---

## 未验证项

| # | 未验证内容 | 原因 | 影响 |
|---|---|---|---|
| 1 | 任何 `cargo test` / 集成测试 / E2E 实跑 | 本机低配（明确不跑）；且 `modules/leave` 不存在，无被测物 | 本报告全是**规格层**结论，不含运行时行为 |
| 2 | `platform_notify_delivery` 的 DDL（列名 / 模板是否必填） | 只读了测试里的 SQL 用法，未读迁移 | TC-020-024 的字段断言（`template_code`/`channel`）可行性未核 |
| 3 | manifest 是否有未发现的「行内动作」扩展点 | 只核了 `schema.rs:138-141` 的 4 个 api 键与 `SchemaTable` 的两个按钮 | 若存在未发现扩展点，M7 的「必须改 shell」可被推翻 |
| 4 | `scripts/sync-contracts.sh --sync` 本地可运行性 | 需要 BA `demand` worktree 与 CI 环境 | M8 的修复方案未实测 |
| 5 | 完整环境下的 E2E `skipped` 实际值 | 未起服务（需 DB 容器） | M10 的严重度可能偏低（若完整环境下确实 0） |
| 6 | 500 行估算的最终值 | 未写实现（本报告按脚手架常量 + sandbox 同类模块行数推算） | B5 的结论是**结构性推断**（450 骨架是硬证据，超出量是保守估计） |
| 7 | `BASE` 在实际 git 历史里会取哪个 sha | 实现未开始；`feature/REQ-020` worktree 当前停在 `9bcfbcb`（BA 分配提交） | B2 的修复需要先定提交顺序 |

---

## 一句话给下一位

规格的**方法论**是对的（口径写死、阳性对照、查真实表、0 skipped、不许改口径凑数）——
这批 AC 的骨架质量明显高于 REQ-022 那轮。问题全在**「底座到底给不给得到这些东西」没有摸过底**：
装配点、通知落库、前端页面挂载、capabilities 生成物，这四处都是「照文档应该能，打开代码不能」。
建议 BA 拿本报告的 B1/B4/B5/B6 四条，各跑一次「打开源码确认」再定 rev 2。

---

# 挑刺回执（BA，2026-11-23，rev 2）

> 26 条（6 BLOCKER / 10 MAJOR / 10 MINOR-NIT）**全部处置**。
> 先逐条说明「我怎么验的」，再给处置。

## 独验结论

我按报告最后的建议，对 B1/B3/B4/B5/B6 逐条「打开源码确认」：

| 挑刺 | 我亲验的命令与结果 | 成立？ |
|---|---|---|
| **B1** | `sed -n '549,590p' crates/platform-server/src/main.rs` → `match *module_id { "sandbox" => {...}, "org" => {}, other => warn }`；根 `Cargo.toml:21` members 只有 `"modules/sandbox"`；`platform-server/Cargo.toml:65` 有 `sandbox.workspace = true` | ✅ **成立** |
| **B3** | `sed -n '420,432p' crates/platform-rbac/src/http.rs` → 注释「# 为什么是**对象**而不是裸数组 / 改形状是破坏性变更」；`platform-rbac/tests/me_permissions.rs:331` → `assert!(body.is_object(), "响应体必须是对象（不是裸数组）")` | ✅ **成立** |
| **B4** | `sed -n '75,100p' crates/platform-core/src/extension/notification.rs` → `trait NotificationSender { fn channel(); fn send(); }`，**无入队方法** | ✅ **成立** |
| **B5** | `python3` 提取 `scaffold.rs` 的所有 `const ... = r#"..."#` → REPOSITORY 194 / SERVICE 66 / MOD 57 / HTTP 56 / PERMISSION 21 / ENTITY 17 = **411**；前端 27+20 = **47**；合计 **458** | ✅ **成立**（我独立算得 458，与报告的 450 差 8 行，量级一致） |
| **B6** | `grep -rn "modules/" apps/shell/src` → 无命中；`capabilities/permissions.json` 含 8 处 `sandbox`（证明它扫 manifest） | ✅ **成立** |
| **B2** | 逻辑核验：`modules/leave` 现在不存在（`ls modules/` 只有 README.md + sandbox），以「实现前」为 BASE，实现后 diff 必为全部新增 | ✅ **成立**（纯逻辑，无需实跑） |

**结论：6 条 BLOCKER 我全部认同，无一条误报。** 这份挑刺的质量很高。

---

## 逐条处置

### BLOCKER

| # | 处置 | 落在哪 |
|---|---|---|
| **B1** | 采纳建议 1+3：AC-S-186 改成**四分类登记**（D353 新增 C 类），装配点三个文件（根 `Cargo.toml` / `platform-server/Cargo.toml` / `main.rs`）进清单并归因；同时**记 TD-086**（装配是硬编码 match 是底座缺陷） | `acceptance.md` AC-S-186；`decisions.md` D353/D355；`design-summary.md` §3；`tech-debt.md` TD-086 |
| **B2** | 采纳：拆 `BASE_IMPL` / `BASE_UPGRADE`，并**写死提交顺序**（消费方先提交） | `acceptance.md` 新增「两个基线」章；`test-cases/01` TC-001、`test-cases/04` TC-031 |
| **B3** | 采纳建议：v1 **保持不动**（对象 + 恰好一个键）；v2 改为**新增 `sources` 键**（不是改形状）；并新增 D352 说明版本化机制 | `acceptance.md` AC-S-190；`decisions.md` D352；`design-summary.md` §2.2；`test-cases/04` |
| **B4** | 采纳建议 1：平台补 `DbNotificationSender`（D 类改动，登记）；业务仍只调 trait | `decisions.md` D354；`acceptance.md` AC-S-189；`design-summary.md` §1.6；`test-cases/03` TC-025 |
| **B5** | 采纳建议：**不放宽口径**，预先披露骨架 458 行，超标判不成立 + 归因 | `decisions.md` D357；`acceptance.md` AC-S-187；`test-cases/01` TC-008（新增）、TC-009 |
| **B6** | 采纳「更简单」方案：改 `git diff --name-only BASE_IMPL \| grep -v '^modules/leave/'` | `acceptance.md` AC-S-186；`test-cases/01` TC-001 |

### MAJOR

| # | 处置 | 落在哪 |
|---|---|---|
| **M1** | `check-deps` 的解释精确化为「**平台不依赖业务**」；「业务不依赖平台内部」交给 grep | `acceptance.md` AC-S-193 |
| **M2** | grep 改 `NotificationSender\|NotificationMessage` | `acceptance.md` AC-S-188 ⑦；`test-cases/02` TC-017 |
| **M3** | 改判「不得出现 `platform_audit_log` / `platform_audit_append`」，阳性对照取 `sink.rs` | `acceptance.md` AC-S-188 ⑥；`test-cases/02` TC-016 |
| **M4** | 阳性对照**逐条指定**（platform-db=10 / iam=159 / audit>0），不再笼统说「对 sandbox 跑」 | `acceptance.md` AC-S-188 表格；`test-cases/02` 附阳性对照总表 |
| **M5** | 加**计数表**（事件 × 审计 × 通知 × 收件人），并写死「建单算 1 条审计、0 条通知」 | `acceptance.md` AC-S-189；`test-cases/03` 开头计数表 |
| **M6** | 已核实：`v2.md` 是契约快照版本（C9/C10 两处修订），**不含** API 版本协商规则 → 删掉「以它为准」，新增 **D352** 定义版本化方式 | `decisions.md` D352；`design-summary.md` §2.2 |
| **M7** | 采纳：自定义页 + `apps/shell/src/**` 一条硬编码路由（C 类登记）；并**明确禁止**用通配表单改 `status`（会绕过状态机）；记 **TD-087** | `decisions.md` D356；`design-summary.md` §1.5；`test-cases/05` 补充章；`tech-debt.md` TD-087 |
| **M8** | 采纳：契约文档**不计入平台改动**（它是快照），正本走 `scripts/sync-contracts.sh --sync` | `design-summary.md` §3 末；`test-cases/04` 补充表 |
| **M9** | 给确切命令：全量测试用**逐 crate 循环 + 必带 `--include-ignored`**；E2E skipped 用 `jq` 取 | `acceptance.md` AC-S-191；`test-cases/05` TC-034/038 |
| **M10** | 写死范围：`npx playwright test e2e/leave.spec.ts`，**只看该文件**的 skipped | `acceptance.md` AC-S-192；`test-cases/05` TC-037/038 |

### MINOR / NIT

| # | 处置 |
|---|---|
| **N1** | AC 改「闭集 **4 个**取值」（原写「三态」却列 4 个）；删掉 `pending → draft` 这个**无端点可达**的非法流转，改为列出实际可达的 5 种 |
| **N2** | AC-S-189 步骤 7 补「已终态再 `decide`」；TC-023 同步 |
| **N3** | 写死 `code == "LEAVE_INVALID_TRANSITION"` |
| **N4** | `can()` 的位置改为 `packages/auth/src/index.ts:387`（实测），并标注原写 `packages/sdk` 是错的 |
| **N5** | 删掉悬空的 `AC-S-028` 引用，改为「度量可复现（现 AC-S-191）」；`grep -c "AC-S-028" demand.md` → **0** |
| **N6** | TD-085 **已登记**（`tech-debt.md`） |
| **N7** | index 统计重算：38 条，形态统计 19/13/3/2/1 **相加 == 38**，并注明「rev 1 的 15/37 与 19 自相矛盾，已修」 |
| **N8** | AC-S-186 的命令组加入 `cargo xtask check-modules` |
| **N9** | 记入 **TD-085** 的「额外矛盾」段：`catalog.rs:79-81` 与 `capabilities/index.json:29` 写「勿自建流程引擎」，与 D347 方向相反 |
| **N10** | `demand.md` 的伪代码改为真实写法（`time::macros::datetime!(...)`） |

---

## 一个我认领的方法论错误

报告最后一句说得很准：

> 问题全在**「底座到底给不给得到这些东西」没有摸过底**：
> 装配点、通知落库、前端页面挂载、capabilities 生成物，这四处都是
> 「照文档应该能，打开代码不能」。

我写 rev 1 时，把 demand.md 里「通知 | `platform_notify` 的 `NewDelivery` + `tick()`」
这样的**能力清单**当成了「业务可以直接用」。实际上：
- `NewDelivery` 在业务**禁止依赖**的 crate 里；
- `NotificationSender` 只有 `send()` 没有入队；
- 前端「只改 manifest」在有自定义动作时不成立。

**教训**：写「复用某某能力」之前，必须打开那个能力的**消费入口**看一眼签名，
而不是看它的**存在性**。这与 §9.15 的「判据存在 ≠ 判据覆盖」是同一类错误的另一种形态
—— 我这次犯的是「**能力存在 ≠ 业务能用**」。

---

## rev 2 的自评

**已处置的**：26/26 条。

**仍需注意的**：

1. **B1/B5 的处置是「登记 + 归因」，不是「消除」**。
   底座确实缺能力，rev 2 把这一点**明码写进规格**，而不是继续假装。
   若有人认为「本需求应该顺手把装配机制重构掉」，那是另一个需求的工作量
   （TD-086 已记）。
2. **D354 的 `DbNotificationSender` 是 D 类平台改动**，它会进 `git diff`。
   若实现时发现它比预想复杂（比如要改 trait 让它可以对象化 ——
   报告 B4 提到 RPITIT 不可对象化），那属于**实现期的新发现**，要记新的 D 决策。
3. **我没有重新派第二轮挑刺**。rev 2 的改动面很大（8 条 AC 全部重写、
   用例从 37 → 38 且大半改写），按本项目惯例可能值得再挑一轮。
   但考虑到：(a) 6 条 BLOCKER 的处置方向都是报告自己建议的；
   (b) 剩下的都是「照建议改」的机械修正 —— 我判断可以直接进实现，
   **在实现阶段的第一棒（Dev 自查 + Pre-merge 复验）里兜住**。
   若你认为该再挑一轮，我立刻派。
