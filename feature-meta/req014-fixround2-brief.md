# REQ-014 修复轮 **第 2 轮**任务卡（Dev 子 Agent）

- **需求**: REQ-014 审计日志
- **工作目录**: `/home/myg133/Projects/system_seed/feature-REQ-014`（分支 `feature/REQ-014`）
- **触发**: 修复轮**独立复核**结论 = FAIL（**0 阻塞** / 1 高 / 2 中 / 3 低）
- **复核报告**: `/home/myg133/Projects/system_seed/BA/feature-meta/qa-recheck-req014.md`
- **裁决**: `BA/demands/REQ-014-audit/decisions.md` rev 5（D130~D195）
- **本卡由 BA（myg133-ba-1）于 2026-11-20 15:40 写下**

---

## 0. 需求来源披露（D122）

REQ-014 的需求来源是**用户原话**「对，审计和国际化也是需要的。」（与 REQ-021 同源），不是 BA 自补。

## 0.1 大前提：**上一轮的两条阻塞已经修好了，别动**

独立复核已**独立注入 6 处变异**确认：
- **B-01**（`nextCursor` 恒 null）→ **已修**（`LIMIT limit+1`），4 条断言能抓红；
- **B-02**（过滤拼出非法 SQL）→ **已修**（每段自带 ` AND `），7 条断言能抓红；
- 黑盒实测：`limit=2` 连翻 10 页取全 **19 条零重复零遗漏**、末页 `nextCursor` 为 null；
  5 个 op 的过滤**全部 200**（不再是 503）。

→ **本轮只做下面 4 件事。** 不要顺手改 F1~F10 的任何东西。

---

## 1. 环境（铁律）

```bash
export XDG_RUNTIME_DIR=/run/user/$(id -u)
export CARGO_TARGET_DIR=/home/myg133/Projects/system_seed/code/target
export DATABASE_URL_TEST="postgres://base:base@127.0.0.1:5432/base_test"
```

- `cargo test --workspace` 在本仓**必失败** → **逐 crate 跑**（11 个）。
- `--all-targets` 不能省；**只能有一个** `--`；`--include-ignored` 在 `--` **之后**；**串行**。
- 统计口径（**自己算**）：
  ```bash
  cargo test -p <crate> --all-targets -- --test-threads=1 --include-ignored \
    | grep -E '^test result' | awk '{p+=$4; f+=$6; i+=$8} END {printf "passed=%d failed=%d ignored=%d\n", p, f, i}'
  ```
- 应用连接用 **`base_app`**；本机**无 psql 客户端** → `podman exec base-postgres psql -U base -d base_test -tAc "…"`。
- **禁止 `cargo install`**；**禁止新增第三方依赖**（workspace 内的 crate 不算第三方，但要遵守分层约定）。
- **不要 `git commit`** —— Dev 不提交，由 BA 统一提交。
- ⚠️ 工作区**已有上一轮未提交的改动** —— **那是上一轮 Dev 的成果**，
  **不要回退、不要 `git checkout`、不要重写**。你只**新增/修改**下面 4 件事涉及的文件。

---

## 2. 本轮要做的 4 件事

### 🔴 F11 —— E2E 有条用例**在干净库上必然失败**（复核 H-01）**必须修**

- **现象**：`e2e/audit.spec.ts` 的 `TC-014-113（回退）` 用例
  **没有调页大小** → 用默认 `limit=20`；而 seed 夹具只播 **11 条**
  （`scripts/seed-e2e.sh` 的 `FOR v_index IN 1..11 LOOP`）
  → `nextCursor` 为 `null` → 「下一页」按钮 `disabled` → 点击超时 → 用例**失败**。
- **为什么上一轮报的是「35 passed / 0 failed」**：因为当时库是**脏的**
  （被先前几轮运行累积到 43 条）→ 按钮可用 → 通过。
  → **上一轮的数字是靠脏库得的，不可复现。**
- **对照**：同一个文件里，前一条用例（`TC-014-112`）**显式**写了
  ```ts
  // 页大小调到 10（默认 20 的话夹具记录数不够凑出两页）。
  await page.getByLabel('每页条数').selectOption('10');
  ```
  → **回退用例漏了这一行**，于是偷偷依赖「tenant-a 记录数 > 20」。
- **钉死的改法**：给 `TC-014-113（回退）` 补上**同样**的页大小设置
  （`selectOption('10')`）+ 一行注释说明为什么必须调。
  - ⚠️ **不要**改 seed 夹具的条数（11 是别的用例的契约，见 `TC-014-115/116` 的前置）。
  - ⚠️ **不要**改产品的默认 `limit`。
- **必须验证（关键）**：**在干净库上**跑 E2E → 必须**全绿**。
  - 干净库怎么来：仓内 `scripts/test-integration.sh` 用 `DROP SCHEMA public CASCADE` 重置；
    或把 `platform_audit_log` 里 tenant-a 的 `e2e_fixture` 记录清干净后重播 seed。
  - ⚠️ **「跑出来绿」但没确认库是干净的 = 没验**。报告里要写清楚**你怎么确认库是干净的**
    （比如跑之前 `select count(*) from platform_audit_log where tenant_id='tenant-a'` 的值）。

### 🟠 F12 —— F3 那条断言的**注释与代码不一致**，且对运行时漂移是盲的（复核 M-01）

- **现象**：`crates/platform-config/tests/config_real_db.rs` 的
  `the_audit_page_default_size_is_the_same_everywhere`
  **文档注释**写「三处来源：1. manifest 2. **`platform_audit::DEFAULT_LIMIT`（运行时默认）** 3. DB」，
  但**代码**第 2 处比的是**硬编码常量** `const CONTRACT_DEFAULT_LIMIT: i64 = 20;`
  —— **没有**引用 `platform_audit::DEFAULT_LIMIT`。
  → **注释与代码不一致**（比没有注释更坏）。
- **复核的实验（决定性）**：把 `platform_audit::query::DEFAULT_LIMIT` 从 20 改成 30 →
  **config 侧这条断言仍然是绿的** → 它对**运行时漂移是盲的**。
  （运行时值由 `platform-audit` 自己的单测 `the_defaults_are_the_contract_defaults` 钉住，
  但那条单测**不知道** manifest / DB 的登记值 —— 两条线各钉一半，**接不上**。）
- **钉死的改法**（两件都做）：
  1. **改文档注释**，让它描述代码**实际**做的事：
     「三处」的第 2 处是**契约 C7 的口径常量**，不是运行时常量；
     并写明「运行时默认 == 20」由 `platform-audit` 的单测钉住。
     （代码的注释块里已经有这个意思，但**上面那段「三处来源」列表写错了** → 改正它。）
  2. **补一条真正把三者接起来的跨 crate 断言**，
     放在 **`crates/platform-server/tests/`**（装配层）——
     `platform-server` **同时**依赖 `platform-audit` 与 `platform-config`，且有 `sqlx` dev-dep
     （已核实）→ 这是唯一能同时看到三处的地方：
     - `platform_audit::DEFAULT_LIMIT`（**运行时**默认）；
     - `platform_config::ConfigRegistry::from_manifest_root(&repo_root())` 里
       `audit.page.defaultSize` 的**登记值**；
     - `platform_config` 表里那一行的**实际值**。
     三者必须**逐字相等**。
     ⚠️ `platform-config` **不得**依赖 `platform-audit`（分层是刻意的）→ 断言放 platform-server。
     ⚠️ **不引新依赖**：`serde_json` / `sqlx` / `platform-audit` / `platform-config` 都已在。
- **必须自证**：把 `platform_audit::query::DEFAULT_LIMIT` 改成 30 → **新断言必须红**
  → 还原并确认逐字节相同。
  （这条是 F12 的全部价值所在 —— 证不出来就等于没做。）

### 🟡 F13 —— Dev 主报告的**结论与工作区矛盾**，数字也过时（复核 M-02 / L-01）

- **矛盾**：`BA/feature-meta/req-014-dev-fixround.md`（12:45 写）称
  F10 的 `crates/platform-server/tests/i18n_titles.rs`「**保持原样（未修改）**」、F10「未落地」；
  而 `req-014-dev-fixround-f10.md`（12:56 写，晚 11 分钟）称「**+214 行已落地**」。
  **工作区实测**：**已落地**（+214 行，且能抓红）→ **主报告的结论是过时的**。
- **数字过时**：主报告写总计 **925**（`platform-server` 125）；
  独立复核实测 **927**（`platform-server` **127**）→ 差 2 = F10 新增的两条用例。
- **钉死的改法**：**订正 `req-014-dev-fixround.md`**（这是你自己的报告）：
  - F10 的状态改成「已落地（+214 行 / 2 条新断言）」，并指向 `req-014-dev-fixround-f10.md`；
  - 总数 **925 → 927**、`platform-server` **125 → 127**；
  - ⚠️ **不要**改 `req-014-dev-fixround-f10.md`（那份是对的）。
  - ⚠️ 订正要**如实标注**「本报告的数字为订正后」，
    **不要**假装当初就写对了（报告是**证据链**的一部分，改了就要留痕）。

### 🟡 F14 —— SSO 桥接的审计事件码**零覆盖**（复核 L-02）

- **现象**：`iam.sso.login.succeeded` / `iam.sso.login.failed`
  **只出现在** `crates/platform-iam/src/sso/bridge.rs`（源码），
  **测试里一条都没有**（`grep -rl "iam.sso.login.succeeded" crates/*/tests/*.rs` → **0 命中**）。
  → AC-S-127 步骤 3（SSO 登录审计）**没有机械判据**。
- **对照**：本地登录的 `iam.login.succeeded` **有**覆盖
  （`crates/platform-iam/tests/audit_events.rs:229-262`）。
- **钉死的改法**：在 `crates/platform-iam/tests/audit_events.rs`（或同 crate 的新测试文件）
  补用例，驱动**生产的** SSO 桥接（`crates/platform-iam/src/sso/bridge.rs` 的
  `PlatformSinkSsoRecorder`），断言：
  - 事件码 = `iam.sso.login.succeeded` / `iam.sso.login.failed`（各 1 条）；
  - `ip` / `user_agent` **逐字**透传（AC-S-127 步骤 3 的口径）。
  - ⚠️ 用**生产的 recorder + mock sink**（仓内既有模式），
    **不要**自己造一个等价实现 —— 那是「验证生产代码的拷贝」。
  - ⚠️ **不需要**真的 OIDC/LDAP provider：这条路径不需要网络。
- **若发现这条路径确实无法在测试里驱动** → **不要伪造通过**，
  在报告里如实写「未验」并说明原因。

---

## 3. 硬约束

- ❌ **不许**改 F1~F10 的任何实现（`query.rs` 的 `LIMIT` / ` AND `、manifest、迁移、
  夹具、`TokenVerifyError`、i18n 两条新断言、两个新测试文件）。
- ❌ **不许**为了让测试变绿而放宽断言 / 改阈值 / 跳过用例。
- ❌ **不许**在 `BA/demands/`、`BA/plan/` 下改任何文件。
- ✅ **允许写**：
  1. feature worktree 里本轮 4 件事涉及的文件；
  2. `BA/feature-meta/req-014-dev-fixround.md`（**只订正 F13 那一处**）。

## 4. 必须跑的验证（报告里要有实际输出）

1. **逐 crate 全量**（11 个，串行）→ 全 **0 failed 0 ignored**；数字**自己用 awk 算**。
   ⚠️ 上一轮独立复核实测 **927**（core 153 / db 46 / iam 178 / org 93 / rbac 83 /
   config 89 / schema 20 / server **127** / audit 54 / sandbox 33 / xtask 51）——
   你加了新用例，总数应**上升**；若**下降** → 有用例被误删，必须查清。
2. **前端**：`pnpm --filter @base/shell test`（≥206）、`@base/i18n`（43）、`@base/sdk`（38）、
   `pnpm -r build`、`svelte-check`。
3. **E2E（干净库）**：`npx playwright test e2e/` → **全绿且 0 skipped**。
   ⚠️ 必须**先确认库是干净的**（见 F11）→ 报告里写明确认方式与清理前后的行数。
   ⚠️ 若环境实在无法清干净库 → 如实写「未在干净库验证」并说明，**不许**拿脏库的结果充数。
4. **F12 的自证**：`DEFAULT_LIMIT` 20 → 30 → **新断言必须红** → 还原 → sha256 逐字节相同。
   还原记得 `touch`（`cp -p` 保留旧 mtime → cargo 复用陈旧二进制）。
5. `cargo clippy --workspace --all-targets` → **新增 warning 为 0**（基线 2 条存量）；
   `cargo fmt --all --check` → 与基线 `develop@0683115` 比对**新增 0 处**
   （基线本来就红 11 个存量文件 —— TD-017；**只格式化你改过的文件**）。
6. `cargo xtask capabilities --check` → 一致。

---

## 5. 交付物

1. **代码改动**（feature worktree，**不提交**）。
2. **订正后的** `BA/feature-meta/req-014-dev-fixround.md`（只改 F13 那一处，如实留痕）。
3. **报告追加节**（在同一份报告里追加，或新写
   `BA/feature-meta/req-014-dev-fixround2.md` —— 二选一，**选后者更清晰**）：
   - F11~F14 逐条：改了什么、`file:line`、为什么；
   - 实测的逐 crate 数字 + 前端数字 + E2E 结果（**含「库怎么确认是干净的」**）；
   - F12 的自证证据链；
   - **偏离声明**（凡与本卡不一致处，明确列出并说明理由）；
   - **未完成/未验证项**（如实写，**不许**写「应该没问题」）。

## 6. 停止条件

- 某条修不动（环境限制 / 依赖前置）→ **不要伪造通过** → 写进「未完成项」并说明。
- 发现本卡某条裁决与源码事实**冲突** → **停下来**，在报告里指出冲突并给证据，
  **不要**自己另定方案。
- 发现超出 F11~F14 的新缺陷 → **写进报告**，**不要**顺手改。

## 7. 回报格式

1. F11~F14 逐条：改了什么 + `file:line`；
2. 实测数字（逐 crate + 前端 + E2E + 库干净性确认方式）；
3. F12 自证证据；
4. 偏离声明 / 未完成项；
5. 已写入的文件（绝对路径）。
