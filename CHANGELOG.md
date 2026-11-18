# 更新日志

本文件记录底座各需求的变更。

## Unreleased

### REQ-009 RBAC 权限与数据范围

- **新建 `crates/platform-rbac`**（平台 crate；依赖方向
  `platform-core ← platform-db ← platform-iam ← platform-rbac ← platform-server`）：
  - **角色模型**：`platform_role` / `platform_role_permission` / `platform_role_inherit` /
    `platform_user_role`（**都带 `tenant_id`** + RLS policy），
    权限点注册表 `platform_permission` 是**平台级**（不带 `tenant_id`）
  - **内置角色按租户物化**：`platform-admin`（`data_scope = ALL`）与 `platform-user`
    （`SELF`）；**不可删除、不可降权**（改 `data_scope` / 改权限点都被拒）
  - **`platform-admin` 的权限集合 = 注册表全集**（**动态得出**，不是通配 `*`）——
    新增权限点它自动拥有，不靠人工补授权
  - **角色继承**在查询时展开；**成环的写入被拒**，错误信息**指出是哪两个角色构成的环**
  - **权限点通配**只允许 `module:*` 与 `module:resource:*`；
    `*:*` 与跨模块通配（`*:demo:read`）**写入被拒**
- **数据范围五种语义**：`ALL` / `OWN_ORG` / `OWN_ORG_AND_BELOW` / `SELF` / `CUSTOM`
  - **兼岗默认按主部门**（`user_org.is_primary`）；取并集必须**显式开启**
    （`platform_role.use_all_memberships`）
  - **主部门缺失或歧义 → fail closed 到 `SELF`**（**不是** `ALL`）：
    DB 侧加部分唯一索引（**预防层**）+ 判定侧退化（**防御层**），两层都要
  - 多角色 scope **取最宽松**（并集语义）；权限点级 scope **覆盖**角色级
- **受限表达式 DSL**（`platform-core::extension::expr`）：
  **白名单字段 + 参数化 + 禁函数调用 + 禁多语句** ——
  生成的 SQL 里**只有占位符**，字面值一律走绑定
- **鉴权入口 `require_permission` 住在 `platform-core`**（不是 `platform-rbac`）：
  业务模块的运行期依赖仍然只有 `platform-core` + `platform-db`（AGENTS.md 硬规则 2）；
  判定实现由平台通过请求扩展注入（`Arc<dyn PermissionChecker>`，方法全同步 → dyn 兼容）
  - 三种拒绝语义（契约 C5）：未认证 **401 `UNAUTHENTICATED`**；
    缺权限点 **403 `INSUFFICIENT_PERMISSION`**；
    记录不在数据范围内 **404 `NOT_FOUND`**（不泄露资源存在性）
- **数据范围过滤在 SQL 层生效**：`platform-db` 的 `TenantQuery::with_scope` /
  `with_condition` —— 条件**插进 WHERE 子句**（不是拼在 `ORDER BY` 后面），
  值走参数绑定；**平台没有「跳过范围」的入口**
- **权限点注册表双向对账**：manifest ↔ 库不一致 → **启动失败**（退出码非 0），
  逐条列出缺失编码与**来源 manifest 路径**
- **缓存 + 数据库 epoch 主动失效**（**不引 Redis 客户端**）：
  角色 / 授权 / 绑定 / 继承 / 数据范围的写操作在同一事务里 `epoch + 1` →
  **撤销在下一个请求即生效**（跨进程也成立）
- **`/api/rbac/*` 管理接口自举防护**：写接口要 `rbac:role:write` /
  `rbac:user-role:write`，读角色列表要 `rbac:role:read` ——
  **普通用户不得调用**（否则任何登录用户都能给自己授权 `platform-admin`）；
  **豁免** `GET /api/rbac/permissions`（前端 `can()` 靠它读注册表）
- **契约检查 C-5**（`extension-points v2`）：已登记权限点必须有**鉴权调用点**，
  鉴权入口**全仓库唯一**，调用参数**必须是字面量** —— 契约测试宏从 4 项变 **5 项**
- **平台基建的连带修复**：
  - `feature-meta/manifest.json` 补 **`contracts` pin**（此前**恒定缺失**，
    `scripts/sync-contracts.sh` 恒 exit 2 → 契约门禁在 CI 上常红）
  - 同步 `docs/contracts/extension-points/v2.md` 快照
  - `manifest.json` 的 `permissions[].condition`（**可选**字段，属性级校验 / ABAC）
  - `platform-db` 的 `BindValue::TextArray`（子树 id 集合走 `= ANY($n)`）
  - `AGENTS.md` 补 `platform-rbac` 目录、C-1~C-5、硬规则 1 的例外说明与
    `require_permission` 用法；脚手架模板带上鉴权调用点与 `condition` 示例
  - `modules/sandbox` 改成**参考实现**：新增 `backend/src/http.rs`（鉴权调用点）、
    `sandbox_scope_item` 迁移（含 `org_node_id` 与 `created_by` 两列）
- **定点修复：多角色 scope 的合并语义改成真正的并集**（BA 裁定 design-summary §4.4 的笔误）：
  - 旧语义「有 `CUSTOM` 就只走 `CUSTOM` 那一路」**丢掉了其它角色的贡献** ——
    用户同时有「本部门」角色和一个「CUSTOM 条件」角色时，
    **本部门里不满足 CUSTOM 条件的记录反而看不见了**（加一个角色**缩小**了可见范围）
  - 新语义：多角色的过滤条件取**并集**（与权限点「任一满足即通过」一致）——
    `CUSTOM` 条件 **OR** 其它角色的最宽松范围；**多个 `CUSTOM` 之间也是 OR**（不是交集）
  - `ScopeFilter` 新增 `CustomOr { conditions, scope }`（平台契约的**纯增量**演进）：
    渲染成**一个**括号包起来的 OR 表达式，值**全部走参数绑定**，
    组织节点子树集合仍是**数组绑定**（`= ANY($n)`，不展开成字符串拼接）

### REQ-008 用户与组织架构

- **新建 `crates/platform-org`**（平台 crate；依赖方向
  `platform-core ← platform-db ← platform-iam ← platform-org ← platform-server`）：
  - **组织树 = 邻接表**（`org_node.parent_id`），查子树用 **PostgreSQL 递归 CTE**
    —— **不用 ltree**（私有化部署常无扩展权限，design-summary D1）
  - **节点类型只有 `company` / `dept` / `team`**；**岗位不是节点**，是挂在部门上的
    属性（`user_position`，D2）
  - **汇报关系是人的属性**（`platform_iam_user.manager_id`，D3）——
    「部门经理」与「我的上级」不混为一谈
  - **用户 ↔ 节点多对多**（`user_org`，含 `is_primary` 标主部门）
  - **`path_labels` 是加速字段**（祖先链），**权威是 `parent_id`**；
    不一致时以 `parent_id` 为准，`rebuild_path_labels` 可重建（D5）
  - 跨表引用都带 `tenant_id`（外键 `(tenant_id, parent_id)`）→
    **结构上**杜绝跨租户挂父节点与悬空成员关系
- **SCIM 2.0 出站**：**自己实现，不引 SCIM 库**（D6）
  - 复用树里已有的 `hyper` / `hyper-util` / `http`（**不引 reqwest**，依赖增量 0）
  - **PUT 幂等**（幂等键 `(provider, external_id)`）；连续两次全量同步**不产生重复条目**
  - **推送必须异步**：本地操作只**入队**就返回，后台 worker 再推（第三方慢不拖垮本地）
  - 失败**指数退避重试**（最多 3 次），超出进同步状态（错误计数）
  - **字段映射外置为配置**（不同 IdP 的用户名 / 邮箱 / 显示名字段名可不同）
- **LDAP / AD 入站**：用 `ldap3`（**`default-features = false`** —— 默认特性会拉
  `native-tls` → OpenSSL 开发包，且 TLS 后端带 `ring` / `aws-lc-rs`）
  - **属性映射外置为 JSON 配置**（D7）
  - **离职检测用位运算表达式**（AD 的 `userAccountControl`）：`bit_and` / `bit_not`，
    不是布尔比较 —— `514` / `546` / `4098` 判为离职，`512` / `660` / `402653184` 判为在职
  - 增量（按 `modifyTimestamp`）+ 全量对账
- **同步冲突队列**（`sync_conflict`，**字段级**）：默认策略 **`manual`** —— 冲突记录到
  `field` / `local_value` / `remote_value`，**绝不静默覆盖**；
  人工裁决（`keep_local` / `take_remote` / `manual`）后按裁决生效
- **离职自动禁用**：禁用账号 + **撤销全部会话与 refresh family**（REQ-007 的 family 作废）
  + 解绑成员关系与岗位 + 审计事件 + 通知 + 出站 `active: false`
- **组织变更事件** → REQ-003 的 `NotificationSender` 扩展点
  （平台 trait 不可 dyn 兼容，故有一层**只转发**的可对象化适配器）
- **新增 `/api/org/*` 路由**（17 个）：组织树 CRUD / 移动 / 子树、成员关系、岗位、
  汇报关系、冲突列表与裁决、SCIM 全量同步、同步状态
  —— **每个接口都验令牌**（组织数据是租户数据），租户只认令牌里的租户
- **平台迁移新增 6 张组织表**（`org_node` / `user_org` / `user_position` /
  `external_identity` / `sync_conflict` / `sync_source_state`）+
  账号表加 3 列（`display_name` / `manager_id` / `departed_at`）
- **RLS 策略由平台统一生成**：`crates/platform-org/manifest.json` 的 `tenantScoped`
  段声明 6 张表 → `cargo xtask tenant-policies` 生成（模块**不手写** policy）；
  平台 crate 的 manifest 现在也参与收集（`platform-db::collect_scopes` 扫 `crates/`）
- **本期只支持明文**：SCIM 只支持 `http://`，LDAP 只支持 `ldap://` ——
  树里**没有任何 TLS 栈**（无 `rustls` / `native-tls`），引入会带 30~50 个 crate
  且拉进 `ring` / `aws-lc-rs`；HTTPS / LDAPS 留后续需求（配置项已留口）
- **依赖**：新增 `ldap3`（`default-features = false`）；SCIM 出站**不新增任何 crate**

### REQ-007 认证与账号体系

- **新建 `crates/platform-iam`**（平台 crate；依赖方向仍是
  `platform-core ← platform-db ← platform-iam ← platform-server`）：
  - **密码**：Argon2id + **PHC 标准串**存储（算法自描述，换算法 / 调参数不改表结构），
    参数可配；本地常见口令词库（本期不做完整 HIBP k-anonymity 接入）
  - **混合令牌**：access token = **JWT**（15 分钟，自包含，带 `kid`）；
    refresh token = **不透明随机串**（30 天，**只存 hash**，可立即撤销）
  - **refresh 轮换 + 重放检测**：旧的立即作废；同一 family 的 refresh 被重复使用
    → 判定泄露 → **整个 family 全部作废**（`TOKEN_REUSE_DETECTED`）
  - **JWT 密钥只从环境注入**（不入库不入 Git），带 `kid` 支持**轮换宽限期**
    （当前 + 上一把同时有效）；**JWKS 只暴露公钥**
  - **TOTP MFA**：绑定 / 校验 / **防重放**（同一时间步不可复用）/ 10 个一次性恢复码；
    **WebAuthn 只留接口不实现**
  - **设备会话**：列表 / 踢下线 / 登出全端；**每个会话一个 refresh family**
  - **所有会话与令牌绑定 `tenant_id`** —— A 租户的令牌不能用于 B 租户
  - **改密码 / 离职撤销全部 refresh family**
- **防账号枚举**（设计摘要 §5.1 / D6）：「账号不存在」与「密码错误」返回
  **完全相同**的错误码与响应体（`INVALID_CREDENTIALS`），且**耗时接近** ——
  账号不存在时也执行一次**假哈希校验**（同参数 Argon2id）并走**同样的数据库工作量**
- **防密码重置枚举**（D7）：`/api/iam/password/forgot` 对「已注册」与「未注册」邮箱
  返回**完全相同**的成功响应
- **登录限流 + 账号锁定**：按 IP 的滑动窗口（DB 计数）+ 按账号的失败次数锁定
- **新增第一个真实 HTTP 业务路由**：`/api/iam/*`（12 个接口）+ `/.well-known/jwks.json`；
  同时落地**租户上下文中间件**（`X-Tenant-Id` → `RequestContext`）——
  闭环 REQ-006 留下的 AC-S-012
- **错误体符合契约 C6**：统一 RFC 9457 Problem Details；**`detail` 必须是英文 ASCII**
  （面向用户的中文文案由前端 i18n 按 `code` 渲染）；密码策略不满足时
  **`params` 指出具体违反哪条规则**
- **幂等键**（契约 C8）：写操作支持 `Idempotency-Key`（UUID），24 小时内重复请求
  返回首次结果；幂等记录**随租户维度存储**
- **平台迁移新增 9 张 IAM 表**（`platform_iam_*`）：全部带 `tenant_id` + RLS 策略
  （fail closed + `WITH CHECK`）；密码哈希与令牌**只存 hash**，TOTP secret **静态加密**
- **`platform-db` 扩展**：`BindValue` 新增 `Timestamp` / `NullableText` /
  `NullableTimestamp`（`timestamptz` ↔ `time::OffsetDateTime`，契约 C13）
- **防假绿护栏**：真实库层缺 `DATABASE_URL_TEST` 时**判为失败**（不是「跳过」），
  另有子进程护栏断言「缺环境变量 → 非零退出 + 输出不含『N passed』」
- **依赖**：新增 `argon2` / `jsonwebtoken`（**`rust_crypto` 后端，不引 ring / aws-lc-rs**）/
  `totp-rs` / `chacha20poly1305` / `p256` / `base64` / `sha2` / `rand`

### REQ-006 多租户与隔离

- **租户隔离双保险**：应用层每次查询都带租户条件，数据库层用 PostgreSQL RLS 再兜一遍
  —— 任一层失效都不至于跨租户泄漏
- **新建 `crates/platform-db`**（不改 `platform-core` 的既有语义）：
  - 租户作用域入口：每次访问在**显式事务**内 `SET LOCAL app.tenant_id`（等价写法
    `set_config('app.tenant_id', $1, true)` —— `SET` 命令不接受绑定参数）
  - **公开 API 表面拿不到裸连接**：只有带 `&RequestContext` 的入口能被业务模块用
    （源码扫描测试守着公开签名里不出现 `PgPool` / `PgConnection` / `Transaction`）
  - 事务封装：单次操作（`fetch_all` / `fetch_optional` / `fetch_one` / `execute`）
    与多操作（`begin` → `TenantTx` → `commit` / `rollback`）两种形态
  - 配额用量计数**写 DB**（`platform_tenant_quota`）+ 限额查询（重启不丢）
  - crate 级文档写清「为什么是 `SET LOCAL` 而不是 `SET`」（连接池串租户风险）
- **RLS 策略由平台统一生成**：模块在 manifest 的 `tenantScoped` 段声明表，
  平台按声明生成 `ENABLE ROW LEVEL SECURITY` + policy
  （`current_setting('app.tenant_id', true)` = **fail closed**），
  **模块不手写 policy**（漏建一张表 = 该表完全无隔离）
  - `cargo xtask tenant-policies`（生成 / `--check` 校验）
  - 策略应用**在一个事务里**完成：`DROP` + `CREATE` 分两条语句会有一个
    「表上没有 policy」的窗口（那一瞬间 RLS 变成全部拒绝）
- **manifest 新增 `tenantScoped` 段**（`crates/platform-core`，CODEOWNERS 保护目录）：
  字段与 JSON Schema 同步，平台校验器与 schema 口径一致
- **C-3 契约检查叠加真实库层**（`base_test` 库）：内存层保留，新增三条断言 ——
  ① tenant_a 的 `list()` 只返本租户行；② tenant_a 的 `find(tenant_b 的 id)` 返回 `None`；
  ③ **绕过应用层**（不设 `app.tenant_id`）**查不到任何行**（RLS fail closed）
- **串租户用例**：tenant_a 查完立刻用**同一个连接池**查 tenant_b（连接池只开一条连接，
  保证复用同一条物理连接）—— 专门验「用的是 `SET LOCAL` 而不是 `SET`」
- **应用角色是 RLS 的前提**：超级用户与 `BYPASSRLS` 角色会**绕过 RLS**
  （租户隔离 fail open）—— 新增 `deploy/seed/02-app-role.sql`，
  应用连接必须用普通角色（组 `base_platform_app` 的成员）
- **部署侧切到普通角色**（QA Pre-merge 复核后的修复）：
  - **角色分工写清楚**：`POSTGRES_USER`（超级用户）只用来建库 / 迁移 / 建策略；
    **应用连接串一律用普通角色**（`APP_DATABASE_URL`）；迁移 / 策略用 `MIGRATION_DATABASE_URL`
  - 开发 compose、私有化 compose、Helm Chart、`.env.example` 全部改成普通角色连接；
    生产新增 `app-role` 步骤（幂等）建角色 —— 生产库不挂 `deploy/seed`（弱口令）
  - **应用启动自检连接角色**：绕过 RLS 就**拒绝启动**（退出码 3）；
    `BASE_ALLOW_SUPERUSER=true` 只在本地开发用（生产环境配它直接启动失败）
  - `prod-preflight.sh` 增加硬闸：应用连接串的用户名**是超级用户就直接拒绝部署**
- **部署流程跑租户作用域策略**：新增 `base tenant-policies`（交付二进制，
  镜像里没有 cargo 也能按 manifest 生成 RLS 策略）；compose 加 `policies` 服务，
  Helm 加 `policies` hook Job（**排在迁移之后**）—— 漏跑这一步 = 租户作用域表
  **完全没有隔离**
- **C-3 真实库层不再假绿**：缺 `DATABASE_URL_TEST` 时**判为失败**（不是「跳过」）
  —— 以前「安静地跳过」会让框架报 "3 passed"，断言一条没跑却被当成通过；
  新增护栏用例：缺环境变量时子进程**必须非零退出**且不得报 "3 passed"
- **应用容器不再拿到超管凭据**（QA 复核后的修复 —— 拆变量名不够，得断在凭据层）：
  - compose 的 `app` 服务**不再 `env_file: .env` 整包注入**，改为**逐个列**它真正需要的
    变量（`DATABASE_URL` ← `APP_DATABASE_URL` + 10 个非敏感项）
  - Helm 的 Deployment **不再 `envFrom` 整包 Secret**，改为逐个 `env:` +
    `valueFrom.secretKeyRef`；迁移 Job 也不再整包引用 Secret
  - 理由：整包 = 应用进程里躺着超管连接串 —— 应用被拿下（环境变量泄露 / 崩溃转储 /
    某个依赖读环境变量）就能拿超管连库，**RLS 被绕过 → 跨租户全量可读**
- **补建命令救得了存量表**（`ALTER DEFAULT PRIVILEGES` **只管未来表**）：
  三处建角色的地方（`02-app-role.sql` / compose `app-role` / Helm `app-role` Job）
  都补上 `GRANT … ON ALL TABLES IN SCHEMA public`（存量）与 `ALTER DEFAULT PRIVILEGES`
  （增量）**两套并存**；RUNBOOK §3.4.3 重写（可直接复制执行的 SQL + 自查命令）
- **配额有了自动化用例**（真实库层 `crates/platform-db/tests/quota_real_db.rs`）：
  计数 → 超限判定 → **重启后计数仍在**（换全新连接池 = 内存态全丢，用量不变），
  外加防假绿护栏（缺 `DATABASE_URL_TEST` 必须非零退出）
- **沙箱模块升级为标准范例**：新增 `tenant_id` 作用域表（`tenantScoped` 声明）
  与走 `platform-db` 租户入口的仓储实现
- **能力索引与门禁**：能力目录新增 `tenant_scoped_db`；
  `check-deps` 的平台 crate 集合收录 `platform-db`（自动纳管生效，有测试守着）
- **新增第三方依赖：无**（`sqlx` 沿用 REQ-005 引入的版本，**不开 `macros` feature**；
  `sqlx-macros` 不参与编译）

### REQ-005 交付基线与发布

- **交付形态**：同一套镜像 + 同一套配置，三处复用 —— 开发（本机 compose）/
  私有化部署（企业内网 compose）/ 云上（K8s Helm）
- **多阶段镜像** `Dockerfile`：`builder`（`rust:lld`，cargo-chef 分层缓存依赖）/
  `frontend`（Node + pnpm）/ `runtime`（`debian:12-slim`，非 root，uid 10001），
  体积目标 < 100MB（CI `build-image` job 硬校验）；**本机不构建**，镜像构建交给 CI
- **私有化部署** `deploy/compose.prod.yaml`：镜像走 tag、不本地构建；
  **迁移是独立服务**（`profiles: ["migrate"]`），`up -d` 不会自动跑迁移
- **Helm Chart** `deploy/helm/base-platform/`：Deployment / Service / Ingress /
  ConfigMap / Secret **引用**（Chart 不生成 Secret）/ 迁移 Job
  （`helm.sh/hook: pre-upgrade,pre-install`）；探针 `/health/live` 与 `/health/ready`
- **配置外置**（AC-S-005）：`crates/platform-server` 的 `DeployConfig` 读环境变量，
  启动时校验、**缺必填项 fail-fast**（退出码 2，稳定错误码
  `CONFIG_MISSING_REQUIRED` / `CONFIG_INVALID_VALUE` / `CONFIG_FORBIDDEN_COMBINATION`）；
  `.env.example` 入库、`.env` 不入库，Secret 不进镜像
- **迁移独立化**（AC-S-006）：`base migrate up|down|status`，迁移文件用 sqlx 可逆格式
  `<版本>_<描述>.up.sql` + `.down.sql`；不可逆迁移拒绝回滚；`scripts/rollback.sh <版本>`
  （回滚前自动备份）
- **健康检查**：`/health/live`（进程活着）与 `/health/ready`（**数据库连通 + 迁移已应用**）；
  未就绪返回 503 + 稳定错误码（`DATABASE_UNAVAILABLE` / `MIGRATIONS_PENDING` /
  `MIGRATION_DIRTY`），错误体按 RFC 9457（契约 C6）
- **备份恢复**：`scripts/backup.sh`（pg_dump + gzip，只留最近 5 份）/
  `scripts/restore.sh` / `scripts/backup-verify.sh`（恢复到临时库 + 冒烟）；
  `deploy/RUNBOOK.md` 含**真实演练记录**与故障排查（迁移失败 / 容器起不来 /
  数据库连不上 / 磁盘满）
- **CI 增补**：`build-image`（构建 + 体积校验 + 健康检查冒烟）、`image-scan`
  （Trivy）、`migration-test`（up → 校验 → down → 再 up）；基础镜像一律官方地址
- **顺带修的 4 项遗留技术债**：`sync-contracts.sh` 拿不到 BA 正本时**明确失败**
  （不再静默跳过）；`core-change-review` 改用 **merge-base** 口径（不再被 base.sha 拖偏）；
  核心路径清单确认覆盖 `platform-core` / `platform-contracts` / `platform-contracts-macros`
  （并加自检，防新增平台 crate 漏进门禁）；`migrations/` 规则在 README 与
  `check-modules` 错误提示里写明「DB 层落地后升级为至少一个 `*.sql`」
- **新增第三方依赖**：`sqlx` 0.8（仅 `runtime-tokio` / `postgres` / `migrate`，
  关闭 `macros` 与 `json` 以压低编译开销）—— 只用于部署期健康检查与迁移，
  业务数据访问层仍待 REQ-006/007
- **修复轮（QA 退回 5 项）**：
  - **契约 C6**：健康检查三个错误路径的 `detail` 统一改为**英文**（此前
    `MIGRATIONS_PENDING` / `MIGRATION_DIRTY` 塞的是中文句子），并统一过
    `dev_detail()`（只保留可打印 ASCII —— 数据库服务端消息会随 `lc_messages` 本地化）；
    面向运维的中文说明移到日志。5 条单测守着
  - **探针超时**：连接池 `acquire_timeout` 由 5s 降到 **2s**（K8s readinessProbe
    与镜像 HEALTHCHECK 仍是 5s）—— 数据库不可达时 ready 在 **2.0s** 返回 503，
    探针不再先超时、拿不到 `DATABASE_UNAVAILABLE`
  - **私有化 compose**：生产口令**无默认值**（`${POSTGRES_PASSWORD:?…}`，缺失即报错
    退出），基础镜像默认值改**官方地址**；新增 `scripts/prod-preflight.sh`
    拒绝弱口令与库名不一致
  - **Helm 口径**：RUNBOOK 与验证产物改为「默认渲染 **5 类资源，不含 Ingress**」
    （`ingress.enabled` 默认 false），并给出 `--set ingress.enabled=true` 的开法
  - **`core-change-detect.sh`**：「平台 crate 必须登记」的检查并进 `--self-test` 入口
- **微修轮（QA 复核新发现 2 项低问题）**：
  - **日志过滤器**：`BASE_LOG_LEVEL` 此前**完全无效** —— 过滤器只写
    `platform_server={level}`，而日志事件实际来自 **bin target `base`**，
    默认配置下一行日志都不输出（RUNBOOK「运维中文说明在日志里」这条兜底通道是死的）。
    现在过滤器同时覆盖 **bin target `base`** 与 **lib target `platform_server`**，
    `info` / `warn` / `error` 三档实测符合预期，**+4 条单测**守着。
    契约 C6 未动：响应体 `detail` 仍是纯 ASCII 英文，中文只在日志里；
    RUNBOOK §5.1 补了「`detail` 英文 / 日志中文」的语言边界表，§5.3 补日志级别说明
  - **RUNBOOK §3.1 的 `env_file` 口径**：此前把 `podman-compose` 的解析基准**说反了**
    （写成按当前目录，实际按 **compose 文件所在目录**），
    且给出的 `ENV_FILE=deploy/.env` 备选写法实测报 `deploy/deploy/.env does not exist`。
    本轮按实测改正（两种 compose 实现一致，附官方文档与源码佐证）、删掉坏的备选写法，
    推荐用法统一为**在 `deploy/` 目录里执行**并实跑通过

### REQ-004 AI 引导体系与能力索引

- 仓库级 `AGENTS.md`（agents.md 开放规范）：目录 / 硬规则 / 真实可用的命令 /
  已有能力 / 尚未提供的能力；「已有能力」段与能力索引逐行一致（由 xtask 测试守住）
- 任务级提示词库 `prompts/`：8 套成套任务书（新建模块 / 加权限点 / 接 SSO /
  定义流程 / 通知模板 / 越权测试 / 补测试用例 / 性能优化），
  每套含目标 / 输入 / 步骤 / 约束 / 验收
- 能力索引 `capabilities/`（**自动生成，勿手改**）：`index.json`（含 `already_provided`
  与 `not_yet_provided`）、`permissions.json`、`extension-points.json`、
  `schema/manifest.schema.json`、`schema/metadata.schema.json`、
  `sdk/<package>.json`、`api.json`（HTTP 层未就绪，占位并注明 REQ-011/012 补）
- 生成器 `cargo xtask capabilities`：`--check` 只校验不写入，CI 用它发现手改；
  产物逐字节可复现（不含墙钟时间戳），**新增第三方依赖：无**
  （trait 声明用正则 + 约定扫描，不引入 `syn` 等解析框架）
- 脚手架 `cargo xtask create-module <name> [--perm-prefix <prefix>]`：
  生成目录 + manifest + backend/frontend/migrations/tests + 模块级 AGENTS.md + README，
  自动登记进工作区成员，并对生成的 Rust 源文件跑 `rustfmt`；
  **产出开箱即过 `check-modules` 与契约检查 C-1 ~ C-4**
- 模块级 `AGENTS.md` 模板由 `create-module` 生成（领域模型 / 扩展点 / 权限点 /
  如何验证 / 易踩的坑）
- CI 护栏：新增 `guardrails` job（`capabilities --check` + `check-deps`）、
  `core-change-label`（平台核心变更自动打标）与 `core-change-review`
  （平台核心变更必须有非作者的批准评审）+ `.github/CODEOWNERS`
- `cargo xtask check-deps` 子命令：把 REQ-003 已有的 `cargo metadata` 依赖图
  遍历暴露给 CI（未重写实现）

### REQ-004 修复轮（首轮 QA PASS-WITH-RISK）

- **CI 补 `pull_request_review` 触发器**：`on:` 增加
  `pull_request_review: types: [submitted, edited, dismissed]`，`pull_request` 补
  `review_requested` —— 修复前评审者批准后 workflow 不会重跑，
  「核心变更需非作者批准」的门禁永远停在旧状态、无法变绿。
  分支保护（required check + Require review from Code Owners）仍是**仓库设置、需人工配置**
- **修正提示词里的假绿命令**：`prompts/02-add-permission.md` 与
  `prompts/07-write-test-cases.md` 的 `cargo test` 命令多了一个 `--`，
  会**跑 0 个用例却 exit 0**；已修正，并把这条坑写进 `AGENTS.md` 硬规则第 8 条
- **契约引用可操作**：`AGENTS.md` / `prompts/03` 不再指向仓库内不存在的 `BA/contracts/...`；
  正本用 `git show demand:contracts/<name>/<version>.md` 取，
  另在 `docs/contracts/` 放随版本发布的**快照**，由 `scripts/sync-contracts.sh`
  从正本生成并校验（CI 强制），不会出现两份正本漂移
- **`cargo xtask capabilities` 会清理多余产物**：此前只写不删，
  `--check` 报的「多余文件」按提示「重新生成」也修不掉；现在按同一口径剪除并清理空目录
- 口径更正：`already_provided` 为 **18** 条（非 17）；
  AGENTS.md 与 `index.json` 是**内容一致、行的顺序不同**（语义序 vs 字典序），非「逐行一致」

### REQ-003 扩展点框架与契约测试

- 扩展点 trait（`crates/platform-core/src/extension/`）：`PlatformModule` / `Repository` /
  `Auditable` / `NotificationSender` / `WorkflowHandler`，配套类型 `RequestContext`（复用 REQ-002）/
  `ListQuery` / `List` / `AuditEventSpec` / `NotificationMessage` / `DeliveryResult` / `TransitionEvent` /
  `ModuleContext`
- 编译期注册：`inventory` 收集 + `discover()` / `discover_enabled()`，业务模块用
  `platform_core::register_platform_module!` 登记
- 契约测试套件（`crates/platform-contracts`）：C-1 manifest 一致 / C-2 权限点声明完整 /
  C-3 租户隔离 / C-4 事务边界，外加简化版 `#[platform_module_contract]` 宏
  （`crates/platform-contracts-macros`，过程宏，零第三方依赖）
- manifest JSON Schema + Rust 校验器：`crates/platform-core/src/extension/manifest.rs`，
  Schema 导出到 `capabilities/schema/manifest.schema.json`
- 目录规范校验补齐（`cargo xtask check-modules`）：manifest 符合 Schema、`manifest.id` 与目录名一致、
  `backend/`/`frontend/`/`migrations/` 非空；新增依赖方向检查（平台 crate 不得依赖业务模块）
- 示例模块 `modules/sandbox/`：扩展点实现 + 契约测试接入，**二次开发的标准范例**

### REQ-002 平台骨架与工程基线

- Rust workspace 骨架：`crates/platform-core`、`crates/platform-contracts`、`crates/xtask`
- 前端 pnpm workspace：`apps/shell`（Vite + Svelte 5）与 `packages/ui|sdk|auth|schema|theme|i18n|hooks` 占位包
- 工具链锁定：`rust-toolchain.toml`、`.node-version`、`.nvmrc`、`pnpm-lock.yaml`、`bun.lock`
- 低配开发基线：`.cargo/config.toml`（`jobs = 3`）、`Cargo.toml` 的 `profile.dev` / `profile.release`
- 本地环境：`compose.yaml`（PostgreSQL 256M + Redis 96M，显式 daocloud 镜像前缀）+ 种子数据
- 目录规范校验：`cargo xtask check-modules`
- CI 模板：`.github/workflows/ci.yml`（lint / test / build / 依赖漏洞扫描 / 契约检查 / E2E）
- 工程文档：`docs/dev-environment.md`、`docs/architecture.md`

### 退回修复轮（QA Pre-merge PASS-WITH-RISK）

- 链接器表述纠正：stable rustc 1.98 的 `x86_64-unknown-linux-gnu` target spec 默认 `linker-flavor = gnu-lld-cc`，
  即默认已用 LLD 22.1.8，无需显式配置；撤销上一轮「链接器偏离」的记账（`docs/dev-environment.md` §2.1、`.cargo/config.toml` 注释）
- 集成测试脚本：`scripts/test-integration.sh` 增加测试库重置（只重置 `base_test`，绝不碰 `base_dev`），
  并把容器数量比对从 `set -e` 的中断路径中解耦 —— 测试失败时比对照常执行，退出码反映测试结果
- 清理未使用依赖：`crates/platform-contracts` 去掉 `platform-core`；`apps/shell` 去掉
  `@base/auth` `@base/hooks` `@base/schema` `@base/theme`
- CI：新增 `e2e` job（Playwright chromium）；`dependency-audit` 去掉 `rustsec/audit-check@v2` 不支持的 `token` 入参

### 复核轮微修（QA Recheck PASS-WITH-RISK）

- 链接器内存对比表换成无采样缺口口径实测值：LLD **132.0 MB** / bfd **135.5 MB** / 差 **2.6%**
  （内核 `VmHWM` 高水位 + `wait4 ru_maxrss` 双仪器互证，真实仓库二进制）；作废上一版「LLD 16.1 MB /
  bfd 134.0 MB，约 8 倍」的说法（瞬时 `VmRSS` 采样 + 探针 crate，`16.1 MB` 低于 `ld.lld` 空载地板）。
  「默认即 LLD、显式配置反而坏事」的结论保留 —— 理由是 stable rustc 1.98 的默认行为、无需配置，
  而不是因为省内存（`docs/dev-environment.md` §2.1）
- 集成测试脚本：`scripts/test-integration.sh` 的测试库重置闸门从黑名单（`*base_dev*`）改为白名单 ——
  只有库名恰好是 `base_test` 才允许重置，其它库名一律明确报错并 `exit 2`（不静默跳过）
