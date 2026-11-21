-- 权限点种子：REQ-015 的 3 个 notify 权限点（D300 / D125 的三件套之一）。
--
-- ⚠️ **只改 manifest 不写种子迁移 → 启动对账 fail-fast，进程起不来**。
-- 对账逻辑见 `crates/platform-rbac/src/registry.rs::require_consistent`
-- 与 `crates/platform-server/src/main.rs::reconcile_permissions`（双向对账，
-- 不一致直接退出，不是告警）。
--
-- `source` 必须是 `'crates/platform-notify/manifest.json'` ——
-- 它由 `crates/platform-rbac/src/registry.rs::collect_manifest_permissions`
-- 用 `format!("{dir_name}/{name}/manifest.json")` 拼出来（`crates` + 目录名）。
--
-- # 为什么**恰好 3 个**（D300）
--
-- **订阅偏好不进权限点**：那是**用户自己的**设置（登录即可改），
-- 与 REQ-016 的 D271 同款口径。若给它加权限点，普通用户就改不了自己的退订了。
--
-- ⚠️ **重投死信也用 `notify:delivery:read`**：不单独拆 `notify:delivery:write`。
-- 理由：能看投递记录的人本来就能看到失败原因，重投是**补救**动作，
-- 拆成两个点只会让「运维能看不能救」。
--
-- 幂等：`ON CONFLICT DO UPDATE`。
-- 可逆：配套 20261121000500_notify_permission_seed.down.sql（只删这 3 行）。

INSERT INTO platform_permission (code, name, kind, condition, source) VALUES
    ('notify:template:read',  '查看通知模板',           'menu',   NULL, 'crates/platform-notify/manifest.json'),
    ('notify:template:write', '维护通知模板',           'button', NULL, 'crates/platform-notify/manifest.json'),
    ('notify:delivery:read',  '查看投递记录与重投死信', 'menu',   NULL, 'crates/platform-notify/manifest.json')
ON CONFLICT (code) DO UPDATE SET
    name      = EXCLUDED.name,
    kind      = EXCLUDED.kind,
    condition = EXCLUDED.condition,
    source    = EXCLUDED.source;
