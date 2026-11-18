# REQ-010「配置、品牌与 Feature Flag」—— QA Pre-merge 报告

- **被验对象**: 分支 `feature/REQ-010`，Dev 提交 `f8b251f` + `dd0b1dd` + `96e4cb6`（共 55 文件 / +7791 −635）
- **基线**: `develop` @ `12366e9`（REQ-009 合并点）
- **判定基线**: `acceptance.md` rev 3（15 条 AC，AC-S-049~063）+ `design-summary.md` + D45~D81
- **环境**: `base_test` / 应用角色 `base_app` / `podman exec base-postgres psql` / `CARGO_TARGET_DIR=code/target`
- **报告日期**: 2026-11-19

## 0. 结论（先看这里）

**VERDICT: FAIL** —— 阻断项只有 **1 条 HIGH**：默认生产日志过滤把 `platform_config` 的 WARN 全吞掉，
导致 **AC-S-059 步骤 1** 与 **AC-S-057 步骤 6** 明确要求的「记录一条结构化 WARN」在真实服务默认配置下**不成立**。
其余 14 条 AC、全部基础设施检查、两轮集成均通过；修复面很小（见 §12.1）。

| 项 | 实测 |
|---|---|
| 集成套件（`scripts/test-integration.sh`，含 ignored） | **539 passed / 0 failed / 0 ignored**，**连跑 3 轮均绿**；基线 461，**+78 条** |
| 默认 Rust 套件（`cargo test --workspace -- --test-threads=2`） | **475 passed / 0 failed / 70 ignored** |
| 前端 `pnpm -r test` | **28 passed / 0 failed**（8 个包） |
| 前端 `bun run test`（根脚本） | Rust 475 + **前端 28**，exit 0 |
| E2E（Playwright，同源） | **2 passed / 0 failed** |
| clippy / fmt / check-deps / check-modules / capabilities | 全部 exit 0，clippy **0 warning** |
| 变异验证 | A~D 四处全部命中；额外 E 处命中；**逐字节还原 + sha256 校验通过** |
| 真实 IAM 令牌端到端 | **已补验**（Dev 声明的未验证项，见 §7） |
| `base migrate down` CLI | **已在 `base_test` 上实跑**（Dev 声明的未验证项，见 §3.4） |

> **前端 16 vs 23 的结论（任务书要求查清）**：**不是测试被删，也不是换了 runner。**
> 实际是 **23 → 28（+5）**：基线 23 = auth 3 + hooks 3 + i18n 3 + schema 3 + sdk 2 + theme 2 + ui 3 + shell nav 4；
> 本期新增 `apps/shell/test/branding.test.ts` **5 条** → 28。`pnpm -r test` 与 `bun run test` 的前端部分**都是 28**。
> Dev 报告写的 **19**（漏了 auth/hooks/schema 共 9 条）与任务书里的 **16** 都是**截断输出导致的少计**；
> `git diff -U0 12366e9..HEAD -- 'apps/**' 'packages/**'` 中删除的 `it(`/`test(` 为 **0 条**。
> **不存在覆盖退化。**

---

## 1. 追溯矩阵（15 条 AC）

| AC | 判定 | 证据 |
|---|---|---|
| AC-S-049 三层覆盖 + `source` | ✅ | 真实 HTTP：T1 `logo`=tenant；T1+S1 `logo`=space；T2 `title`=default；未登记 404；`logo,notRegistered` 整请求 404 且 `params.unknownKeys=["notRegistered"]`；单元 `three_layer_resolution_reports_the_hit_layer`、`source_default_is_distinct_from_an_explicit_platform_value` |
| AC-S-050 作用域越级 | ✅ | `conflictStrategy` tenant/space → 400 `CONFIG_SCOPE_NOT_ALLOWED`；`pageSize` space → 400；`logo` tenant/space → 200；`loginBanner` space → 400（D66 白名单门）；见 §6 |
| AC-S-051 未登记 + 类型 + 顺序 | ✅ | 未登记 400 `CONFIG_KEY_NOT_REGISTERED`；`"20"`/`true` → 400 `CONFIG_TYPE_MISMATCH`（expected/actual）；`20` → 200；同时违「未登记+类型」→ 返回未登记；见 §6 |
| AC-S-052 下一个请求生效 | ✅ | TTL=300 的 `with_ttl` 实例 + 第二实例共享 DB；写平台级后立刻读到新值；跨实例（8080/8081）实测；见 §2 |
| AC-S-053 平台级变更让所有租户失效 | ✅ | 掩蔽顺序实测：tenant epoch 4 > platform 3，改平台 `favicon` 后 T1 立刻读到新值；`max()` 变异必红；见 §2 |
| AC-S-054 品牌公开接口 | ✅ | 无令牌 200；T1=PB、无头=PA、T1+S1=PB（≠PS）、T2=PC；11 个白名单键；`conflictStrategy`/`scimBaseUrl` 不出现；扁平 map；见 §5 |
| AC-S-055 前端无硬编码 | ✅ | `apps/shell/**` 中 `底座壳`/`#1677ff`/`示例平台` 命中 **0**；浏览器实测标题与 `<h1>` 随配置从 `QA平台A`→`QA平台B`，主色 `rgb(0,85,170)`→`rgb(170,85,0)`；E2E 2/2；见 §8 |
| AC-S-056 flag 按租户隔离 | ✅ | 测试 `flags_are_tenant_isolated_and_close_returns_200_semantics` 通过（T1 true/tenant，T2 false/default，匿名 false）；真实 HTTP 未登记名 200+false |
| AC-S-057 灰度确定性/单调/边界 | ✅ | 200 用户：命中 ∈ [70,130]，S50⊊S60，S50'==S50，S10⊂S50，p=0 空/p=100 全；`p=101/-1` PUT 400；两进程集合完全相同；独立公式复算 10/10；见 §4 |
| AC-S-058 无身份 fail closed | ✅ | 单元：p=100 无 user id → false；有 id → true；HTTP 无令牌 → 401；见 §4 |
| AC-S-059 未登记 flag 不 404 / 名字约束 | ✅（行为）/ ⚠️ WARN 见 HIGH | `newinvoce`/`newinvoice` → 200+false+default；`bad\n`/1KB/`new-invoice` → 400 `FLAG_NAME_INVALID`（D77）；非法名不进日志；**但未登记 flag 的 WARN 在默认日志过滤下不落盘**（§12.1） |
| AC-S-060 写接口鉴权 | ✅ | 真实 JWT：无令牌 3 条写路由 401；普通用户 403；entry-only 写平台级 403；全权限 200；T1 令牌 + T2 头 404；通用写路由写 flag → 400 `CONFIG_FLAG_USE_FLAG_ENDPOINT`；见 §7 |
| AC-S-061 C-5 调用点 + 基线 | ✅ | 4 个权限点逐个有 `require_permission` 字面量；删一个 → 扫描失败并点名；基线 org 9/0、rbac 4/3（`rbac:permission:read` 仍缺）、差值 ≥ 0；见 §7 |
| AC-S-062 三表 + REVOKE + jsonb | ✅ | `base_app` SELECT=t、INSERT/UPDATE/DELETE=f；DEFINER 函数可写且 epoch +1；RLS 隔离；`base migrate down/up` 后 `platform_tenant_config` **未删**、5 种 JSON 类型语义不变；伪租户 `platform` → 400 `CONFIG_TENANT_RESERVED`；见 §3 |
| AC-S-063 平台级读授权 | ✅ | M 读 `pageSize` 200；M 读 `scimBaseUrl`（通用路由）403；M 调平台级路由 403；P 调平台级路由 200；混合 `pageSize,scimBaseUrl` 整请求 403 且无 entries；见 §7 |

**15 / 15 行为均有实测证据；唯一未满足的细项是 AC-S-059/057 的 WARN 落盘（HIGH）。**

---

## 2. 缓存失效实测（含掩蔽顺序）

### 2.1 掩蔽顺序（D55 的核心探测器）

真实 `base serve`（`base_app` + 真实 IAM/RBAC）上：

```text
T1 GET favicon                      -> /f2.svg  (source=platform)  [缓存填充，连读两次]
T1 PUT entries pageSize=26          -> 200
platform_config_epoch:              platform=3, tenant:tenant-a=4   （租户 epoch 更高）
PUT platform-entries favicon=/f3.svg-> 200
T1 GET favicon（立刻）              -> /f3.svg  (source=platform)   ✅ 不是旧 /f2.svg
```

若实现用 `SELECT max(epoch)`，第二步后 `max(4,3)=4`，平台写入只把 platform 变 4，`max` 仍为 4 → 会返回 `/f2.svg`。
**实测返回 `/f3.svg` → 成对比较成立。**

### 2.2 跨实例（AC-S-052）

两个独立进程（8080 / 8081，各自缓存、共享 `base_test`）：

```text
实例2 GET favicon  -> /f3.svg   （先填实例2自己的缓存）
实例1 PUT platform favicon=/f4.svg -> 200
实例2 GET favicon（立刻） -> /f4.svg   ✅ 失效信号来自 DB，不是进程内广播
```

测试侧另用 `DbConfigResolver::with_ttl(..., Duration::from_secs(300))` 的实例 r1 复验；写平台级后 r1/r2 都读到新值
（`a_platform_write_is_visible_on_the_next_request` 通过），**不依赖 TTL 到期**。

### 2.3 变异（见 §9）

把 `resolver.rs` 的成对比较改成 `max(platform, tenant)` 后，测试
`a_platform_change_invalidates_every_tenant_and_survives_a_higher_tenant_epoch` 变红：
`left=/favicon-platform2.svg` vs `right=/favicon-platform3.svg`。**探测器有效。**

---

## 3. 平台写通道实测（AC-S-062）

### 3.1 直接 DML（`base_app`，不是超级用户）

```text
current_user = base_app
has_table_privilege: SELECT=t, INSERT=f, UPDATE=f, DELETE=f
INSERT INTO platform_config ... -> ERROR: permission denied for table platform_config
UPDATE platform_config ...       -> ERROR: permission denied for table platform_config
DELETE FROM platform_config ...  -> ERROR: permission denied for table platform_config
```

行数保持 `6`，`hijack` 行 `0`。**REVOKE 不是空话。**

> `deploy/seed/02-app-role.sql` 的 `ALTER DEFAULT PRIVILEGES` 会让新表默认继承写权；Dev 在 seed 脚本里补了
> `to_regclass('public.platform_config')` 条件的**例外 REVOKE**，所以「先跑 seed、后跑迁移」和「重跑 seed」两种顺序
> 都不会把迁移里的 REVOKE 盖回去（测试每次 `setup()` 都重跑该脚本，平台表权限仍为 `t/f/f/f`）。

### 3.2 SECURITY DEFINER 写 + epoch（D71）

```text
SELECT platform_config_upsert('logo','"/branding/qa-definer.svg"'::jsonb)
platform_config_epoch(platform) 1 -> 2
platform_config.logo = "/branding/qa-definer.svg"
```

函数可执行；写一行 epoch **+1**。一次请求写 N 条会 +N（函数每条调用一次），但成对比较是「相等才新鲜」，
任何 +N 都能触发失效；AC-S-052/053 的「下一个请求生效」仍成立（**不构成缺陷**）。

### 3.3 RLS（三表隔离）

```text
SET LOCAL app.tenant_id='tenant-a':
  platform_tenant_config -> 只看到 tenant-a 的 4 行
  platform_space_config  -> 只看到 tenant-a/space-legal 与 tenant-a/space-sales-1
SET LOCAL app.tenant_id='tenant-b':
  platform_space_config  -> 只看到 tenant-b/space-sales-1
```

`platform_config` 无 RLS、全平台可见（设计如此）。跨租 `space_id` 同串（`space-sales-1`）不会串行。

### 3.4 `TEXT -> jsonb` 可逆迁移（up/down/up）

在 `base_test` 上先插入 **5 种** JSON 类型（string/number/boolean/array/object）：

```text
cargo run -p platform-server --bin base -- migrate down --target 20261118000902
  -> 已回滚版本 [20261118001000] → 目标版本 20261118000902

down 后：
  to_regclass('public.platform_tenant_config') = platform_tenant_config   （表还在，未 DROP）
  value 列类型 = text
  5 行全部保留，JSON 文本原样（"qa-string"/42/true/["a","b"]/{"k":1}）

cargo run ... migrate up
  -> 已应用版本 [20261118001000]
up 后：
  value 列类型 = jsonb
  jsonb_typeof 与值完全一致：string/number/boolean/array/object
  has_table_privilege(base_app, platform_config, INSERT)=f, SELECT=t, 函数 EXECUTE=t
```

`migrate status`：`待应用=[]`、`不可逆=[]`。**down 脚本没有 `DROP TABLE platform_tenant_config`。**

### 3.5 伪租户保留值（D72）

以 `X-Tenant-Id: platform` 注册/登录一个真实 IAM 用户后：

```text
GET /api/config/entries?keys=pageSize -> 400 CONFIG_TENANT_RESERVED
GET /api/feature-flags/beta           -> 400 CONFIG_TENANT_RESERVED
```

读写都报错，满足 D72 的「读写都必须报错」分支（平台仍允许以该 id 建 IAM 用户，见 §12.6）。

---

## 4. 灰度实测（AC-S-057 / 058）

### 4.1 公式与边界（真实 HTTP + 独立复算）

```text
p=0    -> enabled=false
p=100  -> enabled=true
p=101  -> 400 CONFIG_VALUE_OUT_OF_RANGE
p=-1   -> 400 CONFIG_VALUE_OUT_OF_RANGE
```

10 个真实令牌用户、`p=50`，两个独立服务进程（8080/8081）判定：

```text
decisions differing across processes: 0 | hits: 5 / 10
用真实 JWT 的 sub（user_id）做 sha256("<flag>:<user_id>") 独立复算：10/10 一致
```

> 注意：灰度哈希输入是 **user_id（令牌 sub）**，不是用户名；用用户名复算会得到不同结果，这是正常的。

### 4.2 200 用户统计（测试层）

`rollout_is_deterministic_monotonic_and_bounded` 通过：

- 同一用户连读 10 次完全一致；
- `p=50` 命中数 ∈ [70,130]；
- `S50 ⊊ S60`；`S50' == S50`；`S10 ⊂ S50`；
- `p=0` 空、`p=100` 全命中；
- 两个独立解析器实例逐个用户一致，且与独立公式一致。

跨进程用例 `rollout_across_two_processes_is_identical` 真起子进程，通过；变异为 `RandomState` 后必红（§9）。

### 4.3 无身份 fail closed（AC-S-058）

单元：`p=100` 且无 user id → `enabled=false`；有 id → `true`。
HTTP：无令牌读 flag → **401**（不是 200）。均实测通过。

---

## 5. 品牌公开接口实测（AC-S-054）

无令牌，真实服务输出（平台值 PA 是 QA 探针写入的 `/branding/qa-definer.svg`，与 PB/PC/PS 两两不同）：

| 请求 | logo | title | 结果 |
|---|---|---|---|
| 无 `X-Tenant-Id` | `/branding/qa-definer.svg`（PA） | `示例平台`（default） | 200 |
| `X-Tenant-Id: tenant-a` | `/branding/tenant-a.svg`（PB） | `T1平台` | 200 |
| `X-Tenant-Id: tenant-a` + `X-Space-Id: space-sales-1` | `/branding/tenant-a.svg`（PB，**≠ PS**） | `T1平台` | 200 |
| `X-Tenant-Id: tenant-b` | `/branding/tenant-b.svg`（PC） | `示例平台` | 200 |

- 响应是**扁平 map**，无 `source`、无包装；
- 键名枚举恰好 **11 个**：`copyright/favicon/footer/loginBackground/logo/primaryColor/theme.density/theme.fontStack/theme.mode/theme.radius/title`；
- `conflictStrategy` / `scimBaseUrl` / `ldapBaseDn` **均不出现**；
- T1+S1 显式断言 `logo == PB` 且 `PB != PS`（`/branding/space-s1.svg`），证明**忽略空间级**；
- 空间级写 `logo=/PS-qa.svg` 后：带 `X-Space-Id` 的 **配置读** 返回 `source=space`、`/PS-qa.svg`，
  但品牌接口仍返回 PB —— 两个接口语义分离正确。

---

## 6. 作用域与类型实测（AC-S-050 / 051）

### 6.1 作用域

```text
PUT entries {"scope":"tenant","key":"conflictStrategy"} -> 400 CONFIG_SCOPE_NOT_ALLOWED (allowed=platform, attempted=tenant)
PUT entries {"scope":"space","key":"conflictStrategy"}  -> 400 CONFIG_SCOPE_NOT_ALLOWED
PUT entries {"scope":"space","key":"pageSize"}          -> 400 CONFIG_SCOPE_NOT_ALLOWED (allowed=tenant)
PUT entries {"scope":"tenant","key":"logo"}             -> 200
PUT entries {"scope":"space","key":"logo"}              -> 200
PUT entries {"scope":"space","key":"loginBanner"}       -> 400 CONFIG_SCOPE_NOT_ALLOWED (reason=whitelist)
```

`loginBanner` 登记为 `space` 但**不在**白名单 → 空间级写 400（不是「写进去后被忽略」）；
租户级写 `loginBanner` 仍 200（白名单只拦空间级）。**D66 的两道门成立。**

### 6.2 类型与顺序

```text
PUT pageSize="20"                         -> 400 CONFIG_TYPE_MISMATCH (expected=number, actual=string)
PUT pageSize=true                         -> 400 CONFIG_TYPE_MISMATCH (expected=number, actual=boolean)
PUT pageSize=20                           -> 200
PUT [notRegistered, pageSize="20"]        -> 400 CONFIG_KEY_NOT_REGISTERED   （注册表先于类型）
PUT [notRegistered, Theme.radius]         -> 400 CONFIG_KEY_INVALID          （键名格式最先）
PUT conflictStrategy=123 (tenant)         -> 400 CONFIG_SCOPE_NOT_ALLOWED    （作用域先于类型）
```

同时违反「未登记 + 类型」返回**注册表**错误，顺序与 D64 一致。

### 6.3 D60 回归

- `ConfigScope` 只有 `Platform/Tenant/Space` **三个变体**（`platform-core/src/config.rs:83-87`），**没有 `Default`**；
- 独立探针（临时目录 `/tmp/qa-probe`，不写入 repo）解析一份把 `logo.scope` 改成 `"default"` 的 manifest：
  ```text
  REJECTED: unknown variant `default`, expected one of `platform`, `tenant`, `space`
  ```
- `writes with scope "default"` → `parse_scope("default")` 返回 400 `CONFIG_SCOPE_NOT_ALLOWED`；
- 若给 `ConfigScope` 加 `Default` 变体，`default_is_a_source_but_never_a_scope` 会失败（测试已在）。

**D60 成立，没有开出第 4 个可写层。**

---

## 7. 鉴权实测（AC-S-060 / 061 / 063）

### 7.1 真实 IAM 令牌端到端（补齐 Dev 声明的未验证项）

不是替身：真实 `base serve`（`base_app` + IAM + RBAC），`POST /api/iam/register` → `POST /api/iam/login` 拿真实 JWT，
SQL 造角色/授权（`platform_role` / `platform_role_permission` / `platform_user_role`）。

| 请求 | 结果 |
|---|---|
| 无令牌 `PUT /api/config/entries` | **401** `UNAUTHENTICATED` |
| 无令牌 `PUT /api/config/platform-entries` | **401** |
| 无令牌 `PUT /api/feature-flags/beta` | **401** |
| 无 config 权限点的普通用户 `PUT entries` | **403** `INSUFFICIENT_PERMISSION` |
| 无 config 权限点的普通用户 `PUT feature-flags/beta` | **403** |
| 只有 `config:entry:write` 的用户写平台级（独立路由） | **403** |
| 持有 `config:platform:write` 的用户写平台级 | **200** |
| T1 令牌 + `X-Tenant-Id: tenant-b` | **404** `NOT_FOUND` |
| 只有 `config:entry:write` 的用户经通用路由写 `newInvoice` | **400** `CONFIG_FLAG_USE_FLAG_ENDPOINT` |
| 同上写 `flags.beta.rolloutPercent` | **400** `CONFIG_FLAG_USE_FLAG_ENDPOINT` |
| 普通用户 `GET entries?keys=pageSize` | **200** |
| 普通用户 `GET entries?keys=scimBaseUrl`（通用路由） | **403** |
| 普通用户 `GET platform-entries?keys=scimBaseUrl` | **403** |
| 持 `config:platform:read` 的用户 `GET platform-entries?keys=scimBaseUrl` | **200** |
| 普通用户 `GET entries?keys=pageSize,scimBaseUrl` | **403（整请求）**，无 `entries` |

### 7.2 C-5（AC-S-061）

- `check_c5_for(manifest, src)` 不依赖 `ContractFixture`，可运行；
- 4 个权限点**逐个**有 `require_permission("字面量")` 调用点：`config:entry:write` / `config:flag:write` /
  `config:platform:write` / `config:platform:read`；
- 负向：删掉 `config:flag:write` 调用点 → 恰好 1 条违例并点名；再删 `config:platform:read` → 2 条；
- 基线：`platform-org` 9 个登记 / **0** 调用点；`platform-rbac` 4 个登记 / 3 调用点，`rbac:permission:read` 仍缺；
  **差值 ≥ 0，本期未扩围**。

### 7.3 平台级读（AC-S-063）

见 7.1 表格；混合批量是**整请求 403**（`params.forbiddenKeys=["scimBaseUrl"]`），不返回 `pageSize` 的部分结果。
品牌接口（AC-S-054）与读授权是两道独立的门：品牌接口不泄露平台运维键，登录后通用读也不泄露。

---

## 8. 前端实测（AC-S-055）

- 静态扫描（排除 `node_modules/dist`）：
  ```text
  底座壳        0
  #1677ff       0
  #1677FF       0
  示例平台      0
  ```
  键名出现是正常的，品牌**值**没有硬编码。
- 同源：`playwright.config.ts` 走 `http://127.0.0.1:8080`，`platform-server` 托管 `apps/shell/dist`；
  本仓无 CORS，E2E 没有跨源。
- E2E：`pnpm exec playwright test` → **2 passed**（标题来自接口、渲染色 ≠ `#1677ff`、菜单过滤）。
- 额外用 Playwright 直接断言**精确值**（E2E 本身只断言 ≠ 默认，见 §12.2）：
  ```text
  改配置前：title=QA平台A, h1=QA平台A, color=rgb(0, 85, 170)
  改配置后：title=QA平台B, h1=QA平台B, color=rgb(170, 85, 0)
  ```
  证明「改 `title`/`primaryColor` → 刷新 → 标题、<h1>、主色都变，且主色不是 `#1677ff`」。

---

## 9. 假绿护栏结果（H 段）

1. **`cargo test` 只有一个 `--`**：`scripts/test-integration.sh` 的命令是
   `cargo test --workspace --tests -- --test-threads=2 --include-ignored`；实际 539 > 0。
2. **变异验证（逐字节还原 + sha256）**：

| # | 变异 | 目标用例 | 结果 | 还原 |
|---|---|---|---|---|
| A | `resolver.rs` 成对比较 → `max(platform, tenant)` | `a_platform_change_invalidates_every_tenant_and_survives_a_higher_tenant_epoch` | **FAILED**（left `/favicon-platform2.svg` vs right `/favicon-platform3.svg`） | sha256 `71b2390a…` 一致，`git diff` 空 |
| B | 迁移里删掉 `REVOKE INSERT,UPDATE,DELETE ON platform_config` | `base_app_can_read_but_cannot_write_the_platform_table_directly` | **FAILED**（`直接 DML 必须被拒`，INSERT 成功） | sha256 `773a6b2e…` 一致 |
| C | `flag.rs` 的 bucket 改成 `RandomState` | `rollout_across_two_processes_is_identical` | **FAILED**（两进程命中集合明显不同） | sha256 `c7a553c5…` 一致 |
| D | 品牌白名单加入 `conflictStrategy` | `branding_over_http_is_whitelisted_and_ignores_the_space_header` | **FAILED**（`!object.contains_key(forbidden)`） | sha256 `f6aa8aac…` 一致 |
| E（额外） | 删掉服务层的「越级写」校验 | `scope_escalation_is_rejected_and_leaves_no_rows` | **FAILED**（第一步即 panic：`租户级越级写应被拒: 1`） | sha256 `78d330d7…` 一致 |

  四处必验变异全部命中，E 用于回答 BA 的第 4 个问题（见 §12.3）。
3. **连跑 ≥2 轮（实跑 3 轮）**：第 1 轮 539/539、第 2 轮 539/539、第 3 轮 539/539，均 `EXIT=0`，无偶发绿。

---

## 10. 契约与基建（I 段）

| 项 | 结果 |
|---|---|
| `feature-meta/manifest.json` pin | `api-conventions: v2` + `extension-points: v2` ✅ |
| `scripts/sync-contracts.sh`（check 模式） | **exit 0**；`docs/contracts/{api-conventions,extension-points}/v2.md` 快照一致 ✅ |
| `cargo xtask capabilities --check` | 通过：13 个文件与源码一致 ✅ |
| 4 个 config 权限点 | `capabilities/permissions.json` 有 4 条；`platform_permission` 共 22 行（18 既有 + 4 新增）✅ |
| 启动对账 | 真实服务在 `base_app` 下启动成功（未 fail-fast）→ manifest ↔ DB 一致 ✅ |
| `extension_point_count` | **5**；`ConfigResolver` 的 `kind="platform-capability"`，不是扩展点 ✅ |
| `AGENTS.md` | 新增 `platform-config`、4 个权限点、`ConfigResolver` 非扩展点、读配置规则 ✅ |
| `cargo xtask check-deps` | 通过（平台 crate 未反向依赖业务模块）✅ |
| `cargo fmt --all --check` / `cargo xtask check-modules` | 通过 ✅ |
| `cargo clippy --workspace --all-targets -- -D warnings` | **exit 0，0 warning** ✅ |

---

## 11. 依赖预算（J 段）

```text
Cargo.lock 包条目：基线 294 -> 当前 295
第三方 registry：  基线 284 -> 当前 284（不变）
新增的 1 条：      name = "platform-config"（本期新建的 workspace crate，不是第三方）
```

`crates/platform-config/Cargo.toml` 未引入 figment / config-rs / launchdarkly；确定性哈希用已在树里的 `sha2`。
**「不新增第三方依赖」成立**；若把「包数不得增长」按字面理解，+1 是新增 workspace crate 的必然结果（INFO）。

---

## 12. 问题（按严重度）

### 12.1 HIGH — 默认生产日志过滤吞掉 `platform_config` 的全部 WARN

**触发条件**：用默认配置启动真实服务（`BASE_LOG_LEVEL=info`，未设 `RUST_LOG`），调用
`GET /api/feature-flags/newinvoice`（合法但未登记），或读到脏的 `rolloutPercent`。

**观察结果**：
```text
GET /api/feature-flags/newinvoice -> 200 {"flag":"newinvoice","enabled":false,"source":"default"}
默认日志（BASE_LOG_LEVEL=info）中 grep "not registered" -> 0 条；grep WARN -> 0 条
用 RUST_LOG='platform_config=warn,base=info' 重启后再调 -> WARN platform_config::resolver: the feature flag is not registered ... flag=newinvoice
```

**根因/证据**：
- `crates/platform-server/src/main.rs:221`：`LOG_TARGETS` 只有 `["base", "platform_server"]`；
- `crates/platform-server/src/main.rs:227-235`：`BASE_LOG_LEVEL` 只对这些 target 生成 directive；
- `crates/platform-config/src/resolver.rs:280/305/366/374`：未登记 flag、非布尔总开关、非整数/越界 `rolloutPercent` 的 WARN 都在 `platform_config::*` target；
- 测试 `crates/platform-config/tests/config_real_db.rs:816-822` 用 `EnvFilter::new("platform_config=warn")` **显式放行**，
  从未经过生产的 `log_filter()`；`crates/platform-server/src/main.rs:772-800` 的 4 条过滤器单测只覆盖 `base` / `platform_server`；
- 部署默认：`deploy/compose.prod.yaml:192` `BASE_LOG_LEVEL: ${BASE_LOG_LEVEL:-info}`；`deploy/helm/base-platform/values.yaml:61` `BASE_LOG_LEVEL: info`；未见 `RUST_LOG`；
- `deploy/RUNBOOK.md:576-590` 自己写了「过滤器必须覆盖事件所在的 target，否则日志被静默丢弃」，但列表只有两个 target。

**影响**：
- **AC-S-059 步骤 1** 要求「同时记录一条结构化 WARN」——默认部署下不记录；
- **AC-S-057 步骤 6** 要求「读路径脏数据 → enabled=false + WARN」——同样不记录；
- 更广：`platform_config` 的 `tracing::error!`（如 DB 故障）也被吞，500 无服务端细节。
  这不是安全边界（行为仍 fail closed），但属于「静默」类问题，正是本需求要避免的。

**修复建议**（最小）：把 `platform_config`（以及其它模块 crate）加入 `LOG_TARGETS`，或把过滤器改成
「全局 `{level}` + 逐 target 覆盖」；并把 `unknown_flags_warn_once` 改成用 `log_filter()` 捕获，而不是自定义 filter。
修完后用真实默认配置重跑 `grep "not registered"` 应为 1。

### 12.2 MEDIUM — E2E 的主色断言过弱，无法证明 `primaryColor` 真的生效

- `e2e/shell.spec.ts:22-26` 只断言 `color !== 'rgb(22, 119, 255)'`。
- 如果 `primaryColor` 完全没有应用（h1 用默认黑色），或 `app.css` 不再使用 `--base-primary`，
  计算色仍是黑色/任意色，`!== #1677ff` 依然成立 → **E2E 会假绿**。
- 本次 QA 另外用 Playwright 直接断言了精确值（`rgb(0,85,170)` / `rgb(170,85,0)`），当前实现是对的；
  但**仓库里的测试**不能守住这个回归。
- 建议：断言计算色等于由接口 `primaryColor` 换算的 `rgb(...)`（或至少等于 `#0055aa` 的 rgb），
  而不是只否定一个默认值。

### 12.3 LOW — TC-010-005 步骤 8 的行数断言对夹具合成行不敏感

- 测试 `crates/platform-config/tests/config_real_db.rs:343-356`：
  ```sql
  tenant_rows = count(*) FROM platform_tenant_config WHERE key='conflictStrategy'                -- 断言 0
  space_rows  = count(*) FROM platform_space_config  WHERE key='conflictStrategy' AND tenant_id <> 'tenant-a' -- 断言 0
  ```
- 夹具里本来就有 `(tenant-a, space-sales-1, conflictStrategy='remote_wins')`；断言排除了 `tenant-a`，
  且用例尝试写的值也是 `remote_wins`。因此**如果越级写被错误放行并覆盖这一行，行数断言抓不到**。
- 但主断言 `expect_err` + `CONFIG_SCOPE_NOT_ALLOWED`（:283-291）是严格的；变异 E 删掉作用域校验后，
  用例在第一步就 panic。**「越级写被拒」没有验松**，松的是「步骤 8 的库侧前后对比」。
- 建议：对 `(tenant-a, space-sales-1, conflictStrategy)` 做**写前/写后快照**（count + value + updated_at），
  断言完全不变；或改用一个与夹具不同的值（如 `local_wins`）并断言仍为 `remote_wins`。

### 12.4 LOW — 用例/文档侧漂移（按 D77/D78 应改用例，不是实现缺陷）

| 文件 | 现状 | 应改为 |
|---|---|---|
| `test-cases/B-branding.md:56-62`（TC-010-013 步骤 4） | P 用**通用读路由** `GET /api/config/entries?keys=conflictStrategy,scimBaseUrl,cacheTtlSeconds` 期望 **200** | 按 AC-S-063/D78：通用路由 → **403**；平台级键改走 `GET /api/config/platform-entries` 才 200 |
| `test-cases/C-feature-flags.md:240-247`（TC-010-030 步骤 4） | `new-invoice` → **200 + false** | 按 D77：kebab 含连字符 → **400 `FLAG_NAME_INVALID`**；「不归一化」改用 `newinvoice`（全小写合法）演示 |
| `test-cases/A-config-scope.md:127-136`（TC-010-005 步骤 8） | 断言另外两张表 `key='conflictStrategy'` 行数**均为 0** | AC-S-054 夹具故意有一行 `(tenant-a, space-sales-1, conflictStrategy)`，该断言不可能成立；改为「本次写未新增/未修改（前后快照相同）」 |
| `test-cases/C-feature-flags.md:18` | 仍写「flag 授权边界未定义（N14）」 | D70 已修：通用写路由收到 flag 命名空间 → 400；该注记应删/改 |
| `design-summary.md:329`（§5.1） | 仍写「无任何配置 → source="platform"」 | 与 AC-S-056 / D75 冲突；应为 `default`（实现已按 `default`） |
| `design-summary.md:190`（§2.3） | 仍写「写入控制必须来自表级权限（只授予平台管理角色）」 | 与 §2.1.1 / D56 冲突；应改为「REVOKE + DEFINER」 |

> 这些是 **BA/用例侧**的修正，不计入实现缺陷；实现按 AC（403 / 400 / default）是对的。

### 12.5 INFO — 前端测试计数/报告口径

- 基线（REQ-009 报告 `feature-meta/qa-report.md:154`）：**23/23**。
- 本期实际：**28/28**（23 + `branding.test.ts` 5）。
- Dev `verification-report.md:21` 写 **19**（漏 auth 3 / hooks 3 / schema 3）；任务书里的 **16** 更低。
  两者都是**截断输出导致的少计**，不是测试被删；`git diff` 中删除的 `it(`/`test(` = 0。
- 建议：报告里直接写 `pnpm -r test` 的**汇总数**（28），不要手抄 tail 的片段。

### 12.6 INFO — 其它风险/边界

| # | 事项 | 说明 |
|---|---|---|
| 1 | `Cargo.lock` 294→295 | +1 是新建 workspace crate `platform-config`；第三方 registry 284 不变，未新增第三方依赖 |
| 2 | `TEXT -> jsonb` 迁移 | `USING value::jsonb` 要求历史 TEXT 是合法 JSON；本仓无生产数据，base_test 实测通过。若未来有非 JSON 历史值，迁移会失败 |
| 3 | 伪租户 `platform` 的 IAM 侧 | 以 `X-Tenant-Id: platform` 仍可注册 IAM 用户；配置读写会 400 `CONFIG_TENANT_RESERVED`（满足 D72 的「读写报错」分支），但 IAM 没有拒绝建该 id 的租户。若要求「拒绝建租户」需另补 |
| 4 | 一次平台写 N 条 → epoch +N | 函数逐条调用、每条 +1；对「相等才新鲜」的判据无影响，AC 未要求「一次请求 +1」 |
| 5 | 日志 target 的更大范围 | 不只是 `platform_config`：`platform_iam` / `platform_rbac` / `platform_org` 的日志在默认过滤下也被吞（既有 `LOG_TARGETS` 的设计）。本需求只按 AC 点名 config 的两条 WARN |

---

## 13. 未覆盖与无法验证

| 项 | 原因 | 影响 |
|---|---|---|
| CI 真跑 / 分支保护 / Trivy / 镜像体积门禁 | 本仓无 remote，GitHub Actions 跑不起来 | 与 REQ-009 相同，已知不验收 |
| 真实多副本 K8s 部署的缓存收敛 | 本机用两个独立进程模拟（8080/8081） | 未在真实 Service/LB 下实测；机制上失效信号来自共享 DB |
| 200 用户 × 真实 HTTP | 需要 200 个真实令牌；用单元层 + 子进程完成 | 10 个真实令牌已实测一致；200 用户统计在测试层 |
| 配置管理界面 / 主题令牌体系 | 明确属 REQ-012 / REQ-011 | 不在本期 |
| 覆盖率数字 | 本期不设门槛 | — |
| `X-Tenant-Id` 在既有 `/api/rbac` 路由的跨租 404 | 既有中间件不解析该头；本需求只在 `/api/config` 自带中间件实现 | 超出本需求范围；已在 Dev 报告 §6 登记 |
| `base_dev` | **从未写入**；所有 DB 操作只对 `base_test` | — |
| 生产日志聚合（Loki/ELK） | 只验证了 stdout 的 `tracing` 过滤 | 12.1 的修复应在真实日志管道复验 |

---

## 14. 复现命令索引（关键项）

```bash
# 环境
export CARGO_TARGET_DIR=/home/myg133/Projects/system_seed/code/target
export DATABASE_URL_TEST='postgres://base:base@127.0.0.1:5432/base_test'

# 集成（两轮）
scripts/test-integration.sh          # -> 539 passed / 0 failed / 0 ignored, EXIT=0

# 默认 Rust + 前端
bun run test                         # -> cargo 475 passed / 70 ignored; pnpm -r test 28 passed

# clippy / 契约 / 结构
cargo clippy --workspace --all-targets -- -D warnings   # -> 0 warning, exit 0
scripts/sync-contracts.sh                               # -> 两个 v2 快照一致, exit 0
cargo xtask capabilities --check                        # -> 13 个文件一致
cargo xtask check-deps                                  # -> 通过

# DB 探针（base_app）
podman exec base-postgres psql -U base_app -d base_test -tAc \
  "select has_table_privilege('base_app','platform_config','INSERT')"   # -> f

# 迁移可逆（base_test）
cargo run -q -p platform-server --bin base -- migrate down --target 20261118000902
cargo run -q -p platform-server --bin base -- migrate up

# 真实 IAM + 掩蔽顺序（需先起 serve；详见 §2/§7）
# 默认日志复现：不设 RUST_LOG，调用 /api/feature-flags/newinvoice，grep "not registered" -> 0
# 放行后复现：RUST_LOG='platform_config=warn,base=info' -> 1 条 WARN
```

---

## 15. 一句话给 BA

**除「默认日志过滤吞掉 AC-S-059/057 要求的 WARN」这一条 HIGH 外，REQ-010 的 15 条 AC 行为、四个假通过探测器、
两轮集成、真实 IAM 令牌矩阵、迁移可逆、前端同源 E2E 都已实测通过。** 建议先修 `LOG_TARGETS`（一行）再合并；
若 BA 认为「WARN 只在显式 `RUST_LOG` 下要求」则本条可降为 MEDIUM/INFO，但当前默认部署确实不落盘。
