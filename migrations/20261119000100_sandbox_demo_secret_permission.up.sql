-- 新增字段级权限点 `sandbox:demo:secret`（REQ-012，D67 三件套的第二件）。
--
-- # 三件套（缺一件进程就起不来）
--
-- | 件 | 位置 |
-- |----|------|
-- | 权限点声明 | `modules/sandbox/manifest.json` 的 `permissions[]`（`type: "field"`） |
-- | **种子迁移** | 本文件 |
-- | 启动对账 | `main.rs` 的 `reconcile_permissions`（D27） |
--
-- 忘了本文件 → 启动时 `platform_permission` 里没有这一行 → **进程起不来**，
-- 逐条列出缺失的编码与来源 manifest。这不是告警：权限点缺失时表现为
-- 「配了权限却不生效」，排查成本极高。
--
-- ⚠️ **不要**改 `20261118000901_rbac_permission_seed.up.sql` ——
-- 已应用的迁移改内容会变 checksum，`sqlx::migrate` 会拒绝启动。
--
-- 幂等：可重复执行（`ON CONFLICT DO UPDATE`）。
-- 可逆：配套 20261119000100_sandbox_demo_secret_permission.down.sql。

INSERT INTO platform_permission (code, name, kind, condition, source) VALUES
    ('sandbox:demo:secret', '查看示例内部备注', 'field', NULL, 'modules/sandbox/manifest.json')
ON CONFLICT (code) DO UPDATE SET
    name      = EXCLUDED.name,
    kind      = EXCLUDED.kind,
    condition = EXCLUDED.condition,
    source    = EXCLUDED.source;
