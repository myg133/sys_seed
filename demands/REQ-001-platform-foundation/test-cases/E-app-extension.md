# E. 应用接入与扩展 —— 测试用例（AC-020 ~ AC-021）

- **需求**: REQ-001 通用项目底座（Base Platform）
- **来源**: [acceptance.md](../acceptance.md) §E、[demand.md](../demand.md) §E、[ADR-0001](../../decisions/ADR-0001-底座技术架构基线.md) D5 / D7
- **用例数**: 4（TC-E-001 ~ TC-E-004）
- **公共夹具**: 见 [index.md](index.md) §3 测试夹具

## E.0 本域约定

- 应用注册信息：`app_id` / `secret` / 类型（`sso` / `data_api` / `embedded`）/ 回调地址 / 令牌有效期 / 授权模式（`authorization_code` + PKCE / `client_credentials` / `device_code`）。
- OAuth 端点：`GET /api/app/oauth/authorize`、`POST /api/app/oauth/token`、`POST /api/app/oauth/introspect`、`GET /.well-known/jwks.json`。
- 授权同意页：用户授权记录可查、可撤销；应用级权限声明使用权限点编码（`module:resource:action`）。
- 后端扩展点：Rust `trait` + 编译期注册；业务模块目录 `modules/<name>/`，含 `manifest.json`。

---

### TC-E-001: 应用授权码 + PKCE 换令牌，应用接口通过 JWKS / introspect 校验
- **关联 AC**: AC-020
- **前置条件**:
  - 已注册应用 `app_demo`（`app_id=app_demo`、`secret=***`、类型 `data_api`、回调 `https://app.example.com/cb`、令牌有效期 15 分钟、授权模式 `authorization_code` + PKCE）。
  - 应用 `app_demo` 声明的应用级权限点为 `order:invoice:read`。
  - 账号 `boss@example.com` 已登录 T1。
- **测试步骤**:
  1. 走完整授权流程：`GET /api/app/oauth/authorize?response_type=code&client_id=app_demo&redirect_uri=https://app.example.com/cb&scope=order:invoice:read&code_challenge=<S256(verifier)>&code_challenge_method=S256` → 授权页同意 → 回调取得 `code`。
  2. `POST /api/app/oauth/token`（`grant_type=authorization_code` + `code` + `code_verifier` + `client_id`）换取令牌。
  3. 用该令牌调用应用接口 `GET /api/app/demo/order/invoice`（`Authorization: Bearer <token>`）。
  4. 另用 `POST /api/app/oauth/introspect`（`token=<token>`）与 `GET /.well-known/jwks.json` 校验令牌。
- **预期结果**:
  1. 授权页展示应用名、logo 与申请范围 `order:invoice:read`，同意后回调带回 `code` 且 `state` 原样返回。
  2. 令牌接口 HTTP 200，返回 `access_token` / `token_type=Bearer` / `expires_in` / `refresh_token` / `scope=order:invoice:read`；响应体不含 `secret` 明文。
  3. 应用接口 HTTP 200，且返回数据受令牌 scope 与用户数据范围双重约束（只看得到 T1 + 本人/本组织范围内数据）。
  4. introspect 返回 `{"active": true, "client_id": "app_demo", "scope": "order:invoice:read"}`；JWKS 端点 HTTP 200 且包含该令牌 `kid` 对应公钥（`kid` 与令牌头一致）。
- **类型**: 功能 / 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 端到端链路 + 正常路径

### TC-E-002: 用户在授权页撤销授权后令牌失效
- **关联 AC**: AC-020
- **前置条件**:
  - 账号 `boss@example.com` 已对应用 `app_demo` 授权（存在授权记录，scope `order:invoice:read`），并已换取一枚有效 access token。
  - 底座提供授权记录查询与撤销入口（如 `GET /api/app/authorizations` / `DELETE /api/app/authorizations/{id}`）。
- **测试步骤**:
  1. 在「我的授权」页面查询授权记录，确认 `app_demo` 存在。
  2. 执行撤销授权。
  3. 用撤销前的 access token 调用 `GET /api/app/demo/order/invoice`；并用 refresh token 尝试换新令牌。
- **预期结果**:
  1. 撤销操作返回 HTTP 200/204，授权记录状态变为 `revoked`。
  2. 旧 access token 调用应用接口返回 HTTP 401。
  3. refresh token 换取新令牌返回 HTTP 400（`code` 如 `invalid_grant`），不签发新令牌。
  4. 撤销动作写入审计日志（操作人、应用、scope、撤销时间）。
- **类型**: 安全
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 状态迁移（已授权 → 已撤销）+ 攻击面枚举（撤销后重放）

### TC-E-003: 新增业务模块插件，平台核心零改动即可编译装配
- **关联 AC**: AC-021
- **前置条件**:
  - 平台已提供扩展点 trait（如 `OrderRepository`、`Auditable`）与编译期注册机制。
  - 准备一个全新模块 `modules/gift/`（仅业务目录：`manifest.json` + `backend/` + `frontend/` + `migrations/` + `tests/contracts.rs`）。
  - 执行前记录仓库基线：`git status` 与 `git diff --stat`（记录平台核心目录 `crates/platform-core/` 的哈希）。
- **测试步骤**:
  1. 在 `modules/gift/` 内实现平台定义的 trait（仅改该目录文件）。
  2. 执行 `cargo build --workspace` 与 `cargo test --workspace`。
  3. 对比 `crates/platform-core/` 目录的改动与编译产物中是否包含 `gift` 模块注册。
- **预期结果**:
  1. 编译成功（exit code 0），模块被自动注册（无需在平台核心写注册代码）。
  2. 契约测试通过（`cargo test --workspace` exit code 0）。
  3. `git diff --stat` 中**不含** `crates/platform-core/` 与任何平台核心文件；平台核心改动行数 = 0。
  4. 运行期接口 `GET /api/gift/...` 可用，菜单中出现「礼品」入口（由 manifest 声明生成）。
- **类型**: 功能 / 可维护性
- **优先级**: P0
- **自动化**: 半自动
- **设计方法**: 零改动断言（diff 校验）

### TC-E-004: 插件声明的权限点出现在权限管理界面
- **关联 AC**: AC-021
- **前置条件**:
  - `modules/gift/manifest.json` 声明权限点 `gift:card:read`、`gift:card:write`（`type` 分别为 `menu` / `button`）。
  - 账号 `owner@example.com` 是 T1 管理员。
- **测试步骤**:
  1. 以 `owner@example.com` 登录，打开「权限管理 → 权限点列表」。
  2. 检索 `gift:card:read` 与 `gift:card:write`。
  3. 调用 `GET /api/rbac/permissions` 核对注册表内容。
  4. 把 `gift:card:read` 授予角色 `member`，用 `staff@example.com` 登录检查前端 `can('gift:card:read')` 与菜单显隐。
- **预期结果**:
  1. 权限管理界面列表中出现这两条权限点，名称取自 manifest（`name` 字段），归属模块显示为 `gift`。
  2. `GET /api/rbac/permissions` 返回中包含两条，字段含 `code` / `name` / `module` / `type`，且 `code` 与 manifest 完全一致。
  3. 授权后 `staff@example.com` 的 `can('gift:card:read')` 返回 `true`，「礼品」菜单可见；未授予的 `gift:card:write` 仍为 `false`。
  4. 插件权限点无需在平台核心的权限常量表里登记即可生效（无手工维护动作）。
- **类型**: 功能
- **优先级**: P0
- **自动化**: 可自动化
- **设计方法**: 端到端一致性 + 边界（未授权项不受影响）
