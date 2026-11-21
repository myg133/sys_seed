# 04 导航下发 / 前端运行时路由（AC-S-162、AC-S-163）

### TC-016-042 有权限的账号：module 项 + app 项都在
- **AC**: AC-S-162 步骤 1 ｜ **类型**: 功能/安全 ｜ P0 ｜ 集成 ｜ 等价类
- **前置**: `u-perm-audit` 持 `audit:events:read`；`tenant-a` 有绑定 `client-A`、`launch_url="https://expense.example/app"` 的 active 应用
- **步骤**: `GET /api/apps/navigation`（Bearer `u-perm-audit`）
- **预期**: `items` 里**同时**存在 `source=="module" && key` 指向 audit 的项 **与** `source=="app"` 的项

### TC-016-043 无 audit 权限的账号：audit 项消失、app 项仍在
- **AC**: AC-S-162 步骤 2 ｜ P0 ｜ 集成 ｜ 对照
- **步骤**: 同一端点，Bearer `u-perm-read`（**不持** `audit:events:read`）
- **预期**: `source=="module"` 的集合里**不含** audit 项（`target=="/admin/audit"` 不存在）；`source=="app"` 的项**仍在**

### TC-016-044 kind / target 形状
- **AC**: AC-S-162 步骤 3 ｜ P0 ｜ 集成 ｜ 等价类
- **预期**: 每个 `module` 项 `kind=="internal"` 且 `target` 以 `/` 开头；每个 `app` 项 `kind=="external"` 且 `target` 以 `https://` 开头

### TC-016-045 app 项的 target 逐字等于 launch_url
- **AC**: AC-S-162 步骤 4 ｜ P0 ｜ 集成 ｜ 等价类
- **预期**: 该 app 项 `target == "https://expense.example/app"`（**相等**，不是「包含」）

### TC-016-046 launch_url 置空 → 该应用不出现〔AC 修正〕
- **AC**: AC-S-162 步骤 5 ｜ P0 ｜ 集成 ｜ 状态迁移
- **步骤**: 按 BA 拍板的清空手段（`PATCH {"launchUrl":null}` + `Option<Option<String>>` / 专用端点）把 `launch_url` 置 NULL → 再 `GET /api/apps/navigation`
- **预期**: 该应用**不在** `items` 里；其它项不变
- **〔AC 修正〕**: 原文没写清空手段，`Option<String>` 下 `null ≡ 缺省` → 步骤**执行不了**（S-08）

### TC-016-047 `perm` 缺省的 module 项总是下发〔AC 补充〕
- **AC**: D269「`menus[].perm` 为 `null`（缺省）→ 总是下发」｜ P1 ｜ 集成 ｜ 判定表
- **步骤**: 用**无任何权限点**的账号（`u-perm-none`）调 `GET /api/apps/navigation`
- **预期**: `platform-app` 自己的「工作台」项（`route=="/workbench"`）**仍出现**在 `items` 里
- **〔AC 补充〕**: AC 原文只验了「有 perm 的被过滤」，**没验**这条规则

### TC-016-048 routesFromNavigation：2 个 internal → 2 条路由、path 逐字等于 target
- **AC**: AC-S-163 步骤 2 ｜ 功能 ｜ P0 ｜ **前端单测** ｜ 等价类
- **步骤**: `routesFromNavigation([{kind:'internal',target:'/a',labelKey:'nav.a',…},{kind:'internal',target:'/b',labelKey:'nav.b',…}])`
- **预期**: 返回长度 **2**；`[0].path=='/a'`、`[1].path=='/b'`；`title` 为对应 `labelKey`

### TC-016-049 external 项不产生路由
- **AC**: AC-S-163 步骤 3 ｜ P0 ｜ 前端单测 ｜ 边界值
- **预期**: `routesFromNavigation([{kind:'external',target:'https://x.example/'}])` → `[]`（**空数组**）

### TC-016-050 router.ts 零改动（钉基线）〔AC 修正〕
- **AC**: AC-S-163 步骤 1 ｜ 可维护性 ｜ P0 ｜ **命令** ｜ 静态
- **步骤**: `git diff --stat aa5b305..HEAD -- apps/shell/src/lib/router.ts`
- **预期**: 输出**为空**（退出码 0 且无行）
- **〔AC 修正〕**: 原文「对比本需求前后」没有基线 ref，CI 不可执行（S-21）
- **⚠️ 前置**: 这条能否成立取决于 **S-03** 怎么解决 —— 若 BA 选「允许改 router.ts」，本条改为
  「`router.ts` 的**既有语义**不变：`createRouter` 的 4 个既有参数与 `resolve()` 行为不变」（并用 TC-016-048/049 兜住）

### TC-016-051 E2E：internal 项点击落到对应页面，标题是翻译后的文案〔AC 修正〕
- **AC**: AC-S-163 步骤 4 ｜ P0 ｜ E2E ｜ 场景
- **前置**: `apps/shell/dist` 已构建；导航里有一个**只由导航产生**的 internal 项（不能用 `/admin/audit` —— 它本来就在 `routes.ts:143`）
- **步骤**: 登录 → 打开工作台 → 点击该导航项
- **预期**: `page.url()` 的 pathname == 该项 `target`；`document.title` == `t(labelKey)` 的**译文**（中文环境为「审计日志」这类）
- **〔AC 修正〕**: ① 原文「标题是 labelKey」歧义（裸键 vs 译文，S-22）；② 必须用**新**路径才有区分力

### TC-016-052 E2E：external 项整页跳转
- **AC**: AC-S-163 步骤 5 ｜ P0 ｜ E2E ｜ 场景
- **步骤**: `page.route('https://expense.example/**', route => route.fulfill({status:200, body:'stub'}))` → 点 app 项
- **预期**: `page.url()` == `https://expense.example/app`；且**没有**触发 SPA 内部导航（`popstate`/`pushState` 未发生，URL 是整页替换）
