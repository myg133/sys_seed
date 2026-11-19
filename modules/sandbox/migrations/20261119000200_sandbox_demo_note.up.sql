-- `demoNote` 实体表（REQ-012 D107：第二个实体，用来证明「新增同类页面无需新增前端代码」）。
--
-- ⚠️ **这里只建表，不建 RLS policy**：租户作用域表由**平台统一生成** policy
-- （manifest 的 `tenantScoped` 段 → `cargo xtask tenant-policies`）。
--
-- # 为什么要有第二个实体
--
-- AC-S-085 的断言是「同一个通用渲染器能渲染**不同形状**的实体」。
-- 只有一个实体时这条 AC 无法验 —— 用环境变量注入第二个实体是测试专属路径，
-- 不是真实能力。所以真建一张表、真声明一份元数据。
--
-- 字段组合**故意**与 `sandbox_demo_item` 不同：
--
-- | `demoItem` | `demoNote` |
-- |------------|------------|
-- | `amount` number / `paid` boolean / `dueAt` date / `ownerId` reference | `content` 长文本 / `pinned` boolean / **无** 数字、日期、引用 |
--
-- 这不影响前端：渲染器按元数据里的 `type` 分派，对实体键一无所知。
--
-- 主键用 (tenant_id, id)：单列主键会让「别的租户已经用过这个 id」通过唯一约束冲突泄露出来。
--
-- 可逆：配套 20261119000200_sandbox_demo_note.down.sql。

CREATE TABLE IF NOT EXISTS sandbox_demo_note (
    tenant_id TEXT    NOT NULL,
    id        TEXT    NOT NULL,
    -- 标题（元数据 maxLength 120 —— 长度约束在应用层，库层不重复实现）。
    title     TEXT    NOT NULL,
    -- 正文（元数据 maxLength 2000；TEXT 不设上限，改上限只改元数据）。
    content   TEXT    NOT NULL,
    -- 置顶。
    pinned    BOOLEAN NOT NULL DEFAULT FALSE,
    -- 标签（逗号分隔的纯文本；本期不做多值字段类型）。
    tags      TEXT,
    PRIMARY KEY (tenant_id, id)
);

CREATE INDEX IF NOT EXISTS sandbox_demo_note_pinned_idx
    ON sandbox_demo_note (tenant_id, pinned);

COMMENT ON TABLE sandbox_demo_note IS '元数据驱动渲染的第二个示例实体（不同字段组合，验证零代码渲染）';
