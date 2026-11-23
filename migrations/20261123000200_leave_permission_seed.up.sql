-- 权限点种子：REQ-020 的 3 个 leave 权限点（D348 / D125 三件套之一）。
--
-- ⚠️ **只声明 manifest 不写种子迁移 → 启动对账 fail-fast，进程起不来**。
-- 对账逻辑见 `crates/platform-rbac/src/registry.rs::require_consistent`
-- （`main.rs::reconcile_permissions` 在启动时调用），不一致直接退出，不是告警。
-- 实测依据：写这条迁移**之前**，本批的 `leave_real_db` 用例在
-- `grant_permission` 处拿到 `the permission ... is not in the registry` ——
-- 也就是说平台里没有任何人能**持有** leave 的权限点。
--
-- `source` 必须是 `'modules/leave/manifest.json'` ——
-- 它由 `collect_manifest_permissions` 用 `format!("{dir_name}/{name}/manifest.json")` 拼出来。
--
-- 3 个点的划分与 manifest 一致：
--   * `read`    —— 列表（menu）；
--   * `write`   —— 建单 + 提交（button，申请人侧）；
--   * `approve` —— 审批通过 / 驳回（button，审批人侧）。
-- 读与写分开：只想让审批人看单不该同时给建单权。
--
-- 幂等：`ON CONFLICT DO UPDATE`。
-- 可逆：配套 20261123000200_leave_permission_seed.down.sql（只删这 3 行）。

INSERT INTO platform_permission (code, name, kind, condition, source) VALUES
    ('leave:request:read',    '查看请假单',         'menu',   NULL, 'modules/leave/manifest.json'),
    ('leave:request:write',   '建单与提交请假单',   'button', NULL, 'modules/leave/manifest.json'),
    ('leave:request:approve', '审批请假单',         'button', NULL, 'modules/leave/manifest.json')
ON CONFLICT (code) DO UPDATE SET
    name      = EXCLUDED.name,
    kind      = EXCLUDED.kind,
    condition = EXCLUDED.condition,
    source    = EXCLUDED.source;
