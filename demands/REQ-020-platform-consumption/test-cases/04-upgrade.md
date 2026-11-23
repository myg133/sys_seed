# 用例：平台升级与弃用头

> 覆盖 **AC-S-190**
> 用例编号：TC-020-027 ~ TC-020-033（7 条）
> **rev 2**：v1 事实修正（B3）、路径前缀（D352）、`BASE_UPGRADE`（B2）、契约不算平台改动（M8）

## 版本化方式（D352）

| 版本 | 路径 | 响应体 |
|---|---|---|
| **v1**（现状，保持不动） | `GET /api/rbac/me/permissions` | `{"permissions": [...]}`（**对象，恰好一个键**） |
| **v2**（新增） | `GET /api/rbac/v2/me/permissions` | `{"permissions": [...], "sources": {"<perm>": ["<role>"]}}` |

⚠️ **rev 2 修正 B3**：rev 1 写「v1 是裸数组 `["rbac:role:read"]`」——**事实错误**。
实测 `crates/platform-rbac/src/http.rs:420-431`：
> `/// # 为什么是**对象**而不是裸数组 … 改形状是破坏性变更。`

且 `crates/platform-rbac/tests/me_permissions.rs:331` 有**已 ratified** 的断言：
`assert!(body.is_object(), "响应体必须是对象（不是裸数组）")`。

→ v1 不能改成裸数组（会打红 REQ-011 的用例）。v2 改为**新增 `sources` 键**。

---

## TC-020-027 · AC-S-190 步骤 3 · v1 旧路径在升级后仍可用

- **层次**: 集成（HTTP）
- **步骤**: 升级完成后，`GET /api/rbac/me/permissions`（不带任何版本头）。
- **期望**: **200**，响应体是**对象**且**恰好一个键** `permissions`。
- **判据形态**: `assert!(body.is_object())` + `assert_eq!(keys, vec!["permissions"])`。
- ⚠️ **必须同时跑 REQ-011 的既有用例** `tc_011_001_the_response_is_an_object_with_exactly_one_key`
  （`platform-rbac/tests/me_permissions.rs:326`）—— 它若变红，说明 v1 被改坏了。

---

## TC-020-028 · AC-S-190 步骤 3 · v1 响应带 `Deprecation` 与 `Sunset`

- **层次**: 集成（HTTP）
- **步骤**: 同 TC-020-027，读响应头。
- **期望**:
  - `Deprecation: true`（逐字）；
  - `Sunset` 存在且能被**严格 IMF-fixdate** 反解（`Sun, 06 Nov 1994 08:49:37 GMT`）。
- **判据形态**: 头存在 + `time` 反解成功。
- ⚠️ **不能只断言头存在**：格式错客户端解析不了。
  本仓 `deprecation.rs:43-52` 是**手写** IMF-fixdate（因为 `time 0.3.55` 没有 `Rfc7231`，
  `Rfc2822` 用 `+0000` 不是 `GMT`）。

---

## TC-020-029 · AC-S-190 步骤 4 · v2 新路径走新语义

- **层次**: 集成（HTTP）
- **步骤**: `GET /api/rbac/v2/me/permissions`。
- **期望**:
  - **200**；
  - 响应体含 `permissions`（与 v1 相同的扁平数组）**和** `sources`（按来源分组的对象）；
  - `sources` 的每个值都是数组。
- **判据形态**: 两个键存在 + `sources` 结构断言。
- ⚠️ **为什么这个变化有验证价值**：
  对「按开放对象消费」的客户端零改动（忽略未知键）；
  对「严格穷举键」的客户端会被打断 —— 这正是「破坏性变更需提前通知」的场景。

---

## TC-020-030 · AC-S-190 · **对照：未弃用端点不带头**

- **层次**: 集成（HTTP，阴性对照）
- **步骤**: 打一个**未登记弃用**的端点（如 `GET /api/leave/requests`）。
- **期望**: **没有** `Deprecation`，**没有** `Sunset`。
- **判据形态**: 两个头都不存在。
- ⚠️ **阴性对照必须有**：否则「所有响应都带头」也能让 TC-020-028 绿 ——
  那说明弃用是按「全局」加的，不是按路径。

---

## TC-020-031 · AC-S-190 步骤 1 · 消费方业务目录零改动

- **层次**: 静态（git）
- **步骤**:
  ```bash
  git diff --name-only BASE_UPGRADE -- modules/leave/
  ```
- **期望**: **空输出**。
- **判据形态**: `wc -l == 0`。
- ⚠️ rev 2 修正 B2：**必须用 `BASE_UPGRADE`**（`modules/leave` 已提交、升级尚未开始），
  **不是** `BASE_IMPL`。用 `BASE_IMPL` 的话 `modules/leave/` 全是新增文件 → 恒红。
- ⚠️ 这要求**提交顺序**：消费方先提交 → 形成 `BASE_UPGRADE` → 升级改动后提交。

---

## TC-020-032 · AC-S-190 步骤 2 · 升级后消费方全部测试与 E2E 通过

- **层次**: 集成 + E2E
- **步骤**:
  1. 跑 `modules/leave` 的全部后端测试；
  2. 跑 `e2e/leave.spec.ts`。
- **期望**: 全部通过，**`e2e/leave.spec.ts` 的 `skipped == 0`**。
- **判据形态**: `test result: ok. N passed; 0 failed; 0 ignored` + E2E JSON 的 skipped 数。
- ⚠️ REQ-014 的教训：`skip ≠ 已验证`（D193）。

---

## TC-020-033 · AC-S-190 · 平台改动清单与全仓 diff 逐项对应

- **层次**: 静态（文档比对）
- **步骤**: 与 TC-020-001 相同的方法，但用 `BASE_IMPL` 且**只看 `crates/**`**。
- **期望**: `crates/` 下的改动**只包含** design-summary §3 的 B 类（2 项）与 D 类（1 项），
  **不含任何 A 类**。
- **判据形态**: 集合包含关系 + A 类计数为 0。
- ⚠️ 与 TC-020-001 的区别：001 是**全仓**闸门，033 专看「升级涉及的 `crates/` 改动
  是否只有 B/D 类」。

---

## 补充：升级演示的变更点（实现时填实）

| # | 类 | 变更 | 文件 |
|---|---|---|---|
| 1 | **B** | v2 端点 + 路由 | `crates/platform-rbac/src/http.rs` |
| 2 | **B** | v1 弃用登记 | `crates/platform-observability/src/http.rs` |
| 3 | **C** | 平台前端兼容 v2 的 `sources`（若不改也能跑 → 删掉本行） | `packages/auth` |
| 4 | — | 契约文档（**不算平台改动**，见下） | BA `demand` 分支正本 |

⚠️ **契约文档不能手改**（M8 修正）：`docs/contracts/README.md:3,34` 明写
「这里的文件是快照，不是正本」「**不要手改本目录下的文件** —— 下次同步会被覆盖，
而且 CI 会因快照与正本不一致而失败」。
→ 正本在 BA `demand` 分支的 `BA/contracts/`，要改走 `scripts/sync-contracts.sh --sync`，
**不计入**平台改动。
