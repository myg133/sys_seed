# 验收标准：配置、品牌与 Feature Flag

- **需求编号**: REQ-010
- **父需求**: REQ-001
- **状态**: 草稿（2026-11-18 23:10 经 QA 用例评审修订，**rev 2**）
- **设计摘要**: [design-summary.md](design-summary.md)
- **更新时间**: 2026-11-18 23:10

> 父需求的完整验收标准见 [REQ-001 acceptance.md](../REQ-001-platform-foundation/acceptance.md)。
> 本文件只列出本子需求承担的部分。

## 编号说明与修订记录

- 编号段：**`AC-S-049` ~ `AC-S-063`**（17 条，全局唯一段，规则见 D23）
- 原占位 `AC-S-015` 已被 `AC-S-052` 取代
- **rev 2** 经 QA 用例评审挑出 **36 条**问题（12 条硬伤），逐条裁定并修订。
  逐条裁定结果见 [test-cases/index.md](test-cases/index.md) §8 与决策 **D55~D68**。

**rev 2 的关键修订**（都是 rev 1 的错）：

| 原问题 | 修订 |
|---|---|
| rev 1 用 `SELECT max(epoch)` 做失效判断 | **错**：租户 epoch 高于平台 epoch 时，平台级写入不改变 `max` → 不失效。改为**成对比较**两个 epoch（D55） |
| rev 1 要求「租户连接写不进平台表」+「运行时写得进平台配置」 | **互斥**：本仓只有一个 `base_app` 池，且 `ALTER DEFAULT PRIVILEGES` 已把新表写权开给它。改为 **REVOKE 直接 DML + SECURITY DEFINER 函数写**（D56） |
| rev 1 让 flag 存 `type: boolean`，又要求 `rolloutPercent` | **无处安放**。改为**派生键** `flags.<flag>.rolloutPercent`（D57） |
| rev 1 说「跑平台的契约测试」验 C-5 | **该入口不存在**（`ContractFixture` 要求 `Repository`+`PlatformModule`）。改为新增不依赖 fixture 的扫描入口（D58） |
| rev 1 一条 `PUT` 路由同时管租户级与平台级写 | **静态 `route_layer` 做不到**。拆成两条路由（D59） |
| rev 1 给 `ConfigScope` 加 `default` 变体 | **会开出第 4 个可写层**。新增独立 `ConfigSource` 枚举（D60） |
| rev 1 品牌接口响应形状未定义 | 未定义就不可判。定为**扁平 map**（D61） |
| rev 1 未定义 400 的稳定错误码 | 定 5 个码（D62） |
| rev 1 的 055 只扫 `apps/shell` | 漏掉 `packages/theme` 与既有测试；且开发形态跨源跑不通。改为同源 + 补测试同步（D63） |
| rev 1 未定校验顺序 | 定死 6 步顺序（D64） |

---

## 承接的父 AC

父 AC：**AC-013**（配置三层覆盖）、**AC-014**（品牌配置前端实时生效）、**AC-015**（Feature Flag 按租户生效）

> ⚠️ **AC-014 在本期收窄**（D52）：原意含「主题令牌体系」，属 REQ-011。
> 本期承担到 **「shell 从接口读品牌并应用，源码里没有硬编码的品牌值」**（AC-S-055）。

---

## 本需求自有 AC

### AC-S-049: 三层覆盖与 `source` 如实反映命中层级
- **前置条件**: 平台级 `logo` = `A`；租户 T1 覆盖为 `B`；T1 下空间 S1 覆盖为 `C`；
  `title` **只登记了 manifest 默认值**、三层都没配
- **测试步骤**:
  1. 读 `logo`（**平台上下文，无租户**）→ 断言 `source`
  2. 以 T1 读 `logo`
  3. 以 T1 + `X-Space-Id: S1` 读 `logo`
  4. 以 T1 读 `title`
  5. 读一个 manifest 里**完全没登记**的键
  6. 一次批量读 `keys=logo,notRegistered`（**混合已登记与未登记**）
- **预期结果**:
  - 1 → `value=A`，`source="platform"`
  - 2 → `value=B`，`source="tenant"`
  - 3 → `value=C`，`source="space"`
  - 4 → `value=<manifest default>`，`source="default"`
  - 5 → **404**，`code=CONFIG_KEY_NOT_REGISTERED`（**404 不带 `source`**）
  - 6 → **整请求 404**（fail closed，与单键语义一致），
    `params.unknownKeys` 列出未登记的键名；**不返回** `logo` 的部分结果
  - `source` 共 **4 个取值**（`platform` / `tenant` / `space` / `default`）——
    **不是 5 个**：第 5 步是 404，没有 `source`
  - ⚠️ 第 1 步在 **HTTP 层不可达**（读接口需要令牌，令牌必带租户）。
    该步在**单元层**用 `RequestContext::new()` 验证；HTTP 层对同一请求断言 **401**
- **类型**: 功能

### AC-S-050: 作用域越级写入被拒
- **前置条件**: `conflictStrategy` 登记 `scope: platform`；`logo` 登记 `scope: space`；
  `pageSize` 登记 `scope: tenant`
- **测试步骤**:
  1. 以 T1 写 `conflictStrategy`（租户级）
  2. 以 T1 + S1 写 `conflictStrategy`（空间级）
  3. 以 T1 写 `pageSize`（租户级）
  4. 以 T1 + S1 写 `pageSize`（空间级）
  5. 以 T1 写 `logo`（租户级）
  6. 以 T1 + S1 写 `logo`（空间级）
- **预期结果**:
  - 1、2、4 → **400**，`code=CONFIG_SCOPE_NOT_ALLOWED`，
    `params` 给出该键**允许的作用域**与实际尝试的作用域
  - 3、5、6 → 成功
  - `scope` 语义 = 「**允许覆盖到的最深层**」：`platform` 不许租户/空间写；
    `tenant` 不许空间写；`space` 三层都可写
  - **空间级写还要求键在品牌白名单内**（D66）：对一个「登记为 `space` 但不在白名单」的键，
    空间级写 → **400**（fail closed），不是「写进去后被忽略」
- **类型**: 安全

### AC-S-051: 未声明的配置键不得写入，且按登记类型校验
- **前置条件**: `pageSize` 登记 `type: number` / `scope: tenant`（**用租户作用域的键**，
  避免「越级写」把类型校验顶包）
- **测试步骤**（**全部在租户级**）:
  1. 写一个 manifest 里**完全没登记**的键
  2. 给 `pageSize` 写字符串 `"20"`（数字的字符串形式）
  3. 给 `pageSize` 写布尔 `true`
  4. 给 `pageSize` 写数字 `20`
  5. 同一请求里同时违反「未登记键」与「类型错」，观察返回哪个错误码
- **预期结果**:
  - 1 → **400**，`code=CONFIG_KEY_NOT_REGISTERED`
  - 2、3 → **400**，`code=CONFIG_TYPE_MISMATCH`，`params` 给出**期望类型与实际类型**
  - 4 → 成功
  - 5 → `CONFIG_KEY_NOT_REGISTERED`（注册表检查**先于**类型检查，见下）
  - 类型判定基于 **JSON 类型**：`"20"` **不等于** `20`；布尔**不是**数字
  - **校验顺序写死**（D64）：`认证 → 权限点 → 键名格式 → 注册表 → 作用域 → 类型`。
    第 5 步证明顺序不是随口说的
- **类型**: 功能 / 安全

### AC-S-052: 配置变更在**下一个请求**生效（原 AC-S-015 写实版）
- **前置条件**: 平台级 `logo` = `A` 已被读过一次（缓存已填充）；缓存 TTL 设为 **300s**
- **测试步骤**:
  1. 读 `logo` 确认是 `A`
  2. 把平台级 `logo` 改为 `A2`（**不重启、不清缓存、不等待**）
  3. **立刻**再读 `logo`（间隔 < 1s）
  4. 用**两个独立的解析器实例**（各自持缓存、共享同一 DB）重复 1~3
- **预期结果**:
  - 3 → `value=A2` —— **不得**出现「改完仍返回旧值」
  - **生效上界 = 下一个请求**（可判定：第 3 步在 1 秒内完成且返回新值）
  - 4 → 实例 1 写入后，**实例 2** 下一次读也必须返回 `A2`
    —— 失效信号来自**共享的数据库**，不是进程内广播
  - TTL 明知很长也不影响结论（证明失效不依赖 TTL 到期）
- **类型**: 功能 / 安全

### AC-S-053: 平台级配置变更使**所有租户**的缓存失效
- **前置条件**: 用 **`favicon`**（T1 **没有**租户级覆盖 —— 否则读不到平台值，
  不能像 `logo` 那样被 T1 覆盖）。
  租户 T1、T2 各自读过平台级 `favicon` = `F`（两租户缓存均已填充）
- **测试步骤**:
  1. 修改**平台级** `favicon` 为 `F2`
  2. **立刻**以 T1 读，再以 T2 读
  3. **构造掩蔽顺序**：先让 T1 的 `tenant:<id>` epoch 被推到**高于** `platform` epoch
     （改一次 T1 的租户级配置），再改**平台级** `favicon`，然后立刻读 T1 的值
- **预期结果**:
  - 2 → T1 与 T2 **都**返回 `F2`
  - 3 → T1 也返回 `F2` —— **本条是核心判据**：
    若实现用 `SELECT max(epoch)`，租户 epoch 更高时平台级写入**不改变 `max`**，
    T1 会继续返回旧值 `F` → **判失败**
  - 失效判断必须是**成对比较**：缓存里存的 `(platform_epoch, tenant_epoch)` 与库里两个值
    **逐一相等**才算新鲜；任一不等即重算
- **类型**: 功能 / 安全

### AC-S-054: 品牌公开接口的白名单是 **fail closed** 的
- **前置条件**: 平台级配 `logo=PA` / `primaryColor=#111111` / `conflictStrategy=manual` / `scimBaseUrl=...`；
  T1 覆盖 `logo=PB` / `title=T1平台`；T2 覆盖 `logo=PC`；
  S1（属 T1）覆盖 `logo=PS`（白名单内键）；另有 `conflictStrategy` 空间级值（白名单外）
- **测试步骤**:
  1. **不带令牌**，带 `X-Tenant-Id: T1` 请求品牌接口
  2. 不带令牌、**不带** `X-Tenant-Id`
  3. 不带令牌，带 `X-Tenant-Id: T1` + `X-Space-Id: S1`
  4. 不带令牌，带 `X-Tenant-Id: T2`
  5. 枚举响应体里出现的**全部**键名
- **预期结果**:
  - 1 → **200**，`logo` 的值 **必须等于 `PB`**（T1 的租户值）——
    **只说"只含白名单键"不够**：平台默认品牌同样只含白名单键，
    不做租户解析的实现也能通过
  - 2 → **200**，`logo` = **`PA`**（平台默认；登录页在不知道租户时也要能渲染）
  - 3 → **200**，`logo` = **`PB`**（租户值），并**显式断言 ≠ `PS`**
    （未认证请求不带可信空间身份 → 忽略空间级覆盖）
  - 4 → `logo` = **`PC`**（证明真的按租户解析，不是恒返回平台值）
  - 5 → 响应键名**逐条枚举**：`conflictStrategy` / `scimBaseUrl` **不得出现**
  - **响应形状 = 扁平 map** `{"logo": "...", "primaryColor": "..."}`（D61），
    不是 `[{key,value,source}]` —— 它的用途是「直接渲染」，不是「配置查询」
- **类型**: 安全

### AC-S-055: 前端无硬编码品牌（父 AC-014 收窄版）
- **前置条件**: 平台级 `title=示例平台`、`primaryColor=#0055aa`、`favicon=<路径>`
- **测试步骤**:
  1. 静态扫描 `apps/shell/**`（含 `index.html`、`App.svelte`、`app.css`）
  2. 改平台级 `title` → 刷新页面 → 看浏览器标题与页面 `<h1>`
  3. 改平台级 `primaryColor` → 刷新页面 → 看页面主色
  4. 确认渲染出的主色**不等于** `packages/theme` 的包默认值 `#1677ff`
- **预期结果**:
  - 1 → `apps/shell/**` 里**品牌具体值**（`底座壳`、`#1677ff`、`#1677ff` 的任意写法）
    命中数 **= 0**；键名出现是正常的，**值**出现不是
  - 2 → 浏览器标题与 `<h1>` 都变成 `示例平台`
  - 3 → 页面主色变成 `#0055aa`
  - 4 → 运行时以配置为准（`packages/theme` 的 `#1677ff` 只作**兜底默认**，
    被配置覆盖后**不得**出现在渲染结果里）
  - **同步更新受影响的既有测试**：`e2e/shell.spec.ts`（断言 heading 为 `底座壳`）、
    `apps/shell/src/App.svelte` 的文案/`<h1>` —— 改完这些测试必须**通过**，
    不允许「留硬编码过 AC」或「改了让测试红」
  - **同源**：E2E 通过 `platform-server` 的静态目录访问 shell
    （本仓**没有 CORS 层**，跨源 fetch 会被浏览器拦掉 → 标题/主色永远不会变）
  - 深色模式 / 组件密度 / 字体栈的完整令牌体系**不在本期**（REQ-011）
- **类型**: 功能

### AC-S-056: Feature Flag 按租户隔离
- **前置条件**: `newInvoice` 在 **T1 的租户级**配为 `true`；T2 无配置（平台默认 `false`）
- **测试步骤**: 1. 以 T1 读 2. 以 T2 读 3. **无身份**读（单元层）
- **预期结果**:
  - 1 → `enabled=true`，`source="tenant"`（**不带"若"** —— 前置已写死配在租户级）
  - 2 → `enabled=false`，`source="default"`（未配 → 用 manifest 默认值）
  - 3 → `enabled=false`（单元层，见 AC-S-058）
  - 关闭时返回 **200 + `enabled=false`**（契约 C10），**不是 404**
  - 一个租户的开关**不得**影响另一个租户
  - `source` 取值与 C9 的四个值一致（`platform` / `tenant` / `default`；flag **无空间级**）
- **类型**: 功能 / 安全

### AC-S-057: 灰度判定的确定性、单调性与边界
- **前置条件**: `beta` 已登记；准备 **n = 200** 个不同用户；
  `rolloutPercent` 通过派生键 `flags.beta.rolloutPercent` 设置
- **测试步骤**:
  1. `p=50`，**同一用户连读 10 次**
  2. `p=50`，200 个用户各读 1 次，记录**命中集合 S50** 与命中数
  3. 改成 `p=60`，记录 **S60**；再改成 `p=50`，记录 **S50'**
  4. 从 `p=60` 改成 `p=50`，检查 S50 ⊇ 关系（反向单调）
  5. `p=0` 与 `p=100` 各读全部用户
  6. `p=101` 与 `p=-1`
  7. 用**两个独立进程**对同一批用户判定
- **预期结果**:
  - 1 → 10 次结果**完全一致**
  - 2 → 命中数 ∈ **[70, 130]**（n=200、p=50 的接受区间，写死以免"看情况"）；
    且 S50 被完整记录
  - 3 → **S50 ⊊ S60**（单调：调高比例不能让已命中者掉出）；
    **S50' == S50**（改回原比例集合**完全复原**）
  - 4 → S50 ⊂ S60（反向也成立：调低时集合只能缩小）
  - 5 → `p=0` 全部不命中；`p=100` 全部命中
  - 6 → **400**，`code=CONFIG_VALUE_OUT_OF_RANGE`，**在 `PUT` 写时**拒绝
    （读路径遇到库里的脏数据 → `enabled=false` + WARN，**不返回 400**）
  - 7 → 两个进程的命中集合**完全相同** —— 专门抓 `RandomState` 之类随机播种的哈希
- **类型**: 安全 / 功能

### AC-S-058: 无用户身份时灰度 fail closed
- **前置条件**: `beta` 配 `rolloutPercent=100`（即"全都命中"）
- **测试步骤**（**单元层**，因为 HTTP 层拿不到「无身份 → 200」）：1. 无 user id 判定 2. 有 user id 判定
- **预期结果**:
  - 1 → `enabled=false` —— 灰度的判据是「这个**用户**在不在比例里」，
    没有身份该判据不成立 → fail closed，**不是**「按比例随机给」
  - 2 → `enabled=true`
  - `p=100` 也**不改变**第 1 步结论（缺的不是比例而是**身份**）
  - HTTP 层对无令牌请求断言 **401**（不是 200）
- **类型**: 安全

### AC-S-059: 未声明的 flag 不返回 404；flag 名受字符集约束
- **前置条件**: 只登记了 `newInvoice`
- **测试步骤**:
  1. 读**合法但未登记**的名字 `newinvoce`（拼错）
  2. 读含换行的名字 `bad\nINJECTED`
  3. 读超长名字（1KB）
  4. 读 `new-invoice`（kebab 写法的**未登记**名字）
- **预期结果**:
  - 1 → **200 + `enabled=false` + `source="default"`**，同时**记录一条结构化 WARN**
    （含该 flag 名，**字段截断**）；**不是** 404、**不是** 500
  - 2、3 → **400**，`code=FLAG_NAME_INVALID` ——
    非法名不该进日志（否则可伪造日志行 / 刷爆日志）
  - 4 → 200 + false（**不做归一化**：`new-invoice` 与 `newInvoice` 是**两个不同**的 flag，
    精确匹配；否则同名不同写法会互相串，灰度 bucket 也会完全不同）
  - flag 名规范：`^[a-z][a-zA-Z0-9]{0,63}$`（camelCase 起手、无连字符）
- **类型**: 功能 / 安全

### AC-S-060: 配置与开关的写接口必须鉴权
- **前置条件**: 用户 M 是普通用户（只有内置角色 `platform-user`）；
  用户 A 持有 `config:entry:write` + `config:flag:write`；
  用户 P 额外持有 `config:platform:write`
- **测试步骤**:
  1. **以 M** 调 `PUT /api/config/entries`（租户级）
  2. **以 M** 调 `PUT /api/feature-flags/{flag}`
  3. 以 A 写**租户级**配置
  4. 以 A 写**平台级**配置 → 走**独立路由** `PUT /api/config/platform-entries`
  5. 以 P 写**平台级**配置（同一路由）
  6. **不带令牌**调 1、2、4 三条路由
  7. 令牌属于 T1、请求头写 `X-Tenant-Id: T2`，写配置
- **预期结果**:
  - 1、2 → **403** `INSUFFICIENT_PERMISSION`
  - 3 → 成功
  - 4 → **403** —— 平台级写入走**另一条**路由，要求**独立的** `config:platform:write`；
    只有 `config:entry:write` **不够**
  - 5 → 成功
  - 6 → **401** `UNAUTHENTICATED`（接口不得裸奔）
  - 7 → **404**（借请求头把写入导到别的租户 → 当不存在处理，不泄露存在性）
  - 权限点共 **4 个**，全部在 `crates/platform-config/manifest.json` 登记，
    **并同时写权限点种子迁移**（否则启动对账 fail-fast，进程起不来 —— D67）
- **类型**: 安全

### AC-S-061: 新增的权限点必须有鉴权调用点；既有缺口如实登记不扩围
- **前置条件**: 契约 C-5 已生效（`extension-points v2`）
- **测试步骤**:
  1. 用**不依赖 `ContractFixture`** 的扫描入口（本需求新增，D58）扫
     `crates/platform-config/{manifest.json,src/}`，核对 4 个权限点
  2. 逐个核对：`config:entry:write` / `config:flag:write` / `config:platform:write` /
     `config:platform:read` 是否各有 `require_permission(...)` 调用点
  3. 删掉其中任一调用点 → 重跑
  4. 记录 `platform-org`（9 个）与 `platform-rbac`（`rbac:permission:read`）的**基线**
- **预期结果**:
  - 1 → 扫描器能跑（**不需要** `Repository` + `PlatformModule`）
  - 2 → 4 个权限点**各自**都有调用点（**逐个列出**，不是抽样）
  - 3 → **失败**并指名该权限点
  - 4 → 基线快照为：`platform-org` 9 个已登记、**0** 个 `require_permission` 调用点；
    `rbac:permission:read` 0 个。**本期不扩围**（扩了当天就失败），
    但**必须把缺口逐条写进 `feature-meta/verification-report.md`**，
    并确认本需求**没有让缺口变大**
  - ⚠️ 「不得恶化」的可判定形式 = **基线快照 + 差值 ≥ 0**，不是「跑一遍没红」
- **类型**: 安全 / 可维护性

### AC-S-062: 三表分离 + REVOKE 直接 DML + `jsonb` 可逆迁移
- **前置条件**: 三张表已建；应用连接角色 `base_app`（经 `base_platform_app` 组授权）
- **测试步骤**:
  1. 以 `base_app` 读 `platform_config`
  2. 以 `base_app` 直接 `INSERT` / `UPDATE` / `DELETE` `platform_config`
  3. 通过平台的 `SECURITY DEFINER` 写函数写 `platform_config`
  4. 以 T1 读 `platform_tenant_config`（库里另有 T2 的行）
  5. 以 T1+S1 读 `platform_space_config`（库里另有 T1+S2 与 T2+S1 的行）
  6. 先插 3 种 JSON 类型值 → 跑 up/down/up，检查类型与值语义
- **预期结果**:
  - 1 → 能读（平台级行全体可见是设计如此）
  - 2 → **三种 DML 全部被拒**（`permission denied`）——
    `base_platform_app` 上的**直接 DML 已被 REVOKE**（D56）。
    ⚠️ **前提必做**：`deploy/seed/02-app-role.sql` 的 `ALTER DEFAULT PRIVILEGES`
    会让**新建表自动继承写权**，所以迁移里必须**显式** `REVOKE INSERT, UPDATE, DELETE`
  - 3 → 成功（这是**唯一**的平台级写通道；应用层权限点仍在外层把关）
  - 4 → 只看到 T1 的行；5 → 只看到 T1+S1 的行（RLS 生效）
  - 6 → 三张表 `value` 均为 **`jsonb`**；up/down/up 后 `jsonb_typeof` 与值**完全一致**
  - ⚠️ **down 迁移不得 `DROP TABLE platform_tenant_config`**
    （既有 `20261118000200_tenant_isolation.down.sql` 是 `DROP TABLE`，新迁移照抄会删掉历史表）
- **类型**: 安全 / 可维护性

### AC-S-063: 平台级配置的**读**也要授权
- **前置条件**: `scimBaseUrl` / `ldapBaseDn` / `conflictStrategy` 均为 `scope: platform`；
  用户 M 是普通用户；用户 P 持有 `config:platform:read`
- **测试步骤**:
  1. 以 M 读 `scimBaseUrl`（平台级键）
  2. 以 M 读 `pageSize`（租户级键）
  3. 以 P 读 `scimBaseUrl`
  4. 不带令牌请求品牌接口，检查是否泄露平台级运维键（**对照 AC-S-054 第 5 步**）
- **预期结果**:
  - 1 → **403** `INSUFFICIENT_PERMISSION` ——
    否则任何租户的普通用户都能读到平台级基础设施配置
    （而 AC-S-054 费力把 `scimBaseUrl` 挡在公开接口外，登录后却能读到，等于白挡）
  - 2 → 成功（租户级键不受限）
  - 3 → 成功
  - 4 → 仍不泄露（品牌白名单与读授权是**两道**独立的门）
- **类型**: 安全

---

## 明确不做的（不在本期验收范围）

- **主题令牌的完整体系**（深色模式、组件密度、字体栈的组件级应用）→ REQ-011
- **配置的管理界面**（表单、列表、可视化编辑）→ REQ-012
- **空间（space）实体与管理** —— 本仓**没有 space 表**；只按 `X-Space-Id` 字串维度存配置，
  且只对展示类白名单键生效
- Feature Flag 的 A/B 分流统计、灰度自动推进、实验平台
- 配置的版本历史与回滚
- 配置项的多语言文案（属 REQ-021）
- 跨租户的配置共享 / 继承
- **把 C-5 扩到平台 crate**（`platform-org` 的 9 个 / `rbac:permission:read` 的缺口登记为技术债，见 AC-S-061）

## 已知不验收项（明确写出来，避免被当成漏做）

| 项 | 原因 | 归属 |
|---|---|---|
| CI 真跑（6 个 job / 镜像体积门禁 / Trivy） | 本仓**无 remote**，GitHub Actions 跑不起来 | 需用户提供 GitHub / Gitea 地址 |
| 分支保护（required check + Code Owners） | 代码里做不到 | 需人工在远端配置 |
| 真实多实例部署下的缓存收敛 | 本机单实例；AC-S-052 / 053 / 057 用**两个独立实例 / 进程**模拟 | 部署侧 |
| 主题令牌在前端组件上的实际渲染效果 | 属 REQ-011 | REQ-011 |
| `platform-org` / `platform-rbac` 的鉴权缺口 | 既有技术债，本需求**不扩围**；已登记（AC-S-061 步骤 4） | 后续需求 |
| `packages/theme` 的令牌体系改造 | 本期只要求「运行时可被配置覆盖」；令牌体系归 REQ-011 | REQ-011 |
| 覆盖率数字 | 本期不设覆盖率门槛 | — |
