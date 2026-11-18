# A. 配置作用域、`source` 与类型校验 —— 测试用例（AC-S-049、AC-S-050、AC-S-051）

- **需求**: REQ-010「配置、品牌与 Feature Flag」（**rev 2**，对齐 2026-11-18 23:10 修订后的 acceptance / design）
- **来源**: [acceptance.md](../acceptance.md) §AC-S-049 ~ AC-S-051、[design-summary.md](../design-summary.md) §3 / §3.3 / §4、[api-conventions v2 §C9](../../../contracts/api-conventions/v2.md)
- **用例数**: 11（TC-010-001 ~ TC-010-011）
- **公共夹具**: 见 [index.md](index.md) §3 测试夹具

## A.0 本域约定（rev 2）

- **读取接口**：`GET /api/config/entries?keys=a,b`（design §6），**需要令牌**；**平台级键另需 `config:platform:read`**（AC-S-063）。本域读的 `logo` / `title` / `pageSize` 都**不是**平台级键（`logo` 是 `space` 作用域，只是值可能来自平台层），所以用普通用户 `u-2001` 即可。
- **读取响应形状**（design 未重新定义，按 §7 A6）：`{"entries":[{"key","value","source"}]}`，按 `keys` 请求顺序；重复键去重；`value` 是 **JSON 原生类型**。
- **`source` 四值**（AC-S-049 明确）：`platform` / `tenant` / `space` / `default`。第 5 步是 404，**没有 `source`**（不是第 5 种取值）。
- **`source` 与 `scope` 是两个枚举**（D60）：`ConfigSource` 四值只用于 `source`；`ConfigScope` 保持三值（`platform|tenant|space`），`default` **不是**可写作用域。
- **写接口与 body**（BA 裁定 A2）：`PUT /api/config/entries`（租户级 / 空间级）与 **`PUT /api/config/platform-entries`**（平台级，D59）分开；body 为
  `{"scope":"tenant|space|platform","entries":[{"key":"<key>","value":<JSON>}, ...]}`。
  租户恒来自令牌，空间来自 `X-Space-Id`；`scope` 必须与路由匹配（在 `/entries` 上写 `platform` → 400 `CONFIG_SCOPE_NOT_ALLOWED`）。
- **判定顺序**（D64，写死）：**认证 401 → 权限点 403 → 键名格式 400 → 注册表 400 → 作用域 400 → 类型/范围 400**。
- **6 个稳定错误码**（D62）：`CONFIG_KEY_INVALID` / `CONFIG_KEY_NOT_REGISTERED` / `CONFIG_SCOPE_NOT_ALLOWED` / `CONFIG_TYPE_MISMATCH` / `CONFIG_VALUE_OUT_OF_RANGE` / `FLAG_NAME_INVALID`。
  读取未登记键 → **404** `CONFIG_KEY_NOT_REGISTERED`；写入未登记键 → **400** 同码。
- **空间级写有两道门**（D66）：`scope: space` **且** 键在品牌白名单内。只满足前者 → 400 `CONFIG_SCOPE_NOT_ALLOWED`。
- **品牌白名单枚举具体键**（D66）：`logo` / `favicon` / `title` / `loginBackground` / `footer` / `copyright` / `primaryColor` / `theme.radius` / `theme.mode` / `theme.density` / `theme.fontStack`（**无 `theme.*` 通配**）。
- **执行位置**：TC-010-001 ~ 011 为**真实库（`base_test`）+ 真机 HTTP**；TC-010-001 第 5 步与 TC-010-003 第 1 步为**单元层**（直接调 `ConfigResolver`）。

---

### TC-010-001: 三层覆盖逐层命中，`source` 与命中层级一一对应（含跨租同名 space_id）
- **关联 AC**: AC-S-049
- **前置条件**:
  - **执行位置**: 真实库（`base_test`）+ 真机 HTTP；第 5 步为单元层。
  - 夹具见 [index.md](index.md) §3.5：平台 `logo`=`/branding/platform-a.svg`（A/PA）；T1 租户级=`/branding/tenant-a.svg`（B/PB）；T2 租户级=`/branding/tenant-b.svg`（PC）；T1+S1=`/branding/space-s1.svg`（C/PS）；T1+S2=`/branding/space-s2.svg`（D）；T2+S1=`/branding/space-t2-s1.svg`（E）。
  - 令牌：`u-2001`（T1，`platform-user`）；读 T2 用 `u-2005`（T2 账号 —— 用 T1 令牌 + `X-Tenant-Id: tenant-b` 会 404）。
- **测试步骤**:
  1. `GET /api/config/entries?keys=logo` + `u-2001` 令牌 + `X-Tenant-Id: tenant-a`
  2. 同一请求 + `X-Space-Id: space-sales-1`
  3. `u-2005` 令牌 + `X-Tenant-Id: tenant-b` + `X-Space-Id: space-sales-1`
  4. `u-2005` 令牌 + `X-Tenant-Id: tenant-b`（**不带**空间头）
  5. 单元层：`resolver.resolve(&RequestContext::new(), &ConfigKey::parse("logo")?)`（无租户上下文）
- **预期结果**:
  1. HTTP **200**；`entries` 长度 **1**；`entries[0].value == "/branding/tenant-a.svg"`；`entries[0].source == "tenant"`。
  2. HTTP **200**；`value == "/branding/space-s1.svg"`；`source == "space"`。
  3. HTTP **200**；`value == "/branding/space-t2-s1.svg"`；`source == "space"` —— **不得**返回 T1 的 `C`：同一 `space_id` 字符串在另一租户下必须解析到**另一行**（复合维度是 `(tenant_id, space_id)`）。
  4. HTTP **200**；`value == "/branding/tenant-b.svg"`；`source == "tenant"`（T2 有租户覆盖，**不能**回落到平台 A）。
  5. 返回 `Some`，`value == "/branding/platform-a.svg"`，`source == Platform`。
  6. 逐条核对：四个 `source` 值与实际存储层一一对应，不是硬编码成同一个值。
- **类型**: 功能 / 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 判定表（3 层 × 2 租户 × 2 空间）+ 跨租对照

### TC-010-002: `source=default` 与「值相同但来源不同」必须可区分
- **关联 AC**: AC-S-049
- **前置条件**:
  - **执行位置**: 真实库 + 真机 HTTP。
  - `title` 登记默认值 `示例平台`，**三层都没写**；T1 有租户级 `title=T1平台`，所以用 **T2** 读默认值。
  - `primaryColor` 登记默认值 `#111111`，且**平台级显式写入同值** `#111111`（判别性设计：值相等、来源不同）。
  - `notRegistered` 未在任何 manifest 登记。
- **测试步骤**:
  1. 以 `u-2005`（T2）读 `title`
  2. 以 `u-2005`（T2）读 `primaryColor`
  3. 以 `u-2003`（P，含 `config:platform:write`）经 `PUT /api/config/platform-entries` 把平台级 `primaryColor` 再写一次 `#111111`，然后重读（同第 2 步）
  4. 以 `u-2001` 读 `notRegistered`
- **预期结果**:
  1. **200**；`value == "示例平台"`；`source == "default"` —— **不得** `platform`。
  2. **200**；`value == "#111111"`；`source == "platform"` —— 虽然值与默认相同，仍必须是 `platform`（证明 `source` 来自存储命中，不是值比对）。
  3. **200**；`value` 仍 `#111111`；`source` 仍 `platform` —— 重复写入不改变来源。
  4. **404**；`code == "CONFIG_KEY_NOT_REGISTERED"`；`params.unknownKeys == ["notRegistered"]`；**不得** 200、**不得** 500；**不得**带 `source` 字段。
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 等价类（值相等 / 来源不同）+ 判别性反例（值比对实现会失败）

### TC-010-003: 「无租户上下文」在单元层验证；HTTP 层断言 401
- **关联 AC**: AC-S-049
- **前置条件**:
  - **执行位置**: 单元层 + 真实库 + 真机 HTTP。
  - 现状：认证中间件在缺头时**由令牌推断租户**（`code/crates/platform-rbac/src/http.rs:187-193`）；`require_permission` 在缺 `tenant_id` 时直接 **401**（`code/crates/platform-core/src/extension/permission.rs:319-323`）。
- **测试步骤**:
  1. 单元层：`RequestContext::new()`（无 tenant / 无 user）解析 `logo`
  2. HTTP：`GET /api/config/entries?keys=logo` + `X-Tenant-Id: tenant-a`，**不带** `Authorization`
  3. HTTP：带 `u-2001` 令牌，**不带** `X-Tenant-Id`
- **预期结果**:
  1. `Some`，`value == "/branding/platform-a.svg"`，`source == Platform`。
  2. **401**，`code == "UNAUTHENTICATED"` —— **不是** 200；读接口「需要令牌」。
  3. **200**；`value == "/branding/tenant-a.svg"`；`source == "tenant"` —— 租户由**令牌**推断（契约 C3），这不是「无租户上下文」。
  4. 结论：AC-S-049 步骤 1 的「平台上下文读」只能在单元层执行；HTTP 层按 AC 原文断言 401。
- **类型**: 功能 / 安全
- **优先级**: P1
- **自动化**: 可自动化
- **设计方法**: 可达性分析（区分「接口拒绝」与「该上下文不存在」）

### TC-010-004: 键名边界与批量读的未登记键
- **关联 AC**: AC-S-049
- **前置条件**:
  - **执行位置**: 真实库 + 真机 HTTP。
  - `theme.radius` 已登记；`Theme.radius` / `theme-radius` 不是合法键名（`ConfigKey::parse` 只接受 lowerCamelCase + `.` 分层，`code/crates/platform-core/src/config.rs:90-100`）。
- **测试步骤**:
  1. 读 `theme.radius`（T1）
  2. 读 `Theme.radius`
  3. 读 `theme-radius`
  4. 读 `keys=logo,notRegistered`（T1，AC-S-049 步骤 6）
  5. 读 `keys=logo,logo`（T1）
- **预期结果**:
  1. **200**；`value == 4`（JSON number）；`source == "default"`。
  2. **400**；`code == "CONFIG_KEY_INVALID"`；`params.key == "Theme.radius"`。
  3. **400**；`code == "CONFIG_KEY_INVALID"`。
  4. **404**；`code == "CONFIG_KEY_NOT_REGISTERED"`；`params.unknownKeys == ["notRegistered"]`；响应体**不含** `entries`（整请求 fail closed，不返回 `logo` 的部分结果）。
  5. **200**；`entries` 长度 **1**（重复键去重，§7 A6）。
- **类型**: 功能
- **优先级**: P1
- **自动化**: 可自动化
- **设计方法**: 边界值（非法键名）+ 批量接口的未知键语义（整请求 404）

### TC-010-005: 作用域越级写：六步判定表 + 平台级正对照
- **关联 AC**: AC-S-050
- **前置条件**:
  - **执行位置**: 真实库 + 真机 HTTP。
  - `conflictStrategy` 登记 `scope: platform`；`logo` 登记 `scope: space`（且在白名单内）；`pageSize` 登记 `scope: tenant`。
  - 写者 `u-2002`（A，含 `config:entry:write`）；平台级对照写者 `u-2003`（P）。
- **测试步骤**:
  1. A：`PUT /api/config/entries` `{"scope":"tenant","entries":[{"key":"conflictStrategy","value":"remote_wins"}]}`（T1）
  2. A：同 body，`"scope":"space"` + `X-Space-Id: space-sales-1`
  3. A：`{"scope":"tenant","entries":[{"key":"pageSize","value":50}]}`
  4. A：`{"scope":"space","entries":[{"key":"pageSize","value":50}]}` + S1
  5. A：`{"scope":"tenant","entries":[{"key":"logo","value":"/branding/tenant-a-new.svg"}]}`
  6. A：`{"scope":"space","entries":[{"key":"logo","value":"/branding/space-s1-new.svg"}]}` + S1
  7. P：`PUT /api/config/platform-entries` `{"scope":"platform","entries":[{"key":"conflictStrategy","value":"local_wins"}]}`
  8. 管理员连接查三张表里 `key='conflictStrategy'` 的行
- **预期结果**:
  1. **400**；`code == "CONFIG_SCOPE_NOT_ALLOWED"`；`params.allowedScope == "platform"`；`params.attemptedScope == "tenant"`。**不得** 403（A 有写权限点，拒绝原因是作用域）。
  2. **400**；同码；`attemptedScope == "space"`。
  3. **200**。
  4. **400**；`code == "CONFIG_SCOPE_NOT_ALLOWED"`；`params.allowedScope == "tenant"`。
  5. **200**；读回 T1 `logo` = `/branding/tenant-a-new.svg`、`source="tenant"`。
  6. **200**；读 T1+S1 `logo` = `/branding/space-s1-new.svg`、`source="space"`。
  7. **200**；平台 `conflictStrategy == "local_wins"`、`source="platform"`（正对照：接口不是恒拒）。
  8. `platform_config` 中该值 = `local_wins`；另外两张表里 `key='conflictStrategy'` 的行数 **均为 0**（越级写没有留下任何行）。
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 判定表（scope × 写入层）+ 正向对照（每档都有 200 的对照组）

### TC-010-006: 空间级写要求「在白名单内」（D66 的两道门）
- **关联 AC**: AC-S-050
- **前置条件**:
  - **执行位置**: 真实库 + 真机 HTTP。
  - `loginBanner` 在 `crates/platform-config/manifest.json` 登记为 `scope: space`，但**不在**品牌白名单内（D66 的判据需要一个这样的键 —— 见 [index.md](index.md) §3.4 与 §8.9 N13）。
  - `logo` 登记 `scope: space` **且在白名单内**。
  - 写者 A。
- **测试步骤**:
  1. A：`{"scope":"space","entries":[{"key":"loginBanner","value":"/banner.png"}]}` + `X-Space-Id: space-sales-1`
  2. A：`{"scope":"tenant","entries":[{"key":"loginBanner","value":"/banner-tenant.png"}]}`（T1）
  3. 读 T1 + S1 的 `loginBanner`
  4. A：`{"scope":"space","entries":[{"key":"logo","value":"/branding/space-ok.svg"}]}` + S1（对照：白名单内的 space 键）
- **预期结果**:
  1. **400**；`code == "CONFIG_SCOPE_NOT_ALLOWED"`；`params.key == "loginBanner"`，且 `detail` / `params` 中能看出拒绝原因是**白名单**（英文 `whitelist`）—— **不是**「写进去后被忽略」。
  2. **200** —— 非白名单只拦**空间级**，租户级仍可写（`scope: space` 的允许最深层含租户层）。
  3. **200**；`value == "/banner-tenant.png"`；`source == "tenant"`（步骤 1 没有产生空间级行）。
  4. **200**；读 T1+S1 `logo` = `/branding/space-ok.svg`、`source="space"` —— 白名单内的空间级写不受影响。
  5. 用管理员连接核对 `platform_space_config` 中 `key='loginBanner'` 的行数 **0**。
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: fail closed 边界（两道门）+ 对照（白名单内成功）+ 库侧证据

### TC-010-007: `scope: space` 的键：三层都可写，覆盖只影响被改的那一层
- **关联 AC**: AC-S-050
- **前置条件**:
  - **执行位置**: 真实库 + 真机 HTTP。
  - `logo` 登记为 `scope: space` 且在白名单内；初始值 A/B/PC/PS/D/E 见 TC-010-001。
  - 写者 A。
- **测试步骤**:
  1. A：`{"scope":"space","entries":[{"key":"logo","value":"/branding/space-s1-new.svg"}]}` + `X-Space-Id: space-sales-1`
  2. A：`{"scope":"tenant","entries":[{"key":"logo","value":"/branding/tenant-a-new.svg"}]}`（T1）
  3. 依次读 `logo`：T1、T1+S1、T1+S2
- **预期结果**:
  1. **200**。2. **200**。
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
  - 写者 A（租户级）。
- **测试步骤**:
  1. A：`{"scope":"tenant","entries":[{"key":"notRegistered","value":1}]}`
  2. A：`{"scope":"space","entries":[{"key":"notRegistered","value":1}]}` + S1
  3. 管理员连接查三张表 `key='notRegistered'` 的行数
  4. 对照：A 写 `{"scope":"tenant","entries":[{"key":"theme.radius","value":8}]}` → 再读
- **预期结果**:
  1. **400**；`code == "CONFIG_KEY_NOT_REGISTERED"`；`params.key == "notRegistered"`。
  2. **400**；同码 —— **不得** `CONFIG_SCOPE_NOT_ALLOWED`（注册表判定先于作用域，D64）。
  3. 三张表行数 **均为 0**（不是"报错了但写进去了"）。
  4. **200**；读回 `theme.radius == 8`、`source=="tenant"` —— 证明接口不是恒拒。
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 未声明键 + 静默放行防护（库侧核对）+ 正向对照

### TC-010-009: 类型按 JSON 类型判定（用 `pageSize`，避免越级写顶包）
- **关联 AC**: AC-S-051
- **前置条件**:
  - **执行位置**: 真实库 + 真机 HTTP。
  - `pageSize` 登记 `type: number` / `scope: tenant`（**租户作用域的键** —— 这就是 AC-S-051 改用它的原因：若用 `scope: platform` 的 `cacheTtlSeconds`，租户级写会先被作用域规则 400，类型校验根本测不到）。
  - 写者 A，**全部在租户级**。
- **测试步骤**:
  1. A：`{"scope":"tenant","entries":[{"key":"pageSize","value":"20"}]}`（JSON 字符串）
  2. A：同键，`value: true`
  3. A：同键，`value: 20`
  4. 读 `pageSize`（T1）
  5. 查库 `jsonb_typeof(value)`
- **预期结果**:
  1. **400**；`code == "CONFIG_TYPE_MISMATCH"`；`params.expected == "number"`；`params.actual == "string"`。
  2. **400**；`params.expected == "number"`；`params.actual == "boolean"`。
  3. **200**。
  4. **200**；`value` 是 JSON **number**（`is_number() == true`）；**不得**是字符串 `"20"`。
  5. `jsonb_typeof(value) == "number"`（数据库层也是数字，不是双重编码）。
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 类型等价类（JSON 类型 ≠ 语言隐式转换）+ 库侧旁证

### TC-010-010: 五种登记类型的正反例（全部写在允许的作用域内）
- **关联 AC**: AC-S-051
- **前置条件**:
  - **执行位置**: 真实库 + 真机 HTTP。
  - 键与类型：`title`(string, space)、`pageSize`(number, tenant)、`newInvoice`(boolean, tenant)、`theme.fontStack`(array, space)、`theme.tokens`(object, tenant)。
  - 写者 A，**全部在租户级**（space 作用域的键允许写到租户层；tenant 作用域的键也可）。
- **测试步骤**:
  1. 逐个写**错误类型**：`title←123`、`pageSize←"4"`、`newInvoice←0`、`theme.fontStack←{}`、`theme.tokens←[]`
  2. 逐个写**正确类型**：`title←"示例平台2"`、`pageSize←8`、`newInvoice←true`、`theme.fontStack←["system-ui","sans-serif"]`、`theme.tokens←{"density":"compact"}`
  3. 逐项读回并核对 JSON 类型
- **预期结果**:
  1. 5 次全部 **400**，`code == "CONFIG_TYPE_MISMATCH"`；`params.expected/actual` 逐条对应：`string/number`、`number/string`、`boolean/number`、`array/object`、`object/array`。**不得**有 200。
  2. 5 次全部 **200**。
  3. 读回：`title` 是 string；`pageSize` 是 number；`newInvoice` 是 boolean 且为 `true`；`theme.fontStack` 是长度 2 的 array；`theme.tokens` 是 object 且 `density=="compact"`。
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 判定表（5 种类型 × 正反）

### TC-010-011: 校验顺序写死：认证 → 权限点 → 键名 → 注册表 → 作用域 → 类型
- **关联 AC**: AC-S-051
- **前置条件**:
  - **执行位置**: 真实库 + 真机 HTTP。
  - A（`u-2002`）有 `config:entry:write`；P（`u-2003`）另有平台权限点。
- **测试步骤**:
  1. A：`{"scope":"space","entries":[{"key":"notRegistered","value":1}]}` + `X-Space-Id: space-sales-1`（**注册表错 + 作用域错**同时成立）
  2. A：`{"scope":"space","entries":[{"key":"pageSize","value":"20"}]}` + S1（**作用域错 + 类型错**同时成立）
  3. A：`{"scope":"tenant","entries":[{"key":"pageSize","value":"20"}]}`（仅类型错）
  4. A：`{"scope":"tenant","entries":[{"key":"Theme.radius","value":1}]}`（**键名格式错 + 未登记**同时成立）
  5. A：`{"scope":"tenant","entries":[{"key":"notRegistered","value":1},{"key":"pageSize","value":"20"}]}`（**同一请求**里未登记 + 类型错，AC-S-051 步骤 5）
- **预期结果**:
  1. **400**；`code == "CONFIG_KEY_NOT_REGISTERED"`（注册表先于作用域；**不得** `CONFIG_SCOPE_NOT_ALLOWED`）。
  2. **400**；`code == "CONFIG_SCOPE_NOT_ALLOWED"`（作用域先于类型；**不得** `CONFIG_TYPE_MISMATCH`）。
  3. **400**；`code == "CONFIG_TYPE_MISMATCH"`。
  4. **400**；`code == "CONFIG_KEY_INVALID"`（键名格式先于注册表；**不得** `CONFIG_KEY_NOT_REGISTERED`）。
  5. **400**；`code == "CONFIG_KEY_NOT_REGISTERED"`（同一批里注册表先于类型）。
  6. 五次错误码与上表逐条对应；若多条规则都命中时返回了更靠后的码，用例判失败。
- **类型**: 安全
- **优先级**: P1
- **自动化**: 可自动化
- **设计方法**: 判定顺序（多错因输入）+ 错误码严格区分

---
