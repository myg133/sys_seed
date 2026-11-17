# C. 配置与品牌 —— 测试用例（AC-013 ~ AC-015）

- **需求**: REQ-001 通用项目底座（Base Platform）
- **来源**: [acceptance.md](../acceptance.md) §C、[demand.md](../demand.md) §C
- **用例数**: 6（TC-C-001 ~ TC-C-006）
- **公共夹具**: 见 [index.md](index.md) §3 测试夹具

## C.0 本域约定

- 配置键名沿用 camelCase（与 design-summary.md `manifest.json` 的 `config.itemsPerPage` 一致）：`logo`、`primaryColor`、`theme.radius`、`theme.font`、`theme.density`、`theme.mode`。
- 配置读取接口：`GET /api/config/{key}`，返回 `{key, value, scope, source}`；`scope` ∈ `platform|tenant|space`，`source` 表示实际命中层。
- 作用域上下文由请求头决定：`X-Tenant-Id`（租户层）、`X-Space-Id`（空间层）。
- 品牌读取接口：`GET /api/tenant/{tenant_id}/brand`；Feature Flag 接口：`GET /api/feature-flags/{key}`，返回 `{key, enabled, rollout}`。
- 配置优先级：**空间 > 租户 > 平台**（AC-013 明确规定）。

---

### TC-C-001: 平台 / 租户 / 空间三层配置依次命中
- **关联 AC**: AC-013
- **前置条件**:
  - 平台默认：`logo = "A"`（值可以是任一可辨识字符串/URL）。
  - 租户 T1 覆盖：`logo = "B"`。
  - 空间 S1（属 T1）覆盖：`logo = "C"`。
  - 三个层级配置均已写入且缓存已预热。
- **测试步骤**:
  1. 请求 `GET /api/config/logo`，仅带 `X-Tenant-Id: T1`（不带头 `X-Space-Id`）。
  2. 请求同一接口，带 `X-Tenant-Id: T1` 与 `X-Space-Id: S1`。
  3. 请求同一接口，不带任何租户/空间头（平台级）。
- **预期结果**:
  1. 第 1 步 HTTP 200，`value = "B"`，`scope = "tenant"`，`source = "tenant"`。
  2. 第 2 步 HTTP 200，`value = "C"`，`scope = "space"`，`source = "space"`。
  3. 第 3 步 HTTP 200，`value = "A"`，`scope = "platform"`，`source = "platform"`。
  4. 三次结果依次为 A / B / C，优先级满足 **空间 > 租户 > 平台**。
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 层级状态 + 优先级判定表

### TC-C-002: 未覆盖的上层回落、配置变更后缓存失效生效
- **关联 AC**: AC-013
- **前置条件**:
  - 平台默认 `primaryColor = "#1976d2"`；T1 **未**覆盖 `primaryColor`；空间 S2（属 T1）覆盖 `primaryColor = "#c62828"`。
  - 租户 T1 配置缓存已预热。
- **测试步骤**:
  1. 请求 `GET /api/config/primaryColor`（带 `X-Tenant-Id: T1`，不带空间头）。
  2. 请求 `GET /api/config/primaryColor`（带 `X-Tenant-Id: T1` + `X-Space-Id: S2`）。
  3. 管理员在 T1 层新增覆盖 `primaryColor = "#2e7d32"`，等待缓存 TTL 或触发显式失效后，重复步骤 1。
- **预期结果**:
  1. 第 1 步 `value = "#1976d2"`（租户未覆盖时回落平台默认），`source = "platform"`。
  2. 第 2 步 `value = "#c62828"`，`source = "space"`。
  3. 第 3 步 `value = "#2e7d32"`，`source = "tenant"`（新值在约定失效时限内可见；不得长期返回旧缓存值）。
  4. 配置写入操作本身产生审计记录（键名、层级、变更前后值）。
- **类型**: 功能
- **优先级**: P1
- **自动化**: 可自动化
- **设计方法**: 边界（缺失层回落）+ 缓存失效

### TC-C-003: 品牌配置前端实时生效，主题令牌同步更新
- **关联 AC**: AC-014
- **前置条件**:
  - 管理员 `owner@example.com` 已在后台把 T1 的 `logo` 改为 `https://cdn.test/logo-t1.png`、`primaryColor` 改为 `#0f766e`。
- **测试步骤**:
  1. 以 `owner@example.com` 登录 T1，刷新前端页面（硬刷新，忽略本地缓存）。
  2. 检查页面 logo 元素、`<html>`/`:root` 上的主题变量与主按钮颜色。
  3. 打开品牌配置接口 `GET /api/tenant/T1/brand`，与页面实际渲染值对比。
- **预期结果**:
  1. 页面 logo 的 `src` 恰为 `https://cdn.test/logo-t1.png`，且不是平台默认 logo。
  2. 主色令牌（如 `--color-primary`）计算值为 `#0f766e`，主按钮/链接颜色随之变化。
  3. 品牌接口返回的 `logo` 与 `primaryColor` 与页面渲染值**完全一致**，无第二套硬编码值。
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 端到端一致性断言

### TC-C-004: 前端不存在硬编码品牌样式
- **关联 AC**: AC-014
- **前置条件**:
  - 仓库前端源码可检；已建立品牌基线值（平台默认 logo 与主色）用于扫描。
- **测试步骤**:
  1. 在前端源码（`apps/`、`packages/`）中检索硬编码的 logo 路径、主色十六进制值与品牌文案（公司名/版权）。
  2. 检查组件样式写法：品牌相关取值是否来自主题令牌变量。
  3. 在前端构建产物中再次检索同一批硬编码值。
- **预期结果**:
  1. 源码中除「平台默认值定义处」外，不存在硬编码 logo URL / 主色色值；命中数 = 0（允许的默认值定义处 ≤ 1 处）。
  2. 组件样式使用 CSS 变量 / 主题令牌引用，而非字面量色值。
  3. 构建产物中同样无硬编码品牌值（除平台默认值定义处）。
- **类型**: 可维护性
- **优先级**: P1
- **自动化**: 可自动化
- **设计方法**: 静态检查（禁用模式扫描）

### TC-C-005: Feature Flag 按租户生效
- **关联 AC**: AC-015
- **前置条件**:
  - 功能开关 `new-invoice` 对 T1 为 `enabled = true`，对 T2 为 `enabled = false`。
  - `/order/invoice/new` 页面由该开关控制。
- **测试步骤**:
  1. 以 T1 身份请求 `GET /api/feature-flags/new-invoice` 并打开 `/order/invoice/new` 页面。
  2. 以 T2 身份（`t2user@example.com`）请求同一接口并打开同一页面 URL。
- **预期结果**:
  1. T1：接口 HTTP 200，`enabled = true`；页面正常渲染新建发票入口与表单。
  2. T2：接口 HTTP 200，`enabled = false`（或 HTTP 404/403 且 `code` 明确表示功能未开启，须二选一并全局统一）；页面不渲染该入口，直接访问 URL 被拦截并提示功能未开启。
  3. 同一开关在两租户互不影响：修改 T1 的开关状态不改变 T2 的取值。
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 等价类（租户维度）+ 判定表

### TC-C-006: 未定义 / 灰度比例为 0% 与 100% 的边界行为
- **关联 AC**: AC-015
- **前置条件**:
  - 租户 T1 存在开关 `beta-report`（灰度比例配置为 `rollout = 0`），另存在未定义开关 `not-exist-flag`。
  - 测试支持按用户 ID 稳定哈希分桶。
- **测试步骤**:
  1. 以 T1 身份请求 `GET /api/feature-flags/beta-report`，并访问其受控页面。
  2. 以 T1 身份请求 `GET /api/feature-flags/not-exist-flag`。
  3. 把 `beta-report` 的 `rollout` 改为 100 后重复步骤 1。
- **预期结果**:
  1. `rollout = 0` 时 `enabled = false`，受控入口不渲染（默认关闭，不得默认开启）。
  2. 未定义开关返回 HTTP 404，`code` 明确为「开关不存在」，**不得**返回 `enabled = true`。
  3. `rollout = 100` 时 `enabled = true`，入口渲染且接口可用。
  4. 三个取值行为稳定可重复（同账号多次请求结果一致）。
- **类型**: 功能 / 边界
- **优先级**: P1
- **自动化**: 可自动化
- **设计方法**: 边界值（0% / 100% / 未定义）
