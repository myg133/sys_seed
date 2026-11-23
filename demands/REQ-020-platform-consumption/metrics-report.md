# REQ-020 度量报告

- 需求：[REQ-020 底座可用性验证](demand.md)
- 分支：`feature/REQ-020`
- 工作树：`/home/myg133/Projects/system_seed/feature-REQ-020`
- 数据采集时间：2026-11-23（本批次）
- 口径：只纳入**可机械复现**的指标（D350）；「上手时间」「AI 一次过率」**不纳入**。

## 基线（两个 sha）

| 基线 | sha | 说明 |
|---|---|---|
| **`BASE_IMPL`** | `9bcfbcb` | 本分支的**实现起点**（`[BA] 分配 REQ-020`）。其父 `1be47f5` = `develop` 当前 tip，二者只差 `feature-meta/manifest.json`（BA 分派文件，不是实现改动） |
| **`BASE_UPGRADE`** | **`fd0d099`** | REQ-020（1/2）：消费方 + 平台装配 + 通知落库（47 文件）。**AC-S-190 以此 sha 为基线** |
| 被测 commit | **`52b9314`**（HEAD） | REQ-020（2/2）：平台 v1→v2 升级 + 弃用登记（5 文件） |

> ⚠️ `BASE_IMPL` 取 `9bcfbcb` 而不是 `develop`（`1be47f5`）的理由：后者会多算 1 个
> 非实现文件（`feature-meta/manifest.json`）。两个口径的差值在下文「平台改动」里写清。
>
> ✅ 提交顺序符合 acceptance.md「两个基线」一节的要求：
> `fd0d099`（消费方 + 装配）**先**提交，`52b9314`（v2 升级）**后**提交
> → 因此 `git diff fd0d099 -- modules/leave/` 的语义正是「升级阶段消费方是否被改动」。

## 指标表

| 指标 | 值 | 取数命令 |
|---|---|---|
| 后端业务代码 | **1374 行** | `find modules/leave/backend/src -name '*.rs' \| xargs wc -l \| tail -1` |
| 前端业务代码 | **604 行** | `find modules/leave/frontend -type f \( -name '*.svelte' -o -name '*.ts' \) \| xargs wc -l \| tail -1` |
| ①②合计 | **1978 行**（预算 500，**超出 1478 行**） | 上两条相加 |
| SQL 迁移（业务模块） | **42 行** | `find modules/leave/migrations -name '*.sql' \| xargs wc -l \| tail -1` |
| 平台改动（全仓，含新文件） | **30 个文件** | `git diff --name-only 9bcfbcb 52b9314 \| grep -v '^modules/leave/' \| grep -v '^feature-meta/' \| wc -l` = **30**（提交后单条命令即可，不再需要「diff + ls-files 未跟踪」两段相加） |
| 其中 A 类（可避免） | **0** | 见「平台改动分类」逐条审视 |
| 依赖面 | `platform-core` / `platform-db`（运行期）、`platform-contracts`（dev） | `grep -E '^platform-' modules/leave/Cargo.toml` |
| 契约检查 | 退出码 **0 / 0 / 0** | `cargo xtask check-modules && cargo xtask check-deps && cargo xtask capabilities --check` |
| 已改 crate 的测试 | **684 passed / 0 failed / 0 ignored** | 见下方逐 crate 循环（6 个 crate） |
| Shell 前端单测 | **422 passed / 29 files** | `pnpm --filter @base/shell test --run` |
| E2E skipped | **0**（1 passed / 0 failed） | `npx playwright test e2e/leave.spec.ts --reporter=json` + `jq '.stats.skipped'` |

> ⚠️ AC-S-191 给的 skipped 命令 `[..|.skipped?//empty|select(.!=0)]|length`
> 在 jq 1.8.2 上是**语法错误**（`?//` 被解析成解构备选运算符）。
> 等价且可执行的写法（实测输出 `0`）：
> `jq '[.. | .skipped? // empty | select(. != 0)] | length'`
> 另 `stats.skipped` 也直接是 `0`（Playwright 自己聚合的口径）。

## 原始命令输出（逐条实跑）

```console
$ git rev-parse 9bcfbcb
9bcfbcbffd355407b92fab39904ea52e2063c3e7

$ find modules/leave/backend/src -name '*.rs' | xargs wc -l | tail -1
 1374 总计

$ find modules/leave/frontend -type f \( -name '*.svelte' -o -name '*.ts' \) | xargs wc -l
   35 modules/leave/frontend/routes.ts
  219 modules/leave/frontend/lib/leave-api.ts
  350 modules/leave/frontend/pages/LeavePage.svelte
  604 总计

$ find modules/leave/migrations -name '*.sql' | xargs wc -l | tail -1
  42 总计

$ git diff --name-only 9bcfbcb | grep -v '^modules/leave/' | wc -l
23

$ git ls-files --others --exclude-standard | grep -v '^modules/leave/' | grep -v '^feature-meta/' | wc -l
7

# 未跟踪的 7 个（`git diff` **看不到**，所以上一条命令会少算）：
apps/shell/test/leave-page.test.ts
crates/platform-notify/src/db_sender.rs
crates/platform-notify/tests/db_sender_real_db.rs
crates/platform-server/tests/leave_real_db.rs
e2e/leave.spec.ts
migrations/20261123000200_leave_permission_seed.down.sql
migrations/20261123000200_leave_permission_seed.up.sql

# 若用 develop 的 tip 作基线（多算 1 个 BA 分派文件 feature-meta/manifest.json）：
$ git diff --name-only develop | grep -v '^modules/leave/' | wc -l
24

$ grep -E '^platform-' modules/leave/Cargo.toml
platform-core.workspace = true
platform-db.workspace = true
platform-contracts.workspace = true
# 其中 platform-contracts 只出现在 [dev-dependencies]（modules/leave/Cargo.toml:41-43）

$ cargo xtask check-modules; echo $?
模块目录校验通过：2 个模块
  ✓ leave
  ✓ sandbox
0

$ cargo xtask check-deps; echo $?
依赖方向校验通过：平台 crate 未反向依赖业务模块
0

$ cargo xtask capabilities --check; echo $?
能力索引校验通过：14 个文件与源码一致
0
# capabilities/ 未被手工改动；本批只改了前端与夹具，Rust 源码的 capabilities 无变化。

$ for c in platform-core platform-notify platform-rbac platform-observability leave platform-server; do
    cargo test -p $c --all-targets -- --test-threads=1 --include-ignored 2>&1 \
      | grep 'test result' | awk -v c=$c '{p+=$4;f+=$6;i+=$8} END{printf "%s: %d/%d/%d\n", c, p, f, i}'
  done
platform-core: 153/0/0
platform-notify: 149/0/0
platform-rbac: 86/0/0
platform-observability: 127/0/0
leave: 25/0/0
platform-server: 144/0/0

$ pnpm --filter @base/shell test --run
 Test Files  29 passed (29)
      Tests  422 passed (422)

$ npx playwright test e2e/leave.spec.ts --reporter=json
# stats: {"expected":1,"skipped":0,"unexpected":0,"flaky":0}

$ npx playwright test e2e/leave.spec.ts --reporter=list
  ✓  1 [chromium] › e2e/leave.spec.ts:105:1 › 请假：列表 → 建单 → 审批通过 → 刷新保持（AC-S-192） (3.8s)

  1 passed (12.4s)
```

## 终版实测（提交后，代码冻结于 `52b9314`）

> 口径（dev-guide §9.14）：**全量只跑一次，且跑期间代码未变动**。
> 本节数字在 **`52b9314`**（代码冻结）与**合并后的 `develop` @ `bd2d81a`** 上**各跑一次**，
> 两次逐 crate 结果**逐个相同**（见文末「口径说明」）。跑之前 `git status --porcelain` 为空。

### 全量：19 个 crate，1673 passed / 0 failed / 0 ignored

逐 crate 跑（本仓 `cargo test --workspace` 必然失败，见下）：
`cargo test -p <crate> --all-targets -- --test-threads=1 --include-ignored`

| crate | passed | failed | ignored |
|---|---:|---:|---:|
| platform-core | 153 | 0 | 0 |
| platform-audit | 59 | 0 | 0 |
| platform-config | 87 | 0 | 0 |
| platform-contracts | 32 | 0 | 0 |
| platform-contracts-macros | 0 | 0 | 0 |
| platform-app | 74 | 0 | 0 |
| platform-notify | 149 | 0 | 0 |
| platform-storage | 159 | 0 | 0 |
| platform-observability | 127 | 0 | 0 |
| platform-db | 48 | 0 | 0 |
| platform-iam | 181 | 0 | 0 |
| platform-oauth | 130 | 0 | 0 |
| platform-org | 95 | 0 | 0 |
| platform-rbac | 86 | 0 | 0 |
| platform-schema | 20 | 0 | 0 |
| platform-server | 144 | 0 | 0 |
| xtask | 71 | 0 | 0 |
| sandbox | 33 | 0 | 0 |
| **leave**（新增） | **25** | 0 | 0 |
| **合计** | **1673** | **0** | **0** |

- `platform-contracts-macros` 为 0 是**正常**：它是 proc-macro crate，测试在被展开的
  `platform-contracts` 侧（32 passed）。
- 与 `develop` 基线的对账：REQ-019 全量是 **1631**，本分支 **1673**，差 **+42**。
  可逐项归因的部分：`leave` 新增 **25**；平台侧新增测试函数 **13**
  （`platform-notify` +5、`platform-server` +6、`platform-rbac` +1、`platform-observability` +1，
  取数 `git diff 1be47f5 HEAD -- <crate> | grep -cE '^\+.*#\[(tokio::)?test\]'`）。
  **余下 4 条差额我未逐条归因** —— 原因是存在**宏 / 表驱动生成**的测试，
  `#\[test\]` 计数会低估（例：`leave` 实测 25 passed，但同法只数到 19）。
  不影响「0 failed / 0 ignored」的结论。

### 守护：4 项，退出码全 0

```console
$ cargo xtask check-modules            → 2 个模块（leave / sandbox）    exit=0
$ cargo xtask check-deps               → 平台 crate 未反向依赖业务模块  exit=0
$ cargo xtask capabilities --check     → 14 个文件与源码一致            exit=0
$ DATABASE_URL=…base… cargo xtask tenant-policies --check
  → 警告：`base` 是超级用户/BYPASSRLS，会 fail open → 需用应用角色复检
$ DATABASE_URL=…base_app… cargo xtask tenant-policies --check
  → 9 个模块声明的表都开了 RLS 且有策略（**无警告**）                     exit=0
```

> ⚠️ `tenant-policies` 用 `base` 跑会**通过但带 fail-open 警告** ——
> 单看退出码会得到假绿灯。必须用应用角色 `base_app` 再跑一次。

**`leave` 的租户隔离是平台生成的**（本需求的正面证据）：
`modules/leave/migrations/20261123000100_leave_request.up.sql:3-5` 明确写「只建表，不建 RLS policy，
由平台统一生成」；实测库里 `leave_request` 的 `relrowsecurity = t`、
策略 `leave_request_tenant_isolation` 存在，来源是 `modules/leave/manifest.json` 的
`tenantScoped.tables = ["leave_request"]` → `cargo xtask tenant-policies` 施加。
**模块侧零 RLS SQL**。

### 前端：422 passed / 0 errors / E2E 1 passed 0 skipped

```console
$ pnpm --filter @base/shell test --run     → 29 files / 422 passed
$ pnpm --filter @base/shell check          → svelte-check found 0 errors and 0 warnings
$ DATABASE_URL_TEST=… npx playwright test e2e/leave.spec.ts --reporter=json
  → stats: {"expected":1,"skipped":0,"unexpected":0,"flaky":0}
```

> ⚠️ E2E 的 `webServer` 命令**必须**带 `DATABASE_URL_TEST`，否则
> `scripts/e2e-server.sh` 在参数检查处直接 `exit 2`，Playwright 报
> 「Process from config.webServer was not able to start」——
> 看起来像页面坏了，其实是夹具前置缺失（本批实测踩到一次）。

### 平台改动面（提交后单条命令复算）

```console
$ git diff --name-only 9bcfbcb 52b9314 | grep -v '^modules/leave/' | grep -v '^feature-meta/' | wc -l
30
$ git diff --name-only 9bcfbcb 52b9314 | grep -c '^modules/leave/'
18
$ git diff --name-only 9bcfbcb 52b9314 | wc -l
52          # = 18 (leave) + 30 (平台) + 4 (feature-meta/*-progress.md 过程记录)
```

**30 = 下表 28 项 + 2 个生成物**（`capabilities/index.json`、`capabilities/permissions.json`）——
逐项相等，无漏项、无虚报。4 个 `feature-meta/req020-batch*-progress.md` 是过程记录，
**不计入**平台改动。

## 超过 500 行的归因（AC-S-187：**判不成立**）

**结论：不成立。** ① + ② = **1978 行**，超出 500 行预算 **1478 行**（3.96 倍）。

### 逐文件构成（总行 / 注释+空行 / 内联测试 / 非注释代码）

| 文件 | 总行 | 注释+空行 | 内联 `#[cfg(test)]` | 非注释代码 |
|---|---:|---:|---:|---:|
| `backend/src/service.rs` | 491 | 144 | 0 | 347 |
| `backend/src/http.rs` | 307 | 88 | 0 | 219 |
| `backend/src/repository.rs` | 253 | 68 | 83 | 102 |
| `backend/src/entity.rs` | 200 | 55 | 45 | 100 |
| `backend/src/mod.rs` | 99 | 56 | 0 | 43 |
| `backend/src/permission.rs` | 24 | 16 | 0 | 8 |
| **后端小计** | **1374** | **427** | **128** | **819** |
| `frontend/lib/leave-api.ts` | 219 | 96 | 0 | 123 |
| `frontend/pages/LeavePage.svelte` | 350 | 62 | 0 | 288 |
| `frontend/routes.ts` | 35 | 24 | 0 | 11 |
| **前端小计** | **604** | **182** | **0** | **422** |
| **总计** | **1978** | **609** | **128** | **1241** |

取数命令：

```bash
wc -l modules/leave/backend/src/*.rs modules/leave/frontend/lib/*.ts \
      modules/leave/frontend/pages/*.svelte modules/leave/frontend/routes.ts
# 注释+空行：grep -cE '^\s*(//|$)' <file>（.svelte 另计 /* * <!-- -->）
# 内联测试：grep -n '^#\[cfg(test)\]' <file> 的行号 → 总行 - 行号 + 1
```

### 归因（样板 vs 业务）

| 项 | 行数 | 必需样板 | 业务逻辑 |
|---|---:|---:|---:|
| 脚手架**模板体积**（`crates/xtask/src/scaffold.rs` 常量） | 模板 458（非空 396） | 真实保留 **107** | 其余为在模板上重写/扩写 |

> ⚠️ **这一行在 rev 1 里写错了，已按独立复核 F-02 修正。**
> rev 1 写「必需样板 458 / 业务逻辑 0」，把**模板体积**当成了**模块内实际存在的样板**。
> 逐行比对（去空白后精确匹配、带重数）的实际结果：
>
> | 骨架常量 | 模板非空行 | 真实文件非空行 | 逐行重合 |
> |---|---:|---:|---:|
> | `REPOSITORY_RS` | 172 | 229 | 29 |
> | `SERVICE_RS` | 58 | 457 | 15 |
> | `BACKEND_MOD_RS` | 46 | 85 | 34 |
> | `HTTP_RS` | 49 | 280 | 11 |
> | `PERMISSION_RS` | 16 | 20 | 4 |
> | `ENTITY_RS` | 15 | 185 | 5 |
> | `FRONTEND_ROUTES_TS` | 23 | 30 | 5 |
> | `FRONTEND_PAGE_SVELTE` | 17 | 327 | 4 |
> | **合计** | **396** | **1613** | **107（27%）** |
>
> 复算命令见本节末尾。
> `REPOSITORY_RS` 重合最低（29/172）的原因很具体：骨架的范例是**内存 `Vec` 存储、不引入 sqlx**，
> 而真实实现必须走 `TenantQuery`（租户占位符 + RLS）→ 范例整体不可用，等于重写。
| 4 个端点的权限分组 + 409 映射 | — | 部分 | `http.rs` 219 行非注释 |
| 三态状态机 + 审计 / 通知编排 + 错误码映射 | — | 0 | `service.rs` 347 行非注释 |
| CAS 状态更新 + `TenantQuery` 构造 | — | 0 | `repository.rs` 102 行非注释 |
| 实体 / 闭集 / `FromRow` / 日期解析 | — | 0 | `entity.rs` 100 行非注释 |
| 页面（列表 + 表单 + 审批按钮） | — | 部分 | `LeavePage.svelte` 288 行非注释 |
| 前端接口封装 + 解析 + 校验 | — | 0 | `leave-api.ts` 123 行非注释 |

**关键事实（不粉饰）**：

1. **把注释、空行、内联测试全部剔除后仍是 1241 行**，仍超 500 的 2.48 倍。
   → 「注释多导致超标」**不成立**，超标是结构性的。
2. **骨架模板没有帮上忙，反而是负担**：模板非空 396 行里只有 **107 行（27%）**
   逐行留在模块代码中；`REPOSITORY_RS` 只留 29/172。
   原因是骨架范例写的是**内存存储**，真实实现必须走 `TenantQuery` →
   「照着骨架抄」的收益接近于零。**样板偏重的根因不是「模板太大」，
   而是「模板的例子不能直接用」+ 真实 DB 化实现必须自带 CAS / 审计 / 通知编排。**
3. 单表、无明细行（D348）已经是最小实体；没有可再砍的业务范围。

骨架重合度的复算命令（模板非空行 vs 真实文件非空行的**带重数**精确匹配）：

```bash
cd /home/myg133/Projects/system_seed/feature-REQ-020
python3 - <<'PY'
import re
src=open('crates/xtask/src/scaffold.rs').read()
consts=dict(re.findall(r'const\s+(\w+):\s*&str\s*=\s*r#"(.*?)"#;', src, re.S))
m={'REPOSITORY_RS':'modules/leave/backend/src/repository.rs',
   'SERVICE_RS':'modules/leave/backend/src/service.rs',
   'BACKEND_MOD_RS':'modules/leave/backend/src/mod.rs',
   'HTTP_RS':'modules/leave/backend/src/http.rs',
   'PERMISSION_RS':'modules/leave/backend/src/permission.rs',
   'ENTITY_RS':'modules/leave/backend/src/entity.rs',
   'FRONTEND_ROUTES_TS':'modules/leave/frontend/routes.ts',
   'FRONTEND_PAGE_SVELTE':'modules/leave/frontend/pages/LeavePage.svelte'}
for k,p in m.items():
    sk=[l.strip() for l in consts[k].splitlines() if l.strip()]
    real=[l.strip() for l in open(p).read().splitlines() if l.strip()]
    bag={}
    for l in real: bag[l]=bag.get(l,0)+1
    ov=0
    for l in sk:
        if bag.get(l,0)>0: bag[l]-=1; ov+=1
    print(f"{k:<22} 模板 {len(sk):>4} / 真实 {len(real):>4} / 重合 {ov:>3}")
PY
# 输出：模板合计 396 / 真实合计 1613 / 重合合计 107
```

**这就是本需求该产出的结论：底座样板代码偏重**（归因见「底座缺口清单」第 5 项）。

## 平台改动分类（AC-S-186 的产出）

> 判据：`git diff --name-only 9bcfbcb 52b9314 | grep -v '^modules/leave/' | grep -v '^feature-meta/'`
> 的集合必须与下表**逐项相等**（28 项手改 + 2 个生成物 = 30）。
> 提交前这 7 个是未跟踪文件、`git diff` 看不见，需要两段相加；**提交后单条命令即可**。

| # | 类 | 文件 | 为什么必需（删掉它，哪条 AC 判不了） |
|---|---|---|---|
| 1 | C | `Cargo.toml` | 不加 `members = "modules/leave"`，crate 不在工作区 → 编译不过 |
| 2 | C | `Cargo.lock` | 新 crate 的锁文件条目 |
| 3 | C | `crates/platform-server/Cargo.toml` | 不加 `leave.workspace = true`，装配臂调不到 `leave::mount` |
| 4 | C | `crates/platform-server/src/main.rs` | 无 `"leave"` 装配臂 → `/api/leave/*` 全 404（AC-S-189/192 无从谈起） |
| 5 | C | `apps/shell/src/lib/routes.ts` | 无 `/leave` 路由 → E2E 打不开页面（M7/D356） |
| 6 | C | `apps/shell/src/App.svelte` | 无渲染分支 → 路由命中但没有页面组件 |
| 7 | C | `apps/shell/tsconfig.json` | 方案 B：页面在模块目录，`include` 不加 → svelte-check 漏检该页面 |
| 8 | C | `apps/shell/test/leave-page.test.ts` | 页面 / 接口层单测（含「审批只走 decide」的断言） |
| 9 | C | `e2e/leave.spec.ts` | AC-S-192 的被测对象本身 |
| 10 | C | `scripts/seed-e2e.sh` | 没有申请人 / 审批人两个账号，AC-S-192 步骤 3（切账号）无法执行 |
| 11 | C | `migrations/20261123000200_leave_permission_seed.up.sql` | 权限点不落 `platform_permission` → 启动对账 fail-fast，服务起不来；且没人能持有 leave 权限点 |
| 12 | C | `migrations/20261123000200_leave_permission_seed.down.sql` | 上一条的可逆配套 |
| 13 | C | `crates/platform-server/tests/router_assembly.rs` | 挂载点断言（变异验证：去掉装配臂 → 红） |
| 14 | C | `crates/platform-server/tests/leave_real_db.rs` | AC-S-189 计数表（真库查审计 / 通知表） |
| 15 | C | `crates/platform-notify/tests/db_sender_real_db.rs` | D354 适配器的真库测试（没有它，「落库」无法自证） |
| 16 | C | `crates/platform-notify/tests/common/mod.rs` | 上述测试的共享夹具 |
| 17 | B | `crates/platform-rbac/src/http.rs` | 无 v2 端点 → AC-S-190 判不了（D352） |
| 18 | B | `crates/platform-rbac/src/service.rs` | `sources` 要真算角色来源（`load_graph` / `user_role_ids` 是 service 私有） |
| 19 | B | `crates/platform-rbac/tests/me_permissions.rs` | v1 不变 + v2 语义的断言 |
| 20 | B | `crates/platform-observability/src/http.rs` | v1 不登记弃用 → 没有 `Deprecation` / `Sunset` 头（AC-S-190 步骤 3） |
| 21 | B | `crates/platform-observability/tests/http_contract.rs` | 弃用头 / Sunset 格式的断言 |
| 22 | D | `crates/platform-notify/src/lib.rs` | 导出 `DbNotificationSender` |
| 23 | D | `crates/platform-notify/src/db_sender.rs` | 通知落库适配器本体（D354；没有它 AC-S-189 的通知计数查不到真表） |
| 24 | D | `crates/platform-core/src/titles.rs` | `LEAVE_INVALID_TRANSITION` 不注册 → `title` 落兜底 `"Error"`，违反契约 C6 |
| 25 | D | `crates/platform-core/src/problem.rs` | 该码不进 `ALL_CODES` → 完备性测试红 |
| 26 | C | `crates/platform-server/tests/i18n_titles.rs` | 新错误码的标题文案断言（**归 C 的规则见下方「分类规则」**） |
| 27 | D | `packages/i18n/src/locales/zh-CN.json` | 错误码的用户可见文案（键集合两语言必须一致） |
| 28 | D | `packages/i18n/src/locales/en-US.json` | 同上（缺一个键 → i18n 键一致性测试红） |
| — | 生成物 | `capabilities/index.json` | 由 `cargo xtask capabilities` 重新生成，**非手改** |
| — | 生成物 | `capabilities/permissions.json` | 同上 |

**A 类（可避免）= 0**：逐条审视后，没有一条能换成「平台已有机制」而不改平台
（装配是硬编码 match、通知无落库入口、schema 无行内动作 —— 三个缺口都是实测的，
见下节）。

#### 分类规则（避免同一性质被分到两类）

- **B**（升级示范）：为 AC-S-190 造的 v1→v2 升级所需的平台改动。
- **C**（装配必需）：不做它，`/api/leave/*` 或页面根本不通。
  **测试文件（判据载体）统一归 C** —— 它们是「让某条 AC 可判」的装配，不是平台新能力。
  这条规则是独立复核 **F-09** 指出后补的：rev 1 把 `i18n_titles.rs` 记为 D、
  其余 4 个同类载体记为 C，同性质两类，已统一。
- **D**（补平台能力）：为满足契约或可用性，平台**新增**的能力
  （通知落库适配器、错误码注册、i18n 文案）。
- **生成物**：由 `cargo xtask capabilities` 重新生成，不手改。
- **A**（可避免）：能用平台已有机制替代却改了平台。**必须为 0。**

分类分布：**C = 17 / B = 5 / D = 6 / 生成物 = 2，合计 30**。

**不计入平台改动**：`feature-meta/*-progress.md`（过程记录，4 个文件）。
若按 `develop` 作基线，还会多 `feature-meta/manifest.json`（BA 分派提交，1 个）。

### ⚠️ 与 design-summary §3 登记清单的差异（需 BA 补登记）

本表是**实际**改动集；`design-summary.md` §3 的清单是设计期的**计划**集。
差异（均已在批次 1~3 的进度文件里如实记录）：

| 实际多出 | 类 | 原因 |
|---|---|---|
| `crates/platform-rbac/src/service.rs` | B | `sources` 无法只改 http 层（批次 3 §4a） |
| `migrations/20261123000200_leave_permission_seed.*` | C | 权限点种子（否则启动对账失败） |
| `packages/i18n/src/locales/*.json` | D | 新错误码文案 |
| `scripts/seed-e2e.sh` | C | E2E 两个账号夹具 |
| `apps/shell/tsconfig.json` | C | 方案 B 的 `include` |
| `apps/shell/test/leave-page.test.ts`、`e2e/leave.spec.ts`、3 个 Rust 测试文件 | C | 判据的可执行载体 |
| `crates/platform-notify/tests/common/mod.rs` | C | 测试夹具 |

## 底座缺口清单（本需求的核心产出）

| # | 缺口 | 实测证据 | 影响 | 建议 |
|---|---|---|---|---|
| 1 | 业务模块装配是**硬编码 match** | `crates/platform-server/src/main.rs` 的 `"leave" => {...}` 臂；`discover_enabled()` 能发现模块但**挂载要手写** | **装配点 3 个**（根 `Cargo.toml` / `platform-server/Cargo.toml` / `main.rs`）；**但新模块的平台改动面不止 3 个**：再加权限种子迁移 2、生成物 2、自定义 UI 时 `apps/shell` 3、判据载体约 5 —— 实测 C 类共 **17** 项（独立复核 F-10：rev 1 只写了「3 个文件」，是装配子集，会低估排期） | **TD-086**：模块自注册路由 |
| 2 | 通知**无落库适配器** | `NotificationSender` 只有 `channel()` / `send()`；`enqueue` 在禁止依赖的 crate | 业务发通知落不了库，AC-S-189 计数查不到真表 | **已补**（D354，`DbNotificationSender`）；仍建议把「落库 sender」做成平台默认装配 |
| 3 | manifest schema **无行内动作** | `platform-core/src/extension/schema.rs` 的 `api` 只有 `list/create/update/delete` | 带审批的业务必须写自定义页 + 改 `apps/shell/**` | **TD-087**：扩展 schema `actions` |
| 4 | **无法按权限点反查用户** | `PermissionChecker` 只有 `has` / `scope` / `effective_scope` | 「提交时通知审批人」不可实现 → 只改了「审批结果通知申请人」 | **TD-088**：权限点 → 用户集合的反查接口 |
| 5 | **样板代码偏重** | 脚手架骨架 458 行（`crates/xtask/src/scaffold.rs` 常量）；本模块实测 1978 行（剔注释 / 测试后 1241 行） | 500 行预算结构性不可达（AC-S-187 判不成立） | 见上方归因表；骨架瘦身是可验证的后续需求 |
| 6 | 前端 i18n **无模块级命名空间** | `packages/i18n` 的 domain 是闭集（键格式测试：`error/common/schema/nav/oauth/app`） | 模块页文案只能**硬编码中文**（与脚手架 `DemoPage.svelte` 同款）或改平台 i18n 契约 | 建议允许 `<module>.*` domain，或提供模块自带 locale 的机制 |
| 7 | 前端**跨模块引用需要改宿主 tsconfig** | 方案 B 下 `apps/shell/tsconfig.json` 必须把 `modules/leave/frontend/**` 加进 `include` | 「业务目录零宿主配置」不成立（但只多 2 行配置，优于把业务页塞进 shell） | 建议 tsconfig 模板里预留 `../../modules/*/frontend/**` |
| **8** | **模块建表迁移没有任何部署路径会应用**（独立复核 F-01） | 干净库实测：`base migrate up` 只跑根 `migrations/` → `to_regclass('public.leave_request')` = `false`、`public.sandbox_tenant_item` = `false`；随后 `cargo xtask tenant-policies` **exit=1**，报 `relation "public.leave_request" does not exist`（`crates/platform-db/src/rls.rs:139` 的 `ALTER TABLE` **不带 `IF EXISTS`**，`apply_policies` 用 `?` 冒泡）。**`develop` 上同样失败**（报 `sandbox_tenant_item`，exit=1）→ **既存缺口，非本分支引入** | 全新环境必须**手工 psql** 应用 `modules/*/migrations/*.up.sql`，否则部署在 `tenant-policies` 步就停；且 `tenant-policies --check` 在 `base_test` 上的绿灯**依赖测试夹具已建表**（假绿风险） | **TD-089**：`migrate up` 应遍历 `modules/*/migrations/`；在那之前写入部署前提 |
| **9** | **`NotificationSender::send` 返回 `impl Future` → trait 不可 dyn**（独立复核 F-03） | `crates/platform-core/src/extension/notification.rs:81-91`（E0038）；同目录 `AuditSink` 却可以 `Arc<dyn>`。每个消费方必须自写装箱 wrapper：`modules/leave/backend/src/service.rs:112-143`（约 18 行） | 这 18 行/模块会持续摊薄「加一个模块的样板量」；与 `AuditSink` 的设计不一致 | **TD-090**：把 `send` 改成返回 `BoxFuture`（平台已有 `BoxFuture` 类型，`leave` 已在用） |
| **10** | **列表没有 cursor 分页，响应信封与平台其余列表接口不一致**（独立复核 F-05） | `modules/leave/backend/src/http.rs:218-232` 返回**裸 JSON 数组** + `limit`；`backend/src/repository.rs:42-52` 注释自认「cursor 的编码形态平台侧还没有统一编解码器」；AC-S-189 的测试直接 `listed.as_array()` | 平台缺 cursor 编解码器这条缺口只写在代码注释里，未登记；消费方各自发明列表形状 | **TD-091**：平台提供 `List` 信封 + cursor 编解码器 |

## 未达成项与口径说明（**不粉饰**）

| AC | 结论 | 说明 |
|---|---|---|
| **AC-S-187**（< 500 行） | **不成立** | 1978 行，超 1478 行；剔除注释 / 测试后仍 1241 行。归因见上。**未修改口径** |
| **AC-S-190**（升级零改动） | **成立** | 实测 `git diff --name-only fd0d099 -- modules/leave/` 输出 **0 行**。升级提交 `52b9314` 只动平台侧 5 个文件（`platform-observability` 弃用头 + `platform-rbac` v2 端点 / `sources`），消费方一行未改 |
| **AC-S-191**（可复算） | 成立（附一处瑕疵） | 每个数字都有命令且已实跑；但 AC 给的 skipped `jq` 表达式在 jq 1.8.2 下是语法错误，已给等价命令并实测 |
| **AC-S-186**（A 类 = 0） | **成立** | 实际 30 个文件（提交后 `git diff --name-only 9bcfbcb 52b9314` 复算，逐项相等）全部归入 B/C/D/生成物，**A 类 0**；`design-summary.md` §3 已补「本表是计划集、权威口径在本文」的说明 |
| **AC-S-192**（E2E） | 成立 | `e2e/leave.spec.ts`：1 passed / **0 skipped** / 0 failed（4 步全过） |
| **AC-S-193**（依赖面） | 成立 | 只有 3 个 `platform-*`，`platform-contracts` 在 dev-dependencies；`check-deps` 退出码 0 |

### 顺带修复（阻塞 E2E，如实记录）

`scripts/seed-e2e.sh` 的 REQ-018 存储夹具有两处**既存**缺陷，会让
`webServer` 起不来 / 第二次运行才红（与 REQ-020 无关，但不修就测不了）：

1. `platform_storage_job.id` 生成的 UUID 最后一段只有 **10 位**十六进制 →
   PostgreSQL 报 `invalid input syntax for type uuid` → 脚本 exit 1；
2. `platform_storage_upload` 每次插入都用 `gen_random_uuid()` 且无冲突键 →
   **不幂等**，重跑会累积 → 「恰好 10 行」断言在第二次运行必红。

修法（最小改动）：补足 12 位；并在夹具块开头按 `e2e/` 对象键与 `probe_row`
实体清理上一轮的行。修后连跑两次均 exit 0、断言 `10 行`。

## 附：本批次的验证输出

```console
$ pnpm --filter @base/shell check
svelte-check found 0 errors and 0 warnings

$ pnpm --filter @base/shell build
✓ 162 modules transformed.
dist/assets/index-D8qXg2H7.js   181.23 kB │ gzip: 59.32 kB
✓ built in 1.75s

$ npx playwright test e2e/leave.spec.ts --reporter=json
# stats: expected=1, skipped=0, unexpected=0, flaky=0
# 用例：请假：列表 → 建单 → 审批通过 → 刷新保持（AC-S-192）→ passed（3.8s）
```

E2E 的 4 步（文件内的 `test.step` 名称，全部 passed）：

1. 打开请假页，列表渲染出已有单据（≥ 1 行，含 API 造的前置单）；
2. 新建表单提交后，列表出现新行（状态「待审批」）；
3. 换审批人账号，点「通过」后状态变为「已通过」；
4. 刷新页面后状态保持（且终态行没有审批按钮）。

---

## 独立复核（实现阶段）的 15 条处置

复核报告：[`../../feature-meta/qa-req020-impl-review.md`](../../feature-meta/qa-req020-impl-review.md)
（独立 Verifier，未复用 BA 结论，未跑 `cargo`，未改代码）
分布：**BLOCKER 1 / MAJOR 2 / MINOR 8 / NIT 4 = 15**。

| # | 复核结论 | 我的处置 |
|---|---|---|
| **F-01** BLOCKER<br>模块建表迁移无部署路径 | **成立，但严重度降级为 MAJOR** | **我自己在干净库上复现了**：`CREATE DATABASE base_test_fresh` → `base migrate up`（exit 0）→ `to_regclass('public.leave_request')` = **false**、`to_regclass('public.sandbox_tenant_item')` = **false** → `cargo xtask tenant-policies` **exit=1**，报 `relation "public.leave_request" does not exist`。<br>**关键修正**：我用 `develop`（`1be47f5`）的 xtask 在**同一个干净库**上跑同一条命令 → **同样 exit=1**（报 `sandbox_tenant_item`）。**所以这是既存缺口，不是本分支引入的回归**，合并不会让任何现在能跑的环境变坏。原判 BLOCKER 的前提（「本分支引入回归」）不成立 → 降为 MAJOR。<br>**但它确实是本需求最该产出的缺口**，已登记 **TD-089** + 缺口清单 **#8**。另记一条：`tenant-policies --check` 在 `base_test` 上的绿灯**依赖测试夹具已建表**（假绿风险），已在终版实测一节标注。 |
| **F-02** MAJOR<br>骨架 458 行归因失准 | **成立，已修正** | 我自己重算（带重数逐行精确匹配）：模板非空 **396** 行，真实文件非空 **1613** 行，逐行重合 **107（27%）**；`REPOSITORY_RS` 只重合 29/172（骨架范例是内存存储，真实实现走 `TenantQuery`）。归因表与「关键事实 2」已改写，并附复算命令。**AC-S-187「不成立」的结论不变**（1978 ≫ 500），变的只是归因口径。 |
| **F-03** MAJOR<br>`NotificationSender` 非 dyn 兼容 | **成立** | 已登记 **TD-090** + 缺口清单 **#9**。这是「每模块多 18 行样板」的直接来源，此前只记了「没有落库适配器」。 |
| **F-04** MINOR<br>写库与审计非原子 | **成立** | 已知限制。审计 sink 故障时会出现「已生效但返回 500」，建单无幂等键。登记 **TD-092**。AC-S-189 的计数表不覆盖此路径，已在测试注释里写明。 |
| **F-05** MINOR<br>列表无 cursor 分页 | **成立** | 已登记 **TD-091** + 缺口清单 **#10**。平台缺 cursor 编解码器这条缺口此前只写在代码注释里。 |
| **F-06** MINOR<br>注释与适配器行为矛盾 | **成立，已改代码** | 我先确认「全仓无任何代码消费 `payload["audience"]`」，且 `crates/platform-notify/src/db_sender.rs:30` 的文档明说「不会凭 `audience` 猜收件人」→ 注释确实是错的。已改为如实说明「适配器当前不解析（TD-088），这里只保留意图，会落为合法失败」。**改后重跑 `-p leave`：25 passed / 0 failed / 0 ignored**（见下方口径说明）。 |
| **F-07** MINOR<br>生产中间件未被直接测试 | **成立** | 已知结构性风险，根因是 TD-076（`main.rs` 的 `authenticate_module` 是 bin 私有）。本轮不修（需把 `build_router`/中间件抽到 lib，属平台重构）。复核已逐行比对：当前测试复刻块与生产块**完全一致**。 |
| **F-08** MINOR<br>同一指标两处不一致（110 vs 127） | **成立** | 权威数取 **127**（终版全量实测，`--all-targets --include-ignored`）。`52b9314` 提交信息里的「110」是加 `http_contract` 新用例之前的旧口径；**提交信息不可改写**，以本报告为准。此类不一致的教训：提交信息里的数字要么贴命令原文，要么不写。 |
| **F-09** MINOR<br>同类改动被分到 C 与 D | **成立，已统一** | 新增「分类规则」小节：**测试文件（判据载体）一律归 C**。`i18n_titles.rs` 由 D 改 C。分布更新为 **C=17 / B=5 / D=6 / 生成物=2**。**A 类仍为 0**。 |
| **F-10** MINOR<br>「每个新模块改 3 个文件」是子集 | **成立，已修正** | 改为「装配点 3 个；但新模块的平台改动面 = 装配 3 + 权限种子 2 + 生成物 2 + 自定义 UI 时 3 + 判据载体约 5，实测 C 类共 **17** 项」。这条很重要——取子集当总数会低估后续排期。 |
| **F-11** MINOR<br>审计真库证据不可事后追溯 | **成立（平台测试卫生，非本需求缺陷）** | `crates/platform-audit/tests/common/mod.rs:169-182` 的 `reset_audit()` 无租户条件全表 `DELETE`，会把别的 crate 留下的 `leave.request` 审计行一起抹掉。**复核也据此排除「装配没写审计」的误判**。登记 **TD-093**。 |
| **F-12** NIT<br>阳性对照数值过期 | **成立，已修** | `acceptance.md` AC-S-188 ④ 的 `iam` 对照值 **159 → 163**（实测）。 |
| **F-13** NIT<br>D353 登记口径残留 | **成立，已修** | `decisions.md` D353 第 2 条由 `git diff BASE -- crates/` 改为「全仓 diff 减 `modules/leave/`」，并注明旧口径会漏判 `apps/`、`packages/`、`migrations/`、`scripts/`、根 `Cargo.toml`。 |
| **F-14** NIT<br>允许自批 | **成立，但不在本需求射程** | 规格未要求职责分离（四眼原则）。已记为观察项 **TD-094**，不修改实现——加这条会改变 AC-S-189 的计数表预期。 |
| **F-15** NIT<br>AC-S-190 区分力弱 | **成立，如实标注** | 消费方前端经 `@base/auth` 读**默认路径** `/api/rbac/me/permissions`，代码里从未出现 `v2`（`grep -rn "v2" modules/leave/ apps/shell/src/ packages/auth/src/` 无命中），v1 又一行未改 → 「零改动」近乎结构性必然。这是 D349/D352 的口径选择，不是实现问题。**结论仍成立，但不宣称「消费方能扛住破坏性变更」**——本需求只验了「加弃用头 + 加 v2 端点」这一种变更。 |

### 本轮的自我纠错（不粉饰）

1. **我原以为 `tenant-policies` 的绿灯是有效的** —— 实测在干净库上是 exit=1。
   我的 `--check` 一直跑在 `base_test` 上，而那张表是测试夹具建出来的。
   **教训：守卫的绿灯要问「它在什么前置条件下才绿」**，不能只看退出码。
2. **我把模板体积当成了模块内样板体积**（F-02）。
   **教训：写「某东西占了多少行」之前，要拿实际文件与它逐行比对，不是拿常量长度。**
3. **提交信息里写了与实测不一致的数字**（F-08）。
   **教训：提交信息里的数字必须是可复算命令的原文粘贴。**

### 口径说明：全量跑了**两次**，第二次覆盖最终提交

1. **第一次**（`52b9314`，代码冻结）→ 1673 / 0 / 0。
2. 之后为处置复核 **F-06** 改了 `modules/leave/backend/src/service.rs` 的**一处注释**
   （无行为变化），提交为 `a8c4b1e`。
3. **第二次**：合并进 `develop`（`bd2d81a`）后**在合并结果上重跑全量 19 crate** →
   **1673 / 0 / 0 / 0**，逐 crate 数字与第一次**逐个相同**；
   同一次也复跑了 4 项守卫（全 exit 0）、shell 单测（422 passed）、
   `svelte-check`（0 errors 0 warnings）、E2E（`expected=1, skipped=0`）。

→ **本报告的全部数字都以合并后的 `develop` @ `bd2d81a` 为准**，不存在
「全量没覆盖最终提交」的缺口。
