# REQ-012 修复轮 —— 独立复核报告

- **需求**: REQ-012「元数据驱动渲染」（AC-S-076 ~ AC-S-087，rev 3）
- **复核对象**: `f9e1c30` `[Dev] REQ-012 修测试资产：夹具授权、用例独立、AGENTS.md 一致`
- **裁定依据**: `qa-fail-ruling.md`（2 HIGH + 1 MED + 6 LOW）、`decisions.md` **D108**
- **复核者**: QA（**未参与修复**；Dev / 上一轮 QA 的数字一律重跑）
- **分支 / worktree**: `feature/REQ-012` @ `/home/myg133/Projects/system_seed/feature-REQ-012`
- **基线**: `develop` @ `a7f7579`
- **时间**: 2026-11-19
- **方法**: 真库（`base_test`）+ 真进程（`base serve` :8090）+ 真浏览器（Playwright chromium）+ 源码/夹具变异（**5 处，全部逐字节还原**）
- **注**: 本文件承接 REQ-011 的同名复核报告（原文保留在 git 历史，替换方式与本仓既有做法一致）

---

## 1. 结论

**PASS**

上一轮的 **2 HIGH + 1 MED + 4 个待修 LOW 全部修好**，且每一处都用「造违规 / 变异」验证过是**真修**而不是「看起来修了」：

| 项 | 结果 | 最关键的一条证据 |
|---|---|---|
| **H-1** 自带夹具下 E2E 全绿且 0 skipped | ✅ | 正序 **17 passed / 0 skipped / 0 failed**；删掉夹具授权后 **6 failed / 0 skipped** |
| **H-2** 用例独立 | ✅ | 正序全绿 + **反序逐条 17/17 全绿**；把零代码用例改成「依赖表非空」→ 空表下**必红** |
| **M-1** AGENTS.md 与 deps.rs 一致 | ✅ | 「不得出现 `<td`」、白名单文字一致；`platform-core → sandbox` 变异 → `check-deps` **exit 1** 且点名，`platform-server` **未被误报** |
| **L-1** 能力索引按 D106 更新且是生成物 | ✅ | 改 `capabilities/index.json` → `--check` **失败**（`E_CAPABILITIES_OUTDATED`）|
| **L-2** 更新方法记忆 | ✅（见 L-2 的边界说明） | 首次编辑 `PATCH 405 → PUT`，同页第二次编辑**只发 PUT** |
| **L-4** thead 里不再有 `<td>` | ✅ | 真 DOM：`thead` 内 8 个单元格**全是 `<th>`** |
| **L-5** 校验闸门不再假绿 | ✅ | `validateForm` 恒返回 `{}` → `apps/shell` **2 failed**（修复前是 10 passed 全绿） |

**未发现被弄坏的已通过项**（F 节 12 条逐项复验，全部通过），**未发现新引入的 HIGH / MED**。

有 5 条 **LOW / 观察项**（第 5 节），均不阻塞合并，其中 3 条是修复轮之前就存在的。

---

## 2. H-1 / H-2 / M-1 / L 逐项复核

### H-1 ★ 自带夹具下 E2E 全绿且 0 skipped —— 通过

**原问题**：`seed-e2e.sh` 只授 `rbac:*`，列表接口 403；spec 把 403 当「环境没准备好」`test.skip` → 6 条全跳、exit 0（假绿）。

**本轮实测**（严格按复现顺序）：

1. `./scripts/test-integration.sh` 把库搞脏 → 退出 0（33 套、618 passed、容器 2→2）
2. `./scripts/seed-e2e.sh` **连跑两遍** → 两次退出 0；核对输出均为
   `u-perm-read user_rows=1 granted=2` / `u-perm-write user_rows=1 granted=5` / `u-perm-none=1 granted=0`
   → **用户数不翻倍、权限点按清单写回**
3. 起 `base serve`（:8090，应用角色 `base_app`）→ `/health/live` 200
4. `BASE_E2E_URL=http://127.0.0.1:8090 npx playwright test e2e/`

**结果（JSON reporter 的 stats，不是我数人头）**：

```
"expected": 17, "skipped": 0, "unexpected": 0, "flaky": 0
```

| 指标 | 上一轮 | 本轮 |
|---|---|---|
| passed | 11（schema 6 条全 skip） | **17** |
| **skipped** | **6** | **0** |
| failed | 0（假绿） | 0 |

代码层证据：`e2e/schema-driven.spec.ts:133-143` 的 `requireMeta()` 只在 **404** 时 `test.skip`；
`:171-176` 的 `expectListReadable()` 对 403 直接 `expect(status).toBe(200)`，错误文案就写着
「403 表示夹具缺 sandbox:demo:* 授权……不是环境前置（D108）」。
全仓 `grep -rn "test.skip|test.fixme|test.fail" e2e/` → **只剩** 这一处 404 skip。

`seed-e2e.sh` 新增的授权（`e2e-role-read` + `sandbox:demo:read`；`e2e-role-write` + `read/write/secret`）
与 spec 头注释的夹具契约一致。

### H-2 ★ 用例独立性 —— 通过

**原问题**：空态用例要求 `demoNote` 表空、零代码用例要求它非空 → 任何初始状态必有一条红。

**本轮实测**：

| 场景 | 结果 |
|---|---|
| 正序全量 | 17/17 绿（上面那次） |
| **反序逐条**（17 条从最后一条到第一条，每条独立浏览器上下文；Playwright 1.58 无 `--shuffle`，用 `file:line` 单跑） | **17/17 绿** |
| 未变异 · 空表下单独跑零代码用例 | **绿**（它自己造数、`finally` 清理） |
| 变异：零代码用例**不造** `demoNote` 数据，空表下单独跑 | **红**（`1 failed`） |

数据前置的设计我已逐个读源码确认：表头用例自造 3 条、游标分页自造 21 条、字段权限用例自造 1 条、
零代码两个实体各造 1 条、CRUD 走真实 UI 并在 `finally` 按 marker 回收、空态用例**先 `clearEntity()` 再断言空**。

> 注：E2E spec 头部注释写「（`--shuffle` 可验）」，但 `npx playwright test --help` **没有** `--shuffle`
> （见第 5 节 LOW-3）。这是注释用语问题，不影响用例独立性本身。

### M-1 ★ AGENTS.md 与 deps.rs 一致 —— 通过

**文档面**（`AGENTS.md:85-94`）：
- 原文「平台 crate 一旦反向依赖业务模块即失败」已改为「……（**装配层 `platform-server` 除外**）」
- 新增例外段 + 双列表格，明确列出 `platform-core / platform-db / platform-iam / platform-org /
  platform-rbac / platform-schema / platform-config / platform-contracts*` ❌、`platform-server` ✅
- **逐字对应**：`crates/xtask/src/deps.rs:63` `ASSEMBLY_CRATES: [&str; 1] = ["platform-server"]`；
  表里点名的 9 个库 crate 与 `crates/` 下实际成员一致（含 `platform-schema`、`platform-config`）

**行为面（造违规，不是只对字面）**：给 `crates/platform-core/Cargo.toml` 加
`sandbox = { path = "../../modules/sandbox" }` →

```
依赖方向校验失败：错误码 E_DEPENDENCY_DIRECTION
  ✗ 平台 crate `platform-core` 反向依赖了业务模块 `sandbox`（依赖链：platform-core → sandbox）
  ✗ platform-config / platform-contracts / platform-db / platform-iam / platform-org / platform-rbac / platform-schema / xtask 同样被点名
（退出码 1）
```

→ 说明例外是**白名单式**（只放行 `platform-server`），不是把检查关掉；
`platform-server` 自身依赖 sandbox 却**没有**被点名，白名单精确到 crate。

已 `cp` 还原，`sha256sum -c` 通过，`cargo xtask check-deps` 复归退出 0。

### L-1 能力索引按 D106 更新且是唯一来源 —— 通过

- 文案已是 D106 语义：「`@base/ui` 的 Table 是**页码分页（依赖 total）**……游标分页没有 total……
  请用 `apps/shell` 的 `SchemaTable`」，`capabilities/index.json` 与
  `crates/xtask/src/catalog.rs:100` **逐字一致**。
- **唯一来源验证**：把 `index.json` 里那句话加一个尾随空格 → `cargo xtask capabilities --check`
  **退出 1**：`能力索引校验失败：错误码 E_CAPABILITIES_OUTDATED / ✗ 内容不一致：capabilities/index.json`。
  还原后 `--check` 退出 0（`14 个文件与源码一致`）。→ 手改会被覆盖，`catalog.rs` 是真来源。

### L-2 更新方法记忆 —— 通过（有边界，见下）

真浏览器探针（真服务 + 真登录 + 真 UI），记录写请求序列：

```
create : ["POST /api/iam/login", "POST /api/sandbox/demo-items"]
edit#1 : ["PATCH /api/sandbox/demo-items/item-…", "PUT /api/sandbox/demo-items/item-…"]
edit#2 : ["PUT /api/sandbox/demo-items/item-…"]
```

- **同一个编辑动作只有一个「成功的写请求」**（PUT 200；PATCH 是 405，服务端未处理）。
- **首次探测后固定 PUT**：同页第二次编辑只发一个 PUT。
- 单元测试同向覆盖（`apps/shell/test/schema-api.test.ts:163-178`：`['PATCH','PUT','PUT']`）。

⚠️ **边界**：缓存在**模块级**，页面 reload 即失效 —— 每个新页面会话的**第一次**编辑仍是
`PATCH 405 → PUT` 两次请求。这与裁定原文「记住上次成功的方法（**首个 405 之后**就一直用 PUT）」
一致，但若期望「任何一次编辑都只有一个 HTTP 写请求」，本轮未达到（见 LOW-4）。

### L-4 `thead` 里不再有 `<td>` —— 通过

真 DOM 取证（真浏览器、有数据时）：

```html
<th>ID</th><th>标题</th><th>金额</th><th>已付款</th><th>到期日</th><th>负责人</th><th>内部备注</th>
<th scope="col" class="base-schema-actions"></th>
```

→ `thead` 内 **0 个 `<td>`**，行操作列是 `<th scope="col">`。组件单测另有一条回归断言
`thead` 不得含 `<td`（`schema-table.test.ts:164-169`）。

⚠️ 观察（LOW-2）：`scope="col"` 只加在**行操作列**，7 个数据表头是裸 `<th>`。
裁定点名的缺陷（`td` 违反 content model）已修；若把「表头必须是 `<th scope="col">`」按字面套到所有列，
则只完成了一部分。

### L-5 校验闸门不再假绿 —— 通过

**变异**：`packages/schema/src/index.ts` 的 `validateForm()` 开头插入 `return {};`（恒通过）。

```
apps/shell: Tests  2 failed | 94 passed (96)
  × 提交闸门 … 必填字段留空 → 不发出任何请求
  × 提交闸门 … number 超出 min/max → 不发出任何请求
```

对比上一轮「同一变异下 10 passed 全绿」→ **假绿已消除**。已还原（`sha256 = bd73c1f5…` 与变异前一致）。

---

## 3. 是否弄坏已通过的项（F 节逐项）

| AC | 复验方式 | 结果 |
|---|---|---|
| **AC-S-079** 元数据过滤 | 真机：读用户 `fields = [id,title,amount,paid,dueAt,ownerId]`（无 `secret`）；顶层键 `[api,entity,fields,label,module]`；`/"secret"\s*:/` 不匹配；`内部备注` 不出现；写用户 7 个字段 | ✅ |
| **AC-S-080** 数据投影 | 真机造数后：写用户 `hasOwnProperty('secret')===true`（值 `库值-QA080`）；读用户 `===false`、`secret` 值 `undefined`（**不是 null**）；用 RBAC API 撤销 `sandbox:demo:secret` → **下一个请求即无该键**；恢复 → **下一个请求即有** | ✅ |
| **AC-S-081** 契约 C-6 | `cargo test -p platform-contracts --test c6_schema_projection` → **5 passed**；`cargo test -p sandbox --test contracts` → **6 passed**（含 `contract_c6_schema_projection_is_used`）；负向用例 `a_module_without_a_project_call_site_fails_and_names_module_entity_field` ok | ✅ |
| **AC-S-082** 渲染 | E2E 表头逐项 equal、空态 0 个 `thead`、游标分页请求带 `cursor` 且第二页不同；单测 `boolean→是/否`、`date→2026-11-19`、`number→12.5`、空值→空串 | ✅ |
| **AC-S-083** 表单 | 真浏览器探针：必填留空 → **0 个 POST**、1 条字段旁错误「标题不能为空」、`aria-invalid=true`、`aria-describedby=schema-error-title`；`amount=-1`（min:0）→ **0 个 POST**、错误在 number 字段、`aria-invalid=true`；合法 → **恰好 1 个 POST**。控件类型由组件单测覆盖（text/number/date/checkbox/reference 只读） | ✅ |
| **AC-S-084** 前端跳过无权字段 | E2E「字段级权限」用例通过：只读用户表头 = 其元数据 label、`body.innerText` 不含 `内部备注` | ✅ |
| **AC-S-085** 零代码 | `grep -rn "demoNote\|demo-note" packages/schema/src apps/shell/src` → **无匹配**；`git diff 737b384 8ce359e -- packages/schema/src apps/shell/src` → **空**（加 demoNote 那次提交零前端改动）；`packages/schema/src/index.ts` 哈希仍 `bd73c1f5…` | ✅（哈希口径见下） |
| **AC-S-086** CRUD 闭环 | E2E「CRUD 闭环」通过；另用真浏览器探针复跑：新建 → 编辑 → 删除后 `rows=0`；全程 API 调用仅 `/api/config/branding`、`/api/iam/login`、`/api/rbac/me/permissions`、`/api/schema/sandbox/demoItem`、`GET/POST/PATCH/PUT/DELETE /api/sandbox/demo-items…`，**无未声明路径** | ✅ |
| **AC-S-087** 容错 | 单测：`type=currency` → 降级文本框 + `console.warn` **恰好 1 次**且含类型名；空值不出现 `undefined/NaN`；E2E 空态文本不含 `undefined/null/nan` | ✅ |
| **AC-S-076** 校验 | `cargo test -p platform-core --lib` → **96 passed**，含 `two_violations_are_both_reported_aggregation_is_kept`（断言 `issues.len()==2` 且分别点名 `fields[2].type` / `fields[3].key`）、`an_unknown_type_…`、`empty_fields_are_reported` | ✅ |

**AC-S-085 的「哈希未变」口径说明（必须写清）**：
- 有意义的口径 = 「加 `demoNote` 这一步没有改前端」→ `737b384..8ce359e` 前端源码 **零 diff**，成立。
- 若机械地拿 **HEAD 与 `737b384`** 比，`apps/shell/src/**` 已变 —— 但那是**修复提交**为了 L-4/L-5
  改的 `SchemaTable.svelte` / `SchemaForm.svelte` / `schema-api.ts` / 新增 `schema-submit.ts`，
  与 `demoNote` 无关（`packages/schema/src/**` 仍逐字节未变）。零代码的更强判据（源码不含实体键）通过。

---

## 4. 新引入的问题

**没有 HIGH / MEDIUM。** 修复轮改动的 14 个文件里，未发现回归或语义破坏：

- `schema-api.ts` 的方法缓存是模块级、按 `api.update` 模板分组，**不改请求语义**（首次仍 PATCH、
  405 才退回 PUT，非 405 不重试 —— 与修复前一致）。
- `schema-submit.ts` 只是把 `prepareSubmit` 的调用从组件内联搬到纯函数；`SchemaForm` 的
  `handleSubmit` 仍把 `errors` 回写、`onSubmit` 仍只在通过时调用（真机 0/0/1 已验证）。
- `SchemaTable` 的 `<td>`→`<th scope="col">` 只影响表头语义；E2E 的 `headerTexts()` 与组件单测的
  表头 helper 同步改成 `th:not(.base-schema-actions)`，两侧口径一致。
- `seed-e2e.sh` 新增的 3 个权限点与 spec 的假设一致；两次连跑幂等。

5 条 LOW / 观察项（均不阻塞）：

| # | 级别 | 问题 | 证据 |
|---|---|---|---|
| **LOW-1** | 低（**修复轮之前就存在**） | `seed-e2e.sh` 的「结果自证」只硬断言**用户行数** `3 1 1 1`，不断言权限点数量 —— 授权写错时脚本仍打印 `✓ 夹具就绪` | 我删掉 `sandbox:demo:read` 后重播种，脚本 **退出 0**，表格里 `granted_permissions` 已变成 2→1。D108 让 E2E 兜住了（6 failed），所以不构成假绿，但脚本自称的「别把播了但不对当成功」名不副实 |
| **LOW-2** | 低 | `scope="col"` 只在行操作列，7 个数据表头是裸 `<th>` | 真 DOM 输出见 §2 L-4 |
| **LOW-3** | 低（文档） | E2E spec 注释写「（`--shuffle` 可验）」，Playwright 1.58 **没有** `--shuffle`（`--help` 无此项） | `npx playwright test --help \| grep -i shuffle` → 无输出 |
| **LOW-4** | 低 | L-2 的缓存在**每次页面加载**后失效：新会话的第一次编辑仍 `PATCH 405 → PUT` | §2 L-2 探针的 `edit#1` 序列 |
| **LOW-5** | 低（**修复轮之前就存在**） | AC-S-086 步骤 8「全程无未声明请求」没有进 E2E 断言，只由人工探针覆盖 | `e2e/schema-driven.spec.ts` 的 CRUD 用例只断言 UI 闭环 |

> **非 Dev 项**：裁定 L-3（只读用户仍见「新建」按钮）要求「**登记技术债**」，
> 但 `BA/plan/tech-debt.md` 里查无此项（TD-001…TD-011 均与它无关）。这是 BA 的动作项，
> 不计入本次修复轮判罚，但**尚未落账**。

---

## 5. 变异验证表

| # | 变异 | 预期 | 实测 | 还原 |
|---|---|---|---|---|
| M1 | `scripts/seed-e2e.sh` 删掉两条 `sandbox:demo:read` 授权后重播种 | 403 必须 **fail**，不许 skip | **6 failed / 11 passed / 0 skipped**，exit 1；失败文案即「demoItem 列表返回 403……不是环境前置（D108）」 | `sha256sum -c` 通过（`8f64dd6f…`），`git diff --exit-code` 干净；重新播种后 write 恢复 5 个权限点 |
| M2 | `e2e/schema-driven.spec.ts` 的零代码用例不再自造 `demoNote` 数据 | 空表下反序必红 | 空表（`count=0`）下单独跑 → **1 failed**；未变异时同状态 **1 passed** | `sha256sum -c` 通过（`0f9d3b29…`） |
| M3 | `packages/schema/src/index.ts` 的 `validateForm` 恒返回 `{}` | `apps/shell` 必红 | **2 failed / 94 passed**（修复前同变异 10 passed 全绿） | `sha256sum -c` 通过（`bd73c1f5…`） |
| M4 | `crates/platform-core/Cargo.toml` 增加 `sandbox` 依赖 | `check-deps` 必失败且点名，`platform-server` 不得被误报 | **exit 1**，`E_DEPENDENCY_DIRECTION`，逐条点名 `platform-core → sandbox` 等 9 个 crate，**无 `platform-server`** | `sha256sum -c` 通过（`a0c1c4ac…`）；`check-deps` 复归退出 0 |
| M5 | `capabilities/index.json` 内容加一个尾随空格 | 生成物校验必失败 | **exit 1**，`E_CAPABILITIES_OUTDATED / 内容不一致` | `sha256sum -c` 通过（`9caccec8…`）；`--check` 复归退出 0 |

**最终工作区状态**：`git status --porcelain` 仅 `M feature-meta/qa-report.md`
（**上一轮遗留的未提交改动，非本次复核产生**）；所有变异文件哈希与变异前逐字节一致。

---

## 6. 没验到的（明确边界）

1. **没有在 CI 里跑**：本仓无 remote（TD-006），全部是本地真机执行。
2. **D108 的 500 分支未造**：我把列表打到 403 验证了「不许 skip」，但没有人为制造 500
   （`expectListReadable` 对 500 与 403 走同一句 `toBe(200)`，代码路径相同，未单独触发）。
3. **元数据 404 → skip 分支未造**：需要临时隐藏一个实体（改 manifest + 重启），
   属被测系统的构造性变更，未做。代码上是显式 `test.skip(true, …)`，与裁定表一致。
4. **AC-S-086 步骤 8 的白名单**由我的临时探针覆盖，**未进仓库的 E2E**（LOW-5）。
5. **跨租户/RLS 层**未重新验（本轮改动未触及后端）。
6. **L-2 的缓存跨页面持久性**只在单页会话内验证；跨 reload 的行为由代码阅读确认（见 LOW-4）。

---

## 7. 实跑数字

**环境**：rustc/cargo 1.98.1 · node v26.13.1 · pnpm 12.19.0 · podman `base-postgres`(17)/`base-redis`
· `CARGO_TARGET_DIR=/home/myg133/Projects/system_seed/code/target` · 服务 :8090（`base_app` 连 `base_test`）

| 命令 | 结果 |
|---|---|
| `./scripts/test-integration.sh` | **exit 0**；33 个套件 **618 passed / 0 failed / 0 ignored**；容器 2 → 2 |
| `./scripts/seed-e2e.sh` ×2 | 两次 **exit 0**；`3 1 1 1`（用户不翻倍）；read=2 / write=5 权限点 |
| `pnpm -r check` | 0 errors / 0 warnings（svelte-check） |
| `pnpm -r build` | 成功（`dist/assets/index-*.js` 82.68 kB） |
| `pnpm -r test` | **240 passed**：i18n 3 · hooks 17 · sdk 34 · schema 38 · theme 12 · auth 26 · ui 14 · **shell 96** |
| `pnpm exec eslint .` | **exit 0** |
| `npx prettier --check .` | **exit 0**（All matched files use Prettier code style） |
| `cargo clippy --workspace --all-targets -- -D warnings` | **exit 0** |
| `cargo xtask capabilities --check` | **exit 0**（14 个文件与源码一致） |
| `cargo xtask check-deps` | **exit 0** |
| `cargo test -p xtask --all-targets` | **51 passed / 0 failed** |
| `cargo test -p platform-core --lib` | **96 passed**（含双违规 `issues.len()==2`） |
| `cargo test -p platform-schema` | **11 + 9 passed** |
| `cargo test -p platform-contracts --test c6_schema_projection` | **5 passed** |
| `cargo test -p sandbox --test contracts` | **6 passed** |
| `playwright test e2e/`（正序，首次） | **17 expected / 0 skipped / 0 unexpected**（27.5s） |
| `playwright test e2e/`（还原后最终复跑） | **17 expected / 0 skipped / 0 unexpected**（26.0s） |
| 反序逐条（17 条，`file:line` 单跑） | **17/17 passed** |
| D108 变异（M1） | 6 failed / 11 passed / **0 skipped** |
| 零代码依赖变异（M2） | 1 failed（空表） |
| `validateForm` 变异（M3） | apps/shell 2 failed / 94 passed |

**总判定：PASS。** H-1 / H-2 / M-1 / L-1 / L-2 / L-4 / L-5 全部实测通过，
无 HIGH / MEDIUM 新问题，已通过项未被弄坏；5 条 LOW 不阻塞合并（其中 3 条修复前即存在）。
