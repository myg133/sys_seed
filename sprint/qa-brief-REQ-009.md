# QA 任务书：REQ-009 RBAC 权限与数据范围（Pre-merge）

- **需求**: REQ-009「RBAC 权限与数据范围」
- **迭代**: Sprint 3
- **基线**: `develop` @ `400cefa`
- **Dev 分支**: `feature/REQ-009`（worktree：`/home/myg133/Projects/system_seed/feature-REQ-009`）
- **性质**: **本底座第一个「权限判定」模块** —— 判定错了就是数据泄漏或权限旁路

---

## 背景

REQ-009 交付「谁能看到什么、能做什么」的**统一判定**：角色模型、数据范围求值、
鉴权中间件、受限表达式 DSL，并把 `extension-points v1` §4 明写的
「已登记权限点必须有鉴权调用点」缺口补上（**契约升 v2，新增 C-5**）。

**本需求是底座里最容易被「看起来对了」骗过去的一个**：
判定逻辑一旦算错，测试往往仍然是绿的（因为夹具单一），
或者错误码对但语义错（403 掩盖了 404）。

> Sprint 2 复盘结论：34 个 QA 问题里 **9 个属于「静默放行」类**。
> 本需求的每个「应当被拦住」的机制，**必须自己造违规复现**，不采信 Dev 的数字。

---

## 角色

你是 **QA**。规则：

- **你不写代码、不改实现**。发现问题写进报告，交给 BA 决定。
- **你也不 commit**。报告写到 `feature-meta/qa-report.md`（工作区里），
  由 BA 落到 `demand` 分支。
- **每项自己复现**，不采信 Dev 自验报告里的数字 —— 那些数字本 Sprint 已被发现过失实。

---

## 需求文档（绝对路径）

| 文件 | 作用 |
|---|---|
| `/home/myg133/Projects/system_seed/BA/demands/REQ-009-rbac/acceptance.md` | **16 条 AC：AC-S-033 ~ AC-S-048**（唯一验收基线） |
| `/home/myg133/Projects/system_seed/BA/demands/REQ-009-rbac/design-summary.md` | 设计决策（角色模型 / 数据范围 / DSL / 中间件 / 缓存 epoch / 既有件清单） |
| `/home/myg133/Projects/system_seed/BA/demands/REQ-009-rbac/test-cases/` | **59 条用例**（`index.md` + A/B/C/D 四域），**你自验时按这些跑** |
| `/home/myg133/Projects/system_seed/BA/contracts/extension-points/v2.md` | 契约 v2（**C-5 是新增的失败级检查**） |
| `/home/myg133/Projects/system_seed/BA/contracts/api-conventions/v1.md` | C1 / C2 / C4 / **C5 越权码** / C6 错误体 / C7 数据范围自动叠加 |

---

## A. ★ 最高优先级：数据范围算错 = 数据泄漏

### A-1 兼岗默认主部门（AC-S-039）

- 前置：用户 U 兼岗 `dept-a`（`is_primary = true`）+ `dept-b`（`is_primary = false`），
  两部门都有记录，`dept-b` 下还有子节点
- **自己造数据**，不要用 Dev 造的夹具（要用 Dev 的夹具也得自己核对 `is_primary` 的值）
- 判据：默认配置下 `OWN_ORG` / `OWN_ORG_AND_BELOW` 的结果集**不含 `dept-b` 的任何一条**
- **反向测**：开了 `use_all_memberships` 后应当**变宽** —— 如果没变宽，说明开关是摆设

### A-2 主部门缺失 / 歧义 fail closed（AC-S-040）

- 分两层验：
  1. **预防层**：再插一条 `is_primary = true` → **DB 必须拒绝**（看错误信息）
  2. **防御层**：临时 `DROP INDEX` 造出两个主部门 → 判定**必须退化为 `SELF`**
- **判据**：结果集 = 只有本人创建的记录。**退化成 `ALL` 或「看到本部门数据」都判失败**
- 这两层都要自己跑，Dev 说做了不算

### A-3 多角色取最宽松（AC-S-041）

- 关键区分点：AC-S-041 第 3 步（**解绑第三个角色**）的结果必须**与第 1 步不同**
- 若相同 → 实现取的是**交集**不是并集 → 判失败
- 权限点级 scope（`export` 收紧为 `SELF`）必须**覆盖**角色级

### A-4 SQL 层过滤，不是内存筛（AC-S-048）

- 看**渲染出的 SQL 文本**里有没有平台追加的范围条件片段，且值走参数绑定
- 「查出来在内存里筛」虽然结果对，但**违反契约 C7**（平台自动叠加、模块不得自行实现）
- 顺带验：**不存在**「跳过数据范围」的开关

---

## B. ★ 鉴权三语义与存在性泄露（AC-S-044 / AC-S-045）

### B-1 三种拒绝语义

| 情形 | 期望 | 陷阱 |
|---|---|---|
| 无 / 无效 token | 401 `UNAUTHENTICATED` | 401 不能写成 403 |
| 有 token、**没权限点** | 403 `INSUFFICIENT_PERMISSION` | **不是** `PERMISSION_DENIED`（契约 C6 示例用的是 `INSUFFICIENT_PERMISSION`） |
| 有权限点、**记录超范围** | 404 `NOT_FOUND` | 403 也「说得过去」但**违反 C5** |

### B-2 ★ 存在性不泄露（AC-S-045）—— 本需求最容易放过的一处

**对照组是「越权访问的资源」vs「不存在的资源」**，不是「有权 vs 无权」。

- 用户 P **有**权限点，但记录 R1 在其范围外 → 404
- 不存在的 id → 404
- **对比两次响应的状态码 / 错误体结构 / 响应头集合** —— 除 `trace_id`、`instance` 外**不得有差异**
- 对照组：范围内记录 → 200（证明接口不是恒 404）
- **动作类 403 不在此条判定范围内**（契约 C5 规定的行为，不是泄露）

### B-3 错误体形状（AC-S-044）

- RFC 9457：`type` / `title` / `status` / `detail` / `instance` / `code` / `trace_id`
- `detail` **必须是英文 ASCII** —— **抓一个中文句子就算失败**
- `detail` 可含权限点编码；**不得**含被拒资源的业务字段（标题 / 金额 / 归属人 / `org_node_id`）

### B-4 ★ 管理接口自举防护（AC-S-034 新增）

- **普通用户**（只有 `platform-user`）调 `/api/rbac/*` 的**写接口**（建角色 / 授权 / 绑定）
  → 必须 **403**
- 读接口同样要保护
- 这是**权限系统的自举口**：不挡住的话，任何登录用户都能给自己授权 `platform-admin`
- **必须自己试**：造一个普通用户的令牌去调，看是不是真的被拒

---

## C. ★ 注册表双向对账（AC-S-033）

- manifest 多一个权限点 → **启动必须失败**（不是告警！），错误信息**逐条列出**差异，
  且每条带**来源 manifest 路径**
- 库里多一个权限点 → 同样启动失败
- **判据是「进程起不来」**，不是「日志里有一行警告」

---

## D. ★ C-5 契约检查（AC-S-046）

四个负向 + 一个正向，**都要自己造**：

| 造法 | 期望 |
|---|---|
| manifest 登记了但代码里没有鉴权调用 | **失败** |
| 模块自己写鉴权函数绕过平台入口 | **失败** |
| 用**非字面量**参数调鉴权入口 | **失败**（无法静态确认 → fail closed） |
| 正常模块 | 通过 |
| 全仓搜鉴权入口的**定义处** | **只能有一个** |

- ⚠️ **特别注意**：C-2 是**字面量扫描**，它**分不出**「定义了一个权限点常量」和「真的调用了鉴权」。
  如果 C-5 沿用了 C-2 的扫描逻辑，**它会假绿**。
  请专门验一下：C-5 是不是真的在找**调用点**，而不是在找字面量。

---

## E. ★ 撤销立即生效 + 跨进程（AC-S-047）

- 撤销后**立刻**再调 → 必须被拒（不许出现「60 秒内仍可用」）
- **跨进程**用**两个独立 checker 实例**（各自持自己的缓存、共享同一个 DB）模拟：
  实例 1 撤销 → **实例 2** 立刻调也必须被拒
- 若实例 2 仍能访问 → 失效信号是进程内的、不是 DB 的 → **多实例部署下撤销不生效** → 判失败

---

## F. 表达式 DSL 逃逸（AC-S-042）

五条表达式逐个试（合法 / 白名单外字段 / 函数调用 / 含注入串 / 多语句）。
含注入串那条的**三条判据**都要满足：
① SQL 文本里不出现该串；② 不出现全表命中的效果；③ 只返回字面量等值的行。

---

## G. 其它 AC 逐条核

AC-S-034 / 035 / 036 / 037 / 038 / 043 逐条对 `acceptance.md` 验。
其中：

- **AC-S-037**：`platform-admin` 的权限集合 = **注册表当前全部权限点，逐个比对**，
  **不能只比数量、不能抽样**；且**不能写死数字**（注册表会变）
- **AC-S-036**：用注册表里**真实存在**的模块前缀（当前只有 `sandbox:` 与 `org:`）

---

## H. ★ 防假绿护栏（承重性验证）

1. **`cargo test` 只能有一个 `--`**。跑完先看输出里的「N passed」，**N 必须 > 0**。
   多写一个 `--` 会让 libtest 把 flag 当过滤串 → **一个用例没跑、退出码还是 0**。
2. **反向验证测试的有效性**：挑 2~3 个最关键的断言，**临时改坏实现**
   （例如把 fail closed 改成 fail open、把默认主部门改成并集、把 404 改成 403），
   确认测试**会红**；**改回来**后确认**会绿**。改坏了**必须逐字节还原**（用 `git diff` 核对）。
3. **跑重复**：至少连跑 2 轮，确认**没有偶发绿**（REQ-008 曾出现过 4 轮里 2 次死锁的间歇性失败）。

---

## I. 基建与契约一致性

- `feature-meta/manifest.json` 必须有 `"contracts": { "api-conventions": "v1", "extension-points": "v2" }`
  —— 缺了 `scripts/sync-contracts.sh` 直接 exit 2，**契约门禁在 CI 上是常红的**（本需求要修掉）
- `bash scripts/sync-contracts.sh` 必须**退出 0**（校验模式）
- `docs/contracts/extension-points/v2.md` 快照必须与 BA 正本一致（**不许手改快照**）
- `cargo xtask capabilities --check` 必须通过（`capabilities/` 是自动生成物，**手改即失败**）
- `AGENTS.md` 必须与实际一致：新增了 `crates/platform-rbac`、C-1~C-5、硬规则 1 的例外说明
- `cargo xtask check-deps` 必须通过（**`platform-rbac` 不得反向依赖业务模块**）

---

## J. 回归

- `cargo test --workspace -- --test-threads=2`（**只有一个 `--`**）全绿，
  记录**实际 passed 数**（基线：REQ-008 合并时 **303/303**，含 ignored **341/341**）
- `cargo clippy --workspace --all-targets -- -D warnings` **0 告警**
- 前端 `bun run test`（基线 23/23）、E2E 1/1（若受影响）
- **依赖预算**：`Cargo.lock` 包数**不得增长**（基线 **293**）——
  不引策略引擎、不引 Redis 客户端、不用 nom/pest。**核 `Cargo.lock` 的 diff，不是看 `Cargo.toml`**

---

## K. 环境注意

| 项 | 值 |
|---|---|
| 应用连接 | **必须用普通角色 `base_app`**：`postgres://base_app:base@127.0.0.1:5432/base_test`（超管会被 `require_isolation` 拒绝启动） |
| 测试库 | **只用 `base_test`**；**`base_dev` 绝不写** |
| 本机没有 psql 客户端 | `podman exec base-postgres psql -U base -d base_test -tAc "..."` |
| 容器 | `base-postgres` / `base-redis` 已在跑（healthy） |
| 容器起不来时 | `export XDG_RUNTIME_DIR=/run/user/$(id -u)`；`cd /home/myg133/Projects/system_seed/code && podman-compose -f compose.yaml up -d`；仍不行则 `podman rm -f base-postgres` 后从 `code/` 重起（volume 保留数据） |
| 磁盘 / 内存 | 低配机（7.7G 内存、剩 114G 磁盘）。**别跑满**；`jobs = 3` 已固定，**不要改** |
| 链接器 | **什么都不要配**（stable rustc 默认即 LLD） |
| 迁移 | REQ-009 有新迁移；**只对 `base_test` 应用**，不要碰 `base_dev` |

---

## 产出（唯一交付物）

写到 `/home/myg133/Projects/system_seed/feature-REQ-009/feature-meta/qa-report.md`
（**不要 commit**）。

## 验证报告 —— Pre-merge

### 追溯
AC-S-033 ~ AC-S-048 × 你实际执行的用例 × 结论（PASS / FAIL / 无法验证）

### 安全断言实测
A / B / C / D / E / F 各条：怎么造的违规、实际返回了什么、判据是什么

### 存在性泄露实测
越权资源 vs 不存在资源：状态码 / 错误体 / 响应头的**逐项对比**

### 假绿护栏结果
- 实际 passed 数
- **变异验证**：改了哪几处 → 红了没有 → 还原后绿了没有 → `git diff` 是否干净
- 重复跑了几轮

### 契约与基建
`sync-contracts.sh` / `capabilities --check` / `check-deps` / `AGENTS.md` 一致性

### 依赖预算
`Cargo.lock` 包数实测值 + 与 293 的差 + 新增/移除的 crate

### 问题（按严重度）
每条：**严重度**（HIGH / MEDIUM / LOW / INFO）· 复现步骤 · 实际 vs 期望 · 证据

### 未覆盖 / 无法验证
**如实列**。没跑的就写没跑，**不要**用「应该没问题」代替

---

## 纪律

- **不写代码、不改实现**（除了 H-2 的临时变异，且必须逐字节还原）
- **不 commit**
- **不采信 Dev 的数字** —— 每个数字自己跑一遍
- 结论要**可复现**：写清命令与实际输出
- **发现问题不等于失败** —— 但**隐瞒问题等于失败**。
  拿不准的写成「存疑」，写清存疑在哪
- 不要在报告里贴大段代码

---

## 回报格式

一段话讲清：**PASS / PASS-WITH-RISK / FAIL** + 问题按严重度的数字
（HIGH x / MEDIUM x / LOW x / INFO x）+ 最需要 BA 拍板的一条 + 未覆盖项清单。
