# REQ-014 修复轮 **独立复核**任务卡（QA 子 Agent）

- **需求**: REQ-014 审计日志
- **被验对象**: `/home/myg133/Projects/system_seed/feature-REQ-014`，分支 `feature/REQ-014`
  - HEAD 仍是 `5f28c0c`（**修复轮尚未提交**）→ 改动在工作区，用 `git status` / `git diff` 看
- **验收基线**: `BA/demands/REQ-014-audit/acceptance.md` rev 3（13 条 AC）
- **决策基线**: `BA/demands/REQ-014-audit/decisions.md` **rev 5**（D130~D195）
- **上轮复验报告**: `BA/feature-meta/qa-premereq-req014.md`（FAIL，阻塞 2 / 高 1 / 中 3 / 低 4）
- **修复轮报告**: `BA/feature-meta/req-014-dev-fixround.md` + `req-014-dev-fixround-f10.md`
- **修复轮任务卡**: `BA/feature-meta/req014-fixround-brief.md`（正文 + **附录 A**）
- **本卡由 BA（myg133-ba-1）于 2026-11-20 14:40 写下**

---

## 0. 你的立场

**独立复核者**。不采信 Dev 的自验报告 —— 上轮复验已经证伪过 Dev 的数字表
（Dev 报 `platform-core` 143 / `platform-iam` 99，实测 150 / 168）。
**所有数字自己重跑、自己用 awk 算。**

上一轮复验的结论是 **FAIL**（2 条阻塞）。本轮要回答的是一个问题：
**「那 2 条阻塞真的修好了吗？修复有没有引入新问题？新补的断言是不是仍然是假绿？」**

---

## 1. 必读

1. `BA/demands/REQ-014-audit/acceptance.md`（rev 3，13 条 AC）
2. `BA/demands/REQ-014-audit/decisions.md`（rev 5，D130~D195）—— **逐条读**
3. `BA/feature-meta/qa-premereq-req014.md`（上轮复验报告 —— 你的**对照基线**）
4. `BA/feature-meta/req014-fixround-brief.md`（正文 + **附录 A**）
5. `BA/feature-meta/req-014-dev-fixround.md` + `req-014-dev-fixround-f10.md`（Dev 的自述 —— **待验证，不是事实**）
6. `BA/plan/dev-guide-low-spec.md` §9 出口标准 12 条
7. `BA/contracts/api-conventions/v1.md`（C6 / C7 / C9 / C12 / C13）

---

## 2. 环境（铁律）

```bash
export XDG_RUNTIME_DIR=/run/user/$(id -u)
export CARGO_TARGET_DIR=/home/myg133/Projects/system_seed/code/target
export DATABASE_URL_TEST="postgres://base:base@127.0.0.1:5432/base_test"
```

- `cargo test --workspace` 在本仓**必失败** → **逐 crate 跑**（11 个 crate）：
  `platform-core platform-db platform-iam platform-org platform-rbac platform-config platform-schema platform-server platform-audit sandbox xtask`
- `--all-targets` 不能省；**只能有一个** `--`；`--include-ignored` 在 `--` **之后**。
- 统计口径（**自己算**）：
  ```bash
  cargo test -p <crate> --all-targets -- --test-threads=1 --include-ignored \
    | grep -E '^test result' | awk '{p+=$4; f+=$6; i+=$8} END {printf "passed=%d failed=%d ignored=%d\n", p, f, i}'
  ```
- **串行**跑（8 线程 / 7.7G；`platform-org`/`platform-iam` 并发下有 CPU 竞争 flake）。
- 应用连接用 **`base_app`**（超管 `base` 带 `BYPASSRLS` → 验 RLS/append-only **必假绿**）。
- 本机**无 psql 客户端** → `podman exec base-postgres psql -U base -d base_test -tAc "…"`。
- ⚠️ **迁移文件被改过**（`20261120000400_audit_config_seed.up.sql`）→
  **sqlx 的 checksum 会不匹配** → 测试库可能需要**重建**。
  仓内 `scripts/test-integration.sh` 用 `DROP SCHEMA public CASCADE` 重置 → 用它。

---

## 3. 🔴 必查 A：两条阻塞修好了吗（**黑盒**，不许只读代码）

| # | 查什么 | 怎么查（可机械判定） |
|---|---|---|
| A1 | **`nextCursor` 不再恒 null** | 起真实服务 + 登录拿 token → `GET /api/audit/events?limit=2` → **`nextCursor` 非 null**；带上它再取一页 → **两页 `id` 无交集**；`total > limit` ⟺ `nextCursor != null`；末页 `nextCursor` 为 null。⚠️ **要看响应体**，不能只看状态码 |
| A2 | **过滤查询不再 503** | `GET /api/audit/events?filter[actorId][eq]=…` / `?filter[action][contains]=…` / `?filter[action][in]=…` / `?filter[createdAt][range]=…` / `?filter[action][ne]=…` → **全部 200**（不是 503），返回行数与 `id` 集合正确 |

---

## 4. 🔴 必查 B：新补的断言**是不是仍然是假绿**（**必须自己注入变异**）

**这是本轮最关键的一项。** Dev 说新断言能抓红 —— 你**自己**验证，别信。

- **B1**：`crates/platform-audit/src/query.rs` 的 `LIMIT` 绑定值 `limit + 1` → 改回 `limit`
  → F1 的新断言**必须红**（`nextCursor` 变 null）。
- **B2**：同一个文件，去掉 `render_filters` 的 ` AND ` 前缀
  → F2 的新断言**必须红**（PG 报 `trailing junk after parameter`）。
- **B3**：`packages/i18n/src/locales/en-US.json` 删掉**某个后端错误码**的 `error.*` 条目
  → `i18n_titles.rs` 的新断言**必须红**。
- **B4**：`packages/i18n/src/locales/zh-CN.json` 删掉**某个后端错误码**的 `error.*` 条目
  （**英文词库保持完好**）→ 新断言**必须红**（证明中文侧真的被检查了）。
- **B5**（可选加分）：`crates/platform-server/tests/audit_chain_real_db.rs` 里让 org 桥接不进 sink
  → F5 的断言必须红。

每个变异都要给**完整证据链**：
```bash
cp <file> /tmp/x.bak                      # ⚠️ 不要用 cp -p（保留旧 mtime → cargo 复用陈旧二进制）
sha256sum <file> > /tmp/sha-before.txt
# 注入 → 跑对应测试 → 确认红
cp /tmp/x.bak <file> && touch <file>      # 还原 + touch
sha256sum -c /tmp/sha-before.txt          # 必须逐字节相同
grep -c 'MUTATION' <file>                 # 必须 0
git status --short                        # 必须干净
```
⚠️ 未跟踪文件 `git checkout` 还原不了 → 新文件做变异**必须先备份 + 记 sha256**。

---

## 5. 🟡 必查 C：修复轮其余 8 条

| # | 查什么 |
|---|---|
| C1 | **F3 / D182**：`audit.page.defaultSize` 的**登记值 == 20**（`crates/platform-config/manifest.json` + `migrations/20261120000400_audit_config_seed.up.sql` + **DB 里的实际行**三处一致）；label 里那句补丁说明**已删** |
| C2 | **F4 / D187**：`crates/platform-config/tests/common/mod.rs` 的夹具复位**只删自己管的 6 个键**；跑完 `platform-config` 测试后，`platform_config` 里 `audit.*` 的 4 个键**仍在**（上轮复验实测是 **count = 0**） |
| C3 | **F5 / D188**：`crates/platform-server/tests/audit_chain_real_db.rs` 里 org 与 config 两条链路**真的进了 `platform_audit_log`**（不是只测了 mock sink）；事件码 / `object_type` / `before`·`after` 形状符合 **D163**；⚠️ 确认它**走的是生产适配器**（`PlatformSinkAuditRecorder` / `ConfigService::with_audit_sink`），不是自己造的等价实现 |
| C4 | **F6 / D189**：`crates/platform-audit/src/query.rs` 顶部注释**不再**写「违反契约 C6 的 `application/problem+json` 要求」 |
| C5 | **F7 / D190**：`crates/platform-audit/tests/audit_chain.rs:1` 的 2 个 U+FFFD 已改回「分」 |
| C6 | **F8 / D191**：`crates/platform-server/src/audit.rs` 的 `TokenVerifier::verify` 已改成**带类型的错误**；`platform-rbac`（`RbacError`）与 `platform-schema`（`AuthError`）的同名 trait **未被波及**（⚠️ 它们是各自独立的 trait，改一个不该动另两个）；**没有引入新依赖** |
| C7 | **F9 / D190**：配置种子迁移里「待 BA 确认」的注释已改成 D181 的口径 |
| C8 | **F10 / D194**：`crates/platform-server/tests/i18n_titles.rs` 的两条新断言是**包含**关系（不是集合相等）；组织事件码用**来源标签**排除；**两个 locale 都查**；读不到词库文件**直接 panic**（不静默返回空集）；`TITLES` 表**一行未改**（仍 84 行），`the_title_table_has_the_pinned_number_of_rows` 的钉死值**未改** |

---

## 6. 🟡 必查 D：越界与回归

| # | 查什么 |
|---|---|
| D1 | **改动范围**：修复轮只应动这些文件 —— `crates/platform-audit/{src/query.rs,tests/audit_chain.rs,tests/query_real_db.rs}`、`crates/platform-config/{manifest.json,src/service.rs,tests/audit_events.rs,tests/common/mod.rs,tests/config_real_db.rs}`、`crates/platform-iam/src/service.rs`、`crates/platform-rbac/tests/audit_events.rs`、`crates/platform-server/{src/audit.rs,tests/audit_chain_real_db.rs,tests/i18n_titles.rs}`、`migrations/20261120000400_audit_config_seed.up.sql`。**有超出这个集合的文件 → 报出来**（不必是错，但要知道） |
| D2 | **逐 crate 全量**（11 个 crate，串行）→ 全 **0 failed 0 ignored**。Dev 报 **925**（上轮基线 912 + 13）—— 你自己算。⚠️ **总数若下降 → 有用例被误删，必须查清** |
| D3 | **前端**：`pnpm --filter @base/shell test` ≥ **206**；`@base/i18n` **43**；`@base/sdk` **38**；`pnpm -r build`；`svelte-check` 0 error |
| D4 | **E2E**：`npx playwright test e2e/` → Dev 报 **35 passed / 0 failed / 0 skipped**（上轮复验是 31 passed / **4 failed**）。⚠️ **任何 skip 的理由只允许「被测系统未就绪」**（D108）；审计页的 6 条用例**必须真跑** |
| D5 | **出口标准**：`cargo clippy --workspace --all-targets` → **新增 warning 必须为 0**（基线 2 条存量）；`cargo fmt --all --check` → 与基线 `develop@0683115` 比对**新增 0 处**（基线本来就红 11 个存量文件 —— TD-017） |
| D6 | `cargo xtask capabilities --check` → 一致；`aw.py verify` → 0 FAIL / 0 WARN |

---

## 7. 🟢 必查 E：上轮复验的「未验证项」能补几个补几个

上轮列了 U-01~U-09（见 `qa-premereq-req014.md` §5）。**能验就验，验不了就如实写「未验证」** ——
**不许写「应该没问题」**。优先尝试：
- **U-03**（org / config 落真表）→ F5 已补用例，**验它是不是真的覆盖了**；
- **U-04**（`cargo xtask audit verify` 不带 `--tenant` 的全租户形态）；
- **U-06**（`deploy/seed/` 其余脚本）。

---

## 8. 交付物

**主交付物** `/home/myg133/Projects/system_seed/BA/feature-meta/qa-recheck-req014.md`：
1. **结论**：PASS / FAIL / PASS-WITH-RISK + 分档汇总（阻塞 / 高 / 中 / 低 各几条）；
2. **逐条问题**：可判定性分档（主体不存在 / 物理不可达 / 不可判定 / 口径不一致 / 表述）+ **证据**（`file:line` 或命令输出）+ **建议改法**；
3. **你实测的逐 crate 数字**（自己跑、自己算）；
4. **变异验证 ≥3 处**（B1~B4 里至少 3 处）的完整证据链；
5. **未验证项清单**（如实写）；
6. 对 **Dev 声明的偏离**的逐条判定（接受 / 拒绝 / 需 BA 裁决）——
   Dev 声明的偏离见 `req-014-dev-fixround.md` §11 与 `req-014-dev-fixround-f10.md` §4。

---

## 9. 停止条件与权限

- **你的角色是「复核者」不是「修复者」**：
  - ❌ **不许**修改 feature worktree 的任何生产代码 / 测试代码 / 迁移 / 配置来「让检查通过」；
  - ❌ **不许**放宽断言、改阈值、跳过用例；
  - ✅ 发现问题 → **写进报告**，附证据与建议改法，**由 BA 决定是否派修复轮**。
- ⚠️ **唯一例外**：§4 的**变异注入**允许临时改动文件，但**必须逐字节还原**
  （`sha256sum -c` 通过 + `git status --short` 干净 + `grep -c 'MUTATION'` 为 0）。
  「不修改」指的是**不得留下任何改动**。
- ✅ **允许写**：`/home/myg133/Projects/system_seed/BA/feature-meta/qa-recheck-req014.md`（**唯一**）。
- ❌ **不许 `git commit`**。
- 遇到无法判定的问题 → 写进报告，**不要**自己拍板改规格。

---

## 10. 回报格式

1. 结论（PASS/FAIL）+ 分档汇总；
2. 阻塞级清单（逐条：AC 号 / 档位 / 证据 / 建议改法）；
3. 你实测的逐 crate 数字；
4. 变异验证 ≥3 处的证据；
5. 未验证项清单；
6. Dev 偏离声明的逐条判定；
7. 已写入的文件（绝对路径）。

---

*本卡的所有判据都写成「可机械判定」的形式 —— 复验的产物是**证据**，不是意见。*
