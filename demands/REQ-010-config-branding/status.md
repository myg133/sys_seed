# 状态：REQ-010 配置、品牌与 Feature Flag

- **状态**: 待验证
- **更新者**: myg133-ba-1 (mcp:proc-83964)
- **更新时间**: 2026-11-19 02:15
- **分支**: `feature/REQ-010` @ `a4af6ee`（修复轮已闭合）

## 状态历史

| 时间 | 从 | 到 | 操作者 | 备注 |
|------|----|----|--------|------|
| 2026-11-17 23:45 | — | 草稿 | myg133-ba-1 | 从 REQ-001 拆分创建 |
| 2026-11-18 22:40 | 草稿 | 已就绪 | myg133-ba-1 | 规格写实：14 条 AC（AC-S-049~062）+ 设计摘要 11 节；记 D45~D54 |
| 2026-11-18 22:45 | 已就绪 | 已就绪 | myg133-ba-1 | QA 派出写用例（明确要求挑规格毛病）；`api-conventions` 升 v2 |
| 2026-11-18 23:05 | — | — | qa | QA 交付 **46 条用例 + 36 条规格问题（12 条硬伤）**，全部有 `文件:行号` 证据 |
| 2026-11-18 23:10 | 已就绪 | 已就绪 | myg133-ba-1 | **规格 rev 2**：36 条全部接受并修订（新增 AC-S-063）；记 **D55~D68**。其中 7 条是 BA 自己的设计错误 |
| 2026-11-18 23:15 | — | — | qa | QA 按 rev 2 重写 51 条用例，并挑出 **14 条**新问题（含 2 条**权限漏洞**） |
| 2026-11-18 23:30 | 已就绪 | 已就绪 | myg133-ba-1 | **规格 rev 3 定稿**：15 条 AC（修正计数错误）；记 **D69~D76**（读路由同构、flag 越权、upsert 自增 epoch、伪租户保留值等）；契约 v2 补 9.7/9.8 |
| 2026-11-18 23:33 | 已就绪 | 进行中 | dev | Dev 认领，worktree `feature-REQ-010` |
| 2026-11-19 00:22 | 进行中 | 待验证 | dev | Dev 交回：`f8b251f`，54 文件 / +7787 −628；自验 537 passed / clippy 0 / E2E 2 passed / 6 处变异验证 |
| 2026-11-19 00:25 | 待验证 | 待验证 | ba | 派 QA Pre-merge（按 [qa-brief-REQ-010.md](../../sprint/qa-brief-REQ-010.md)），重点四个「假通过探测器」 |
| 2026-11-19 00:52 | 待验证 | 已退回 | ba | QA 结论 **FAIL**：HIGH 1（默认日志过滤吞 WARN）/ MEDIUM 1 / LOW 2 / INFO 6。集成 539/539 连跑 3 轮、前端确认 23→28（+5，无退化） |
| 2026-11-19 01:57 | 已退回 | 进行中 | dev | 修复轮：`f0f0ed0` 根因修日志过滤器（抽到 `platform-core::log`，测试与生产同一函数）+ `a4af6ee` E2E 精确断言与越级写快照比对 |
| 2026-11-19 02:15 | 进行中 | 待验证 | ba | 派 QA 修复复核（重点：默认部署 WARN 端到端、E2E 断言强度、快照比对、用例漂移） |

## rev 2 修掉的 7 条 BA 自己的设计错误（Dev 必读，别再按初版写）

| # | 初版怎么写 | 为什么错 | rev 2 |
|---|---|---|---|
| 1 | `SELECT max(epoch)` 做失效判断 | 租户 epoch 高于 platform 时，平台级写入不改变 `max` → **失效不掉** | 缓存存**一对** epoch，逐一比较（D55） |
| 2 | 「租户连接连写都写不进去（表级权限）」 | 只有一个 `base_app` 池、无平台管理角色，且 `ALTER DEFAULT PRIVILEGES` 让新表**自动继承写权** | `REVOKE` 直接 DML + `SECURITY DEFINER` 函数（D56） |
| 3 | flag 登记 `type: boolean` 又要 `rolloutPercent` | `ConfigSpec` 无该字段；另立键被「未声明 400」拒；塞进 boolean 被类型校验拒 | 派生键 `flags.<F>.rolloutPercent`（D57） |
| 4 | 「跑平台的契约测试」验 C-5 | `ContractFixture` 要求 `Repository`+`PlatformModule`，平台 crate 没有也不该有 | 新增不依赖 fixture 的扫描入口（D58） |
| 5 | 一条 `PUT` 路由管租户级与平台级写 | 静态 `route_layer` 会让平台权限点挡住租户级写；改 handler 内判定则无调用点 → C-5 失败 | 拆两条路由（D59） |
| 6 | 给 `ConfigScope` 加 `default` 变体 | `ConfigScope` 同时是写入作用域的类型 → 凭空开出**第 4 个可写层** | 新增独立 `ConfigSource`（D60） |
| 7 | 未定校验顺序 | 越级写会把类型校验顶包 → 类型校验根本没被验到 | 写死 6 步顺序（D64） |

> **这 7 条如果留到 Dev 实现后才发现，每一条都要返工一轮。** 它们都属于「看起来对」。

## 规格要点（Dev 必读）

| 项 | 口径 |
|---|---|
| 父 AC | AC-013（三层覆盖）、AC-014（品牌，**本期收窄**，见 D52）、AC-015（Feature Flag） |
| 本需求 AC | **AC-S-049 ~ AC-S-063**（**17 条**，全局唯一段；原占位 `AC-S-015` 已被 `AC-S-052` 取代） |
| 契约 | `api-conventions` **v2**（已 ratified，本需求是第一个消费方）；`extension-points v2`（**C-5** 要覆盖新权限点） |
| 表 | **三张分离**：`platform_config`（无 tenant_id、无 RLS）/ `platform_tenant_config`（已存在）/ `platform_space_config`（新建）。**不做单表 nullable**（D46） |
| 值列 | 三张表统一 **`jsonb`**；`platform_tenant_config.value` 从 `TEXT` 迁移（含 down，D47） |
| 缓存失效 | `scope_key` = `'platform'` 或 `'tenant:<id>'`；**成对比较两个 epoch**，不取 max（D55） |
| 灰度 | **确定性**：`sha256("<flag>:<user>") % 100`；**禁 `RandomState`**；无身份 → `enabled=false`（D49） |
| 品牌接口 | **公开（无令牌）** + **显式白名单键** + **忽略空间级覆盖**（D50） |
| 空间级 | 只对 `scope: space` 的**展示类**键生效；**Feature Flag 不支持空间级**（D51） |
| 键注册表 | 来自 manifest 的 `config` 段；**未声明的键 → 400**；`scope` = 「**允许覆盖到的最深层**」（D53） |
| 权限点 | **4 个**：`config:entry:write` / `config:flag:write` / `config:platform:write` / **`config:platform:read`**；**必须同时写种子迁移**（D67） |
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

1. QA 按 rev 2 修订用例（进行中）
2. 认领 REQ-010 → Dev 实现 → QA Pre-merge → 合并 → 复盘

## 阻塞项

无。REQ-006 与 REQ-009 均已完成并合并 `develop`。
