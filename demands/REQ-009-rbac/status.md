# 状态：REQ-009 RBAC 权限与数据范围

- **状态**: 待验证
- **更新者**: myg133-ba-1 (mcp:proc-83964)
- **更新时间**: 2026-11-18 20:20
- **分支**: `feature/REQ-009` @ `05bc6bd`（Dev 交回）

## 状态历史

| 时间 | 从 | 到 | 操作者 | 备注 |
|------|----|----|--------|------|
| 2026-11-17 23:45 | — | 草稿 | myg133-ba-1 | 从 REQ-001 拆分创建 |
| 2026-11-18 20:50 | 草稿 | 已就绪 | myg133-ba-1 | 规格写实：16 条 AC（AC-S-033~048）+ 设计摘要修订；`extension-points` 升 v2（新增 C-5） |
| 2026-11-18 18:57 | 已就绪 | 进行中 | dev | Dev 认领，worktree `feature-REQ-009` |
| 2026-11-18 19:15 | 进行中 | 待验证 | ba | 规格经 QA 用例评审**修订三轮**（QA 挑出 22+3 条问题，全部接受），BA 派 Dev 实现 |
| 2026-11-18 20:01 | 待验证 | 进行中 | dev | Dev 交回：`628ac19`，61 文件 / +11692 行；自验 439 passed / clippy 0 告警 |
| 2026-11-18 20:05 | 进行中 | 待验证 | ba | BA 裁定 Dev 上报的规格问题（D37 真并集 / D38 trait 扩展），派定点修复 |
| 2026-11-18 20:16 | 待验证 | 进行中 | dev | 修复轮：`05bc6bd` 修正 CUSTOM 多角色合并语义为真并集，452 passed |
| 2026-11-18 20:20 | 进行中 | 待验证 | ba | 派 QA Pre-merge（按 [qa-brief-REQ-009.md](../../sprint/qa-brief-REQ-009.md)） |

## 规格要点（Dev 必读）

| 项 | 口径 |
|---|---|
| 父 AC | AC-001（权限点注册表驱动前后端判定）、AC-002（数据范围隔离生效） |
| 本需求 AC | **AC-S-033 ~ AC-S-048**（共 16 条，全局唯一编号段） |
| 契约 | `api-conventions v1` C4 / C5 / C6 / C7；`extension-points v2`（**C-5 是新增的失败级检查**） |
| 错误码 | 401 `UNAUTHENTICATED` / 403 `INSUFFICIENT_PERMISSION` / 404 `NOT_FOUND`（对齐契约 C6 示例，**不用** `PERMISSION_DENIED`） |
| 数据范围 | `ALL` / `OWN_ORG` / `OWN_ORG_AND_BELOW` / `SELF` / `CUSTOM` |
| 兼岗 | **默认主部门**（`user_org.is_primary`）；取并集必须显式开启（角色级 `use_all_memberships`） |
| 主部门缺失 / 歧义 | **fail closed 到 `SELF`**（不是 `ALL`）；DB 加部分唯一索引兜底 |
| **多角色合并** | **过滤条件的并集**（D37）：`CUSTOM` 之间 OR，`CUSTOM` 与包含型 scope 也 OR，含 `ALL` 直接 `ALL` |
| 属性条件非法 | 返回**永不匹配的条件**（不是 `None`）→ fail closed（D40） |
| 缓存 | 进程内 + **DB epoch 主动失效**；**不引 Redis 客户端** |
| 对账 | manifest ↔ 库不一致 → **启动失败**（不是告警） |

## 开工前必须先修的既有缺陷（**Dev 已修**）

`scripts/sync-contracts.sh` 在 `develop` 上**恒定失败**（exit 2）：

```
✗ feature-meta/manifest.json 缺少非空的 contracts pin。
```

**修法**：`feature-meta/manifest.json` 加

```json
"contracts": { "api-conventions": "v1", "extension-points": "v2" }
```

并把 `docs/contracts/extension-points/v2.md` 快照同步进来（`scripts/sync-contracts.sh --sync`）。
脚手架模板（`crates/xtask/src/scaffold.rs`）也应带上这个 pin，否则下一个需求还会踩。

> ✅ Dev 已在 `05bc6bd` 中修掉：`sync-contracts.sh` 现 exit 0。

## 阻塞项

无。REQ-006 / REQ-007 / REQ-008 均已完成并合并 `develop` @ `400cefa`。
