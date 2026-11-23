# 用例：目录边界、代码量与依赖面

> 覆盖 **AC-S-186 / AC-S-187 / AC-S-193**
> 用例编号：TC-020-001 ~ TC-020-011（11 条）
> **rev 2**：BASE 拆分（B2）、口径改全仓（B6）、加 `check-modules`（N8）

---

## TC-020-001 · AC-S-186 · 全仓改动面与清单逐项相等

- **层次**: 静态（git）
- **步骤**:
  ```bash
  # ① 实际改动面（rev 2：不再只量 crates/**，见 B6）
  git diff --name-only BASE_IMPL | grep -v '^modules/leave/' | sort > /tmp/actual.txt
  # ② 从 design-summary.md §3 提取清单（逐项写成一行）
  sort /tmp/declared.txt > /tmp/declared.sorted.txt
  # ③ 比对
  diff /tmp/actual.txt /tmp/declared.sorted.txt
  ```
- **期望**: 步骤 ③ **无差异**。
- **判据形态**: `diff` 退出码 0。
- ⚠️ **`BASE_IMPL` 必须是「实现开始前」的 develop commit**，且要写进交付摘要。
  写错 BASE 会让这条永远通过。
- ⚠️ rev 1 只量 `crates/**`，漏掉根 `Cargo.toml`、`capabilities/**`、`apps/`、`packages/`
  → 那是一条**无上限的逃生通道**（B6 实测）。

---

## TC-020-002 · AC-S-186 · A 类改动为 0

- **层次**: 静态（清单审视）
- **步骤**: 逐条审视 `/tmp/declared.txt` 的每一项，判定它属于哪一类（D353）。
- **期望**: **A 类条目数 == 0**。
- **判据形态**: 计数。
- **判定方法**（A 类的定义）：
  问「平台有没有既有机制可用？」
  - 有 → 该项**本不该改** → A 类 → 判红；
  - 没有 → 属于 C/D 类（真缺口）→ 允许。
- ⚠️ 这条防的是「随便改平台，然后说『这是必需的』」。

---

## TC-020-003 · AC-S-186 · 每个 B/C/D 类都有归因

- **层次**: 静态（文档审查）
- **步骤**: 检查 `design-summary.md` §3 表格的「为什么必需」列。
- **期望**: 每行都有 **「删掉它，哪条 AC 就判不了」** 的具体答案。
- **判据形态**: 逐行检查，无空/无套话。
- ⚠️ 「为了跑起来」不算归因；「不加则 `/api/leave/*` 全 404」才算。

---

## TC-020-004 · AC-S-186 · 生成物已重新生成

- **层次**: 静态（xtask）
- **步骤**:
  ```bash
  cargo xtask check-modules      # rev 2 新增（N8：模块目录规范）
  cargo xtask check-deps
  cargo xtask capabilities --check
  ```
- **期望**: 三条退出码均为 **0**。
- **判据形态**: 进程退出码。
- ⚠️ `capabilities --check` 会**强制**产生 `capabilities/permissions.json` 的新内容
  （因为新增了 `modules/leave/manifest.json` 的权限点）→ 必须跑
  `cargo xtask capabilities` 重新生成。这个文件在**仓库根**，rev 1 的口径看不见它（B6）。

---

## TC-020-005 · AC-S-187 · 后端业务代码行数

- **层次**: 静态（wc）
- **步骤**:
  ```bash
  find modules/leave/backend/src -name '*.rs' | xargs wc -l | tail -1
  ```
- **期望**: 输出实际值（整数）。
- **判据形态**: 数值（与 TC-020-006 相加后 < 500）。
- ⚠️ **必须贴原始输出**，不能只写「约 XXX 行」。

---

## TC-020-006 · AC-S-187 · 前端业务代码行数

- **层次**: 静态（wc）
- **步骤**:
  ```bash
  find modules/leave/frontend -type f \( -name '*.svelte' -o -name '*.ts' \) | xargs wc -l | tail -1
  ```
- **期望**: 输出实际值。
- **判据形态**: 数值（与 TC-020-005 相加 < 500）。

---

## TC-020-007 · AC-S-187 · SQL 迁移单独统计

- **层次**: 静态（wc）
- **步骤**:
  ```bash
  find modules/leave/migrations -name '*.sql' | xargs wc -l | tail -1
  ```
- **期望**: 输出实际值，**不计入** 500 行合计。
- **判据形态**: 数值被单独列出，未被并入合计。
- ⚠️ 对照类：防「把 SQL 算进业务代码，让数字好看或难看」。

---

## TC-020-008 · AC-S-187 · **骨架已占 458 行的预先披露**

- **层次**: 静态（文档审查）
- **步骤**: 检查 `acceptance.md` AC-S-187 与 `design-summary.md` §7 是否**预先**写明了
  骨架占用（411 后端 + 47 前端 = 458）。
- **期望**: 两处都有该数据 + 取数方式。
- **判据形态**: 文档中存在该表。
- ⚠️ **这条不是验实现，而是验「规格没有隐瞒结构性风险」**。
  若规格没写，实现后再说「预算不够」就是事后找借口。

---

## TC-020-009 · AC-S-187 · **超标时必须如实归因**

- **层次**: 静态（判定）
- **步骤**: 若 TC-020-005 + 006 ≥ 500 → 检查度量报告的归因表。
- **期望**:
  - 合计 < 500 → AC 达成，本条不适用；
  - 合计 ≥ 500 → **AC 判不成立**，但报告必须含「超 N 行 + 逐文件归因（必需样板 vs 业务逻辑）」。
- **判据形态**: 报告里存在归因表（或合计 < 500）。
- ⚠️ **阴性对照**：若报告出现「口径调整为仅后端」之类措辞 → 直接判不成立（D357）。

---

## TC-020-010 · AC-S-193 · 依赖面只有平台公开面

- **层次**: 静态（grep manifest）
- **步骤**:
  ```bash
  grep -E '^platform-' modules/leave/Cargo.toml
  ```
- **期望**: 只出现 `platform-core`、`platform-db`、`platform-contracts`（dev）。
- **判据形态**: **不含** `platform-server` / `platform-iam` / `platform-notify` /
  `platform-rbac` / `platform-observability` / `platform-org`。
- ⚠️ 业务要发通知时**不得**依赖 `platform-notify`，必须用
  `NotificationSender` trait 注入（D354）。

---

## TC-020-011 · AC-S-186 · **对照：扫描器确实会报违规**

- **层次**: 变异（临时）
- **步骤**:
  1. 临时在 `crates/platform-core/src/lib.rs` 末尾加一行注释；
  2. 跑 TC-020-001 的比对；
  3. **还原**并 `touch`（§9.4：不改 mtime 会复用陈旧判定）。
- **期望**: 步骤 2 **红**（`actual` 比 `declared` 多一个文件）。
- **判据形态**: 变异后 diff 非空。
- ⚠️ **阳性对照（§9.2）**：没有它，「零改动」可能是扫描器根本没跑。
