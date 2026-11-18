# B. 品牌公开接口与前端接入 —— 测试用例（AC-S-054、AC-S-055）

- **需求**: REQ-010「配置、品牌与 Feature Flag」
- **来源**: [acceptance.md](../acceptance.md) §AC-S-054 / AC-S-055、[design-summary.md](../design-summary.md) §4.1 / §6.1 / §8、[api-conventions v2 §C9](../../../contracts/api-conventions/v2.md)
- **用例数**: 8（TC-010-012 ~ TC-010-019）
- **公共夹具**: 见 [index.md](index.md) §3 测试夹具

## B.0 本域约定

- **接口**：`GET /api/config/branding`，**不需要令牌**（design §6）—— 这是本仓第一个公开业务接口（现状只有 `/health/*` 未认证，见 `code/crates/platform-server/src/main.rs:292-296`）。
- **响应形状**（design 未写死，本用例集按 §7 A5 取值）：**扁平对象** `{"logo": "...", "title": "...", ...}`，无 `key`/`value`/`source` 包装。若实现选择 C9 的条目数组，TC-010-012 ~ 016 的取值断言需整体改写成 `entries[].value`（见 [index.md](index.md) §8 Q-11）。
- **白名单**（design §6.1）：`logo` / `favicon` / `title` / `loginBackground` / `footer` / `copyright` / `primaryColor` / `theme.*`。
- **租户来源**：`X-Tenant-Id` 请求头。**缺失 → 平台默认品牌**（200，不是 401/404）。
- **忽略空间级**：`X-Space-Id` 不参与取值（未认证请求没有可信空间身份，design §4.1）。
- **前端硬编码的真实位置**（已核实）：
  - `code/apps/shell/index.html:6` `<title>底座壳</title>`
  - `code/apps/shell/src/App.svelte:16` `<h1 ...>底座壳</h1>`
  - `code/apps/shell/src/App.svelte:9` `messages = { SHELL_READY: '底座壳已就绪', ... }`
  - `code/apps/shell/src/app.css:16` `color: #1677ff;`
  - `code/packages/theme/src/index.ts:21` `primaryColor: '#1677ff'`
  - AC-S-055 只写了 `index.html` 与 `apps/shell/src/**`，**漏掉 `packages/theme`**（见 [index.md](index.md) §8 Q-25）。
- **执行位置**：TC-010-012 ~ 016 为真实库 + 真机 HTTP（无需令牌）；TC-010-017 为静态扫描（无库）；TC-010-018 / 019 为浏览器（Playwright），**必须同源**（见 §8 Q-27）。

---

### TC-010-012: 公开可读，且租户级取值真的生效
- **关联 AC**: AC-S-054
- **前置条件**:
  - **执行位置**: 真实库（`base_test`）+ 真机 HTTP。**不带** `Authorization`。
  - 夹具见 [index.md](index.md) §3.5：平台 `logo`=A、`title` 未配（默认 `示例平台`）、`primaryColor` 平台显式 `#2f54eb`；T1 租户级 `logo`=B；T2 租户级 `title`=`另一公司`。
- **测试步骤**:
  1. `GET /api/config/branding` + `X-Tenant-Id: tenant-a`
  2. 同请求 + `X-Tenant-Id: tenant-b`
  3. 同请求**不带** `X-Tenant-Id`
- **预期结果**:
  1. HTTP **200**（**不需要令牌**，响应 `Content-Type: application/json`）；`body.logo == "/branding/tenant-a.svg"`（B）；`body.title == "示例平台"`；`body.primaryColor == "#2f54eb"`。**不得** 401。
  2. HTTP **200**；`body.logo == "/branding/platform-a.svg"`（A，T2 无租户覆盖）；`body.title == "另一公司"`（T2 租户级）。
  3. HTTP **200**；`body.logo == "/branding/platform-a.svg"`（A）；`body.title == "示例平台"`。
  4. 三条结论：① 公开可读；② 租户级覆盖生效（`logo` 在 T1 是 B，不是 A）；③ 缺租户头回落平台默认（不是报错）。
- **类型**: 功能 / 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 等价类（租户维度）+ 值断言（不是只断言"键存在"）

### TC-010-013: 白名单 fail closed：非白名单键**缺失**（不是报错），且有权的对照读证明数据存在
- **关联 AC**: AC-S-054
- **前置条件**:
  - **执行位置**: 真实库 + 真机 HTTP。
  - 平台已配 `conflictStrategy`=`manual`、`scimBaseUrl`=`https://scim.example.com`、`cacheTtlSeconds`=300（三者都**不在**白名单）。
  - 对照读用 P（`u-2003`）的令牌调 `GET /api/config/entries`。
- **测试步骤**:
  1. `GET /api/config/branding` + `X-Tenant-Id: tenant-a`，取出响应里**全部**键名，记为集合 K
  2. 断言 K ⊆ 白名单集合
  3. 断言 `conflictStrategy`、`scimBaseUrl`、`cacheTtlSeconds` **均不在** K
  4. 以 P 调 `GET /api/config/entries?keys=conflictStrategy,scimBaseUrl,cacheTtlSeconds`（T1）
  5. 断言 `logo`、`primaryColor` **在** K 中
- **预期结果**:
  1. K 逐条列出；K 至少含 `logo`、`primaryColor`、`title`、`favicon`。
  2. K 与白名单的差集为 **∅**（没有任何非白名单键）。
  3. 三个键在 K 中**缺席**；响应**不是** 4xx/5xx，也不是把值置空 —— 是键不存在。
  4. **200**；`entries` 长度 **3**，值分别为 `manual` / `https://scim.example.com` / `300`。
  5. `logo` 与 `primaryColor` 都在 K 中 —— 白名单不是"什么都不返回"。
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 集合断言（子集 + 缺席）+ 存在性对照（防"接口永远返回空对象"）

### TC-010-014: 缺租户头 / 未知租户 / 空串头都回落平台默认（记录取舍）
- **关联 AC**: AC-S-054
- **前置条件**:
  - **执行位置**: 真实库 + 真机 HTTP。
  - 本仓**没有租户注册表**（全仓没有 `platform_tenant` 表），`tenant` 只是字符串 —— 因此「未知租户」无从校验（见 [index.md](index.md) §7 A11）。
- **测试步骤**:
  1. 不带 `X-Tenant-Id`
  2. `X-Tenant-Id: tenant-does-not-exist`
  3. `X-Tenant-Id: ""`（空串）
  4. `X-Tenant-Id: tenant-b`（存在的租户，对照）
- **预期结果**:
  1. **200**；`logo == "/branding/platform-a.svg"`（A）。
  2. **200**；`logo == A` —— 与第 1 步**相同**（未知租户 = 无租户覆盖）。**不得** 404/500。
  3. **200**；`logo == A`（空串按"缺失"处理）。
  4. **200**；`logo == A`、`title == "另一公司"`（T2 有自己的租户级 title，证明第 2/3 步不是碰巧命中）。
  5. 判据说明：本用例断言的是**设计取舍**（未知租户回落平台默认）；若 BA 裁定未知租户必须 404，则第 2 步改断言 **404 + 不泄露租户是否存在**，其余不变。
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 边界（缺失 / 未知 / 空串）+ 存在租户对照

### TC-010-015: 忽略空间级覆盖（修正 AC-S-054 步骤 3 缺租户头的问题）
- **关联 AC**: AC-S-054
- **前置条件**:
  - **执行位置**: 真实库 + 真机 HTTP。
  - 空间级行：T1+S1 `logo`=C、T1+S2 `logo`=D、T2+S1 `logo`=E（见 §3.5）。
  - **不经令牌**：`X-Tenant-Id` 是唯一租户来源。
- **测试步骤**:
  1. `GET /api/config/branding` + `X-Tenant-Id: tenant-a` + `X-Space-Id: space-sales-1`
  2. 同请求 + `X-Tenant-Id: tenant-b` + `X-Space-Id: space-sales-1`
  3. 同请求 + `X-Tenant-Id: tenant-a` + `X-Space-Id: space-does-not-exist`
- **预期结果**:
  1. **200**；`logo == "/branding/tenant-a.svg"`（**B**）—— 显式断言 **≠ C**（空间级被忽略，且不是"没读到空间就报错"）。
  2. **200**；`logo == "/branding/platform-a.svg"`（**A**）—— 显式断言 **≠ E**（另一租户的同名空间也不能被读到）。
  3. **200**；`logo == B` —— 未知空间 id 不改变结果、不报错（C3：`X-Space-Id` 缺失/无效都不报错）。
  4. 结论：未校验的 `X-Space-Id` **不能**改变公开接口的取值（design §4.1 / R1）。
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 攻击面（未校验头）+ 反例优先（显式断言"不得等于空间值"）

### TC-010-016: 品牌接口与认证读接口取值一致（不是另一套硬编码）
- **关联 AC**: AC-S-054
- **前置条件**:
  - **执行位置**: 真实库 + 真机 HTTP。
  - 同 TC-010-013 的对照读能力（P 令牌）。
- **测试步骤**:
  1. T1：比较 `branding.logo` 与 `entries?keys=logo` 的 `entries[0].value`
  2. T2：比较 `branding.logo` 与 `entries?keys=logo`
  3. P 改平台 `logo` = `/branding/platform-a2.svg`，重复第 1、2 步
- **预期结果**:
  1. 两值**相等**（`/branding/tenant-a.svg`）。
  2. 两值**相等**（`/branding/platform-a.svg`）。
  3. T1 两值仍相等（`/branding/tenant-a.svg`，未受平台变更影响）；T2 两值都变成 `/branding/platform-a2.svg`。
  4. 三次比较都相等 —— 公开接口不是独立实现/独立缓存；若两处取值不同即判失败。
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 双通道一致性 + 变更传播

### TC-010-017: 前端源码无硬编码品牌值（静态扫描，覆盖真实硬编码位置）
- **关联 AC**: AC-S-055
- **前置条件**:
  - **执行位置**: 静态扫描（不连库）。
  - 品牌值基线：manifest 默认 `title`=`示例平台`（§3.4）；`primaryColor` 在用例中设为 `#0055aa`（AC-S-055 的取值）。**注意**：`#1677ff` 是**当前**硬编码值，`#0055aa` 是**改动后**要出现的值；两者在源码里的判定不同（前者必须消失，后者不得作为常量出现）。
- **测试步骤**:
  1. 搜 `底座壳`：`code/apps/shell/index.html`、`code/apps/shell/src/**`、`code/e2e/shell.spec.ts`
  2. 搜 `#1677ff`：`code/apps/shell/**`、`code/packages/theme/src/**`
  3. 搜改动后的品牌值 `示例平台` / `#0055aa`：`code/apps/**`、`code/packages/**`
  4. 搜品牌接口调用：`api/config/branding` 在 `code/apps/shell/src/**` 出现次数
  5. （可选）对 `pnpm --filter @base/shell build` 产物再搜第 2、3 步的值
- **预期结果**:
  1. 命中数 **0** —— `index.html:6` 与 `App.svelte:16` / `App.svelte:9` 的 `底座壳` 都必须消失；`e2e/shell.spec.ts:5` 的 `getByRole('heading', { name: '底座壳' })` 必须改成运行时标题断言。
  2. 命中数 **0** —— `app.css:16` 与 `packages/theme/src/index.ts:21` 都必须改为从品牌配置取值（或删除常量）。
  3. 命中数 **0** —— 品牌值不得作为字面量出现在前端源码（否则"改配置"只是改了另一个常量）。
  4. 命中数 **≥ 1**（正对照：证明是运行时取值，不是把品牌删掉）。
  5. 构建产物命中数 **0**（sourcemap 除外，需在报告中注明排除规则）。
- **类型**: 功能 / 可维护性
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 禁用模式扫描 + 正对照（必须存在 fetch）+ 构建产物复核

### TC-010-018: 改平台 `title` / `primaryColor` → 浏览器标题与主色随之变化（Playwright，同源）
- **关联 AC**: AC-S-055
- **前置条件**:
  - **执行位置**: 浏览器（Playwright chromium）；**页面与 API 同源**。
  - 现状风险：`App.svelte:8` 把 API base 写死成 `http://127.0.0.1:8080`，而 `playwright.config.ts:9-13` 的预览端口是 `4173`，且服务端**没有 CORS 层** → 跨源 fetch 会被浏览器拦（见 [index.md](index.md) §8 Q-27）。**本用例要求**：把 `apps/shell/dist` 挂到 `platform-server` 的静态目录（`BASE_STATIC_DIR`）、用 `http://127.0.0.1:8080` 打开页面（同源），或先补 CORS。
  - 平台值可写：P（`u-2003`）或管理员 SQL。
- **测试步骤**:
  1. 设平台 `title`=`示例平台`、`primaryColor`=`#0055aa`
  2. `page.goto('/')`，等待 `networkidle`
  3. 断言 `await page.title()`
  4. 断言 `.base-tone--primary` 的 `getComputedStyle(...).color`
  5. 改平台 `title`=`验证平台2`、`primaryColor`=`#aa0000` → `page.reload()` → 重复第 3、4 步
- **预期结果**:
  1. 第 3 步 `page.title() === "示例平台"`（**精确相等**）。
  2. 第 4 步计算色为 `rgb(0, 85, 170)`（`#0055aa` 的 RGB）。
  3. 第 5 步刷新后：标题为 `验证平台2`，计算色为 `rgb(170, 0, 0)`。
  4. 浏览器 console **无** CORS / 网络错误；页面发出的 `/api/config/branding` 请求状态码 **200**。
  5. 两次改动都在一次刷新内生效（不需要重新构建前端）。
- **类型**: 功能
- **优先级**: P0
- **自动化**: 半自动（Playwright 可跑，但依赖前端构建 + 同源部署）
- **设计方法**: 端到端一致性 + 变更生效（端到端）

### TC-010-019: 品牌接口不可用时前端降级，且不得回落到被删掉的硬编码
- **关联 AC**: AC-S-055
- **前置条件**:
  - **执行位置**: 浏览器（Playwright）。
  - 用例性质：**派生** —— AC-S-055 只要求"改值生效"，没写"取不到时怎么办"；但一旦删掉硬编码，降级路径就是唯一的显示来源（见 [index.md](index.md) §8 Q-25 / Q-27）。
- **测试步骤**:
  1. `page.route('**/api/config/branding', route => route.fulfill({ status: 500, body: '{}' }))`
  2. `page.goto('/')`，等待 `networkidle`
  3. 检查 `#app` 是否有渲染内容、`page.title()`、主色
  4. 检查 console 是否有明确告警（含 `branding`）
- **预期结果**:
  1. 页面**仍渲染**（`#app` 非空；不得白屏、不得抛出未捕获异常）。
  2. `page.title()` **不等于** `底座壳`（硬编码已删除，不得回落）；标题为空串或某个**非品牌的中性值**（用例只断言"不是被删掉的硬编码值"，不锁定具体兜底文案）。
  3. 主色为 CSS 兜底值（不是 `#1677ff`）。
  4. console 至少有 **1 条**明确告警，内容含 `branding`（便于发现"品牌没生效"）。
  5. 恢复路由后刷新，标题与主色恢复正常（证明降级不是永久性的）。
- **类型**: 安全
- **优先级**: P1
- **自动化**: 半自动
- **设计方法**: 异常路径（依赖不可用）+ fail closed（不泄露旧品牌）

---
