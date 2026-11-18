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
DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'base_app') THEN
        CREATE ROLE base_app LOGIN PASSWORD 'base';
    END IF;
END
$$;

-- 登录角色加入组角色
GRANT base_platform_app TO base_app;

-- 之后由本角色（base）创建的表自动授权给应用角色组（省掉逐表 GRANT）
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
