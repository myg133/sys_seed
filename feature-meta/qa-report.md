# REQ-012「元数据驱动渲染」Pre-merge QA 报告

- **需求**: REQ-012（AC-S-076 ~ AC-S-087，共 12 条，rev 2）
- **验证者**: QA（独立复现，不采信 Dev 数字）
- **分支 / worktree**: `feature/REQ-012` @ `/home/myg133/Projects/system_seed/feature-REQ-012`
- **被验提交**: `8990d65`（含 `737b384` 前端 / `e6e092f` 后端 / `8ce359e` demoNote）
- **基线**: `develop` @ `a7f7579`
- **时间**: 2026-11-19
- **方法**: 真库（`base_test`）+ 真进程（`base serve` 8092）+ 真浏览器（Playwright chromium）+ 源码变异（7 处，全部逐字节还原）
- **注**: 本文件替换 REQ-011 的 `qa-report.md`（原文保留在 git 历史 `a7f7579`）

---

## 1. 结论

**FAIL**

实现侧的功能行为我逐条独立复现，**12 条 AC 的功能面全部成立**（含最关键的字段级权限与零代码闭环）。
判 FAIL 的原因不在实现，而在**唯一被要求的自动化证据链**：

1. **假绿**：用仓库自带的 E2E 夹具（`scripts/seed-e2e.sh`）跑新用例，**6 条全部 skip，退出码 0**
   —— AC-S-082/084/085/086 的真浏览器证明为**空**，且 CI 会把它当成通过。
2. **必红**：补上权限后，`e2e/schema-driven.spec.ts` 里「空态」与「零代码」两条用例对**同一实体**
   提出**互斥的数据前置**，任何库状态下必有一条红（我两种状态都复现了）。

另有一处**裁定落点缺失**（AGENTS.md 未写装配层例外，且现文档与实现矛盾）。

> 说明：以下「通过」全部是我自己跑出来的结果；Dev 报告只当线索。

---

## 2. 逐 AC 结论

| AC | 结论 | 证据（我自己跑的） |
|---|---|---|
| **AC-S-076** manifest 声明 schema，非法起不来 | **PASS** | `BASE_MANIFEST_ROOT=/tmp/q12/mroot`（真库 `base_app`）+ spawn `base serve`：<br>· 合法 → `HEALTH_OK` / `HTTP=200`，SIGTERM 退出 143<br>· `type:"currency"` → 退出 1，`1 entity metadata problem(s)`，点名 `schema.demoItem.fields[2].type` + 允许集合<br>· 双违规（type 非法 + key 重复）→ 退出 1，`2 entity metadata problem(s)`，两条分别点名 `fields[2].type` / `fields[4].key`<br>· `fields:[]` → 退出 1，`must not be empty`<br>· 未登记权限点 `sandbox:demo:nope` → 退出 1，`is not declared in any manifest`<br>· `org/notAnEntity` → 退出 1，`declares no entity` |
| **AC-S-077** JSON Schema 自描述且进能力索引 | **PASS** | `capabilities/schema/entity.schema.json` 存在，`required=["label","fields"]`、`additionalProperties:false`；字段 `type.enum = [text,number,boolean,date,reference]` 与 `FieldType::ALL`（`crates/platform-core/src/extension/schema.rs:61`）一致；`manifest.schema.json` 含 `schema` 属性；`cargo xtask capabilities --check` → 退出 0（14 个文件与源码一致，说明是生成物，手改会被覆盖） |
| **AC-S-078** 元数据接口形状 / 顺序 / 404 / 401 | **PASS** | 200；顶层键 = `["api","entity","fields","label","module"]`；`fields` = `[id,title,amount,paid,dueAt,ownerId,secret]`（= manifest 声明顺序）；无 `null` 值；未知实体 → 404 `SCHEMA_ENTITY_NOT_FOUND`；未知模块 → 404 `SCHEMA_MODULE_NOT_FOUND`；无令牌 → 401 `UNAUTHENTICATED` |
| **AC-S-079** 元数据按权限过滤（结构化判据） | **PASS** | 无 `sandbox:demo:secret` 用户：`fields.map(key)` **深等** `[id,title,amount,paid,dueAt,ownerId]`；顶层键集合精确 5 个；`/"secret"\s*:/` **不匹配**；`内部备注` 在响应中**不出现**；<br>授予后**同一令牌的下一个请求**即 7 个字段；撤销后下一个请求回到 6 个 |
| **AC-S-080** 数据侧剔除（键不存在） | **PASS** | 列表每条 `'secret' in item === false`（不是 `null`/`''`）；授予后下一个请求含 `secret`，值与库一致（`库值-1/2/3` ↔ `sandbox_demo_item.secret`）；<br>**创建 / 更新响应同样投影**（临时撤掉写角色的 secret → 201/200 响应均无 `secret` 键）；<br>`limit=1` 时信封仍是 `{items,nextCursor}` 且 `nextCursor` 非空（`project_items` 未把信封剔掉） |
| **AC-S-081** 契约 C-6 | **PASS** | 合规：`cargo test -p platform-contracts --test c6_schema_projection` → 5 passed；`cargo test -p sandbox --test contracts` 含 `contract_c6_schema_projection_is_used` ok；<br>负向：`a_module_without_a_project_call_site_fails_and_names_module_entity_field` 断言 `check_id=="C-6"` 且点名 secret（真实执行，非恒通过）；<br>弱判据 `a_single_call_site_is_enough` 按 design §4.4 通过（**未**据此判失败） |
| **AC-S-082** `SchemaTable` 渲染 | **PASS（行为）** | 真浏览器：表头 `["ID","标题","金额","已付款","到期日","负责人","内部备注"]` 与 `label` 顺序**精确相等**；单元格 `paid=true→是`、`dueAt=2026-03-05→2026-03-05`（无时区换算）、空值→空串、`amount→12`、`ownerId→demo-1`(id 原值)；<br>`nextCursor=null` 时「下一页」`disabled=true`；点击「下一页」发出带 `cursor` 的请求且第二页数据不同（E2E 用例 6 通过）；空态只渲染 `暂无数据`、`table thead` 数量 0。<br>⚠️ 其**自动化交付物**见问题 H-2 |
| **AC-S-083** `SchemaForm` 渲染与校验 | **PASS（步骤 5 不可按字面复现）** | 控件：`text→input[type=text]`、`number→input[type=number]`、`boolean→input[type=checkbox]`、`date→input[type=date]`、`reference→只读文本`、`readonly→只读文本`；<br>必填留空提交 → **0 个 POST**；错误节点 `.base-field-error` 在**同一字段容器内**（count=1），文案 `标题不能为空`，控件 `aria-invalid="true"` + `aria-describedby="schema-error-title"`；<br>合法提交 → 1 个 POST，body = `{"title":…,"paid":false,"secret":""}`（**只含元数据声明的可写字段**）；<br>步骤 5（number 填非数字）在真浏览器**不可达**：`<input type=number>` 会把 `abc` 归零为 `""`，故不会触发校验（见问题 L-6）；该逻辑仅由包级单测覆盖（`packages/schema/test/schema.test.ts:134`） |
| **AC-S-084** 前端不渲染无权字段 | **PASS** | 无权用户：表头 = 6 列（无「内部备注」）；`body.innerText` **不含** `内部备注`（不是 display:none）；表单不渲染该字段；提交 body 不含 `secret`（`formValues()` 只取 `visibleFields()`，`packages/schema/src/index.ts:199-210`） |
| **AC-S-085** 新增同类页面零前端代码 | **PASS** | `GET /api/schema/sandbox/demoNote` → 200，`fields=[id,title,content,pinned,tags]`；<br>`/schema/sandbox/demoNote` 真浏览器**渲染 + 新建/编辑/删除全部成功**（标题/内容/置顶/标签，表头 5 列）；<br>`grep -rn "demoNote\|demo-note" packages/schema/src apps/shell/src` → **无匹配**；<br>`737b384`（加 demoNote 之前）→ `HEAD` 的 `packages/schema/src/**` + `apps/shell/src/**` **零 diff**，逐文件 sha256 相同（`index.ts bd73c1f5…`、`SchemaPage.svelte e22e9176…`、`SchemaTable.svelte f26e01cb…`、`SchemaForm.svelte fd1eb21d…`、`router.ts bba664fa…`） |
| **AC-S-086** 零代码 CRUD 闭环 | **PASS** | 真浏览器：列表 → 新建（唯一标题命中 1 行）→ 编辑（新值命中 1 行）→ 删除确认 → 该行 0 条；<br>请求白名单（减去页面加载基线 `GET /api/config/branding`、`GET /api/rbac/me/permissions`）：`GET /api/schema/sandbox/demoItem`、`GET /api/sandbox/demo-items?limit=20`、`POST /api/sandbox/demo-items`、`PATCH|PUT /api/sandbox/demo-items/{id}`、`DELETE …/{id}` —— 全部匹配 `^/api/(schema|sandbox)/` 或元数据 `api.*`，**无未声明路径** |
| **AC-S-087** 演进容错 | **PASS** | ① 新增字段（路由拦截注入 `extraNew`）→ 多一列「新增字段」，单元格空串，无报错；② 删除字段（注入去掉 `dueAt`）→ **不渲染**、数据里仍带该键**被忽略**、无「未知列」；③ 未知 type `currency` → 整页正常、单元格降级为文本、`console.warn` **恰好一条**且含类型名与字段名；④ 缺非必填字段（`dueAt`/`ownerId` 为 null）→ 空串，整页 `undefined`/`NaN` 均不出现 |

---

## 3. 问题清单

### 🔴 高

#### H-1 ★ E2E 用自带夹具时 6 条全部 skip，退出码 0（假绿）

- **问题**：`scripts/seed-e2e.sh` 只给 `e2e-role-read/write` 授了 `rbac:*` 权限点
  （`scripts/seed-e2e.sh:160-165`），**没有** `sandbox:demo:read/write`。而
  `e2e/schema-driven.spec.ts` 的 `listStatus()`（`:103`）在列表非 200 时 `test.skip(...)`。
  于是新增的 6 条用例**全部跳过**，Playwright **退出 0**。
- **证据**（实跑）：
  ```
  $ DELETE FROM platform_role_permission WHERE ... permission_code LIKE 'sandbox%';
  $ BASE_E2E_URL=http://127.0.0.1:8092 npx playwright test e2e/schema-driven.spec.ts
    -  1 … 表头文本与顺序…（AC-S-082）
    -  2 … 空态…（AC-S-082 步骤 5）
    -  3 … 字段级权限…（AC-S-084）
    -  4 … 零代码…（AC-S-085 / 086）
    -  5 … CRUD 闭环（AC-S-086）
    -  6 … 游标分页（AC-S-082 步骤 4）
    6 skipped
  EXIT=0
  ```
  而实际接口是对的：同一状态下读用户 `GET /api/sandbox/demo-items` 返回 **403**（缺 `sandbox:demo:read`），
  也就是说**不是后端未就绪，是夹具没配**。
- **复现**：干净 `base_test` → `./scripts/seed-e2e.sh` → 起服务 → 跑上述命令。
- **影响**：AC-S-082/084/085/086 的「真浏览器证明」在 CI 里等于**不存在**，且比不存在更坏
  （报告显示 0 failed、exit 0）。这正是本仓 `docs`/`design §6.3` 反复警告的假绿模式。
- **建议**：在 `scripts/seed-e2e.sh` 里给两个角色补 `sandbox:demo:read`（写用户另加 `write`/`secret`）；
  或把 skip 理由从「夹具权限」改成**失败**（未就绪的必需前置不该是 skip）。

#### H-2 ★ 空态用例与零代码用例数据前置互斥 → 套件必红

- **问题**：`e2e/schema-driven.spec.ts:143` 要求 `SECOND_ENTITY`(demoNote) **为空**
  （断言 `.base-schema-empty` 可见且 `table thead` 计数为 0，`:154-156`）；
  同一文件的 `:199` 要求 demoNote **有数据**（断言 `table thead th` 文本等于 `label` 数组，`:217`）。
  两条用例共用同一实体、都不播种/清理数据 → **任何库状态下必有一条红**。
- **证据**（实跑，两次）：
  ```
  # A) demoNote 空
  ✘ e2e/schema-driven.spec.ts:199 零代码…
    Expected ["ID","标题","内容","置顶","标签"]  Received []
    > 217 | expect(await headerTexts(page)).toEqual(...)

  # B) demoNote 有 3 条
  ✘ e2e/schema-driven.spec.ts:143 空态…
    Error: element(s) not found — waiting for locator('.base-schema-empty')
    > 154 | await expect(empty).toBeVisible();
  ```
  全量跑（17 条）结果为 `15 passed / 1 skipped / 1 failed`，`E2E_EXIT=1`。
- **复现**：给夹具补上 `sandbox:demo:read` 后，分别在 demoNote 空 / 非空两种状态跑 `npx playwright test e2e/`。
- **影响**：E2E 门禁**不可能变绿**；同时它掩盖了 H-1（补权限后才暴露）。
- **建议**：两条用例各自**独立幂等播种/清理**（REQ-011 D101 的既有要求）：
  空态用例用一个专用 tenant 或先清空；零代码用例先建 ≥1 条再断言表头。

### 🟡 中

#### M-1 偏差裁定要求写进 AGENTS.md 的「装配层例外」没落，且现有文档与实现**矛盾**

- **问题**：`deviation-ruling.md` §1.6 明确「**必须写进 AGENTS.md**」。实际只改了能力表（27 条），
  `AGENTS.md:85-86` 仍是「**平台 crate 一旦反向依赖业务模块即失败**」——与实现相反。
- **证据**：`crates/xtask/src/deps.rs:63` `pub const ASSEMBLY_CRATES: [&str; 1] = ["platform-server"];`
  （`:151` 在检查集合里排除它），即 `platform-server → sandbox` **合法**；`AGENTS.md:85-86` 说必然失败。
- **影响**：下一个人（或 AI）按文档判断依赖方向会得出错误结论；裁定要求的三处落点少了一处。
- **建议**：在 AGENTS.md 规则 1/2 附近补一张表（平台库 crate ❌ / `platform-server` ✅ 装配层唯一例外）。

### 🟢 低

#### L-1 `capabilities/index.json` 的 `ui_table_component` 未按 design §5.1 同步
- **证据**：`capabilities/index.json` 里仍是「@base/ui 的 Table 契约…勿自造表格渲染」，
  未写成「Table 是**页码分页**的只读表格；**游标分页**请用 `SchemaTable`」（`design-summary.md:305-306` 的「同步要求」）。
- **影响**：按能力索引办事的人会拿 `Table` 去渲染游标分页（它需要 `total`，语义不符）。
- **建议**：改生成器里的目录条目并重生成（`cargo xtask capabilities`）。

#### L-2 每次「编辑」固定发两次写请求（PATCH 405 → PUT）
- **证据**：真浏览器抓包一次编辑产生
  `PATCH /api/sandbox/demo-items/{id}` 然后 `PUT /api/sandbox/demo-items/{id}`
  （`apps/shell/src/lib/schema-api.ts:92-98` 的 405 回退）。
- **影响**：每次编辑都有一次**必然失败**的请求；日志噪声、错误监控误报风险。
- **建议**：元数据 `api` 段声明更新动词（或后端支持 PATCH），去掉「先错后对」。

#### L-3 只读用户仍看到「新建」按钮
- **证据**：无 `sandbox:demo:write` 用户页面上 `新建` 按钮 count=1；点开填完提交得到 403
  `INSUFFICIENT_PERMISSION`。
- **说明**：AC 未要求（`<Can>` 属 REQ-011），不计失败；但与「渲染器只渲染元数据」的体验不一致。
- **建议**：元数据 `api` 段增加写权限点声明，或接 `<Can>`。

#### L-4 `SchemaTable` 在 `<thead><tr>` 内渲染 `<td>`（HTML content model 违规）
- **证据**：`apps/shell/src/components/SchemaTable.svelte:113`（注释解释了「为了不让表头多一列」）。
- **影响**：浏览器容忍，但 HTML 校验/部分无障碍工具会有异议；测试用 `thead th` 判定时依赖这一取舍。
- **建议**：用 `th scope="col"` + `aria-label="操作"` 并把表头断言改为排除操作列，或干脆把操作列移到行尾并用 `colspan` 对齐。

#### L-5 变异 5 暴露的覆盖缺口：组件级测试不覆盖「校验失败不发请求」
- **证据**：`validateForm` 恒返回 `{}` 时，`packages/schema` 5 条红，但
  `apps/shell/test/schema-form.test.ts` **10 passed 仍全绿**（该文件只测渲染，不测提交路径）。
- **影响**：AC-S-083 步骤 3/4 的行为只有包级单测 + 本次的人工浏览器探针兜底，没有自动化回归。
- **建议**：把「留空提交 → 0 请求 + aria-invalid」固化成 E2E（顺带修 H-2 时一起做）。

#### L-6 AC-S-083 步骤 5（number 填非数字）在真浏览器**不可复现**——AC 内部矛盾
- **证据**：实测 `<input type="number">` 里写入 `abc` 后 `inputValue() === ""`
  （浏览器强制归零），因此永远不会触发「金额必须是数字」，会直接以空值提交成功。
- **性质**：不是实现缺陷（AC 步骤 2 又要求 `number → 数字输入`），是**验收条目的自相矛盾**。
- **建议**：BA 二选一——① 步骤 5 改为「服务端拒绝非数字」（集成测试）；② 接受「浏览器层不可达，只有包级单测覆盖」。

---

## 4. 变异验证

每处变异后均 `git checkout --` 还原，并 `sha256sum -c` 逐文件核对（最终 `git status --porcelain` **为空**）。

| # | 变异点 | 预期红 | 实际 | 结论 |
|---|---|---|---|---|
| 1 | `SchemaProjector::project` 对对象直接 `return raw`（`crates/platform-core/src/extension/schema.rs`） | AC-S-080 | `cargo test -p sandbox --test demo_items -- --include-ignored` → **3 failed / 4 passed**（无 secret 键、创建/更新投影、未声明字段） | ✅ 有效 |
| 2 | 元数据接口不过滤（`schema.fields` 代替 `visible_fields`，`crates/platform-schema/src/http.rs`） | AC-S-079 | `cargo test -p platform-schema` → **3 failed / 6 passed** | ✅ 有效 |
| 3 | `check_c6_for` 恒 `Ok(vec![])`（`crates/platform-contracts/src/checks.rs`） | AC-S-081 负向 | `c6_schema_projection` → **3 failed / 2 passed** | ✅ 有效 |
| 4 | `formatCell` 的 boolean 返回 `String(value)`（`packages/schema/src/index.ts`） | AC-S-082 | `@base/schema` → **1 failed / 37 passed** | ✅ 有效 |
| 5 | `validateForm` 恒返回 `{}`（同上） | AC-S-083 | `@base/schema` → **5 failed / 33 passed**（注：`apps/shell` 组件测试**仍全绿**，见 L-5） | ✅ 有效（包级） |
| 6 | 去掉 router 的 `:param` 支持（`apps/shell/src/lib/router.ts`） | AC-S-085 | `@base/shell` → **5 failed / 85 passed**（全部 `router-params`） | ✅ 有效 |
| 7 | `project_items` 改为对**信封整体**投影（`modules/sandbox/backend/src/demo_item_http.rs`） | AC-S-082 信封 / AC-S-080 | `demo_items` → **3 failed**，`left: [] right: ["items","nextCursor"]` | ✅ 有效 |

> 结论：7 处变异**全部**让对应用例变红，没有出现「变异了点还绿」的情况。
> 唯一需要说明的是 #5 在**组件级**测试上保持绿——那是覆盖缺口（L-5），不是变异点选错。

---

## 5. 没验到的（**不要**当成通过）

| 项 | 原因 |
|---|---|
| GitHub Actions 上真实执行 | 本地只跑通了同样的命令；未触发远端 CI |
| CI 的 `e2e` job 本身 | **既有缺陷（非本需求引入）**：`.github/workflows/ci.yml` 的 e2e job 既无 postgres service 也无 `DATABASE_URL_TEST`，`webServer` 会跑 `scripts/e2e-server.sh` 并在缺库时退出 2 → 该 job 在 `a7f7579` 上同样起不来。未在本轮修复，故「E2E 在 CI 能否落地」无法验 |
| AC-S-082「排序」 | AC 标题提到排序，但步骤 1~6 与预期结果**没有**排序判据；实现也没有排序 UI → 无判据可验 |
| AC-S-083 步骤 5 | 见 L-6：真浏览器不可达（AC 自相矛盾），只有包级单测覆盖 |
| 多租户 RLS 隔离在 schema 接口/沙箱接口上的行为 | 不在本需求 AC 范围（REQ-009 既有资产）；只顺手确认了「头租户 ≠ 令牌租户 → 404」的代码路径存在 |
| 非 chromium 浏览器 | 仓库 Playwright 配置只有 chromium（既有选择） |
| 写侧字段级权限 | 验收标准明确**本期不做**（acceptance.md 末表） |
| 生产部署形态（Helm/compose） | 不在本需求范围 |

---

## 6. 实跑数字

| 命令 | 结果 |
|---|---|
| `./scripts/test-integration.sh`（真库，`--include-ignored`） | **exit 0**；33 个 `test result: ok`，合计 **618 passed / 0 failed**；`容器数量比对通过：2 → 2` |
| `pnpm -r check` | exit 0（`svelte-check found 0 errors and 0 warnings`） |
| `pnpm -r build` | exit 0 |
| `pnpm -r test` | exit 0；8 组 **234 passed**（i18n 3 / schema 38 / hooks 17 / sdk 34 / theme 12 / auth 26 / ui 14 / shell 90） |
| `pnpm exec eslint .` | exit 0 |
| `npx prettier --check .` | `All matched files use Prettier code style!` |
| `cargo clippy --workspace --all-targets -- -D warnings` | exit 0，**0 warning** |
| `cargo xtask capabilities --check` | exit 0（**14 个文件**与源码一致） |
| `cargo test -p xtask --all-targets` | **51 passed** |
| `cargo test -p platform-contracts --test c6_schema_projection` | **5 passed** |
| manifest 违规 5 场景（真库 + `base serve`） | 合法 → HTTP 200 / 退出 143；4 类违规 → **退出 1**，issues = **1 / 2 / 1 / 1 / 1** |
| 元数据接口（curl + 结构化断言） | 200 / 5 键 / 7 字段顺序；无权用户 6 字段；404×2 + 401 |
| 数据接口字段级权限 | 3 条记录 `hasOwnProperty('secret') === false`；授权后下一个请求值 = 库值（3/3 一致） |
| Playwright 全量 `e2e/`（17 条） | **15 passed / 1 skipped / 1 failed**，exit 1 |
| Playwright `schema-driven.spec.ts`（自带夹具） | **6 skipped**，exit **0** ← 假绿（H-1） |
| Playwright `schema-driven.spec.ts`（补权限 + demoNote 空） | 5 passed / 1 failed（零代码用例） |
| Playwright `schema-driven.spec.ts`（补权限 + demoNote 3 条） | 5 passed / 1 failed（空态用例） |
| 变异 7 处 | 7/7 变红 |
| 最终 `git status --porcelain` | **空**（无实现改动、无提交） |

> 环境备注：验证用临时夹具（`tenant-a` 的 `e2e-role-read/write` 补 `sandbox:demo:*`）是**通过 RBAC 接口**写入 `base_test` 的测试数据，
> 未改任何仓库文件；验证结束后 8092 服务已停止、端口已释放。
