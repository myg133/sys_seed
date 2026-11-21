# 03 工作台 / 常用 / 最近（AC-S-160、AC-S-161）

> **前置修订（因 S-04）**：AC 原文的播种**没给 `launch_url`**，而 `design-summary.md:174`/`:184` 要求
> `launch_url` 非空才可见/可记访问。**BA 必须先统一口径**：
> - **方案 A（推荐）**：工作台**不**按 `launch_url` 过滤（改 `design` §4.2/R6），下面用例按 A 写；
> - **方案 B**：保留过滤 → 播种必须给 A~E 全部填 `launch_url`（用例预期不变）。

### 共用夹具（tenant-a，全部 `status=active` 除 E）

| 应用 | client | category | sortOrder | displayName | launch_url |
|---|---|---|---|---|---|
| A | `client-A` | finance | 20 | `差旅报销` | `https://a.example/` |
| B | `client-B` | finance | 10 | `费用申请` | `https://b.example/` |
| C | `client-C` | general | 0 | `C 应用` | `https://c.example/` |
| D | `client-D` | general | 0 | `D 应用` | `https://d.example/` |
| E | `client-E` | general | 0 | `E 应用` | `https://e.example/`（**status=hidden**） |

### TC-016-030 工作台 200 + 恰好 2 组 + 组顺序
- **AC**: AC-S-160 步骤 1、3 ｜ **类型**: 功能 ｜ P0 ｜ 集成 ｜ 等价类
- **步骤**: 以 `u-perm-read`（登录即可）`GET /api/apps/workspace`
- **预期**: 200；`groups.length == 2`；`groups[0].category=="finance"`、`groups[1].category=="general"`（字典序升序）

### TC-016-031 组内排序：finance 为 B→A；general 为 C→D
- **AC**: AC-S-160 步骤 3 ｜ P0 ｜ 集成 ｜ 边界值（并列 sortOrder）
- **预期**: `groups[0].apps.map(displayName) == ["费用申请","差旅报销"]`（sortOrder 10<20）；`groups[1].apps.map(displayName) == ["C 应用","D 应用"]`（sortOrder 并列 → displayName 升序）

### TC-016-032 hidden 应用不在任何组里
- **AC**: AC-S-160 步骤 3 ｜ P0 ｜ 集成 ｜ 判定表
- **预期**: 所有 `groups[*].apps[*].appId` **不含** E 的 `appId`；总数 == 4

### TC-016-033 确定性：3 次响应逐字节相同 + 完整期望数组〔AC 修正〕
- **AC**: AC-S-160 步骤 2 ｜ P0 ｜ 集成 ｜ 因果
- **步骤**: 连打 3 次，`serde_json::to_string(&groups)` 比较
- **预期**: ① 3 次字符串**逐字节相同**；② **并且**整体等于写死的期望 JSON（category/appId 顺序全部钉死）
- **〔AC 修正〕**: 只比「3 次相同」会被「稳定但错序」蒙过（S-19）；`HashMap` 分组还可能偶发红

### TC-016-034 maxApps=2 → 恰好 2 条且是**指定的**那两条〔AC 修正〕
- **AC**: AC-S-160 步骤 4-5 ｜ P0 ｜ 集成 ｜ 边界值
- **步骤**: 1) `base` 执行 `SELECT platform_config_upsert('app.workspace.maxApps','2'::jsonb)` 2) 立刻 `GET /api/apps/workspace`
- **预期**: 应用总数**恰好 2**；且 == 全局 `(category, sortOrder, displayName)` 排序后的前 2 条（按 `design` §4.2 的语义：`finance` 组 B、A）
- **〔AC 修正〕**: ① 原文「不超过 2」容许 0/1（S-18）→ **D 已改成「恰好 2」**；② 原文没写改配置的手段 → 必须用 `platform_config_upsert`（自增 epoch），直连 `UPDATE` 不失效（S-07）
- **⚠️ 需先定 S-18**：截断在分组前还是后；本条按「全局排序后取前 N，再分组」写

### TC-016-035 launch_url 为空的应用不出现在工作台〔AC 补充〕
- **AC**: D269/design §12 R6 ｜ P1 ｜ 集成 ｜ 边界值
- **前置**: 若 BA 选**方案 B**：把 D 的 `launch_url` 置 NULL
- **预期**: D 不在任何组里；若选**方案 A**，本用例作废（并删掉 design 的该条过滤）

### TC-016-036 recent 首条是最后访问的应用
- **AC**: AC-S-161 步骤 1-2 ｜ 功能 ｜ P0 ｜ 集成 ｜ 状态迁移
- **前置**: `platform_app_access` 里 `(tenant-a, u-perm-read, client-A/B/C)` 恰好 3 行（`access_count=1`）
- **步骤**: 1) `POST /api/apps/{A}/access` 2) `POST /api/apps/{B}/access` 3) `POST /api/apps/{A}/access` 4) `GET /api/apps/recent`
- **预期**: `recent[0].appId == A`；长度 ≤ 6（`app.recentCount` 默认 6）

### TC-016-037 frequent 首条是 access_count 最大的
- **AC**: AC-S-161 步骤 3 ｜ P0 ｜ 集成 ｜ 边界值
- **预期**: `frequent[0].appId == A`（`access_count==2`）；并列时按 `last_accessed_at DESC`

### TC-016-038 UPSERT 累加：count→3，行数不变
- **AC**: AC-S-161 步骤 4 ｜ P0 ｜ 集成 ｜ 状态迁移
- **步骤**: 再 `POST /api/apps/{A}/access` → `base` 查 `platform_app_access`
- **预期**: ① 响应 `{status:"recorded", accessCount:3}`；② A 那行 `access_count==3`；③ `SELECT count(*) WHERE (tenant_id,user_id) = ('tenant-a','u-perm-read')` 仍 **3**；④ `client_id` 集合 == `{client-A,client-B,client-C}`
- **〔AC 修正〕**: 原文「行数仍为 3」没绑定 `user_id`（S-28）

### TC-016-039 hidden 后 B 不在 recent
- **AC**: AC-S-161 步骤 5 ｜ P0 ｜ 集成 ｜ 因果
- **步骤**: `PATCH /api/apps/{B}`（或直连）把 `status='hidden'` → `GET /api/apps/recent`
- **预期**: `recent` 里**不含** B；A 仍在

### TC-016-040 recentCount=1 → 长度 1
- **AC**: AC-S-161 步骤 6 ｜ P0 ｜ 集成 ｜ 边界值
- **步骤**: `base` 执行 `SELECT platform_config_upsert('app.recentCount','1'::jsonb)` → 立刻 `GET /api/apps/recent`
- **预期**: `recent.length == 1` 且该条是 A
- **〔AC 修正〕**: 原文没写改配置的手段（S-07）

### TC-016-041 只返回在册且 active（孤儿 access 行不可见）〔AC 补充〕
- **AC**: D268「查询时要 join platform_app，只返回当前租户仍在册且 active 的应用」｜ P1 ｜ 集成 ｜ 边界值
- **前置**: 造一行 `client-orphan` 的访问记录，且 `platform_app` 里**没有**对应行（模拟 client 被删后残留，S-13）
- **步骤**: `GET /api/apps/recent` / `GET /api/apps/frequent`
- **预期**: 两个响应**都不含**该 client 对应的应用；长度不受孤儿行影响
