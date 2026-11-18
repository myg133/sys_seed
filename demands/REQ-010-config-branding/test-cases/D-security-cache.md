# D. 缓存失效、写接口鉴权、C-5、三表隔离与读授权 —— 测试用例（AC-S-052、AC-S-053、AC-S-060 ~ AC-S-063）

- **需求**: REQ-010「配置、品牌与 Feature Flag」（**rev 2**）
- **来源**: [acceptance.md](../acceptance.md) §AC-S-052 / 053 / 060 ~ 063、[design-summary.md](../design-summary.md) §2 / §2.1.1 / §6 / §6.0 / §7.1、[extension-points v2 §4](../../../contracts/extension-points/v2.md)
- **用例数**: 20（TC-010-032 ~ TC-010-046、TC-010-047 ~ TC-010-051）
- **公共夹具**: 见 [index.md](index.md) §3 测试夹具

## D.0 本域约定（rev 2）

- **epoch 成对比较**（D55）：缓存项存 `(platform_epoch, tenant_epoch)`；**逐一相等**才算新鲜。**不得**取 `max`、**不得**只比一个。
  `platform_config_epoch(scope_key PK, epoch BIGINT, updated_at)`，`scope_key` ∈ `'platform'` / `'tenant:<id>'`。
- **两条写路由**（D59）：`PUT /api/config/entries`（租户/空间，`config:entry:write`）与 **`PUT /api/config/platform-entries`**（平台级，`config:platform:write`）。
- **4 个权限点**（AC-S-060 / 061）：`config:entry:write` / `config:flag:write` / `config:platform:write` / **`config:platform:read`**。
  必须 **manifest + 权限点种子迁移 + 重生成 `capabilities/permissions.json`** 三件套（D67），否则启动对账 fail-fast。
- **平台级写通道**（D56）：迁移里 `REVOKE INSERT, UPDATE, DELETE ON platform_config FROM base_platform_app`（保留 `SELECT`）；
  唯一写通道是 `platform_config_upsert(TEXT, JSONB)` 的 `SECURITY DEFINER` 函数（`GRANT EXECUTE` 给 `base_platform_app`）。
- **读授权**（AC-S-063）：`GET /api/config/entries` 需要令牌；**平台级键另外需要 `config:platform:read`**；租户级/空间级键不受限。
  「平台级键」= **键的登记 `scope` 是 `platform`**，不是「这次命中了平台层」（`logo` 是 `space` 作用域，即使回落平台值也不需要该权限点）。
  ⚠️ 读侧的「按 key 授权」与写侧同类的单路由/静态层矛盾未在 design 解决 —— 见 [index.md](index.md) §8.9 N6；本域用例按「键级判定由实现承担」写行为断言，不预设实现形态。
- **写者**：M=`u-2001`（`platform-user`）；A=`u-2002`（entry+flag）；P=`u-2003`（entry+flag+platform:write+platform:read）；O=`u-2004`（**只有** platform:write）；R=`u-2006`（**只有** platform:read）。
- **错误码**：401 `UNAUTHENTICATED` / 403 `INSUFFICIENT_PERMISSION` / 404 `NOT_FOUND`（跨租头）或 `CONFIG_KEY_NOT_REGISTERED`（读未登记键）；400 见 [A 域 §A.0](A-config-scope.md)。
- **执行位置**：全部**真实库（`base_test`）**；业务连接用 `base_app`，造数/DDL 核对用管理员连接。跨进程用例（TC-010-028）用**两个独立进程**。

---

### TC-010-032: 平台级改配置在**下一个请求**生效（进程内，<1s，TTL=300s）
- **关联 AC**: AC-S-052
- **前置条件**:
  - **执行位置**: 真实库 + 真机 HTTP。
  - 启动前 seed：平台 `cacheTtlSeconds = 300`、平台 `favicon = /favicon-platform.svg`（F）。
  - **用 `favicon` 读平台值**：T1 与 T2 都**没有** `favicon` 覆盖，能命中平台层（AC-S-052 原文写 `logo`，但 AC-S-054 给 T1/T2 都配了租户级 `logo`，HTTP 层读不到平台值 —— 见 §8.9 N4）。读 T2 用 `u-2005`。
  - 写者 P。
- **测试步骤**:
  1. 以 `u-2005`（T2）`GET /api/config/entries?keys=favicon`，记录时刻 `T1`
  2. P：`PUT /api/config/platform-entries` `{"scope":"platform","entries":[{"key":"favicon","value":"/favicon-platform2.svg"}]}`（**不重启、不清缓存、不等待**）
  3. **立刻**再读（同一实例、同一令牌），记录时刻 `T2`
- **预期结果**:
  1. 第 1 步 **200**；`value == "/favicon-platform.svg"`；`source == "platform"`。
  2. 第 2 步 **200**。
  3. 第 3 步 **200**；`value == "/favicon-platform2.svg"` —— **不得**返回旧值。
  4. `T2 - T1 < 1s`（生效上界 = 下一个请求；不是等 300s TTL）。
  5. 进程未重启、未执行任何手工清缓存命令。
- **类型**: 功能 / 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 时序断言（<1s）+ 反例（改完仍旧值即失败）

### TC-010-033: 两个独立实例：实例 1 写 → 实例 2 **下一次读**就是新值
- **关联 AC**: AC-S-052
- **前置条件**:
  - **执行位置**: 真实库 + 两个独立实例（各自缓存、共享 DB）。
  - 实例 1 / 实例 2 此前**各自**读过 T2 的 `favicon`（缓存都是 F）。
- **测试步骤**:
  1. 实例 1 读（确认 F）→ 实例 2 读（确认 F）
  2. 通过 `PUT /api/config/platform-entries` 写平台 `favicon = F2`（该请求由实例 1 所属服务进程处理；`ConfigResolver` trait 只读，写入只能走 HTTP/DB）
  3. **立刻**用**实例 2** 读（同一 `u-2005` 令牌）
  4. 用**实例 1** 读
  5. 管理员连接读 `platform_config_epoch` 的 `'platform'` 行
- **预期结果**:
  1. 两次都是 F / `platform`。
  2. 写操作 **200**。
  3. 实例 2 → **F2**（`< 1s`）—— 实例 2 没有收到任何进程内广播；失效信号只能来自**共享数据库**。
  4. 实例 1 → F2。
  5. `'platform'` 行的 `epoch` 比第 1 步之前 **+1**；两个实例读到的 `epoch` 相同。
  6. 反例：若实例 2 返回 F，说明失效是进程内广播/只清了写入方自己的缓存 —— 用例失败。
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 跨实例一致性 + epoch 旁证 + 缓存归属判别

### TC-010-034: 平台级变更使**所有租户**缓存失效
- **关联 AC**: AC-S-053
- **前置条件**:
  - **执行位置**: 真实库 + 真机 HTTP。
  - 用 **`favicon`**（T1/T2 都无覆盖 → 解析到平台值；这是 AC-S-053 指定的键）。
  - 平台 `favicon = F`。读 T1 用 `u-2001`，读 T2 用 `u-2005`。写者 P。
- **测试步骤**:
  1. T1 读 `favicon`；T2 读 `favicon`（两租户缓存都已填充）
  2. P 改平台 `favicon = F2`（platform-entries）
  3. **立刻**读 T1、T2，再读 T1 + `X-Space-Id: space-sales-1`
- **预期结果**:
  1. 第 1 步两次都是 F / `platform`。
  2. 第 3 步：T1 = **F2**、T2 = **F2**、T1+S1 = **F2**（空间无覆盖，仍回落平台）。
  3. 三次都 `< 1s`。
  4. **不得**出现"只有 T2 更新、T1 仍旧值"，也**不得**出现"只有第一个读的租户更新"。
  5. `'platform'` 的 epoch +1；`'tenant:tenant-a'` / `'tenant:tenant-b'` 的 epoch **不变**（平台写入不该伪装成租户写入）。
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 多租户广播 + 集合断言 + epoch 归属

### TC-010-035: 租户 epoch 高于平台 epoch 时，平台变更仍必须失效（抓 `max(epoch)` 的掩蔽缺陷）
- **关联 AC**: AC-S-053（步骤 3）
- **前置条件**:
  - **执行位置**: 真实库 + 进程内读（便于精确控制顺序）。
  - 初始：`'platform' = E0`、`'tenant:tenant-a' = E0`。
  - 本用例是**设计缺陷判别型**：rev 1 的 `SELECT max(epoch)` 会被更高的租户 epoch 掩蔽（D55 已改为成对比较，本用例守住这个修正）。
- **测试步骤**:
  1. A 对 T1 连续写 5 次租户级 `pageSize`（值 51~55），使 `'tenant:tenant-a'` 的 epoch = `E0 + 5`
  2. 查两行 epoch：确认 `tenant:tenant-a > platform`
  3. T1 读 `favicon`（F）—— 缓存以当时的两个 epoch 为版本
  4. P 改平台 `favicon = F2`（`'platform'` = `E0 + 1`，仍 **小于** `E0 + 5`）
  5. **立刻** T1 读 `favicon`
  6. 再读两行 epoch
- **预期结果**:
  1. 第 2 步：`tenant:tenant-a.epoch == E0 + 5`；`platform.epoch == E0`。
  2. 第 3 步：F / `platform`。
  3. 第 4 步 **200**；第 6 步：`platform.epoch == E0 + 1`，`tenant:tenant-a.epoch == E0 + 5`（**platform 仍更小**）。
  4. 第 5 步：**F2** —— 必须是新值。
  5. 若实现用 `max(platform, tenant)` 做新鲜度比较，第 5 步会返回 **F** → 用例失败。
  6. 结论：失效判据必须是**成对比较**；任一 epoch 不等即重算（D55 / design §7.1）。
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 反例优先（构造掩蔽序关系）+ 设计缺陷判别

### TC-010-036: 写接口鉴权矩阵（M / A / P 五步 + 4 个权限点）
- **关联 AC**: AC-S-060
- **前置条件**:
  - **执行位置**: 真实库 + 真机 HTTP。
  - M=`u-2001`；A=`u-2002`；P=`u-2003`；全部带正确 `X-Tenant-Id: tenant-a`。
- **测试步骤**:
  1. M：`PUT /api/config/entries` `{"scope":"tenant","entries":[{"key":"pageSize","value":30}]}`
  2. M：`PUT /api/feature-flags/newInvoice` `{"enabled":true}`
  3. A：`{"scope":"tenant","entries":[{"key":"pageSize","value":40}]}` → 再读
  4. A：`PUT /api/config/platform-entries` `{"scope":"platform","entries":[{"key":"logo","value":"/branding/x.svg"}]}`
  5. P：同第 4 步的 body（值 `/branding/y.svg`）→ 再读
- **预期结果**:
  1. **403**；`code == "INSUFFICIENT_PERMISSION"`；`params.required == "config:entry:write"`。
  2. **403**；`params.required == "config:flag:write"`。
  3. **200**；读回 `pageSize == 40`、`source == "tenant"`。
  4. **403**；`params.required == "config:platform:write"` —— **另一条路由**（D59），只有 entry 写权限**不够**。
  5. **200**；读回平台 `logo == "/branding/y.svg"`、`source == "platform"`。
  6. 三档中每档都有 200 的对照（M 没有、A 有租户级、P 有平台级）—— 证明接口不是恒拒。
  7. 权限点共 **4 个**（含 `config:platform:read`），全部在 `crates/platform-config/manifest.json` 登记；**且**存在权限点种子迁移（否则启动对账 fail-fast，D67）。
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 权限矩阵（角色 × 路由）+ 正向对照 + 三件套核对

### TC-010-037: 未认证写 → 401（三条路由逐一），且错误体符合 C6
- **关联 AC**: AC-S-060
- **前置条件**:
  - **执行位置**: 真实库 + 真机 HTTP。
- **测试步骤**:
  1. 不带 `Authorization`：`PUT /api/config/entries`（合法 body）
  2. 不带 `Authorization`：`PUT /api/feature-flags/newInvoice`
  3. 不带 `Authorization`：`PUT /api/config/platform-entries`
  4. `Authorization: Bearer not-a-jwt` 打第 1 条路由
  5. 检查第 1 步响应体字段
- **预期结果**:
  1. **401**；`code == "UNAUTHENTICATED"`。
  2. **401**；同码。
  3. **401**；同码（AC-S-060 步骤 6：接口不得裸奔）。
  4. **401**（无效令牌不得降级成"匿名可写"或 500）。
  5. 响应体含 C6 全部字段：`type` / `title` / `status`(401) / `detail` / `instance` / `code` / `trace_id`；`detail` **只含 ASCII**。
  6. 四次请求都**没有**产生任何写入（管理员连接核对相关键未变）。
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 攻击面（未认证写）+ 错误体形状 + 无副作用

### TC-010-038: 跨租户头写：令牌是 T1 + `X-Tenant-Id: tenant-b` → 404 且不落库
- **关联 AC**: AC-S-060
- **前置条件**:
  - **执行位置**: 真实库 + 真机 HTTP。
  - A（`u-2002`）的令牌属于 `tenant-a`。
- **测试步骤**:
  1. A：`PUT /api/config/entries` `{"scope":"tenant","entries":[{"key":"pageSize","value":77}]}` + `X-Tenant-Id: tenant-b`
  2. 查 `platform_tenant_config` 中 `tenant_id='tenant-b' AND key='pageSize'`
  3. 对照：同请求 + `X-Tenant-Id: tenant-a`
- **预期结果**:
  1. **404**；`code == "NOT_FOUND"`（AC-S-060 步骤 7；契约 C3：不泄露资源存在性）；**不是** 403、**不是** 400。
  2. tenant-b 的行数 **0**（越权写没有发生）。
  3. **200**；tenant-a 的行值 = 77。
  4. 第 1 步的 `detail` 不得出现 `tenant-b` 的任何业务信息。
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 跨租越权 + 库侧"未发生"证据 + 正向对照

### TC-010-039: 权限点互不蕴含：只有 `config:platform:write` 的用户写不了租户级
- **关联 AC**: AC-S-060
- **前置条件**:
  - **执行位置**: 真实库 + 真机 HTTP。
  - O=`u-2004`，角色 `cfg-platform-only` **只有** `config:platform:write`。
- **测试步骤**:
  1. O：`PUT /api/config/entries` `{"scope":"tenant","entries":[{"key":"pageSize","value":66}]}`
  2. O：`PUT /api/feature-flags/newInvoice` `{"enabled":true}`
  3. O：`PUT /api/config/platform-entries` `{"scope":"platform","entries":[{"key":"logo","value":"/branding/o.svg"}]}`
- **预期结果**:
  1. **403**；`params.required == "config:entry:write"`（平台写**不**蕴含租户写）。
  2. **403**；`params.required == "config:flag:write"`。
  3. **200**；平台 `logo` 更新为 `/branding/o.svg`。
  4. 4 个权限点是**彼此独立**的开关；若第 1/2 步返回 200，说明实现做了层级/通配 —— 用例失败。
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 权限正交性（反向蕴含）+ 正向对照

### TC-010-040: AC-S-060 与 AC-S-061 必须同时成立（独立路由 + 真实调用点）
- **关联 AC**: AC-S-060、AC-S-061
- **前置条件**:
  - **执行位置**: 真实库 + 真机 HTTP + 静态扫描。
  - `require_permission(...)` 是**静态路由层**（`code/crates/platform-core/src/extension/permission.rs:219-249`），挂上就对整条路由生效 —— D59 用**独立路由**解决。
- **测试步骤**:
  1. A：`PUT /api/config/entries` `{"scope":"tenant","entries":[{"key":"pageSize","value":30}]}` → 读回
  2. A：`PUT /api/config/platform-entries` `{"scope":"platform","entries":[{"key":"logo","value":"/branding/a.svg"}]}`
  3. 静态扫描 `crates/platform-config/src/**`：`require_permission("config:platform:write")` 字面量调用点数
  4. 该调用点挂载的路由用 A / P 各打一次
  5. 同样核对 `config:entry:write`、`config:flag:write`、`config:platform:read` 各有 ≥1 条调用点，且挂载路由行为与 AC 一致
- **预期结果**:
  1. **200**；读回 `pageSize == 30`。
  2. **403**；`params.required == "config:platform:write"`。
  3. `config:platform:write` 的字面量调用点 **≥ 1**，且位于独立路由（`/api/config/platform-entries`）。
  4. 该路由：A **403**、P **200**（不是"挂了个永远 403 的空路由"，也不是"挂在不被调用的路由上"）。
  5. 4 个权限点各自 ≥1 条调用点，且每条对上真实可用的路由。
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 静态扫描 + 行为反证（防"为过检查挂空调用点"）

### TC-010-041: C-5 负向：删掉任一权限点的调用点 → 失败并**点名**
- **关联 AC**: AC-S-061
- **前置条件**:
  - **执行位置**: 单元测试（`platform-contracts` 扫描器 + 临时目录）。**不改仓库源码**。
  - 复制 `crates/platform-config/{manifest.json,src/**}` 到临时目录。
- **测试步骤**:
  1. 对**未改动**的副本跑 C-5（字面量调用点集合 = manifest 声明集合）
  2. 删除 `config:flag:write` 的调用点，再跑
  3. 再删 `config:platform:read`，再跑
- **预期结果**:
  1. 第 1 次：违例数 **0**（4 个权限点都有调用点）。
  2. 第 2 次：违例数 **1**；`check_id == "C-5"`；`message` 同时含 `config:flag:write` 与「no authorization call site」；**不得**把另外 3 个也报成违例。
  3. 第 3 次：违例数 **2**，分别为 `config:flag:write` 与 `config:platform:read`。
  4. 违例是 **violation**，**不得**降级为 warning。
- **类型**: 安全 / 可维护性
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 负向用例（假权限）+ 点名 + 不误报

### TC-010-042: 平台 crate 的 C-5 入口存在，且既有基线「差值 ≥ 0」如实登记
- **关联 AC**: AC-S-061
- **前置条件**:
  - **执行位置**: 单元测试 + 静态统计。
  - **现状（已核实）**：`ContractFixture` 要求 `Repository` + `PlatformModule`（`code/crates/platform-contracts/src/fixture.rs:31-67`），平台 crate 没有；全仓只有 `modules/sandbox/tests/contracts.rs`。故 D58 要求新增**不依赖 fixture** 的扫描入口。
  - **基线**：`platform-org` 9 个已登记权限点、`require_permission` 调用点 **0**；`platform-rbac` 的 `rbac:permission:read` **0** 调用点。
- **测试步骤**:
  1. 确认存在能对 `crates/platform-config` 执行 C-5 的入口（`check_c5_for(manifest_path, src_dir)` 或 `cargo test -p platform-config --test contracts`），并跑通
  2. 断言被检查的 manifest 路径 = `crates/platform-config/manifest.json`，被扫描目录 = `crates/platform-config/src`
  3. 跑 `cargo test -p sandbox --test contracts`，检查报告里**没有**任何 `config:` 权限点
  4. 统计 `platform-org` / `platform-rbac` 的调用点数，与基线比较；把缺口逐条写进 `feature-meta/verification-report.md`
- **预期结果**:
  1. 第 1 步命令**存在且通过**（若只有 `cargo test -p sandbox`，失败）。
  2. 第 2 步路径逐条相符；扫描目录是 `src`（平台 crate 没有 `backend/`）。
  3. 第 3 步报告里 `config:entry:write` / `config:flag:write` / `config:platform:write` / `config:platform:read` **均不出现** —— 证明 sandbox 的契约测试不能替代。
  4. 第 4 步：`platform-org` = **0**（9 个点全无调用点）、`platform-rbac` = **3**（4 个点里 `rbac:permission:read` 缺）；本需求**差值 ≥ 0**（不得让这两个数字下降），且缺口已登记。
  5. **不扩围**：本期没有把 C-5 应用到 `crates/platform-org` / `platform-rbac`（扩了当天就失败，D58）。
- **类型**: 安全 / 可维护性
- **优先级**: P1
- **自动化**: 可自动化
- **设计方法**: 入口存在性 + 覆盖范围反证 + 基线快照（差值 ≥ 0）

### TC-010-043: 平台级表：`base_app` 可 `SELECT`、**直接 DML 三种全拒**
- **关联 AC**: AC-S-062
- **前置条件**:
  - **执行位置**: 真实库（`base_test`）；`base_app`（普通角色）+ 管理员连接。
  - `platform_config` 已有 `logo` 等行（§3.5）。
  - 迁移已执行 **`REVOKE INSERT, UPDATE, DELETE ON platform_config FROM base_platform_app`**（保留 `SELECT`）—— 否则 `deploy/seed/02-app-role.sql:50,54-55` 的默认授权会让本用例失败（D56 / R12）。
- **测试步骤**:
  1. `base_app`：`SELECT count(*) FROM platform_config` → 记录 `n`
  2. `INSERT INTO platform_config (key, value) VALUES ('hijack', '"x"'::jsonb)`
  3. `UPDATE platform_config SET value = '"hijack"'::jsonb WHERE key = 'logo'`
  4. `DELETE FROM platform_config WHERE key = 'logo'`
  5. `SELECT has_table_privilege('base_app','platform_config','SELECT'/'INSERT'/'UPDATE'/'DELETE')` 四项
  6. 管理员连接核对 `logo` 的值、行数、`key='hijack'` 是否存在
- **预期结果**:
  1. 第 1 步成功；`n > 0`（平台级行全体可见是设计）。
  2. 第 2 步失败，SQLSTATE **`42501`**（`permission denied for table platform_config`）；**不是** 0 行受影响、**不是**静默成功。
  3. 第 3、4 步同样 `42501`。
  4. 第 5 步：`SELECT = true`；`INSERT / UPDATE / DELETE = false`（四项逐一断言）。
  5. 第 6 步 `logo` 仍为原值，行数仍为 `n`，`key='hijack'` 行数 **0**。
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 表级权限 + 精确 SQLSTATE + 三种 DML 逐一覆盖

### TC-010-044: 租户表与空间表的 RLS 读隔离（含跨租同名 `space_id`）
- **关联 AC**: AC-S-062
- **前置条件**:
  - **执行位置**: 真实库 + `base_app`。
  - 数据：`platform_tenant_config` 有 T1/T2 行；`platform_space_config` 有 T1+S1、T1+S2、T2+S1（**S1 在 T1 与 T2 是同一字符串**）。
  - 租户上下文 = 会话变量 `app.tenant_id`（`code/crates/platform-db/src/rls.rs:141-150`）。
- **测试步骤**:
  1. `app.tenant_id='tenant-a'` → `SELECT DISTINCT tenant_id FROM platform_tenant_config`
  2. `app.tenant_id='tenant-b'` → 同查询
  3. **不设** `app.tenant_id` → 同查询
  4. `app.tenant_id='tenant-a'` → `SELECT tenant_id, space_id, value FROM platform_space_config WHERE space_id='space-sales-1' AND key='logo'`
  5. `app.tenant_id='tenant-b'` → 同查询
  6. `app.tenant_id='tenant-a'` → `SELECT DISTINCT space_id FROM platform_space_config`
  7. 查两张表的 `pg_class.relrowsecurity` 与 `pg_policies` 的 `qual` / `with_check`
- **预期结果**:
  1. 结果集 = `{tenant-a}`。2. = `{tenant-b}`。
  3. **0 行**（不是报错 —— `current_setting(..., true)` 返回 NULL → 行不可见，fail closed）。
  4. 只有一条：`(tenant-a, space-sales-1, logo)`，值 PS —— **不含** T2 的同名空间行（E）。
  5. 只有一条：`(tenant-b, space-sales-1, logo)`，值 E。
  6. 结果集 = `{space-sales-1, space-legal}`（T1 的两个空间），**不含** T2。
  7. 两张表 `relrowsecurity = true`；各 **≥1** 条 policy；`qual` 与 `with_check` **都**引用 `tenant_id`。
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: RLS 读侧 + 复合维度（tenant, space）+ 未设上下文 fail closed + policy 结构

### TC-010-045: 空间表写侧 RLS（`WITH CHECK`）：写不进他租户的行
- **关联 AC**: AC-S-062
- **前置条件**:
  - **执行位置**: 真实库 + `base_app`。
  - 用例性质：**派生** —— AC-S-062 步骤 2 覆盖平台表写侧、步骤 4/5 覆盖读侧；空间表**写侧**未被任何步骤直接覆盖。
- **测试步骤**:
  1. `app.tenant_id='tenant-a'`：`INSERT INTO platform_space_config (tenant_id, space_id, key, value) VALUES ('tenant-a','space-sales-1','footer','"ok"'::jsonb)`
  2. 同上下文：`INSERT ... VALUES ('tenant-b','space-sales-1','footer','"hijack"'::jsonb)`
  3. 同上下文：`UPDATE platform_space_config SET tenant_id='tenant-b' WHERE tenant_id='tenant-a' AND space_id='space-sales-1' AND key='logo'`
  4. 管理员连接核对
- **预期结果**:
  1. 第 1 步成功（本租户写入）。
  2. 第 2 步失败，SQLSTATE **`42501`**（`WITH CHECK` 拒绝）；**不得**"写进去但读不出来"。
  3. 第 3 步失败 `42501`（不能把本租户的行改挂到别的租户）。
  4. `tenant-b` 的 `footer` 行数 **0**；`tenant-a` 的 `logo` 行仍属 `tenant-a`。
  5. 结论：写侧越权在**数据库层**被拒，不是只靠应用层过滤。
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: RLS 写侧（WITH CHECK）+ 精确 SQLSTATE + 归属核对

### TC-010-046: 三张表 `value` 均 `jsonb`；up/down/up 后类型与值语义不变
- **关联 AC**: AC-S-062（步骤 6）
- **前置条件**:
  - **执行位置**: 真实库 + `sqlx migrate`。
  - 迁移命名假设：`20261118001000_config_schema`（现有最大号 `20261118000901`），含 `.down.sql`。
  - `platform_tenant_config` 迁移前为 `TEXT`（`code/migrations/20261118000200_tenant_isolation.up.sql:22`）。
- **测试步骤**:
  1. 查 `information_schema.columns`：三张表 `value` 的 `data_type`
  2. 向 `platform_tenant_config` 插三条值：`'"s"'::jsonb`、`'60'::jsonb`、`'true'::jsonb`，读回 `jsonb_typeof(value)`
  3. 执行本需求迁移的 **down**
  4. 查 `platform_tenant_config.value` 的 `data_type`；`SELECT value` 三条不报错
  5. 再执行 **up**；查类型并读回三条值的 `jsonb_typeof` 与内容
  6. `sqlx migrate status`：无 dirty；`.down.sql` 存在且可执行；**down 里没有 `DROP TABLE platform_tenant_config`**
- **预期结果**:
  1. 三张表全部 `jsonb`（逐张：`platform_config` / `platform_tenant_config` / `platform_space_config`）。
  2. `jsonb_typeof` 分别为 `string` / `number` / `boolean`。
  3. 第 3 步成功（退出码 0），无手工修库。
  4. 第 4 步 `platform_tenant_config.value` 变回 `text`；三条值可查（文本可以是带引号的 JSON 文本，不算失败）。
  5. 第 5 步回到 `jsonb`；三条值的 `jsonb_typeof` 与**语义**与第 2 步**完全一致** —— 这是"可逆"的可判定判据（空表上的"可逆"是空断言）。
  6. 第 6 步无 dirty；**down 没有 `DROP TABLE`**（若照抄 `20261118000200` 的 down，会把 REQ-006 的历史表删掉）。
- **类型**: 可维护性 / 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 迁移可逆性（先插 3 种类型再 up/down/up）+ 空断言防护

### TC-010-047: AC-S-063 读授权：普通用户读平台级键 403、读租户级键 200、读未登记键 404
- **关联 AC**: AC-S-063
- **前置条件**:
  - **执行位置**: 真实库 + 真机 HTTP。
  - `scimBaseUrl` / `ldapBaseDn` / `conflictStrategy` 登记 `scope: platform`；`pageSize` 登记 `scope: tenant`；`logo` 登记 `scope: space`（但平台层有值）。
  - M=`u-2001`（`platform-user`，**无** `config:platform:read`）。
- **测试步骤**:
  1. M：`GET /api/config/entries?keys=scimBaseUrl`（T1）
  2. M：`GET /api/config/entries?keys=pageSize`（T1）
  3. M：`GET /api/config/entries?keys=notRegistered`
  4. M：`GET /api/config/entries?keys=favicon`（T1；**空间作用域**的键，没有租户覆盖 → 值来自平台层）
- **预期结果**:
  1. **403**；`code == "INSUFFICIENT_PERMISSION"`；`params.required == "config:platform:read"`。**不得**返回 `scimBaseUrl` 的值。
  2. **200**；`value == 20`；`source == "tenant"`（租户级键不受限）。
  3. **404**；`code == "CONFIG_KEY_NOT_REGISTERED"`（注册表先于键级授权：未登记键没有"级别"可判，不能被误报成 403）。
  4. **200**；`value == "/favicon-platform.svg"`（F）；`source == "platform"`。
  5. 第 4 步是最关键的判别：**「平台级键」= 键的登记 `scope` 是 `platform`**，不是「这次命中了平台层」。若实现按命中层判权限，第 4 步会 403 → 用例失败。
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 键级授权 + 判别性反例（登记作用域 vs 命中层级）+ 注册表优先级

### TC-010-048: 读权限点与写权限点彼此独立；只读用户不能写
- **关联 AC**: AC-S-063
- **前置条件**:
  - **执行位置**: 真实库 + 真机 HTTP。
  - P=`u-2003`（含 `config:platform:read`）；O=`u-2004`（只有 `config:platform:write`，**无 read**）；R=`u-2006`（只有 `config:platform:read`）。
- **测试步骤**:
  1. P：读 `scimBaseUrl`
  2. O：读 `scimBaseUrl`
  3. R：读 `scimBaseUrl`
  4. R：`PUT /api/config/entries` `{"scope":"tenant","entries":[{"key":"pageSize","value":12}]}`
  5. R：`PUT /api/config/platform-entries` `{"scope":"platform","entries":[{"key":"logo","value":"/branding/r.svg"}]}`
- **预期结果**:
  1. **200**；`value == "https://scim.example.com"`；`source == "platform"`。
  2. **403**；`params.required == "config:platform:read"`（**写权限不蕴含读权限**）。
  3. **200**（只读权限点足够读平台级键）。
  4. **403**；`params.required == "config:entry:write"`。
  5. **403**；`params.required == "config:platform:write"`（**读权限不蕴含写权限**）。
  6. 结论：4 个权限点两两正交；若 O 的第 2 步返回 200 或 R 的第 4/5 步返回 200，用例失败。
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 权限正交性（读 vs 写双向）+ 正向对照

### TC-010-049: 批量读混合平台级与租户级键：整体 403（fail closed，待裁定）
- **关联 AC**: AC-S-063
- **前置条件**:
  - **执行位置**: 真实库 + 真机 HTTP。
  - M=`u-2001`（无 `config:platform:read`）。
  - 用例性质：**派生** —— AC-S-063 没有规定「同一批里既有平台级键又有租户级键」的行为（§8.9 N7）。本用例按 **fail closed 整体 403** 写；若 BA 裁定"允许部分返回"，本用例改为断言 `entries` 只含租户级键、且 `params.forbiddenKeys` 列出平台级键。
- **测试步骤**:
  1. M：`GET /api/config/entries?keys=pageSize,scimBaseUrl`（T1）
  2. M：`GET /api/config/entries?keys=pageSize,logo`（两个非平台键，对照）
- **预期结果**:
  1. **403**；`code == "INSUFFICIENT_PERMISSION"`；`params.required == "config:platform:read"`；**响应体不含 `entries`**（不得把 `pageSize` 的值先返回出去 —— 那等于用"部分成功"绕过键级授权）。
  2. **200**；`entries` 长度 2（`pageSize`、`logo`），都返回各自的值。
  3. 若第 1 步返回 200 且带部分结果，用例失败。
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 批量接口的授权边界 + fail closed + 对照（全非平台键必须 200）

### TC-010-050: 品牌接口与读授权是**两道独立的门**
- **关联 AC**: AC-S-063
- **前置条件**:
  - **执行位置**: 真实库 + 真机 HTTP。
  - P 有 `config:platform:read`；M 没有。
- **测试步骤**:
  1. 不带令牌：`GET /api/config/branding`（T1）→ 取键集合
  2. M：`GET /api/config/entries?keys=scimBaseUrl,conflictStrategy,cacheTtlSeconds`
  3. P：同第 2 步的 keys
  4. 再检查第 1 步响应里没有 `scimBaseUrl` / `conflictStrategy` / `cacheTtlSeconds`
- **预期结果**:
  1. **200**（公开）；键集合不含任何平台级运维键。
  2. **403**；`params.required == "config:platform:read"`（**登录后仍要被挡**）。
  3. **200**；三个键都有值（`https://scim.example.com` / `manual` / `300`）。
  4. 第 4 步与第 3 步共同证明：匿名被白名单挡、登录后被权限点挡 —— **两道门各自独立**；缺任何一道，`scimBaseUrl` 都会泄露给不该看的人。
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 双门对照（匿名白名单 + 已认证键级授权）

### TC-010-051: `SECURITY DEFINER` 是唯一平台级写通道；HTTP 写路径要 bump epoch
- **关联 AC**: AC-S-062（步骤 3）
- **前置条件**:
  - **执行位置**: 真实库 + `base_app` + 真机 HTTP。
  - 函数 `platform_config_upsert(p_key TEXT, p_value JSONB)` 已建，`SECURITY DEFINER`，`GRANT EXECUTE` 给 `base_platform_app`；直接 DML 已 REVOKE（TC-010-043）。
- **测试步骤**:
  1. `base_app`：`SELECT platform_config_upsert('logo', '"/branding/definer.svg"'::jsonb)`
  2. 管理员连接读 `platform_config.logo`
  3. `base_app` 直接 `INSERT`（复验一次）→ 仍被拒
  4. P：`PUT /api/config/platform-entries` 写 `logo = /branding/http.svg`
  5. 读 `platform_config.logo` 与 `platform_config_epoch` 的 `'platform'` 行
- **预期结果**:
  1. 第 1 步**成功**（函数可执行；这是 AC-S-062 步骤 3 的唯一写通道）。
  2. 第 2 步值 = `/branding/definer.svg`。
  3. 第 3 步仍 **42501**（`SECURITY DEFINER` 不改变直接 DML 的授权）。
  4. 第 4 步 **200**；第 5 步值 = `/branding/http.svg`，且 `'platform'` 的 epoch **+1**。
  5. 第 5 步是**派生断言**（§8.9 N12）：函数体只 upsert 配置、**不自增 epoch**；若 HTTP 写路径忘了在同一事务里 +1，缓存会永不失效。本步把"写进去"和"失效信号"一起验。
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 唯一写通道 + 直接 DML 复验 + 写路径失效信号（transactions）

---
