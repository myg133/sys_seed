# REQ-016 独立复核报告（Verifier，非 BA）

- **复核对象**：`feature/REQ-016` @ `5aba742`（基于 `develop@aa5b305`），worktree `/home/myg133/Projects/system_seed/feature-REQ-016`
- **被复核报告**：`BA/feature-meta/req-016-final-verification.md`（§4 的 11 行「判据落点」）
- **复核者**：verifier 子 Agent（独立，非 BA 自查）
- **复核时间**：2026-11-21 21:12 ~ 21:5x CST
- **只读纪律**：未修改任何源码 / 测试 / 报告。变异一律「备份副本 + `touch` 还原」，DB 变异用 SQL 精确还原。

---

## 1. 复核范围与方法

### 1.1 实际执行的命令

```bash
export XDG_RUNTIME_DIR=/run/user/$(id -u)
export CARGO_TARGET_DIR=/home/myg133/Projects/system_seed/code/target
export DATABASE_URL_TEST="postgres://base:base@127.0.0.1:5432/base_test"

cargo test -p platform-app    --all-targets -- --test-threads=1 --include-ignored
cargo test -p platform-server --all-targets -- --test-threads=1 --include-ignored
cargo test -p platform-audit  --all-targets -- --test-threads=1 --include-ignored
cargo test -p platform-oauth  --all-targets -- --test-threads=1 --include-ignored
pnpm --filter @base/shell test
cargo xtask capabilities --check
npx playwright test e2e/apps.spec.ts        # 播种后、无并发干扰
```

DB 访问（本机无 `psql`）：`podman exec base-postgres psql -U base -d base_test -tAc "…"`

### 1.2 §4 逐行核对方法（每一步都做了）

对 11 行里的每一个 `file:line`：
1. **`sed -n "${n}p"` 打开那一行** —— 确认该行**真的是**报告点名的那个函数 / `it()`；
2. **读完整函数体** —— 确认报告描述的那个 `assert!` / `assert_eq!` / `expect(...)` **真的存在**；
3. **判断区分力** —— 用变异或逻辑分析确认「错误实现会得到错误结果」。

### 1.3 变异抽查（我自己做的，不是复述 BA 的）

| # | 变异位置 | 变异内容 | 结果 |
|---|---|---|---|
| **V1** | `crates/platform-app/src/http.rs:223` | 注释掉 `ensure_fresh` 调用 | ✅ 红（`a_failing_permission_refresh_fails_the_protected_endpoint_closed`） |
| **V2** | `crates/platform-app/manifest.json` | `app.workspace.maxApps.scope` `platform`→`tenant` | ✅ 红（`left: Tenant / right: Platform`） |
| **V3** | DB 约束 `platform_app_client_id_fkey` | `ON DELETE CASCADE`→`RESTRICT` | ✅ 红 ×2（`23503` 外键违反） |
| **V4** | `apps/shell/src/lib/nav.ts:77` | 让 `external` 也产出路由 | ✅ 红 ×3（含「external → `[]`」） |
| **V5** | `crates/platform-app/src/authz.rs:41` | `require_permission("字面量")` 改成变量 | ✅ 红（C-5 两条违例） |
| **V6** | `crates/platform-audit/src/sink.rs` | 生产代码塞裸 INSERT（守卫敏感度） | ✅ 红（报 `sink.rs:143`） |
| **V7** | `crates/platform-app/src/store.rs:134` | INSERT 时把 `description` 丢成 NULL | ❌ **全链路无覆盖（见 §9 FINDING-1）** |
| **V8** | `crates/platform-app/src/model.rs` | `view_of` 里 `description: None` | ❌ **同样无覆盖** |

**环境恢复（逐字节校验）**：

```
OK src/http.rs / model.rs / authz.rs / store.rs / manifest.json / nav.ts / sink.rs
DB: platform_app_client_id_fkey confdeltype = 'c' (CASCADE)
grep -rn MUTATION → 空
```
---

## 2. 【核心】终版报告 §4 那 11 行的逐行核对

> **判定口径**：`指的位置存在` × `断言存在` × `有区分力` 三者全为 ✅ 才算通过。
> 我**逐行打开了源码**，不是看报告措辞。

### 行 024 — 撤销后内省立即失效（回归）

| 检查项 | 结果 |
|---|---|
| 指的位置存在 | ✅ `crates/platform-oauth/tests/oauth_http.rs` 存在；撤销用例在 `:2223-2232` |
| 断言存在 | ⚠️ **部分**：`assert_eq!(body["active"], false)` 存在（`:2232`）<br>❌ **`exp - iat == access_token_expires_in` 这个断言不存在**（全文件 grep 无该模式）<br>实际只有 `assert_eq!(body["access_token_expires_in"], 3600)`（`:433`），是元数据的值，**不是** `exp - iat` 的比对 |
| 有区分力 | ✅ 对「撤销后 `active=false`」有；❌ 对「令牌有效期自洽」无（那句是写出来的，不是测出来的） |

**结论**：⚠️ **措辞超出实际断言**。核心判据（`active=false`）真实；但「`exp - iat == access_token_expires_in`」在测试源码里**不存在** —— 这与 REQ-022 的失败模式同型，虽然本行不影响 AC-024 的主要结论。

### 行 156 — 注册表 CRUD + 引用不复制

| 检查项 | 结果 |
|---|---|
| 指的位置存在 | ✅ **9 个引用全部精确命中、零行号漂移**：`:44` `an_app_can_be_created_and_read_back_verbatim`、`:132`、`:171`、`:213`、`:277`、`:347`、`:403`、`:460`、`:500`（函数名逐字一致） |
| 断言存在 | ✅ 201（`:67`）、`clientId/displayName/category/iconUrl/launchUrl/sortOrder/status/createdBy/scopes/createdAt` 逐字回读（`:70-100`）；PATCH 只改传入字段（`:160-165`）；DELETE 204→404→404（`:194-207`）；**恰好 12 列**双向相等（`:242`）；6 个 client 列不得出现（`:245-257`）；无 scope 列（`:263`）；409 双路径 + 唯一组合阳性对照 201（`:310-339`）；5 种非法 URL 400 + https 201（`:374-397`）；icon 同款（`:431-454`）；4 种畸形体 400 + Problem Details（`:480-489`） |
| 有区分力 | ✅ **强**。409 有阳性对照（唯一 client+名字→201），排除「一律拒绝」；URL 有 https→201 阳性对照，排除「一律拒绝」；列集合双向相等，多一列/少一列都红 |
| ⚠️ 但 | **`description` 字段传入了（`:59`）却从未断言回读** —— 见 **FINDING-1**（我 V7/V8 变异证实全链路无覆盖）。报告写「10 字段逐字回读」不准确，实际断言了 11 个字段、**缺 `description`** |

**结论**：✅ 主体通过（引用不复制、409、URL scheme、12 列这些**真判据**都在且有区分力）；⚠️ 有一个字段级覆盖缺口。

### 行 157 — 租户隔离

| 检查项 | 结果 |
|---|---|
| 指的位置存在 | ✅ `:562`、`:646`、`:677` 全部命中 |
| 断言存在 | ✅ `base` 见 2 租户（`:599`）；tenant-a/b 各见自己（`:617`/`:632`）；**不设上下文 → 0 行**（`:668`）；**跨租户 INSERT 断言 `42501` / `row-level security`**（`:712-715`，且注释明确**排除** 23503/23502）；本租户 INSERT 成功（`:732`） |
| 有区分力 | ✅ **强**。先做「base 能看到 2 租户」的非空对照，排除「表本来就空」；RLS 错误码断言精确排除外键/非空误判 |

**结论**：✅ 通过。这行**完全站得住**。

### 行 158 — 必须绑可用 client + 级联删除

| 检查项 | 结果 |
|---|---|
| 指的位置存在 | ✅ `:743`、`:800`、`:866` |
| 断言存在 | ✅ 不存在 client→400（`:756-757`）；disabled→400（`:767-780`）；**expired→400**（`:782-795`）；删 client **成功** `deleted==1`（`:848`）；应用行 `count==0`（`:854`）；别的 client 应用仍在（`:860`）；跨租户共享 client 两租户都为 0（`:903`） |
| 有区分力 | ✅ **强，且我独立复现**（V3）：DB 改 RESTRICT 后本行两条精确变红，报 `23503 violates foreign key constraint` |

**结论**：✅ 通过，且有我自己的变异背书。

### 行 159 — scope 是只读投影

| 检查项 | 结果 |
|---|---|
| 指的位置存在 | ✅ `:914` `the_app_scopes_are_a_live_projection_of_the_client` + `:213`（列集合） |
| 断言存在 | ✅ 初始 `["rbac:role:read"]`（`:939-943`）；改 client 后**单个读**变两项（`:959-963`）；**列表读**也变（`:970-974`）；**无任何 scope 列**（`:263-266`） |
| 有区分力 | ✅ **强**。覆盖单读+列表两条路径，只改一条会被抓；`information_schema` 直查列名，存了 scope 列就红 |

**结论**：✅ 通过。

### 行 160 — 工作台分组/排序/过滤

| 检查项 | 结果 |
|---|---|
| 指的位置存在 | ✅ `app_integration.rs:1114`、**`app_real_stack.rs:558`**、`app_real_stack.rs:638` 全部命中 |
| 断言存在 | ✅ `groups.len()==2`（`:1126`）；finance 在前（`:1127`）；组内 `["费用申请","差旅报销"]`（`:1136`）/`["C 应用","D 应用"]`（`:1144-1148`）；hidden E 不在（`:1156`）；总数 4（`:1157`）；**3 次逐字节相同**（`:1168-1169`）；**原始 JSON 顺序钉死**（`app_real_stack.rs:600-615`，id `app-z`→C 应用、`app-a2`→D 应用**刻意反序**）；`maxApps=2` 恰好 2 条（`app_real_stack.rs:671-675`） |
| 有区分力 | ✅ **最强的一行**。反序 id 设计让「按 id 兜底」必错；3 次逐字节排除不稳定排序；真库通道（`platform_config_upsert`）与内存通道都覆盖 |

**结论**：✅ 通过。**夹具区分力的论证（`:1091-1093` 注释）与代码一致**。

### 行 161 — 常用/最近

| 检查项 | 结果 |
|---|---|
| 指的位置存在 | ✅ `:1240`、`:1322` |
| 断言存在 | ✅ 第 3 次 `accessCount==3`（`:1296-1299`）；库里 `access_count==3`（`:1309`）；**行数仍 3**（`:1316`，绑 `user_id`）；`recent[0]`==`app-a`（`:1270-1273`）；`frequent[0]`==`app-a`（`:1280-1283`）；hidden 被过滤（`:1357`） |
| 有区分力 | ✅ 强。访问顺序钉死 `A→B→C→A`，否则 `recent[0]` 不成立；行数 3 证明是 UPSERT 不是插入 |

**结论**：✅ 通过。

### 行 162 — 导航两项来源 + 权限过滤 + `kind`

| 检查项 | 结果 |
|---|---|
| 指的位置存在 | ✅ E2E `apps.spec.ts`（现 `:415`，报告写 `:397` —— 文件被并发改过，漂移 18 行，**内容仍是那一节**）；前端 `app-api.test.ts:141-198`；后端 `app_integration.rs:1420` |
| 断言存在 | ✅ **module 项恰好 6 键**（`app_integration.rs:1478-1482`，逐字列出 `icon/kind/label/requiredPermission/source/target`）；**app 项恰好 7 键**（`:1495-1507`）；**`label == "审计日志"` 中文文案**（`:1485`）；`kind` 由来源推出（`:1483`/`:1508`）；`requiredPermission` app 项为 null（`:1509-1512`）；audit 项随权限出现/消失而 app 项两边都在（`:1530-1537`）；清空 launch_url 不下发（`:1650-1657`）<br>前端 `toEqual` **精确比对** 6 字段（`app-api.test.ts:152-159`）与 7 字段（`:171-180`） |
| 有区分力 | ✅ 强。键集合**排序后逐字相等**，多一个少一个都红；前端 `toEqual` 是精确结构比对 |

**结论**：✅ 通过（行号漂移 18 行，属文件被并发编辑，非「指的位置没有那个断言」）。

### 行 163 — 程序级路由 + `external` 整页跳

| 检查项 | 结果 |
|---|---|
| 指的位置存在 | ✅ `app-navigation.test.ts:107`、`:121`、`:183-221`；E2E `apps.spec.ts`（现 `:351`，报告写 `:333`，漂移）；`git diff` router.ts |
| 断言存在 | ✅ 2 internal → 2 条、`path==target`、`title==label`（`:113-117`）；**external → `[]`**（`:121`）；title 是文案不是键（`:126-128`）；**真路由器 `resolve('https://…')` → `route===undefined`**（`:216`）；**`written===[]`（`pushState` 0 次）**（`:220`）；E2E `page.url()===EXTERNAL_URL` + `pushState` 计数不增（`:383-392`）；`router.ts` **零改动**（我实测 `git diff 5aba742..HEAD` 与 worktree 均空） |
| 有区分力 | ✅ **强，我独立复现**（V4）：让 external 也产出路由后，本行 3 条精确变红 |

**结论**：✅ 通过。

### 行 164 — 审计记录

| 检查项 | 结果 |
|---|---|
| 指的位置存在 | ✅ `crates/platform-app/src/events.rs`（3 码 + 单测）、`app_integration.rs:1687`、`app_real_stack.rs:741` |
| 断言存在 | ✅ 3 个事件码各 1 条（`:1687-1734`）；`object_type=="platform_app"`；`object_id==appId`；`actor_id`/`tenant_id` 齐；**真表**读回 3 条（`app_real_stack.rs:788-797`）；**哈希链 `report.ok`**（`:818`） |
| ⚠️ 但 | 事件码是 **`app.registry.registered/updated/deleted`**，报告写 `app.registered/updated/deleted`；`object_type` 是 **`platform_app`**，报告写 `platform_app`（一致） |
| 有区分力 | ✅ 有。3 码逐个 `with_action` 计数 + 真表 + 链校验 |

**结论**：✅ **断言真实且有区分力**；⚠️ **报告的事件码写法与实现不符**（抄了 acceptance 原文而没抄实现）—— 见 **FINDING-2**，这不是「假的判据」，是**报告措辞错误 + acceptance 未同步**。

### 行 165 — 权限点三处齐 + fail-fast + C-5

| 检查项 | 结果 |
|---|---|
| 指的位置存在 | ✅ `platform-app/tests/contract_c5.rs`、`manifest.json` |
| 断言存在 | ✅ 3 个权限点恰好、字面量调用点、无自建鉴权入口（`contract_c5.rs:46-80`）；manifest 合法 + `tenantScoped`（`:90-`）；**3 个 config 键都 `scope: platform`**（`:135-180`）；`capabilities --check` 我实跑 **「14 个文件与源码一致」**；`permissions.json total == 35`（我实测） |
| 有区分力 | ✅ **强，我独立复现**（V5）：改成非字面量后两条 C-5 违例精确报出 |

**结论**：✅ 通过。

### 2.1 11 行汇总（核心指标）

| 判定 | 行数 | 行号 |
|---|---|---|
| ✅ **完全站得住**（位置对 + 断言真实 + 有区分力） | **8** | 156 · 157 · 158 · 159 · 160 · 161 · 163 · 165 |
| ⚠️ **断言真实但报告措辞超出实际** | **3** | 024（`exp-iat` 不存在）· 162（行号漂移，内容对）· 164（事件码写错 `app.registered` vs `app.registry.*`） |
| ❌ **「指到的位置根本没有那个断言」** | **0** | — |

> **关键结论**：**11 行中没有一行是 REQ-022 那种「指的字符串在源码里根本不存在」的致命类型。**
> 但 024 行的 `exp - iat` 与 164 行的 `app.registered` 是**两处措辞超出实际断言**的小型同型问题 —— 不算 FAIL，但必须记下。

### 2.2 我到底打开了什么才敢判这 8 行通过

不是看报告措辞。我实际 `sed -n` 打开并逐行读完了：

- `app_integration.rs` 的 **19 个函数体**（`:44` 到 `:1322`，约 1000 行），确认每个 `assert_eq!` 的字面量；
- `app_real_stack.rs:558-821`（工作台原始 JSON + 配置真库 + 审计链）；
- `app-navigation.test.ts:95-230`（函数级 + 真路由器 + pushState 计数）；
- `app-api.test.ts:138-198`（6/7 字段 `toEqual` 精确比对）；
- `e2e/apps.spec.ts:245-432`（工作台 2 组、hidden、权限对照、external 整页跳）；
- `model.rs` 的 `AppView`(12 字段) / `NavigationItem` / `app_columns()`；
- `service.rs` 的排序四元组（`:382-390`）、导航排序（`:525-534`）、scope 投影；
- `store.rs` 的 INSERT 绑定、PATCH `COALESCE`、访问 UPSERT；
- `http.rs` 的路由表（`:148-177`）、`authenticate`（`:197-240`）、`clear_launch_url`（`:338-347`）。
---

## 3. D290 的核实（修了一个既有红）

### 3.1 这个红灯在 `develop` 上真的是红吗？ —— ✅ **真的红，我实测复现**

BA 声称 `no_rust_source_writes_the_audit_table_directly` 在 `develop@aa5b305` 上一直是红的。

**我的独立验证**（关键：不能只看代码推理，要实跑）：

```bash
cd /home/myg133/Projects/system_seed/code     # 这是 develop@aa5b305
# 第一次跑 → ok（❌ 假象！）
# 原因：CARGO_TARGET_DIR 是共享的，复用了 feature-REQ-016 编好的二进制
#        （CARGO_MANIFEST_DIR 在编译期已烧死），跑的是**本分支**代码
cargo test -p platform-audit --test audit_real_db --no-run
#   → Executable audit_real_db-a0b46ed5aa798743  ← 与本分支同一个二进制

# 强制重编 develop 源码
touch crates/platform-audit/tests/audit_real_db.rs
cargo test -p platform-audit --test audit_real_db --no-run
#   → Compiling platform-audit v0.1.0 (/home/myg133/Projects/system_seed/code/crates/platform-audit)

# 再跑
cargo test -p platform-audit --test audit_real_db -- --test-threads=1 \
  no_rust_source_writes_the_audit_table_directly
```

**结果**：

```
test no_rust_source_writes_the_audit_table_directly ... FAILED
panicked at crates/platform-audit/tests/audit_real_db.rs:646:5:
crates/**/*.rs 里不得出现裸 INSERT：
  ["/home/myg133/Projects/system_seed/code/crates/platform-audit/../../crates/platform-server/tests/oauth_real_stack.rs:761"]
```

✅ **与 BA 报告的描述逐字一致**（包括报出的路径和行号）。

**根因确认**：
- `develop` 的 `scan_dir`（`audit_real_db.rs:1012-1015`）是 `if path.is_dir() { stack.push(path); continue; }` —— **无条件递归，不跳过 `tests/`**；
- `crates/platform-server/tests/oauth_real_stack.rs:761` 确实有 `"INSERT INTO platform_audit_log \`（REQ-022 的阳性对照，为验 AC-S-154 步骤 4 的敏感值扫描必须直插）。

### 3.2 修法是否削弱了守卫？ —— ✅ **没有削弱（我独立做了敏感度验证 V6）**

**修法**（`audit_real_db.rs:1025-1032`）：
```rust
if path.is_dir() {
    // 跳过测试/基准/示例目录（它们的阳性对照必须直插）。
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if matches!(name, "tests" | "benches" | "examples" | "target") {
        continue;
    }
    stack.push(path);
    continue;
}
```

**我自己做的变异**（不是复述 BA）：

往**生产代码** `crates/platform-audit/src/sink.rs` 塞：
```rust
const _MUTATION_PROBE: &str = "INSERT INTO platform_audit_log (id) VALUES ('x')";
```

**结果**：✅ **仍然红**，精确报 `crates/platform-audit/src/sink.rs:143`。
→ 守卫对**生产代码**的敏感度**完全保留**；跳过 `tests/` 只放过「阳性对照必须直插」的测试代码。

### 3.3 `scan_dir` 的注释与代码一致吗？ —— ✅ **完全一致**

注释（`:1004-1015`）明写「**必须跳过 `tests/` 目录（REQ-016 修）**」「只扫**非 test** 的源码：`tests/`、`benches/`、`examples/` 一律跳过」，代码 `matches!(name, "tests" | "benches" | "examples" | "target")` 逐字兑现。

### 3.4 ⚠️ 我需要标注的一个残余风险

跳过 `tests/` 意味着：**今后若有人在 `tests/` 里写生产路径的裸 INSERT，该守卫不会报**。
- 现实影响：低（`tests/` 里的直插是**测试夹具**，不是生产逃逸路径；且 `platform_app` 的审计经 `AuditSink` 走生产代码，仍被覆盖）；
- 但这是一个**语义上的守卫收窄**，值得在 TD-053 的关闭说明里写清楚「已接受的残余风险」，而不是只写「误报已修」。
→ 记为 **FINDING-4（低）**。

### 3.5 我踩过 / 避开的一个坑（记录给后来者）

`CARGO_TARGET_DIR` 是**跨 worktree 共享**的。直接 `cd code && cargo test` 会**静默复用** `feature-REQ-016` 编出的二进制 → **得到错误结论（假绿）**。
复现他分支的测试结论，**必须先 `touch` 目标测试文件强制重编**，并确认输出里有 `Compiling … (/code/crates/…)`。
---

## 4. 变异抽查（我自己做的 8 个）

> 方法：`cp` 备份 → `sed`/`python3` 改**生产那一行** → 跑测试 → `cp` 还原 + `touch` → `cmp` 逐字节校验 + `grep MUTATION` = 0。

| # | 变异（改的是生产那一行） | 预期 | 实测 | 还原 |
|---|---|---|---|---|
| **V1** | `http.rs:223` 去掉 `ensure_fresh` 调用 | 500 断言红 | ✅ 红：`a_failing_permission_refresh_fails_the_protected_endpoint_closed` 失败<br>⚠️ 注意：`the_registry_endpoints_enforce_their_permissions` **仍绿**（因默认夹具 `AlwaysFresh` 是 no-op） | ✅ `cmp` OK |
| **V2** | `manifest.json` 的 `app.workspace.maxApps.scope` → `tenant` | scope 测试红 | ✅ 红：`left: Tenant / right: Platform`，精确点名 `app.workspace.maxApps`<br>📌 附带发现：`cargo xtask capabilities --check` **不检查 scope**（仍绿），只有 `contract_c5` 抓得到 | ✅ `cmp` OK |
| **V3** | DB `platform_app_client_id_fkey` → `ON DELETE RESTRICT` | 级联测试红 | ✅ 红 ×2：`23503 violates foreign key constraint "platform_app_client_id_fkey"` | ✅ `confdeltype='c'` |
| **V4** | `nav.ts:77` 让 `external` 也产出路由 | AC-163 红 | ✅ 红 ×3：`toHaveLength(2)`、`external → []`、`路由表无 http 条目` | ✅ `cmp` OK |
| **V5** | `authz.rs:41` 字面量 → 变量 | C-5 红 | ✅ 红：两条违例（「no authorization call site」+「does not use a string literal」） | ✅ `cmp` OK |
| **V6** | `sink.rs` 生产代码塞裸 INSERT | 守卫红 | ✅ 红：报 `sink.rs:143` | ✅ `cmp` OK |
| **V7** | `store.rs:134` INSERT 时 `description` 丢成 `None` | **应红** | ❌ **27/27 全绿** —— 无测试抓到 | ✅ `cmp` OK |
| **V8** | `model.rs` 的 `view_of` 里 `description: None`（两处） | **应红** | ❌ 全绿 —— 无测试抓到 | ✅ `cmp` OK |

### 4.1 环境恢复确认

```
cmp 逐字节：http.rs / model.rs / authz.rs / store.rs / manifest.json / nav.ts / sink.rs 全部 OK
DB：platform_app_client_id_fkey confdeltype = c (CASCADE)
grep -rn MUTATION（crates/ apps/ migrations/ e2e/）→ 空
平台测试复跑：app_integration 27/27 ok（连跑 3 次）
```

### 4.2 V1 的一个值得注意的细节（不是缺陷，但值得知道）

删掉 `ensure_fresh` 后，**只有** `a_failing_permission_refresh_fails_the_protected_endpoint_closed` 变红，
而 `the_registry_endpoints_enforce_their_permissions`（测「只读能读、不能写、无权限 403、未认证 401」）
**依然全绿**。

原因：默认夹具 `AlwaysFresh::ensure_fresh` 是 **no-op**（`common/mod.rs:159-166`），
所以「漏调」在默认夹具下**不可观测** —— 这正是 TD-047 那类「第一次失败、重试就好」缺陷的形态。
**当前的覆盖是对的**（用 `FailingOnceRefresher` + `calls() >= 1` 专门测），
但要注意：**如果把那条专用用例删了/跳过，整个缺席就没人盯着**。这是覆盖结构上的单点，记为 **FINDING-5（低）**。
---

## 5. 跨栈契约逐字段核对（D258 / D284）

### 5.1 `AppView` 12 字段（camelCase）

后端 `crates/platform-app/src/model.rs:253-278` `#[serde(rename_all = "camelCase")]`
vs 前端 `apps/shell/src/lib/app-api.ts:168-193`：

| # | 后端（snake） | 序列化（camel） | 前端 | 一致 |
|---|---|---|---|---|
| 1 | `app_id` | `appId` | `appId: string` | ✅ |
| 2 | `client_id` | `clientId` | `clientId: string` | ✅ |
| 3 | `display_name` | `displayName` | `displayName: string` | ✅ |
| 4 | `description` | `description` | `description: string \| null` | ✅ |
| 5 | `category` | `category` | `category: string` | ✅ |
| 6 | `icon_url` | `iconUrl` | `iconUrl: string \| null` | ✅ |
| 7 | `launch_url` | `launchUrl` | `launchUrl: string \| null` | ✅ |
| 8 | `sort_order` | `sortOrder` | `sortOrder: number` | ✅ |
| 9 | `status` | `status` | `status: string` | ✅ |
| 10 | `scopes` | `scopes` | `scopes: readonly string[]` | ✅ |
| 11 | `created_at` | `createdAt` | `createdAt: string` | ✅ |
| 12 | `created_by` | `createdBy` | `createdBy: string` | ✅ |

**12/12 一致，可空性也一致**（`description`/`iconUrl`/`launchUrl` 前端都是 `| null`）。

### 5.2 `NavigationItem`：module 6 字段 / app 7 字段（**不能多也不能少**）

后端 `model.rs:319-335`（`app_id` 带 `skip_serializing_if = "Option::is_none"`）
vs 前端 `app-api.ts:210-243`：

| module 项（6） | 后端 | 前端 | app 项（7） | 后端 | 前端 |
|---|---|---|---|---|---|
| `source` | ✅ | ✅ | `source` | ✅ | ✅ |
| `label` | ✅ | ✅ | `appId` | ✅（`Some` 才序列化） | ✅ |
| `icon` | ✅ | ✅ | `label` | ✅ | ✅ |
| `target` | ✅ | ✅ | `icon` | ✅ | ✅ |
| `kind` | ✅ | ✅ | `target` | ✅ | ✅ |
| `requiredPermission` | ✅ | ✅ | `kind` | ✅ | ✅ |
| | | | `requiredPermission` | ✅ | ✅ |

**恰好 6 / 7，双向一致。** 双向都有「恰好」断言：

- 后端 `app_integration.rs:1478-1482`（module 6 键，排序后逐字相等）与 `:1495-1507`（app 7 键）；
- 前端 `app-api.test.ts:152-159`（module 6 键 `toEqual`）与 `:171-180`（app 7 字段 `toEqual` 精确结构）。

### 5.3 `label` 是中文文案不是 `labelKey`（D284）

| 位置 | 证据 |
|---|---|
| 后端 | `model.rs:325` 注释「`module` 用 `menus[].label` 的中文文案」；`app_integration.rs:1485` `assert_eq!(module_item["label"], "审计日志")` |
| 前端 | `app-navigation.test.ts:126-128` `expect(routes[0]?.title).toBe('审计日志')` + `not.toMatch(/^[a-z]+\.[a-z]/)`（排除翻译键形态） |
| 前端类型 | `app-api.ts:213` 注释「**已渲染的中文文案**，不是翻译键 —— D284」 |

✅ 一致，且**有区分力**（`not.toMatch` 专门排除 `nav.audit` 这种键）。

### 5.4 响应形状（裸数组 vs 对象包装）

| 端点 | 契约 | 后端实现 | 后端断言 | 前端解析 | 一致 |
|---|---|---|---|---|---|
| `GET /api/apps` | **裸数组** | `Json(rows)`（`:364`-ish `list_apps`） | `:112` `list.as_array()`，「列表必须是数组」 | `parseAppList`（`:342-347`）要求 `Array.isArray` | ✅ |
| `GET /api/apps/recent` | **裸数组** | `Json(rows)`（`:364`） | `:1266-1273` 直接 `recent[0]` 下标 | `parseAppList` | ✅ |
| `GET /api/apps/frequent` | **裸数组** | `Json(rows)`（`:372`） | `:1276-1283` `frequent[0]` | `parseAppList` | ✅ |
| `GET /api/apps/workspace` | `{groups}` | `Json(WorkspaceView)` | `:1125` `body["groups"].as_array()` | `parseWorkspace`（`:350-362`）读 `raw['groups']` | ✅ |
| `GET /api/apps/navigation` | `{items}` | `Json(NavigationView)` | `:1449` `body["items"].as_array()` | `parseNavigation` 读 `items` | ✅ |

### 5.5 `DELETE /api/apps/{id}/launch-url` → 204

| 检查项 | 证据 |
|---|---|
| 后端路由 | `http.rs:166-168` `delete(clear_launch_url).route_layer(require_registry_write())` |
| 后端返回 | `http.rs:344` `StatusCode::NO_CONTENT.into_response()` |
| 后端断言 | `app_integration.rs:1623` `assert_eq!(status, 204, "清空 launch_url 必须 204")` + `:1631` 库里必须是 `NULL`（不是空串） |
| 前端调用 | `app-api.ts:75-77` `appLaunchUrlPath` → `/launch-url`；`app-registry.test.ts:150-160` `method === 'DELETE'` + `body === undefined` |

✅ 全链路一致，且「无请求体」这一点也被断言（这正是需要专用端点而非 `PATCH null` 的理由）。

### 5.6 契约核对结论

**无字段名不一致、无可空性不一致、无形状不一致。** D284 的「label 是文案」在两端都有排他性断言。
唯一的字段级问题是 **`description` 没有回读断言**（前端会渲染它，见 §9 FINDING-1）。
---

## 6. 注释与代码一致性（抽查 12 处）

| # | 位置 | 注释声称 | 代码实际 | 一致 |
|---|---|---|---|---|
| 1 | `http.rs:19-27` + `:222-223` | 认证中间件**必须**调 `ensure_fresh` | `state.refresher.ensure_fresh(&ctx).await` 真实存在，且 `router()` 最外层挂 `from_fn_with_state(authenticate)`（`:183-186`） | ✅ |
| 2 | `http.rs:29-33` | 每个端点只挂**一个**权限点，不能叠 `route_layer` | `:148-168` 逐个 `MethodRouter::route_layer`，一 method 一层 | ✅ |
| 3 | `http.rs:35-38` | `src/` 里**不得**出现 `authorize` 等标识符 | 实测 grep：`authorize` 只出现在**注释**（`lib.rs:42`/`authz.rs:26,29`/`http.rs:37` 的禁忌说明），代码中 0 处 | ✅ |
| 4 | `service.rs:361-372` | 工作台排序 **全局** `(category, sortOrder, displayName, id)`，**截断在分组前**，**不按 launch_url 过滤** | `:382-390` 四元组 `sort_by` + `rows.truncate(...)` 在分组前 | ✅ |
| 5 | `service.rs:18-19` | `scopes` **直接取自** client，应用表**不存** scope | `scopes_for_one`/`scopes_for` 每处 `view_of(&row, scopes)`；`app_columns()` 无 scope 列 | ✅ |
| 6 | `service.rs:525` | 导航排序：module 在前、app 在后，组内按 `label` 升序 | `:531-540` `source_rank(...)` 再 `label` 再 `target` 再 `app_id` | ✅ |
| 7 | `model.rs:345-349` | `app_columns()` 是**唯一**一份列清单，避免两处漂移 | `store.rs` 的 6 处 SELECT 全部 `columns = app_columns()`（`:129,149,161,176,204,485`） | ✅ |
| 8 | `nav.ts:13-16` + `:73-79` | `routesFromNavigation` **只**把 `internal` 变路由 | `.filter((item) => item.kind === 'internal')` —— **我 V4 变异证实**它真的只放 internal | ✅ |
| 9 | `audit_real_db.rs:1004-1015` | `scan_dir` 必须跳过 `tests/`（REQ-016 修） | `:1028` `matches!(name, "tests" \| "benches" \| "examples" \| "target")` | ✅ |
| 10 | `store.rs:193-201` | PATCH 只改**传入**字段（其余保留） | 每个可改列都是 `COALESCE(?, 原列)` | ✅ |
| 11 | `store.rs:309-312` | 访问记录 UPSERT **累加** `access_count` | `ON CONFLICT (tenant_id,user_id,client_id) DO UPDATE SET access_count = {table}.access_count + 1` | ✅ |
| 12 | `app-api.ts:70-76` + `:621` | 置空 `launch_url` **必须**用专用 `DELETE`（`PATCH null` 与省略键分不开） | 前端实际调 `DELETE /launch-url`（`app-registry.test.ts:150-160`），后端路由存在（`http.rs:166-168`） | ✅ |

**12/12 注释与代码一致。** 我没有找到「注释说的是 A、代码做的是 B」的案例。

> 对照教训：REQ-022 的失败模式是「报告复述 AC 期望，而非源码断言」。本次**注释层面**没有这个问题；
> 问题只出现在**报告措辞**（§2 的 024/164）与**测试覆盖**（§9 FINDING-1 的 `description`）。
---

## 7. 测试数字核对（我自己跑 vs 报告）

| 套件 | 报告声称 | 我实跑 | 一致 |
|---|---|---|---|
| `platform-app --all-targets` | **76** | **76** = 46(lib) + 27(app_integration) + 3(contract_c5) | ✅ |
| `platform-server --all-targets` | **136** | **136** = 70+8+5+5+1+17+5+6+3+9+2+1+4 | ✅ |
| `platform-audit --all-targets` | **54** | **54** = 24+7+13+2+8 | ✅ |
| `pnpm --filter @base/shell test` | **345** | **345**（26 files） | ✅ |
| `platform-oauth --all-targets`（回归 024） | **131** | **131** = 91+2+36+2 | ✅ |
| `cargo xtask capabilities --check` | 14 文件 | 「14 个文件与源码一致」 | ✅ |
| `permissions.json total` | 35 | 35（我读 JSON） | ✅ |
| E2E `apps.spec.ts` | **6/6** | **6/6**（清理播种 + 无并发后） | ✅ |

**全部 8 项数字逐一对上，无虚报。**

### 7.1 一个必须说清的「flaky」观察（不是产品缺陷，但有意义）

我第一次跑 `platform-audit` 时，`audit_chain.rs:156` 的
`the_created_at_is_strictly_increasing_per_tenant` **失败过一次**（报告声称 54/54 全绿）。

**我做了对照**：

| 场景 | 结果 |
|---|---|
| 单独跑该用例 ×3 | 3/3 通过 |
| 整文件 `--test-threads=1` 连跑 ×4 | 4/4 通过（7 passed） |

**结论**：**并发干扰**（我复核期间有另一个 agent 在同一 `base_test` 上跑 `cargo test`，
`platform-app` 夹具的 `DELETE FROM platform_app` 会清表）。**不是产品缺陷，不构成对 BA 数字的否定**。
但这说明：**该库被多个 agent 共享时数字不可复现** → 建议合并前用干净 schema 复跑一次。

**我没有修改 `audit_chain.rs`**（`git diff` 确认该文件本轮零改动）。

### 7.2 一个我自己造成的假象（诚实记录）

我第一次跑 `platform-oauth` 时 `oauth_http.rs` 36 个全 FAILED —— 原因是我**漏设** `DATABASE_URL_TEST`。
补上环境变量后 36/36 通过。**这是我的操作错误，不是产品缺陷**，不应记入对 BA 的指控。
---

## 8. 两个「环境 / 流程」声称的核实

### 8.1 E2E 首跑 5 failed 的根因 —— ✅ **成立**

BA 声称：「8080 上是 `code/`（develop）的服务 → `/api/apps/*` 全 404 → SDK 报网络不可用
→ `reuseExistingServer: true` 复用了错的服务」。

**我核实的两条**：

**① `code/` 里有没有 `platform-app` crate？** —— ❌ **没有**：

```
$ ls code/crates/
platform-audit platform-config platform-contracts platform-contracts-macros
platform-core platform-db platform-iam platform-oauth platform-org
platform-rbac platform-schema platform-server xtask
$ ls -d code/crates/platform-app
不存在 platform-app
```

且 `code/` 是 `develop@aa5b305`（REQ-016 未合并）。→ **`develop` 的服务确实不可能有 `/api/apps/*` 路由。**

**② 当前 8080 上是谁？** —— ✅ **是 `feature-REQ-016`**（我实测）：

```
GET /health/live            → 200
GET /api/apps/navigation    → 401   ← 路由存在（未认证），不是 404
GET /api/apps               → 401 {"code":"UNAUTHENTICATED",...}  ← 路由存在
```

若跑的是 `develop` 服务，这两条会是 **404**。→ **BA 的处置（杀掉旧服务 → 用本 worktree 重启）是必要且正确的。**

**③ 我自己的 E2E 实测**：

| 轮次 | 条件 | 结果 |
|---|---|---|
| 第 1 次 | 我刚跑完 V3 变异测试（`platform_app` 表被夹具 `DELETE` 清空）+ 另一个 agent 并发跑 `cargo test` | ❌ 4 failed / 2 passed |
| 第 2 次 | `./scripts/seed-e2e.sh` 重新播种 + 确认无并发 | ✅ **6/6 passed** |

第 1 次失败快照里出现了 `A 的接入`（那是 `app_integration` 的夹具数据），
**证实失败是我自己的测试污染 + 并发干扰，不是产品缺陷**。
→ **BA 的「6/6 全绿」成立**（前提是干净播种 + 不并发跑集成测试）。

### 8.2 `router.ts` 零改动 —— ✅ **成立**

```bash
git diff --stat 5aba742..HEAD -- apps/shell/src/lib/router.ts   → 空
git diff --stat -- apps/shell/src/lib/router.ts                 → 空（worktree 内也未改）
```

AC-163 的「程序级路由**不改路由器**」这一条**真实成立**。

### 8.3 ⚠️ 我需要指出：`apps.spec.ts` 在复核期间被**并发修改**

| 时刻 | 行数 | `test(` 行号 |
|---|---|---|
| 我首次读（21:16 左右） | 534 | 245 / 290 / 333 / 397 / 433 / 505 |
| 之后（21:23:07 mtime） | **550** | **245 / 308 / 351 / 415 / 451 / 523** |

**含义**：BA 报告里写的 `apps.spec.ts:290 / :333 / :397` 与**当前文件**不一致（漂移 16~18 行），
但**内容仍是同一批用例**（我逐一打开确认）。这不是「指向不存在的位置」，而是
**文件在我复核期间被另一个 agent 编辑**。

→ 记 **FINDING-3（中）**：**复核/交付期间 worktree 仍在被并发写**，这会让任何行号引用不可复现。
---

## 9. 发现的问题（按严重度）

> 分级口径：**阻塞** = 必须改才能合并；**高** = 影响判据可信度，应改；**中** = 会造成误导/不可复现，建议改；**低** = 记录即可。

### 🔴 阻塞 —— **0 条**

没有发现「判据根本不存在」「测试无法通过」「契约撕裂」这类阻塞问题。

---

### 🟠 FINDING-1（高）：`description` 字段**全链路零覆盖**，而报告声称「10 字段逐字回读」

- **触发条件**：任何把 `description` 写丢 / 读丢的实现错误。
- **实测影响**（我的变异 V7 / V8）：
  - `store.rs:134` 把 INSERT 的 `description` 绑成 `None` → `app_integration` **27/27 全绿**；
  - `model.rs` 的 `view_of` 把 `description` 恒置 `None` → 同样全绿；
  - `platform-server/tests/app_real_stack.rs`（5 个用例）**也没抓到**。
- **为什么这是真问题**：`description` 是**用户可见字段** ——
  `Workbench.svelte:225-226` 渲染它、`AppRegistry.svelte:152,170-171` 用它回填编辑表单。
  一个「保存了但读不回来」或「读了但写不进去」的缺陷会**直接暴露给用户**，且**测试全绿**。
- **AC 依据**：`acceptance.md` AC-S-156 预期结果明写「步骤 3/4：字段**逐字**回读一致」，
  且步骤 1 的创建包**包含 `description`**（`:150-156` 区间）。→ **AC 要求的，测试没测。**
- **证据位置**（我扫了**全部四个测试层**，都不覆盖）：
  - 后端：`crates/platform-app/tests/app_integration.rs:70-100` 断言了 11 个字段，**独缺 `description`**；
    `grep -rn 'description"]'` 全仓 → **0 处**
  - 真栈：`crates/platform-server/tests/app_real_stack.rs` 5 个用例 → 无 description 断言
  - 前端：`app-api.test.ts:130` 只有 `expect(view.description).toBeNull()`（**缺省归 null 的分支**，不是「值透传」）
  - **E2E**：`e2e/apps.spec.ts:481-497` 注册表单只填「客户端标识/名称/分类/入口地址」，
    **从不填 `description`，也从不断言它** —— 页面级同样无覆盖
  - 报告措辞：`BA/feature-meta/req-016-final-verification.md:134`（「10 字段逐字回读」）
- **建议**：在 `an_app_can_be_created_and_read_back_verbatim` 补
  `assert_eq!(created["description"], "差旅报销")`，并在 PATCH 用例补一条 description 改动断言。
  （**我不改，只报告** —— verifier 不改交付物。）

---

### 🟡 FINDING-2（中）：AC-164 的事件码口径在 acceptance 里**未同步**，报告抄了错的原文

- **事实三层不一致**：

| 层 | 事件码 | `object_type` |
|---|---|---|
| `acceptance.md` rev 4 正文（`:73`, `:388-392`） | `app.registered` / `app.updated` / `app.deleted` | `app` |
| 用例集 `test-cases/05-audit-permission.md`（TC-016-053/054） | `app.registry.registered/…` | `platform_app` |
| **实现 + 测试**（`events.rs:33-37`、`app_real_stack.rs:792-799`） | `app.registry.registered/…` | `platform_app` |

- **QA 已提出** S-12（`qa-req016-spec-review.md:253-262`），建议就是改成三段式 + `platform_app`；
  **但 `decisions.md` 里没有对应的 D 编号裁决，acceptance 正文也没改**。
- **实现方是「对的」**（跟随已交付的 `oauth.client.registered` + 用例集），
  且 `events.rs:17-28` 的注释**诚实写明了这个矛盾**并上报 BA 复核。
- **报告的问题**：`req-016-final-verification.md:142` 写「3 个事件码 `app.registered/updated/deleted`」
  —— **抄的是 acceptance 的错原文，不是实际断言的 `app.registry.*`**。
  这正是 REQ-022 同型的「复述 AC 而非源码」。
- **影响**：判据本身真实且有区分力（我核实了），但**报告与代码不符**，且**规格仍是错的**。
- **建议**：把 acceptance AC-164 同步为 `app.registry.*` + `platform_app`，补一条 D 裁决，并更正报告措辞。

---

### 🟡 FINDING-3（中）：复核/交付期间 worktree 仍被并发写入

- **观测**：`e2e/apps.spec.ts` 从 534 行 → **550 行**（mtime 21:23:07，在我复核过程中），
  `test(` 行号整体漂移 16~18 行；同时有另一个 agent 在 `base_test` 上跑 `cargo test`。
- **影响**：
  1. 任何 `file:line` 引用**不可复现**（报告里的 `:290/:333/:397` 现在指向别处）；
  2. 共享库被并发清表 → 测试数字与 E2E 结果**不可复现**（我亲身踩到：E2E 4 failed、audit flaky 1 次）。
- **这不是产品缺陷**，但它**直接削弱了「终验」的可信度**。
- **建议**：合并前**冻结写入**（所有 agent 停手）→ 干净 schema → 重跑 5 个套件 + E2E，并**以那次输出为准**。

---

### 🟢 FINDING-4（低）：`scan_dir` 跳过 `tests/` 是**守卫语义收窄**，应在 TD-053 写明为已接受风险

- 修法本身**正确**（我 V6 证实生产代码裸 INSERT 仍红）。
- 但「`tests/` 里的裸 INSERT 不再被检查」是一个**永久的语义变化**，
  而 TD-053 只写了「误报 → 已修」，没写「接受了一个残余风险」。
- **建议**：TD-053 的关闭说明补一句：「已接受：`tests/` 内的直插不再被本守卫覆盖（阳性对照所需）。」

---

### 🟢 FINDING-5（低）：`ensure_fresh` 的覆盖是**单点**

- 删掉 `ensure_fresh` 后，只有 `a_failing_permission_refresh_fails_the_protected_endpoint_closed`
  变红；其余 26 个用例全绿（因默认夹具 `AlwaysFresh` 是 no-op）。
- 当前覆盖**是对的**，但**只有一条用例在盯**。若它被跳过/删除，整个缺席无人发现。
- **建议**：无需改，记录即可。

---

### 🟢 FINDING-6（低）：报告措辞「`exp - iat == access_token_expires_in`」在测试里不存在

- 实际只有 `assert_eq!(body["access_token_expires_in"], 3600)`（`oauth_http.rs:433`）。
- AC-024 的**核心判据**（撤销后 `active=false`）真实存在（`:2232`），所以不影响结论。
- **建议**：报告删掉那半句，或补上真正的 `exp - iat` 断言。

---

### 问题统计

| 级别 | 条数 |
|---|---|
| 阻塞 | **0** |
| 高 | **1**（FINDING-1） |
| 中 | **2**（FINDING-2 / FINDING-3） |
| 低 | **3**（FINDING-4 / 5 / 6） |
| **合计** | **6** |
---

## 10. 总结论

# **PASS-WITH-RISK**

### 10.1 为什么不是 FAIL

REQ-022 的失败模式是「**13 行判据落点里指的字符串在测试源码里根本不存在**」。
本次我**逐行打开 11 行涉及的全部源码**，结论是：

- **11 行中 0 行**属于「指到的位置根本没有那个断言」；
- **8 行完全站得住**（位置对 / 断言真实 / 有区分力），其中 **4 行我用自己的变异亲自打红过**（V3/V4/V5/V6）；
- **3 行是报告措辞问题**（024 的 `exp-iat`、164 的事件码写错、162 的行号因并发漂移），**不是判据造假**；
- **所有 8 项测试数字逐一复现**（76 / 136 / 54 / 345 / 131 / 14 / 35 / 6-6）。

**BA 这次的方法论（dev-guide §9.15 / §9.15.1）在源码层面是真的落地了** ——
这不是「看起来有」，是「打开就有」。

### 10.2 为什么不是纯 PASS —— 三个风险

| # | 风险 | 严重度 | 会不会挡合并 |
|---|---|---|---|
| 1 | **`description` 全链路零覆盖**，而 AC-156 明确要求字段逐字回读 | **高** | **建议补测试再合并**（一行 assert 的事） |
| 2 | AC-164 事件码在 **acceptance 里仍是错的口径**（`app.registered` / `object_type="app"`），实现与用例集是对的 | 中 | 不挡合并，但**规格欠债**必须记 |
| 3 | 复核期间 **worktree 仍被并发写 + 共享库被并发清表** → 数字与行号不可复现 | 中 | **合并前必须冻结写入重跑一次** |

### 10.3 合并前的最小建议动作

1. **补一条 `description` 回读断言**（FINDING-1）—— 一行 `assert_eq!`，把 AC-156 的「逐字回读」补齐；
2. **同步 acceptance AC-164** 到 `app.registry.*` + `platform_app`，补 D 裁决（FINDING-2）；
3. **冻结所有 agent 的写入**，用干净 schema 重跑 `platform-app / server / audit / shell / capabilities / e2e`，
   以**那一次**的输出作为终验数字（FINDING-3）；
4. 更正报告 §4 的两处措辞（024 的 `exp-iat`、164 的事件码）；
5. TD-053 关闭说明补「已接受 `tests/` 不扫的残余风险」（FINDING-4）。

### 10.4 我最想提醒的一点

> **BA 这次的判据落点是真的（8/11 完全站得住，且我逐行打开源码确认）。**
> **但「判据存在」≠「判据覆盖了 AC 要求的全部字段」。**
> `description` 就是活例子：**11 行全绿、76+136+54+345 全绿、E2E 6/6 全绿，
> 而我一行变异就能让这个用户可见字段彻底失效且测试全绿。**
>
> 下一轮不要只检查「判据是否存在」，还要检查
> **「AC 列出的每一个字段 / 每一个分支，是否都有对应的断言」**
> —— 这正是「有测试」和「测到位」的差别。

---

## 附录 A：本报告的所有命令均可复现

```bash
cd /home/myg133/Projects/system_seed/feature-REQ-016
export XDG_RUNTIME_DIR=/run/user/$(id -u)
export CARGO_TARGET_DIR=/home/myg133/Projects/system_seed/code/target
export DATABASE_URL_TEST="postgres://base:base@127.0.0.1:5432/base_test"

# 数字
cargo test -p platform-app    --all-targets -- --test-threads=1 --include-ignored   # 76
cargo test -p platform-server --all-targets -- --test-threads=1 --include-ignored   # 136
cargo test -p platform-audit  --all-targets -- --test-threads=1 --include-ignored   # 54
cargo test -p platform-oauth  --all-targets -- --test-threads=1 --include-ignored   # 131
pnpm --filter @base/shell test                                                      # 345
cargo xtask capabilities --check                                                    # 14

# E2E（务必先播种、且确认无并发 cargo test）
./scripts/seed-e2e.sh && npx playwright test e2e/apps.spec.ts                       # 6/6
```

## 附录 B：D290 复现的「坑」

`CARGO_TARGET_DIR` 跨 worktree 共享 → 在 `code/` 里直接跑测试会**复用本分支的二进制**得到假绿。
复现前必须 `touch` 目标测试文件并确认输出有 `Compiling … (/code/crates/…)`。

## 附录 C：环境恢复声明

- 所有变异文件与备份 `cmp` 逐字节一致（7 个文件）；
- DB 约束 `platform_app_client_id_fkey` 已还原为 `confdeltype='c'`（CASCADE）；
- `grep -rn MUTATION`（crates/ apps/ migrations/ e2e/）→ **空**；
- `git status` 与复核开始时一致（仅有 REQ-016 原有的未提交改动）；
- `code/` 未修改内容（仅 `touch` 了 mtime）；
- **未修改任何源码 / 测试 / 报告**（除本报告自身）。
