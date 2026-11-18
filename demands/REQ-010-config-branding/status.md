# 状态：REQ-010 配置、品牌与 Feature Flag

- **状态**: 已就绪
- **更新者**: myg133-ba-1 (mcp:proc-83964)
- **更新时间**: 2026-11-18 22:40
- **分支**: 待认领后创建 `feature/REQ-010`

## 状态历史

| 时间 | 从 | 到 | 操作者 | 备注 |
|------|----|----|--------|------|
| 2026-11-17 23:45 | — | 草稿 | myg133-ba-1 | 从 REQ-001 拆分创建 |
| 2026-11-18 22:40 | 草稿 | 已就绪 | myg133-ba-1 | 规格写实：**14 条 AC（AC-S-049~062）** + 设计摘要 11 节；记 D45~D54 |

## 规格要点（Dev 必读）

| 项 | 口径 |
|---|---|
| 父 AC | AC-013（三层覆盖）、AC-014（品牌，**本期收窄**，见 D52）、AC-015（Feature Flag） |
| 本需求 AC | **AC-S-049 ~ AC-S-062**（14 条，全局唯一段；原占位 `AC-S-015` 已被 `AC-S-052` 取代） |
| 契约 | `api-conventions v1` C9 / C10 / C3 / C6；`extension-points v2`（**C-5** 要覆盖新权限点） |
| 表 | **三张分离**：`platform_config`（无 tenant_id、无 RLS）/ `platform_tenant_config`（已存在）/ `platform_space_config`（新建）。**不做单表 nullable**（D46） |
| 值列 | 三张表统一 **`jsonb`**；`platform_tenant_config.value` 从 `TEXT` 迁移（含 down，D47） |
| 缓存失效 | **`scope_key`** = `'platform'` 或 `'tenant:<id>'`，读 `max(epoch)` 一条查询（D48） |
| 灰度 | **确定性**：`sha256("<flag>:<user>") % 100`；**禁 `RandomState`**；无身份 → `enabled=false`（D49） |
| 品牌接口 | **公开（无令牌）** + **显式白名单键** + **忽略空间级覆盖**（D50） |
| 空间级 | 只对 `scope: space` 的**展示类**键生效；**Feature Flag 不支持空间级**（D51） |
| 键注册表 | 来自 manifest 的 `config` 段；**未声明的键 → 400**；`scope` = 「**允许覆盖到的最深层**」（D53） |
| 权限点 | `config:entry:write` / `config:flag:write` / **`config:platform:write`**（平台级写入必须独立权限点，AC-S-060） |
| 前端 | shell 的 `<title>` / favicon / 主色**从接口读**，去掉硬编码；令牌体系归 REQ-011（AC-S-055） |

## 开工前必须知道的既有事实（已核实）

| 事实 | 影响 |
|---|---|
| `crates/platform-core/src/config.rs` **已有一个 `ConfigResolver`**（REQ-002 骨架，纯内存 `Vec<ConfigEntry>`，无 DB 无缓存） | 本需求要把它**改成 trait**，骨架保留为 `InMemoryConfigResolver`。**不是新增同名类型** —— Dev 要 grep 全部引用点（D-风险 R9） |
| `platform_tenant_config` 表**已存在**（REQ-006 建），`value` 是 `TEXT` | 要写迁移改成 `jsonb`，**带 down 脚本** |
| 本仓**没有 space 表**，`X-Space-Id` 只是请求头字串 | 空间级配置只能按字符串存；安全边界见 D50 / D51 |
| 现有权限点 **18 个**（sandbox 5 / org 9 / rbac 4），**没有任何 config 相关** | 本需求要新增 3 个并保证 C-5 有调用点 |
| `apps/shell/index.html` 有 `<title>底座壳</title>`；`packages/theme/src/index.ts` 有 `primaryColor: '#1677ff'` | 这两个是 AC-S-055 要改掉的硬编码 |
| 平台级表的先例：`platform_meta` / `platform_permission`（**无 tenant_id、无 RLS**） | `platform_config` 照此办理；但**写入必须靠表级权限挡住租户连接**（AC-S-062 第 2 步） |
| `sha2` **已在依赖树里**（REQ-007 引入） | 确定性哈希不新增依赖 |

## 下一步

1. **QA 写用例 + 挑规格毛病**（进行中）→ BA 裁定
2. 认领 → Dev 实现 → QA Pre-merge → 合并 → 复盘

## 阻塞项

无。REQ-006 与 REQ-009 均已完成并合并 `develop`。
