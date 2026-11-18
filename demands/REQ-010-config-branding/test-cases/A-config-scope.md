# A. 配置作用域、`source` 与类型校验 —— 测试用例（AC-S-049、AC-S-050、AC-S-051）

- **需求**: REQ-010「配置、品牌与 Feature Flag」
- **来源**: [acceptance.md](../acceptance.md) §AC-S-049 ~ AC-S-051、[design-summary.md](../design-summary.md) §3 / §4、[api-conventions v2 §C9](../../../contracts/api-conventions/v2.md)
- **用例数**: 11（TC-010-001 ~ TC-010-011）
- **公共夹具**: 见 [index.md](index.md) §3 测试夹具

## A.0 本域约定

- **读取接口**：`GET /api/config/entries?keys=a,b`（design §6），**需要令牌**（任意已认证用户；本域读用普通用户 `u-2001`）。
- **响应形状**（design 未写死，本用例集按 §7 A6 取值）：`{"entries":[{"key","value","source"}, ...]}`，按 `keys` 的请求顺序；重复键去重；`value` 是 **JSON 原生类型**（不是字符串化的 JSON）。
- **`source` 四值**：`platform` / `tenant` / `space` / `default`（C9 v2 §9.3）。**只有四个** —— AC-S-049 写「五种」为计数错误（见 [index.md](index.md) §8 Q-06）。
- **`source` 与 `scope` 不是同一个类型**：写入作用域只有三值（`platform|tenant|space`）；`default` **不得**成为可写作用域（见 §8 Q-12、§7 A9）。
- **写入接口**：`PUT /api/config/entries`，body `{"scope","key","value"}`（§7 A2）。租户恒来自令牌，空间来自 `X-Space-Id`。
- **判定顺序**（§7 A9，AC 未写死）：**认证 401 → 权限点 403 → 键名格式 400 → 注册表 400 → 作用域 400 → 类型 400**。本域用例按此顺序断言，避免「错因顶包」。
- **400 的稳定错误码**（§7 A3，现有 4 个码里**没有** 400 码，见 §8 Q-16）：`CONFIG_KEY_NOT_REGISTERED` / `CONFIG_SCOPE_FORBIDDEN` / `CONFIG_TYPE_MISMATCH` / `CONFIG_VALUE_INVALID` / `INVALID_CONFIG_KEY`（后者是 `platform-core` 已有的键名错误码）。
- **`source` 的判别性**：`value` 相等 **不代表** `source` 相等 —— 平台级显式配 `#2f54eb` 与 manifest 默认 `#2f54eb` 必须一个 `platform`、一个 `default`（TC-010-002）。
- **执行位置**：TC-010-001 ~ 011 为**真实库（`base_test`）+ 真机 HTTP**（`/api/config/*` 由 `platform-server` 挂载）；TC-010-001 第 5 步与 TC-010-003 第 1 步为**单元层**（直接调 `ConfigResolver`）。

---

### TC-010-001: 三层覆盖逐层命中，`source` 与命中层级一一对应（含跨租同名 space_id）
- **关联 AC**: AC-S-049
- **前置条件**:
  - **执行位置**: 真实库（`base_test`）+ 真机 HTTP；第 5 步为单元层。
  - 夹具见 [index.md](index.md) §3.5：平台 `logo` = `/branding/platform-a.svg`（A）；T1 租户级 = `/branding/tenant-a.svg`（B）；T1+S1 = `/branding/space-s1.svg`（C）；T1+S2 = `/branding/space-s2.svg`（D）；T2+S1 = `/branding/space-t2-s1.svg`（E）；T2 无租户级覆盖。
  - 令牌：`u-2001`（T1，`platform-user`，无 config 权限点 —— 读接口只要令牌）；读 T2 用 `u-2005`（T2 的账号；**不能**用 T1 令牌：头与令牌不一致 → 404）。
- **测试步骤**:
  1. `GET /api/config/entries?keys=logo` + `Authorization: Bearer <u-2001>` + `X-Tenant-Id: tenant-a`
  2. 同一请求 + `X-Space-Id: space-sales-1`
  3. `Authorization: Bearer <u-2005>` + `X-Tenant-Id: tenant-b` + `X-Space-Id: space-sales-1`
  4. `Authorization: Bearer <u-2005>` + `X-Tenant-Id: tenant-b`（**不带**空间头）
  5. 单元层：`ConfigResolver::resolve(&RequestContext::new(), &ConfigKey::parse("logo")?)`
- **预期结果**:
  1. HTTP **200**；`entries` 长度 **1**；`entries[0].key == "logo"`；`entries[0].value == "/branding/tenant-a.svg"`；`entries[0].source == "tenant"`。
  2. HTTP **200**；`value == "/branding/space-s1.svg"`；`source == "space"`。
  3. HTTP **200**；`value == "/branding/space-t2-s1.svg"`；`source == "space"` —— **不得**返回 T1 的 `C`：`space_id` 相同的字符串在另一租户下必须解析到**另一行**（复合维度是 `(tenant_id, space_id)`，不是 `space_id`）。
  4. HTTP **200**；`value == "/branding/platform-a.svg"`；`source == "platform"`。
  5. 返回 `Some`，`value == "/branding/platform-a.svg"`，`source == Platform`。
  6. 逐条核对：`tenant ≠ platform ≠ space`，且每个 `source` 都与**实际存储层**对应（不是硬编码成同一个值）。
- **类型**: 功能 / 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 判定表（3 层 × 2 租户 × 2 空间）+ 跨租对照

### TC-010-002: `source=default` 与「值相同但来源不同」必须可区分
- **关联 AC**: AC-S-049
- **前置条件**:
  - **执行位置**: 真实库（`base_test`）+ 真机 HTTP。
  - `title` 在 manifest 登记默认值 `示例平台`，**三层都没写**。
  - `primaryColor` 在 manifest 登记默认值 `#2f54eb`，且**平台级显式写入同值** `#2f54eb`（这是判别性设计：值相等、来源不同）。
  - `notRegistered` 未在任何 manifest 登记。
- **测试步骤**:
  1. 以 `u-2001` 读 `title`（`X-Tenant-Id: tenant-a`）
  2. 以 `u-2001` 读 `primaryColor`（同租户）
  3. 以 `u-2003`（P，含 `config:platform:write`）把平台级 `primaryColor` 再写一次 `#2f54eb`，然后重读（同第 2 步）
  4. 以 `u-2001` 读 `notRegistered`
- **预期结果**:
  1. **200**；`value == "示例平台"`；`source == "default"` —— **不得** `platform`。
  2. **200**；`value == "#2f54eb"`；`source == "platform"` —— 虽然值与默认相同，仍必须是 `platform`（证明 `source` 来自存储命中，不是值比对）。
  3. **200**；`value` 仍 `#2f54eb`；`source` 仍 `platform` —— 重复写入不改变来源。
  4. **404**；`code == "NOT_FOUND"`；`detail` 为英文 ASCII 且含 `notRegistered`；**不得** 200、**不得** 500。
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 等价类（值相等 / 来源不同）+ 判别性反例（值比对实现会失败）

### TC-010-003: 「无租户上下文」只能在该解析器层构造；HTTP 层表现为 401
- **关联 AC**: AC-S-049
- **前置条件**:
  - **执行位置**: 单元层 + 真实库 + 真机 HTTP。
  - 现状：`/api/rbac/*` 的认证中间件在缺头时**由令牌推断租户**（`code/crates/platform-rbac/src/http.rs:187-193`）；`require_permission` 在缺 `tenant_id` 时直接 **401**（`code/crates/platform-core/src/extension/permission.rs:319-323`）。
- **测试步骤**:
  1. 单元层：`RequestContext::new()`（无 tenant / 无 user）解析 `logo`
  2. HTTP：`GET /api/config/entries?keys=logo` + `X-Tenant-Id: tenant-a`，**不带** `Authorization`
  3. HTTP：带 `u-2001` 令牌，**不带** `X-Tenant-Id`
- **预期结果**:
  1. `Some`，`value == "/branding/platform-a.svg"`，`source == Platform`。
  2. **401**，`code == "UNAUTHENTICATED"` —— **不是** 200；读接口「需要令牌」。
  3. **200**；`value == "/branding/tenant-a.svg"`；`source == "tenant"` —— 租户由**令牌**推断（契约 C3），这不是「无租户上下文」。
  4. 结论记录：AC-S-049 步骤 1 的「以无租户上下文读」在**已认证 HTTP 路径上不可达**；本用例把该断言的唯一可执行形态固定在单元层（见 [index.md](index.md) §8 Q-07）。
- **类型**: 功能 / 安全
- **优先级**: P1
- **自动化**: 可自动化
- **设计方法**: 可达性分析（区分「接口拒绝」与「该上下文不存在」）

### TC-010-004: 键名边界与批量读的未登记键
- **关联 AC**: AC-S-049
- **前置条件**:
  - **执行位置**: 真实库 + 真机 HTTP。
  - `theme.radius` 已登记（`type: number`，默认 `4`）；`Theme.radius` / `theme-radius` 不是合法键名（`ConfigKey::parse` 只接受 lowerCamelCase + `.` 分层，`code/crates/platform-core/src/config.rs:90-100`）。
- **测试步骤**:
  1. 读 `theme.radius`（T1）
  2. 读 `Theme.radius`
  3. 读 `theme-radius`
  4. 读 `keys=logo,notRegistered`（T1）
  5. 读 `keys=logo,logo`（T1）
- **预期结果**:
  1. **200**；`value == 4`（JSON number）；`source == "default"`。
  2. **400**；`code == "INVALID_CONFIG_KEY"`；`params.key == "Theme.radius"`。
  3. **400**；`code == "INVALID_CONFIG_KEY"`。
  4. **404**；`code == "NOT_FOUND"`；`params.unknownKeys == ["notRegistered"]`；响应体**不含** `entries`（不做部分返回）。
  5. **200**；`entries` 长度 **1**（去重；§7 A6）。
- **类型**: 功能
- **优先级**: P1
- **自动化**: 可自动化
- **设计方法**: 边界值（非法键名）+ 批量接口的未知键语义

### TC-010-005: `scope: platform` 的键：租户/空间写一律 400（且有权的平台写必须成功）
- **关联 AC**: AC-S-050
- **前置条件**:
  - **执行位置**: 真实库 + 真机 HTTP。
  - `conflictStrategy` 在 `crates/platform-org/manifest.json` 登记为 `scope: platform`（已核实）。
  - 写者 `u-2002`（A，含 `config:entry:write`）；对照写者 `u-2003`（P，含 `config:platform:write`）。
- **测试步骤**:
  1. A：`PUT /api/config/entries` body `{"scope":"tenant","key":"conflictStrategy","value":"remote_wins"}`（`X-Tenant-Id: tenant-a`）
  2. A：同 body，`"scope":"space"` + `X-Space-Id: space-sales-1`
  3. P：`{"scope":"platform","key":"conflictStrategy","value":"local_wins"}`
  4. 用管理员连接查三张表里 `key = 'conflictStrategy'` 的行
- **预期结果**:
  1. **400**；`code == "CONFIG_SCOPE_FORBIDDEN"`；`params.allowedScope == "platform"`；`params.key == "conflictStrategy"`。**不得** 403 —— A 有写权限点，拒绝原因是**作用域**。
  2. **400**；同码同 `params`。
  3. **200**；响应 `{"key":"conflictStrategy","value":"local_wins","source":"platform"}`。
  4. `platform_config` 中该值为 `local_wins`；`platform_tenant_config` 与 `platform_space_config` 中 `key='conflictStrategy'` 的行数 **均为 0**（越级写没有留下任何行）。
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 越级写 + 正向对照（三步都是 400 的接口无法证明不是恒拒）

### TC-010-006: `scope: tenant` 的键：租户可写、空间不可写
- **关联 AC**: AC-S-050
- **前置条件**:
  - **执行位置**: 真实库 + 真机 HTTP。
  - `pageSize` 在 `modules/sandbox/manifest.json` 登记为 `scope: tenant`（已核实）。
  - 写者 A（`u-2002`）。
- **测试步骤**:
  1. A：`{"scope":"tenant","key":"pageSize","value":50}`（T1）
  2. A：`{"scope":"space","key":"pageSize","value":50}` + `X-Space-Id: space-sales-1`
  3. 读 `pageSize`（T1，`u-2001`）
- **预期结果**:
  1. **200**；`value == 50`。
  2. **400**；`code == "CONFIG_SCOPE_FORBIDDEN"`；`params.allowedScope == "tenant"`。
  3. **200**；`value == 50`；`source == "tenant"` —— 第 1 步的写入生效，第 2 步没有产生空间级行。
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 判定表（允许最深层 = tenant）+ 状态验证

### TC-010-007: `scope: space` 的键：三层都可写，覆盖只影响被改的那一层
- **关联 AC**: AC-S-050
- **前置条件**:
  - **执行位置**: 真实库 + 真机 HTTP。
  - `logo` 登记为 `scope: space`；初始值 A/B/C/D/E 见 TC-010-001。
  - 写者 A（`u-2002`）。
- **测试步骤**:
  1. A：`{"scope":"space","key":"logo","value":"/branding/space-s1-new.svg"}` + `X-Space-Id: space-sales-1`
  2. A：`{"scope":"tenant","key":"logo","value":"/branding/tenant-a-new.svg"}`（T1）
  3. 依次读 `logo`：T1、T1+S1、T1+S2
- **预期结果**:
  1. **200**。
  2. **200**。
  3. T1 → `/branding/tenant-a-new.svg` / `tenant`；T1+S1 → `/branding/space-s1-new.svg` / `space`；T1+S2 → **`/branding/space-s2.svg`**（D，未被步骤 1 波及）/ `space`。
  4. 第 3 步的 T1+S2 是**局部性判据**：写 S1 不得改动 S2。
- **类型**: 功能 / 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 三层写入 + 局部性断言

### TC-010-008: 未登记键写入被拒，且不落库
- **关联 AC**: AC-S-051
- **前置条件**:
  - **执行位置**: 真实库 + 真机 HTTP。
  - `notRegistered` 不在任何 manifest 的 `config` 段；`theme.radius` 已登记。
  - 写者 P（`u-2003`），确保权限与作用域都不是拒绝原因。
- **测试步骤**:
  1. P：`{"scope":"platform","key":"notRegistered","value":1}`
  2. P：`{"scope":"tenant","key":"notRegistered","value":1}`（T1）
  3. 用管理员连接查三张表 `key='notRegistered'` 的行数
  4. 对照：P 写 `{"scope":"platform","key":"theme.radius","value":8}` → 再读
- **预期结果**:
  1. **400**；`code == "CONFIG_KEY_NOT_REGISTERED"`；`params.key == "notRegistered"`。
  2. **400**；同码 —— **不得** `CONFIG_SCOPE_FORBIDDEN`（注册表判定先于作用域，§7 A9）。
  3. 三张表行数 **均为 0**（不是"报错了但写进去了"）。
  4. **200**；读回 `theme.radius == 8`、`source == "platform"` —— 证明接口不是恒拒。
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 未声明键 + 静默放行防护（库侧核对）+ 正向对照

### TC-010-009: 类型按 JSON 类型判定：字符串 `"60"` / 布尔 `true` / 数字 `60`
- **关联 AC**: AC-S-051
- **前置条件**:
  - **执行位置**: 真实库 + 真机 HTTP。
  - `cacheTtlSeconds` 在 `crates/platform-rbac/manifest.json` 登记为 `type: number`、`scope: platform`（已核实）。
  - 写者 P。**必须写平台级** —— 若写租户级，会先被作用域规则拒绝，类型校验得不到验证（见 [index.md](index.md) §8 Q-14）。
- **测试步骤**:
  1. P：`{"scope":"platform","key":"cacheTtlSeconds","value":"60"}`（JSON 字符串）
  2. P：同键，`value: true`
  3. P：同键，`value: 60`
  4. 读 `cacheTtlSeconds`（T1）
  5. 查库 `jsonb_typeof(value)`
  6. **收尾**：把平台级 `cacheTtlSeconds` 恢复成 `300`（否则会影响 AC-S-052 的 TTL 前提）
- **预期结果**:
  1. **400**；`code == "CONFIG_TYPE_MISMATCH"`；`params.expected == "number"`；`params.actual == "string"`。
  2. **400**；`params.expected == "number"`；`params.actual == "boolean"`。
  3. **200**；`value == 60`。
  4. **200**；`value` 是 JSON **number**（`value.is_number() == true`）；**不得**是字符串 `"60"`。
  5. `jsonb_typeof(value) == "number"`（数据库层也是数字，不是双重编码）。
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 类型等价类（JSON 类型 ≠ 语言隐式转换）+ 库侧旁证

### TC-010-010: 五种登记类型的正反例（string / number / boolean / array / object）
- **关联 AC**: AC-S-051
- **前置条件**:
  - **执行位置**: 真实库 + 真机 HTTP。
  - 夹具登记的键与类型：`title`(string)、`theme.radius`(number)、`beta`(boolean)、`theme.fontStack`(array)、`theme.tokens`(object)。
  - 写者 P（平台级写）。
- **测试步骤**:
  1. 逐个写**错误类型**：`title←123`、`theme.radius←"4"`、`beta←0`、`theme.fontStack←{}`、`theme.tokens←[]`
  2. 逐个写**正确类型**：`title←"示例平台2"`、`theme.radius←8`、`beta←true`、`theme.fontStack←["system-ui","sans-serif"]`、`theme.tokens←{"density":"compact"}`
  3. 逐项读回并核对 JSON 类型
- **预期结果**:
  1. 5 次全部 **400**，`code == "CONFIG_TYPE_MISMATCH"`；每次的 `params.expected` / `params.actual` 逐条对应：`string/number`、`number/string`、`boolean/number`、`array/object`、`object/array`。**不得**有 200。
  2. 5 次全部 **200**。
  3. 读回：`title` 是 string；`theme.radius` 是 number；`beta` 是 boolean 且为 `true`；`theme.fontStack` 是长度为 2 的 array；`theme.tokens` 是 object 且 `density == "compact"`。
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 判定表（5 种类型 × 正反）

### TC-010-011: 校验顺序固定：认证 → 权限点 → 键名格式 → 注册表 → 作用域 → 类型
- **关联 AC**: AC-S-051
- **前置条件**:
  - **执行位置**: 真实库 + 真机 HTTP。
  - A（`u-2002`）有 `config:entry:write`；P（`u-2003`）另有 `config:platform:write`。
- **测试步骤**:
  1. A：`{"scope":"space","key":"notRegistered","value":1}` + `X-Space-Id: space-sales-1`（**三类错误同时成立**：未登记 + 越级 + 无类型信息）
  2. A：`{"scope":"tenant","key":"cacheTtlSeconds","value":"60"}`（越级 + 类型错）
  3. P：`{"scope":"platform","key":"cacheTtlSeconds","value":"60"}`（仅类型错）
  4. P：`{"scope":"platform","key":"Theme.radius","value":1}`（键名格式非法 + 未登记）
- **预期结果**:
  1. **400**；`code == "CONFIG_KEY_NOT_REGISTERED"`（注册表优先；**不得** `CONFIG_SCOPE_FORBIDDEN`）。
  2. **400**；`code == "CONFIG_SCOPE_FORBIDDEN"`（作用域先于类型；**不得** `CONFIG_TYPE_MISMATCH`）。
  3. **400**；`code == "CONFIG_TYPE_MISMATCH"`。
  4. **400**；`code == "INVALID_CONFIG_KEY"`（键名格式先于注册表；**不得** `CONFIG_KEY_NOT_REGISTERED`）。
  5. 四次错误码**互不相同**且与上表逐条对应 —— 这是"错因可区分"的判据；若都返回同一个码，用例判失败。
- **类型**: 安全
- **优先级**: P1
- **自动化**: 可自动化
- **设计方法**: 判定顺序（多错因输入）+ 错误码严格区分（第 2 步同时验证 AC-S-050 的作用域判定参与排序；矩阵按主 AC 计入）

---
