# 状态：REQ-003 扩展点框架与契约测试

- **状态**: 已完成
- **更新者**: myg133-ba-1 (mcp:proc-83964)
- **更新时间**: 2026-11-18 01:50
- **分支**: `develop` @ `c8236b0`（已合并）

## 状态历史

| 时间 | 从 | 到 | 操作者 | 备注 |
|------|----|----|--------|------|
| 2026-11-17 23:45 | — | 已就绪 | myg133-ba-1 | 从 REQ-001 拆分创建 |
| 2026-11-18 01:10 | 已就绪 | 进行中 | dev | Dev 交回：48 文件 / +3793 行，Rust 64/64，5 个 trait 冻结 + C-1~C-4 契约检查 |
| 2026-11-18 01:25 | 进行中 | 待验证 | ba | 派 QA Pre-merge |
| 2026-11-18 01:30 | 待验证 | 已退回 | ba | QA 结论 PASS-WITH-RISK，2 中 + 4 低 + 2 提示 |
| 2026-11-18 01:40 | 已退回 | 进行中 | dev | 修复轮：2 中 + 2 低 + 1 提示全修，Rust 73/73 |
| 2026-11-18 01:48 | 进行中 | 待验证 | ba | QA 轻量复核 **PASS**，5/5 改动点复验通过 |
| 2026-11-18 01:50 | 待验证 | 已完成 | ba | 合并到 develop @ c8236b0，worktree 与分支已回收 |

## 完成记录

| 项 | 结果 |
|---|---|
| 合并 | `develop` @ `c8236b0`，`--no-ff` |
| 改动 | 8 个 Dev 提交 + 1 个 BA 归档提交 / 60 文件 / +4907 -550 行 |
| 回收 | worktree 与分支已删除；两份 QA 报告随合并保留在 `develop` |
| 回归 | Rust 73/73、clippy 0 告警、前端 23/23、E2E 1/1、**Cargo.lock 零新第三方包** |
| 契约 | 新增 [extension-points v1](../../contracts/extension-points/v1.md)（ratified） |

## QA 两轮发现的问题与处置

| 级别 | 问题 | 处置 |
|---|---|---|
| 中 | `check-modules` 静默吞掉结构级 manifest 违规 | 已修：serde 错误作为 manifest_issues 报出（`{}` / 缺字段 / 未知字段三种均 exit 1） |
| 中 | 依赖方向检查是字符串扫描，可被 workspace 继承 / 未列入清单 crate 绕过 | 已修：改用 `cargo metadata --no-deps` resolve 图 + 可达性遍历，平台 crate 集合含 `platform-contracts-macros`；QA 独立复现两个绕过均被拦 |
| 低 | manifest Schema 与平台校验器判定不一致 | 已修：schema 收紧到与实现一致（design-summary §5 已同步）。**遗留**：U+FEFF / U+0085 两个 Unicode 空白码位仍分歧 |
| 低 | sandbox 排序白名单外字段静默忽略（与契约 C7 不符） | 已修：改为返回错误，并在 sandbox README 写明 |
| 低 | 扩展点冻结签名的 dyn 兼容性变更未登记 | **BA 已关闭**：design-summary §2 + extension-points v1 均写明 RPITIT 形态不可用于 trait object |
| 低 | C-2 无法区分「登记了权限点」与「真的做了鉴权」 | **BA 已定口径**：C-2 只验声明完整性；鉴权执行属 REQ-009。已写入 design-summary §4.3 与 extension-points v1 |
| 提示 | `claim` 工具重建 manifest.json 时冲掉契约 pin | 已补回，并记入 roadmap R10 |
| 提示 | `migrations/` 只有 README 占位也能过「非空」校验 | 记入 roadmap R9：REQ-006/007 落地 DB 层时升级为「至少一个 `*.sql`」 |

## 阻塞项

无 —— 依赖 REQ-002 已完成，REQ-003 已合并。

## 遗留技术债（供后续 Sprint 取用）

| # | 债务 | 建议归属 |
|---|------|----------|
| 1 | U+FEFF / U+0085 两个 Unicode 空白码位，schema 求值器与平台实现判定分歧 | REQ-004（能力索引生成器会复用 schema） |
| 2 | C-2 无法验证「权限点真的被鉴权」 | REQ-009（RBAC 落地鉴权中间件后补检查） |
| 3 | 过滤字段白名单外仍静默忽略（本期只改了排序） | REQ-009 / REQ-011 |
| 4 | 扩展点 trait 非 dyn-compatible，异构汇聚会撞墙 | REQ-015 / REQ-017 决策 |
