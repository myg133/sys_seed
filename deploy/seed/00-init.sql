-- 本地开发/测试库初始化
--
-- 由 compose 挂载到 /docker-entrypoint-initdb.d/，仅在数据卷首次创建时执行。
-- 职责边界：只建库与种子登记表；业务表由各模块的迁移脚本负责。

-- 集成测试库，与开发库 base_dev 完全隔离（不使用 testcontainers）
-- CREATE DATABASE 不支持 IF NOT EXISTS，用 \gexec 做条件建库
SELECT 'CREATE DATABASE base_test OWNER base'
WHERE NOT EXISTS (SELECT FROM pg_database WHERE datname = 'base_test')
\gexec

-- 种子登记表：记录已应用的种子批次
CREATE TABLE IF NOT EXISTS platform_seed_log (
    id         serial PRIMARY KEY,
    batch      text NOT NULL UNIQUE,
    applied_at timestamptz NOT NULL DEFAULT now()
);

\connect base_test

CREATE TABLE IF NOT EXISTS platform_seed_log (
    id         serial PRIMARY KEY,
    batch      text NOT NULL UNIQUE,
    applied_at timestamptz NOT NULL DEFAULT now()
);
