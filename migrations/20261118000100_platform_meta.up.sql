-- 平台元数据表。
--
-- 平台自身的部署期元数据（安装信息、版本标记等），与业务数据无关：
-- 业务模块的表由各模块自己的迁移负责（见 modules/<name>/migrations/）。
--
-- 可逆：配套 20261118000100_platform_meta.down.sql。

CREATE TABLE IF NOT EXISTS platform_meta (
    key        TEXT PRIMARY KEY,
    value      TEXT        NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

COMMENT ON TABLE platform_meta IS '平台自身元数据（部署期基建）';
