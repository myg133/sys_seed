# 02 租户隔离 / client 生命周期 / 撤销回归（AC-S-157、AC-S-158、AC-S-024）

### TC-016-001 撤销后内省立即 active=false
- **AC**: AC-S-024 步骤 1-2 ｜ **类型**: 安全 ｜ P0 ｜ 集成 + 真 HTTP ｜ 状态迁移
- **前置**: `tenant-a` 用户已对 `client-A` 授权（`POST /api/oauth/authorize/confirm`）；持一枚有效 access token
- **步骤**: 1) `POST /api/oauth/introspect`（撤销前，取基线） 2) `POST /api/oauth/grants/revoke` 3) **立即**（不 sleep）再内省
- **预期**: 撤销前 `active==true`；撤销后 `active==false`（不等待任何 TTL）

### TC-016-002 验签侧 exp 未被撤销改动〔AC 修正〕
- **AC**: AC-S-024 步骤 3-4 ｜ P0 ｜ 集成 ｜ 因果
- **前置**: 同 TC-016-001；记录签发时刻 `iat`（解 JWT payload 的 `iat`）
- **步骤**: 1) 撤销前后各解一次 JWT（**只用 JWKS 验签，不调内省**） 2) 读 `exp`、`iat` 3) 读元数据 `access_token_expires_in`
- **预期**: ① 撤销后签名**仍然有效**；② **`exp - iat == access_token_expires_in`**（逐字节相等）；③ `exp > 撤销时刻`
- **〔AC 修正〕**: 原文「`exp` 距撤销时刻 ≤ TTL」**恒真**（撤销必然晚于签发，S-10）→ 零敏感度；改成上面的双向等式

### TC-016-003 应用注册表存在后，撤销路径未被破坏（跨需求回归）
- **AC**: AC-S-024（rev 3 的定位）｜ P1 ｜ 集成 ｜ 回归
- **前置**: `platform_app` 里有绑定 `client-A` 的 active 应用
- **步骤**: 重跑 TC-016-001 + 再 `GET /api/apps/{appId}` 确认应用仍在
- **预期**: 内省结果与 TC-016-001 一致；应用行**未被**撤销动作影响（撤销 grant ≠ 删 client）

### TC-016-015 tenant-a 上下文只看到自己租户的行
- **AC**: AC-S-157 步骤 1 ｜ **类型**: 安全 ｜ P0 ｜ 集成（`base_app`）｜ 等价类
- **前置**: `tenant-a` 有 ≥1 个应用与 ≥1 条访问记录；`tenant-b` 同理
- **步骤**: 单事务内 `set_config('app.tenant_id','tenant-a',true)` → `SELECT tenant_id, count(*) FROM platform_app GROUP BY 1`；对 `platform_app_access` 同样
- **预期**: 两张表都只有 **`('tenant-a', N)`** 一行，且 `N>=1`；结果集里**不出现** `'tenant-b'`

### TC-016-016 tenant-b 上下文只看到自己租户的行
- **AC**: AC-S-157 步骤 2 ｜ P0 ｜ 集成 ｜ 等价类
- **预期**: 与 TC-016-015 对称：只有 `('tenant-b', M)`，`M>=1`

### TC-016-017 base 能看到两租户全部行（非空对照）
- **AC**: AC-S-157 步骤 3 ｜ P0 ｜ 集成（`base`）｜ 对照
- **预期**: `count(DISTINCT tenant_id) == 2`，且 `count(*) >= N+M`（证明前两条不是「表本来就空」）

### TC-016-018 base_app 不设上下文 → 0 行；单事务设上下文 → 有行
- **AC**: AC-S-157 步骤 4 ｜ P0 ｜ 集成 ｜ 状态迁移
- **步骤**: 1) `base_app` 连接（**不** `set_config`）`SELECT count(*) FROM platform_app` 2) 同一连接单事务内 `set_config('app.tenant_id','tenant-a',true)` 再查
- **预期**: ① **0**；② `>=1`（`fail closed` 语义）

### TC-016-019 写侧 WITH CHECK：跨租户 INSERT 被拒、本租户 INSERT 成功
- **AC**: AC-S-157 步骤 5 ｜ P0 ｜ 集成 ｜ 判定表
- **步骤**: `base_app` 单事务 `set_config('app.tenant_id','tenant-a',true)` 后：
  1) `INSERT INTO platform_app (id,tenant_id,client_id,display_name,created_at,created_by) VALUES ('t-b-probe','tenant-b','client-B','跨租户探针',now(),'probe')`
  2) 同事务再插一行 `tenant_id='tenant-a'`
- **预期**: ① 第 1 条**失败**，SQLSTATE **42501** 且报错含 `row-level security`；② 第 2 条**成功**
- **〔AC 修正〕**: 原文只写「INSERT 一行 `tenant_id='tenant-b'`」，缺 `client_id`(NOT NULL+FK)/`user_id` → 会先因 **23503/23502** 失败，按错的理由变绿（S-23）。必须断言 **42501**

### TC-016-020 tenant-policies --check 退出 0 + 两张表各 1 条 policy
- **AC**: AC-S-157 步骤 6 ｜ P0 ｜ 命令 ｜ 静态+库查
- **步骤**: 1) `DATABASE_URL=$DATABASE_URL_TEST cargo xtask tenant-policies --check`（捕获退出码）2) `SELECT tablename, policyname FROM pg_policies WHERE tablename IN ('platform_app','platform_app_access')`
- **预期**: ① 退出码 **0**；② 结果**恰好 2 行**：`platform_app|platform_app_tenant_isolation`、`platform_app_access|platform_app_access_tenant_isolation`

### TC-016-021 绑定不存在的 client → 400 APP_CLIENT_INVALID
- **AC**: AC-S-158 路径 1 ｜ 功能/安全 ｜ P0 ｜ 集成 ｜ 边界值
- **步骤**: `POST /api/apps` `{clientId:"no-such-client", displayName:"孤儿探针"}`
- **预期**: 400 + `APP_CLIENT_INVALID`

### TC-016-022 绑定 disabled 的 client → 400 APP_CLIENT_INVALID
- **AC**: AC-S-158 路径 2 ｜ P0 ｜ 集成 ｜ 状态迁移
- **前置**: 用**专用** client `client-lifecycle-probe`（**不用 `client-A`**，见 S-06）
- **步骤**: 1) `PATCH /api/oauth/clients/client-lifecycle-probe {status:"disabled"}` 2) `POST /api/apps` 绑它
- **预期**: PATCH 200；`POST` 400 + `APP_CLIENT_INVALID`

### TC-016-023 删除 client 不被外键阻塞 → 204
- **AC**: AC-S-158 路径 3 ｜ P0 ｜ 集成 ｜ 状态迁移
- **前置**: `client-lifecycle-probe` 改回 `active` 且已绑定一个应用
- **步骤**: `DELETE /api/oauth/clients/client-lifecycle-probe`
- **预期**: **204**（**不是** 409/500）；随后 `GET /api/oauth/clients/client-lifecycle-probe` → 404 `OAUTH_CLIENT_NOT_FOUND`（= REQ-022 AC-S-135 步骤 7 未被破坏）
- **〔AC 修正〕**: AC 原文直接删 `client-A` → 打掉 024/156/159/162 的夹具与 seed 自检（S-06）

### TC-016-024 应用行被级联删除（count == 0）
- **AC**: AC-S-158 路径 4 ｜ P0 ｜ 集成 ｜ 因果
- **预期**: `SELECT count(*) FROM platform_app WHERE id='<步骤 3 的 appId>'` → **0**（`base` 连接）

### TC-016-025 跨租户引用同一 client → 两租户的行都被删〔AC 补充〕
- **AC**: AC-S-158 + D264 的「必须测的判据」｜ P1 ｜ 集成 ｜ 边界值
- **前置**: `client-lifecycle-probe-shared` 被 `tenant-a` 与 `tenant-b` 各接入一次
- **步骤**: `DELETE /api/oauth/clients/client-lifecycle-probe-shared`
- **预期**: 两个租户的 `platform_app` 行**都**为 0；`platform_app_access` 里引用该 client 的访问记录：按 S-13 的决定断言（有 FK → 也删；无 FK → 断言成孤儿行并按 join 过滤不可见）
- **〔AC 补充〕**: 这是 D264 自己列出的后果（`decisions.md:110`），AC-S-158 原文**没验**
