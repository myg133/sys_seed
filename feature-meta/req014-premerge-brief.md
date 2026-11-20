# REQ-014 Pre-merge 复验任务卡（QA 子 Agent）

- **需求**: REQ-014 审计日志
- **被验对象**: `/home/myg133/Projects/system_seed/feature-REQ-014`，分支 `feature/REQ-014`
- **验收基线**: `BA/demands/REQ-014-audit/acceptance.md` **rev 3**（13 条 AC）
- **决策基线**: `BA/demands/REQ-014-audit/decisions.md` **rev 2**（**D130~D180**）
- **设计基线**: `BA/demands/REQ-014-audit/design-summary.md` rev 2
- **用例基线**: `BA/demands/REQ-014-audit/test-cases/`（120 条，QA 产出）
- **契约**: `BA/contracts/api-conventions/v1.md`（C6 / C7 / C12 / C13）
- **Dev 自验报告**: `feature-meta/req-014-dev-backend-foundation.md` /
  `req-014-dev-backend-integration.md` / `req-014-dev-frontend.md` / `req-014-dev-frontend-round2.md`
- **本卡由 BA（myg133-ba-1）于 2026-11-20 写下**

---

## 0. 复验者的立场

**不采信 Dev 的自验报告。** Dev 报告的数字**至少有一处被 BA 实测证伪**：

| Dev 报的 | BA 实测 | 差 |
|---|---|---|
| `platform-core` 143 | **150** | −7 |
| `platform-iam` 99 | **168** | −69 |
| `platform-audit` 44 | 44 | 0 |

→ **所有数字必须你自己重跑并自己算**：
```bash
cargo test -p <crate> --all-targets -- --test-threads=1 --include-ignored \
  | grep -E '^test result' | awk '{p+=$4; f+=$6; i+=$8} END {printf "passed=%d failed=%d ignored=%d\n", p, f, i}'
```

**BA 实测基线（2026-11-20）**：`platform-core` **150** / `platform-iam` **168** / `platform-audit` **44**，
全 0 failed 0 ignored。`platform-iam` 的 168 与 REQ-013 合并后的基线一致 → **没丢用例**。

---

## 1. 必读

1. `BA/demands/REQ-014-audit/acceptance.md`（rev 3，13 条 AC）
2. `BA/demands/REQ-014-audit/decisions.md`（rev 2，D130~D180）—— **逐条读**
3. `BA/demands/REQ-014-audit/design-summary.md`（rev 2）
4. `BA/demands/REQ-014-audit/test-cases/`（120 条；**注意 `frontend.md` 是 rev 1 口径**，
   已被 D169 + C7 推翻 → 以 D169/C7 为准）
5. `BA/plan/dev-guide-low-spec.md` **§9 出口标准 12 条**
6. `BA/contracts/api-conventions/v1.md`
7. `BA/feature-meta/qa-premereq-req013.md`（**格式参考**，REQ-013 的同款复验报告）

---

## 2. 环境（铁律）

```bash
export XDG_RUNTIME_DIR=/run/user/$(id -u)
export CARGO_TARGET_DIR=/home/myg133/Projects/system_seed/code/target
export DATABASE_URL_TEST="postgres://base:base@127.0.0.1:5432/base_test"
```

- `cargo test --workspace` 在本仓**必失败** → **逐 crate 跑**。
- `--all-targets` 不能省；只能有**一个** `--`；`--include-ignored` 在 `--` **之后**。
- **必须设 `DATABASE_URL_TEST`**，否则真实库用例会 panic。
- 测试库只有 `base_test`；应用连接用 **`base_app`**（超管 `base` 被 `require_isolation` 拒绝，
  且 `base` 是表 owner + `BYPASSRLS` → **用它验 RLS 或 append-only 一定假绿**）。
- 本机**无 psql 客户端** → `podman exec base-postgres psql -U base -d base_test -tAc "…"`
- 低配机（8 线程 / 7.7G）→ **串行**跑，别并发。

---

## 3. 必查清单（按「可判定性」分级，不按严重程度）

> dev-guide §9.12 的五档：**主体不存在 / 物理不可达 / 不可判定 / 口径不一致 / 表述**。
> 前三档的问题**不会自己暴露** —— 「按错的规格实现 + 按错的规格验收」是自洽的。

### 3.1 🔴 必查（这 9 条是本轮真正的雷区）

| # | 查什么 | 怎么查（可机械判定） |
|---|---|---|
| 1 | **append-only 能否被部署脚本破功** | 跑完迁移后**重跑** `deploy/seed/02-app-role.sql`，再用 `base_app` 执行 `UPDATE`/`DELETE` → **必须 permission denied**（AC-S-123 步骤 4~5）。⚠️ 只测迁移后的状态**不算数** |
| 2 | **并发是否分叉** | 两个**独立连接**同时向同一租户各写 1 条 → 两条 `prev_hash` **互不相同**，且第 2 条 == 第 1 条的 `hash`；链校验 `ok == true`（AC-S-124 步骤 5~6）。⚠️ 若实现用的是 `FOR UPDATE` 锁上一条 → **必分叉**（READ COMMITTED 看不到先到者新插入的行） |
| 3 | **敏感键的驼峰绕过口** | `{"apiKey":"x"}` **必须命中**（D172）；`{"secretary":"x"}` / `{"tokenizer":"y"}` **必须不命中**（完整键名匹配，非子串）。两侧（Rust + DB）**都要测** + **一致性测试真的存在** |
| 4 | **`tip_hash` 截断检出** | 在线表**末尾删掉一条** → 链校验必须报出截断（D171）。⚠️ 若 `tip_hash` 没实现或没维护 → **检不出**，是「不可篡改」承诺的漏 |
| 5 | **链校验的 reason 取值** | 改 `after` → `hash_mismatch`；改存储 `hash` → 第 2 条 `hash_mismatch` **且** 第 3 条 `prev_hash_mismatch`（D146）。两个 reason **都要有真实触发路径**，不能有死值 |
| 6 | **查询接口的分页口径** | `?limit=2` → **2 条 + total + nextCursor**；`?cursor=&limit=2` → **不与上一页重复**；`?cursor` 与 `?page` 同给 → **400**；`?limit=0` / `?limit=abc` → **400 + `AUDIT_INVALID_FILTER`**；`?limit=1000` → **夹到 200**；`?sort=unknownField` → **400**；`?tenant_id=…` → **被忽略**（D162）；**游标 + 非默认 `sort`** → **400**（D179） |
| 7 | **400/403 的响应体形态** | 必须是 **RFC 9457 Problem Details 形状**（`code`/`status`/`detail`/`instance`/`trace_id`，契约 C6）。⚠️ axum 的 `QueryRejection` 是**纯文本** → 若实现直接用 `Query<T>` 反序列化，**C6 被破**。🔑 **Content-Type 跟随平台现状 `application/json`**（D183 —— C6 **没有**规定 Content-Type，别按 `application/problem+json` 判） |
| 8 | **未注册 `/api/*` 的回落** | `GET /api/does-not-exist` → **404**，**不是** 200 + HTML（D180）。同时确认静态资源（`/`、`/assets/**`）仍正常回落 |
| 9 | **C-5 契约检查** | `crates/platform-audit/src/` 里必须有 `require_permission("字面量")` 调用点（3 个权限点）。跑 `platform-config` 的契约测试看它是否红 |

### 3.2 🟡 必查（口径一致性）

| # | 查什么 |
|---|---|
| 10 | **权限点三处齐**：`crates/platform-audit/manifest.json` 的 `permissions[]` + 迁移的 INSERT + `source` 列 == `'crates/platform-audit/manifest.json'`（D27 漏任一处 → **启动失败**） |
| 11 | **配置项登记**：`audit.retention.*` / `audit.page.*` 在 `crates/platform-config/manifest.json` **和** 迁移 INSERT 里**都**有（D178） |
| 12 | **`TITLES` 条数** == **84**（79 + 5 个新错误码）。⚠️ 计数**必须**用宽松正则 `\(\s*"([^"]*)"\s*,\s*`（全大写正则会漏 6 条 `org.*`） |
| 13 | **13 个错误码的 8 处编辑点**（D125）一个不缺 |
| 14 | **既有 recorder trait 签名未被破坏**（`platform-org` 的 `AuditRecorder`、`platform-iam` 的 `SsoAuditRecorder`）→ 既有测试全绿（AC-S-129 步骤 6） |
| 15 | **`sso_audit.rs` 那条断言的修改**（D177）保留了**原意图**（不许偷偷建 SSO 专用表）**且加了阳性对照**（平台级通用审计表应当存在） |
| 16 | **登录审计**：成功/失败各 1 条；`ip`/`userAgent` 逐字等于 `X-Forwarded-For: 203.0.113.9` / `User-Agent`；**全列文本**里不含密码/token；**失败记录不含账号名**（不可枚举）；**审计写失败 → 拒绝登录 503**（D157） |
| 17 | **权限变更审计**：授予/撤销/创建/删除**各 1 条**；事件码 / `object_type` / `before`·`after` 形状**逐字**符合 D163 |
| 18 | **归档**：单租户（跨租户调用**必须**被拒 —— `SECURITY DEFINER` 绕过 RLS，靠**函数内显式守卫**）；幂等；归档表 `UPDATE`/`DELETE` 被拒；归档表列集合 == 在线表 ∪ `{archived_at}`；**归档后链校验仍 `ok == true`** |
| 19 | **RLS**：用 `base_app` **在单事务内** `set_config` → 查询（裸连接设一次就查 = **事务结束即失效 → 恒 0 行 → 假绿**）；不设上下文 → **0 行不报错**；写侧 `WITH CHECK` 生效；归档表同样受约束 |
| 20 | **敏感键硬拒绝**：`before` 含 `{"password":"..."}` → **写入被拒**（明确错误码，非 500）**且未落库**；正常事件正常落库 |
| 21 | **业务模块不写审计表**：扫 `crates/**/*.rs` → `INSERT INTO platform_audit_log` **0 命中**；扫 `migrations/**/*.sql` → **恰好 1 处**；**两条扫描都要有阳性对照** |

### 3.3 🟢 必查（回归与一致性）

| # | 查什么 |
|---|---|
| 22 | **逐 crate 全量回归**（自己跑、自己算数字）：`platform-core` `platform-db` `platform-iam` `platform-org` `platform-rbac` `platform-config` `platform-schema` `platform-server` `platform-audit` `sandbox` —— 全 **0 failed 0 ignored** |
| 23 | **前端**：`pnpm --filter @base/shell test` ≥ **206**；`@base/i18n` **43**（错误域条目 **84** == 抽样列表 **84**）；`@base/sdk` **38**；`pnpm -r build` 通过；`svelte-check` 0 error |
| 24 | **E2E**：`npx playwright test e2e/` → **全绿且审计页的 5 条数据用例**从 skip 变成**真跑通过**（后端就绪后）。⚠️ 任何 skip 的理由**必须**是「被测系统未就绪」，**不得**有「夹具不对」「授权没配好」（D108） |
| 25 | **`cargo fmt --all --check`** 与 **`cargo clippy`**：与基线 @ `0683115` 比对，**新增**问题必须为 0（基线 fmt 本来就红 11 个存量文件 —— TD-017） |
| 26 | **`cargo xtask capabilities --check`** → 一致（`capabilities/permissions.json` 应含 3 个新权限点） |
| 27 | **`aw.py verify`** → 0 FAIL / 0 WARN |

---

## 4. 复验方法（本项目的既有规矩）

### 4.1 变异验证 —— **必须自己注入，不采信 Dev 的**

对**生产那一行**注入最小变异 → 跑测试确认变红 → 还原 → 确认逐字节相同。

```bash
cp -p <file> /tmp/x.bak                 # 备份
sha256sum <file> > /tmp/sha-before.txt
# 注入最小变异（打生产那一行，不是抽出来的函数）
cp /tmp/x.bak <file> && touch <file>    # 还原 + touch（9.4：cp -p 保留 mtime → cargo 复用陈旧二进制）
sha256sum -c /tmp/sha-before.txt       # 必须逐字节相同
grep -c 'MUTATION' <file>               # 必须 0
git status --short                      # 必须干净
```

⚠️ **未跟踪文件 `git checkout` 还原不了** → 新建文件做变异**必须先备份**并给 `sha256` 基线。

**建议优先变异的位置**（最容易被假绿掩盖的）：
1. `pg_advisory_xact_lock` 那行**去掉** → 并发用例必须红；
2. `REVOKE UPDATE, DELETE` 那行**去掉** → append-only 用例必须红；
3. `prev_hash_mismatch` 的判定改成**永不命中** → 篡改用例必须红；
4. 敏感键检查的**驼峰拆词**那一步去掉 → `apiKey` 用例必须红；
5. `tip_hash` 的比较改成恒真 → 截断用例必须红。

### 4.2 静态扫描必须带**阳性对照**（dev-guide §9.2）

「0 命中」与「扫描器根本没生效」在输出上**完全一样**。
→ 每条扫描断言都要有一条**「对已知含目标样例必然命中」**的用例作阳性对照。

### 4.3 回归数字

- 逐 crate 跑，**串行**（`-- --test-threads=1`）；`platform-org` / `platform-iam` 在并发下有 CPU 竞争 flake。
- 数字用 4.1 之外那条 `awk` 口径**自己算**。

---

## 5. 交付物

1. `/home/myg133/Projects/system_seed/BA/feature-meta/qa-premereq-req014.md` —— 复验报告：
   - 结论（**PASS / FAIL**）+ 汇总表（阻塞 / 高 / 中 / 低 各自条数）
   - 逐条问题的**可判定性分档** + **证据**（`file:line` / 命令输出）+ **建议改法**
   - 你**实测**的逐 crate 数字（不是 Dev 报的）
   - 变异验证 ≥3 处的证据链
   - 你**没验到**的项（明确写「未验证」，不许写「应该没问题」）
2. `/home/myg133/Projects/system_seed/BA/demands/REQ-014-audit/test-cases/frontend.md` ——
   把 rev 1 口径（`page=2` / 具名过滤参数）**更新到 rev 3 口径**（游标 + `filter[field][op]`），
   并在文件头注明「按 D169 + C7 修订」。**只改这一份**，其余用例文件不要动。

---

## 6. 停止条件

- 发现**阻塞级**问题（AC 不可达 / 安全缺口 / 假绿）→ **先写完报告**，不要顺手改代码。
- 复验**不修改** feature worktree 的任何文件（除上面第 5.2 明确要求的 `test-cases/frontend.md`）。
- 复验前后 `git status` 应保持一致（除了 `test-cases/frontend.md`）。
- 遇到无法判定的问题 → 写进报告，**不要**自己拍板改规格。

---

## 7. 回报格式

1. 结论（PASS/FAIL）+ 分档汇总
2. 阻塞级清单（逐条：AC 号 / 档位 / 证据 / 建议改法）
3. 你实测的逐 crate 数字
4. 变异验证 ≥3 处的证据
5. 未验证项清单
6. 已写入的文件（绝对路径）
