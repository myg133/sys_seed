#!/usr/bin/env bash
# E2E 夹具播种：显式、幂等、**只写 base_test**（D101 / REQ-011 MED-NEW-2）。
#
# # 为什么需要它
#
# `e2e/shell-auth.spec.ts` 需要三个固定账号（租户 `tenant-a`），但旧版本靠
# `crates/platform-rbac/tests/me_permissions.rs` 的副作用建号：
#   - 该测试每个用例建一次、**从不清理** → 同名用户堆积（实测跑完回归后 6 个）；
#   - 它把 username 当邮箱再拼一次域名 → `u-perm-read@example.com@example.com`；
#   - `u-perm-write@example.com` 根本没有测试建。
# 结果：按「先跑集成测试 → 再跑 E2E」的顺序必红（实测 5 failed）。
#
# 现在夹具**只由本脚本创建**：E2E 需要哪些用户，下面逐条列清，
# 不再依赖任何其它测试的副作用。
#
# # 用法
#
#   ./scripts/test-integration.sh      # 会用干净 schema 重置 base_test
#   ./scripts/seed-e2e.sh              # 本脚本：幂等播种（可重复跑）
#   BASE_E2E_URL=http://127.0.0.1:8096 npx playwright test e2e/
#
# # 安全边界（照抄 scripts/test-integration.sh 的白名单闸门）
#
# 库名必须**恰好**是 `base_test`；`base_dev` 与其它任何库名一律拒绝并非 0 退出。
# 播种用**管理员**连接（造数是夹具，不是业务访问；管理员绕过 RLS 是有意为之）。
#
# # 幂等
#
# 用户 / 角色 / 授权 / 绑定全部 `ON CONFLICT` 收敛；用户已存在时**修好**
# 登录所需的字段（邮箱单域名、密码、账号状态、失败计数 / 锁定），
# 并清掉该租户的失败流水（否则残留的限流计数会让播种后的登录仍 401）。
#
# ⚠️ 同名既有行的 **id 可能不是**下面的固定值（例如该用户名早先由 IAM 注册接口 /
# 手工建过）：upsert 不修改它的 id，因此**绑定与核对一律用查出来的实际 id**
# （写死 `e2e-user-perm-*` 会触发 `platform_user_role` 的外键违约）。
# 也不改成 `DO UPDATE SET id = EXCLUDED.id` —— 外键（`platform_user_role` 之外的
# `user_org` / `user_position` / `external_identity`）没有 `ON UPDATE CASCADE`。
set -euo pipefail

cd "$(dirname "$0")/.."

export XDG_RUNTIME_DIR="/run/user/$(id -u)"

ADMIN_URL="${DATABASE_URL_TEST:-postgres://base:base@127.0.0.1:5432/base_test}"

# --- 白名单闸门：只允许 base_test --------------------------------------------
DB_NAME="$(printf '%s' "$ADMIN_URL" | sed -E 's#^[^/]*//[^/]*/([^?]*).*#\1#')"
if [ "$DB_NAME" != 'base_test' ]; then
  if [ "$DB_NAME" = 'base_dev' ]; then
    echo "✗ 拒绝播种 base_dev —— 夹具只能写测试库 base_test。" >&2
  else
    echo "✗ DATABASE_URL_TEST 指向的库 '$DB_NAME' 不是 base_test，拒绝播种（$ADMIN_URL）。" >&2
  fi
  echo "  这道闸是白名单：只有库名恰好是 base_test 才允许写入。" >&2
  exit 2
fi

# --- SQL 执行器（与 scripts/test-integration.sh 同一套约定） -------------------
if command -v psql >/dev/null 2>&1; then
  HAVE_PSQL=1
else
  HAVE_PSQL=0
fi

if [ "$HAVE_PSQL" -eq 0 ] && ! command -v podman >/dev/null 2>&1; then
  echo "✗ 既没有 psql 也没有 podman，无法连测试库。" >&2
  echo "  装一个 psql 客户端，或先启动本机 base-postgres 容器（bun run db:up）。" >&2
  exit 1
fi

run_sql() {
  if [ "$HAVE_PSQL" -eq 1 ]; then
    psql "$ADMIN_URL" -v ON_ERROR_STOP=1 -q "$@"
  else
    podman exec -i base-postgres psql -v ON_ERROR_STOP=1 -q -U base -d "$DB_NAME" "$@"
  fi
}

if ! printf 'SELECT 1;\n' | run_sql >/dev/null 2>&1; then
  echo "✗ 测试库不可达（$ADMIN_URL）。" >&2
  echo "  先启动数据库：bun run db:up" >&2
  exit 1
fi

# --- 模式检查：没有平台表就先跑迁移 ------------------------------------------
has_schema() {
  local out
  out="$(printf "SELECT to_regclass('public.platform_iam_user') IS NOT NULL;\n" | run_sql -tA)" || return 1
  [ "$out" = 't' ]
}

if ! has_schema; then
  echo "base_test 还没有平台表 → 先应用迁移（管理员连接）…"
  if [ -z "${CARGO_TARGET_DIR:-}" ] && [ -d ../code/target ]; then
    CARGO_TARGET_DIR="$(cd ../code/target && pwd)"
    export CARGO_TARGET_DIR
  fi
  DATABASE_URL="$ADMIN_URL" cargo run -q -p platform-server --bin base -- migrate up
  if ! has_schema; then
    echo "✗ 迁移后仍然缺 platform_iam_user —— 迁移可能失败了。" >&2
    exit 1
  fi
fi

# --- 夹具密码 ----------------------------------------------------------------
#
# 明文：`Tr0ub4dor&3xKq`（与 E2E spec 中的常量一致；满足密码策略：≥12 位、不在词库）。
# 下面是它的 Argon2id PHC 串（m=16384, t=2, p=1, 32 字节输出）。
# 校验走的是**串里的参数**（`platform_iam::password::verify_password`），
# 所以参数与线上配置不同也照常登录。
E2E_PASSWORD_PHC='$argon2id$v=19$m=16384,t=2,p=1$ZTJlLWZpeHR1cmUtc2FsdC0wMDAx$uNSbspROt/aqSXp9tqUGUw4ahUfIUB6pVuY+YFGqltQ'

# --- 播种 ---------------------------------------------------------------------
#
# 用户清单（E2E 需要哪些，这里就写哪些）：
#
# | 用户 | 权限点 | 用途 |
# |---|---|---|
# | u-perm-read@example.com  | rbac:role:read                      | 只读 → 深链 403 |
# | u-perm-write@example.com | rbac:role:read + rbac:role:write     | 写用户 → 深链可达 |
# | u-perm-none@example.com  | （无）                              | fail closed |
#
# id 用固定值（不是随机 uuid）：重复执行时能精确命中同一行，
# 也让「用户数不翻倍」这件事一眼可查。**同名既有行除外** —— 冲突时沿用它的 id
# （见文件头的说明），所以下面的绑定 / 核对都按实际 id 查，不写死这些常量。
echo "播种 E2E 夹具（tenant-a，管理员连接）…"
run_sql -v phc="$E2E_PASSWORD_PHC" <<'SQL'
BEGIN;

-- 1) 账号：存在则修好登录必需字段（邮箱单域名 / 密码 / 状态 / 锁定状态）。
INSERT INTO platform_iam_user (id, tenant_id, username, email, password_hash, status)
VALUES
    ('e2e-user-perm-read',  'tenant-a', 'u-perm-read@example.com',  'u-perm-read@example.com',  :'phc', 'active'),
    ('e2e-user-perm-write', 'tenant-a', 'u-perm-write@example.com', 'u-perm-write@example.com', :'phc', 'active'),
    ('e2e-user-perm-none',  'tenant-a', 'u-perm-none@example.com',  'u-perm-none@example.com',  :'phc', 'active')
ON CONFLICT (tenant_id, username) DO UPDATE SET
    email            = EXCLUDED.email,
    password_hash    = EXCLUDED.password_hash,
    status           = 'active',
    failed_attempts  = 0,
    locked_until     = NULL,
    updated_at       = now();

-- 2) 角色：两个自定义角色（非内置；内置管理员会拿到注册表全集，不是这里要的）。
INSERT INTO platform_role (tenant_id, id, code, name, data_scope, builtin)
VALUES
    ('tenant-a', 'e2e-role-read',  'e2e-role-read',  'E2E 只读角色',   'ALL', FALSE),
    ('tenant-a', 'e2e-role-write', 'e2e-role-write', 'E2E 读写角色',   'ALL', FALSE)
ON CONFLICT (tenant_id, id) DO UPDATE SET
    code       = EXCLUDED.code,
    name       = EXCLUDED.name,
    data_scope = EXCLUDED.data_scope,
    builtin    = FALSE;

-- 3) 授权：先清空这两个角色的授权再按清单写回（收敛，不积累历史垃圾）。
DELETE FROM platform_role_permission
 WHERE tenant_id = 'tenant-a'
   AND role_id IN ('e2e-role-read', 'e2e-role-write');

INSERT INTO platform_role_permission (tenant_id, role_id, permission_code, data_scope)
VALUES
    ('tenant-a', 'e2e-role-read',  'rbac:role:read',  NULL),
    ('tenant-a', 'e2e-role-write', 'rbac:role:read',  NULL),
    ('tenant-a', 'e2e-role-write', 'rbac:role:write', NULL)
ON CONFLICT (tenant_id, role_id, permission_code) DO NOTHING;

-- 4) 绑定：**用查出来的实际 id**（第 1 步的 upsert 只把同名既有行「修好登录字段」，
--    不改它的 id —— 该用户名可能早已由 IAM 注册接口 / 手工建过），
--    同样先清后写，保证「u-perm-none 一定没有角色」。
DELETE FROM platform_user_role
 WHERE tenant_id = 'tenant-a'
   AND user_id IN (
       SELECT id FROM platform_iam_user
        WHERE tenant_id = 'tenant-a'
          AND username IN ('u-perm-read@example.com', 'u-perm-write@example.com', 'u-perm-none@example.com')
   );

INSERT INTO platform_user_role (tenant_id, user_id, role_id)
SELECT 'tenant-a', u.id, r.role_id
  FROM platform_iam_user u
  JOIN (VALUES
        ('u-perm-read@example.com',  'e2e-role-read'),
        ('u-perm-write@example.com', 'e2e-role-write')
       ) AS r(username, role_id) ON r.username = u.username
 WHERE u.tenant_id = 'tenant-a'
ON CONFLICT (tenant_id, user_id, role_id) DO NOTHING;

-- 5) 权限缓存 epoch +1：任何授权 / 绑定写操作都要打失效信号。
INSERT INTO platform_permission_epoch (tenant_id, epoch)
VALUES ('tenant-a', 1)
ON CONFLICT (tenant_id) DO UPDATE SET
    epoch      = platform_permission_epoch.epoch + 1,
    updated_at = now();

-- 6) 清掉 tenant-a 的登录失败流水：限流按 (租户, IP) 滑动窗口计数，
--    上一轮 E2E / 手测留下的失败会把播种后的登录顶成 401（限流），
--    让「刚播种完就登不进去」重现。只动夹具租户，不动别的租户。
DELETE FROM platform_iam_login_attempt WHERE tenant_id = 'tenant-a';

COMMIT;
SQL

# --- 结果自证：逐条核对（失败即非 0 退出，别把「播了但不对」当成功） ----------
echo "核对夹具状态…"
run_sql <<'SQL'
WITH fixture AS (
    SELECT * FROM (VALUES
        ('u-perm-read@example.com',  2),
        ('u-perm-write@example.com', 3),
        ('u-perm-none@example.com',  1)
    ) AS t(username, expected_users)
)
SELECT u.username,
       u.email,
       u.status,
       (SELECT count(*) FROM platform_iam_user x
         WHERE x.tenant_id = 'tenant-a' AND x.username = u.username) AS user_rows,
       (SELECT count(*) FROM platform_user_role r
         WHERE r.tenant_id = 'tenant-a' AND r.user_id = u.id) AS role_rows,
       (SELECT count(*) FROM platform_role_permission p
         JOIN platform_user_role r
           ON r.tenant_id = p.tenant_id AND r.role_id = p.role_id
         WHERE r.tenant_id = 'tenant-a' AND r.user_id = u.id) AS granted_permissions
  FROM platform_iam_user u
 WHERE u.tenant_id = 'tenant-a'
   AND u.username IN ('u-perm-read@example.com', 'u-perm-write@example.com', 'u-perm-none@example.com')
 ORDER BY u.username;
SQL

# 强断言：三个用户各 **恰好 1 行**（不翻倍），权限点数目符合清单。
bad="$(printf '%s\n' \
  "SELECT count(*) FROM platform_iam_user WHERE tenant_id = 'tenant-a' AND username IN ('u-perm-read@example.com', 'u-perm-write@example.com', 'u-perm-none@example.com');" \
  "SELECT count(*) FROM platform_iam_user WHERE tenant_id = 'tenant-a' AND username = 'u-perm-read@example.com';" \
  "SELECT count(*) FROM platform_iam_user WHERE tenant_id = 'tenant-a' AND username = 'u-perm-write@example.com';" \
  "SELECT count(*) FROM platform_iam_user WHERE tenant_id = 'tenant-a' AND username = 'u-perm-none@example.com';" \
  | run_sql -tA | tr '\n' ' ')"
if [ "$bad" != '3 1 1 1 ' ]; then
  echo "✗ 夹具核对失败：期望 '3 1 1 1'，实际 '$bad'" >&2
  exit 1
fi

echo "✓ E2E 夹具就绪：u-perm-read / u-perm-write / u-perm-none（tenant-a，密码 Tr0ub4dor&3xKq）"
