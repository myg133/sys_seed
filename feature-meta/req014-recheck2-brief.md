# REQ-014 修复轮 **第 2 轮聚焦复核**任务卡（QA 子 Agent）

- **需求**: REQ-014 审计日志
- **被验对象**: `/home/myg133/Projects/system_seed/feature-REQ-014`（分支 `feature/REQ-014`）
  ⚠️ **修复轮 1 + 2 都未提交** —— HEAD 仍是 `5f28c0c`，改动**在工作区**。
- **验收基线**: `BA/demands/REQ-014-audit/acceptance.md` rev 3
- **决策基线**: `BA/demands/REQ-014-audit/decisions.md` rev 5（D130~D195）
- **上轮复核报告**: `BA/feature-meta/qa-recheck-req014.md`（FAIL：0 阻塞 / 1 高 / 2 中 / 3 低）
- **第 2 轮报告**: `BA/feature-meta/req-014-dev-fixround2.md`
- **第 2 轮任务卡**: `BA/feature-meta/req014-fixround2-brief.md`
- **本卡由 BA（myg133-ba-1）于 2026-11-20 16:30 写下**

---

## 0. 你的立场与任务边界

**独立复核者**。不采信 Dev 的自述 —— 上一轮复核已经证伪过 Dev 的数字表
（Dev 报 925，独立复核实测 927）。

**本轮是**「聚焦复核」**——只查第 2 轮做的 4 件事（F11~F14）+ 它们有没有带来回归**。
不是从头再复核一遍全部 13 条 AC（上轮已做过，且结论是 0 阻塞）。

---

## 1. 必读

1. `BA/feature-meta/req-014-dev-fixround2.md`（第 2 轮 Dev 自述 —— **待验证**）
2. `BA/feature-meta/req014-fixround2-brief.md`（第 2 轮任务卡）
3. `BA/feature-meta/qa-recheck-req014.md`（上轮复核 —— 你的对照基线）
4. `BA/demands/REQ-014-audit/decisions.md` rev 5（**只读** D194 / D195 与相关条目）

---

## 2. 环境（铁律）

```bash
export XDG_RUNTIME_DIR=/run/user/$(id -u)
export CARGO_TARGET_DIR=/home/myg133/Projects/system_seed/code/target
export DATABASE_URL_TEST="postgres://base:base@127.0.0.1:5432/base_test"
```

- `cargo test --workspace` 在本仓**必失败** → **逐 crate 跑**（11 个）；`--all-targets` 不能省；
  **只能有一个** `--`；`--include-ignored` 在 `--` **之后**；**串行**。
- 统计口径（**自己算**）：
  ```bash
  cargo test -p <crate> --all-targets -- --test-threads=1 --include-ignored \
    | grep -E '^test result' | awk '{p+=$4; f+=$6; i+=$8} END {printf "passed=%d failed=%d ignored=%d\n", p, f, i}'
  ```
- 应用连接用 **`base_app`**；本机**无 psql 客户端** → `podman exec base-postgres psql -U base -d base_test -tAc "…"`。
- 低配机（8 线程 / 7.7G）→ 串行，别并发。
- 还原变异记得 `touch`（`cp -p` 保留旧 mtime → cargo 复用陈旧二进制）；未跟踪文件 `git checkout` 还原不了。

---

## 3. 🔴 必查 A：F11 —— E2E **在干净库上**是否真的全绿

这是本轮**最关键**的一条。第 1 轮 Dev 报「35 passed / 0 failed」是在**脏库**上得的，
第 2 轮才补了负向对照。**你要自己从零重建库，再跑一遍。**

### A.1 「干净库」怎么来

第 2 轮 Dev 的做法（细节见 `req-014-dev-fixround2.md`）：

```bash
# 1) 整库重建
podman exec base-postgres psql -U base -d base_test -tAc "DROP SCHEMA public CASCADE;"
# 2) 平台迁移
#    DATABASE_URL=<admin> cargo run -q -p platform-server --bin base -- migrate up
# 3) 应用角色授权（scripts/e2e-server.sh **不**跑这步 —— 见 TD-039）
#    podman exec -i base-postgres psql -U base -d base_test < deploy/seed/02-app-role.sql
# 4) sandbox 模块迁移（scripts/e2e-server.sh **不**跑这步 —— 见 TD-039）
#    modules/sandbox/migrations/*.up.sql
# 5) 租户策略
#    cargo run -q -p platform-server --bin base -- tenant-policies
# 6) E2E 夹具
#    DATABASE_URL_TEST=<admin> ./scripts/seed-e2e.sh
```

⚠️ **你自己确认每一步都真的跑了** —— 第 2 轮 Dev 实测过：
只跑 `migrate up` 而不跑第 3 步 → `base serve` 以 `permission denied for table platform_permission` 退出。

### A.2 判据

1. 播种后 **tenant-a 的审计记录数 == 11**（`scripts/seed-e2e.sh` 的 `FOR v_index IN 1..11 LOOP`）
   → **11 < 默认 limit 20** → 这正是旧代码必然失败的场景；
   - 查法：`podman exec base-postgres psql -U base -d base_test -tAc "SELECT count(*) FROM platform_audit_log WHERE tenant_id='tenant-a';"`
2. `npx playwright test e2e/` → **全绿且 0 skipped**；
3. ⚠️ **审计页的 6 条用例必须逐条实跑**（不许 skip）；
   理由：skip 的理由**只允许**「被测系统未就绪」（D108）。

### A.3 负向对照（**必须做**）

把 `e2e/audit.spec.ts` 里第 2 轮补的那行 `selectOption('10')` **删掉**
→ 在**同一个干净库**上重跑 → 「回退」那条用例**必须失败**（按钮 disabled → 超时）
→ 还原 → `sha256sum -c` 通过 + `git status` 干净。

> 这一步证明「全绿」是**修复带来的**，不是环境恰好 favorable。

---

## 4. 🔴 必查 B：F12 —— 跨 crate 三处一致断言**不是死代码**

- B1：把 `crates/platform-audit/src/query.rs` 的
  `pub const DEFAULT_LIMIT: usize = 20;` 改成 **30**
  → `crates/platform-server/tests/audit_default_size_real_db.rs` 的新断言**必须红**
  → 还原 → `sha256sum -c` 通过 + `touch`。
- B2：把 `crates/platform-config/tests/config_real_db.rs` 里那条断言的
  **manifest 登记值**改成 21（`json!(20)` → `json!(21)`）
  → **必须红**（否则它对 manifest 侧是盲的）。
- B3：把 `platform_config` 表里 `audit.page.defaultSize` 的行值改成 21
  → **必须红**（否则它对 DB 侧是盲的）。
- B4：`crates/platform-config/tests/config_real_db.rs` 的**文档注释**是否已改成
  **描述代码实际做的事**（第 2 处是**契约口径常量**，不是 `platform_audit::DEFAULT_LIMIT`）
  → 注释里**不得**再出现「比对 `platform_audit::DEFAULT_LIMIT`（运行时默认）」这种
  与代码不符的说法。

---

## 5. 🟡 必查 C：F13 / F14

| # | 查什么 |
|---|---|
| C1 | **F13**：`BA/feature-meta/req-014-dev-fixround.md` 是否已订正 —— F10 状态改成「已落地」、`platform-server` 125 → **127**、合计 925 → **927**；**且**订正要**如实留痕**（标注「这不是当初就写对了」），**不得**抹掉原始结论。`req-014-dev-fixround-f10.md` 不应被改。 |
| C2 | **F14**：`crates/platform-iam/tests/sso_login_audit.rs` 存在；断言了**事件码** / `object_type` / `tenant_id` / `ip` / `user_agent`；⚠️ 确认它**驱动的是生产写入路径**（`IamService::complete_oidc_callback` 一侧），**不是** `PlatformSinkSsoRecorder`（桥接器**故意跳过**这两个登录码 —— `crates/platform-iam/src/sso/bridge.rs` 的模块注释写明了理由：D157 要「写失败即 503」，而 `record` 是**同步**的表达不了；且两边都写会产生**两条**记录 → 破 AC-S-127 的「1 条」）。 |
| C3 | **F14 的敏感度**：把成功分支的 `ip` 改成别的值 → 用例 1 必须红；把失败分支的 sso 标志改错 → 用例 2 必须红。（各还原 + sha256 校验。） |

---

## 6. 🟡 必查 D：回归与越界

| # | 查什么 |
|---|---|
| D1 | **逐 crate 全量**（11 个，串行）→ 全 **0 failed 0 ignored**；**自己用 awk 算**。Dev 报 **930**（core 153 / db 46 / iam **180** / org 93 / rbac 83 / config 89 / schema 20 / server **128** / audit 54 / sandbox 33 / xtask 51）。⚠️ 若**下降** → 有用例被误删，必须查清 |
| D2 | **前端**：`pnpm --filter @base/shell test` ≥ **206**；`@base/i18n` **43**；`@base/sdk` **38**；`pnpm -r build`；`svelte-check` 0 error |
| D3 | **clippy**：`cargo clippy --workspace --all-targets` → **新增 warning 为 0**（基线 2 条存量：`platform-iam/src/sso/config.rs:153`、`platform-iam/src/sso/oidc.rs:234`）；**fmt**：`cargo fmt --all --check` → 与基线 `develop@0683115` 比对**新增 0 处**（基线本来就红 11 个存量文件 —— TD-017） |
| D4 | **越界检查**：`git status --short` 的文件集合应当**恰好**是这 15 个：<br>`crates/platform-audit/src/query.rs`、`crates/platform-audit/tests/audit_chain.rs`、`crates/platform-audit/tests/query_real_db.rs`（新）、`crates/platform-config/manifest.json`、`crates/platform-config/src/service.rs`、`crates/platform-config/tests/audit_events.rs`、`crates/platform-config/tests/common/mod.rs`、`crates/platform-config/tests/config_real_db.rs`、`crates/platform-iam/src/service.rs`、`crates/platform-iam/tests/sso_login_audit.rs`（新）、`crates/platform-rbac/tests/audit_events.rs`、`crates/platform-server/src/audit.rs`、`crates/platform-server/tests/audit_chain_real_db.rs`（新）、`crates/platform-server/tests/audit_default_size_real_db.rs`（新）、`crates/platform-server/tests/i18n_titles.rs`、`e2e/audit.spec.ts`、`migrations/20261120000400_audit_config_seed.up.sql`。<br>**有超出这个集合的文件 → 报出来**（不必是错，但要知道） |
| D5 | `cargo xtask capabilities --check` → 一致；`aw verify` → 0 FAIL / 0 WARN |

---

## 7. 🟢 必查 E：本轮**已知**的两个遗留（**不用查，只需确认没被本轮改坏**）

- **TD-039**：`scripts/e2e-server.sh` **不能从零建库**（不跑 `02-app-role.sql`、
  不跑 `modules/sandbox/migrations/`、不跑 `tenant-policies`）→ 本轮**不修**，
  但你重建库时**必须**自己补这三步（见 §3.1）。
- **TD-040**：SSO 登录审计只到 **mock sink** 层，真库落表无判据 → 本轮**不修**。
  ⚠️ 顺带核实一条**事实纠正**：第 2 轮 Dev 说「加 `platform-audit` dev-dep 会**成环**」
  → BA 已核实**不成环**（`platform-iam` 只依赖 core+db；`platform-audit` 只依赖 core+db+contracts；
  `platform-contracts` 只依赖 core+macros → 两条线没有交点）。
  → 这条**纠正**已记入 TD-040，你只需确认**依赖图确实如 BA 所述**。

---

## 8. 交付物

**唯一允许写**的文件：`/home/myg133/Projects/system_seed/BA/feature-meta/qa-recheck2-req014.md`

必须包含：
1. **结论**：PASS / FAIL / PASS-WITH-RISK + 分档汇总（阻塞 / 高 / 中 / 低）；
2. **逐条问题**：可判定性分档 + **证据**（`file:line` 或命令输出）+ **建议改法**；
3. **你实测的逐 crate 数字**（自己跑、自己算）；
4. **变异验证 ≥3 处**（A.3 / B1~B3 / C3 里至少 3 处）的完整证据链；
5. **未验证项清单**（如实写，**不许**写「应该没问题」）；
6. 对第 2 轮 Dev 的 4 条偏离声明（D-1~D-4）的**逐条判定**（接受 / 拒绝 / 需 BA 裁决）。

---

## 9. 停止条件与权限

- **你是复核者不是修复者**：
  - ❌ **不许**修改 feature worktree 的任何生产代码 / 测试代码 / 迁移 / 配置来「让检查通过」；
  - ❌ **不许**放宽断言、改阈值、跳过用例；
  - ✅ 发现问题 → **写进报告**，附证据与建议改法，**由 BA 决定是否派修复轮**。
- ⚠️ **唯一例外**：本卡的**变异注入**允许临时改动文件，但**必须逐字节还原**
  （`sha256sum -c` 通过 + `git status --short` 干净 + `grep -c 'MUTATION'` 为 0）。
- ✅ **允许写**：`/home/myg133/Projects/system_seed/BA/feature-meta/qa-recheck2-req014.md`（**唯一**）。
- ❌ **不许 `git commit`**。
- 遇到无法判定的问题 → 写进报告，**不要**自己拍板改规格。
- ⚠️ 你要重建测试库 → 那是**允许**的（测试库是可弃资源），但**不要**动容器本身
  （`base-postgres` / `base-redis` 保持运行）。

---

## 10. 回报格式

1. 结论（PASS/FAIL）+ 分档汇总；
2. 阻塞级清单（逐条：AC 号 / 档位 / 证据 / 建议改法）；
3. 你实测的逐 crate 数字；
4. 变异验证 ≥3 处的证据；
5. 未验证项清单；
6. 第 2 轮 Dev 4 条偏离声明的逐条判定；
7. 已写入的文件（绝对路径）。

---

*本轮是合并前**最后一道**复核。请把「干净库 E2E 可复现」这件事钉死 —— 它是这一整轮返工的起点。*
