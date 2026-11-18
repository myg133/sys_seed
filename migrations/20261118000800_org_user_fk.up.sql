-- 用户侧的引用完整性（REQ-008 修复轮 · 中 2）。
--
-- QA 实测：`user_org.user_id` / `user_position.user_id` / `external_identity.user_id`
-- **一条外键都没有** —— 能写进**不存在的用户**（幽灵 user_id）与**任意跨租户**的 user_id。
-- 节点侧的外键（`org_node_parent_fk` / `user_org_node_fk` / `user_position_node_fk`）
-- **都带 tenant_id**，用户侧裸奔 → 与设计摘要 §2「外键全带 tenant_id → **结构上**杜绝
-- 跨租户引用与悬空成员」的承诺不符。
--
-- 这里补上**复合外键** `(tenant_id, user_id)` → `platform_iam_user (tenant_id, id)`：
--
-- * **幽灵 user_id** → 数据库直接拒绝（外键违约）；
-- * **跨租户 user_id** → 数据库直接拒绝（复合键里带 tenant_id，别人的行**匹配不上**）。
--
-- 前置条件：被引用的 `(tenant_id, id)` 必须有**唯一约束**。
-- `platform_iam_user` 原来只有单列主键 `id`（全局唯一），**不足以**作为复合外键的目标
-- —— PostgreSQL 要求被引用列组上存在唯一约束。这是 REQ-007 遗留的结构缺口，这里补齐。
--
-- `sync_conflict` **没有 user_id 列**（它按 `(source, external_id, field)` 定位**远端**条目），
-- 因此**不需要**用户侧外键 —— 本迁移不碰它。
--
-- ⚠️ 目标库若已有**悬空的成员行**，本迁移会失败（这是刻意的：把脏数据暴露出来，
--   而不是悄悄留着）。先清理悬空行再迁移。
--
-- 可逆：配套 20261118000800_org_user_fk.down.sql。

-- 复合外键的目标：`(tenant_id, id)` 唯一。
ALTER TABLE platform_iam_user
    ADD CONSTRAINT platform_iam_user_tenant_id_uq UNIQUE (tenant_id, id);

-- 成员关系 → 本地账号（同租户）。
ALTER TABLE user_org
    ADD CONSTRAINT user_org_user_fk
    FOREIGN KEY (tenant_id, user_id) REFERENCES platform_iam_user (tenant_id, id);

-- 岗位 → 本地账号（同租户）。
ALTER TABLE user_position
    ADD CONSTRAINT user_position_user_fk
    FOREIGN KEY (tenant_id, user_id) REFERENCES platform_iam_user (tenant_id, id);

-- 外部身份 → 本地账号（同租户）。
ALTER TABLE external_identity
    ADD CONSTRAINT external_identity_user_fk
    FOREIGN KEY (tenant_id, user_id) REFERENCES platform_iam_user (tenant_id, id);
