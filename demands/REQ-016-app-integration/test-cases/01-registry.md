# 01 注册表 CRUD / 引用 client / scheme 校验（AC-S-156、AC-S-159）

### TC-016-004 注册应用主路径 → 201 + 回读
- **AC**: AC-S-156 步骤 1-2 ｜ **类型**: 功能 ｜ **优先级**: P0 ｜ **自动化**: 集成 ｜ **方法**: 等价类（合法输入）
- **前置**: `client-A` 存在且 `active`；`u-perm-app` 持 `app:registry:write`；`display_name` 未被占用
- **步骤**: 1) `POST /api/apps` body `{clientId:"client-A", displayName:"报销系统", description:"…", category:"finance", iconUrl:"https://cdn.example/i.png", launchUrl:"https://expense.example/app", sortOrder:10}` 2) 读响应
- **预期**: 201；`appId` 非空且匹配 `^[A-Za-z0-9_-]{8,}$`；`clientId=="client-A"`；`displayName`/`category`/`iconUrl`/`launchUrl`/`sortOrder` **逐字**等于请求；`status=="active"`；`scopes` 为数组；`createdAt` 匹配 RFC3339 微秒 6 位；`createdBy=="u-perm-app@example.com"`；响应**不含** `clientSecret*`

### TC-016-005 GET 列表与单个 → 字段逐字一致
- **AC**: AC-S-156 步骤 3-4 ｜ P0 ｜ 集成 ｜ 等价类
- **前置**: TC-016-004 的应用已建
- **步骤**: 1) `GET /api/apps` 2) `GET /api/apps/{appId}`
- **预期**: ① 响应是**数组**（不是 `{apps:[]}`）；② 数组里含该 `appId`，其字段与 TC-016-004 的响应**逐字**相同；③ 单个响应与列表中该项**逐字**相同

### TC-016-006 PATCH 改 displayName + sortOrder → 200
- **AC**: AC-S-156 步骤 5 ｜ P0 ｜ 集成 ｜ 状态迁移
- **步骤**: 1) `PATCH /api/apps/{appId}` `{displayName:"报销系统 v2", sortOrder:99}` 2) `GET` 回读
- **预期**: 200；回读 `displayName=="报销系统 v2"` 且 `sortOrder==99`；**其它字段不变**（`launchUrl` 仍为原值）

### TC-016-007 DELETE → 404 APP_NOT_FOUND
- **AC**: AC-S-156 步骤 6 ｜ P0 ｜ 集成 ｜ 状态迁移
- **步骤**: 1) `DELETE /api/apps/{appId}` 2) `GET /api/apps/{appId}` 3) 再 `DELETE` 一次
- **预期**: ① 删除返回 204；② `GET` 404 且 `code=="APP_NOT_FOUND"`、`Content-Type: application/problem+json`；③ 第二次 `DELETE` 也是 404 + `APP_NOT_FOUND`（**不是** 500）

### TC-016-008 列集合恰好 12 列 + 不含 6 个 client 列
- **AC**: AC-S-156 步骤 7 ｜ P0 ｜ 集成（`information_schema`）｜ 边界值
- **步骤**: `SELECT column_name FROM information_schema.columns WHERE table_schema='public' AND table_name='platform_app' ORDER BY column_name`
- **预期**: 集合**恰好** = `{category, client_id, created_at, created_by, description, display_name, icon_url, id, launch_url, sort_order, status, tenant_id}`（**12 个**，双向相等，不是「包含」）；且**不含** `client_secret_hash`/`redirect_uris`/`allowed_grant_types`/`allowed_scopes`/`backchannel_logout_uri`/`frontchannel_logout_uri`

### TC-016-009 路径 2a：重复 clientId → 409 APP_DUPLICATED
- **AC**: AC-S-156 步骤 8（前半）｜ P0 ｜ 集成 ｜ 边界值
- **前置**: TC-016-004 的应用存在（已占用 `client-A`）
- **步骤**: `POST /api/apps` `{clientId:"client-A", displayName:"另一个名字", launchUrl:"https://x.example/a"}`
- **预期**: 409 + `code=="APP_DUPLICATED"`

### TC-016-010 路径 2b：重复 displayName（换 client-B）→ 409 APP_DUPLICATED
- **AC**: AC-S-156 步骤 8（后半）｜ P0 ｜ 集成 ｜ 边界值
- **前置**: `client-B` 存在且未被本租户接入；`"报销系统 v2"` 已被占用
- **步骤**: `POST /api/apps` `{clientId:"client-B", displayName:"报销系统 v2"}`
- **预期**: 409 + `APP_DUPLICATED`
- **注**: AC 原文未写 `client-B`（S-05）—— 本条已修正为**显式使用 `client-B`**

### TC-016-011 路径 3：launch_url 5 种非法 + 2 种合法〔AC 修正〕
- **AC**: AC-S-156 步骤 9 ｜ P0 ｜ 集成 ｜ 边界值 + 因果
- **前置**: 每次用**新建的一次性 client**（`client-probe-url-<i>`）与**唯一** displayName（避免 409，S-05）
- **步骤**: 依次 `POST /api/apps`，`launchUrl` = `"not-a-url"` / `"javascript:alert(1)"` / `"data:text/html,<script>alert(1)</script>"` / `"ftp://x.example/a"` / `"http://x.example/a"`（`app.icon.allowHttp=false`）/ **`"https://x.example/a"`（阳性对照）** / **`""`（空串，见预期）**
- **预期**: ① 前 5 次**全部 400 + `APP_LAUNCH_URL_INVALID`**；② **第 6 次 201**（阳性对照 —— AC 原文缺失，否则「全拒」实现也能过，S-11）；③ 空串：按 `design` 未定，**需 BA 拍板**（建议 201 且落库为 NULL）
- **〔AC 修正〕**: 补第 6 次与第 7 次

### TC-016-012 路径 4：icon_url 5 种非法 + 阳性对照〔AC 修正〕
- **AC**: AC-S-156 步骤 10 ｜ P0 ｜ 集成 ｜ 边界值
- **步骤**: 同 TC-016-011，把值填到 `iconUrl`（`launchUrl` 固定 `"https://x.example/a"`）
- **预期**: 前 5 次 400 + `APP_ICON_URL_INVALID`；`"https://cdn.example/i.png"` → **201**

### TC-016-013 路径 5：缺必填 / 类型错 → 400 APP_INVALID_REQUEST
- **AC**: AC-S-156 步骤 11 ｜ P0 ｜ 集成 ｜ 边界值
- **步骤**: 1) 省略 `clientId` 2) 省略 `displayName` 3) `sortOrder:"abc"` 4) `sortOrder: 1e100`（溢出 int4）
- **预期**: 4 次**全部 400 + `APP_INVALID_REQUEST`**；响应为 Problem Details（不是 axum 纯文本）

### TC-016-014 校验顺序钉死（同请求多个错误）〔AC 修正〕
- **AC**: AC-S-156 步骤 8~11（交叉）｜ P0 ｜ 集成 ｜ 判定表
- **步骤**: 用**已占用的 `client-A`** + `launchUrl:"javascript:alert(1)"` + 已有 `displayName` 一次 `POST`
- **预期**: 返回 **`APP_LAUNCH_URL_INVALID`**（顺序：必填/类型 → client 可用 → URL scheme → 唯一性）
- **〔AC 修正〕**: AC 原文没定顺序，两条判据会互斥（S-05）；本条把顺序钉成可判定的

### TC-016-026 GET 单个 → scopes 逐字等于 client.allowed_scopes
- **AC**: AC-S-159 步骤 1-2 ｜ P0 ｜ 集成 ｜ 等价类
- **前置**: `client-A.allowed_scopes = {rbac:role:read}`；应用绑定 `client-A`；以 `u-perm-app`（`app:registry:read`）调
- **步骤**: 1) `GET /api/apps/{appId}` 读 `scopes` 2) `base` 直连 `SELECT allowed_scopes FROM platform_oauth_client WHERE client_id='client-A'`
- **预期**: `scopes` **逐字等于** `["rbac:role:read"]`（顺序也一致；断言**相等**不是「包含」）

### TC-016-027 改 client.allowed_scopes → 响应跟着变（投影非拷贝）
- **AC**: AC-S-159 步骤 3-4 ｜ P0 ｜ 集成 ｜ 因果
- **步骤**: 1) `base` 执行 `UPDATE platform_oauth_client SET allowed_scopes='{rbac:role:read,rbac:role:write}' WHERE client_id='client-A'` 2) 再 `GET /api/apps/{appId}`
- **预期**: `scopes == ["rbac:role:read","rbac:role:write"]`（**变了两项** → 证明是投影）

### TC-016-028 platform_app 无 scope 列
- **AC**: AC-S-159 步骤 5 ｜ P0 ｜ 集成 ｜ 边界值
- **预期**: `information_schema` 里 `platform_app` 的列名**不含** `scopes` / `allowed_scopes`（且不含任何名称里带 `scope` 的列）

### TC-016-029 GET /api/apps 列表的 scopes 同样来自 client〔AC 补充〕
- **AC**: AC-S-159（D266 明写「`GET /api/apps` 的响应里带 `scopes`」）｜ P1 ｜ 集成 ｜ 因果
- **步骤**: 改 client 的 `allowed_scopes` → `GET /api/apps`
- **预期**: 列表中该应用的 `scopes` 同步变化（**列表与单个两条读取路径都要覆盖**，AC 原文只测单个）
