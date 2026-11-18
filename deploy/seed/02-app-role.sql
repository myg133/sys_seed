-- 应用角色（RLS 前提，REQ-006）。
--
-- ⚠️ **为什么必须有这个角色**：PostgreSQL 的 RLS 对**超级用户**与带 `BYPASSRLS`
-- 属性的角色**不生效**。开发库里 `base` 是超级用户 —— 用它连应用，RLS 形同虚设，
-- 租户隔离会 fail open（数据库层兜底失效）。
--
-- 所以应用连接必须用**普通角色**：
--
--   base_platform_app  组角色（NOLOGIN）：持有各张租户作用域表的权限
--   base_app           登录角色（开发环境用）：是 base_platform_app 的成员
--
-- 生产环境：由部署流程创建自己的登录角色，并 `GRANT base_platform_app TO <角色>`。
--
-- 幂等：可重复执行。

-- 组角色：持有租户作用域表的权限
DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'base_platform_app') THEN
        CREATE ROLE base_platform_app NOLOGIN;
    END IF;
END
$$;

-- 开发环境登录角色
--
-- ⚠️ 口令 `base` 是**开发环境专用弱口令**：只用于本机 base_dev / base_test。
--    生产环境**不得**用它 —— 生产角色由部署流程用 `APP_ROLE_PASSWORD` 建
--    （compose 的 `app-role` 步骤 / Helm 的 `app-role` hook Job），
--    口令要求见 deploy/RUNBOOK.md §3.4.2。
DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'base_app') THEN
        CREATE ROLE base_app LOGIN PASSWORD 'base';
    END IF;
END
$$;

-- 登录角色加入组角色
GRANT base_platform_app TO base_app;

-- **存量对象**的显式授权（补建场景的关键）。
--
-- ⚠️ `ALTER DEFAULT PRIVILEGES` **只影响之后创建的表** —— 对**已经存在**的表无效。
--    而「数据卷已存在、角色/权限要补建」的场景恰恰是「表早就建好了」：
--    只跑默认权限的话，存量表的权限仍然是 `f`（应用报 permission denied）。
--    所以下面这几句与后面的 `ALTER DEFAULT PRIVILEGES` **都要有**：
--    这里管存量，那边管增量。
GRANT USAGE ON SCHEMA public TO base_platform_app;
GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA public TO base_platform_app;
GRANT USAGE, SELECT ON ALL SEQUENCES IN SCHEMA public TO base_platform_app;

-- **增量**：之后由本角色（base）创建的表自动授权给应用角色组（省掉逐表 GRANT）
ALTER DEFAULT PRIVILEGES IN SCHEMA public
    GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO base_platform_app;

-- 让应用角色能连上测试库（开发库 base_dev 不由测试触碰，这里一并授权）
DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM pg_database WHERE datname = 'base_test') THEN
        EXECUTE 'GRANT CONNECT ON DATABASE base_test TO base_platform_app';
    END IF;
END
$$;
