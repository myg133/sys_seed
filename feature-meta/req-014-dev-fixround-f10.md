# REQ-014 修复轮 F10 小报告（跨栈 i18n 断言）

- **需求**: REQ-014 审计日志（修复轮 F10）
- **工单**: `BA/feature-meta/req014-fixround-brief.md` 附录 A（A.1~A.4）
- **裁决**: `BA/demands/REQ-014-audit/decisions.md` rev 5 的「rev 5」段（**D194 / D195**）
- **工作目录**: `/home/myg133/Projects/system_seed/feature-REQ-014`（分支 `feature/REQ-014`，HEAD `5f28c0c`）
- **依据**: D194 作废了 D192 的「集合相等」判据 → 本轮按【包含】关系实现
- **Dev 于 2026-11-20 写下；未提交（由 BA 统一提交）**

---

## 1. 改了什么

**只改了一个文件**：`crates/platform-server/tests/i18n_titles.rs`（+214 行，0 删除 / 0 修改既有断言）。

| 位置 | 内容 |
|---|---|
| `i18n_titles.rs:29-53` | 模块注释新增「跨栈」一节（钉住**集合不相等**的理由与三条断言的对应关系）；新增常量 `ORG_EVENT_SOURCE`（`:47`）、`LOCALES`（`:52`）；`use std::path::{Path, PathBuf}` |
| `i18n_titles.rs:206-215` | `fn repo_root()` —— `CARGO_MANIFEST_DIR` 往上**两级**（= 仓库根，与 `i18n_no_chinese_titles.rs:492` 同款惯例） |
| `i18n_titles.rs:217-270` | `fn locale_error_codes(locale)` —— 读 `packages/i18n/src/locales/<locale>.json`，返回 `error.*` 去掉前缀后的码集合；**读不到文件直接 panic**（不静默返回空集合）；词条为空串也判失败 |
| `i18n_titles.rs:272-282` | `fn error_codes()` —— `all_codes()` 里按**来源标签** `ORG_EVENT_SOURCE` 排除组织事件码 |
| `i18n_titles.rs:357-406` | **断言 2**：`every_error_code_is_registered_in_every_locale` |
| `i18n_titles.rs:413-445` | **断言 3**：`titles_error_codes_are_registered_in_every_locale` |

### 断言 2 的实现要点（`i18n_titles.rs:358`）

- 被测集合 = `error_codes()`（即 `all_codes()` 去掉 `platform-org::notify` 那一段）。
- **组织事件码按来源标签排除，不用正则猜**：`all_codes()` 里 `OrgEventKind::ALL_CODES`
  那一段的来源字面量就是 `"platform-org::notify"`（原文件 `:57-58`），直接复用它。
  这就避免了「用前缀 `org.` 猜事件码」的两类误伤（将来某个真错误码以 `org.` 开头、
  或事件码前缀写错）。
- **排除动作自证**：`org_codes.len()` 必须等于 `OrgEventKind::ALL_CODES.len()`，
  且 `codes.is_disjoint(&org_codes)` —— 防止「排除分支写错 = 一个都没排除」而静默通过。
- 判据是**包含**：对**每个** locale 求 `codes \ locale_error_codes(locale)`，
  非空即失败，并把两侧缺失清单打全。

### 断言 3 的实现要点（`i18n_titles.rs:414`）

- 被测集合 = `TITLES` 的键 **∩** `error_codes()`（= 78 个错误码；6 个 `org.*` 不进交集）。
- 同样逐个 locale 断言这些码都有 `error.<code>`。
- 方向与断言 2 相反：断言 2 抓「后端有、前端没文案的响应体码」，断言 3 抓
  「`TITLES` 登记了但词库没人写的键」。

### 硬要求的落实

- ✅ 两条断言都 `for locale in LOCALES`（`["en-US", "zh-CN"]`），**不是只查英文**。
- ✅ 不引新依赖：只用 `serde_json`（`platform-server` 已在 `[dependencies]`，
  `crates/platform-server/Cargo.toml:15`）；`Cargo.toml` **未改**。
- ✅ **未改** `crates/platform-core/src/titles.rs`（6 个码不该补标题）。
- ✅ **未改** `the_title_table_has_the_pinned_number_of_rows` 的钉死值（仍是 84）。
- ✅ **没有**写「集合相等」断言。

### 实现中撞到并自行修正的一处事实错误

首次实现把 `repo_root()` 写成「往上三级」，实测报
`读不到词库 /home/myg133/Projects/system_seed/packages/i18n/src/locales/en-US.json`。
`env!("CARGO_MANIFEST_DIR")` **本身**就是 `crates/platform-server` → 往上**两级**才是仓库根。
已改为 `.parent().and_then(Path::parent)`（与 `i18n_no_chinese_titles.rs:492` 一致）。
**这是实现细节的自我修正，不涉及判据变更。**

---

## 2. `platform-server` 实测数字（自己算）

命令（§1 铁律环境 + 工单指定口径）：

```bash
cargo test -p platform-server --all-targets -- --test-threads=1 --include-ignored \
  | grep -E '^test result' | awk '{p+=$4; f+=$6; i+=$8} END {printf "passed=%d failed=%d ignored=%d\n", p, f, i}'
```

实测输出：

```
passed=127 failed=0 ignored=0
```

- 上一轮基线 `platform-server = 125`；本轮**新增 2 条用例** → 125 + 2 = **127**，
  数字**上升**且差值正好等于新增用例数 → 无用例被误删。
- 单跑 `--test i18n_titles`：`test result: ok. 9 passed; 0 failed; 0 ignored`（原 7 条 + 新增 2 条）。
- 其中两条新用例的 stdout（证明它们真的跑了判据、不是空转）：

```
every_error_code_is_registered_in_every_locale
  → 错误码 78 个（已排除 6 个组织事件码），2 个 locale 均齐全
titles_error_codes_are_registered_in_every_locale
  → TITLES 的错误码 78 个（另有 6 个组织事件码不参与），2 个 locale 均齐全
titles_cover_every_code
  → TITLES = 84 行；各 crate 的 code 合计 102 条（去重后 84 条）
```

（78 + 6 = 84，与 D194 事实表一致。）

---

## 3. 敏感度自证证据

备份用 `cp`（**未用 `cp -p`**）后记 sha256：

```
$ cp packages/i18n/src/locales/en-US.json /tmp/f10/en-US.json.bak
$ cp packages/i18n/src/locales/zh-CN.json /tmp/f10/zh-CN.json.bak
$ sha256sum packages/i18n/src/locales/en-US.json packages/i18n/src/locales/zh-CN.json | tee /tmp/f10/sha-before.txt
34b4d6528fccea6b4cedd5b11a5ca7f7b6fab77a2cfdeeceb93a372271cfb823  packages/i18n/src/locales/en-US.json
ee3cc4f251785ac4784786bb0f5c47c9914221948f56579cb0d4dfcc452177a9  packages/i18n/src/locales/zh-CN.json
```

### 3.1 en 侧（删 `error.AUDIT_CHAIN_BROKEN`）

临时删除后 `grep -c '"error\.'` = **83**，跑 `--test i18n_titles`：

```
thread 'every_error_code_is_registered_in_every_locale' panicked at crates/platform-server/tests/i18n_titles.rs:392:5:
78 个错误码在词库里没登记（D125：会进响应体的码必须两边都登记）：
en-US 缺 1 条：["AUDIT_CHAIN_BROKEN"]

thread 'titles_error_codes_are_registered_in_every_locale' panicked at crates/platform-server/tests/i18n_titles.rs:439:5:
TITLES 里 78 个错误码在词库里没登记：
en-US 缺 1 条：["AUDIT_CHAIN_BROKEN"]

test result: FAILED. 7 passed; 2 failed; 0 ignored
```

→ **两条新断言同时红**，且报错点名到具体码与 locale。

### 3.2 zh 侧（删 `error.AUDIT_EVENT_NOT_FOUND`，**只动中文词库**）

还原 en 后（`sha256sum -c` 通过），只删 `zh-CN.json` 的一条，`grep -c '"error\.'` = **83**：

```
thread 'every_error_code_is_registered_in_every_locale' panicked at crates/platform-server/tests/i18n_titles.rs:392:5:
78 个错误码在词库里没登记（D125：会进响应体的码必须两边都登记）：
zh-CN 缺 1 条：["AUDIT_EVENT_NOT_FOUND"]

thread 'titles_error_codes_are_registered_in_every_locale' panicked at crates/platform-server/tests/i18n_titles.rs:439:5:
TITLES 里 78 个错误码在词库里没登记：
zh-CN 缺 1 条：["AUDIT_EVENT_NOT_FOUND"]

test result: FAILED. 7 passed; 2 failed; 0 ignored
```

→ **英文词库完好、只少中文那一条，仍然红** —— 这直接证明 `zh-CN.json` 真的被检查了
（如果只查 en-US，这一条会假绿）。

### 3.3 还原校验

```
$ cp /tmp/f10/zh-CN.json.bak packages/i18n/src/locales/zh-CN.json
$ sha256sum -c /tmp/f10/sha-before.txt
packages/i18n/src/locales/en-US.json: 成功
packages/i18n/src/locales/zh-CN.json: 成功

$ git status --short packages/i18n/src/locales/
（无输出 = 两个词库文件干净）
```

还原后重跑 → `test result: ok. 9 passed; 0 failed`。

---

## 4. 其他验证

### 4.1 `cargo fmt --all --check`

- 基线 `code` worktree（`develop@0683115`）与 feature worktree 各跑一次，**归一化为仓库相对路径**后比对：

```
base=11 feat=11
=== 新增（feat 有、base 无）===      ← 空
=== 消失（base 有、feat 无）===      ← 空
=== 我改的文件是否在 feat 列表里 === 0（不在）
```

→ **新增 0 处**；红的就是那 11 个存量文件（TD-017），集合与基线**完全相同**；
`i18n_titles.rs` **不在** fmt 失败列表里。

### 4.2 `cargo clippy -p platform-server --all-targets`

- 输出仅有 1 行 `warning: platform-iam (lib) generated 2 warnings`（既有存量，且在别的 crate）。
- `grep -n "i18n_titles"` → **无命中** → 新代码 0 条 clippy warning。

### 4.3 工作区状态

```
$ git status --short
 M crates/platform-audit/src/query.rs              ← F1~F9（上一轮 Dev）
 M crates/platform-audit/tests/audit_chain.rs
 M crates/platform-config/manifest.json
 M crates/platform-config/src/service.rs
 M crates/platform-config/tests/audit_events.rs
 M crates/platform-config/tests/common/mod.rs
 M crates/platform-config/tests/config_real_db.rs
 M crates/platform-iam/src/service.rs
 M crates/platform-rbac/tests/audit_events.rs
 M crates/platform-server/src/audit.rs
 M crates/platform-server/tests/i18n_titles.rs     ← 本轮唯一改动（+214）
 M migrations/20261120000400_audit_config_seed.up.sql
?? crates/platform-audit/tests/query_real_db.rs    ← 上一轮新增
?? crates/platform-server/tests/audit_chain_real_db.rs
```

- F1~F9 的改动**一处未回退、未重写**（`git diff --stat` 显示我对 `i18n_titles.rs`
  只有 **214 insertions / 0 deletions**）。
- 词库两个 json **干净**。
- 无 `MUTATION` / `dbg!` / 调试残留（`git diff | grep -E "MUTATION|dbg!|TODO|FIXME"` 无命中）。
- **未 `git commit`。**

---

## 5. 偏离声明 / 未完成项

### 偏离

1. **未按 D192 的「集合相等」实现** —— 这是**工单明确要求**的（附录 A.1/A.2 + D194），不是偏离。
   记录在此仅为准确定性溯源：D192 已作废，本实现依据 D194。
2. **`repo_root()` 层级由「三级」改为「二级」** —— 工单附录 A.2.2 写「`../../..`」，
   实测 `CARGO_MANIFEST_DIR` = `crates/platform-server`，二级才到仓库根。
   **若照工单写 `../../..` 会指到 `/home/myg133/Projects/system_seed`（workspace 根，
   其上还有 `code/` 与 `feature-REQ-014/` 两个 worktree）→ 词库路径不存在 → 测试必失败。**
   已按仓内既有惯例（`i18n_no_chinese_titles.rs:492`、`support/mod.rs:14`）改为二级。
   ⚠️ **请 BA 确认**：这是工单相对路径笔误，判据本身不受影响。
3. **`error_codes()` 用来源标签排除组织事件码**，而不是在 `all_codes()` 内部改结构
   —— 保持 `all_codes()`（及其被 `titles_cover_every_code` / `no_orphan_titles` 复用的语义）**原样不动**。

### 未做（按工单「只落 F10」的要求）

- 未跑 F1~F9 涉及的逐 crate 全量（工单只要求 `platform-server`）。
- 未跑前端 `pnpm --filter @base/i18n test`（工单未要求；且本轮未动前端文件）。
- 未跑 `cargo clippy --workspace --all-targets` / `cargo xtask capabilities --check`
  （工单未要求；本轮只动一个测试文件）。

### 风险 / 需 BA 注意

1. `LOCALES` 是**手写常量** `["en-US", "zh-CN"]`。若将来新增语言（如 `ja-JP.json`），
   本断言**不会自动覆盖**新 locale。
   - 不用「扫目录取文件名」的理由：工单要求钉死这两个、且扫目录会让「误删某个 json」
     反而通过（少一个 locale = 少查一个）。**如果要覆盖新增语言，需要同步 `LOCALES`。**
   - 已记在此，供 BA 决定是否另立 REQ / TD。
2. 断言读的是**工作区文件**，不是编译进二进制的资源 → 它校验的是「仓库里的词库」，
   与 `packages/i18n` 打包产物的一致性**不在本断言覆盖范围内**。
3. `TITLES.len()`（84）与 i18n 条数（84）仍相等 —— 但两侧集合差 6↔6。
   **将来任何一侧增删都可能碰巧保持 84**，本断言按【包含】判，不会因此假绿。

---

## 6. 已写入的文件（绝对路径）

1. `/home/myg133/Projects/system_seed/feature-REQ-014/crates/platform-server/tests/i18n_titles.rs`（代码，+214 行）
2. `/home/myg133/Projects/system_seed/BA/feature-meta/req-014-dev-fixround-f10.md`（本报告）

未改动：`crates/platform-server/Cargo.toml`、`crates/platform-core/src/titles.rs`、
`packages/i18n/src/locales/*.json`（敏感度自证后已逐字节还原）。
