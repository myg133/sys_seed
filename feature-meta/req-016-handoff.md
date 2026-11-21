# REQ-016 断点（2026-11-21 10:15 暂停）

> **用户要出门，暂停。回来从这里继续。**
> 本文件是**唯一入口** —— 不看对话历史也能接上。

---

## 1. 一句话状态

**REQ-016 规格已定稿，QA 挑出 39 条（阻塞 9），BA 已核实其中 4 条全部成立。
下一步是「裁决轮」—— 把 39 条逐条裁定，然后改规格。**

**还没派 Dev。** 代码零改动（`feature-REQ-016` 只有 BA 的派单提交）。

---

## 2. 拓扑与分支

```
/home/myg133/Projects/system_seed                  18eeb99 [workspace]
/home/myg133/Projects/system_seed/BA               b24b970 [demand]      ← QA 产出已提交
/home/myg133/Projects/system_seed/Deploy           367ec11 [deploy]
/home/myg133/Projects/system_seed/code             aa5b305 [develop]     ← REQ-022 已合并
/home/myg133/Projects/system_seed/feature-REQ-016  5aba742 [feature/REQ-016]  ← 干净，等待开工
```

- REQ-016 **已认领**：`agent_id=myg133-ba-1`，`holding=REQ-016`
- develop 上 REQ-022 已合并（`aa5b305`），全量 1094 passed

---

## 3. 已完成到哪一步

| 步骤 | 状态 | 落点 |
|---|---|---|
| 认领 REQ-016 | ✅ | worktree `feature-REQ-016` @ `5aba742` |
| 规格四件套补齐（1 条 AC → **11 条**） | ✅ | `demand.md` rev 3 / `acceptance.md` rev 3 / `decisions.md` **D262~D275** / `design-summary.md` 新建 |
| BA 自查修正（删 2 个死码、AC-S-156 补 5 条路径、修 3 处数字） | ✅ | commit `0c88234` |
| **QA 挑刺 39 条 + 61 条用例** | ✅ | commit `b24b970` |
| **裁决轮**（逐条裁定 39 条） | ⬜ **下一步** | — |
| Dev 两棒并行 | ⬜ | — |
| 终版全量 + E2E | ⬜ | — |
| 独立复核 | ⬜ | — |
| 合并 + 回收 worktree | ⬜ | — |

---

## 4. ⚠️ 回来第一件事：裁决这 9 条**阻塞级**

BA 已独立核实前 4 条**全部成立**（不是 QA 错报）：

| # | 阻塞项 | BA 的核实结果 | 严重度 |
|---|---|---|---|
| **S-01** | 配置项缺 `scope: "platform"` | ✅ 成立：`ConfigSpec` 的 `default_scope()` 返回 **`ConfigScope::Tenant`**（`crates/platform-core/src/extension/manifest.rs:264-266`）。而 `app.recentCount` / `app.workspace.maxApps` 是**平台级**配置 → 平台级读写只认 `Platform` scope → **AC-S-160 步骤 5 / AC-S-161 步骤 6 的「配置生效」永远达不到**，且**只有运行时静默过滤**（manifest 能解析、迁移能写库、编译能过）| 🔴 **最危险**（失败现象像业务 bug，会白烧一轮返工） |
| **S-03** | **运行时路由在这套壳层里没有落点** | ✅ 成立：① `RouteDefinition` **没有 `component` 字段**（`apps/shell/src/lib/router.ts:40-60`）；② `App.svelte` 按路径**硬编码 if/else 共 11 处**（`:221-256`）；③ `routes` 是 **`const`**（`App.svelte:68`）→ 「加路由就能渲染新页面」**做不到**；要真生效**必须改 `router.ts`**，又**违反 D270** | 🔴 **最重**（AC-S-163 在设计层是死路） |
| **S-09** | D269 的导航字段凭空多出 3 个 | ✅ 成立：`MenuSpec` 只有 **4 个字段**（`label` / `icon` / `route` / `perm`，`manifest.rs:120-133`）→ 我 D269 里写的 `labelKey` / `category` / `sortOrder` / `key` **全部不存在** | 🔴 高（AC-S-163 步骤 4 的「标题 = labelKey」直接落空；要修得改 manifest schema = 跨需求，D109 要声明） |
| **S-10** | AC-S-024 的「`exp` 距撤销时刻 ≤ TTL」**数学恒真** | ✅ 成立：`exp = iat + TTL`，撤销必然晚于签发 → `exp - t_revoke = TTL - (t_revoke - iat) < TTL` **恒成立** → 零区分力 | 🟡 中（QA 给了替代：改成 `exp - iat == TTL` 的双向等式） |

**另外 5 条阻塞（S-02 / S-04 / S-05 / S-06 / S-07 之类的编号）我还没逐条核实** —— 回来先读
`BA/feature-meta/qa-req016-spec-review.md` 的 §1 总览与逐条清单。

---

## 5. 我自己已经知道的**规格问题**（裁决时要一并处理）

| 问题 | 现状 | 要改成 |
|---|---|---|
| `decisions.md:423` 还留着「**11 列**」 | D265 的表是 **12 列**（含 `launch_url`） | 改成 12 |
| `design-summary.md:64` 说「D265 漏了 `launch_url`」 | **错的说法** —— D265:125 就有 | 删掉这句（或改成「已含」） |
| D274 标题写「**5 个**配置项」 | 实际只有 **3 行** | 改成 3 |
| `capabilities/` 的组成 | 我写「8 个固定 + 6 个 sdk」 | 实际 **7 + 7 = 14**（QA 的 S-15 验的） |
| `design-summary.md:174/184` 要求 `launch_url` 非空才可见 | 与 AC-S-160 的播种（没给 `launch_url`）**冲突** | **选方案 A**（工作台不按 `launch_url` 过滤）或 **B**（播种全填）；QA 推荐 A |

---

## 6. 关键决策速查（D262~D275）

| 决策 | 内容 |
|---|---|
| **D262** | 应用注册表 = 新表 `platform_app`，**租户级**（RLS），**引用**（不复制）`platform_oauth_client` |
| **D263** | 图标用 URL，**不做上传**（文件存储归 REQ-018） |
| **D264** | client 删除 → 应用 **`ON DELETE CASCADE`**（**不能用 RESTRICT**，否则改坏 REQ-022 的 AC-S-135 步骤 7） |
| **D265** | `platform_app` 的 **12 列**（含 `launch_url`） |
| **D266** | 应用级权限声明 = client `allowed_scopes` 的**只读投影**，应用表不存 scope |
| **D267** | 工作台 = `active` 过滤 + 确定性排序（category 字典序 → sortOrder → displayName） |
| **D268** | 访问记录表 `platform_app_access`，用 **`client_id`** 不用 `app_id`（app 行会被级联删）；`access_count` 必须**累加** |
| **D269** | 菜单下发 = 统一「导航项」，`module` + `app` 两来源合并；**必须带 `kind`**（internal/external） |
| **D270** | 前端**不动 `router.ts`**；新增纯函数 `routesFromNavigation`（只处理 internal）—— ⚠️ **被 S-03 推翻，需重新设计** |
| **D271** | 权限点 **3 个**；工作台/导航**不加**权限点 |
| **D272** | AC 编号 **AC-S-156~165**（实测 155 是全仓最大，156~165 空闲） |
| **D273** | 新 crate **`platform-app`**（不塞进 platform-oauth） |
| **D274** | 配置项 **3 个**（`app.recentCount` / `app.workspace.maxApps` / `app.icon.allowHttp`） |
| **D275** | **AC 里任何数字必须有 `file:line` 或实测命令支撑** |

---

## 7. 6 个关键数字（已实测核对）

| 数 | 当前值 | 本轮目标 | 依据 |
|---|---|---|---|
| `TITLES` 行数 | **93** | **99**（+6） | 脚本数出 93；6 个 `APP_*` 码，每个都有 AC 触发 |
| `capabilities/permissions.json` 的 `total` | **32** | **35**（+3） | 3 个 `app:*` |
| `capabilities/` 文件数 | **14** | **14（不增不减）** | 生成器按类别产出：**7 固定 + 7 sdk** |
| `platform_app` 列数 | — | **12** | D265 |
| 错误码 | — | **6 个** | 原 8 个，删 2 个死码 |
| 事件码 | — | **3 个** | `app.registered` / `app.updated` / `app.deleted` |

---

## 8. 回来后按这个顺序做

1. **读** `BA/feature-meta/qa-req016-spec-review.md`（49 KB，39 条逐条 + 证据）
2. **读** `BA/demands/REQ-016-app-integration/test-cases/index.md`（61 条用例的索引与追溯矩阵）
3. **裁决轮**：39 条逐条裁定（接受 / 接受但改判据 / 驳回+理由），重点先定 **S-03**（运行时路由怎么做）
4. 改规格 → 提交 → 派 **worker** 角色做 Dev（**不要派 explore，它不能写文件**）
5. Dev → 终版全量 + E2E → 独立复核 → 合并

---

## 9. 两个派单教训（这轮踩的，别重复）

1. **⚠️ 要落盘交付的子 Agent 必须派 `worker`，不能派 `explore`**
   —— 我给 QA 派了 explore，它因只读约束**拒绝写文件**（做得对，主动声明了 ROLE_MISMATCH），
   我只能从它的回复里逐字恢复。**下次派 QA 用 worker。**
2. **⚠️ 「写了但没落盘」已出现 2 次**
   —— 棒1（REQ-022）第一次只读不写零产出；QA（REQ-016）也是。**派单时明确要求「产物必须用 write 工具落盘，回复里只给路径与摘要」。**

---

## 10. REQ-016 之后还有 2 个（Sprint 5 剩余）

| 顺序 | 需求 | 依赖 | 备注 |
|---|---|---|---|
| 2 | **REQ-015 通知系统** | REQ-002 ✅ + REQ-021 ✅ | ⚠️ **它是「次长关键链」的最后一环**（`002→006→007→010→021→015`，6 环），做完这条链就通了 |
| 3 | **REQ-018 文件存储与导入导出** | REQ-002 ✅ | 最独立，可与 015 并行 |

之后：Sprint 6（REQ-019 可观测性）→ Sprint 7（REQ-020 真实消费方验证）。
