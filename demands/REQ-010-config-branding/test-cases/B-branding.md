# B. 品牌公开接口与前端接入 —— 测试用例（AC-S-054、AC-S-055）

- **需求**: REQ-010「配置、品牌与 Feature Flag」（**rev 2**）
- **来源**: [acceptance.md](../acceptance.md) §AC-S-054 / AC-S-055、[design-summary.md](../design-summary.md) §4.1 / §6.1 / §8
- **用例数**: 8（TC-010-012 ~ TC-010-019）
- **公共夹具**: 见 [index.md](index.md) §3 测试夹具

## B.0 本域约定（rev 2）

- **接口**：`GET /api/config/branding`，**不需要令牌** —— 本仓第一个公开业务接口（现状只有 `/health/*` 未认证）。
- **响应形状 = 扁平 map**（D61）：`{"logo": "...", "title": "...", "primaryColor": "#111111", "theme.radius": 6, ...}`，**没有** `source` 字段、**没有** `entries` 包装。它的用途是「直接渲染」，不是配置查询。
- **白名单枚举具体键**（D66）：`logo` / `favicon` / `title` / `loginBackground` / `footer` / `copyright` / `primaryColor` / `theme.radius` / `theme.mode` / `theme.density` / `theme.fontStack`。**没有 `theme.*` 通配**。
- **租户来源**：`X-Tenant-Id` 请求头；**缺失 → 平台默认品牌**（200，不是 401/404）；未知租户同样回落平台默认（无租户注册表，§7 A11）。
- **忽略空间级**：`X-Space-Id` 不参与取值（未认证请求没有可信空间身份，design §4.1）。
- **品牌值与夹具**（§3.5）：平台 `logo=PA`、`primaryColor=#111111`；T1 `logo=PB`、`title=T1平台`；T2 `logo=PC`；T1+S1 `logo=PS`。
- **前端硬编码的真实位置**（已核实）：
  - `code/apps/shell/index.html:6` `<title>底座壳</title>`
  - `code/apps/shell/src/App.svelte:16` `<h1 ...>底座壳</h1>`、`:9` 的 `'底座壳已就绪'`
  - `code/apps/shell/src/app.css:16` `color: #1677ff;`
  - `code/e2e/shell.spec.ts:5` 断言 heading 名为 `底座壳`
- **`packages/theme` 的定位（rev 2）**：它的 `#1677ff` **允许保留为兜底默认**；AC-S-055 只要求**运行时渲染结果**不等于 `#1677ff`（步骤 4）。因此静态扫描范围是 **`apps/shell/**`**，不再要求 `packages/theme/src` 命中数为 0（这与 rev 1 的 Q-25 部分不同 —— BA 把范围收到 shell，并把「兜底默认」显式允许了）。
- **同源要求**：E2E 必须通过 `platform-server` 的静态目录（`BASE_STATIC_DIR`）访问 shell；本仓**没有 CORS 层**，跨源 fetch 会被浏览器拦。
- **执行位置**：TC-010-012 ~ 016 为真实库 + 真机 HTTP（无需令牌）；TC-010-017 为静态扫描（无库）；TC-010-018 / 019 为浏览器（Playwright，同源）。

---

### TC-010-012: 公开可读，且真的按租户解析（值断言，不只验键名）
- **关联 AC**: AC-S-054
- **前置条件**:
  - **执行位置**: 真实库 + 真机 HTTP。**不带** `Authorization`。
  - 夹具见 §3.5：平台 `logo=PA`、`primaryColor=#111111`、`title` 默认 `示例平台`；T1 `logo=PB`、`title=T1平台`；T2 `logo=PC`。
- **测试步骤**:
  1. `GET /api/config/branding` + `X-Tenant-Id: tenant-a`
  2. 同请求 + `X-Tenant-Id: tenant-b`
  3. 同请求**不带** `X-Tenant-Id`
- **预期结果**:
  1. HTTP **200**（不需要令牌，`Content-Type: application/json`）；`body.logo == "/branding/tenant-a.svg"`（**PB**）；`body.title == "T1平台"`；`body.primaryColor == "#111111"`。**不得** 401；**不得**返回 PA。
  2. HTTP **200**；`body.logo == "/branding/tenant-b.svg"`（**PC**）—— 与第 1 步不同，证明真的按租户解析，不是恒返回平台值。
  3. HTTP **200**；`body.logo == "/branding/platform-a.svg"`（**PA**）；`body.title == "示例平台"`。
  4. 三条结论：① 公开可读；② 租户级覆盖生效（T1=PB、T2=PC）；③ 缺租户头回落平台默认。
- **类型**: 功能 / 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 等价类（租户维度 × 3）+ 值断言（不是只断言键存在）

### TC-010-013: 白名单 fail closed：非白名单键**缺失**，且有权的对照读证明数据存在
- **关联 AC**: AC-S-054
- **前置条件**:
  - **执行位置**: 真实库 + 真机 HTTP。
  - 平台已配 `conflictStrategy=manual`、`scimBaseUrl=https://scim.example.com`、`cacheTtlSeconds=300`（三者都不在白名单）；空间表里另有一行 T1+S1 `conflictStrategy=remote_wins`（**管理员 SQL 合成的行** —— `scope: platform` 的键无法通过 API 写进空间级，见 §8.9 N5）。
  - 对照读用 P（`u-2003`，含 `config:platform:read`）。
- **测试步骤**:
  1. `GET /api/config/branding` + `X-Tenant-Id: tenant-a`，取出响应里**全部**键名，记为集合 K
  2. 断言 K ⊆ 白名单（11 个具体键）
  3. 断言 `conflictStrategy`、`scimBaseUrl`、`cacheTtlSeconds` 均不在 K
  4. 以 P 调 **`GET /api/config/platform-entries?keys=conflictStrategy,scimBaseUrl,cacheTtlSeconds`**（T1）
     —— ⚠️ 按 AC-S-063 / D78：平台级键走**独立读路由**；用通用路由 `GET /api/config/entries`
     会得到整请求 **403**（rev 2 时此处写的是通用路由期望 200，已过时）
  5. 断言 `logo`、`primaryColor`、`theme.radius` 在 K 中
- **预期结果**:
  1. K 逐条列出；K 至少含 `logo`、`primaryColor`、`title`、`favicon`。
  2. K 与白名单的差集为 **∅**。
  3. 三个键在 K 中**缺席**；响应**不是** 4xx/5xx，也不是把值置空 —— 是键不存在。
  4. **200**；`entries` 长度 **3**，值分别为 `manual` / `https://scim.example.com` / `300`。
  5. 三个白名单键都在 K 中 —— 白名单不是"什么都不返回"。
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 集合断言（子集 + 缺席）+ 存在性对照（防"接口永远返回空对象"）

### TC-010-014: 缺租户头 / 未知租户 / 空串头都回落平台默认
- **关联 AC**: AC-S-054
- **前置条件**:
  - **执行位置**: 真实库 + 真机 HTTP。
  - 本仓**没有租户注册表**，`tenant` 只是字符串 —— 「未知租户」无从校验（§7 A11）。
- **测试步骤**:
  1. 不带 `X-Tenant-Id`
  2. `X-Tenant-Id: tenant-does-not-exist`
  3. `X-Tenant-Id: ""`（空串）
  4. `X-Tenant-Id: tenant-b`（存在且有租户级覆盖，对照）
- **预期结果**:
  1. **200**；`logo == PA`。
  2. **200**；`logo == PA` —— 与第 1 步相同。**不得** 404/500。
  3. **200**；`logo == PA`（空串按"缺失"处理）。
  4. **200**；`logo == PC`（T2 有自己的租户值，证明第 2/3 步不是碰巧命中）。
  5. 判据说明：本用例断言的是设计取舍（未知租户回落平台默认）；若 BA 后续裁定未知租户必须 404，第 2 步改断言 **404 + 不泄露租户是否存在**。
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 边界（缺失 / 未知 / 空串）+ 存在租户对照

### TC-010-015: 忽略空间级覆盖（显式断言 ≠ 空间值）
- **关联 AC**: AC-S-054
- **前置条件**:
  - **执行位置**: 真实库 + 真机 HTTP。
  - 空间级行：T1+S1 `logo=PS`、T1+S2 `logo=D`、T2+S1 `logo=E`（§3.5）。
- **测试步骤**:
  1. `GET /api/config/branding` + `X-Tenant-Id: tenant-a` + `X-Space-Id: space-sales-1`
  2. 同请求 + `X-Tenant-Id: tenant-b` + `X-Space-Id: space-sales-1`
  3. 同请求 + `X-Tenant-Id: tenant-a` + `X-Space-Id: space-does-not-exist`
- **预期结果**:
  1. **200**；`logo == PB` —— 显式断言 **≠ PS**（空间级被忽略，且不是"没读到空间就报错"）。
  2. **200**；`logo == PC` —— 显式断言 **≠ E**（另一租户的同名空间也读不到）。
  3. **200**；`logo == PB` —— 未知空间 id 不改变结果、不报错（C3）。
  4. 结论：未校验的 `X-Space-Id` **不能**改变公开接口的取值（design §4.1 / R1）。
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 攻击面（未校验头）+ 反例优先（显式断言"不得等于空间值"）

### TC-010-016: 品牌接口与认证读接口取值一致（不是另一套实现）
- **关联 AC**: AC-S-054
- **前置条件**:
  - **执行位置**: 真实库 + 真机 HTTP。
  - 对照读能力：P 令牌（含 `config:platform:read`）。
- **测试步骤**:
  1. T1：比较 `branding.logo` 与 `entries?keys=logo` 的 `entries[0].value`
  2. T2：比较 `branding.logo` 与 `entries?keys=logo`
  3. P 改平台 `logo=PA2`，重复第 1、2 步与"无租户头"的品牌读
- **预期结果**:
  1. 两值**相等**（`/branding/tenant-a.svg`）。
  2. 两值**相等**（`/branding/tenant-b.svg`）。
  3. T1 两值仍相等（`PB`，不受平台变更影响）；T2 仍相等（`PC`）；无租户头 → `PA2`（平台变更已生效）。
  4. 三次比较都相等 —— 公开接口不是独立实现/独立缓存；取值不同即判失败。
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 双通道一致性 + 变更传播

### TC-010-017: `apps/shell/**` 无硬编码品牌值 + 既有测试同步更新
- **关联 AC**: AC-S-055
- **前置条件**:
  - **执行位置**: 静态扫描（不连库）。
  - 扫描范围**只看 `apps/shell/**`**（rev 2 收窄；`packages/theme` 的 `#1677ff` 作为兜底默认**允许保留**，由 TC-010-018 步骤 4 从**渲染结果**上把关）。
- **测试步骤**:
  1. 搜 `底座壳`：`code/apps/shell/index.html`、`code/apps/shell/src/**`
  2. 搜 `#1677ff`（及大小写/简写变体）：`code/apps/shell/**`
  3. 搜改动后的品牌值 `示例平台` / `#0055aa`：`code/apps/shell/**`
  4. 搜品牌接口调用：`api/config/branding` 在 `code/apps/shell/src/**` 出现次数
  5. 检查 `code/e2e/shell.spec.ts`：不得再断言 `底座壳`，必须断言**运行时标题**
  6. （可选）对 `pnpm --filter @base/shell build` 产物再搜第 2、3 步的值
- **预期结果**:
  1. 命中数 **0** —— `index.html:6` 与 `App.svelte:16` / `:9` 的 `底座壳` 都必须消失。
  2. 命中数 **0** —— `app.css:16` 的 `#1677ff` 必须改为从品牌配置取值。
  3. 命中数 **0** —— 品牌值不得作为字面量出现在 shell 源码（否则"改配置"只是改了另一个常量）。
  4. 命中数 **≥ 1**（正对照：证明是运行时取值，不是把品牌删掉）。
  5. `e2e/shell.spec.ts` 里 `底座壳` 命中 **0**，且存在对 `示例平台`（或运行时标题）的断言；该测试**必须通过**（"改完让测试红"也算失败）。
  6. 构建产物命中数 **0**（sourcemap 除外，需在报告注明排除规则）。
- **类型**: 功能 / 可维护性
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 禁用模式扫描 + 正对照（必须有 fetch）+ 既有测试联动检查

### TC-010-018: 改平台 `title` / `primaryColor` → 标题、`<h1>` 与主色随之变化（Playwright，同源）
- **关联 AC**: AC-S-055
- **前置条件**:
  - **执行位置**: 浏览器（Playwright chromium）；**页面与 API 同源**。
  - 现状风险：`App.svelte:8` 把 API base 写死 `http://127.0.0.1:8080`，`playwright.config.ts:9-13` 的预览端口是 `4173`，服务端**没有 CORS 层**。**本用例要求**：把 `apps/shell/dist` 挂到 `platform-server` 的静态目录（`BASE_STATIC_DIR`），用 `http://127.0.0.1:8080` 打开页面。
  - 平台值可写：P（`u-2003`）或管理员 SQL。
- **测试步骤**:
  1. 设平台 `title=示例平台`、`primaryColor=#0055aa`
  2. `page.goto('/')`，等 `networkidle`
  3. 断言 `document.title` 与页面 `<h1>` 的文本
  4. 断言 `.base-tone--primary` 的 `getComputedStyle(...).color`
  5. 改平台 `title=验证平台2`、`primaryColor=#aa0000` → `page.reload()` → 重复第 3、4 步
- **预期结果**:
  1. 第 3 步：`page.title() === "示例平台"` **且** `<h1>` 文本为 `示例平台`（两者都要变，AC-S-055 步骤 2 明写 `<h1>`）。
  2. 第 4 步：计算色为 `rgb(0, 85, 170)`（`#0055aa`）。
  3. **该计算色 ≠ `rgb(22, 119, 255)`**（即 `#1677ff`）—— AC-S-055 步骤 4：`packages/theme` 的兜底默认**不得**出现在渲染结果里。
  4. 第 5 步刷新后：标题与 `<h1>` 为 `验证平台2`，计算色 `rgb(170, 0, 0)`。
  5. 浏览器 console **无** CORS / 网络错误；`/api/config/branding` 请求状态码 **200**。
  6. 两次改动都在一次刷新内生效（不需要重新构建前端）。
- **类型**: 功能
- **优先级**: P0
- **自动化**: 半自动（Playwright 可跑，但依赖前端构建 + 同源部署）
- **设计方法**: 端到端一致性 + 变更生效 + 「渲染值 ≠ 兜底默认」的判别

### TC-010-019: 品牌接口不可用时前端降级，且不得回落到被删掉的硬编码
- **关联 AC**: AC-S-055
- **前置条件**:
  - **执行位置**: 浏览器（Playwright）。
  - 用例性质：**派生** —— AC-S-055 只要求"改值生效"，没写"取不到时怎么办"；但硬编码删掉后，降级路径就是唯一显示来源（见 [index.md](index.md) §8.9 N8/N9 的同类思路）。
- **测试步骤**:
  1. `page.route('**/api/config/branding', route => route.fulfill({ status: 500, body: '{}' }))`
  2. `page.goto('/')`，等待 `networkidle`
  3. 检查 `#app` 是否有渲染内容、`page.title()`、`<h1>` 文本、主色
  4. 检查 console 是否有明确告警（含 `branding`）
- **预期结果**:
  1. 页面**仍渲染**（`#app` 非空；不得白屏、不得未捕获异常）。
  2. `page.title()` 与 `<h1>` **不等于** `底座壳`（硬编码已删除，不得回落）；允许空串或某个**非品牌的中性值**。
  3. 主色不等于 `#0055aa`（配置没取到），也**不等于** `#1677ff`（不得显示成"看起来像配好了"的兜底品牌色）；应为中性 CSS 兜底。
  4. console 至少 **1 条**明确告警，内容含 `branding`。
  5. 恢复路由后刷新，标题/主色恢复正常（降级不是永久性的）。
- **类型**: 安全
- **优先级**: P1
- **自动化**: 半自动
- **设计方法**: 异常路径（依赖不可用）+ fail closed（不泄露旧品牌、不伪装成默认品牌）

---
