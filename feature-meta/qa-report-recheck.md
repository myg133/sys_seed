# 验证报告 —— 修复复核（Recheck）

- 需求: REQ-003 扩展点框架与契约测试
- 分支: feature/REQ-003 @ `fd12156668b5779ca2daab0ab354cabb819386c2`
- 基线（首轮 QA 所审提交）: `04b57d39dc92ebf336584a91d65e7ed658a7fb58` → 修复后 `fd12156`（7 个小步提交）
- 首轮报告: `feature-meta/qa-report.md`（结论 PASS-WITH-RISK，中 2 + 低 4 + 提示 2，共 8 条）
- 复核范围: **5 个改动点 + 一次回归**（首轮已确认成立的 5 个 trait 签名、C-1~C-4 破坏性验证、依赖方向基线、sandbox 模块质量未重跑）
- 验证者: QA Agent（独立复现；**未采信** Dev 自验报告中的任何数字与结论，全部自己跑）
- 工具链: rustc/cargo 1.98.1 / pnpm 12.19.0 / bun / node（ECMA-262 正则对照）/ python3（独立正则引擎）
- 日期: 2026-11-18
- 结论: **PASS**
  - 本轮 5 个改动点 **5/5 复验通过**，两条「中」级门禁盲区（首轮 #1 / #2）经独立造违规复现确认已封堵；
  - 回归全绿（Rust 73/73、前端 23/23、E2E 1/1、clippy 0 告警、fmt 通过、`Cargo.lock` 零新第三方包）；
  - 残留：**1 条低**（低 5「完全一致」在 2 个 Unicode 空白码位上仍有差异）、**首轮遗留 1 条低**（低 4 C-2 口径，BA 侧待定）、
    另有 4 条提示（含 Dev 自陈未做项）。**均不阻塞合并**。

---

## 逐项复核

| # | 问题 | Dev 声称 | 复核结果 | 证据 |
|----|------|---------|---------|------|
| **中 1** | `check-modules` 静默吞掉 manifest 结构级违规 | `check_manifest_schema` 解析失败时把 serde 错误报为一条 `manifest_issues` | ✅ **通过**（3/3 违规全部拦住，exit 1，字段路径可见） | 见 §1 |
| **中 2** | 依赖方向检查是文本扫描，可被绕过 | 改读 `cargo metadata --no-deps` 成员依赖图；`platform-contracts-macros` 纳入平台 crate | ✅ **通过**（2/2 绕过全部拦住） | 见 §2 |
| **低 5** | schema 与平台实现判定不一致 | schema 收紧到与实现完全一致 | ⚠️ **基本通过**（常规字符 0 分歧；**2 个 Unicode 空白码位仍有分歧**，见仍存在的问题 #1） | 见 §3 |
| **低 6** | sandbox 排序白名单外字段被静默忽略 | 改为返回错误，README / AGENTS.md 写明 | ✅ **通过** | 见 §4 |
| **提示 7** | `feature-meta/manifest.json` 契约 pin 被冲掉 | 补回 `contracts` / `adrs` / `delivered_by` | ✅ **通过** | 见 §5 |

---

## §1 中 1 —— `xtask check-modules` 报出结构级 manifest 违规

**实现**（`crates/xtask/src/modules.rs:196-219`）：不再走 `Manifest::from_path(...).ok()`，
改为 `Manifest::parse_str(&raw)`，serde 失败时把 `err.reason()` 整条报出；JSON 语法错误仍由 `invalid_manifest` 兜底（不重复报）。

**独立造违规复现**（临时探针模块目录，建在 `.local/qa-recheck/probe-m1/`，**未改 `modules/sandbox/`**，验完已删）：

```text
$ ./target/debug/xtask check-modules --root .local/qa-recheck/probe-m1
模块目录校验失败：错误码 E_MODULE_LAYOUT
  ✗ empty
      - manifest.json 不符合 Schema：missing field `id` at line 1 column 2
  ✗ missingid
      - manifest.json 不符合 Schema：missing field `id` at line 1 column 31
  ✗ unknownfield
      - manifest.json 不符合 Schema：unknown field `bogus`, expected one of `id`, `name`, `version`, `enabled`, `permissions`, `menus`, `dependencies`, `config` at line 6 column 9
EXIT CODE = 1
```

| # | 造的违规 | 期望 | 实测 | 结论 |
|---|---------|------|------|------|
| 1 | `manifest.json` = `{}` | 报 `missing field 'id'` | `manifest.json 不符合 Schema：missing field \`id\` at line 1 column 2` | ✅ |
| 2 | `{"name":"Probe","enabled":true}` | 报 `missing field 'id'` | `manifest.json 不符合 Schema：missing field \`id\` at line 1 column 31` | ✅ |
| 3 | 含未知字段 `"bogus":1` | 报 `unknown field 'bogus'` | `manifest.json 不符合 Schema：unknown field \`bogus\`, expected one of \`id\`, \`name\`, … at line 6 column 9` | ✅ |

**附加探测（同一类违规，Dev 未列）**：字段类型错 `"enabled":"yes"` → 也被拦住：

```text
  ✗ typeerr
      - manifest.json 不符合 Schema：invalid type: string "yes", expected a boolean at line 1 column 64
```

**对照**：首轮同一探针是 `exit 0`「模块目录校验通过」。本轮 `exit 1`。

**残留（提示级）**：结构级违规的消息格式是
`manifest.json 不符合 Schema：<serde 原始消息>`
而字段级违规是 `manifest.json 不符合 Schema：<字段路径>：<原因>`
两段式。前者的「字段路径」混在 serde 消息里（`missing field \`id\``），没有独立的字段路径段，
与首轮要求的「给出字段路径与原因」在**格式上不统一**。信息本身完整，不影响判失败。

---

## §2 中 2 —— 依赖方向检查改为 cargo metadata 图，且不可绕过

**实现核对**（`crates/xtask/src/deps.rs`）：

```text
$ grep -n "read_to_string\|lines()\|contains(\"modules/\")\|starts_with(|c" crates/xtask/src/deps.rs
  -> 无任何文本扫描（0 命中）
```

- 数据来源：`cargo metadata --format-version 1 --no-deps`（`deps.rs:17,197`），读 `packages[].dependencies[].path` 的**解析后绝对路径**；
- 平台 crate 集合 = 显式清单 `PLATFORM_CRATES`（已含 `platform-contracts-macros`）∪ `crates/` 下的工作区成员（`deps.rs:124-138`）→ 新增平台 crate 不会漏检；
- 可达性遍历（`deps.rs:140-176`）按依赖边下钻，遇 `modules/` 下的路径即违规，**不做文本匹配**。

**两个绕过分别复现（均被拦住）**。检查入口是 `cargo test -p xtask deps::tests::the_real_workspace_has_no_reverse_dependency`
（`xtask` 暂无 `check-deps` 子命令，见仍存在的问题 #4）。

**绕过 ① —— workspace 继承写法**（`crates/platform-contracts/Cargo.toml` 加 `sandbox.workspace = true`，路径放到根 `Cargo.toml` 的 `[workspace.dependencies]`）：

```text
test deps::tests::the_real_workspace_has_no_reverse_dependency ... FAILED
平台 crate 不得依赖业务模块：["平台 crate `platform-contracts` 反向依赖了业务模块 `sandbox`（路径：modules/sandbox，依赖链：platform-contracts → sandbox）"]
TEST EXIT = 101
```

**绕过 ② —— 未列入清单的 crate**（`crates/platform-contracts-macros/Cargo.toml` 加 `sandbox = { path = "../../modules/sandbox" }`）：

```text
$ cargo tree -p platform-contracts-macros
platform-contracts-macros v0.1.0 (proc-macro) (crates/platform-contracts-macros)
└── sandbox v0.1.0 (modules/sandbox)

test deps::tests::the_real_workspace_has_no_reverse_dependency ... FAILED
平台 crate 不得依赖业务模块：["平台 crate `platform-contracts` 反向依赖了业务模块 `sandbox`（路径：modules/sandbox，依赖链：platform-contracts → platform-contracts-macros → sandbox）", "平台 crate `platform-contracts-macros` 反向依赖了业务模块 `sandbox`（路径：modules/sandbox，依赖链：platform-contracts-macros → sandbox）"]
TEST EXIT = 101
```

绕过 ② 连**间接链**（`platform-contracts → platform-contracts-macros → sandbox`）也一并报出 —— 这一点比首轮要求更强。

**逐字节还原**：每次植入前备份、植入后回写，`sha256sum` 与基线逐位一致（见 §还原与工作树状态）。

---

## §3 低 5 —— schema 与平台实现判定一致吗？**没有循环论证**

### 3.1 确认「不是用实现验实现」

`crates/platform-core/src/extension/manifest_schema.rs:244-442` 有一个**自写正则求值器** `mod pattern`
（注释明写「用一个**独立于平台实现**的求值器去读 schema 里的 pattern」）。判定链路是三条独立的腿：

```text
schema 里的 pattern  ──► 自写求值器 mod pattern（606 样本，与 Python re 0 分歧）
                          ↕ 断言「两边必须给出同一个判定」
平台实现 is_module_id ──► 硬编码样本表 MODULE_ID_SAMPLES（15 条，期望值写死在测试里）
```

即：**期望值是写死的常量表**，不是从实现里推导出来的 → 不构成循环论证。

### 3.2 但求值器本身对不对？—— 用独立引擎交叉验证

把 `mod pattern` **逐字节抽取**（`sed -n '244,442p'`，199 行，sha256 `affc2970…e444`）编译成独立 oracle，
与 **Python 标准 `re`** 引擎对同一批样本对拍：

```text
schema.id.pattern                = ^[a-z][a-z0-9]*(-[a-z][a-z0-9]*)*$
schema.dependencies[].pattern    = ^[a-z][a-z0-9]*(-[a-z][a-z0-9]*)*$
id == dependencies pattern?      = True

hand-written evaluator  vs  python re  (id pattern)      : 606 samples, mismatches=0
hand-written evaluator  vs  python re  (dependencies pat): 606 samples, mismatches=0
id pattern  vs  dependencies pattern (evaluator)         : 606 samples, mismatches=0
id pattern  vs  dependencies pattern (python)            : 606 samples, mismatches=0
```

→ 求值器**本身是正确的独立实现**，不是「自洽但错」。606 样本 = 任务要求的 12 条 + 32 条边界 + 562 条随机 fuzz。

### 3.3 平台实现 vs schema —— 用真实 Rust 实现（不是我的复刻）逐样本比对

平台侧判定**不走我重写的逻辑**，而是走真实代码路径：
造临时探针模块 → `cargo xtask check-modules` → `Manifest::validate()` → `is_module_id()`。
schema 侧用 Python `re`（标准引擎）读 `capabilities/schema/manifest.schema.json` 的 pattern。

```text
MISMATCH COUNT (platform vs schema): 0
```

覆盖：`id` 44 样本 + `dependencies[]` 44 样本 + 顶层 `name` 8 样本 + `permissions[].name` 8 样本 = **104 组 0 分歧**。

任务点名的 12 个样本逐条结果（三条腿完全一致）：

| 样本 | 平台实现 | schema（Python re） | schema（自写求值器） |
|------|---------|--------------------|--------------------|
| `sandbox` | 接受 | 接受 | 接受 |
| `demo2` | 接受 | 接受 | 接受 |
| `a-b1` | 接受 | 接受 | 接受 |
| `a1-b` | 接受 | 接受 | 接受 |
| `sandbox-1demo` | 拒绝 | 拒绝 | 拒绝 |
| `a-1` | 拒绝 | 拒绝 | 拒绝 |
| `sandbox-1` | 拒绝 | 拒绝 | 拒绝 |
| `Demo` | 拒绝 | 拒绝 | 拒绝 |
| `demo_x` | 拒绝 | 拒绝 | 拒绝 |
| ``（空串） | 拒绝 | 拒绝 | 拒绝 |
| `demo-` | 拒绝 | 拒绝 | 拒绝 |
| `a--b` | 拒绝 | 拒绝 | 拒绝 |

**首轮指出的 3 处 schema/实现不匹配已全部对齐**：
`id` pattern 收紧为 `^[a-z][a-z0-9]*(-[a-z][a-z0-9]*)*$`；
`dependencies[].items` 补上同款 pattern（原先 schema 完全没有 pattern）；
顶层 `name` 与 `permissions[].name` 补上 `minLength: 1` + `pattern: \S`。

**残留 1 处**（Unicode 空白码位）→ 见「仍存在的问题 #1」。

---

## §4 低 6 —— sandbox 排序白名单外字段返回错误

**实现**（`modules/sandbox/backend/src/repository.rs:147-151`）：

```rust
if !SORTABLE_FIELDS.contains(&spec.field.as_str()) {
    return Err(anyhow!("sort field {} is not sortable", spec.field));
}
```

`list()` 用 `sort_rows(&mut rows, &query.sort)?` 向上传播（`repository.rs:89`）—— 是**返回错误**，不再是 `continue` 静默跳过。

**实测**（`cargo test -p sandbox`）：

```text
test repository::tests::list_rejects_a_sort_field_outside_the_whitelist ... ok
test repository::tests::list_sorts_by_a_declared_field ... ok
test result: ok. 2 passed; 0 failed
```

**README 已写明**（`modules/sandbox/README.md:52-58`）：

```text
**排序字段不在白名单里 → 直接报错**，不静默忽略：
sort field unknown is not sortable
对应契约 C7「排序 … 未声明字段返回 400」。
```

`modules/sandbox/AGENTS.md:37-38` 也补了同款约束（「排序字段必须先声明」）。

---

## §5 提示 7 —— 契约 pin 已补回

`feature-meta/manifest.json` 实测：

```json
  "demand_dir": "BA/demands/REQ-003-extension-framework",
  "contracts": {
    "api-conventions": "v1"
  },
  "adrs": ["ADR-0001"],
  "delivered_by": "myg133-dev-1"
```

```text
contracts    = {'api-conventions': 'v1'}
adrs         = ['ADR-0001']
delivered_by = myg133-dev-1
提示7 三个 pin 齐全 = True
```

三个键齐全，与 REQ-002 同格式。首轮 QA 记录的「`claim` 工具重建文件时冲掉 pin」确属工具行为，本轮未复现（BA 已记入 roadmap R10）。

---

## 回归

| 项 | 命令 | 结果 |
|----|------|------|
| Rust 单测 + 契约测试 | `cargo test --workspace -- --test-threads=2` | **73/73 passed**，0 failed，3 doctest ignored，exit 0 |
| └ 分布 | — | `platform-core` lib 41 ｜ `platform-contracts` lib 8 ｜ `xtask` lib 18 ｜ `sandbox` lib 2 + `tests/contracts.rs` 4 |
| Clippy | `cargo clippy --workspace --all-targets -- -D warnings` | **0 告警**，exit 0 |
| 格式 | `cargo fmt --all --check` | **通过**，exit 0 |
| 前端单测 | `pnpm -r test` | **23/23**，exit 0（shell 4 ｜ auth/hooks/i18n/schema 各 3 ｜ ui 3 ｜ sdk 2 ｜ theme 2） |
| E2E | `bun run test:e2e` | **1/1**，chromium，exit 0 |
| `Cargo.lock` 新增第三方包 | 逐包比对 `04b57d3` vs `fd12156` | **107 → 107 包，ADDED = NONE，REMOVED = NONE**；唯一差异是 `xtask` 的依赖列表多一行 `serde`（`serde` 早已在 workspace） |
| `serde` 依赖写法 | `crates/xtask/Cargo.toml:14` | `serde.workspace = true`（**workspace 继承，非硬编码版本号**） |

**首轮基线对比**：首轮 64/64 → 本轮 73/73（`platform-core` 39→41、`xtask` 13→18、`sandbox` 0→2），
增量与本轮新增测试一一对应（结构级 manifest ×2、依赖方向 ×3、schema 一致性 ×2、排序白名单 ×2）。

### 通用清单

| 项 | 结果 |
|----|------|
| TODO / FIXME / `dbg!` / `println!` / `eprintln!` / `todo!` / `unimplemented!` | 改动文件内 **0 命中** |
| 硬编码密钥 / token / password | 改动文件扫描 **0 命中** |
| 新增依赖是否 workspace 继承 | ✅ `serde.workspace = true` |
| `Cargo.lock` 禁止清单（sqlx / reqwest / aws-sdk / opendal / jsonwebtoken / diesel / async-trait） | 本轮未新增任何包（107→107），清单不受影响 |

---

## 仍存在的问题

| # | 级别 | 描述 | 证据 | 建议 |
|---|------|------|------|------|
| **1** | **低** | **低 5「完全一致」在 2 个 Unicode 空白码位上仍未达成。** schema 的 `name` / `permissions[].name` 用 `pattern: \S`（ECMA-262 语义），平台用 `str::trim()`（Rust `char::is_whitespace`，Unicode White_Space 属性）。两者定义不同：<br>• **U+FEFF (ZWNBSP)**：ECMA-262 视为空白 → schema 拒绝；Rust `is_whitespace=false` → **平台接受**（平台更宽松）<br>• **U+0085 (NEL)**：ECMA-262 `\S` 匹配（非空白）→ schema 接受；Rust `is_whitespace=true` → **平台拒绝**（平台更严格）<br>方向相反、无安全洞（最坏是一个「看起来是空白」的模块名被平台接受），但「schema 与平台判定完全一致」这条目标未字面达成。 | Node（ECMA-262）实测：U+FEFF `\S` 不匹配；U+0085 `\S` 匹配。<br>Rust 实测：`'\u{FEFF}'.is_whitespace()=false`、`'\u{0085}'.is_whitespace()=true`。<br>平台侧探针：`name="\ufeff"` 的模块**未出现在失败清单里**（= 平台接受）；`name="\u0085"` → `manifest.json 不符合 Schema：name：must not be empty`（= 平台拒绝）。<br>Dev 已在 `feature-meta/verification-report.md` 风险 #10 自陈，但未修 | 要么把 schema 的 `\S` 换成平台同源的约束（例如 `pattern` 改用更精确的写法 / 或在 schema 里显式声明「以平台实现为准」的例外），要么在 `api-conventions/v1.md` 写明「名称非空以平台 trim 语义为准」。**影响面极小，可排入 REQ-004 批次** |
| **2** | **低** | **首轮低 4（C-2「登记了权限点」≠「代码里真的做了鉴权」）本轮未修** —— 不在本轮 5 个改动点范围内，Dev 未擅自扩范围（合规）。`crates/platform-contracts/src/manifest_scan.rs` 本轮 **0 改动**，仍是扫 `backend/**/*.rs` 的字符串字面量，因此「只在 `permission.rs` 里定义常量、服务端从不做鉴权判断」的权限点仍能全绿过 C-2。 | `git diff --stat 04b57d3 fd12156 -- crates/platform-contracts/src/manifest_scan.rs` → 空；`manifest_scan.rs:17,56` 仍是 `read_to_string` + `string_literals` | BA 定口径后再改（首轮结论）。Dev 的口径说明（走 AST 需 `syn`，低配机冷编译敏感）成立 |
| **3** | **提示** | **依赖方向检查把 `dev-dependency` 边也当违规**（不按 `kind` 过滤）。实测：`platform-contracts` 以 `[dev-dependencies]` 依赖 `sandbox` 同样被判违规。属于**比 ADR D7 更严**，不是安全洞；但若将来平台 crate 需要对业务模块写契约测试（dev-only），会被误报。 | 实测输出：`平台 crate \`platform-contracts\` 反向依赖了业务模块 \`sandbox\`（路径：modules/sandbox，依赖链：platform-contracts → sandbox）`（dev 依赖）；`cargo metadata` 输出中 `sandbox → platform-contracts` 明确带 `kind=dev`，而 `deps.rs` 未按 kind 过滤 | 可不改。若要改：按 `kind == "dev"` 分级为「警告」而非「违规」 |
| **4** | **提示** | **依赖方向门禁仍在单元测试里，没有 CLI 子命令。** `xtask` 只有 `check-modules` 一个子命令；D7 检查靠 `cargo test --workspace` 的 `deps::tests::the_real_workspace_has_no_reverse_dependency` 承担。CI 的 test job 会跑，**不会漏**，但无法像 `check-modules` 那样单独调用/在本地快速验。 | `crates/xtask/src/main.rs:13-19`（只有 `Command::CheckModules`）；`cli.rs:13-18` | 本期可接受（Dev 已自陈风险 #8）。若要提升门禁可见性，后续加 `xtask check-deps` 子命令 |
| **5** | **提示** | **过滤字段白名单外仍被静默忽略**（本轮只处理了 QA 列出的「排序」，`matches_filter` 对白名单外过滤字段仍按「不参与过滤」处理）。与 C7「未声明字段返回 400」仍有差距，但首轮未列此项。 | `modules/sandbox/backend/src/repository.rs:166-199`（`matches_filters` / `matches_filter` 无白名单报错分支）；Dev 自陈风险 #9 | 若要把过滤也对齐 C7，需另开一项 |
| **6** | **提示** | **`migrations/` 仍是单份 8 行 README 占位**（首轮提示 8，本轮未改）。`check-modules` 的「非空」规则被 README 满足。 | `modules/sandbox/migrations/README.md`（8 行）；`crates/xtask/src/modules.rs:146-151`（`is_empty_dir` 只判目录项数） | 本期（无 DB 层）可接受。REQ-006/007 落地 DB 层时升级为「至少一个 `*.sql`」 |

### 已关闭（首轮问题的当前状态）

| 首轮 # | 级别 | 问题 | 状态 |
|--------|------|------|------|
| #1 | 中 | `check-modules` 结构级 manifest 违规漏报 | ✅ **已修并复验**（§1） |
| #2 | 中 | 依赖方向检查可被绕过 | ✅ **已修并复验**（§2） |
| #3 | 低 | 扩展点冻结签名的 dyn 兼容性变更未登记 | ✅ **已由 BA 关闭** —— `BA/demands/REQ-003-extension-framework/design-summary.md:48-58` 已补注记：「本期起 5 个 trait 为 RPITIT 形态…不可用于 trait object…异构汇聚延后到 REQ-015 / REQ-017 决策」（该文件不在本工作树，本轮只读核对） |
| #5 | 低 | schema 与平台实现判定不一致 | ⚠️ **基本修复**，残留 1 处 Unicode 码位差异（仍存在的问题 #1） |
| #6 | 低 | sandbox 排序白名单外字段被静默忽略 | ✅ **已修并复验**（§4） |
| #7 | 提示 | `feature-meta/manifest.json` 的 pin 被冲掉 | ✅ **已修并复验**（§5）；归属说明问题随本次补回一并消失 |
| #4 | 低 | C-2 无法区分「登记」与「鉴权」 | ⬜ **未修**，见仍存在的问题 #2 |
| #8 | 提示 | `migrations/` 只有 README 占位 | ⬜ **未改**，见仍存在的问题 #6 |

---

## 未验证

| 项 | 状态 |
|----|------|
| **CI 六个 job 实跑** | **未验证**。本仓库无 remote，GitHub Actions 跑不起来。本地已跑等价命令（`cargo test --workspace`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo fmt --all --check`、`pnpm -r test`），CI 的 `cargo test -p sandbox` 一步已含在 workspace 测试内 |
| **覆盖率数字** | **未验证**。本机无覆盖率工具，按低配机约束未 `cargo install`。角色规范的「覆盖率 ≥ 80%」未量化，以「每条 AC 均有自动化测试 + 破坏性验证」替代 |
| **圈复杂度 / 认知复杂度** | **未验证**。本机无 `lizard` / `complexipy`，未安装 |
| **冷编译峰值内存 / 耗时** | **未验证**。按要求未 `cargo clean`、未做 RSS 采样 |
| **C-3 的真实多租户数据库行为** | **不适用**（BA 已澄清本期不连库）。C-3 验的是契约语义，非真实库口径 |
| **C-4 的真实事务回滚** | **不适用**（本期无 DB 访问层） |
| **REQ-002 的数据库集成脚本 `scripts/test-integration.sh`** | **未运行**（需 podman + `psql`，本机无 psql 客户端）。与本需求无耦合 |
| **`#[platform_module_contract]` 的错误路径** | **未验证**。宏对「传参」「非 mod item」「缺 `contract_subject()`」的处理代码存在但无对应测试，本轮未额外造违规（不在验收要求内） |
| **`inventory` 跨平台行为** | **未验证**。依赖 ELF `.init_array`，本仓库只在 Linux 跑 |
| **`api-conventions/v1.md` 的 C7 措辞与 sandbox 新行为是否逐字对齐** | **未验证**。该文件不在本工作树；本轮只核对了 sandbox README / AGENTS.md 的表述与实现一致 |

---

## 还原与工作树状态

- 本轮对已跟踪文件的变异共 **5 次**（绕过①×1、绕过②×1、dev-dep 附加探测×1；每次都连带 `Cargo.lock`），
  每次均**备份 → 植入 → 回写 → `sha256sum` 校验 → `git status --porcelain` 复核**。
- 终态 sha256 与基线**逐位一致**：

  ```text
  9ecc4a8d88e7d8bd9362b342b1d2f6dcc881c1c5af407126659256443e8afff6  Cargo.toml
  996b8e6bdedb3643161de28d80250da0933fc779d21e3762e23b8a085bf847d9  crates/platform-contracts/Cargo.toml
  e2ef182e0998ed90314846c5fc5e895ddb1357a8ea312242053b15e040de0d24  crates/platform-contracts-macros/Cargo.toml
  564f6b6f706791549590fb5aa0eda757611973d0bff77ffe99422f3e56d19bc6  Cargo.lock
  ```

- 临时探针目录（`manifest` 结构违规 ×4、schema 样本 ×104、Unicode 空白 ×3）与临时 oracle/脚本
  全部建在 `.local/qa-recheck/`（已被 `.gitignore` 覆盖），**已整目录删除**；`.local/` 现为空。
- **被审实现代码与配置零改动**。
- 结束时 `git status --porcelain`：

  ```text
   M feature-meta/qa-report.md            <- 首轮 QA 报告（本轮开始前即存在，未提交）
   M feature-meta/qa-report-recheck.md    <- 本文件（槽位覆写）
  ```

  关于第二条：`feature-meta/qa-report-recheck.md` 原为 **REQ-002** 的复核轮报告（tracked，blob sha256 `59a2595d…c9a6e`）。
  按任务指定路径写入本轮报告属**槽位覆写**，与本仓库既有约定一致（首轮 QA 覆写 `qa-report.md` 是同一模式）。
  **REQ-002 的复核轮报告未丢失**：完整保留在提交 `3ff22fb` 与 `develop:feature-meta/qa-report-recheck.md`，可随时取回。
- 未改动任何被审实现代码，未 commit / push / merge。

---

## 给 BA 的处置建议

1. **可以合** —— 本轮 5 个改动点经独立造违规复现确认全部生效；两条「中」级门禁盲区已封堵；
   回归全绿、`Cargo.lock` 零新第三方包。
2. **合前可选**处理仍存在的问题 #1（U+FEFF / U+0085 两个码位的 schema/平台分歧）——
   一行 schema 调整或一行契约注记即可，但影响面极小，不阻塞。
3. **BA 侧待办**：仍存在的问题 #2（C-2「登记 ≠ 鉴权」口径）—— 首轮已转给 BA 定口径，Dev 未擅自扩范围，符合预期。
4. **REQ-004 批次候选**：仍存在的问题 #5（过滤字段白名单）、#6（`migrations/` 升级为「至少一个 `*.sql`」）。
