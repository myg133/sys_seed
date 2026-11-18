-- 组织架构的租户作用域表（REQ-008）。
--
-- ⚠️ **这里只建表，不建 RLS policy**：
-- 租户作用域表由**平台统一生成** policy（`crates/platform-org/manifest.json` 的
-- `tenantScoped` 段 → `cargo xtask tenant-policies` / `base tenant-policies`）。
-- 手写 policy 迟早会漏 —— **漏一张表 = 该表完全无隔离**。
--
-- 主键用 (tenant_id, id) 而不是单列 id：单列主键会让「别的租户已经用过这个 id」
-- 通过唯一约束冲突**泄露出来**（报错本身也是信息）。
--
-- 跨表引用都带 `tenant_id`：
-- * `org_node.parent_id` 的外键是 (tenant_id, parent_id) → **结构上**杜绝跨租户挂父节点；
-- * `user_org.org_node_id` 的外键同理 → **结构上**杜绝悬空成员关系（R4）。
--
-- 可逆：配套 20261118000700_org_schema.down.sql。

-- 组织节点（邻接表，D1）
CREATE TABLE IF NOT EXISTS org_node (
    tenant_id   TEXT NOT NULL,
    id          TEXT NOT NULL,
    parent_id   TEXT,
    -- 节点类型只有 company / dept / team —— **岗位不是节点**（D2）
    node_type   TEXT NOT NULL,
    code        TEXT NOT NULL,
    name        TEXT NOT NULL,
    -- 祖先链（从根到本节点，含本节点）：**加速字段，不是权威来源**（D5）
    path_labels TEXT NOT NULL DEFAULT '[]',
    PRIMARY KEY (tenant_id, id),
    CONSTRAINT org_node_type_check CHECK (node_type IN ('company', 'dept', 'team')),
    CONSTRAINT org_node_code_uq UNIQUE (tenant_id, code),
    CONSTRAINT org_node_parent_fk FOREIGN KEY (tenant_id, parent_id)
        REFERENCES org_node (tenant_id, id)
);

CREATE INDEX IF NOT EXISTS org_node_parent_idx ON org_node (tenant_id, parent_id);

COMMENT ON TABLE org_node IS '组织节点（邻接表；子树查询用 PostgreSQL 递归 CTE，不用 ltree）';
COMMENT ON COLUMN org_node.path_labels IS '祖先链（加速字段；权威是 parent_id，不一致时可重建）';

-- 用户 ↔ 节点成员关系（多对多，D3）
CREATE TABLE IF NOT EXISTS user_org (
    tenant_id   TEXT NOT NULL,
    id          TEXT NOT NULL,
    user_id     TEXT NOT NULL,
    org_node_id TEXT NOT NULL,
    -- 在该节点上的角色（如「部门经理」）；**不是**平台的权限角色
    role        TEXT,
    is_primary  BOOLEAN NOT NULL DEFAULT FALSE,
    PRIMARY KEY (tenant_id, id),
    CONSTRAINT user_org_uq UNIQUE (tenant_id, user_id, org_node_id),
    CONSTRAINT user_org_node_fk FOREIGN KEY (tenant_id, org_node_id)
        REFERENCES org_node (tenant_id, id)
);

CREATE INDEX IF NOT EXISTS user_org_user_idx ON user_org (tenant_id, user_id);
CREATE INDEX IF NOT EXISTS user_org_node_idx ON user_org (tenant_id, org_node_id);

COMMENT ON TABLE user_org IS '用户 ↔ 组织节点成员关系（一个人可属多个节点；is_primary 标主部门）';

-- 岗位（挂在部门上的属性，D2）
CREATE TABLE IF NOT EXISTS user_position (
    tenant_id     TEXT NOT NULL,
    id            TEXT NOT NULL,
    user_id       TEXT NOT NULL,
    position_code TEXT NOT NULL,
    org_node_id   TEXT,
    PRIMARY KEY (tenant_id, id),
    CONSTRAINT user_position_node_fk FOREIGN KEY (tenant_id, org_node_id)
        REFERENCES org_node (tenant_id, id)
);

CREATE INDEX IF NOT EXISTS user_position_user_idx ON user_position (tenant_id, user_id);

COMMENT ON TABLE user_position IS '岗位（挂在部门上的属性，不是组织层级的一层）';

-- 外部身份映射（幂等键 (provider, external_id)，D6）
CREATE TABLE IF NOT EXISTS external_identity (
    tenant_id      TEXT NOT NULL,
    id             TEXT NOT NULL,
    provider       TEXT NOT NULL,
    external_id    TEXT NOT NULL,
    user_id        TEXT NOT NULL,
    last_synced_at TIMESTAMPTZ,
    PRIMARY KEY (tenant_id, id),
    CONSTRAINT external_identity_uq UNIQUE (tenant_id, provider, external_id)
);

CREATE INDEX IF NOT EXISTS external_identity_user_idx ON external_identity (tenant_id, user_id);

COMMENT ON TABLE external_identity IS '外部身份映射（幂等键：provider + external_id）';

-- 同步冲突（**字段级**，D8）
CREATE TABLE IF NOT EXISTS sync_conflict (
    tenant_id    TEXT NOT NULL,
    id           TEXT NOT NULL,
    source       TEXT NOT NULL,
    external_id  TEXT NOT NULL,
    -- 冲突字段（username / email / display_name / department）
    field        TEXT NOT NULL,
    local_value  TEXT NOT NULL DEFAULT '',
    remote_value TEXT NOT NULL DEFAULT '',
    status       TEXT NOT NULL DEFAULT 'pending',
    resolution   TEXT,
    resolved_by  TEXT,
    resolved_at  TIMESTAMPTZ,
    PRIMARY KEY (tenant_id, id),
    CONSTRAINT sync_conflict_uq UNIQUE (tenant_id, source, external_id, field),
    CONSTRAINT sync_conflict_status_check CHECK (status IN ('pending', 'resolved', 'ignored')),
    CONSTRAINT sync_conflict_resolution_check
        CHECK (resolution IS NULL OR resolution IN ('local', 'remote', 'manual'))
);

CREATE INDEX IF NOT EXISTS sync_conflict_status_idx ON sync_conflict (tenant_id, status);

COMMENT ON TABLE sync_conflict IS '同步冲突队列（字段级；默认策略 manual —— 绝不静默覆盖）';

-- 同步状态可见性（上次同步时间、同步错误计数）
CREATE TABLE IF NOT EXISTS sync_source_state (
    tenant_id    TEXT NOT NULL,
    source       TEXT NOT NULL,
    last_sync_at TIMESTAMPTZ,
    last_error   TEXT,
    error_count  INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (tenant_id, source)
);

COMMENT ON TABLE sync_source_state IS '同步状态（上次同步时间、错误计数）';
