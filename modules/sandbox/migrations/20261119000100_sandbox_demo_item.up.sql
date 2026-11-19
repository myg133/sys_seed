-- `demoItem` 实体表（REQ-012：元数据驱动渲染 + 字段级权限的活样本）。
--
-- ⚠️ **这里只建表，不建 RLS policy**：租户作用域表由**平台统一生成** policy
-- （manifest 的 `tenantScoped` 段 → `cargo xtask tenant-policies`）。
--
-- # 为什么列比元数据多
--
-- `modules/sandbox/manifest.json` 的 `schema.demoItem` 只声明 7 个字段
-- （`id` / `title` / `amount` / `paid` / `dueAt` / `ownerId` / `secret`），
-- 表里另有两列**故意不写进元数据**：
--
-- | 列 | 用途 |
-- |----|------|
-- | `created_at` | 服务端造的时间戳，前端不需要编辑 |
-- | `internal_note` | **用来证明白名单语义**：实体里多出来的字段默认不输出（fail closed） |
--
-- 主键用 (tenant_id, id)：单列主键会让「别的租户已经用过这个 id」通过唯一约束冲突泄露出来。
--
-- 可逆：配套 20261119000100_sandbox_demo_item.down.sql。

CREATE TABLE IF NOT EXISTS sandbox_demo_item (
    tenant_id     TEXT    NOT NULL,
    id            TEXT    NOT NULL,
    title         TEXT    NOT NULL,
    -- 金额（最小单位整数，契约 C13）。
    amount        BIGINT  NOT NULL DEFAULT 0,
    paid          BOOLEAN NOT NULL DEFAULT FALSE,
    -- 到期日：`YYYY-MM-DD` 文本 —— **不做时区换算**（D18 的文案约定）。
    due_at        TEXT,
    -- 负责人（引用另一个实体的 id；本期只读文本，D105）。
    owner_id      TEXT    NOT NULL DEFAULT '',
    -- 内部备注：**字段级权限**（无 sandbox:demo:secret 的用户看不到）。
    secret        TEXT,
    -- 未写进元数据的内部字段（**永不出现在响应里**）。
    internal_note TEXT,
    created_at    TEXT    NOT NULL DEFAULT '',
    PRIMARY KEY (tenant_id, id)
);

CREATE INDEX IF NOT EXISTS sandbox_demo_item_created_idx
    ON sandbox_demo_item (tenant_id, created_at);

COMMENT ON TABLE sandbox_demo_item IS '元数据驱动渲染的示例实体（字段级权限 + 白名单投影）';
