# 状态：REQ-020 底座可用性验证

- **状态**: **已完成**（规格 rev 2 + 实现 4 批 + 独立复核 15 条全处置）
- **更新者**: myg133-ba-1
- **更新时间**: 2026-11-23
- **代码**: `feature/REQ-020` @ `a8c4b1e`（`fd0d099` = BASE_UPGRADE，`52b9314` = v1→v2 升级，`a8c4b1e` = 注释修正）
- **已合并**: `develop` @ **`bd2d81a`**（`--no-ff`），合并后全量 1673 / 0 / 0 已复跑

## 文件清单

| 文件 | 状态 |
|---|---|
| [demand.md](demand.md) | ✅ rev 2（来源披露 + 实测基线） |
| [acceptance.md](acceptance.md) | ✅ **rev 3**（8 条 AC；阳性对照数值实测刷新） |
| [decisions.md](decisions.md) | ✅ **rev 3**（D347~D357；D353 登记口径修正） |
| [design-summary.md](design-summary.md) | ✅ **rev 3**（新增 §3b「模块迁移必须手工应用」部署前提） |
| [metrics-report.md](metrics-report.md) | ✅ **实现后产出**（508 行，含终版实测 + 15 条复核处置回执） |
| `test-cases/` | ✅ **38 条**（TC-020-001 ~ TC-020-038） |
| 独立挑刺（规格阶段） | ✅ **26 条**全处置 |
| 独立复核（实现阶段） | ✅ **15 条**（1 BLOCKER / 2 MAJOR / 8 MINOR / 4 NIT）全处置 → [`../../feature-meta/qa-req020-impl-review.md`](../../feature-meta/qa-req020-impl-review.md) |

## 状态历史

| 时间 | 从 | 到 | 操作者 | 备注 |
|------|----|----|--------|------|
| 2026-11-17 23:50 | — | — | myg133-ba-1 | 因 REQ-021 补建、REQ-015/017/019 顺延，顺延到 Sprint 7 |
| 2026-11-23 21:10 | 草稿 | 已就绪（rev 1） | myg133-ba-1 | 规格从 1 条空泛 AC → 8 条；用户裁定 3 项（D347/D349/D350） |
| 2026-11-23 21:40 | 已就绪（rev 1） | 已就绪（rev 2） | myg133-ba-1 | 独立挑刺 26 条全处置；新增 D352~D357 + TD-085~087 |
| 2026-11-23 23:40 | 已就绪（rev 2） | **实现完成** | myg133-ba-1 | 4 批派 worker 实现（用户裁定分工）；全量 19 crate 1673/0/0；三守卫 + 前端 + E2E 全过 |
| 2026-11-23 23:55 | 实现完成 | **已完成** | myg133-ba-1 | 独立复核 15 条全处置；新增 TD-089~094；规格 rev 3 |

## 8 条 AC 的最终结论

| AC | 结论 | 证据 |
|---|---|---|
| **AC-S-186** 平台改动四分类，A 类 = 0 | ✅ **成立** | 提交后实测 **30 个文件**（`git diff --name-only 9bcfbcb 52b9314 \| grep -v '^modules/leave/' \| grep -v '^feature-meta/'`），与分类表**逐项相等、无漏无虚**；**A = 0**；分布 C17 / B5 / D6 / 生成物2 |
| **AC-S-187** 代码量 < 500 行 | ❌ **不成立（如实）** | **1978 行**（后端 1374 + 前端 604），超 1478 行；剔注释/空行/内联测试后仍 **1241 行**。归因已按复核 F-02 修正（模板 396 行里只有 107 行被真实保留） |
| **AC-S-188** 复用面 8 组 grep | ✅ **成立** | 8/8 全中，5 个阳性对照全部有效 |
| **AC-S-189** 审批流 + 计数表 | ✅ **成立** | 计数表逐行断言（`platform-server/tests/leave_real_db.rs:930-1047`）：建单 1 审计 0 通知 / 提交 2 审计 0 通知 / 审批 3 审计 1 通知 / 非法流转零增量 |
| **AC-S-190** 升级零改动 | ✅ **成立（区分力弱，如实标注）** | `git diff --name-only fd0d099 -- modules/leave/` = **0 行**；升级只动平台侧 5 文件。⚠️ 消费方从未使用 v2，故「零改动」近乎结构性必然（复核 F-15） |
| **AC-S-191** 度量可复算 | ✅ **成立** | 复核抽查 12 个数字全部逐字复现（含实跑 vitest 29 files / 422 tests） |
| **AC-S-192** E2E 1 passed / 0 skipped | ✅ **成立** | `stats: {"expected":1,"skipped":0,"unexpected":0,"flaky":0}`；4 个 `test.step` 全过，无 `test.skip` |
| **AC-S-193** 依赖面 | ✅ **成立** | 仅 `platform-core` / `platform-db` + `platform-contracts`(dev)；`Cargo.lock` 无新增第三方 crate |

## 终版实测数字（代码冻结于 `52b9314`）

```
全量 19 crate            1673 passed / 0 failed / 0 ignored
check-modules            exit 0（2 个模块：leave / sandbox）
check-deps               exit 0
capabilities --check     exit 0（14 个文件一致）
tenant-policies --check  exit 0（base_app 角色；base 角色会带 fail-open 警告）
shell 单测               422 passed / 29 files
svelte-check             0 errors / 0 warnings
E2E                      1 passed / 0 skipped / 0 failed
```

> ⚠️ **口径披露**：全量跑了**两次** ——
> ① `52b9314`（代码冻结）→ 1673 / 0 / 0；
> ② 处置复核 F-06 改了一处注释（`a8c4b1e`，无行为变化）后，合并进 `develop`（`bd2d81a`）
> 并**在合并结果上重跑全量 19 crate** → **1673 / 0 / 0**，逐 crate 数字与第一次逐个相同；
> 同一次复跑 4 项守卫（全 exit 0）、shell 422 passed、`svelte-check` 0/0、E2E 1 passed 0 skipped。
> **所有数字以合并后的 `develop` @ `bd2d81a` 为准。**

## 底座缺口清单（本需求的核心产出，8 条）

| # | 缺口 | TD |
|---|---|---|
| 1 | 业务模块装配是**硬编码 match**（新增模块要改 `main.rs` 装配臂） | TD-086（P1） |
| 2 | 通知**无落库适配器** | ✅ 已补（D354 `DbNotificationSender`） |
| 3 | manifest schema **无行内动作**（带审批要写自定义页 + 改 shell） | TD-087（P2） |
| 4 | **无法按权限点反查用户**（「提交时通知审批人」做不到） | TD-088（P2） |
| 5 | **样板代码偏重**（骨架范例是内存存储，真实 DB 化要整体重写） | 见归因 |
| 6 | 前端 i18n **无模块级命名空间** | 建议 |
| 7 | 前端跨模块引用**要改宿主 tsconfig** | 建议 |
| **8** | **模块建表迁移无任何部署路径会应用**（全新环境部署硬失败） | **TD-089（P1）** |
| 9 | `NotificationSender` **非 dyn 兼容**（每模块自写 18 行装箱 wrapper） | TD-090（P2） |
| 10 | 列表**无 cursor 分页**，信封与平台不一致 | TD-091（P2） |

（#1~#7 为规格阶段预判并实测确认；#8~#10 为实现阶段独立复核新发现。）

## 新增技术债

| TD | 标题 | 优先级 |
|---|---|---|
| TD-089 | 模块建表迁移无部署应用路径 | **P1** |
| TD-090 | `NotificationSender` 非 dyn 兼容 | P2 |
| TD-091 | 列表无 cursor 分页 / 信封不一致 | P2 |
| TD-092 | 写库与审计非原子（审计故障 → 已生效但 500） | P2 |
| TD-093 | 审计测试夹具全表 `DELETE`，真库判据不可事后复核 | P2 |
| TD-094 | 请假审批允许自批（申请人 == 审批人） | P3 |

## 本轮的方法论教训（三条，均已在 metrics-report 里写明）

1. **守卫的绿灯要问「它在什么前置条件下才绿」** —— `tenant-policies --check` 在 `base_test` 上
   一直是绿的，因为表是测试夹具建的；干净库上 exit=1。
2. **「某东西占了多少行」必须拿实际文件逐行比对**，不能用常量长度代替（F-02）。
3. **提交信息里的数字必须是可复算命令的原文粘贴**，否则会留下不可解释的不一致（F-08：110 vs 127）。

## 遗留（非阻塞）

- **合并**：`52b9314` 需合入 `develop`
- **部署前提**：模块迁移必须手工应用（TD-089 未修前）；租户需自种 `leave_decision` 模板
- **口径**：AC-S-190 未验证「消费方能扛住破坏性变更」（D349/D352 的选择），如实标注
