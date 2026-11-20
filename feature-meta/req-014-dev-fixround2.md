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

---

# 附录 B 微修（B1~B3，2026-11-20 18:10 追加）

> 依据：`BA/feature-meta/req014-fixround2-brief.md` 附录 B（聚焦复核 PASS-WITH-RISK，
> 0 阻塞 / 0 高 / 0 中 / 3 低）。本节**只**记录 B1~B3，不改动上面 §1~§9 的原文。
> 工作区未提交改动（修复轮 1 + 2 的成果）**未回退、未重写**。

## B-1. B1~B3 逐条：改了什么

| # | 文件:行 | 改动 | 为什么 |
|---|---|---|---|
| **B1** | `crates/platform-iam/tests/sso_login_audit.rs:331-337` | 失败分支新增 `assert_eq!(event.user_agent.as_deref(), Some(USER_AGENT), "失败事件的 \`userAgent\` 也必须逐字透传")` | 复核 L-01：成功分支有 UA 断言、失败分支**漏了** → 把 UA 置 `""` 用例仍绿。AC-S-127 步骤 3 的 ip/userAgent 口径**不分成功/失败** |
| B1（附带） | 同文件 `:282-291` | 订正失败用例的 doc：「失败载荷只带 provider/code/ip」说的是 `SsoAuditPayload` 的白名单（`sso/audit.rs:28`），本用例断言的是 `AuditEvent`（`login_event` → `AuditSink`），两者别混 | 那段 doc 正是「漏断言」的合理化借口；只补断言不订正 doc 会留下同一处误导 |
| **B2** | `crates/platform-config/tests/config_real_db.rs:1837` | 用例名 `the_audit_page_default_size_is_the_same_everywhere` → `the_audit_page_default_size_matches_the_contract_in_manifest_and_db` | 复核 L-02：doc 已改成「实际覆盖两处（manifest + DB）」，名字仍说「三处一样」→ 名字在骗人。**只改名**：断言逻辑与 doc 一字未动 |
| **B3** | `crates/platform-server/tests/audit_chain_real_db.rs:302-415`（用例 `a_config_flag_write_really_lands_in_the_audit_table`，定义在 `:325`） | 新增 flag 写入的**真库落表**用例：`ConfigService::new` + `with_audit_sink(PgAuditSink)` → `write_flag("newInvoice", …)` 两次 → 查 `platform_audit_log` | 复核 V-03：D163 的两个配置事件码里，`config.entries.write` 已有真库判据，`config.flag.write` **只有 mock sink 形状断言**。代码两条码都在发（`config/src/service.rs:462` / `:547`）→ 不是缺陷，是**判据缺失** |

B3 的断言口径（D163）与既有 config 用例同款：

| 字段 | 期望 | 本用例取值 |
|---|---|---|
| `action` | `config.flag.write` | 两次都断（事件码是字面量，引用常量会让「常量被改错」两边同时错） |
| `object_type` | `config_flag` | ✔ |
| `object_id` | **flag 名**（区别于 `config.entries.write` 的「作用域」） | `newInvoice` |
| `before` / `after` | 该租户层「flag 名 → 值」的映射 | 首次 `{"newInvoice": null}` → `{"newInvoice": false}`；第二次 `{"newInvoice": false}` → `{"newInvoice": true}` |

`before` 用 `null` 而不是「省略键」，是 `layer_values` 的既有口径
（`platform-config/src/service.rs:183-204`：该层没有这个键 → `Value::Null`），
与既有 `a_config_write_really_lands_in_the_audit_table` 的 `{"title": null}` 一致。

## B-2. 实测数字（自己用 awk 算，不引用旧报告）

命令（串行、逐 crate）：

```bash
cargo test -p <crate> --all-targets -- --test-threads=1 --include-ignored \
  | grep -E '^test result' | awk '{p+=$4; f+=$6; i+=$8} END {printf "passed=%d failed=%d ignored=%d\n", p, f, i}'
```

| crate | passed | failed | ignored | 相对基线 930 |
|---|---|---|---|---|
| platform-core | 153 | 0 | 0 | — |
| platform-db | 46 | 0 | 0 | — |
| platform-iam | 180 | 0 | 0 | —（B1 是补断言，不是补用例） |
| platform-org | 93 | 0 | 0 | — |
| platform-rbac | 83 | 0 | 0 | — |
| platform-config | 89 | 0 | 0 | —（B2 只改名） |
| platform-schema | 20 | 0 | 0 | — |
| platform-server | **129** | 0 | 0 | **+1（B3）** |
| platform-audit | 54 | 0 | 0 | — |
| sandbox | 33 | 0 | 0 | — |
| xtask | 51 | 0 | 0 | — |
| **合计** | **931** | **0** | **0** | **+1（预期 931）** |

⚠️ 环境事实：第 11 个 crate 的**包名是 `sandbox`**（`modules/sandbox/backend/Cargo.toml:2`），
不是任务卡写的 `platform-sandbox` —— 用 `-p platform-sandbox` 会得到
`error: package ID specification did not match any packages`（rc=101，0 条 test result）。
换成 `-p sandbox` 后 33 passed / 0 failed / 0 ignored，总数才回到 931。

### E2E（干净库）

**怎么确认库是干净的**：跑之前我**先把库清到 0 行**——不是靠假设：

```sql
-- 清理前：all=5 / tenant-a=5 / fixture=0（这 5 行是 platform-audit 用例的
--         demo.item.page0..4 残留，会把 tenant-a 顶到 16 行，不再是文档口径的 11）
DELETE FROM platform_audit_log WHERE tenant_id='tenant-a';        -- DELETE 5
DELETE FROM platform_audit_chain_state WHERE tenant_id='tenant-a'; -- DELETE 1
-- 清理后：all=0 / tenant-a=0 / fixture=0
```

然后 `npx playwright test e2e/`（`webServer` 会先 `vite build` + `e2e-server.sh`；
后者跑迁移并 `seed-e2e.sh` 幂等播 11 条夹具 → 起点 tenant-a=11 < 默认 limit 20）：

```text
Running 35 tests using 1 worker
...
  35 passed (45.4s)
```

→ **35 passed / 0 failed / 0 skipped**。跑完后库累积为
`all=33 / tenant-a=32 / e2e_fixture=11`（登录等真实事件会写审计表，属正常累积）。

## B-3. 自证证据（红绿由被断言的那一行决定）

### B1 —— 失败分支 UA 置空 → 用例必须红

```text
# 基线 sha256（改前）
8ea4e3ff18f369d2be0a36845a647a6fd5747c2941fe4ec5e8cd421e76769c28  crates/platform-iam/src/service.rs
# 变异：:981  &meta.user_agent → ""
# 变异后 sha256
67f78b559255229e5de967cbeac25f181f10508d977e1c726b1e97f358f1f327

test a_failed_sso_login_produces_exactly_one_sso_failure_event ... FAILED
thread '...' panicked at crates/platform-iam/tests/sso_login_audit.rs:333:5:
assertion `left == right` failed: 失败事件的 `userAgent` 也必须逐字透传
  left: Some("")
 right: Some("audit-test-agent")
test result: FAILED. 1 passed; 1 failed; 0 ignored
```

还原（`cp` 回备份 + `touch`）后：

```text
crates/platform-iam/src/service.rs: 成功      # sha256sum -c 逐字节相同
```

### B3 —— flag 埋点改码 → 新用例必须红

```text
# 基线 sha256（改前）
9cfd68f0e0f9c1a8788d51ad8514cd635aee904133e6715687993cec2cc80c3a  crates/platform-config/src/service.rs
# 变异：:546  "config.flag.write" → "config.flag.wrote"

test a_config_flag_write_really_lands_in_the_audit_table ... FAILED
thread '...' panicked at crates/platform-server/tests/audit_chain_real_db.rs:368:5:
assertion `left == right` failed: 事件码（D163）
  left: "config.flag.wrote"
 right: "config.flag.write"
test result: FAILED. 4 passed; 1 failed; 0 ignored
```

关键：**只有 flag 用例红**，同文件的 `a_config_write_really_lands_in_the_audit_table`
（entries）仍然绿 → 新断言确实只盯 `config.flag.write`，不是顺带被 entries 顶绿的。

还原后 `sha256sum -c` → `crates/platform-config/src/service.rs: 成功`。
两个变异文件均已 `touch`（避免 cargo 复用陈旧二进制），最终 `sha256sum -c` 双双成功。

### B2 —— 改名，无需自证

改名后 `cargo test -p platform-config` 89 passed / 0 failed；
`git grep the_audit_page_default_size_is_the_same_everywhere` 在源码里 **0 命中**
（仅历史 QA 报告引用旧名，未改）。

## B-4. 其他验证

- `cargo clippy --workspace --all-targets` → **rc=0**，**2 条存量 warning**
  （`platform-iam` lib 的 `large size difference between variants` /
  `very complex type used`；lib test 报 2 条重复）→ **新增 0**。
- `cargo fmt --all --check` → 红 11 个**存量**文件（TD-017 基线：
  platform-contracts 3 / platform-schema 3 / platform-server 3 / sandbox 3 个文件口径），
  我改的三个文件 **0 处 diff**（单文件 `rustfmt --check` 退出码 0）。

## B-5. 偏离声明

| # | 与任务卡不一致处 | 理由 |
|---|---|---|
| **DB-1** | B1 **附带订正了失败用例的 doc**（`sso_login_audit.rs:282-291`） | 那段 doc（「失败载荷只带 provider / code / ip」）把 `SsoAuditPayload` 的白名单与本用例断言的 `AuditEvent` 混为一谈，正是漏 UA 断言的合理化来源。任务卡要求「只补断言」；不改 doc 会留下**新的**注释-代码不一致（上一轮 M-01 被罚的正是这个模式）。断言逻辑未动 |
| DB-2 | 第 11 个 crate 用 `-p sandbox` 而非任务卡写的 `platform-sandbox` | 包名事实如此（`modules/sandbox/backend/Cargo.toml:2`）。按卡上的名字会 rc=101、0 用例，报告若照抄就是**少跑一个 crate 还看不出来** |
| DB-3 | E2E 前**删了 5 行 `demo.item.*` 残留 + 1 行链状态** | 任务卡说「测试库现在处于干净起点（11/11/11）」；我跑完 11 个 crate 后实测是 `all=5 / tenant-a=5`，已不再是那个起点。为让 E2E 结果可复现，按任务卡允许的「清干净后重播 seed」口径恢复成 0 行起点（不是拿脏库充数，也不是改 seed 条数） |

## B-6. 未完成 / 未验证项（如实）

| # | 项 | 状态 | 说明 |
|---|---|---|---|
| B-U1 | 前端单测（`@base/shell` ≥206 / `@base/i18n` 43 / `@base/sdk` 38）、`pnpm -r build`、`svelte-check` | **本轮未跑** | 附录 B.3 的必跑清单只有逐 crate + E2E + clippy/fmt；B1~B3 均不触碰前端。上一轮已跑过，本轮无前端改动 |
| B-U2 | `config.flag.write` 的 `rollout_percent` 分支（派生键 `flags.<F>.rolloutPercent`）真库落表 | **未单测** | B3 只覆盖 `enabled`（D163 的形状断言主体）。派生键同走一次 `write_scoped` + 一条审计事件，但「`after` 同时含 flag 与 rollout 两个键」的形状**没有**机械判据 |
| B-U3 | 生产/既有库的迁移落表 | 未同步 | 沿用 §8 U-6（迁移已进 `_sqlx_migrations` 不重跑，部署时需幂等 `platform_config_upsert`） |

## B-7. 本节写入的文件

**代码（feature worktree，未提交）**

1. `/home/myg133/Projects/system_seed/feature-REQ-014/crates/platform-iam/tests/sso_login_audit.rs`（B1：+断言 +doc 订正）
2. `/home/myg133/Projects/system_seed/feature-REQ-014/crates/platform-config/tests/config_real_db.rs`（B2：仅用例名）
3. `/home/myg133/Projects/system_seed/feature-REQ-014/crates/platform-server/tests/audit_chain_real_db.rs`（B3：新增 1 条真库用例）

**报告**

4. `/home/myg133/Projects/system_seed/BA/feature-meta/req-014-dev-fixround2.md`（**本节为追加**，§1~§9 原文未改）

**未改动**（确认）：`crates/platform-iam/src/service.rs`、`crates/platform-config/src/service.rs`
（两个变异均已还原，`sha256sum -c` 逐字节成功）、F1~F14 的既有实现、
`BA/demands/`、`BA/plan/`、`e2e/`。
