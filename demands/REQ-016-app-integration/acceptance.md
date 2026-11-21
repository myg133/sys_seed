# 验收标准：应用接入与应用注册表

- **需求编号**: REQ-016
- **父需求**: REQ-001
- **状态**: **rev 4**（2026-11-21 —— QA 挑刺 39 条全部接受后的裁决已落地：**D276~D285**）
- **对应验收标准**: **11 条**（AC-S-024（既有）+ **AC-S-156 ~ AC-S-165**）
  + **1 个「明确不做」清单**（AC-S-163.b，**不是 AC**，是防止后人以为漏了的说明）
- **决策依据**: [decisions.md](decisions.md) **rev 2（D262~D285）**
- **基线代码**: `develop` @ `aa5b305`
- **更新时间**: 2026-11-21

> 父需求的完整验收标准见 [REQ-001 acceptance.md](../REQ-001-platform-foundation/acceptance.md)。
> 本文件只列出本子需求承担的部分。

## rev 3 变更摘要（**Dev 必读**）

rev 2 只有 **1 条 AC**（AC-S-024），而 `demand.md` 列了 **4 项功能要求** →
**1 条 AC 验收不了 4 项功能**。rev 3 补齐到 **11 条**。

**三个实测发现的缺口（在 `develop` @ `aa5b305` 上验的）**：

1. **`manifest.json` 的 `menus[]` 有 schema 校验但没有任何 API 下发它**
   （`crates/platform-core/src/extension/manifest.rs:120` `MenuSpec`；
   `grep -rn "menus" crates/*/src/*.rs | grep -v manifest.rs` → 只有测试夹具里的 `"menus": []`）
   → AC-S-162 是**从零建**的。
2. **前端 `nav.ts` 是硬编码占位**，它自己的注释写着「**本期为占位数据，后续由应用注册表下发**」
   → 本需求就是那个「后续」。
3. **`platform_oauth_client` 没有「应用入口 URL」列**（只有 `redirect_uris` + 两个 logout URI）
   → 必须新增 `platform_app.launch_url`（D269）。

---

## 全局约定

| 项 | 约定 | 依据 |
|---|---|---|
| 应用连接角色 | **`base_app`**（`base_platform_app` 是 NOLOGIN 组角色，连不上） | D165 |
| 造数 / 篡改连接 | **`base`**（表 owner + 超管） | — |
| 库 | **`base_test`**（绝不写 `base_dev`） | dev-guide §9.8 |
| `app.tenant_id` | **事务局部**；凡涉及租户上下文的查询**必须在单个事务内** `set_config` → 查询 | D148 |
| 错误体 | **一律 Problem Details**（RFC 9457，契约 C6）—— 本需求**没有**协议端点 | 契约 C6 |
| 字段名 | **camelCase**（契约 C13） | D167 |
| 时间格式 | UTC、RFC3339、**微秒 6 位** | D142 |
| 静态扫描 | 必须带**阳性对照**（否则「0 命中」不可信） | dev-guide §9.2 |
| 判据落点 | 报告必须指到 `file:line` 的 `assert!`，且**同时给函数名** | **dev-guide §9.15 / §9.15.1** |

### 本需求的平台错误码（**6 个**）

| 码 | HTTP | 何时 | 哪个 AC 触发它 |
|---|---|---|---|
| `APP_NOT_FOUND` | 404 | 应用不存在 | AC-S-156 步骤 6 |
| `APP_DUPLICATED` | 409 | 同租户内 `client_id` 或 `display_name` 重复 | AC-S-156 **路径 2** |
| `APP_CLIENT_INVALID` | 400 | 引用的 client 不存在 / 已停用 / 已过期 | AC-S-158 路径 1/2 |
| `APP_LAUNCH_URL_INVALID` | 400 | `launch_url` 的 scheme 非法 | AC-S-156 **路径 3** |
| `APP_ICON_URL_INVALID` | 400 | `icon_url` 的 scheme 非法 | AC-S-156 **路径 4** |
| `APP_INVALID_REQUEST` | 400 | 缺必填字段 / 类型错 | AC-S-156 **路径 5** |

> ⚠️ **rev 3 修正：错误码从 8 个**删到** 6 个**（删掉 `APP_REGISTRY_MISMATCH` 与
> `APP_INVALID_SCOPE_PROJECTION`）。
>
> **理由与 REQ-022 的 D226 完全同款**：一个**没有任何 AC 会触发它**的码 = **死码**，
> 却要在 i18n 词库占一条、在 `TITLES` 占一行。
>
> - **`APP_REGISTRY_MISMATCH`**：**启动对账失败时进程直接退出，根本没有 HTTP 响应**
>   → 它不可能是 HTTP 码（AC-S-165 步骤 2 的判据是「**退出码非 0** + 日志列出缺失编码」，
>   不是「返回某个码」）。**删除**。
> - **`APP_INVALID_SCOPE_PROJECTION`**：scope 是**只读投影**（D266），
>   应用表根本不存 scope → 这个「投影与 client 不符」的状态**构造不出来**。**删除**。
>
> → **规则（再次）**：**每个错误码必须有一个 AC 会真的触发它**，否则不许登记。
> → `TITLES` 行数：**93 → 99**（58 + 21 + 5 + 9 + **6**），**不是 101**。
>
> ⚠️ 事件码（**3 个** `app.*`：`app.registered` / `app.updated` / `app.deleted`）
> **不进 `TITLES`**（D223 同款口径）。

---

## 本需求自有 AC

---

### AC-S-024: 应用可撤销授权：用户撤销后，该应用经**内省**校验的令牌**立即**失效

> ⚠️ **rev 2 已按 REQ-022 的 D210 改写**（原文见 rev 1）。本条**不变**，只在 rev 3 里补上
> 「**REQ-022 已交付，本条现在是回归判据**」的说明。

- **前置条件**: `tenant-a` 下有用户，已对 `client-A` 授权；`client-A` 持有一枚有效 access token；
  平台元数据可读（`oauth.accessTokenTtlSeconds`）。
- **测试步骤**:
  1. `POST /api/oauth/grants/revoke`（**REQ-022 的端点**）；
  2. 内省那枚令牌：`POST /api/oauth/introspect`；
  3. 第三方用 JWKS 验签那枚令牌，读 `exp`；
  4. 读元数据里的 `access_token_expires_in`。
- **预期结果**:
  - 步骤 2：`active = false` —— **立即失效**（不等待任何 TTL）；
  - 步骤 3：签名**仍然有效**（`VerifyOutcome::Valid`）—— 证明撤销**没改**令牌本身；
  - 步骤 4：**`exp - iat` 与 `access_token_expires_in` 逐字节相等**
    （⚠️ **不再是**「距撤销时刻 ≤ TTL」—— 那**数学恒真**、零区分力，见 **D285 / S-10**）。
  - **三条判据互相独立**：①②③ 任一条被改坏都会红。
- **类型**: 安全
- **⚠️ 本条在 rev 3 里的定位**：REQ-022 的 AC-S-143 / 144 / 147 已验过同一件事。
  本条是**跨需求的端到端回归判据** —— 在**应用注册表存在之后**再验一遍，
  证明本需求**没有**破坏它。

---

### AC-S-156: 应用注册表可增删改查，且**引用**（不复制）client 的敏感字段

- **前置条件**:
  - `tenant-a` 下有管理员账号 `u-perm-app@example.com`，
    **持 `app:registry:read` / `app:registry:write` / `app:registry:delete`**
    —— **由 `scripts/seed-e2e.sh` 播种**；
  - 已存在 `client-A`（REQ-022 的 client，`confidential`）；
  - ⚠️ **路径 2~5 的前置（D280 / S-05）**：**每次调用前先新建一个专用 client**
    `client-probe-<n>`（用 REQ-022 的 `POST /api/oauth/clients` 建，`n` 递增），
    **且每次用唯一的 `displayName`**（`探针-<n>`）
    —— 否则会先撞 `APP_DUPLICATED`（409），**测的不是目标错误码**。
- **测试步骤**（**主路径 + 5 条独立路径**）:
  1. （主）`POST /api/apps`，body 含 `clientId` / `displayName` / `description` / `category` /
     `iconUrl` / `launchUrl` / `sortOrder`；
  2. （主）读响应；3. `GET /api/apps` 列出；4. `GET /api/apps/{appId}` 读单个；
  5. （主）`PATCH /api/apps/{appId}` 改 `displayName` 与 `sortOrder`；
  6. （主）`DELETE /api/apps/{appId}` 后再查；
  7. （主）**查 `platform_app` 的列集合**（`information_schema.columns`）；
  8. **路径 2（重复）**：用**同一个 `clientId`** 再 `POST` 一次 → 再改用**同一个 `displayName`**
     但**另一个** `clientId` `POST` 一次；
  9. **路径 3（`launch_url` 的 scheme）**：连打 **5 次** `POST`，`launchUrl` 依次为
     `"not-a-url"` / `"javascript:alert(1)"` / `"data:text/html,<script>alert(1)</script>"` /
     `"ftp://x.example/a"` / `"http://x.example/a"`（**默认配置**下 `app.icon.allowHttp = false`）；
  10. **路径 4（`icon_url` 的 scheme）**：同路径 3 的 5 种取值，改成填 `iconUrl`；
  11. **路径 5（缺必填）**：分别省略 `clientId` / `displayName` 各 `POST` 一次，
      再传 `sortOrder: "abc"`（类型错）一次。
- **预期结果**:
  - 步骤 2：**201 CREATED**，含 `appId`（非空、唯一）、`clientId`、`displayName` …；
  - 步骤 3/4：字段**逐字**回读一致；
  - 步骤 5：**200**，改后的值生效；
  - 步骤 6：`GET` 返回 **404** + `APP_NOT_FOUND`；
  - 步骤 7：列集合**恰好**是 D265 那 **12** 列 ——
    ⚠️ **不含** `client_secret_hash` / `redirect_uris` / `allowed_grant_types` /
    `allowed_scopes` / `backchannel_logout_uri` / `frontchannel_logout_uri`（**引用而非复制**）；
  - 步骤 8：**两次都是 409** + `APP_DUPLICATED`；
  - 步骤 9：**5 次全部 400** + `APP_LAUNCH_URL_INVALID`
    ⚠️ **`javascript:` 与 `data:` 必须被拒**（前者是脚本注入、后者可嵌 HTML → 打开即 XSS）
    → 这条是**安全判据**，不是格式洁癖；
  - 步骤 10：**5 次全部 400** + `APP_ICON_URL_INVALID`（同上）；
  - 步骤 11：**3 次全部 400** + `APP_INVALID_REQUEST`。
- **类型**: 功能 / 安全

> ⚠️ **rev 3 补充说明**：步骤 8~11 是**为了让 4 个错误码都有触发点**而加的
> （原先 8 个码里有 6 个没有任何 AC 会触发 = 死码，见 §全局约定 的修正说明）。
> 其中**步骤 9 的 `javascript:` / `data:` 是安全判据** —— `launch_url` 由租户管理员填，
> 但用户点击时是**整页跳转**（`kind="external"`，D269）→ 若允许 `javascript:`，
> 等于给了租户管理员一个对**本租户所有用户**执行脚本的入口。

---

### AC-S-157: 应用注册表**租户隔离**

- **前置条件**: `tenant-a` 与 `tenant-b` 各有 1 个应用（**由 `scripts/seed-e2e.sh` 播种**）；
  `crates/platform-app/manifest.json` 含 `tenantScoped` 段。
- **测试步骤**:
  1. 以 `tenant-a` 上下文查 `platform_app` 与 `platform_app_access`；
  2. 以 `tenant-b` 上下文查同样两张表；
  3. 以 `base`（超管）查全部行；
  4. 以 `base_app` 角色（**不**手动 `set_config`）查 → 再在**单个事务内** `set_config` → 查；
  5. **写侧**：以 `base_app`，单个事务内 `set_config('app.tenant_id','tenant-a')` 后
     `INSERT` 一行 `tenant_id='tenant-b'` → 再 `INSERT` 一行 `tenant_id='tenant-a'`；
  6. `cargo xtask tenant-policies --check`；查 `pg_policies`。
- **预期结果**:
  - 步骤 1/2：各自只看到自己租户的行；
  - 步骤 3：能看到**两租户**的全部行（证明步骤 1/2 不是「表本来就空」）；
  - 步骤 4：不 `set_config` → **0 行**；单事务内 `set_config('tenant-a')` → 有行；
  - 步骤 5：插 `tenant-b` 的行 → **失败**（RLS 的 `WITH CHECK` 拒绝）；插 `tenant-a` → **成功**；
  - 步骤 6：退出码 **0**，且 `pg_policies` 里这 **2 张表各 1 条** `<table>_tenant_isolation` policy。
- **类型**: 安全

---

### AC-S-158: 应用**必须绑定存在且可用的** client；client 删除 → 应用**级联删除**

- **前置条件**: `platform_app.client_id` 是外键，`ON DELETE CASCADE`（D264）。
- **测试步骤**（**四条独立路径**）:
  1. `POST /api/apps` 用一个**不存在**的 `clientId`；
  2. **新建**一次性 client `client-lifecycle-probe`（**不用共享夹具 `client-A`**，D281 / S-06）
     → 把它的 `status` 改成 `disabled`（REQ-022 的 `PATCH /api/oauth/clients/{id}`）
     → 再 `POST /api/apps` 绑定它；
  3. 把它改回 `active` → 建一个应用 → `DELETE /api/oauth/clients/client-lifecycle-probe`；
  4. 查 `platform_app` 里步骤 3 那行；
  5. **回归**：`GET /api/oauth/clients/client-A` 仍 **200**（**共享夹具未被碰**）。
- **预期结果**:
  - 路径 1：**400** + `APP_CLIENT_INVALID`；
  - 路径 2：**400** + `APP_CLIENT_INVALID`（**停用的 client 不能接入**）；
  - 路径 3：删除 client **成功**（**204**，且**不**因外键受阻）
    → **证明没有改坏 REQ-022 的 AC-S-135 步骤 7**；
  - 路径 4：该应用行**已被级联删除**（按 `appId` 查 `count == 0`）；
  - 路径 5：`client-A` **仍然存在**（200）—— 证明本条用例**没有污染共享夹具**
    （D281 / S-06 的根因就是原写法会删掉它）。
- **类型**: 功能 / 安全

---

### AC-S-159: 应用级权限声明 = client `allowed_scopes` 的**只读投影**，不得超出

- **前置条件**: `client-A` 的 `allowed_scopes` = `["rbac:role:read"]`（**取自 client 表**）。
- **测试步骤**:
  1. `GET /api/apps/{appId}`（绑定 `client-A`）→ 读 `scopes`；
  2. 以 `base` 直连查 `platform_oauth_client.allowed_scopes`（`client-A`）；
  3. 以 `base` 直连把 `client-A` 的 `allowed_scopes` 改成 `["rbac:role:read","rbac:role:write"]`；
  4. 再 `GET /api/apps/{appId}` → 读 `scopes`；
  5. 查 `platform_app` 的列集合里有没有 scope 相关的列。
- **预期结果**:
  - 步骤 1：`scopes` **逐字等于** `["rbac:role:read"]`；
  - 步骤 4：`scopes` **跟着变成**两项 → **证明是投影不是拷贝**（若是拷贝，改了 client 它不会变）；
  - 步骤 5：**没有** `scopes` / `allowed_scopes` 列（D266 / D270 的判据）。
- **类型**: 安全

---

### AC-S-160: 工作台清单：`active` 过滤 + **确定性排序** + 分组

- **前置条件**（**由 `scripts/seed-e2e.sh` 播种**，`tenant-a`）:
  - 5 个应用：`A`(category=finance, sortOrder=20, active) /
    `B`(category=finance, sortOrder=10, active) /
    `C`(category=general, sortOrder=0, active) /
    `D`(category=general, sortOrder=0, active) /
    `E`(category=general, sortOrder=0, **status=hidden**)。
- **测试步骤**:
  0. **前置断言（D276 / S-01）**：
     `SELECT count(*) FROM platform_config WHERE key LIKE 'app.%' AND scope = 'platform'` == **3**
     —— 证明 3 个配置键**都是平台级**（漏写 `scope` 会落到 tenant，平台端点直接拒）；
  1. `GET /api/apps/workspace`（用户会话）；
  2. 连打 **3 次**，逐次比较 `groups` 的 JSON；
  3. 读 `groups` 的结构与顺序；
  4. 查配置 `app.workspace.maxApps` 的当前值（应为 **60**）；
  5. 以 `base` 执行 **`SELECT platform_config_upsert('app.workspace.maxApps', '2'::jsonb);`**
     （**必须用这个函数** —— 直连 `UPDATE` 不会 bump epoch，改了不生效，D282 / S-07）
     → **立刻**（不 sleep）`GET`。
- **预期结果**:
  - 步骤 1：**200**；
  - 步骤 2：**3 次响应逐字节相同**（`serde_json` 序列化后比较字符串）→ **确定性排序**；
  - 步骤 3：
    - `groups` **恰好 2 组**：`finance` 在前、`general` 在后（category 字典序升序）；
    - `finance` 组内顺序 **B → A**（`sortOrder` 10 < 20）；
    - `general` 组内顺序 **C → D**（`sortOrder` 相同 → 按 `displayName` 升序）；
    - **E 不在任何组里**（`status=hidden` 被过滤）；
  - 步骤 5：应用总数**恰好 2**（**不是**「不超过 2」—— 那容许 0/1，零区分力，S-18），
    且是全局 `(sortOrder, displayName)` 排序后的**前 2 条**（钉死是哪两条，S-19）；
  - ⚠️ **工作台不按 `launch_url` 过滤**（D279 / S-04）：播种**不必须**给 `launch_url`。
    工作台是「接入了哪些应用」的**总览**；导航才是「点了去哪」（那里才要求非空）。
- **类型**: 功能

---

### AC-S-161: 常用应用 / 最近访问（UPSERT + **只返回在册且 active 的**）

- **前置条件**: `tenant-a` 用户对 3 个 active 应用（`A`/`B`/`C`）有访问记录
  （`app.recentCount` 默认 6）。
- **测试步骤**:
  1. `POST /api/apps/{A}/access` → `POST /api/apps/{B}/access` → `POST /api/apps/{A}/access`
     （**A 访问两次**）；
  2. `GET /api/apps/recent`；
  3. `GET /api/apps/frequent`；
  4. `POST /api/apps/{A}/access` 再打一次 → 查 `platform_app_access` 里 A 那行；
  5. 把应用 `B` 的 `status` 改成 `hidden` → 再 `GET /api/apps/recent`；
  6. 以 `base` 执行 **`SELECT platform_config_upsert('app.recentCount', '1'::jsonb);`**（D282 / S-07）
     → **立刻** `GET /api/apps/recent`。
- **预期结果**:
  - 步骤 2：`recent` 的**第一条是 A**（最后访问）；
  - 步骤 3：`frequent` 的**第一条是 A**（`access_count = 2` 最大）；
  - 步骤 4：A 那行 `access_count` **变成 3**（UPSERT 累加，**不是**插入新行）
    → 且 `platform_app_access` 里 **`WHERE tenant_id='tenant-a' AND user_id=<该用户>`**
    的行数**仍为 3**（⚠️ 必须绑定 `user_id` —— 不绑会被别的用例的行污染，S-28）；
  - 步骤 5：`B` **不在** `recent` 里（`hidden` 被过滤）；
  - 步骤 6：`recent` 长度 **== 1**（配置生效）。
- **类型**: 功能

---

### AC-S-162: 导航下发 API：两类来源合并 + **按权限点过滤** + `kind` 区分

- **前置条件**:
  - `tenant-a` 下有一个绑定 `client-A`（`launch_url = "https://expense.example/app"`）的 active 应用；
  - 平台模块的 `menus[]` 里至少有一个带 `perm` 的项（`audit` 的 `/admin/audit` + `audit:events:read`）；
  - 两个账号：`u-perm-audit@example.com`（**持** `audit:events:read`）、
    `u-perm-read@example.com`（**不持**）；外加 `u-perm-none@example.com`（**无任何权限点**）。
- **测试步骤**:
  1. 用 `u-perm-audit` 调 `GET /api/apps/navigation`；
  2. 用 `u-perm-read` 调同一端点；
  3. 用 `u-perm-none` 调同一端点（**验 `perm` 缺省的项仍然下发**）；
  4. 读响应里每一项的 `source` / `kind` / `target` / 字段集；
  5. 查应用那项的 `target` 与 `platform_app.launch_url` 的关系；
  6. `DELETE /api/apps/{appId}/launch-url`（**专用端点**，D283 / S-08）→ 再调。
- **预期结果**:
  - 步骤 1：`items` 里**既有** `source="module"` 的项（含 audit 那条）、**也有** `source="app"` 的项；
  - 步骤 2：`source="module"` 里**没有** audit 那条；但 `source="app"` 的项**仍在**
    （应用不是权限点）；
  - 步骤 3：`platform-app` 自己的「工作台」项（`menus[].perm` **缺省**）**仍出现**
    —— 证明「`perm` 缺省 = 登录即可见」（D269）；
  - 步骤 4：**字段集按来源分别钉死**（D284 / S-09 —— `MenuSpec` 只有 4 个字段，
    **没有** `labelKey` / `category` / `sortOrder` / `key`）：

    | 来源 | 字段（**恰好这些，不多不少**） |
    |---|---|
    | `module` | `source` / `label` / `icon` / `target` / `kind` / `requiredPermission` |
    | `app` | `source` / `appId` / `label` / `icon` / `target` / `kind` / `requiredPermission` |

    - `module` 项：`kind == "internal"`、`target` 是**站内路径**（以 `/` 开头）、
      `label` 是 **manifest 的中文文案**（**不是翻译键**）；
    - `app` 项：`kind == "external"`、`target` 是 `https://` 开头的**外部 URL**、
      `requiredPermission == null`；
  - 步骤 5：`target` **逐字等于** `launch_url`；
  - 步骤 6：**204**，且该应用**不出现在** `items` 里（没入口不下发）。

- **排序**（确定性，D284）: `module` 项**在前**、`app` 项**在后**；各组内按 `label` 升序。
- **类型**: 功能 / 安全

> ⚠️ **rev 4 的三处修正**（QA 的 S-08 / S-09 / S-24）：
> 1. **`label` 不是 `labelKey`** —— `MenuSpec` 只有 `label`（中文文案）。
>    → **代价**：模块菜单**不随语言切换** → 记 **TD-049**。
> 2. **取消 `category` / `sortOrder` / `key`** —— `MenuSpec` 里不存在；`platform_app` 的
>    `category`/`sort_order` **只影响工作台**，不影响导航排序。
> 3. **置空 `launch_url` 用专用端点** `DELETE /api/apps/{appId}/launch-url`
>    —— `Option<String>` 下 `PATCH` 的 `null` 与「省略键」**分不开**（D283）。

---

### AC-S-163: 前端**运行时**路由注册表 —— `routesFromNavigation` 纯函数 + `external` 走整页跳转

> 🔴 **rev 4 重写（D278 / S-03）**。原文要求「点击导航项 → **落到对应页面**（渲染新组件）」——
> **在现有壳层里做不到**，四个事实（BA 实测）：
>
> | 事实 | 证据 |
> |---|---|
> | `RouteDefinition` **无 component 字段** | `apps/shell/src/lib/router.ts:41-62` |
> | `createRouter` **捕获数组引用** | `router.ts:156` `const { routes } = options` |
> | `Router` **无 `addRoute` / `setRoutes`** | `router.ts:113` |
> | 页面渲染是**逐路径硬编码** | `App.svelte:221-266`，**10 个** `{:else if}`，兜底 `<Home/>` |
>
> **根因**：第三方应用是**独立站点**（`launch_url`），**没有本 SPA 的组件** → 天生不该进 SPA 路由表。
> → 见 D278；将来真要「后端下发页面」需要三件事（**不在本需求范围**）→ **TD-048**。

- **前置条件**: `apps/shell/dist` 已构建；导航 API 可返回 `kind="internal"` 与 `kind="external"` 两类项。
- **测试步骤**（**三条独立判据**）:
  1. **函数级**：单测 `routesFromNavigation(items)` ——
     传入 **2 个 `internal`** 项（`target` 分别为 `/a`、`/b`，`label` 分别为 `A`、`B`）
     **+ 1 个 `external`** 项（`target` = `https://x.example/`）；
  2. **函数级**：只传 1 个 `external` 项；
  3. **入口级（E2E）**：登录 → 打开工作台 → 找到 `external` 应用项 → 点击；
  4. **回归级**：`git diff --stat <本需求基线>..HEAD -- apps/shell/src/lib/router.ts`。
- **预期结果**:
  - 步骤 1：返回 **恰好 2 条**路由；`[0].path == "/a"` 且 `[0].title == "A"`、
    `[1].path == "/b"` 且 `[1].title == "B"`
    （⚠️ `title` 用 **`label` 的文案**，不是翻译键 —— D284）；
  - 步骤 2：返回 **空数组** `[]`
    ⚠️ **这条是关键**：若把 `external` 也塞进 SPA 路由表，
    `resolve()` 会拿外部 URL 去 `match()` → **静默失败**；
  - 步骤 3：`page.url()` **逐字等于** `https://x.example/`（`page.route` 拦截后返 200 桩页）；
    且**没有**触发 SPA 内部导航（`history.pushState` 未被调用 → 是**整页跳转**）；
  - 步骤 4：输出**为空**（`router.ts` **零改动**）。
- **类型**: 功能 / 可维护性

### AC-S-163.b: **明确不做**的事项（写进规格，防止「后面以为漏了」）

| 不做 | 为什么 |
|---|---|
| ❌ 「点导航项 → 落到 SPA 内的新页面」 | `RouteDefinition` 无组件槽；`App.svelte` 硬编码 10 处 if/else → **做不到** |
| ❌ 「运行时把新路由注入路由器」 | `createRouter` 捕获数组、`Router` 无 `addRoute` → **做不到** |
| ❌ 「微前端容器（qiankun / wujie）」 | ADR-0001 D4 明确**只预留接口不实现** |

→ 若要真做，需要**三件事同时**（**记 TD-048**）：
① `RouteDefinition` 加 `component`；② `App.svelte` 改按 `route.component` 查表渲染；
③ `Router` 加 `addRoute` 或把 `routes` 改 getter。

---

### AC-S-164: 应用注册/修改/删除产生**审计记录**

- **前置条件**: 审计 sink 已接真实库（`crates/platform-server/src/main.rs` 的装配路径，REQ-014 D150）。
- **测试步骤**:
  1. 走一遍：注册应用 → 修改 → 删除；
  2. 查 `platform_audit_log`（`tenant-a`）；
  3. 查每条记录的字段集（契约 C12）。
- **预期结果**:
  - 至少产生这 **3** 个事件码（D215 同款命名规范：三段式 `app.*`）：
    `app.registered` / `app.updated` / `app.deleted`；
  - 每条含 `actor_id` / `action` / `object_type` / `object_id` / `ip` / `user_agent` /
    `created_at` / `prev_hash` / `hash`（契约 C12 的固定字段集）；
  - `object_type` **恰好**是 `app`；
  - `object_id` 是该应用的 `appId`；
  - **哈希链自洽**：`ChainVerifier::verify(tenant-a)` → `ok == true`。
- **类型**: 安全

---

### AC-S-165: 权限点三处齐、启动对账 **fail-fast**、C-5 在本 crate 上通过

- **前置条件**: 本需求新增了 `crates/platform-app/manifest.json` 与权限点种子迁移。
- **测试步骤**:
  1a. `cargo xtask capabilities` **重新生成**；
  1b. `cargo test -p platform-app --test contract_c5`（**该测试文件由本需求新增**）；
  2. **只改 manifest、不加种子 INSERT**，起服务；
  3. 把 manifest 里的某个权限点从代码的 `require_permission` 调用点**删掉**（改成非字面量），再跑 1b；
  4. 在 `crates/platform-app/src/` 里放一个叫 `authorize` 的函数，再跑 1b。
- **预期结果**:
  - 步骤 1a：`capabilities/` 下**恰好 14 个文件**（**不增不减**）；
    ⚠️ **实测依据**（`crates/xtask/src/capabilities.rs:57-79` 的 `generate`）：
    生成器固定产出 8 个（`permissions` / `extension-points` / `schema/{manifest,metadata,entity}` /
    `api` / `index`）+ 前端包各一个 `sdk/<pkg>.json`（**6 个**）= **14**。
    **新增 Rust crate 只让 `permissions.json` 里多一个 module 条目，不新增文件**
    （⚠️ 不要凭直觉写「新增 crate 就多一个文件」—— BA 在本轮写规格时差点这么写，实测纠正）；
    且 `capabilities/permissions.json` 的 `total == 35`（32 + 3 个 `app:*`），
    且 `modules` 里**新增**一个 `platform-app` 条目（`manifest` 字段 = `crates/platform-app/manifest.json`），
    且含 `app:registry:read` / `app:registry:write` / `app:registry:delete` **逐字 3 条**；
  - 步骤 1b：**通过**（真 C-5 命令）；
  - 步骤 2：服务**起不来**（退出码非 0），日志**逐条**列出缺失的权限点编码与
    来源 manifest 路径 `crates/platform-app/manifest.json`；
  - 步骤 3：C-5 **失败**（已登记但无鉴权调用点）→ **阳性对照**；
  - 步骤 4：C-5 **失败**（出现第二个鉴权入口）→ 证明命名禁区有效。
- **类型**: 可维护性

---

## 完成定义对照

| 能力 | AC |
|---|---|
| 应用注册表（增删改查 + 租户隔离 + 引用 client） | 156 / 157 |
| 应用与 client 的生命周期绑定 | 158 |
| 应用级权限声明（只读投影） | 159 |
| 工作台 / 应用市场 | 160 / 161 |
| 菜单下发（两类来源 + 按权限过滤） | 162 |
| 前端运行时路由注册表 | 163 |
| 审计埋点 | 164 |
| 权限点 / C-5 / 启动对账 | 165 |
| 撤销后令牌失效（回归） | 024 |
