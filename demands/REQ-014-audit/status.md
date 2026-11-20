# 状态：REQ-014 审计日志

- **状态**: **已合并**（`develop` @ `310e4d3`）
- **更新者**: myg133-ba-1 (mcp:proc-83964)
- **更新时间**: 2026-11-20 18:30

## 状态历史

| 时间 | 从 | 到 | 操作者 | 备注 |
|------|----|----|--------|------|
| 2026-11-17 23:45 | — | 草稿 | myg133-ba-1 | 从 REQ-001 拆分创建 |
| 2026-11-20 09:30 | 草稿 | 已就绪 | myg133-ba-1 | 规格细化：13 条 AC（AC-S-022 + AC-S-122~133）+ D130~D144 + 设计摘要；依赖 REQ-002/006 已完成 |
| 2026-11-20 09:40 | 已就绪 | 进行中 | myg133-ba-1 | BA 认领 REQ-014 → 建 `feature-REQ-014`（基于 develop@0683115） |
| 2026-11-20 10:10 | 进行中 | 进行中（spec rev 3） | myg133-ba-1 | QA 挑刺 **36 条（12 阻塞）→ 13 条 AC 全部有问题**；BA **36 条全接受** → D145~D168；AC rev 3 + design-summary rev 2 |
| 2026-11-20 10:35 | 进行中（spec rev 3） | 进行中（棒 1 完成） | myg133-ba-1 | **棒 1 完成**：前端（`Audit.svelte` + `audit-api.ts` + 37 单测 + 5 E2E，vitest 158→195）+ 后端（`platform-core` 审计类型 + 新 crate `platform-audit` 8 模块 + 3 张表 + 4 个迁移 + 种子脚本例外块） |
| 2026-11-20 11:05 | 进行中（棒 1 完成） | 进行中（棒 2 进行中） | myg133-ba-1 | 裁决 Dev-A 的 **9 条偏离** → **D171~D178**；**实测校正** Dev 的测试数字表；棒 2 分两个并行 Dev（后端接入 / 前端收尾） |
| 2026-11-20 11:35 | 进行中（棒 2 进行中） | 进行中（棒 2 前端完成） | myg133-ba-1 | **棒 2 前端完成**：D169 游标取数 + 游标栈页码指示、5 个错误码 i18n、E2E 夹具（`u-perm-audit` + 恒 11 条）。裁决 **D179 / D180** |
| 2026-11-20 12:20 | 进行中 | 进行中（棒 2 完成） | myg133-ba-1 | **棒 2 完成**，`feature/REQ-014` @ `5f28c0c`（79 文件 / +12209 / -263），BA 统一提交 |
| 2026-11-20 13:10 | 进行中 | **Pre-merge 复验 FAIL** | QA 子 Agent | 复验报告 `qa-premereq-req014.md`：**阻塞 2 / 高 1 / 中 3 / 低 4**。B-01（`nextCursor` 恒 null）、B-02（过滤拼非法 SQL → 503）。BA **独立读码复核属实** → D184~D193 |
| 2026-11-20 14:05 | 复验 FAIL | 修复轮进行中 | myg133-ba-1 | 修复轮 F1~F10；F10 的判据被 BA 查实是**自己写错的**（集合相等 → 改「包含」关系，D194）+ D195 |
| 2026-11-20 15:10 | 修复轮 | **独立复核 FAIL** | QA 子 Agent | 复核报告 `qa-recheck-req014.md`：**0 阻塞 / 1 高 / 2 中 / 3 低**。H-01 = E2E「回退」用例**在干净库上必败**（靠脏库才绿） |
| 2026-11-20 16:00 | 复核 FAIL | 修复轮 2 完成 | myg133-ba-1 | 修复轮 2：F11（E2E 隐含依赖）/ F12（注释与代码不一致 + 跨 crate 三处一致断言）/ F13（报告订正）/ F14（SSO 事件码零覆盖） |
| 2026-11-20 17:30 | 修复轮 2 | **聚焦复核 PASS-WITH-RISK** | QA 子 Agent | 复核报告 `qa-recheck2-req014.md`：**0 阻塞 / 0 高 / 0 中 / 3 低**。干净库 E2E 钉死（35/0/0 + 负向对照） |
| 2026-11-20 18:00 | 聚焦复核 | 微修完成 | myg133-ba-1 | 附录 B：B1（失败分支补 `user_agent` 断言）/ B2（用例名与 doc 对齐）/ B3（`config.flag.write` 真库落表判据） |
| 2026-11-20 18:10 | 微修完成 | **已验证，待合并** | myg133-ba-1 | **BA 终验**：亲自注入 F1/F2 变异 → 各自变红 → 逐字节还原；12 crate 全量回归；干净库 E2E |

## 最终实测基线（BA 亲测，2026-11-20 18:10）

| crate | passed | failed | ignored |
|---|---|---|---|
| `platform-core` | 153 | 0 | 0 |
| `platform-db` | 46 | 0 | 0 |
| `platform-iam` | 180 | 0 | 0 |
| `platform-org` | 93 | 0 | 0 |
| `platform-rbac` | 83 | 0 | 0 |
| `platform-config` | 89 | 0 | 0 |
| `platform-contracts` | **32** | 0 | 0 |
| `platform-schema` | 20 | 0 | 0 |
| `platform-server` | 129 | 0 | 0 |
| `platform-audit` | 54 | 0 | 0 |
| `xtask` | 51 | 0 | 0 |
| `sandbox` | 33 | 0 | 0 |

> 统计口径：`grep -E '^test result' | awk '{p+=$4; f+=$6; i+=$8}'`
> **⚠️ 这里是 12 个 crate，不是 11 个** —— 整个 REQ-014 周期里所有轮次都只报了 11 个，
> **漏了 `platform-contracts`**（它有 `tests/c6_schema_projection.rs`，
> 而 **C6 正是错误响应契约** —— REQ-014 恰恰改过错误响应）。
> BA 终验时补跑：**32 passed / 0 failed** → 无回归，但「11 crate」这个口径本身是错的。
> 正确口径：workspace **13** 个成员（`platform-contracts-macros` 是 proc-macro，**无测试**）
> → **12 个有测试的 crate**。

前端：`@base/shell` **206** / `@base/i18n` **43** / `@base/sdk` **38**；
`pnpm -r build` 通过；`svelte-check` **0 error 0 warning**。
E2E：**35 passed / 0 failed / 0 skipped**（**干净库**，`tenant-a` 起点 11 条 < 默认 limit 20）。
出口标准：clippy **新增 0**（基线 2 条存量）；fmt **新增 0**（基线 11 个存量文件 —— TD-017）；
`cargo xtask capabilities --check` 14 文件一致；`aw verify` **0 FAIL / 0 WARN**。

## BA 终验记录（2026-11-20 18:00~18:10）

| 项 | 做法 | 结果 |
|---|---|---|
| F1 变异 | `query.rs` 的 `limit + 1` → `limit` | **3 条新断言变红**（`has_more_pages_iff_the_total_exceeds_the_limit` / `the_total_used_by_the_cursor_check_is_the_filtered_total` / `two_consecutive_pages_share_no_ids`）；还原后 `sha256sum -c` **成功**、`BA_MUTATION` **0** |
| F2 变异 | 去掉 `render_filters` 的 ` AND ` 前缀 | **2 条断言变红**，报错**逐字** `trailing junk after parameter at or near "$1actor_id"`（与 Pre-merge 报告 B-02 原始证据一字不差）；还原后 `sha256sum -c` **成功** |
| 改动范围 | `git status --short` | **恰好 17 个文件**（13 改 + 4 新），与预期集合**完全一致**，无越界 |
| 回归 | 12 crate 串行 | 见上表 |
| E2E | 干净库 | 35 / 0 / 0 |

## 规格摘要

- **需求来源**：用户原话「对，审计和国际化也是需要的。」（见 [decisions.md](decisions.md) §0）
- **验收标准**：13 条（[acceptance.md](acceptance.md) rev 3）
- **设计依据**：[design-summary.md](design-summary.md)（含全部 `file:line` 实测证据）
- **决策**：D130~D195（[decisions.md](decisions.md) rev 5）

## 验收标准达成情况

| AC | 判据落地方式 | 状态 |
|---|---|---|
| AC-S-022 | 平台层审计能力（父 AC） | ✅ |
| AC-S-122 | 审计表 13 列（契约 C12「不增不减」） | ✅ 真库实测 |
| AC-S-123 | append-only（`REVOKE`）+ 部署脚本不得破功 | ✅ **重跑 `deploy/seed/02-app-role.sql` 后实测仍拒 UPDATE/DELETE** |
| AC-S-124 | 哈希链（分链 / 规范化 / 接续） | ✅ 独立实现复算 + 5 处变异 |
| AC-S-125 | 敏感键硬拒绝（14 键基线 + 驼峰拆词） | ✅ 两侧都测 + 一致性测试 |
| AC-S-126 | 链校验的两个 reason 都有真实触发路径 | ✅ HTTP 实测 + 变异 |
| AC-S-127 | 登录审计（成功/失败/ip/UA/不含账号名） | ✅（SSO 分支见下） |
| AC-S-128 | 权限变更审计（授予/撤销/创建/删除各 1 条） | ✅ D163 形状逐字符合 |
| AC-S-129 | 事件真的进了审计表 | ✅ org / config / rbac / iam 均有真库判据 |
| AC-S-130 | 查询接口口径（游标 / 过滤 / 排序 / 400） | ✅ **B-01 / B-02 已修 + 库级断言 + 变异自证** |
| AC-S-131 | 归档（单租户 / 幂等 / 归档表只读） | ✅ 跨租户调用被拒 |
| AC-S-132 | 前端审计页（游标翻页 / 过滤 / 403） | ✅ **干净库 E2E 全绿 + 负向对照** |
| AC-S-133 | RLS 隔离 | ✅ **单事务内** `set_config` 实测（非空对照） |

## 遗留与未验证（如实列出）

| # | 事项 | 状态 |
|---|---|---|
| 1 | SSO 登录审计只到 **mock sink** 层，真库落表无判据 | **TD-040** |
| 2 | `config.flag.write` 的 `rollout_percent` 派生键分支，`after` 的双键形状无判据 | 已知未验（B3 只覆盖 `enabled`） |
| 3 | `platform_audit_chain_state` 的跨租户 RLS 行为 | 已知未验（风险低） |
| 4 | 5 个新错误码的中文文案**语义**未人工校对 | 存在性已断言；语义待人工 |
| 5 | `scripts/e2e-server.sh` **不能从零建库** | **TD-039**（本轮靠手工补 3 步） |
| 6 | 真实多实例部署下的缓存收敛 | 本机单实例，未验 |

## 阻塞项

**无。**

## 收尾（2026-11-20 18:30）

1. ✅ BA 提交修复轮 1 + 2 + 微修 → `feature/REQ-014` @ `c8dab33`
2. ✅ 合并 `feature/REQ-014` → `develop` @ **`310e4d3`**（85 文件 / +14162 / -264）
3. ✅ 回收 `feature-REQ-014` worktree（分支保留，与 REQ-011/012/013/021 一致）
4. ✅ 复盘：[req014-retrospective.md](../../feature-meta/req014-retrospective.md)
5. ✅ roadmap 同步 + dev-guide §9.8 / §9.9 / §9.13 写入本轮三条新规矩
6. ✅ 技术债 35 → **40 条**（TD-036~TD-040）
