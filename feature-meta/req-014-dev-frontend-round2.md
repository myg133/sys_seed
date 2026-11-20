# REQ-014 审计查询界面（前端）—— Dev 自验报告 · **棒 2（前端收尾 + i18n + E2E 夹具）**

- **需求**: REQ-014「审计日志」
- **分支**: `feature/REQ-014`（基线 `develop@0683115`）｜工作区 `feature-REQ-014/`
- **我负责的 AC**: **AC-S-132**（审计查询界面）；本轮另做 **D169**（游标取数）、**D170**（i18n + E2E 夹具）
- **规格基线**: `BA/demands/REQ-014-audit/acceptance.md`（rev 3）、`decisions.md`（rev 2，**D145 / D155 / D167 / D169 / D170**）、`design-summary.md`（rev 2 §5）、`test-cases/frontend.md`（TC-014-111 ~ 120）、契约 `api-conventions v1`（C4 / C7 / C13）
- **文件边界**: 只碰 `apps/shell/**`、`packages/i18n/**`、`scripts/**`、`e2e/**`；**未碰** `crates/**` 与 `migrations/**`（后端棒的文件集）
- **未 commit**（按铁律 1，交给 BA 统一提交）

> 棒 1 的交付（`audit-api.ts` / `Audit.svelte` / `routes.ts` / 37 条单测 / 5 条 E2E）**不重做**，
> 本轮在其上按 D169 改造取数方式、补 D170 的两项遗留。

---

## 1. 交付清单（文件 → 判据）

| 文件 | 性质 | 本轮对应的判据 |
|---|---|---|
| `apps/shell/src/lib/audit-api.ts` | 改 | **D169**：游标模式成为**默认**取数方式；`buildAuditQuery` 游标分支**不发 `sort`**（后端拒绝）；新增**游标栈**纯函数（`newAuditCursorStack` / `auditCursorPage` / `auditCursorOf` / `pushAuditCursor` / `popAuditCursor`）；`loadAuditFirstPage` 改为「首页 = 不带游标 + limit」 |
| `apps/shell/src/pages/Audit.svelte` | 改 | **D169**：状态从「页码」改为**游标栈**；页码指示 = 栈长度；「下一页」= 压栈 + 请求、「上一页」= 弹栈 + 请求；**自画游标分页器**（指示如实写成「本次浏览第 N 页」）；过滤 / 改页大小 → 游标栈回首页 |
| `apps/shell/src/components/Table.svelte` | 改 | 新增**可选** prop `hidePager`（壳层扩展，`@base/ui` 的 `TableProps` 契约未动）：自带的「第 N 页 / 共 M 页」分页器与游标分页语义不符 → 审计页不渲染它 |
| `apps/shell/test/audit-api.test.ts` | 改 | 单测随实现更新（游标模式**不带 `sort`**、游标 + 非默认排序 → 抛错、首页 query 形状）+ **新增 5 条游标栈单测** |
| `apps/shell/test/audit-page.test.ts` | 改 | 单测随实现更新（首屏 query **不带 cursor / page**）+ **新增 3 条页码指示单测**（指示文案、不渲染自带分页器、下一页禁用态） |
| `packages/i18n/src/locales/zh-CN.json` | 改 | **D170-1**：5 个错误码文案 + 2 个审计页文案（页码指示 / 游标提示） |
| `packages/i18n/src/locales/en-US.json` | 改 | 同上（**键集合与 zh-CN 逐字一致**） |
| `packages/i18n/test/i18n.test.ts` | 改 | **D170-1 的「两侧同改」**：把 5 个新错误码加进 `codes` **抽样列表**；**断言本身一个字没动**（见 §5-偏离 1） |
| `scripts/seed-e2e.sh` | 改 | **D170-2**：新增账号 `u-perm-audit@example.com`（授 `audit:events:read`）+ 角色 `e2e-role-audit` + **11 条**审计记录夹具（走平台写通道）+ 4 条自证断言；模式检查纳入审计写通道 |
| `e2e/audit.spec.ts` | 改 | 改为**游标**口径的 E2E（请求带 `cursor` / **不带 `page`**、指示「本次浏览第 2 页」、回退不带游标）；就绪探测加**内容类型**判据（见 §5-发现 2）；登录带 `X-Forwarded-For` |
| `feature-meta/req-014-dev-frontend-round2.md` | 新增 | 本报告 |
| `feature-meta/verification-report.md` | 追加 | AC-S-132 的更新行（**不覆盖**棒 1 已写的内容） |

**diff 规模**：12 个文件（10 个代码 + 2 个报告）。

---

## 2. D169：游标取数 + 游标栈（页码指示）

### 2.1 为什么必须改

AC-S-132 步骤 2「翻页后记录**不重复**」是硬判据；审计表**持续追加** →
页码分页在变动数据集上必然重复 / 遗漏（契约 C7 把游标定为主口径的原话理由，D145 理由 2）。
BA 已裁定（**D169**）：**取数必须用游标**，页码指示用**游标栈**。

### 2.2 实现

| 环节 | 实现 |
|---|---|
| 请求 | `cursor` + `limit`（**不发 `sort`**，见 §5-发现 1）；首页不带 `cursor` |
| 游标栈 | `cursors: (string \| null)[]`，下标 0 = 首页哨兵（`null`）—— 与既有 `SchemaPage.svelte` **同一套机制**（REQ-012 的数据页） |
| 页码指示 | **= 栈长度**（首页 = 1）—— D169 的原文口径 |
| 下一页 | 响应 `nextCursor` **压栈** → 用它请求 |
| 上一页 | **弹栈** → 用新的栈顶请求（首页弹不动） |
| 过滤 / 改页大小 | 游标栈**回首页**（过滤条件变了，旧的游标路径没有意义） |

### 2.3 UI 文案（如实表达，D169）

| 键 | zh-CN | en-US |
|---|---|---|
| `common.auditPageIndicator` | 本次浏览第 {page} 页 | Page {page} of this browse |
| `common.auditCursorHint` | 审计记录持续追加，列表按游标顺序翻动，不能跳到任意页 | Audit records keep being appended; the list follows a cursor order and cannot jump to an arbitrary page |

- **不写**「第 3 页 / 共 20 页」：单测里有**反向断言**（`expect(body).not.toMatch(/\d+\s*\/\s*\d+/)`），
  E2E 里断言指示文本逐字等于「本次浏览第 2 页」。
- 「共 {count} 条记录」（服务端给的 `total`）保留 —— 那是**记录总数**，不是页数，说法本身不误导。
- 自带的页码分页器（`Table.svelte` 的 `{page} / {pageCount}`）在审计页**不渲染**（`hidePager`）。

### 2.4 与 QA 用例的口径差异（如实记录）

QA 的 `test-cases/frontend.md` TC-014-113 写的是「后端收到的请求参数含 **`page=2`**」——
那是 rev 1 的口径，已被 **D169** 推翻（该文件写于 09:21，D169 出在 10:20 之后）。
本轮按 **D169** 实现（游标），并把 E2E 的断言改成「**带 `cursor`、不带 `page`**」。

---

## 3. D170-1：5 个错误码的 i18n 本地化

| 错误码 | 何时 | zh-CN | en-US |
|---|---|---|---|
| `AUDIT_SENSITIVE_PAYLOAD_REJECTED` | `before`/`after` 含敏感键 | 审计内容包含敏感字段，已拒绝写入 | The audit payload contains sensitive keys and was rejected |
| `AUDIT_CHAIN_BROKEN` | 校验发现链断裂 | 审计链完整性校验未通过 | The audit chain integrity check failed |
| `AUDIT_EVENT_NOT_FOUND` | 查不到指定审计记录 | 审计记录不存在 | The audit event is not found |
| `AUDIT_INVALID_FILTER` | 过滤 / 分页 / 排序参数非法 | 审计查询条件不合法 | The audit query parameters are invalid |
| `AUDIT_SINK_UNAVAILABLE` | 审计写入失败（登录被拒） | 审计记录写入失败，操作已被拒绝 | The audit record could not be written, so the operation was rejected |

- 错误码**逐字**取自 `crates/platform-core/src/problem.rs:51-61`（只读，未改）。
- 两个语言包**键集合一致**（既有 i18n 测试断言）→ **79 + 5 = 84 个错误域条目**。
- 审计页的 `describeError` → `error.<CODE>` → 现在能显示**本地化**文案（此前回退成 `"AUDIT_INVALID_FILTER (HTTP 400)"`）。

---

## 4. D170-2：E2E 夹具

### 4.1 账号

| 账号 | 权限点 | 状态 |
|---|---|---|
| `u-perm-audit@example.com` | `audit:events:read` | ✅ 已进 `scripts/seed-e2e.sh`（新建 `e2e-role-audit` 角色承载授权） |

### 4.2 审计记录：**走平台的写通道**，不裸 INSERT

- 用 `platform_audit_append(...)`（`SECURITY DEFINER`；同租户 advisory lock；敏感键硬拒绝；
  `clock_timestamp()` 生成时间戳；规范事件体 → sha256）—— 这正是后端 `PgAuditSink`
  （`crates/platform-audit/src/sink.rs:115`）调用的那个函数 → **平台唯一的审计写通道**。
- **比裸 INSERT 好**：裸 INSERT 的行 `hash` / `prev_hash` 与内容不自洽 → 链校验必然报断裂，
  还会连累别的判据；走写通道的夹具**链是自洽的**，
  所以**不需要**「这些是夹具数据，不参与链校验」的免责声明。
  **实测**：`SELECT count(*) FILTER (WHERE prev_hash <> lag(hash) …)` → **11 行 / 0 处断裂**。
- **幂等**（D101）：按 `(tenant_id, object_type, object_id)` 判存在 → 已存在就**不重复写**；
  **不删行、不改行** → 重跑不打断已有的链。**实测**：连跑两次，夹具行数恒为 **11**。
- **11 条**的由来：翻页用例把每页条数调到 10 → 第 1 页 10 条 + 第 2 页 1 条
  → 响应带 `nextCursor`（「下一页」可点）→「翻页后记录不重复」可判。
- 形状：`actor_id` / `action` **互不相同** → 按操作人 / 按动作过滤各自只命中 1 条
  （QA 的 TC-014-115 / TC-014-116 的前置条件）；`ip` 给文档地址段 `203.0.113.10`
  → 「来源 IP」列非空（TC-014-112）。

### 4.3 播种脚本的自证（D108：夹具缺失必须 fail）

`scripts/seed-e2e.sh` 末尾的断言从 3 条加到 **4 条**：

| 断言 | 内容 | 期望 |
|---|---|---|
| ① | 4 个用户各**恰好 1 行** | `4 1 1 1 1` |
| ② | 权限点数目 | read=2 / write=5 / none=0 / **audit=1** |
| ③ | SSO 配置键 | 12 |
| ④ | **审计夹具行数**（`object_type = 'e2e_fixture'`） | **≥ 11** |

另：`has_schema` 的判据从「有 `platform_iam_user`」扩到
「有 `platform_iam_user` **且**有 `platform_audit_append`」——
迁移没跑就**先跑迁移**（而不是让夹具静默写不进去）。

**实测**：`./scripts/seed-e2e.sh` 连跑两次 → 都 `exit 0`，输出「审计夹具：11 条」。

---

## 5. 验证记录（真跑）

### 5.1 vitest（逐个包）

```bash
export XDG_RUNTIME_DIR=/run/user/$(id -u)
pnpm --filter @base/shell test
pnpm --filter @base/i18n test
pnpm --filter @base/sdk test      # 先 pnpm -r build
```

| 包 | 棒 1 后 | **本轮后** | 差 |
|---|---|---|---|
| `@base/shell` | 195 passed（17 文件） | **206 passed**（17 文件） | **+11** |
| `@base/i18n` | 43 passed | **43 passed** | 0（键数变了，条数不变） |
| `@base/sdk` | 38 passed | **38 passed** | 0 |

- `@base/shell` 的 +11 = `audit-api.test.ts` **+8**（游标 query 形状 ×3、游标栈 ×5）+ `audit-page.test.ts` **+3**（页码指示）。
- `@base/i18n` 的 43 条**全绿**：两语言包键集合一致、无重复文案、键名规范、
  **错误域条目数（84）== 抽样列表长度（84）**。

### 5.2 构建 / 类型 / 静态检查

```bash
pnpm -r build          # → 全绿（apps/shell: ✓ built in 2.46s）
pnpm --filter @base/shell check   # svelte-check → 0 errors / 0 warnings
npx tsc --noEmit --strict e2e/audit.spec.ts   # → 0 错误
npx eslint apps/shell/src apps/shell/test e2e # → 0 问题
npx prettier --check apps/shell/src apps/shell/test e2e packages/i18n/src  # → 全部符合
```

### 5.3 E2E（真跑）

```bash
export DATABASE_URL_TEST="postgres://base:base@127.0.0.1:5432/base_test"
./scripts/e2e-server.sh &        # 迁移 → 播种 → base serve（:8080）
npx playwright test e2e/
```

**全量 E2E：30 passed / 5 skipped / 0 failed（34.3s）** —— 其中审计页 **1 passed / 5 skipped**。
无回归（`schema-driven` / `shell` / `shell-auth` / `sso` / `i18n` 全部通过 → `Table.svelte` 的
`hidePager` 扩展没影响既有页面）。

**skip 清单（逐条；理由必须都是「被测系统未就绪」）**：

| 用例 | skip 位置 | 理由 | 判定 |
|---|---|---|---|
| TC-014-111 / TC-014-112（列表与五列） | `e2e/audit.spec.ts`「TC-014-111 / TC-014-112」开头 | `审计接口尚未实现（被测系统未就绪）` | ✅ 合规 |
| TC-014-113 / TC-014-114（游标翻页 + 不重复） | 同上文件「TC-014-113 / TC-014-114」开头 | 同上 | ✅ 合规 |
| TC-014-113 回退（上一页不带游标） | 同上文件「TC-014-113（回退）」开头 | 同上 | ✅ 合规 |
| TC-014-115 / TC-014-117（操作人过滤 + 交叉验证） | 同上文件「TC-014-115 / TC-014-117」开头 | 同上 | ✅ 合规 |
| TC-014-116（动作过滤） | 同上文件「TC-014-116」开头 | 同上 | ✅ 合规 |
| TC-014-118 / TC-014-119（403 页） | **无 skip** | — | ✅ 真跑**通过**（不需要后端：守卫在请求之前拦下） |

**skip 的探测口径**（`auditApiReady()`）：`GET /api/audit/events` →
① **404 / 501** → 路由不存在 → 未就绪 → 允许 skip；
② 响应**不是 JSON**（拿到了 `text/html`）→ 静态回落把未注册的 API 路径回落成了
`index.html` → 路由还没实现 → 未就绪 → 允许 skip；
③ **其它一律「已就绪」→ 必须跑，出问题就 fail**（401 / 403 / 200 / 500 都算）。
见 §6-发现 2。

**没有**任何一条 skip 是因为「夹具不对」「授权没配好」（D108：那些必须 **fail**）。

---

## 6. QA 用例（TC-014-111 ~ 120）我独立实现的判据

| 用例 | 我实现的判据 | 状态 |
|---|---|---|
| TC-014-111 | 页面渲染出行；空态文案「暂无审计记录」不出现；请求数 ≥ 1 | 单测（列 / 空态）+ E2E（**跳过**：未就绪） |
| TC-014-112 | 列标签集合含 操作人 / 时间 / 动作 / 对象 / 来源 IP（走 i18n，不是硬编码中文）；第一行这 5 格**都非空** | 单测（列标签、零硬编码）+ E2E（**跳过**） |
| TC-014-113 | 翻页 = **一次新请求**，请求带 `cursor` + `limit`、**不带 `page`**；页码指示 1 → 2 | 单测（query 形状、指示文案）+ E2E（**跳过**） |
| TC-014-114 | 每页一次新请求；行**整体替换**（不 append）→ 前端不会自己造重复；两页 id 无交集 | 单测（query 形状）+ E2E（**跳过**） |
| TC-014-115 | 请求带 `filter[actorId][eq]=…`；行的操作人与输入**逐字相同** | 单测（参数）+ E2E（**跳过**） |
| TC-014-116 | 请求带 `filter[action][eq]=…`；行的动作与输入**逐字相同** | 单测（参数）+ E2E（**跳过**） |
| TC-014-117 | 前端发的过滤条件与直接调接口用的**同形**；E2E 直接比对两边的 id 集合 | E2E 写了（**跳过**）；前端侧保证 = 参数同形（单测） |
| TC-014-118 | 最终地址 `/403`；`data-testid="forbidden"` 存在；`/api/audit/events` 请求数 **0** | ✅ **单测 + E2E 双跑**（真跑通过） |
| TC-014-119 | 可见文本非空 + `<h2>403</h2>` + 不含任何审计记录内容 | ✅ **E2E 真跑通过**（复用既有 `Forbidden.svelte`） |
| TC-014-120 | 全文件只有**一种** skip 理由（被测系统未就绪）；**没有**针对「无权限」「403」「数据为空」的 skip | ✅ 人工核对（§5.3 清单） |

---

## 7. 与规格不一致的发现 / 偏离

### 发现 1（⚠️ **需 BA 确认**）：后端**拒绝**「游标 + 任何排序」→ 前端游标模式不发 `sort`

- 后端 `crates/platform-audit/src/query.rs:215`：
  `if parsed.cursor.is_some() && !parsed.sort.is_empty() { return Err(invalid("the cursor is only valid with the default sort order")) }`
- 契约 C7 只规定「`cursor` 与 `page` 互斥」，**没有**这条。
- 前端默认排序就是后端的默认排序（`-createdAt` = `created_at DESC, id DESC`，D155）→
  **不发 `sort` 与发默认排序等价** → 前端按后端口径适配（游标模式不发 `sort`；
  非默认排序 + 游标 → 前端直接抛错，不发非法请求）。
- **要 BA 确认**：这是后端**收紧**了 C7，还是规格漏写？若 BA 认为后端该放开，
  前端把 `sort` 加回去即可（改动点在 `buildAuditQuery` 一个分支）。

### 发现 2（⚠️ **需 BA / 后端确认**）：未注册的 `/api/...` 回落成 SPA → E2E 的就绪探测不能只看状态码

- `crates/platform-server/src/static_files.rs:33`：路径**没有扩展名** → 回落 `index.html`。
  → 未实现的 `GET /api/audit/events` 返回的是 **200 + `text/html`**（**不是 404**）。
- 后果：棒 1 的探测（`status !== 404 && status !== 501` → 「已就绪」）会**误判** →
  数据用例会**红**（而不是按「未就绪」跳过）→ 违反 TC-014-120 的 skip 纪律。
  本轮已把探测改成「**404/501 或响应不是 JSON** → 未就绪」（§5.3）。
- 建议（**属后端文件集，我只报告不改**）：静态回落**跳过 `/api/` 前缀**，
  让未知 API 路径返回 404 → 探测口径回到「状态码即可」，也更符合契约 C6。

### 发现 3：i18n 的精确计数断言要求**两侧同改**（断言本身没动）

- `packages/i18n/test/i18n.test.ts` 的断言是
  「错误域条目数 **==** `codes` 列表长度」→ 只改语言包会红（84 ≠ 79）。
- 按 D170 的原话（「必须两侧同改」），本轮把 5 个码**加进 `codes` 抽样列表**，
  **断言一个字没动**（没放宽、没改阈值）。
- 加完 **43 条 i18n 测试全绿**。

### 发现 4：QA 的 `frontend.md` 是 rev 1 口径（与 D169 / C7 不一致）

- TC-014-113 写「请求参数含 `page=2`」、TC-014-115/116 写「请求带 `actorId=…` / `action=…`」
  —— 已被 D169（游标）与 C7（`filter[field][op]`）推翻。
- 本轮按 **D169 + C7** 实现；E2E 的断言同步改成游标口径。

### 偏离 1（文件边界）：改了 `packages/i18n/test/i18n.test.ts` 的**数据列表**

- 任务卡说「不要改测试断言」—— **断言没改**；改的是 `codes` **抽样列表**（数据），
  这是 D170 明写的前置条件（「两侧同改」），不改它 i18n 测试必红。
- 只加 5 行（`AUDIT_*`），不触碰任何断言与阈值。

### 偏离 2：`apps/shell/src/components/Table.svelte` 加了可选 prop `hidePager`

- `@base/ui` 的 `TableProps`（`packages/ui/**`，**不在**我的文件集）没有「隐藏分页器」的口子，
  而自带的分页器是**页码**语义（「N / M 页」+ 可跳页），与游标分页**语义不符**（D169）。
- 做法：在**壳层封装**上加一个**可选** prop（默认 `false` → 既有页面行为不变），
  `@base/ui` 的契约**一个字没动**；E2E 全量跑过 → 既有页面无回归。

---

## 8. 未完成 / 待别人做的部分

| 项 | 原因 | 归属 |
|---|---|---|
| 审计页数据类 E2E 真跑（TC-014-111 ~ 117） | 后端 `crates/platform-server/src/audit.rs` **尚不存在** → `/api/audit/*` 路由未注册 → 按「被测系统未就绪」跳过（探测已实现，路由一落地就自动真跑） | 后端 Dev |
| `buildAuditQuery` 的**页码模式** | 按 D169「**保留**（C7 允许的兼容路径），只是默认不走」→ 实现与单测都在，未删 | —（有意保留） |
| 「共 N 条记录」随翻页刷新 | 服务端 `total` 是**当前过滤下**的总数（后端 count 查询无条件执行）→ 翻页不变，符合预期 | — |

---

## 9. 复现命令

```bash
cd /home/myg133/Projects/system_seed/feature-REQ-014
export XDG_RUNTIME_DIR=/run/user/$(id -u)

pnpm --filter @base/shell test      # → 206 passed
pnpm --filter @base/i18n test       # → 43 passed
pnpm -r build                      # → 全绿
pnpm --filter @base/sdk test       # → 38 passed
pnpm --filter @base/shell check    # → 0 errors / 0 warnings

# E2E（需要测试库；本机用 podman 里的 base-postgres）
export DATABASE_URL_TEST="postgres://base:base@127.0.0.1:5432/base_test"
./scripts/seed-e2e.sh              # → 夹具就绪（审计夹具 11 条）
./scripts/e2e-server.sh &          # 迁移 → 播种 → base serve :8080
npx playwright test e2e/           # → 30 passed / 5 skipped
```

> ⚠️ `pnpm -r build`、cargo 构建与 playwright **串行跑**（低配机 8 线程，dev-guide §5 / §9）。
