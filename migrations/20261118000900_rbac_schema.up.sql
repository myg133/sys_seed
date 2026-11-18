-- 角色与权限的平台表（REQ-009）。
--
-- ⚠️ **这里只建表，不建 RLS policy**：
-- 租户作用域表由**平台统一生成** policy（`crates/platform-rbac/manifest.json` 的
-- `tenantScoped` 段 → `cargo xtask tenant-policies` / `base tenant-policies`）。
-- 手写 policy 迟早会漏 —— **漏一张表 = 该表完全无隔离**。
--
-- 租户维度（design-summary §2 / §3）：
--
-- | 表 | 租户维度 | 说明 |
-- |----|----------|------|
-- | `platform_permission` | **无**（平台级） | 权限点来自代码（manifest），全平台一致 |
-- | `platform_role` / `platform_role_permission` / `platform_role_inherit` / `platform_user_role` | 有 | 角色与绑定是租户数据 |
-- | `platform_permission_epoch` | 有（主键 `tenant_id`） | 缓存主动失效的信号来源 |
--
-- 主键一律用 (tenant_id, id) 而不是单列 id：单列主键会让「别的租户已经用过这个 id」
-- 通过唯一约束冲突**泄露出来**（报错本身也是信息）。
--
-- 可逆：配套 20261118000900_rbac_schema.down.sql。

-- 权限点注册表（**平台级**：不带 tenant_id、不建 RLS，同 platform_meta 的处理）。
--
-- ⚠️ 这里的行由**迁移**写入（见 20261118000901_rbac_permission_seed.up.sql），
-- **不是**运行时自动改库 —— 运行时自动改库会让「启动对账」失去意义。
CREATE TABLE IF NOT EXISTS platform_permission (
    code      TEXT PRIMARY KEY,
    name      TEXT NOT NULL,
    kind      TEXT NOT NULL DEFAULT 'button',
    -- 属性级校验条件（RBAC + ABAC 混合）：可空 = 无条件。
    condition TEXT,
    -- 来源 manifest 的路径（对账报错时逐条列出）。
    source    TEXT NOT NULL DEFAULT ''
);

CREATE INDEX IF NOT EXISTS platform_permission_source_idx ON platform_permission (source);

COMMENT ON TABLE platform_permission IS '权限点注册表（平台级；由迁移写入，启动时与 manifest 双向对账）';
COMMENT ON COLUMN platform_permission.condition IS '属性级校验表达式（可空 = 无条件）';

-- 角色（租户数据）。
CREATE TABLE IF NOT EXISTS platform_role (
    tenant_id           TEXT NOT NULL,
    id                  TEXT NOT NULL,
    code                TEXT NOT NULL,
    name                TEXT NOT NULL,
    -- ALL / OWN_ORG / OWN_ORG_AND_BELOW / SELF / CUSTOM
    data_scope          TEXT NOT NULL DEFAULT 'SELF',
    -- CUSTOM 的过滤表达式（仅 data_scope = CUSTOM 时有效）。
    custom_filter       TEXT,
    -- 是否取所有成员节点的并集（默认 FALSE = 只按主部门）。
    use_all_memberships BOOLEAN NOT NULL DEFAULT FALSE,
    -- 内置角色（不可删除、不可降权）。
    builtin             BOOLEAN NOT NULL DEFAULT FALSE,
    PRIMARY KEY (tenant_id, id),
    CONSTRAINT platform_role_code_uq UNIQUE (tenant_id, code),
    CONSTRAINT platform_role_scope_check
        CHECK (data_scope IN ('ALL', 'OWN_ORG', 'OWN_ORG_AND_BELOW', 'SELF', 'CUSTOM'))
);

CREATE INDEX IF NOT EXISTS platform_role_builtin_idx ON platform_role (tenant_id, builtin);

COMMENT ON TABLE platform_role IS '角色（租户数据；内置角色按租户物化）';
COMMENT ON COLUMN platform_role.use_all_memberships IS '兼岗取并集的开关（默认 FALSE = 只按主部门）';

-- 角色-权限（租户数据）。
--
-- `permission_code` 可以是通配（`module:*` / `module:resource:*`）；
-- `*:*` 与跨模块通配由**服务层**拒绝（写不进来）。
--
-- `data_scope` **可空 = 不覆盖角色级**（权限点级 scope 的优先级高于角色级）。
CREATE TABLE IF NOT EXISTS platform_role_permission (
    tenant_id       TEXT NOT NULL,
    role_id         TEXT NOT NULL,
    permission_code TEXT NOT NULL,
    data_scope      TEXT,
    PRIMARY KEY (tenant_id, role_id, permission_code),
    CONSTRAINT platform_role_permission_role_fk
        FOREIGN KEY (tenant_id, role_id) REFERENCES platform_role (tenant_id, id) ON DELETE CASCADE,
    CONSTRAINT platform_role_permission_scope_check
        CHECK (data_scope IS NULL OR data_scope IN ('ALL', 'OWN_ORG', 'OWN_ORG_AND_BELOW', 'SELF', 'CUSTOM'))
);

COMMENT ON TABLE platform_role_permission IS '角色-权限（租户数据；data_scope 可空 = 不覆盖角色级）';

-- 角色继承（自引用；租户数据）。
CREATE TABLE IF NOT EXISTS platform_role_inherit (
    tenant_id      TEXT NOT NULL,
    role_id        TEXT NOT NULL,
    parent_role_id TEXT NOT NULL,
    PRIMARY KEY (tenant_id, role_id, parent_role_id),
    CONSTRAINT platform_role_inherit_role_fk
        FOREIGN KEY (tenant_id, role_id) REFERENCES platform_role (tenant_id, id) ON DELETE CASCADE,
    CONSTRAINT platform_role_inherit_parent_fk
        FOREIGN KEY (tenant_id, parent_role_id) REFERENCES platform_role (tenant_id, id) ON DELETE CASCADE,
    -- 不许自己继承自己（成环的最小形态）。
    CONSTRAINT platform_role_inherit_not_self CHECK (role_id <> parent_role_id)
);

COMMENT ON TABLE platform_role_inherit IS '角色继承（租户数据；成环在服务层被拒）';

-- 用户-角色绑定（租户数据）。
CREATE TABLE IF NOT EXISTS platform_user_role (
    tenant_id TEXT NOT NULL,
    user_id   TEXT NOT NULL,
    role_id   TEXT NOT NULL,
    PRIMARY KEY (tenant_id, user_id, role_id),
    CONSTRAINT platform_user_role_role_fk
        FOREIGN KEY (tenant_id, role_id) REFERENCES platform_role (tenant_id, id) ON DELETE CASCADE,
    -- 复合外键 → 本地账号（同租户）：幽灵 user_id 与跨租户 user_id 都被数据库拒绝。
    CONSTRAINT platform_user_role_user_fk
        FOREIGN KEY (tenant_id, user_id) REFERENCES platform_iam_user (tenant_id, id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS platform_user_role_role_idx ON platform_user_role (tenant_id, role_id);

COMMENT ON TABLE platform_user_role IS '用户-角色绑定（租户数据）';

-- 缓存主动失效的 epoch（租户数据）。
--
-- 任何角色 / 授权 / 绑定 / 继承 / 数据范围的写操作，都在同一事务里 epoch + 1。
-- 读路径先读 epoch —— 与缓存里的 epoch 一致才用缓存。
-- **失效信号来自共享的数据库** → 多实例部署下撤销也在下一个请求即生效。
CREATE TABLE IF NOT EXISTS platform_permission_epoch (
    tenant_id  TEXT PRIMARY KEY,
    epoch      BIGINT NOT NULL DEFAULT 0,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

COMMENT ON TABLE platform_permission_epoch IS '权限缓存的失效信号（写操作同事务 epoch + 1）';

-- 「至多一个主部门」变成**数据库约束**（预防层）。
--
-- ⚠️ `user_org` 原来的唯一约束是 (tenant_id, user_id, org_node_id) ——
-- **没有**约束「一个用户至多一个主部门」，于是「主部门歧义」是可能的。
-- 这里补上部分唯一索引：歧义状态**写不进来**。
--
-- 判定层（platform-rbac）**另外**还有一道防御：主部门缺失或歧义 → fail closed 到 SELF。
-- 两层都要 —— 索引防「新的歧义」，判定层防「绕过索引造出来的歧义」。
--
-- 已核对当前 base_test 数据：无任何用户存在多条主部门记录，加该索引不会破坏既有数据。
CREATE UNIQUE INDEX IF NOT EXISTS user_org_one_primary_idx
    ON user_org (tenant_id, user_id) WHERE is_primary;

COMMENT ON INDEX user_org_one_primary_idx IS '一个用户至多一个主部门（歧义在判定层仍要 fail closed）';
