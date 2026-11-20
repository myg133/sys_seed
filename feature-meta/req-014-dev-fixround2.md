# REQ-014 修复轮**第 2 轮**报告（Dev）

- **工作目录**：`/home/myg133/Projects/system_seed/feature-REQ-014`（分支 `feature/REQ-014`，HEAD `5f28c0c`）
- **任务卡**：`BA/feature-meta/req014-fixround2-brief.md`（F11~F14）
- **裁决基线**：`BA/demands/REQ-014-audit/decisions.md` **rev 5**（D130~D195）
- **被修对象**：`BA/feature-meta/qa-recheck-req014.md`（FAIL：0 阻塞 / 1 高 / 2 中 / 3 低）
- **代码状态**：**未提交**（按任务卡；Dev 不提交）
- **日期**：2026-11-20
- **前序报告**：`req-014-dev-fixround.md`（主，**已按 F13 订正**）+ `req-014-dev-fixround-f10.md`

---

## 0. 结论摘要

| # | 问题 | 档位 | 本轮处置 | 结果 |
|---|---|---|---|---|
| **F11** | E2E「回退」用例在**干净库上必红** | 高 | 补 `selectOption('10')` + 注释；**干净库**上跑全量 E2E + **负向对照** | ✅ 完成 |
| **F12** | F3 断言注释与代码不一致，对运行时漂移是盲的 | 中 | 改注释（三处 → 两处 + 常量）+ 新增跨 crate 三处比对断言 | ✅ 完成（含自证） |
| **F13** | 主报告 F10 结论与工作区矛盾、数字过时 | 中 | 订正主报告（925→927 / 125→127 / F10 已落地），如实留痕 | ✅ 完成 |
| **F14** | SSO 登录事件码零测试覆盖 | 低 | 新增 2 条用例驱动**生产**写入路径（不是桥接器，见 §4 冲突说明） | ✅ 完成（含 2 处变异） |

> **偏离任务卡 1 处**：**F14 的落点与任务卡不同** —— 任务卡说这些事件码由
> `PlatformSinkSsoRecorder` 桥接，实际**不是**（桥接器明确跳过它们）。
> 我按源码事实驱动了真正的写入路径，并保留了任务卡的原意（生产 recorder + mock sink）。
> 详见 §4.1，**这是任务卡与源码的事实冲突，已按任务卡 §6 停下并在报告中指出**。

---

## 1. 环境与验证口径

```bash
export XDG_RUNTIME_DIR=/run/user/$(id -u)
export CARGO_TARGET_DIR=/home/myg133/Projects/system_seed/code/target
export DATABASE_URL_TEST="postgres://base:base@127.0.0.1:5432/base_test"
```

- 逐 crate **串行**跑 11 个，`--all-targets`，**只有一个** `--`，
  `--include-ignored` 在 `--` **之后**，`-- --test-threads=1`；
- 数字用任务卡给的 `awk` 口径**自己算**（不引用任何旧报告）；
- 数据库读写一律经 `podman exec base-postgres psql -U base -d base_test`
  （本机无 `psql` 客户端）。

---

## 2. F11 —— E2E「回退」用例在干净库上必红（复核 H-01）

### 改了什么

`e2e/audit.spec.ts:228-237`（用例从 `:221` 开始）

```ts
  // 页大小调到 10（默认 20 的话夹具记录数不够凑出两页）。
  // ⚠️ 不许省这一行：夹具只播 11 条（`scripts/seed-e2e.sh` 的 `FOR v_index IN 1..11`），
  // 默认 `limit=20` 时 `total(11) < limit(20)` → `nextCursor == null`
  // →「下一页」按钮 `disabled` → 点击超时。省掉它就会偷偷依赖「历史运行把库喂到 > 20 条」，
  // 干净库（`DROP SCHEMA public CASCADE` 后重播 seed）上**必红**。
  await page.getByLabel('每页条数').selectOption('10');   // ← :235
  await expect(page.getByTestId('audit-pager')).toBeVisible();
```

与同文件 `:192`（`TC-014-113`）的写法**逐字对齐**。

**没有**改 seed 夹具条数（11 是 `TC-014-115/116` 的前置），**没有**改产品默认 `limit`。

### 验证（**关键：如何在干净库上验**）

**库怎么确认是干净的** —— 不是「我删了几行」，而是**整库重建**：

```bash
DROP SCHEMA IF EXISTS public CASCADE;   # 审计表直接不存在了
CREATE SCHEMA public; + deploy/seed/00-init.sql
```

重建后按 `scripts/test-integration.sh` 的口径补齐（这一步**任务卡没写、我遇到的坑**，见 §6）：

```
[1/6] DROP SCHEMA public CASCADE + 基线 DDL      → platform_audit_log 不存在
[2/6] 平台迁移（base migrate up）                 → 16 个版本
[3/6] sandbox 模块迁移（modules/sandbox/migrations/*.up.sql）
[4/6] 应用角色（deploy/seed/02-app-role.sql）
[5/6] RLS 策略（cargo xtask tenant-policies）+ 授权
[6/6] 播种夹具（./scripts/seed-e2e.sh）
```

**实测行数（这就是「干净」的证据）**：

| 时点 | `all_audit_rows` | `tenant-a_total` | `tenant-a_fixture` |
|---|---|---|---|
| 重置后（迁移前） | 表不存在 | — | — |
| 迁移后、播种前 | **0** | **0** | **0** |
| **播种后（E2E 起点）** | **11** | **11** | **11** |
| E2E 跑完 | — | 32 | 11（登录事件写入，属正常） |

→ **起点是 `tenant-a = 11 < 默认 limit 20`**，正是复核 H-01 描述的必红条件。

### 负向对照（证明「绿」是这行修复带来的，不是脏库假绿）

把 `:235` 那行**临时删掉**（其余不动），在**同一个干净库**（跑前 `tenant-a_total=11`）跑：

```
✓ 1 TC-014-118 / 119  无权限 → 403 页，请求数 0
✓ 2 TC-014-111 / 112  列出记录
✓ 3 TC-014-113 / 114  翻页 → 带 cursor，且不重复
✘ 4 TC-014-113（回退） 「上一页」→ 回到第 1 页            ← 30.1s 超时
✓ 5 TC-014-115 / 117  按操作人过滤
✓ 6 TC-014-116        按动作过滤
1 failed, 5 passed

Error: locator.click: Test timeout of 30000ms exceeded.
  - locator resolved to <button disabled ... data-testid="audit-next-page">下一页</button>
```

**逐字复现了复核 H-01 的现象**。随后还原：

```
sha256sum e2e/audit.spec.ts
edc03d6c4a020291b263b1bd0b8e9349b11db20ef0d9b6f8935f8c48d198ba86   ← 与变异前相同
```

### 正向验证（干净库 + 修复版）

再次整库重建 → 起点 `tenant-a_total=11` → 全量 E2E：

```
35 passed (42.5s)
```

**0 failed / 0 skipped**，且 `audit.spec.ts` 6 条全绿（含第 4 条「回退」）。

> **F11 结论**：红 → 绿由**这一行**决定；两种数据状态下结果一致（都绿），
> 不再「只在脏库上绿」。

---

## 3. F12 —— F3 断言注释与代码不一致 + 对运行时漂移是盲的（复核 M-01）

### 3.1 改注释（`crates/platform-config/tests/config_real_db.rs:1810-1832`）

原文宣称「**三处**来源」，其中第 2 处写成
「`platform_audit::DEFAULT_LIMIT`（运行时默认）」—— 但代码比的是硬编码常量。
**改成如实描述**：

- 标题：「`audit.page.defaultSize` 的**登记值**必须与契约 C7 的口径一致」；
- 新增 `# 本用例实际覆盖的**两处**（不是三处）` 段，明写第 2 处是
  **契约 C7 的口径常量**（`CONTRACT_DEFAULT_LIMIT`），**不是**运行时常量；
- 新增 `# 运行时默认由谁钉住` 段，指向 `platform-audit` 的
  `the_defaults_are_the_contract_defaults`，并明写「两条线各自只钉一半」；
- 新增跨引用，指向新增的装配层断言文件。

行内断言消息也从「与契约 C7 的**运行时**默认必须相等」改为「与契约 C7 的**口径**（20）必须相等」
（原文这句话本身就是错的表述）。

### 3.2 新增跨 crate 三处比对（`crates/platform-server/tests/audit_default_size_real_db.rs`，新文件）

`crates/platform-server/tests/audit_default_size_real_db.rs:87`
`the_audit_page_default_size_is_identical_in_runtime_manifest_and_db`

| # | 来源 | 怎么取 |
|---|---|---|
| 1 | **运行时**默认 | `platform_audit::DEFAULT_LIMIT`（**引用常量**，不抄 20 —— 这正是本文件的意义） |
| 2 | **manifest** 登记值 | `platform_config::ConfigRegistry::from_manifest_root(&repo_root())` → `get("audit.page.defaultSize").default` |
| 3 | **DB** 实际值 | `SELECT value FROM platform_config WHERE key = $1` |

三者**逐字相等**（三对 `assert_eq!`，失败消息标明是哪两处不一致）。

**为什么放 `platform-server`**：它是唯一同时依赖 `platform-audit` 与 `platform-config` 的 crate；
`platform-config` **不得**依赖 `platform-audit`（分层刻意）。**未新增任何依赖**
（`serde_json` / `sqlx` / 两个 crate 都已在 `Cargo.toml` 里）。

### 3.3 自证（F12 的全部价值）

| 步骤 | 操作 | 结果 |
|---|---|---|
| 1 | 备份 `query.rs` | `sha256 = fd4dc6122c93ef43…89c83` |
| 2 | `DEFAULT_LIMIT: usize = 20` → **`30`** | `:51` 确认已改 |
| 3 | 跑新断言 | **FAILED**（见下） |
| 4 | `cp` 备份还原 + **`touch`**（防 cargo 复用陈旧二进制） | `sha256 = fd4dc6122c93ef43…89c83` **逐字节相同** |
| 5 | 复跑 | **ok**（1 passed） |

变异后的失败原文：

```
test the_audit_page_default_size_is_identical_in_runtime_manifest_and_db ... FAILED

panicked at crates/platform-server/tests/audit_default_size_real_db.rs:122:5:
assertion `left == right` failed: manifest 登记的 audit.page.defaultSize（20）与
**运行时**默认 `platform_audit::DEFAULT_LIMIT`（30）不一致 —— 运维看到的值会骗人（D182）
  left: Number(20)
 right: Number(30)
```

→ **与复核 M-01 的实验形成对照**：同样的变异下，
config 侧那条断言仍是绿的（复核实测），**新断言红了** —— 判据缺口已补上。

---

## 4. F14 —— SSO 登录事件码零覆盖（复核 L-02）

### 4.1 ⚠️ 任务卡与源码的事实冲突（按任务卡 §6 指出，不自行改方案）

任务卡 §F14 写：

> 在 `crates/platform-iam/tests/audit_events.rs` 补用例，驱动**生产的** SSO 桥接
> （`crates/platform-iam/src/sso/bridge.rs` 的 `PlatformSinkSsoRecorder`），断言
> 事件码 = `iam.sso.login.succeeded` / `iam.sso.login.failed` …

**源码事实**：`PlatformSinkSsoRecorder` **故意跳过**这两个码：

```rust
// crates/platform-iam/src/sso/bridge.rs:96
impl SsoAuditRecorder for PlatformSinkSsoRecorder {
    fn record(&self, event: &SsoAuditEvent) {
        // 服务自己写的那两个事件码 → **不重复**写（否则一次登录两条记录）。
        if Self::is_written_by_the_service(event.code) { return; }   // :53 / :96
```

原因写在 `bridge.rs:12-22`：**D157 要求「审计写不进去 → 拒绝登录（503）」**，
而 `SsoAuditRecorder::record` 是**同步**的、表达不了「写失败要拒绝」
→ 两个**登录**码由 `IamService` 自己 `await` 写（`crates/platform-iam/src/service.rs:962-986`，
经 `record_audit` → `AuditSink`）。

**若按任务卡字面做**（驱动桥接器断言这两个码）→ 永远拿到 0 条 →
要么写成恒红的用例，要么写成「断言跳过」的假绿。**两者都不允许**。

**我的处置**（保留任务卡原意「生产实现 + mock sink」，只换对的落点）：

- 驱动**真正的生产写入路径**：`IamService::complete_oidc_callback`（`:944`）
  → `record_audit`（`:397`）→ 注入的 `AuditSink`；
- 用 mock sink（`RecordingSink`，与 `tests/audit_events.rs` 同一模式）；
- 走真实 OIDC 回调（进程内 mock IdP，真端口，**不需要外网**）；
- 生产装配依据：`crates/platform-server/src/main.rs:340` 把同一个 `PgAuditSink`
  交给 `with_audit_sink`，`:514` 交给 `with_sso_audit_recorder` —— **同一个 sink**，
  所以注入 `AuditSink` 等价于验证生产落库那一路。
- 桥接器「**不该**写这两个码」这件事已由
  `bridge.rs::tests::the_login_codes_are_left_to_the_service` 钉住（既有用例，未改）。

### 4.2 新增用例（`crates/platform-iam/tests/sso_login_audit.rs`，新文件）

| # | 用例 | 行 | 断言 |
|---|---|---|---|
| 1 | `a_successful_sso_login_produces_exactly_one_sso_success_event` | `:233` | 恰好 1 条 `iam.sso.login.succeeded`；`object_type = iam_sso_session`；`tenant_id`；**`ip` 逐字 `203.0.113.9`**；**`user_agent` 逐字 `audit-test-agent`**；`actor_id` 有值；`before`/`after` 为 `None`；**阳性对照**：0 条 failed |
| 2 | `a_failed_sso_login_produces_exactly_one_sso_failure_event` | `:286` | 恰好 1 条 `iam.sso.login.failed`（用 `alg: none` 制造验签失败）；`object_type`；`tenant_id`；**`ip` 逐字**；`actor_id` 为 `None`；**阳性对照**：0 条 succeeded |

事件码用**字面量**常量（不用源码常量）—— 本用例要断言的正是「线上真的发这个字符串」，
引用常量会让「常量被改错」在两处同时错、仍然绿。

`ip` / `user_agent` 取 AC-S-127 步骤 1/2 指定的值（`203.0.113.9` / `audit-test-agent`），
与步骤 3 的「逐字相等」口径一致。

### 4.3 变异自证（2 处）

| # | 变异 | 结果 |
|---|---|---|
| 1 | `service.rs:973-979` 成功分支的 `&meta.ip` → `"0.0.0.0"` | **失败**用例 1 红：`left: Some("0.0.0.0") / right: Some("203.0.113.9")` |
| 2 | `service.rs:981` 失败分支 `login_event(..., true, ...)` → `..., false, ...`（sso 标志位） | **失败**用例 2 红：`实际记录的事件码：["iam.login.failed"]`（`left: 0, right: 1`） |

还原：`cp` 备份 + `touch`，`sha256 = 8ea4e3ff18f369d2…69c28` **逐字节相同**，
且 `git diff` 只剩**上一轮**对该文件的 4 处改动（`&tenant` → `tenant`），我的变异已全部清除。

---

## 5. F13 —— 主报告矛盾与过时数字（复核 M-02 / L-01）

**订正了** `BA/feature-meta/req-014-dev-fixround.md`（**只动 F13 涉及处**，
其余内容一字未改；`req-014-dev-fixround-f10.md` **未动** —— 那份是对的）：

| 位置 | 原文 | 订正后 |
|---|---|---|
| 文件头 | （无声明） | 新增 **「⚠️ 订正声明」** 块：写明原报告写于 12:45:56、f10 补丁写于 12:56:57、未同步的原因，并注明「**这不是当初就写对了**」 |
| §0 表 F10 行 | **未落地** / 「裁决与源码事实冲突」 | **已落地（+214 行 / 2 条新断言）**，指向 f10 报告；附工作区实测证据 |
| §8.1 表 | `platform-server = 125`，合计 **925**，+13 | `platform-server = **127**`，合计 **927**，+15；新增「+6」构成里点名 F10 的 2 条 |
| §11 D-1 | 「F10 未落地」 | 改为「**F10 已落地**（~~原记未落地~~）」，说明是时序未同步 |
| §12「现状」 | 「`i18n_titles.rs` **保持原样**（未修改）」 | 改为「**已按 D194 落地**（+214 行）」+ 两条断言行号；原句保留并标注「12:45 的当时状态」 |
| §13 U-1 | 「F10 未落地」 | 划掉并改为「**已落地**」 |
| §14 总结 | 「F1~F9 …925…F10 未落地」 | 「F1~**F10** …**927**…F10 已按 D194 落地」，**并保留原文引用**以留痕 |

**订正依据（工作区实测）**：
`git diff --stat crates/platform-server/tests/i18n_titles.rs` → `214 ++++++++++++++++++++++++++++`；
`:358 every_error_code_is_registered_in_every_locale`、`:414 titles_error_codes_are_registered_in_every_locale`。

---

## 6. 实测数字（全部自己跑、自己算）

### 6.1 逐 crate 全量（11 个，串行，变异已全部还原后跑）

| crate | passed | failed | ignored | rc | 上轮复核 | 差 |
|---|---|---|---|---|---|---|
| platform-core | **153** | 0 | 0 | 0 | 153 | 0 |
| platform-db | **46** | 0 | 0 | 0 | 46 | 0 |
| platform-iam | **180** | 0 | 0 | 0 | 178 | **+2** ← F14 |
| platform-org | **93** | 0 | 0 | 0 | 93 | 0 |
| platform-rbac | **83** | 0 | 0 | 0 | 83 | 0 |
| platform-config | **89** | 0 | 0 | 0 | 89 | 0 |
| platform-schema | **20** | 0 | 0 | 0 | 20 | 0 |
| platform-server | **128** | 0 | 0 | 0 | 127 | **+1** ← F12 |
| platform-audit | **54** | 0 | 0 | 0 | 54 | 0 |
| sandbox | **33** | 0 | 0 | 0 | 33 | 0 |
| xtask | **51** | 0 | 0 | 0 | 51 | 0 |
| **合计** | **930** | **0** | **0** | — | 927 | **+3** |

**+3 的构成**（与改动一一对应，**无下降 ⇒ 无用例被误删**）：

- `platform-iam` **+2**：`tests/sso_login_audit.rs`（F14 的两条）
- `platform-server` **+1**：`tests/audit_default_size_real_db.rs`（F12 的跨 crate 断言）

### 6.2 前端

| 项 | 实测 | 判据 | 判定 |
|---|---|---|---|
| `pnpm --filter @base/shell test` | **206 passed / 0 failed**（17 files） | ≥ 206 | ✅ |
| `pnpm --filter @base/i18n test` | **43 passed / 0 failed** | 43 | ✅ |
| `pnpm --filter @base/sdk test` | **38 passed / 0 failed** | 38 | ✅ |
| `pnpm -r build` | 通过（`dist/assets/index-CzJOhtSa.js` 128.54 kB） | 通过 | ✅ |
| `npx svelte-check --threshold error` | **0 errors and 0 warnings** | 0 error | ✅ |

### 6.3 E2E（干净库）

| 项 | 实测 |
|---|---|
| 跑前 `tenant-a` 审计行数 | **11**（全部 `e2e_fixture`；`all_audit_rows = 11`） |
| 干净性确认方式 | `DROP SCHEMA public CASCADE` 整库重建 → 迁移 → 播种；**播种前实测 `0` 行** |
| `npx playwright test e2e/` | **35 passed / 0 failed / 0 skipped** |
| 负向对照（删掉 F11 那行，同一干净库） | **1 failed / 5 passed**（「回退」超时，按钮 `disabled`） |

### 6.4 clippy / fmt / capabilities

| 项 | 实测 | 判定 |
|---|---|---|
| `cargo clippy --workspace --all-targets` | **2 条**，均为**基线存量**：`platform-iam/src/sso/config.rs:153`、`sso/oidc.rs:234` | 新增 **0** ✅ |
| `cargo fmt --all --check` | **11 个文件 / 36 处**（= 基线 TD-017 存量） | 新增 **0** ✅（新文件与 `config_real_db.rs` 均**不在**红名单） |
| `cargo xtask capabilities --check` | 「能力索引校验通过：**14 个文件**与源码一致」 | ✅ |

---

## 7. 偏离声明

| # | 与任务卡/裁决不一致处 | 理由 |
|---|---|---|
| **D-1** | **F14 的落点从 `PlatformSinkSsoRecorder` 改为 `IamService::complete_oidc_callback`** | 任务卡说这两个码由桥接器写，**源码事实是桥接器明确跳过**（`bridge.rs:53/96`，理由见 `:12-22`：D157 要「写失败即拒绝登录」，而 `record` 是同步的）。按字面做只能得到恒红或假绿 → 按任务卡 §6「发现冲突就停下并给证据」处置，同时**保留原意**（生产实现 + mock sink + 真实回调）。证据见 §4.1 |
| D-2 | F14 的用例放在**新文件** `tests/sso_login_audit.rs`，不是追加进 `tests/audit_events.rs` | 任务卡允许两者之一（「或同 crate 的新测试文件」）；新文件要 `mod sso_support;`，塞进 `audit_events.rs` 会与它自己的夹具函数名冲突 |
| D-3 | 干净库重建**多做了 3 步**（sandbox 模块迁移 / `02-app-role.sql` / `xtask tenant-policies`） | `DROP SCHEMA public CASCADE` 会连带清掉**表、授权、RLS 策略**；而 `scripts/e2e-server.sh` 只跑平台迁移 + 播种 → 直接跑会因 `permission denied` 起不来。详见 §8 U-1 |
| D-4 | F12 附带改了**行内断言消息**（「运行时默认」→「口径（20）」） | 那句消息本身就是 M-01 指出的错误表述；只改 doc 不改它会留下同一处不一致 |

---

## 8. 未完成 / 未验证项（如实列出，**没有**「应该没问题」这类表述）

| # | 项 | 状态 | 说明 |
|---|---|---|---|
| **U-1** | `scripts/e2e-server.sh` **无法从零重建库** | ⚠️ **本轮发现的环境缺陷，未修（超出 F11~F14）** | 它只跑平台 `migrate up` + `seed-e2e.sh`，**不跑** `deploy/seed/02-app-role.sql`、**不跑** `modules/sandbox/migrations/`、**不跑** `xtask tenant-policies`。在没有历史残留的库上，`base serve` 会以 `permission denied for table platform_permission` 退出（我实测到），sandbox 的 6 条 E2E 会 500。**建议另立 REQ**：让 `e2e-server.sh` 像 `test-integration.sh` 一样自足。我**没有**改这个脚本（不在 F11~F14 范围） |
| **U-2** | F14 **未在真库上断言落库**（只断言到 `AuditSink` 收到的 `AuditEvent`） | 部分验证 | 用的是**生产的服务写入路径** + mock sink；「`PgAuditSink` 把它写进 `platform_audit_log`」这一段由既有模式（`platform-server/tests/audit_chain_real_db.rs`）覆盖同类链路，但**本用例本身**没读表。若要端到端读表，需要把 `platform-audit` 的 `PgAuditSink` 注入 `platform-iam` 的测试 → `platform-iam` 的 dev-deps 里**没有** `platform-audit`（会成环），故**未做** |
| **U-3** | 复核报告 U-02（D157 写失败 503） | **已被现有用例覆盖，无需修** | BA 已核实：`crates/platform-iam/tests/audit_events.rs:380-420` 已断言 `error.code() == code::AUDIT_SINK_UNAVAILABLE`。**本条不是缺陷**，仅在此注明复核误报 |
| **U-4** | 复核 L-03 的 U-05 / U-07 / U-09（`config.flag.write` 形状、5 条中文文案人工校对、`platform_audit_chain_state` 跨租户 RLS） | **仍未验**（不在 F11~F14 范围） | 如实列出，本轮**未**声称已验证 |
| **U-5** | E2E 只在 `chromium` 一个 project 上跑 | 未覆盖其它浏览器 | 仓内 `playwright.config.ts` 只配了 chromium（与复核口径一致） |
| **U-6** | 生产/既有库的 `audit.page.defaultSize` 行 | 未同步 | 沿用上一轮 U-3；迁移已进 `_sqlx_migrations` 不重跑，需部署时跑一次幂等 `platform_config_upsert` |

---

## 9. 已写入的文件

**代码（feature worktree，未提交）**

1. `/home/myg133/Projects/system_seed/feature-REQ-014/e2e/audit.spec.ts`（F11，`+8` 行）
2. `/home/myg133/Projects/system_seed/feature-REQ-014/crates/platform-config/tests/config_real_db.rs`（F12 注释订正）
3. `/home/myg133/Projects/system_seed/feature-REQ-014/crates/platform-server/tests/audit_default_size_real_db.rs`（**新增**，F12 跨 crate 断言）
4. `/home/myg133/Projects/system_seed/feature-REQ-014/crates/platform-iam/tests/sso_login_audit.rs`（**新增**，F14 两条用例）

**报告（BA/feature-meta）**

5. `/home/myg133/Projects/system_seed/BA/feature-meta/req-014-dev-fixround.md`（**订正** F13 的 7 处）
6. `/home/myg133/Projects/system_seed/BA/feature-meta/req-014-dev-fixround2.md`（**本文件**）

**未改动**（确认）：`F1~F10` 的任何实现、`BA/demands/`、`BA/plan/`、
`req-014-dev-fixround-f10.md`。`git status --short` 无 MUTATION 残留。
