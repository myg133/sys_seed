# 用例：度量可复算与端到端

> 覆盖 **AC-S-191 / AC-S-192**
> 用例编号：TC-020-034 ~ TC-020-038（5 条）
> **rev 2**：度量命令写死（M9）、E2E 范围写死（M10）、前端装配缺口（M7）

---

## TC-020-034 · AC-S-191 · 报告里每个数字都有**可执行命令**

- **层次**: 文档审查
- **步骤**:
  1. 打开 `metrics-report.md`；
  2. 逐行检查「值」与「取数命令」两列；
  3. **实际执行**每一条命令，确认它能跑。
- **期望**: 每行都有非空命令，且命令**可直接复制执行**（不是「输出模式」）。
- **判据形态**: 逐行检查 + 抽 3 条实跑。
- ⚠️ **rev 2 修正 M9**：rev 1 的「测试通过数」写的是
  「全量输出的 `test result: ok. N passed`」—— 那是**输出模式**，不是命令；
  且没说要不要 `--include-ignored` → 同一 commit 能报出两个 N。
  rev 2 在 acceptance AC-S-191 里给了**完整的逐 crate 循环**（必带 `--include-ignored`）。

---

## TC-020-035 · AC-S-191 · 换人重跑得同一结果

- **层次**: 静态（可复现性）
- **步骤**:
  1. 记录被测 commit sha；
  2. 按报告命令**逐条重跑**；
  3. 逐项比对数值。
- **期望**: 同一 commit 上**每个数字逐字相同**。
- **判据形态**: 逐项字符串相等。
- ⚠️ 若某条命令依赖「当前时间」「随机数」「未提交文件」→ 不可复现，必须删改。

---

## TC-020-036 · AC-S-191 · **对照：不得纳入不可复现指标**

- **层次**: 文档审查（阴性对照）
- **步骤**:
  ```bash
  grep -niE "上手时间|一次过|AI 通过率|学习曲线" \
    BA/demands/REQ-020-platform-consumption/metrics-report.md
  ```
- **期望**: **0 命中**（或在明确标注「附录（不作判据）」的章节里）。
- **判据形态**: grep 计数；若命中，检查是否在附录且标注「不作判据」。
- ⚠️ D350 的落地判据：这些指标与 AC-S-028「换人复测结论一致」直接冲突。

---

## TC-020-037 · AC-S-192 · E2E 全流程（**范围写死**）

- **层次**: E2E（浏览器）
- **步骤**:
  ```bash
  npx playwright test e2e/leave.spec.ts --reporter=json
  ```
  1. 打开请假列表页 → 渲染出至少 1 行（夹具）；
  2. 点「新建」→ 填表 → 提交 → 列表出现新行；
  3. 切审批人账号 → 点「通过」→ 该行状态变为「已通过」。
- **期望**: 三步全通过。
- **判据形态**: Playwright 断言（`expect(locator).toBeVisible()` 等）。
- ⚠️ **rev 2 修正 M10**：rev 1 只说「读 E2E 报告的 skipped 数」，没说范围。
  实测现有 suite 里就有条件 skip（`e2e/audit.spec.ts:157,186,225,254,298`、
  `e2e/schema-driven.spec.ts:137`）→ 跑全量 vs 只跑本模块，**数不同**
  → 「换个执行范围就能从红变绿」。
  → rev 2 写死成**单文件** `e2e/leave.spec.ts`。
- ⚠️ 步骤 2 必须**真的填表并提交**，不能直接调 API 造数 —— 那验不到「表单能用」。

---

## TC-020-038 · AC-S-192 · 刷新后状态保持 + 本文件零 skip

- **层次**: E2E（浏览器）
- **步骤**:
  1. 承接 TC-020-037 的最终状态；
  2. `page.reload()`；
  3. 断言状态仍是「已通过」；
  4. 取 `e2e/leave.spec.ts` 的 skipped 数。
- **期望**:
  - 步骤 3：状态保持（证明不是前端内存态）；
  - 步骤 4：**`skipped == 0`**（仅该文件）。
- **判据形态**: 刷新后断言 + JSON 报告的 skipped 计数。
- ⚠️ **取数命令**（rev 2 补，M9：rev 1 只说「读报告」没给方法）：
  ```bash
  npx playwright test e2e/leave.spec.ts --reporter=json \
    | jq '[.. | .skipped? // empty | select(. != 0)] | length'
  # 期望输出 0
  ```
- ⚠️ **两个判据都必须有**：
  - 只测「提交后即时渲染」→ 验不到是否真的落库；
  - 只看 skipped → 验不到状态是否持久。

---

## 补充：前端装配的前置（M7 / D356）

⚠️ rev 2 明确指出：**「前端 shell 已装配模块页面」这个前置条件不存在**。

实测：
- `apps/shell/src/lib/routes.ts:47`：「`/schema/:moduleId/:entityKey` 是**唯一**的
  数据页面路由：新增实体只改后端 manifest，`apps/shell/src/**` 一行不改」；
- 通配页动作列只有编辑/删除（`SchemaTable.svelte:131-148`）；
- manifest 的 `api` 只有 4 个键（`platform-core/src/extension/schema.rs:138-141`）
  → **无「审批」行内动作**；
- `grep -rn "modules/" apps/shell/src` → **无命中**。

→ 本文件的两条 E2E 用例前置依赖：
1. `modules/leave/frontend/pages/LeavePage.svelte` 已写；
2. `apps/shell/src/**` 加了一条硬编码路由（**C 类改动**，登记）。

**禁止**：用通配表单直接改 `status` 字段 —— 那绕过状态机与 409 校验
（AC-S-189 的安全判据被 UI 旁路）。
