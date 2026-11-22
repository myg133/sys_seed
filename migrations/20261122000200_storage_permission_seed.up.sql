-- 权限点种子：REQ-018 的 3 个 storage 权限点（design §4 / D125 三件套之一）。
--
-- ⚠️ **只改 manifest 不写种子迁移 → 启动对账 fail-fast，进程起不来**。
-- 对账逻辑见 `crates/platform-rbac/src/registry.rs::require_consistent`
-- 与 `crates/platform-storage/manifest.json` 的 `permissions` 段（双向对账，
-- 不一致直接退出，不是告警）。
--
-- `source` 必须是 `'crates/platform-storage/manifest.json'` ——
-- 它由 `crates/platform-rbac/src/registry.rs::collect_manifest_permissions`
-- 用 `format!("{dir_name}/{name}/manifest.json")` 拼出来（`crates` + 目录名）。
--
-- # 为什么**恰好 3 个**
--
-- 读与写必须分开：`storage:file:read` 是「能看见并下载」，
-- `storage:file:write` 是「能上传与删除」—— 一个只想给业务方发文件的租户
-- 不该同时获得删除权。
--
-- 任务查看单独成点：导入/导出任务会暴露**行级错误回执**（可能含业务数据），
-- 与文件读写不是同一风险面，复用 `storage:file:read` 会把它顺带放开。
--
-- 分片上传**不单独设点**：分片是 `storage:file:write` 的实现细节，
-- 客户端不会只传一个分片而不完成上传。
--
-- 幂等：`ON CONFLICT DO UPDATE`。
-- 可逆：配套 20261122000200_storage_permission_seed.down.sql（只删这 3 行）。

INSERT INTO platform_permission (code, name, kind, condition, source) VALUES
    ('storage:file:read', '查看与下载文件', 'menu',   NULL, 'crates/platform-storage/manifest.json'),
    ('storage:file:write','上传与删除文件', 'button', NULL, 'crates/platform-storage/manifest.json'),
    ('storage:job:read',  '查看导入导出任务','menu',  NULL, 'crates/platform-storage/manifest.json')
ON CONFLICT (code) DO UPDATE SET
    name      = EXCLUDED.name,
    kind      = EXCLUDED.kind,
    condition = EXCLUDED.condition,
    source    = EXCLUDED.source;
