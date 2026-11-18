# D. 缓存失效、写接口鉴权、C-5 与三表隔离 —— 测试用例（AC-S-052、AC-S-053、AC-S-060、AC-S-061、AC-S-062）

- **需求**: REQ-010「配置、品牌与 Feature Flag」
- **来源**: [acceptance.md](../acceptance.md) §AC-S-052 / AC-S-053 / AC-S-060 / AC-S-061 / AC-S-062、[design-summary.md](../design-summary.md) §2 / §6.2 / §7 / §7.1、[extension-points v2 §4](../../../contracts/extension-points/v2.md)
- **用例数**: 15（TC-010-032 ~ TC-010-046）
- **公共夹具**: 见 [index.md](index.md) §3 测试夹具

## D.0 本域约定

- **两个独立实例**：两个各自持进程内缓存的 `ConfigResolver` 实现，**共享同一个 `base_test`**（模拟两个进程；本机不起多实例）。这与 REQ-009 的 epoch 口径一致（`code/crates/platform-rbac/tests/...`）。真多进程收敛属部署侧，不在本期（acceptance「已知不验收项」已列）。
- **缓存 TTL**：启动**前**把平台级 `cacheTtlSeconds` seed 成 `300`（该键已登记为 `scope: platform`、`type: number`，`code/crates/platform-rbac/manifest.json`）。**不在用例中途改 TTL** —— 改它本身会触发一次平台级失效，污染时序断言。
- **epoch 表**（design §7.1）：`platform_config_epoch(scope_key TEXT PRIMARY KEY, epoch BIGINT NOT NULL, updated_at TIMESTAMPTZ)`；写租户级 → `scope_key='tenant:<id>'` +1；写平台级 → `scope_key='platform'` +1。
- **写者**：A=`u-2002`（`config:entry:write` + `config:flag:write`）；P=`u-2003`（再加 `config:platform:write`）；M=`u-2001`（仅 `platform-user`）；O=`u-2004`（**只有** `config:platform:write`）。
- **权限矩阵**（AC-S-060 + 契约 C6）：未认证 **401 `UNAUTHENTICATED`**；缺权限点 **403 `INSUFFICIENT_PERMISSION`**；跨租头 **404 `NOT_FOUND`**（契约 C3）。
- **错误码**：403 用 `INSUFFICIENT_PERMISSION`（D26）；400 的码见 [A 域 §A.0](A-config-scope.md) 与 §7 A3。
- **三张表与授权现状（已核实）**：`platform_tenant_config` 已存在（`TEXT`，`code/migrations/20261118000200_tenant_isolation.up.sql:19-31`）；`platform_config` / `platform_space_config` 待建；`platform_config_epoch` 待建。**应用角色 `base_app` 当前对新表会自动获得 `SELECT/INSERT/UPDATE/DELETE`**（`code/deploy/seed/02-app-role.sql:50,54-55` 的 `GRANT ... ALL TABLES` + `ALTER DEFAULT PRIVILEGES`）—— 这是 AC-S-062 步骤 2 的**前置修复项**（§8 Q-28）。
- **执行位置**：全部为**真实库（`base_test`）**；应用连接用普通角色 `base_app`（超管会绕过 RLS，`code/crates/platform-server/src/isolation.rs`），造数与 DDL 核对用管理员连接。

---

### TC-010-032: 平台级改配置在**下一个请求**生效（进程内，<1s，TTL=300s）
- **关联 AC**: AC-S-052
- **前置条件**:
  - **执行位置**: 真实库 + 真机 HTTP。
  - 启动前 seed：平台 `cacheTtlSeconds = 300`、平台 `logo = /branding/platform-a.svg`（A）。
  - 用 **T2** 读平台 `logo`（T2 没有租户级覆盖，能命中平台值；**读 T2 必须用 `u-2005` 的令牌**，用 T1 令牌会 404）。**不用 T1** —— T1 的 `logo` 有租户覆盖（B），永远读不到平台值，AC-S-052 用 `logo` + T1 会与 AC-S-049 的夹具互斥（§8 Q-08）。
  - 写者 P。
- **测试步骤**:
  1. 以 `u-2005`（T2）`GET /api/config/entries?keys=logo`，记录时刻 `T1`
  2. P 写 `{"scope":"platform","key":"logo","value":"/branding/platform-a2.svg"}`（**不重启、不清缓存、不等待**）
  3. **立刻**再读（同一实例、同一 `u-2005` 令牌），记录时刻 `T2`
- **预期结果**:
  1. 第 1 步 **200**；`value == "/branding/platform-a.svg"`；`source == "platform"`。
  2. 第 2 步 **200**。
  3. 第 3 步 **200**；`value == "/branding/platform-a2.svg"` —— **不得**返回旧值 A。
  4. `T2 - T1 < 1s`（生效上界 = 下一个请求；不是等 300s TTL）。
  5. 进程未重启（PID 不变）、未执行任何手工清缓存命令。
- **类型**: 功能 / 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 时序断言（<1s）+ 反例（改完仍旧值即失败）

### TC-010-033: 两个独立解析器实例：实例 1 写 → 实例 2 **下一次读**就是新值
- **关联 AC**: AC-S-052
- **前置条件**:
  - **执行位置**: 真实库 + 两个独立实例（各自缓存、共享 DB）。
  - 实例 1 / 实例 2 此前**各自**读过 T2 的 `logo`（用 `u-2005` 的令牌；缓存都是 A）。
- **测试步骤**:
  1. 实例 1 读（确认 A）→ 实例 2 读（确认 A）
  2. 通过 `PUT /api/config/entries` 写平台 `logo = A2`（**该请求由实例 1 所属的服务进程处理**；`ConfigResolver` trait 本身是只读的，写入只能走 HTTP/DB）
  3. **立刻**用**实例 2** 读（同一 `u-2005` 令牌）
  4. 用**实例 1** 读
  5. 用管理员连接读 `platform_config_epoch` 中 `scope_key='platform'` 的行
- **预期结果**:
  1. 两次都是 A / `platform`。
  2. 写操作 **200**。
  3. 实例 2 → **A2**（`< 1s`）—— 实例 2 **没有**收到任何进程内广播；失效信号只能来自**共享数据库**。
  4. 实例 1 → A2。
  5. `platform` 行的 `epoch` 比第 1 步之前 **+1**；两个实例读到的 `epoch` 值相同。
  6. 反例：若实例 2 返回 A，说明失效是进程内广播或只清了写入方自己的缓存 —— 用例失败。
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 跨实例一致性 + epoch 旁证 + 缓存归属判别

### TC-010-034: 平台级变更使**所有租户**缓存失效（不只改动的那个租户）
- **关联 AC**: AC-S-053
- **前置条件**:
  - **执行位置**: 真实库 + 真机 HTTP（或单实例 + 多租户请求）。
  - 用 **`favicon`**（登记为 `scope: space`，但 T1/T2 都**没有**任何层级覆盖 → 解析到平台值）。**不用 `logo`**：T1 有租户覆盖，读不到平台值（§8 Q-08）。
  - 平台 `favicon = /favicon-platform.svg`（F）。写者 P。读 T1 用 `u-2001`，读 T2 用 `u-2005`。
- **测试步骤**:
  1. T1 读 `favicon`；T2 读 `favicon`（两租户缓存都已填充）
  2. P 改平台 `favicon = /favicon-platform2.svg`（F2）
  3. **立刻**读 T1、T2，再读 T1 + `X-Space-Id: space-sales-1`
- **预期结果**:
  1. 第 1 步两次都是 F / `platform`。
  2. 第 3 步：T1 = **F2**、T2 = **F2**、T1+S1 = **F2**（空间无覆盖，仍回落到平台）。
  3. 三次都 `< 1s`。
  4. **不得**出现"只有 T2 更新、T1 仍旧值"，也**不得**出现"只有第一个读的租户更新"。
  5. `platform` 的 epoch +1；`tenant:tenant-a` / `tenant:tenant-b` 的 epoch **不变**（平台写入不该伪装成租户写入）。
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 多租户广播 + 集合断言 + epoch 归属

### TC-010-035: 租户 epoch 高于平台 epoch 时，平台变更仍必须失效（抓 `max(epoch)` 的掩蔽缺陷）
- **关联 AC**: AC-S-053
- **前置条件**:
  - **执行位置**: 真实库 + 单元/进程内读（便于精确控制顺序）。
  - 初始 `platform_config_epoch`：`platform` = `E0`、`tenant:tenant-a` = `E0`。
  - 本用例是**设计缺陷判别型**：design §7.1 的读路径是 `SELECT max(epoch) WHERE scope_key IN ('platform','tenant:<id>')`。两行**各自独立自增** → 当 `tenant:<id>` 的 epoch 高于 `platform` 时，平台级写入只让 `platform` +1，`max` **不变** → 缓存被判为新鲜。见 [index.md](index.md) §8 Q-02。
- **测试步骤**:
  1. A 对 T1 连续写 5 次租户级 `pageSize`（值 51~55），使 `tenant:tenant-a` 的 epoch = `E0 + 5`
  2. 查两行 epoch：确认 `tenant:tenant-a > platform`（**顺序是本用例的关键**）
  3. T1 读 `favicon`（F）—— 缓存以「掩蔽后的 max」为版本
  4. P 改平台 `favicon = F2`（`platform` epoch = `E0 + 1`，仍 **小于** `E0 + 5`）
  5. **立刻** T1 读 `favicon`
  6. 再读两行 epoch
- **预期结果**:
  1. 第 2 步：`tenant:tenant-a.epoch == E0 + 5`；`platform.epoch == E0`。
  2. 第 3 步：F / `platform`。
  3. 第 4 步 **200**；第 6 步：`platform.epoch == E0 + 1`，`tenant:tenant-a.epoch == E0 + 5`（**platform 仍更小**）。
  4. 第 5 步：**F2** —— 必须是新值。
  5. 若实现只用 `max(platform, tenant)` 做新鲜度比较，第 5 步会返回 **F**（旧值）→ 用例失败。这是本条的唯一目的：**让"设计按字面实现会漏失效"变成可复现的失败**。
  6. 结论若不通过：design §7.1 的读路径必须改成「两行分别比较」或「单一全局 epoch 行」或「平台写入时把所有租户行抬到 ≥ platform」（§8 Q-02）。
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 反例优先（构造掩蔽序关系）+ 设计缺陷判别

### TC-010-036: 写接口鉴权矩阵（M / A / P 三档五步，逐步断言错误码）
- **关联 AC**: AC-S-060
- **前置条件**:
  - **执行位置**: 真实库 + 真机 HTTP。
  - 账号：M=`u-2001`（仅 `platform-user`）；A=`u-2002`（`config:entry:write` + `config:flag:write`）；P=`u-2003`（再加 `config:platform:write`）。
  - 全部请求带正确的 `X-Tenant-Id: tenant-a`。
- **测试步骤**:
  1. M：`PUT /api/config/entries` body `{"scope":"tenant","key":"pageSize","value":30}`
  2. M：`PUT /api/feature-flags/beta` body `{"enabled":true}`
  3. A：`{"scope":"tenant","key":"pageSize","value":40}` → 再读
  4. A：`{"scope":"platform","key":"logo","value":"/branding/x.svg"}`
  5. P：`{"scope":"platform","key":"logo","value":"/branding/y.svg"}` → 再读
- **预期结果**:
  1. **403**；`code == "INSUFFICIENT_PERMISSION"`；`params.required == "config:entry:write"`。
  2. **403**；`params.required == "config:flag:write"`。
  3. **200**；读回 `pageSize == 40`、`source == "tenant"`（A 有 entry 写权限 → 成功）。
  4. **403**；`params.required == "config:platform:write"` —— 只有 `config:entry:write` **不够**（**不是** 400：权限点先于作用域/类型判定，§7 A9）。
  5. **200**；读回 `logo` 平台值 = `/branding/y.svg`、`source == "platform"`；库里 `platform_config` 已更新。
  6. 三档中每一档都至少有一个 **200** 的对照（M 没有、A 有租户级、P 有平台级）—— 证明接口不是恒拒。
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 权限矩阵（角色 × 操作）+ 正向对照 + 精确错误码

### TC-010-037: 未认证写 → 401（不是 403/400），且错误体符合 C6
- **关联 AC**: AC-S-060
- **前置条件**:
  - **执行位置**: 真实库 + 真机 HTTP。
  - 用例性质：**派生** —— AC-S-060 只测"普通用户 403"，没测"根本没有身份"（§8 Q-24）。
- **测试步骤**:
  1. 不带 `Authorization`：`PUT /api/config/entries`（合法 body）
  2. 不带 `Authorization`：`PUT /api/feature-flags/beta`（合法 body）
  3. `Authorization: Bearer not-a-jwt`
  4. 检查第 1 步响应体字段
- **预期结果**:
  1. **401**；`code == "UNAUTHENTICATED"`。
  2. **401**；同码。
  3. **401**（无效令牌不得降级成"匿名可写"或 500）。
  4. 响应体含 C6 全部字段：`type` / `title` / `status`(401) / `detail` / `instance` / `code` / `trace_id`；`detail` **只含 ASCII**（C6：不得拼中文）。
  5. 三次请求都**没有**产生任何写入（用管理员连接核对相关键未变）。
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 攻击面（未认证写）+ 错误体形状 + 无副作用

### TC-010-038: 跨租户头写：令牌是 tenant-a + `X-Tenant-Id: tenant-b` → 404 且不落库
- **关联 AC**: AC-S-060
- **前置条件**:
  - **执行位置**: 真实库 + 真机 HTTP。
  - A（`u-2002`）的令牌属于 `tenant-a`。
  - 用例性质：**派生** —— 契约 C3 的"头与令牌不一致 → 404"在 AC-S-060 里没有对应用例（§8 Q-24）。
- **测试步骤**:
  1. A：`PUT /api/config/entries` `{"scope":"tenant","key":"pageSize","value":77}` + `X-Tenant-Id: tenant-b`
  2. 查 `platform_tenant_config` 中 `tenant_id='tenant-b' AND key='pageSize'` 的行
  3. 对照：同请求 + `X-Tenant-Id: tenant-a`
- **预期结果**:
  1. **404**；`code == "NOT_FOUND"`（契约 C3：不泄露资源存在性）；**不是** 403、**不是** 400。
  2. tenant-b 的行数 **0**（越权写没有发生）。
  3. **200**；tenant-a 的行值 = 77。
  4. 第 1 步的 `detail` 不得出现 `tenant-b` 的任何业务信息（只允许通用描述）。
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 跨租越权 + 库侧"未发生"证据 + 正向对照

### TC-010-039: 权限点互不蕴含：只有 `config:platform:write` 的用户写不了租户级
- **关联 AC**: AC-S-060
- **前置条件**:
  - **执行位置**: 真实库 + 真机 HTTP。
  - O=`u-2004`，角色 `cfg-platform-only` **只有** `config:platform:write`。
  - 用例性质：**派生** —— AC-S-060 只验证"entry 不够写平台"，没验"platform 不够写租户"（反向蕴含）。
- **测试步骤**:
  1. O：`PUT /api/config/entries` `{"scope":"tenant","key":"pageSize","value":66}`（T1）
  2. O：`PUT /api/feature-flags/beta` `{"enabled":true}`
  3. O：`{"scope":"platform","key":"logo","value":"/branding/o.svg"}`
- **预期结果**:
  1. **403**；`params.required == "config:entry:write"`（平台写权限**不**隐含租户写）。
  2. **403**；`params.required == "config:flag:write"`。
  3. **200**；平台 `logo` 更新为 `/branding/o.svg`。
  4. 三个权限点是**三个独立的开关**，不存在"高权限顺带包含低权限"的隐式规则；若第 1/2 步返回 200，说明实现把权限点做成了层级/通配 —— 用例失败。
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 权限正交性（反向蕴含）+ 正向对照

### TC-010-040: AC-S-060 与 AC-S-061 必须同时成立（租户写 200 / 平台写 403 / `platform:write` 有真实调用点）
- **关联 AC**: AC-S-060、AC-S-061
- **前置条件**:
  - **执行位置**: 真实库 + 真机 HTTP + 静态扫描。
  - 现状约束：`require_permission(...)` 是**静态路由层**（`code/crates/platform-core/src/extension/permission.rs:219-249`），挂上就对整条路由生效。若把 `config:platform:write` 挂到与租户写**同一条** `PUT /api/config/entries` 上，A 连租户写都会 403（与 AC-S-060 步骤 3 冲突）；若只在 handler 里按 body 判定，`require_permission` 扫描不到该权限点（与 AC-S-061 冲突）。**本用例把"两者必须同时成立"写成可判定断言**（§8 Q-05）。
- **测试步骤**:
  1. A（只有 entry+flag）：`PUT /api/config/entries` `{"scope":"tenant","key":"pageSize","value":30}` → 读回
  2. A：平台级同接口（或实现选定的平台级入口）→ 记录状态码
  3. 静态扫描 `crates/platform-config/src/**`：`require_permission("config:platform:write")` 的字面量调用点数
  4. 找到该调用点挂载的**路由**，用 A 与 P 各打一次：A → 403、P → 200
  5. 同样核对 `config:entry:write` 与 `config:flag:write` 各有 ≥1 条调用点，且挂载路由的行为与 AC-S-060 一致
- **预期结果**:
  1. **200**；读回 `pageSize` 已更新。
  2. **403**；`params.required == "config:platform:write"`。
  3. `config:platform:write` 的字面量调用点 **≥ 1**。
  4. 该调用点对应的路由：A **403**、P **200**（**不是**"挂了一个永远返回 403 的空路由"；也不是"只挂在一个从不被调用的路由上"）。
  5. 三个权限点各自 ≥1 条调用点，且每条都对上真实可用的路由。
  6. 若实现选择"按请求判定权限"，第 3 步仍必须找到 `config:platform:write` 的 `require_permission` 调用点（C-5 只认这个形态）；否则用例失败并指向 §8 Q-05。
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 静态扫描 + 行为反证（防"为过检查而挂的空调用点"）

### TC-010-041: C-5 负向：删掉任一权限点的调用点 → 失败并**点名**该权限点
- **关联 AC**: AC-S-061
- **前置条件**:
  - **执行位置**: 单元测试（`platform-contracts` 的扫描器 + 临时目录）。**不改仓库源码**。
  - 复制 `crates/platform-config/manifest.json` 与 `src/**` 到临时目录（文件名保持 `*.rs`）。
- **测试步骤**:
  1. 对**未改动**的副本跑 C-5 扫描（字面量调用点集合 = manifest 声明集合）
  2. 在副本里删除 `config:flag:write` 的调用点（保留另外两个）
  3. 再跑一次
  4. 再删 `config:platform:write`，再跑
- **预期结果**:
  1. 第 1 次：违例数 **0**（三个权限点都有调用点）。
  2. 第 3 次：违例数 **1**；`check_id == "C-5"`；`message` **同时**含 `config:flag:write` 与「no authorization call site」的含义；**不得**把另外两个也报成违例。
  3. 第 4 次：违例数 **2**，分别为 `config:flag:write` 与 `config:platform:write`。
  4. 违例是 **violation**，**不得**降级为 warning（`ContractWarning` 不算失败 —— C-5 明确要求失败）。
- **类型**: 安全 / 可维护性
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 负向用例（假权限）+ 违例点名 + 不误报

### TC-010-042: 平台 crate 的 C-5 入口必须真的存在，且既有基线不变（现状：`platform-org` 0 个调用点）
- **关联 AC**: AC-S-061
- **前置条件**:
  - **执行位置**: 单元测试 + 静态统计。
  - **现状（已核实）**：`contracts` 套件只覆盖 `modules/<name>` —— `ContractFixture` 要求 `Repository` + `PlatformModule`（`code/crates/platform-contracts/src/fixture.rs:31-67`），平台 crate 没有；全仓只有 `modules/sandbox/tests/contracts.rs`。因此 AC-S-061 的"跑平台的契约测试"**当前不存在**（§8 Q-04）。
  - **现状（已核实）**：`platform-org` 声明 9 个权限点，`require_permission` 调用点 **0**（`code/crates/platform-org/src/http.rs:295-314` 无鉴权层）；`platform-rbac` 4 个权限点里 `rbac:permission:read` **0** 调用点（豁免的注册表路由，`code/crates/platform-rbac/src/http.rs:419-420`）。
- **测试步骤**:
  1. 确认存在一个能对 `crates/platform-config` 执行 C-5 的入口（例如 `cargo test -p platform-config --test contracts`，或 `platform-contracts` 里新增的 `check_c5_for(manifest_path, src_dir)`），并跑通
  2. 在该入口断言"被检查的 manifest 路径"与"被扫描的源码目录"分别是 `crates/platform-config/manifest.json` 与 `crates/platform-config/src`
  3. 跑 `cargo test -p sandbox --test contracts`，检查其报告里**没有**任何 `config:` 权限点
  4. 统计 `platform-org` 与 `platform-rbac` 的 `require_permission("<字面量>")` 调用点数，与仓库基线比较
- **预期结果**:
  1. 第 1 步命令**存在且通过**（若只有 `cargo test -p sandbox`，第 1 步失败）。
  2. 第 2 步路径逐条相符；扫描目录是 `src`（平台 crate 没有 `backend/`）。
  3. 第 3 步报告里 `config:entry:write` / `config:flag:write` / `config:platform:write` **均不出现** —— 证明 sandbox 的契约测试**不能**替代对 platform-config 的检查。
  4. 第 4 步基线快照：`platform-org` = **0**（9 个已登记点全部无调用点）、`platform-rbac` = **3**（4 个点里 `rbac:permission:read` 缺）。本需求**不得**让这两个数字下降；若 C-5 被扩到全部平台 crate，则会因为既有缺口**立即失败** —— 该扩围必须有 BA 裁定（§8 Q-04）。
- **类型**: 安全 / 可维护性
- **优先级**: P1
- **自动化**: 可自动化
- **设计方法**: 入口存在性 + 覆盖范围反证 + 基线快照

### TC-010-043: 平台级表：租户连接可 `SELECT`、**不可** `INSERT/UPDATE/DELETE`
- **关联 AC**: AC-S-062
- **前置条件**:
  - **执行位置**: 真实库（`base_test`）；`base_app`（普通角色，不绕过 RLS）+ 管理员连接做核对。
  - `platform_config` 已有 `logo` 等行（§3.5）。
  - 本需求的配置迁移已应用。**前置修复**：迁移必须显式 `REVOKE INSERT, UPDATE, DELETE ON platform_config FROM base_platform_app` 并只 `GRANT SELECT` —— 否则 `deploy/seed/02-app-role.sql:50,54-55` 的默认授权会让本用例失败（§8 Q-28）。
- **测试步骤**:
  1. `base_app` 连接：`SELECT count(*) FROM platform_config` → 记录 `n`
  2. `INSERT INTO platform_config (key, value) VALUES ('hijack', '"x"'::jsonb)`（用**不存在的键** —— 若权限错放，插入会成功；若用已存在的 `logo`，权限错放时会报主键冲突 `23505`，混淆失败原因）
  3. `UPDATE platform_config SET value = '"hijack"'::jsonb WHERE key = 'logo'`
  4. `DELETE FROM platform_config WHERE key = 'logo'`
  5. `SELECT has_table_privilege('base_app','platform_config','SELECT'/'INSERT'/'UPDATE'/'DELETE')` 四项
  6. 管理员连接核对 `logo` 的值、行数，以及 `key='hijack'` 的行是否存在
- **预期结果**:
  1. 第 1 步成功；`n > 0`（平台级行**对全体可见**是设计）。
  2. 第 2 步失败，SQLSTATE **`42501`**（`permission denied for table platform_config`）；**不是** 0 行受影响、**不是**静默成功。
  3. 第 3、4 步同样 `42501`。
  4. 第 5 步：`SELECT = true`；`INSERT / UPDATE / DELETE = false`（四项逐一断言，AC-S-062 只写了 INSERT —— UPDATE/DELETE 同样能改平台级配置，见 §8 Q-28）。
  5. 第 6 步 `logo` 仍为原值，行数仍为 `n`，且 `key='hijack'` 的行数 **0**（写侧越权没有留下任何行）。
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 表级权限 + 精确 SQLSTATE + 写侧越权（AC-S-062 的原始目的）

### TC-010-044: 租户表与空间表的 RLS 读隔离（含跨租同名 `space_id`）
- **关联 AC**: AC-S-062
- **前置条件**:
  - **执行位置**: 真实库（`base_test`）+ `base_app`。
  - 数据：`platform_tenant_config` 有 T1 与 T2 的行；`platform_space_config` 有 T1+S1、T1+S2、T2+S1（**S1 在 T1 与 T2 是同一个字符串**，§3.5）。
  - 租户上下文 = 会话变量 `app.tenant_id`（平台 RLS policy 读它，`code/crates/platform-db/src/rls.rs:141-150`）。
- **测试步骤**:
  1. `SET LOCAL app.tenant_id = 'tenant-a'` → `SELECT DISTINCT tenant_id FROM platform_tenant_config`
  2. `SET LOCAL app.tenant_id = 'tenant-b'` → 同查询
  3. **不设** `app.tenant_id` → 同查询
  4. `app.tenant_id = 'tenant-a'` → `SELECT tenant_id, space_id, key FROM platform_space_config WHERE space_id = 'space-sales-1'`
  5. `app.tenant_id = 'tenant-b'` → 同查询
  6. `app.tenant_id = 'tenant-a'` → `SELECT DISTINCT space_id FROM platform_space_config`
  7. 查 `pg_class.relrowsecurity`（两张表）与 `pg_policies` 的 `qual` / `with_check`
- **预期结果**:
  1. 结果集 = `{tenant-a}`（**逐条**列出）。
  2. 结果集 = `{tenant-b}`。
  3. **0 行**（不是报错 —— `current_setting(..., true)` 返回 NULL → 行不可见，fail closed）。
  4. 只有一条：`(tenant-a, space-sales-1, logo)`，值 C —— **不含** T2 的同名空间行（E）。
  5. 只有一条：`(tenant-b, space-sales-1, logo)`，值 E。
  6. 结果集 = `{space-sales-1, space-legal}`（T1 的两个空间），**不含** T2 的任何行。
  7. 两张表 `relrowsecurity = true`；各 **≥ 1** 条 policy；两张表的 `qual` 与 `with_check` **都**引用 `tenant_id`（不只 `qual`）。
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: RLS 读侧 + 复合维度（tenant, space）+ 未设上下文 fail closed + policy 结构核对

### TC-010-045: 空间表写侧 RLS（`WITH CHECK`）：写不进他租户的行
- **关联 AC**: AC-S-062
- **前置条件**:
  - **执行位置**: 真实库 + `base_app`。
  - 用例性质：**派生** —— AC-S-062 步骤 2 只覆盖平台级表的写侧、步骤 3/4 只覆盖读侧；空间表的**写侧**没有被任何 AC 覆盖（§8 Q-28 的同类缺口）。
- **测试步骤**:
  1. `app.tenant_id = 'tenant-a'`：`INSERT INTO platform_space_config (tenant_id, space_id, key, value) VALUES ('tenant-a','space-sales-1','footer','"ok"'::jsonb)`
  2. 同上下文：`INSERT ... VALUES ('tenant-b','space-sales-1','footer','"hijack"'::jsonb)`
  3. 同上下文：`UPDATE platform_space_config SET tenant_id='tenant-b' WHERE tenant_id='tenant-a' AND space_id='space-sales-1' AND key='logo'`
  4. 管理员连接核对：`tenant-b` 的 `footer` 行数、`tenant-a` 的 `logo` 行归属
- **预期结果**:
  1. 第 1 步成功（本租户写入）。
  2. 第 2 步失败，SQLSTATE **`42501`**（`WITH CHECK` 拒绝：写入的 `tenant_id` 与会话变量不符）；**不得**"写进去但读不出来"。
  3. 第 3 步失败 `42501`（不能把本租户的行改挂到别的租户）。
  4. 第 4 步：`tenant-b` 的 `footer` 行数 **0**；`tenant-a` 的 `logo` 行仍是 `tenant-a`。
  5. 结论：写侧越权在**数据库层**被拒，而不是只靠应用层过滤。
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: RLS 写侧（WITH CHECK）+ 精确 SQLSTATE + 归属核对

### TC-010-046: 三张表 `value` 列均为 `jsonb`；TEXT→jsonb 迁移的 down/up 可逆
- **关联 AC**: AC-S-062
- **前置条件**:
  - **执行位置**: 真实库（`base_test`）+ `sqlx migrate`。
  - 迁移命名假设：本需求的配置迁移为 `20261118001000_config_schema`（现有迁移最大号是 `20261118000901`），含配套 `.down.sql`（§7 A7）。
  - `platform_tenant_config` 迁移前为 `TEXT`（`code/migrations/20261118000200_tenant_isolation.up.sql:22`）。
- **测试步骤**:
  1. 查 `information_schema.columns`：三张表 `value` 的 `data_type`
  2. 向 `platform_tenant_config` 插三条值：`'"s"'::jsonb`、`'60'::jsonb`、`'true'::jsonb`，读回 `jsonb_typeof(value)`
  3. 执行本需求迁移的 **down**（`sqlx migrate down` 到前一版本，或直接执行 `.down.sql`）
  4. 查 `platform_tenant_config.value` 的 `data_type`；`SELECT value` 三条不报错
  5. 再执行 **up**；查类型并读回三条值的 `jsonb_typeof` 与内容
  6. `sqlx migrate status`：无 dirty；down 文件存在且可执行
- **预期结果**:
  1. 三张表全部 `jsonb`（**逐张**列出：`platform_config` / `platform_tenant_config` / `platform_space_config`）。
  2. `jsonb_typeof` 分别为 `string` / `number` / `boolean`。
  3. 第 3 步成功（退出码 0），无手工修库。
  4. 第 4 步 `platform_tenant_config.value` 变回 `text`；三条值可查（文本形态可以是带引号的 JSON 文本，这不算失败）；按 §7 A7 的迁移划分，本需求新建的 `platform_config` 与 `platform_space_config` 在 down 后**不存在**（`to_regclass(...) IS NULL`）。
  5. 第 5 步回到 `jsonb`；三条值的 `jsonb_typeof` 与**语义**（字符串仍是字符串、数字仍是数字、布尔仍是布尔）与第 2 步**完全一致** —— 这才是"可逆"的可判定判据（空表上的"可逆"是空断言，§8 Q-29）。
  6. 第 6 步无 dirty；`.down.sql` 存在；**不得**在 down 里 `DROP TABLE platform_tenant_config`（那是 `20261118000200` 的 down 的职责，重复 drop 会让"只回滚本需求"变成丢历史表）。
- **类型**: 可维护性 / 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 迁移可逆性（结构 + 值语义）+ 空断言防护

---
