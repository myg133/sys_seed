# REQ-022 夜间流水线排期（2026-11-21 01:05 → 明早）

> **给用户的一句话**：你明早来看时，流程已自动跑到终点 —— 棒1 交付 → 终版全量 + E2E →
> 修复轮 → 独立复核 → BA 终验（22 条 AC 逐条判定）→ 合并 `develop` + 回收 worktree + 复盘。
> **只在需要你拍板规格问题时才停**（会列在这里）。

---

## 0. 排期总表

| 阶段 | 窗口（预计） | 谁做 | 做什么 | 产出 | 闸门（不过就不往下走） |
|---|---|---|---|---|---|
| **P0 棒1 交付** | 01:05 ~ 03:00 | 子 Agent `bg_78de9ae3` | 后端实现收尾 + 修 D259 两处 | `feature-meta/req-022-dev-backend.md` | 交付物齐全 + 我验 3 个契约点 |
| **P1 冻结与核对** | 03:00 ~ 03:20 | **BA 亲跑** | 核对改动集 = 预期文件集；报告里的数字与工作区对得上 | 本清单 §1 勾选 | 零越界改动 |
| **P2 终版全量** | 03:20 ~ 04:30 | **BA 亲跑** | 13 crate 逐个 + 前端三包 + `svelte-check` + `capabilities --check` + 真 C-5 | `req-022-final-verification.md`（数字自己 `awk` 算） | `failed == 0` 且 `ignored == 0` |
| **P3 E2E** | 04:30 ~ 05:10 | **BA 亲跑** | 清库 + 手工补 3 步（TD-039）→ `npx playwright test e2e/` | 同上 | **0 skipped**；「真的点了按钮」有断言 |
| **P4 22 条 AC 逐条判定** | 05:10 ~ 05:30 | **BA 亲跑** | 每条给「通过 / 不通过 / 判不了+理由」 | 同上 | 判不了的必须写清缺什么 |
| **P5 修复轮**（仅当 P2~P4 有红） | 05:30 ~ 06:30 | 子 Agent（worker） | 按失败项定点修 | `req-022-fix.md` | 修完**重跑**受影响的部分 |
| **P6 独立复核**（**强制**） | 06:30 ~ 07:10 | 子 Agent（**verifier**，非 BA） | 独立复核 P2~P5 的产物 | `req-022-independent-review.md` | 复核不通过 → 回 P5 |
| **P7 BA 终验** | 07:10 ~ 07:30 | **BA** | 汇总复核结论 + 22 条 AC 结论 | `req-022-final-verification.md` 定稿 | 全绿或有明确技术债编号 |
| **P8 合并与收尾** | 07:30 ~ 08:00 | **BA** | 合并 `develop` + `aw.py claim release --remove-worktree` + roadmap/tech-debt/sprint 同步 + 复盘 | `BA/sprint/req-022-retro.md` + 交付总结 | 合并后 `develop` 绿 |

> ⚠️ **全量只跑一次**（§9.14）：P2 的全量在**代码冻结后**跑。
> 若 P5 有修复，**只重跑被改动的 crate + 受影响的 E2E**，不重跑全量。
> 例外：若修复动到了 `platform-core` / `platform-iam` / `platform-server` / `platform-oauth`
> → **重跑这 4 个 crate + E2E**（跨栈影响），并把这次重跑记进报告。

---

## 1. P1 冻结核对清单（BA 亲跑）

- [ ] `git status --short` 的改动集 **==** 两张任务卡 §1.1 的并集（不多不少）
- [ ] 棒1 报告的「文件 → AC」表与工作区实际文件**逐字对得上**
- [ ] 棒1 报告的测试数字与 **P2 我自己跑的**对得上（口径差异要能解释）
- [ ] 棒2 报告的 279 / 45 与我重跑的一致
- [ ] 两份报告的「未完成项」都**已消化**（补完 或 有技术债编号）

---

## 2. P2 终版全量的硬规矩（dev-guide §9.14）

| 规矩 | 理由 |
|---|---|
| workspace 成员**现场数**（不手抄） | 13 个成员；`modules/sandbox` 的**包名**是 `sandbox`；`platform-contracts-macros` 是 proc-macro crate **也不许跳过** |
| **逐 crate** 跑，不跑 `--workspace` | workspace 级跑必失败 |
| `--all-targets` 不可省 / **只能一个 `--`** / `--include-ignored` 在 `--` 之后 / **串行** | 双 `--` → 0 passed 假绿；`--include-ignored` 不加 → 真实库用例被 skip |
| 数字**自己 `awk` 算**，不抄 Dev 报告 | 口径不同 |
| `ignored == 0` 也要查 | 还有 `#[ignore]` 没解开 → 查 |

```bash
grep -E '^test result' /tmp/req022-full.log \
  | awk '{p+=$4; f+=$6; i+=$8} END {printf "passed=%d failed=%d ignored=%d\n", p, f, i}'
```

---

## 3. P3 E2E 的硬规矩

- [ ] **清库 + 手工补 3 步**（TD-039）：
      ① `deploy/seed/02-app-role.sql` ② `modules/sandbox/migrations/*.up.sql` ③ `cargo xtask tenant-policies`
- [ ] `target/e2e-jwt-private.pem` 存在、**0600**、**不在 git 里**
- [ ] `BASE_JWT_ISSUER=https://e2e.base` ≠ `oauth.frontendBaseUrl=http://127.0.0.1:8080`（D222 的前提）
- [ ] **0 skipped**（skip 只允许「被测系统未就绪」；**夹具缺失必须 fail**）
- [ ] ⚠️ **「真的点了按钮」** —— 同意页允许/拒绝、设备码允许、授权撤销、登出 iframe，
      每条都要有「点了之后产生可观测后果」的断言。
      **只测后端接口不点按钮 → 判 fail**（前端点击链没人验过 → 这就是无 DOM 测试环境的兜底）

---

## 4. 需要用户拍板才会停的点

| # | 触发条件 | 我会怎么做 |
|---|---|---|
| 1 | 修复需要**改规格**（改 AC / 改决策 / 加第 10 个错误码） | **停下**，写进 `req-022-final-verification.md` 的「待用户裁定」段，明早处理 |
| 2 | 修复需要**新增第三方依赖** | **停下**（约束是零新增依赖） |
| 3 | 修复需要**改 REQ-011/013/014 已交付的行为** | **停下**（跨需求改动要显式声明，D109） |
| 4 | E2E 因为**环境**跑不起来（如缺 Playwright 浏览器） | 记录并跳过 E2E 段（**不**动前端/后端代码），明确标注「E2E 判不了」 |
| 5 | 22 条 AC 里有**判不了**的 | 写清缺什么，**不**猜测通过 |

> 其余情况（实现 bug、测试红、注释不一致、越界改动）**我自己修**，不打断你。

---

## 5. 明早你来看的入口

| 文件 | 内容 |
|---|---|
| `BA/feature-meta/req-022-delivery-summary.md` | **总览**：22 条 AC 结论表 + 全量数字 + E2E 结果 + 合并信息 + 遗留技术债 |
| `BA/feature-meta/req-022-final-verification.md` | 终版全量 + E2E 的**原始证据**（命令 + 输出） |
| `BA/feature-meta/req-022-independent-review.md` | 独立复核报告（**不是** BA 自己复核） |
| `BA/feature-meta/req-022-dev-backend.md` | 棒1 自验报告 |
| `BA/feature-meta/req-022-dev-frontend.md` | 棒2 自验报告 |
| `BA/sprint/req-022-retro.md` | 复盘 |
| `BA/plan/tech-debt.md` | 新增技术债（TD-045 起） |
