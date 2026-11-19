# BA 裁定：REQ-012 实现期偏差（Dev 提出 6 项 + BA 补 1 项）

- **裁定者**: myg133-ba-1 (BA)
- **时间**: 2026-11-19 16:50
- **输入**: `feature-meta/dev-report-backend.md` §5（Dev 提出的偏差）+ BA 自查
- **结论**: **6 项全部接受**，另加 **1 项 BA 补定**（AC-S-085 需第二实体）

---

## 1. Dev 提出的 6 项（全部接受）

### 1.1 元数据响应带 `api` 子段

**Dev 说**：`TC-012-014/023` 的「响应 4 键」过时；前端已提交的 `EntityMeta` 把 `api` 定为**必填**，
不返回则 AC-S-085/086 断链。

**裁定**：**接受**。

**理由**：这是 **D102 的直接推论** —— 我定了「schema 实体声明 `api` 子段」，
那元数据接口**当然要返回它**，否则前端拿到元数据也不知道去哪取数据（正是 QA #5 挑的洞）。

`TC-012-014/023` 的「4 键」是 QA 写用例时的**预设**，不是规格。规格（rev 2）已经要求 `api`。

**落点**：用例集要更新（BA 负责），规格无需改。

### 1.2 错误体键是 `trace_id` 不是 `traceId`

**Dev 说**：`TC-012-019` 断言 `traceId`，但平台实际是 `trace_id`；
且这与 `api-conventions v1` C6 冲突。

**裁定**：**接受，按契约走（`trace_id`）**。

**理由**：**契约优先于用例**。契约 C6 是全平台错误体的唯一约定，
不能因为一个用例写错就改契约（那会波及所有模块）。

**落点**：用例集更新。

> ⚠️ 顺带记一笔：`trace_id` 是 snake_case，而契约 C13 说「JSON 字段 camelCase」。
> 这是**契约内部的不一致**，但属既有问题，**不在本期扩围**。登记到技术债。

### 1.3 `TC-012-019` 的「`"title"` 不出现」不可执行

**Dev 说**：RFC 9457 错误体**自带** `title`。

**裁定**：**接受**。用例错。

**理由**：这与 QA 自己挑出的 **#4（「全文不出现字段名」必然误报）** 是**同一类错** ——
「作为键不出现」≠「作为子串不出现」，而 `title` 本身是 RFC 9457 的标准键，
断言它不出现**必然失败**。

**讽刺的是**：QA 挑了 BA 的这个错，自己又在新用例里犯了同一个错。
**这印证了「挑刺也可能有假」** —— 接受前要核证据（我在 rev 2 裁定里已就 #13 这么做过）。

### 1.4 `ownerId.reference` 指向 `sandbox/demoItem` 而非 `org/user`

**Dev 说**：`platform-org` 没有 `schema` 段且不在它的文件范围；
指向 `org/user` 会让**进程起不来**（阶段 2 校验：reference 目标必须存在）。

**裁定**：**接受**。

**理由**：这是**阶段 2 校验按设计工作的证据** ——
Dev 试图写一个指向不存在实体的 reference，被校验拦住了。
给 `platform-org` 加 `schema` 段是**另一个需求**（不在本期范围）。

**落点**：`demoItem.ownerId` 指向 `sandbox/demoItem`（自引用，合法且能验证 reference 机制）。

### 1.5 C-5 / C-2 排除 `type: "field"` 权限点

**Dev 说**：`field` 类型的权限点**不该**要求 `require_permission` 调用点，它们由 C-6 守。

**裁定**：**接受**。

**理由**：`PermissionType::Field` 的语义是「**这个字段看不看得到**」，
不是「这个操作能不能做」。用 `require_permission` 守字段权限是**用错工具**：

| 类型 | 作用面 | 守它的契约 |
|---|---|---|
| `menu` / `button` / `data` | 接口/操作 | **C-5**（必须有鉴权调用点） |
| `field` | 响应字段 | **C-6**（必须走序列化器） |

不排除的话，`sandbox:demo:secret` 会被 C-5 判为「已登记但无调用点」→ **假红**，
然后有人会去加一个假的 `require_permission` 来让它变绿 —— **那才是真问题**。

### 1.6 `deps.rs` 给装配层 `platform-server` 开依赖业务模块的例外

**Dev 说**：装配层必须依赖业务模块才能挂载。

**裁定**：**接受**，但**必须写进 `AGENTS.md`**。

**理由**：ADR A7 说「平台 → trait ← 业务」，`platform-server` 是**装配层**，
它的职责就是「把平台与业务装到一起」—— 它**必须**依赖两边。

**但必须写清楚边界**，否则下一个人会以为「平台 crate 可以随便依赖业务模块」：

| 谁 | 能否依赖业务模块 | 为什么 |
|---|---|---|
| `platform-core` / `platform-db` / `platform-iam` / `platform-org` / `platform-rbac` / `platform-schema` | ❌ **不能** | 它们是**平台能力**，依赖业务就成环 |
| `platform-server` | ✅ **可以**（唯一例外） | 它是**装配层**，职责就是装 |

`deps.rs` 的例外必须是**白名单式的**（只放行 `platform-server`），不是「关掉检查」。

---

## 2. BA 补定：AC-S-085 需要第二个实体（D107）

### 问题

manifest 现在只有 **`demoItem` 一个实体**。而 **AC-S-085**（「新增同类页面无需新增前端代码」）
是本需求的**立身之本** —— 没有第二个实体，**这条 AC 无法验**：

- 只对比「加实体前后源码哈希」需要真的加一个实体
- 若 E2E 用环境变量 `SCHEMA_SECOND_ENTITY` 注入，那是**测试专属路径**，不是真实能力

### 裁定：**加第二个实体 `demoNote`**

**要求**：
- 与 `demoItem` **字段组合不同**（如 `title` / `content` / `pinned`（boolean）/ `tags`（text））
- **完整 CRUD**（与 demoItem 同款：list/create/update/delete）
- manifest 的 `schema` 段声明它 + `api` 子段
- 迁移建表（tenant_id + RLS）
- **不加**字段级权限点（保持简单；字段权限已由 demoItem 的 `secret` 验证）
- 前端**一行不改**

**验收方式**（比哈希更强）：
1. `GET /api/schema/sandbox/demoNote` → 200
2. `/schema/sandbox/demoNote` 页面**可用**（渲染 + CRUD）
3. **`grep` 全部前端源码**，断言**不含** `demoNote` 这个实体键
   （证明没有任何一个前端文件为它写过特化代码）
4. `packages/schema/src/**` 与 `apps/shell/src/**` 的**哈希未变**

**新增决策 D107**。

---

## 3. 顺带修正：`trace_id` 的命名不一致（登记技术债）

**发现**：契约 C13 说「JSON 字段 camelCase」，但错误体的 `trace_id` 是 snake_case。

**裁定**：**不在本期改**（会波及所有模块的错误体），**登记技术债**。

---

## 4. 下一棒

1. **后端补 `demoNote`**（D107）—— 小改动，复用 demoItem 的模式
2. **BA 更新用例集**（6 项偏差对应的用例）
3. **QA Pre-merge 验证**（真服务 + 真浏览器 + E2E，含 AC-S-085）
4. 修复轮 → 独立复核 → 合并
