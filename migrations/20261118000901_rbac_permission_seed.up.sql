-- 权限点注册表的**初始内容**（REQ-009）。
--
-- ⚠️ **为什么是迁移而不是运行时自动写**：
-- 「库里的权限点」由迁移写入，「manifest 侧」由代码声明 —— 启动时**双向对账**。
-- 若运行时自动把 manifest 同步进库，对账就永远通过，检查失去意义。
--
-- **新增权限点的流程**：改 `manifest.json` → 加一条 INSERT → 跑迁移 → 启动对账通过。
-- 忘了加 INSERT → **启动失败**（逐条列出缺失的编码与来源 manifest 路径）。
--
-- 幂等：可重复执行（`ON CONFLICT DO UPDATE`）。
--
-- 可逆：配套 20261118000901_rbac_permission_seed.down.sql。

-- -- 业务模块：sandbox --------------------------------------------------
INSERT INTO platform_permission (code, name, kind, condition, source) VALUES
    ('sandbox:demo:read',        '查看沙箱示例',         'menu',   NULL, 'modules/sandbox/manifest.json'),
    ('sandbox:demo:write',       '编辑沙箱示例',         'button', NULL, 'modules/sandbox/manifest.json'),
    ('sandbox:scope-item:read',  '查看数据范围示例',     'button', NULL, 'modules/sandbox/manifest.json'),
    -- 属性级校验（RBAC + ABAC 混合）：金额超过 10000 不许审批。
    ('sandbox:scope-item:approve','审批数据范围示例',    'button', '$record.amount <= 10000', 'modules/sandbox/manifest.json'),
    ('sandbox:scope-item:export', '导出数据范围示例',     'button', NULL, 'modules/sandbox/manifest.json')
ON CONFLICT (code) DO UPDATE SET
    name      = EXCLUDED.name,
    kind      = EXCLUDED.kind,
    condition = EXCLUDED.condition,
    source    = EXCLUDED.source;

-- -- 平台 crate：org（REQ-008） -----------------------------------------
INSERT INTO platform_permission (code, name, kind, condition, source) VALUES
    ('org:org-node:read',        '查看组织架构',         'menu',   NULL, 'crates/platform-org/manifest.json'),
    ('org:org-node:write',       '编辑组织架构',         'button', NULL, 'crates/platform-org/manifest.json'),
    ('org:user-org:read',        '查看组织成员关系',     'button', NULL, 'crates/platform-org/manifest.json'),
    ('org:user-org:write',       '编辑组织成员关系',     'button', NULL, 'crates/platform-org/manifest.json'),
    ('org:user-position:read',   '查看岗位',             'button', NULL, 'crates/platform-org/manifest.json'),
    ('org:user-position:write',  '编辑岗位',             'button', NULL, 'crates/platform-org/manifest.json'),
    ('org:sync-conflict:read',   '查看同步冲突',         'menu',   NULL, 'crates/platform-org/manifest.json'),
    ('org:sync-conflict:resolve','裁决同步冲突',         'button', NULL, 'crates/platform-org/manifest.json'),
    ('org:scim-sync:run',        '触发 SCIM 全量同步',   'button', NULL, 'crates/platform-org/manifest.json')
ON CONFLICT (code) DO UPDATE SET
    name      = EXCLUDED.name,
    kind      = EXCLUDED.kind,
    condition = EXCLUDED.condition,
    source    = EXCLUDED.source;

-- -- 平台 crate：rbac（REQ-009） ----------------------------------------
--
-- ⚠️ 这几条是**权限系统的自举口**：普通用户（只有 `platform-user`）没有它们，
-- 所以调不了 `/api/rbac/*` 的管理接口 —— 否则任何登录用户都能给自己授权
-- `platform-admin`，整个权限系统被架空。
INSERT INTO platform_permission (code, name, kind, condition, source) VALUES
    ('rbac:role:read',        '查看角色与授权',     'menu',   NULL, 'crates/platform-rbac/manifest.json'),
    ('rbac:role:write',       '编辑角色与授权',     'button', NULL, 'crates/platform-rbac/manifest.json'),
    ('rbac:user-role:write',  '绑定用户角色',       'button', NULL, 'crates/platform-rbac/manifest.json'),
    ('rbac:permission:read',  '查看权限点注册表',   'button', NULL, 'crates/platform-rbac/manifest.json')
ON CONFLICT (code) DO UPDATE SET
    name      = EXCLUDED.name,
    kind      = EXCLUDED.kind,
    condition = EXCLUDED.condition,
    source    = EXCLUDED.source;
