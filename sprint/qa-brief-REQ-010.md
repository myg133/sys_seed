# QA 任务书：REQ-010 配置、品牌与 Feature Flag（Pre-merge）

- **需求**: REQ-010「配置、品牌与 Feature Flag」
- **迭代**: Sprint 3（第二棒）
- **基线**: `develop` @ `12366e9`（REQ-009 合并点）
- **Dev 分支**: `feature/REQ-010`
- **性质**: 底座里**第一个「运行时可变的配置」** —— 判定错了就是「改完不生效」或「跨租户串配置」

---

## 背景

REQ-010 把配置从编译期常量变成按作用域分层的数据：平台 → 租户 → 空间，
改完**下一个请求**就生效；功能开关按租户与灰度比例**确定性**判定。

**本需求的规格被 QA（你自己）在两轮里挑出 36 条问题、12 条硬伤**，
其中 7 条是 BA 自己的设计错误。这 7 条特别值得盯 —— 它们都属于「看起来对」：

| # | 初版的错误 | 你要验的正面 |
|---|---|---|
| 1 | `SELECT max(epoch)` 做失效判断 | 平台级变更在**租户 epoch 更高**时仍然失效（AC-S-053 步骤 3） |
| 2 | 「租户连接连写都写不进去」是空话 | `REVOKE` 真的生效（AC-S-062 步骤 2）；`SECURITY DEFINER` 函数能写（步骤 3） |
| 3 | flag 登记 boolean 又要 `rolloutPercent` | 派生键 `flags.<F>.rolloutPercent` 真的可读写（AC-S-057） |
| 4 | 「跑平台的契约测试」不存在 | 新增的扫描入口能跑（AC-S-061 步骤 1） |
| 5 | 一条 `PUT` 路由管两种权限 | 平台级走独立路由且**只有** `config:platform:write` 够用（AC-S-060 步骤 4） |
| 6 | 给 `ConfigScope` 加 `default` 会开出第 4 个可写层 | `"scope": "default"` **写不进** manifest、写接口也**不接受** |
| 7 | 未定校验顺序 | 越级写**不会**顶包类型校验（AC-S-051 步骤 5） |

> Sprint 2 复盘结论：34 个 QA 问题里 **9 个属于「静默放行」类**。
> 本需求的每个「应当被拦住」的机制，**必须自己造违规复现**，不采信 Dev 的数字。

---

## 角色

你是 **QA**。规则：

- **你不写代码、不改实现**（临时变异除外，且必须逐字节还原）
- **你不 commit**。报告写到 `feature-meta/qa-report.md`，由 BA 落到 `demand` 分支
- **每项自己复现**，不采信 Dev 自验报告的数字

---

## 需求文档（绝对路径）

| 文件 | 作用 |
|---|---|
| `BA/demands/REQ-010-config-branding/acceptance.md` | **17 条 AC（AC-S-049~063）**，唯一验收基线 |
| `BA/demands/REQ-010-config-branding/design-summary.md` | 设计决策（**§2.1.1 写通道 / §3.3 校验顺序与错误码 / §5.3 派生键 / §6.0 路由 / §7.1 epoch / §11 风险 R1~R19**） |
| `BA/demands/REQ-010-config-branding/test-cases/` | 用例集（按它跑） |
| `BA/contracts/api-conventions/v2.md` | **C9 / C10 的 v2**（`source: default`、作用域约束、类型校验、灰度确定性） |
| `BA/contracts/extension-points/v2.md` | C-5 |
| `BA/demands/REQ-001-platform-foundation/decisions.md` | **D45~D68** 本需求的全部裁定 |

---

## A. ★ 最高优先级：缓存失效（「改完不生效」是本需求最容易出的假通过）

### A-1 平台级变更要让**所有租户**失效（AC-S-053）

- 租户 T1、T2 各自读过平台级 `favicon`（两缓存都填充）
- 改平台级 → **立刻**读 T1 与 T2 → 都必须是新值
- **★ 关键：构造掩蔽顺序** —— 先改一次 T1 的**租户级**配置（把 `tenant:T1` 的 epoch 推高），
  再改**平台级**，然后立刻读 T1
- **若实现用 `SELECT max(epoch)`：T1 会返回旧值 → 判失败。** 这条是本需求的「假通过探测器」

### A-2 跨进程（AC-S-052）

- 两个独立解析器实例（各自缓存、共享 DB）：实例 1 写 → 实例 2 读必须是新值
- TTL 设 300s，证明失效**不依赖** TTL 到期

---

## B. ★ 平台级写通道（AC-S-062）—— 这条最容易「以为做了」

| 步骤 | 必须实测 |
|---|---|
| 1 | `base_app` 能 `SELECT` `platform_config` |
| 2 | `base_app` **直接** `INSERT` / `UPDATE` / `DELETE` → **三种全部 `permission denied`** |
| 3 | 通过 `SECURITY DEFINER` 函数写 → 成功 |
| 4 | ⚠️ **必查**：`deploy/seed/02-app-role.sql` 有 `ALTER DEFAULT PRIVILEGES`，
新建表**自动继承写权** → 迁移里若忘了 `REVOKE`，步骤 2 会**成功**（= 失败） |

**还要查**：`platform_tenant_config` / `platform_space_config` 的 RLS 是否真的隔离（T1 看不到 T2 的行）。

---

## C. ★ 灰度确定性（AC-S-057 / 058）

- 同一用户连读 10 次 → 完全一致
- n=200、p=50 → 命中数 ∈ **[70,130]**；记录命中集合 S50
- p=60 → **S50 ⊊ S60**；p 改回 50 → **S50' == S50**
- p=60 → 50 → **S50 ⊂ S60**（反向单调）
- p=0 全不命中；p=100 全命中；p=101 / -1 → **400 `CONFIG_VALUE_OUT_OF_RANGE`（在 PUT）**
- **两个独立进程**判同一批用户 → 集合完全相同
  （专门抓 `RandomState` 之类**每进程随机播种**的哈希 —— 那种实现单进程测不出来）

**无身份 fail closed**（AC-S-058）：`p=100` 且无 user id → `enabled=false`。在**单元层**验；
HTTP 层对无令牌断言 **401**。

---

## D. ★ 品牌公开接口（AC-S-054）—— 无令牌，所以每一处都要盯

- 带 `X-Tenant-Id: T1` → `logo` **必须等于 T1 的值**（不是「只含白名单键」就行）
- 不带 `X-Tenant-Id` → 平台默认值
- 带 `X-Tenant-Id: T1` + `X-Space-Id: S1` → **T1 的值**，并显式断言 **≠ S1 的值**
- 带 `X-Tenant-Id: T2` → T2 的值（证明真的按租户解析）
- 枚举响应**全部键名** → `conflictStrategy` / `scimBaseUrl` **不出现**
- 形状是**扁平 map**，不是 `[{key,value,source}]`

**另外验 AC-S-063**：普通用户读 `entries?keys=scimBaseUrl`（平台级键）→ **403**；
读 `pageSize`（租户级键）→ 200。这是**第二道门**，与白名单独立。

---

## E. 作用域与类型校验（AC-S-050 / 051）

- 越级写：`conflictStrategy`（platform）在租户级/空间级 → **400 `CONFIG_SCOPE_NOT_ALLOWED`**
- `pageSize`（tenant）在空间级 → 400
- **空间级两道门**：登记 `space` **且** 在白名单内；只满足前者 → **400**
- 未登记键 → 400 `CONFIG_KEY_NOT_REGISTERED`
- 类型：`"20"` / `true` 写进 number 键 → 400 `CONFIG_TYPE_MISMATCH`
- **校验顺序**：同一请求同时违「未登记」+「类型错」→ 返回 `CONFIG_KEY_NOT_REGISTERED`
  （证明顺序是 认证→权限点→键名→注册表→作用域→类型，不是随口说的）

**D60 回归**：`"scope": "default"` 必须**写不进** manifest（serde 拒），
写接口也**不接受**「default 作用域」。若实现给 `ConfigScope` 加了 `Default` 变体 → 判失败。

---

## F. 鉴权（AC-S-060 / 061）

| 步骤 | 期望 |
|---|---|
| 普通用户写配置/开关 | 403 `INSUFFICIENT_PERMISSION` |
| 只有 `config:entry:write` 写平台级 | **403**（要走独立路由 + 独立权限点） |
| 有 `config:platform:write` 写平台级 | 200 |
| **无令牌**调三条写路由 | **401** |
| 令牌属 T1、头写 T2 | **404**（不许借头把写入导到别的租户） |

**C-5**：4 个权限点**逐个**核对有 `require_permission` 调用点；
删掉任一个 → 扫描失败并指名。**基线快照**：`platform-org` 9 个 / `rbac:permission:read` 0 个调用点 ——
本期**不扩围**，但要确认缺口**没变大**。

---

## G. 前端无硬编码（AC-S-055）

- `apps/shell/**` 里 `底座壳` / `#1677ff` 等**品牌具体值**命中数 = 0（键名出现是正常的）
- 改 `title` → 刷新 → 浏览器标题与 `<h1>` 都变
- 改 `primaryColor` → 刷新 → 页面主色变，且**≠ `packages/theme` 的 `#1677ff`**
- `e2e/shell.spec.ts` **已同步更新**且**通过**（不许「留硬编码过 AC」或「改了让测试红」）
- **同源**：本仓**没有 CORS 层**，E2E 若跨源（:4173 → :8080）fetch 会被浏览器拦掉，
  标题/主色永远不会变 → 必须走 server 静态目录

---

## H. ★ 防假绿护栏

1. **`cargo test` 只能有一个 `--`**；看「N passed」，**N 必须 > 0**
2. **变异验证**（挑 2~3 个最关键的点，逐字节还原 + sha256 校验）：
   - epoch 改回 `max()` → A-1 必须红
   - 去掉 `REVOKE` → B-2 必须红
   - 灰度改成 `rand()` 或 `RandomState` → C 的跨进程那条必须红
   - 品牌白名单加一个非白名单键 → D 的键名枚举必须红
3. **连跑 ≥2 轮**，确认没有偶发绿

---

## I. 契约与基建

- `feature-meta/manifest.json` pin：`api-conventions: v2` + `extension-points: v2`
- `scripts/sync-contracts.sh` **exit 0**；`docs/contracts/api-conventions/v2.md` 快照存在且一致
- `cargo xtask capabilities --check` 通过（`permissions.json` 应有 **4 个** config 权限点）
- **启动对账**：新增 4 个权限点，`platform_permission` 表里必须有对应行
  （只改 manifest 不写种子迁移 → **进程起不来**）
- `AGENTS.md` 已同步（新 crate、新权限点、`ConfigResolver` 是 platform-capability 非扩展点）
- `cargo xtask check-deps` 通过（`platform-config` 不得反向依赖业务模块）

---

## J. 回归

- `scripts/test-integration.sh`（**只有一个 `--`**）全绿，记录**实际 passed 数**
  （基线：REQ-009 合并时 **461/461**）
- `cargo clippy --workspace --all-targets -- -D warnings` → 0 告警
- 前端 `bun run test`（基线 23/23）+ **E2E**（本需求动了 shell，**必须跑**）
- **依赖预算**：`Cargo.lock` 包数**不得增长**（基线 **294 包 / 第三方 registry 284**）——
  确定性哈希用已在树里的 `sha2`，**不引** figment / config-rs / launchdarkly。**核 `Cargo.lock` diff**

---

## K. 环境

| 项 | 值 |
|---|---|
| 应用连接 | **普通角色 `base_app`**（超管会被 `require_isolation` 拒绝启动） |
| 测试库 | **只用 `base_test`**（`scripts/test-integration.sh` 会重置）；**`base_dev` 绝不写** |
| psql | 本机没有 → `podman exec base-postgres psql -U base -d base_test -tAc "..."` |
| target | `export CARGO_TARGET_DIR=/home/myg133/Projects/system_seed/code/target` |
| 链接器 | **什么都不要配**（stable rustc 默认即 LLD） |
| 磁盘/内存 | 剩 ~109G / ~4.8G 可用，**别跑满**；`jobs = 3` 已固定 |
| 迁移 | 本需求有 `TEXT → jsonb` 迁移；**只对 `base_test`**，且要验 up/down/up |

---

## 产出

`feature-REQ-010/feature-meta/qa-report.md`（**不要 commit**）。

结构：追溯矩阵（17 条 AC）/ 缓存失效实测（含掩蔽顺序）/ 平台写通道实测 /
灰度实测（含跨进程）/ 品牌接口实测（含键名枚举）/ 作用域与类型实测（含顺序）/
鉴权实测 / 前端实测 / 假绿护栏结果 / 契约与基建 / 依赖预算 / 问题（按严重度）/ 未覆盖与无法验证。

---

## 纪律

- **不写代码、不改实现**（变异除外，必须逐字节还原）
- **不 commit**
- **不采信 Dev 的数字**
- 结论要**可复现**：写清命令与实际输出
- **发现问题不等于失败；隐瞒问题等于失败**。拿不准写「存疑」
- 规格本身有问题的照样写进报告
- 不要在报告里贴大段代码

---

## 回报（一段话）

**PASS / PASS-WITH-RISK / FAIL** + 问题按严重度（HIGH x / MEDIUM x / LOW x / INFO x）
+ 最需要 BA 拍板的一条 + **实际 passed 数** + 未覆盖项清单。
