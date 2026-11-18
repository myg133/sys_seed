-- 沙箱模块的租户作用域表（REQ-006：标准范例）。
--
-- ⚠️ **这里只建表，不建 RLS policy**：
-- 租户作用域表由**平台统一生成** policy（manifest 的 `tenantScoped` 段 →
-- `cargo xtask tenant-policies`）。模块手写 policy 迟早会漏 ——
-- 漏一张表 = 该表完全无隔离。
--
-- 主键用 (tenant_id, id) 而不是单列 id：单列主键会让「别的租户已经用过这个 id」
-- 通过唯一约束冲突**泄露出来**（报错本身也是信息）。
--
-- 可逆：配套 20261118000300_sandbox_tenant_item.down.sql。

CREATE TABLE IF NOT EXISTS sandbox_tenant_item (
    tenant_id TEXT    NOT NULL,
    id        TEXT    NOT NULL,
    title     TEXT    NOT NULL,
    revision  INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (tenant_id, id)
);

COMMENT ON TABLE sandbox_tenant_item IS '沙箱示例：租户作用域表（RLS policy 由平台按 manifest 生成）';
