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
#
# ⚠️ 审计夹具（REQ-014）需要 `platform_audit_append`（审计写通道）。
# 迁移没跑 → 夹具写不进去 → E2E 的翻页 / 过滤用例会因为「没有数据」而红。
# 所以这里把它也算进「模式是否就绪」的判据。
has_schema() {
  local out
  out="$(printf "SELECT to_regclass('public.platform_iam_user') IS NOT NULL AND to_regprocedure('public.platform_audit_append(text,text,text,text,text,text,text,text,text)') IS NOT NULL;\n" | run_sql -tA)" || return 1
  [ "$out" = 't' ]
}

if ! has_schema; then
  echo "base_test 还没有平台表 / 审计写通道 → 先应用迁移（管理员连接）…"
  if [ -z "${CARGO_TARGET_DIR:-}" ] && [ -d ../code/target ]; then
    CARGO_TARGET_DIR="$(cd ../code/target && pwd)"
    export CARGO_TARGET_DIR
  fi
  DATABASE_URL="$ADMIN_URL" cargo run -q -p platform-server --bin base -- migrate up
  if ! has_schema; then
    echo "✗ 迁移后仍然缺 platform_iam_user / platform_audit_append —— 迁移可能失败了。" >&2
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

# --- OAuth 夹具的 client secret（REQ-022 / D236）------------------------------
#
# ⚠️ **明文**是**固定值**：E2E（另一个进程）要用它换令牌 → 必须能从 stdout
# 或夹具文件读到 → 不能每次随机。
#
# ⚠️ 库里**只存摘要**：`digest_base64url(明文)` = SHA-256 的**原始 32 字节**
# → base64url **无填充** → **43 字符**（D228）—— **不是** hex。
digest_base64url() {
  printf '%s' "$1" | openssl dgst -sha256 -binary | base64 | tr -d '\n=' | tr '+/' '-_'
}

OAUTH_CLIENT_A_SECRET='e2e-oauth-client-a-secret'
OAUTH_CLIENT_B_SECRET='e2e-oauth-client-b-secret'
OAUTH_CLIENT_A_HASH="$(digest_base64url "$OAUTH_CLIENT_A_SECRET")"
OAUTH_CLIENT_B_HASH="$(digest_base64url "$OAUTH_CLIENT_B_SECRET")"

# back-channel 登出回调：回环地址的 **http**（生产必须 https；回环是唯一例外）。
#
# ⚠️ **夹具顺序钉死**（D237 §2）：**先起 mock → 再播种** —— 这个 URI 里含
# `$MOCK_PORT` → 端口必须已经定下来（`scripts/mock-logout-receiver.sh` 监听它）。
MOCK_PORT="${MOCK_PORT:-19999}"
OAUTH_BACKCHANNEL_URI="http://127.0.0.1:${MOCK_PORT}/logout"

# --- 播种 ---------------------------------------------------------------------
#
# 用户清单（E2E 需要哪些，这里就写哪些）：
#
# | 用户 | 权限点 | 用途 |
# |---|---|---|
# | u-perm-read@example.com  | rbac:role:read + sandbox:demo:read + oauth:grant:read + oauth:grant:revoke | 只读 → 能读列表、不能写；看不到 `secret` 字段 |
# | u-perm-write@example.com | rbac:role:read + rbac:role:write + sandbox:demo:read + sandbox:demo:write + sandbox:demo:secret | 写用户 → 深链可达、CRUD 可跑、能看到 `secret` 字段 |
# | u-perm-none@example.com  | （无）                              | fail closed |
# | u-perm-audit@example.com | audit:events:read                   | 审计页的数据用例（REQ-014 / AC-S-132） |
# | u-perm-app@example.com   | app:registry:read/write/delete      | **应用管理员**（REQ-016 / AC-S-156 前置） |
# | u-perm-oauth@example.com | oauth:client:read/write/delete + oauth:grant:read/revoke | **OAuth 管理员**（REQ-022 / D236） |
# | e2e-user-oauth@example.com | rbac:role:read                   | **OAuth 普通用户**（REQ-022 / D236）：授权流程的主体 |
# | u-perm-other@example.com（**tenant-b**） | rbac:role:read + oauth:grant:read + oauth:grant:revoke | **跨租户对照**（REQ-022 / D236 / TC-022-088） |
#
# `sandbox:demo:*` 是 REQ-012 的权限点（`sandbox` 模块的两个实体共用）。
# **漏授就会让 `e2e/schema-driven.spec.ts` 的列表请求 403**，而按 D108，
# 403 必须让用例 **fail**（不是 skip）—— 所以这份清单是 E2E 能否真跑的前提。
# `sandbox:demo:secret` 只授给写用户：AC-S-084 要的就是「两个用户看到的字段不同」。
#
# `audit:events:read` 是 REQ-014 的权限点（登记在
# `crates/platform-audit/manifest.json`，并由迁移
# `20261120000300_audit_permission_seed.up.sql` 写进 `platform_permission`）。
# **漏授就会让 `e2e/audit.spec.ts` 的数据用例 403** → 同样必须 fail，不能 skip。
#
# id 用固定值（不是随机 uuid）：重复执行时能精确命中同一行，
# 也让「用户数不翻倍」这件事一眼可查。**同名既有行除外** —— 冲突时沿用它的 id
# （见文件头的说明），所以下面的绑定 / 核对都按实际 id 查，不写死这些常量。
echo "播种 E2E 夹具（tenant-a，管理员连接）…"
run_sql -v phc="$E2E_PASSWORD_PHC" \
         -v secret_a="$OAUTH_CLIENT_A_HASH" \
         -v secret_b="$OAUTH_CLIENT_B_HASH" \
         -v backchannel="$OAUTH_BACKCHANNEL_URI" <<'SQL'
BEGIN;

-- 1) 账号：存在则修好登录必需字段（邮箱单域名 / 密码 / 状态 / 锁定状态）。
INSERT INTO platform_iam_user (id, tenant_id, username, email, password_hash, status)
VALUES
    ('e2e-user-perm-read',  'tenant-a', 'u-perm-read@example.com',  'u-perm-read@example.com',  :'phc', 'active'),
    ('e2e-user-perm-write', 'tenant-a', 'u-perm-write@example.com', 'u-perm-write@example.com', :'phc', 'active'),
    ('e2e-user-perm-none',  'tenant-a', 'u-perm-none@example.com',  'u-perm-none@example.com',  :'phc', 'active'),
    ('e2e-user-perm-audit', 'tenant-a', 'u-perm-audit@example.com', 'u-perm-audit@example.com', :'phc', 'active'),
    -- REQ-022（D236）：OAuth 管理员 + 普通用户（tenant-a）+ 跨租户对照（tenant-b）。
    -- REQ-016：应用注册表管理员（AC-S-156 的前置账号）。
    ('e2e-user-app',         'tenant-a', 'u-perm-app@example.com',     'u-perm-app@example.com',     :'phc', 'active'),
    ('e2e-user-oauth',       'tenant-a', 'u-perm-oauth@example.com',   'u-perm-oauth@example.com',   :'phc', 'active'),
    ('e2e-user-oauth-plain', 'tenant-a', 'e2e-user-oauth@example.com', 'e2e-user-oauth@example.com', :'phc', 'active'),
    ('e2e-user-other',       'tenant-b', 'u-perm-other@example.com',   'u-perm-other@example.com',   :'phc', 'active')
ON CONFLICT (tenant_id, username) DO UPDATE SET
    email            = EXCLUDED.email,
    password_hash    = EXCLUDED.password_hash,
    status           = 'active',
    failed_attempts  = 0,
    locked_until     = NULL,
    updated_at       = now();

-- 2) 角色：三个自定义角色（非内置；内置管理员会拿到注册表全集，不是这里要的）。
INSERT INTO platform_role (tenant_id, id, code, name, data_scope, builtin)
VALUES
    ('tenant-a', 'e2e-role-read',  'e2e-role-read',  'E2E 只读角色',   'ALL', FALSE),
    ('tenant-a', 'e2e-role-write', 'e2e-role-write', 'E2E 读写角色',   'ALL', FALSE),
    ('tenant-a', 'e2e-role-audit', 'e2e-role-audit', 'E2E 审计角色',   'ALL', FALSE),
    -- REQ-016（D271）：应用注册表管理员（3 个 app:* 权限点）。
    ('tenant-a', 'e2e-role-app',        'e2e-role-app',        'E2E 应用管理员角色',     'ALL', FALSE),
    -- REQ-022（D236）。
    ('tenant-a', 'e2e-role-oauth',      'e2e-role-oauth',      'E2E OAuth 管理员角色',   'ALL', FALSE),
    ('tenant-a', 'e2e-role-oauth-user', 'e2e-role-oauth-user', 'E2E OAuth 普通用户角色', 'ALL', FALSE),
    ('tenant-b', 'e2e-role-other',      'e2e-role-other',      'E2E 跨租户对照角色',     'ALL', FALSE)
ON CONFLICT (tenant_id, id) DO UPDATE SET
    code       = EXCLUDED.code,
    name       = EXCLUDED.name,
    data_scope = EXCLUDED.data_scope,
    builtin    = FALSE;

-- 3) 授权：先清空这些角色的授权再按清单写回（收敛，不积累历史垃圾）。
--
--    ⚠️ `tenant-b` 要**自己**的角色：`platform_role` / `platform_role_permission`
--    / `platform_user_role` 都**按租户**隔离 → 租户 b 的授权不能复用租户 a 的。
DELETE FROM platform_role_permission
 WHERE tenant_id = 'tenant-a'
   AND role_id IN ('e2e-role-read', 'e2e-role-write', 'e2e-role-audit',
                   'e2e-role-oauth', 'e2e-role-oauth-user', 'e2e-role-app');

DELETE FROM platform_role_permission
 WHERE tenant_id = 'tenant-b'
   AND role_id = 'e2e-role-other';

INSERT INTO platform_role_permission (tenant_id, role_id, permission_code, data_scope)
VALUES
    -- ⚠️ `e2e-role-read` 多了两个 OAuth 权限点（REQ-022 / TC-022-084~087）：
    -- 「我的授权」页要用**只读用户**也能看 / 撤**自己**的授权记录。
    ('tenant-a', 'e2e-role-read',  'rbac:role:read',     NULL),
    ('tenant-a', 'e2e-role-read',  'sandbox:demo:read',  NULL),
    ('tenant-a', 'e2e-role-read',  'oauth:grant:read',   NULL),
    ('tenant-a', 'e2e-role-read',  'oauth:grant:revoke', NULL),
    ('tenant-a', 'e2e-role-write', 'rbac:role:read',     NULL),
    ('tenant-a', 'e2e-role-write', 'rbac:role:write',    NULL),
    ('tenant-a', 'e2e-role-write', 'sandbox:demo:read',  NULL),
    ('tenant-a', 'e2e-role-write', 'sandbox:demo:write', NULL),
    ('tenant-a', 'e2e-role-write', 'sandbox:demo:secret', NULL),
    ('tenant-a', 'e2e-role-audit', 'audit:events:read',  NULL),
    -- REQ-016（D271）：3 个 app:* 权限点（**恰好 3 个** —— 工作台 / 导航不进权限点）。
    ('tenant-a', 'e2e-role-app',   'app:registry:read',   NULL),
    ('tenant-a', 'e2e-role-app',   'app:registry:write',  NULL),
    ('tenant-a', 'e2e-role-app',   'app:registry:delete', NULL),
    -- REQ-022（D236）：OAuth 管理员 5 个 + 普通用户 1 个。
    ('tenant-a', 'e2e-role-oauth',      'oauth:client:read',   NULL),
    ('tenant-a', 'e2e-role-oauth',      'oauth:client:write',  NULL),
    ('tenant-a', 'e2e-role-oauth',      'oauth:client:delete', NULL),
    ('tenant-a', 'e2e-role-oauth',      'oauth:grant:read',    NULL),
    ('tenant-a', 'e2e-role-oauth',      'oauth:grant:revoke',  NULL),
    ('tenant-a', 'e2e-role-oauth-user', 'rbac:role:read',      NULL),
    -- 跨租户对照（tenant-b）：能读 / 撤**自己**的授权记录 → 但**看不到** tenant-a 的。
    ('tenant-b', 'e2e-role-other',      'rbac:role:read',      NULL),
    ('tenant-b', 'e2e-role-other',      'oauth:grant:read',    NULL),
    ('tenant-b', 'e2e-role-other',      'oauth:grant:revoke',  NULL)
ON CONFLICT (tenant_id, role_id, permission_code) DO NOTHING;

-- 4) 绑定：**用查出来的实际 id**（第 1 步的 upsert 只把同名既有行「修好登录字段」，
--    不改它的 id —— 该用户名可能早已由 IAM 注册接口 / 手工建过），
--    同样先清后写，保证「u-perm-none 一定没有角色」。
DELETE FROM platform_user_role
 WHERE tenant_id = 'tenant-a'
   AND user_id IN (
       SELECT id FROM platform_iam_user
        WHERE tenant_id = 'tenant-a'
          AND username IN ('u-perm-read@example.com', 'u-perm-write@example.com', 'u-perm-none@example.com', 'u-perm-audit@example.com',
                           'u-perm-app@example.com', 'u-perm-oauth@example.com', 'e2e-user-oauth@example.com')
   );

DELETE FROM platform_user_role
 WHERE tenant_id = 'tenant-b'
   AND user_id IN (
       SELECT id FROM platform_iam_user
        WHERE tenant_id = 'tenant-b' AND username = 'u-perm-other@example.com'
   );

INSERT INTO platform_user_role (tenant_id, user_id, role_id)
SELECT 'tenant-a', u.id, r.role_id
  FROM platform_iam_user u
  JOIN (VALUES
        ('u-perm-read@example.com',  'e2e-role-read'),
        ('u-perm-write@example.com', 'e2e-role-write'),
        ('u-perm-audit@example.com', 'e2e-role-audit'),
        ('u-perm-app@example.com',   'e2e-role-app'),
        ('u-perm-oauth@example.com', 'e2e-role-oauth'),
        ('e2e-user-oauth@example.com', 'e2e-role-oauth-user')
       ) AS r(username, role_id) ON r.username = u.username
 WHERE u.tenant_id = 'tenant-a'
ON CONFLICT (tenant_id, user_id, role_id) DO NOTHING;

INSERT INTO platform_user_role (tenant_id, user_id, role_id)
SELECT 'tenant-b', u.id, 'e2e-role-other'
  FROM platform_iam_user u
 WHERE u.tenant_id = 'tenant-b' AND u.username = 'u-perm-other@example.com'
ON CONFLICT (tenant_id, user_id, role_id) DO NOTHING;

-- 5) 权限缓存 epoch +1：任何授权 / 绑定写操作都要打失效信号。
INSERT INTO platform_permission_epoch (tenant_id, epoch)
VALUES ('tenant-a', 1)
ON CONFLICT (tenant_id) DO UPDATE SET
    epoch      = platform_permission_epoch.epoch + 1,
    updated_at = now();

-- ⚠️ **tenant-b 也要 bump**（REQ-022 / D236）：它是**另一个租户**的缓存信号
-- → 只 bump tenant-a 的话，tenant-b 的用户拿到的还是旧权限集。
INSERT INTO platform_permission_epoch (tenant_id, epoch)
VALUES ('tenant-b', 1)
ON CONFLICT (tenant_id) DO UPDATE SET
    epoch      = platform_permission_epoch.epoch + 1,
    updated_at = now();

-- 6) 清掉 tenant-a 的登录失败流水：限流按 (租户, IP) 滑动窗口计数，
--    上一轮 E2E / 手测留下的失败会把播种后的登录顶成 401（限流），
--    让「刚播种完就登不进去」重现。只动夹具租户，不动别的租户。
DELETE FROM platform_iam_login_attempt WHERE tenant_id = 'tenant-a';

-- 7) SSO 入站（REQ-013）：12 个配置键（`scope: tenant`，已在
--    `crates/platform-config/manifest.json` 登记 —— 未登记的键解析不出来）。
--
--    为什么必须播种：登录页要显示「企业单点登录」入口，而
--    `GET /api/iam/sso/providers` 只列出**启用且配置合法**的 provider。
--    漏了这 12 个键 → providers 恒为空 → `e2e/sso.spec.ts` 按 D108 **直接红**
--    （403/404 不是可跳过条件）。
--
--    ⚠️ `sso.enforce` 必须是 **false**：为 true 时本地密码登录全被 403
--    `SSO_REQUIRED` 拒掉，`e2e/shell-auth.spec.ts` 的三条登录用例会一起红。
--    issuer / url 是**不会被访问**的占位地址（只做 URL 格式校验）。
INSERT INTO platform_tenant_config (tenant_id, key, value) VALUES
    ('tenant-a', 'sso.enforce',             'false'::jsonb),
    ('tenant-a', 'sso.jitProvisioning',     'false'::jsonb),
    ('tenant-a', 'sso.oidc.enabled',        'true'::jsonb),
    ('tenant-a', 'sso.oidc.issuer',         '"http://127.0.0.1:9/oidc"'::jsonb),
    ('tenant-a', 'sso.oidc.clientId',       '"e2e-client"'::jsonb),
    ('tenant-a', 'sso.oidc.scopes',         '"openid email profile"'::jsonb),
    ('tenant-a', 'sso.oidc.redirectUri',    '"http://127.0.0.1:8096/api/iam/sso/oidc/callback"'::jsonb),
    ('tenant-a', 'sso.ldap.enabled',        'true'::jsonb),
    ('tenant-a', 'sso.ldap.url',            '"ldap://127.0.0.1:389"'::jsonb),
    ('tenant-a', 'sso.ldap.baseDn',         '"dc=example,dc=com"'::jsonb),
    ('tenant-a', 'sso.ldap.userSearchFilter', '"(uid={username})"'::jsonb),
    ('tenant-a', 'sso.ldap.mailAttribute',  '"mail"'::jsonb)
ON CONFLICT (tenant_id, key) DO UPDATE SET
    value      = EXCLUDED.value,
    updated_at = now();

-- 配置缓存版本：resolver **成对比较** `platform` 与 `tenant:<id>` 两行（D71）。
-- 直接写表而不 bump epoch 的话，已启动的实例会继续用旧快照。
INSERT INTO platform_config_epoch (scope_key, epoch) VALUES ('platform', 1)
ON CONFLICT (scope_key) DO UPDATE SET
    epoch      = platform_config_epoch.epoch + 1,
    updated_at = now();
INSERT INTO platform_config_epoch (scope_key, epoch) VALUES ('tenant:tenant-a', 1)
ON CONFLICT (scope_key) DO UPDATE SET
    epoch      = platform_config_epoch.epoch + 1,
    updated_at = now();

-- 8) REQ-014 的审计记录夹具（AC-S-132 的翻页 / 过滤用例需要）。
--
--    # 为什么走 `platform_audit_append`（而不是直接 INSERT）
--
--    直接 INSERT 的行 `hash` / `prev_hash` 与内容**不自洽** → 链校验必然报断裂，
--    还会连累别的判据。`platform_audit_append` 是**平台唯一的审计写通道**
--    （SECURITY DEFINER；同租户 advisory lock；敏感键硬拒绝；`clock_timestamp()`
--      生成时间戳；规范事件体 → sha256）→ 用它造的夹具**链是自洽的**，
--    不参与链校验断言也不会污染任何别的判据。
--
--    # 幂等（D101）
--
--    按 (tenant_id, object_type, object_id) 判存在：已存在就**不重复写**。
--    既不删行也不改行 → 重跑不产生重复数据，也**不打断**已有的链。
--
--    # 为什么是 11 条
--
--    翻页用例把每页条数调到 10 → 第 1 页 10 条 + 第 2 页 1 条
--    → 响应带 `nextCursor`（「下一页」可点）→ 「翻页后记录不重复」可判。
--
--    # 形状
--
--    `actor_id` / `action` **互不相同** → 按操作人 / 按动作过滤各自只命中 1 条
--    （QA 的 TC-014-115 / TC-014-116 的前置条件）。
--    `ip` 给一个文档地址段（TEST-NET-3）→ 「来源 IP」列非空（TC-014-112）。
DO $$
DECLARE
    v_index  INTEGER;
    v_actor  TEXT;
    v_object TEXT;
    v_action TEXT;
BEGIN
    FOR v_index IN 1..11 LOOP
        v_actor  := format('e2e-actor-%s',  lpad(v_index::text, 2, '0'));
        v_object := format('e2e-obj-%s',    lpad(v_index::text, 2, '0'));
        v_action := format('e2e.fixture.item-%s', lpad(v_index::text, 2, '0'));
        IF NOT EXISTS (
            SELECT 1
              FROM platform_audit_log
             WHERE tenant_id = 'tenant-a'
               AND object_type = 'e2e_fixture'
               AND object_id = v_object
        ) THEN
            PERFORM platform_audit_append(
                'tenant-a',
                v_actor,
                v_action,
                'e2e_fixture',
                v_object,
                '{"name":"old"}',
                '{"name":"new"}',
                '203.0.113.10',
                'e2e-seed-agent'
            );
        END IF;
    END LOOP;
END
$$;

-- 9) REQ-022 的 OAuth client 夹具（D236）。
--
--    ⚠️ `platform_oauth_client` 是**平台级**表（**无** `tenant_id`、**不启用**
--    RLS，D219）→ 所有租户共享同一批 client。
--
--    ⚠️ `client_secret_hash` = `digest_base64url(明文)`（43 字符的 base64url
--    无填充摘要）—— 由脚本用 `openssl` + `base64` **独立算**，**不是** hex（D228）。
--
--    ⚠️ `backchannel_logout_uri` 含 `$MOCK_PORT` → **先起 mock → 再播种**。
INSERT INTO platform_oauth_client (
    client_id, client_secret_hash, client_type, name, redirect_uris,
    allowed_grant_types, allowed_scopes, backchannel_logout_uri,
    frontchannel_logout_uri, status, created_at
)
VALUES
    ('client-A', :'secret_a', 'confidential', 'E2E 应用 A',
     ARRAY['https://client.example/cb'],
     ARRAY['authorization_code', 'client_credentials', 'urn:ietf:params:oauth:grant-type:device_code'],
     ARRAY['rbac:role:read'],
     :'backchannel', 'https://client.example/front', 'active', now()),
    -- ⚠️ client-B **不**配 front-channel URI（AC-S-150：front-channel 列表
    -- 只含**配了**的 client）。
    ('client-B', :'secret_b', 'confidential', 'E2E 应用 B',
     ARRAY['https://client.example/cb'],
     ARRAY['client_credentials'],
     ARRAY['rbac:role:read'],
     :'backchannel', NULL, 'active', now()),
    -- ⚠️ public client **没有** secret（`client_secret_hash` 为 NULL）→
    -- 只能用 `client_id`（`none` 方式）认证（AC-S-143 步骤 5）。
    ('public-client', NULL, 'public', 'E2E 公开应用',
     ARRAY['https://client.example/cb'],
     ARRAY['authorization_code'],
     ARRAY['rbac:role:read'],
     NULL, NULL, 'active', now())
ON CONFLICT (client_id) DO UPDATE SET
    client_secret_hash      = EXCLUDED.client_secret_hash,
    client_type             = EXCLUDED.client_type,
    name                    = EXCLUDED.name,
    redirect_uris           = EXCLUDED.redirect_uris,
    allowed_grant_types     = EXCLUDED.allowed_grant_types,
    allowed_scopes          = EXCLUDED.allowed_scopes,
    backchannel_logout_uri  = EXCLUDED.backchannel_logout_uri,
    frontchannel_logout_uri = EXCLUDED.frontchannel_logout_uri,
    status                  = 'active';

-- 8) 应用接入（REQ-016）：tenant-a 的 5 个应用（AC-S-160 的分组/排序/隐藏判据）
--    + tenant-b 的 1 个应用（AC-S-157 的跨租户对照）。
--
--    ⚠️ **每个应用用自己的一次性 client**（`client-app-<x>`）：
--    `UNIQUE (tenant_id, client_id)` 规定**同一租户不能用一个 client 接两个应用** →
--    复用 `client-A`（下面 AC-S-156 / 159 / 162 还要拿它建应用）会直接撞唯一索引。
--    这也让本脚本可以反复执行（幂等）。
--
--    ⚠️ **client 必须先存在**（`ON DELETE CASCADE` 的 FK，D264）。
--
--    ⚠️ **工作台不按 `launch_url` 过滤**（D279）：这里都给了入口，便于导航用例；
--    工作台用例不依赖它。
--
--    ⚠️ 这里用**裸连接**（脚本跑在管理员角色上）→ RLS 不挡写入（表 owner / 超管）。
INSERT INTO platform_oauth_client
    (client_id, client_secret_hash, client_type, name, redirect_uris,
     allowed_grant_types, allowed_scopes, status, created_at)
VALUES
    ('client-app-a',     NULL, 'public', 'E2E 应用 A 的 client',
     ARRAY['https://client.example/cb'], ARRAY['authorization_code'], ARRAY['rbac:role:read'], 'active', now()),
    ('client-app-b',     NULL, 'public', 'E2E 应用 B 的 client',
     ARRAY['https://client.example/cb'], ARRAY['authorization_code'], ARRAY['rbac:role:read'], 'active', now()),
    ('client-app-c',     NULL, 'public', 'E2E 应用 C 的 client',
     ARRAY['https://client.example/cb'], ARRAY['authorization_code'], ARRAY['rbac:role:read'], 'active', now()),
    ('client-app-d',     NULL, 'public', 'E2E 应用 D 的 client',
     ARRAY['https://client.example/cb'], ARRAY['authorization_code'], ARRAY['rbac:role:read'], 'active', now()),
    ('client-app-e',     NULL, 'public', 'E2E 应用 E 的 client',
     ARRAY['https://client.example/cb'], ARRAY['authorization_code'], ARRAY['rbac:role:read'], 'active', now()),
    ('client-app-other', NULL, 'public', 'B 租户应用的 client',
     ARRAY['https://client.example/cb'], ARRAY['authorization_code'], ARRAY['rbac:role:read'], 'active', now())
ON CONFLICT (client_id) DO UPDATE SET status = 'active';

INSERT INTO platform_app
    (id, tenant_id, client_id, display_name, description, category, icon_url,
     launch_url, sort_order, status, created_at, created_by)
VALUES
    -- tenant-a：AC-S-160 步骤 3 的**钉死期望**——finance 组 B(10)→A(20)；
    -- general 组 C 与 D 的 sortOrder 都 0 → 按 displayName 升序（C → D）。
    -- ⚠️ **id 与 displayName 的顺序刻意相反**（C 叫 "e2e-app-z"、D 叫 "e2e-app-d0"）：
    --    让「按 id 兜底排序」的实现得到 D → C（错），只有按 displayName 才是 C → D（对）。
    ('e2e-app-a',   'tenant-a', 'client-app-a', 'E2E 差旅报销',  'AC-S-160 的 A', 'finance', NULL,
     'https://a.example/',  20, 'active', now(), 'seed-e2e'),
    ('e2e-app-b',   'tenant-a', 'client-app-b', 'E2E 费用申请',  'AC-S-160 的 B', 'finance', NULL,
     'https://b.example/',  10, 'active', now(), 'seed-e2e'),
    ('e2e-app-z',   'tenant-a', 'client-app-c', 'E2E C 应用',    'AC-S-160 的 C', 'general', NULL,
     'https://c.example/',   0, 'active', now(), 'seed-e2e'),
    ('e2e-app-d0',  'tenant-a', 'client-app-d', 'E2E D 应用',    'AC-S-160 的 D', 'general', NULL,
     'https://d.example/',   0, 'active', now(), 'seed-e2e'),
    -- E 是 **hidden**（AC-S-160 步骤 3：不在任何组里）。
    ('e2e-app-e',   'tenant-a', 'client-app-e', 'E2E E 应用',    'AC-S-160 的 E（hidden）', 'general', NULL,
     'https://e.example/',   0, 'hidden', now(), 'seed-e2e'),
    -- tenant-b：跨租户对照（AC-S-157）。
    ('e2e-app-other', 'tenant-b', 'client-app-other', 'B 租户的应用', 'AC-S-157 的跨租户对照', 'general', NULL,
     'https://other.example/', 0, 'active', now(), 'seed-e2e')
ON CONFLICT (id) DO UPDATE SET
    client_id    = EXCLUDED.client_id,
    display_name = EXCLUDED.display_name,
    category     = EXCLUDED.category,
    launch_url   = EXCLUDED.launch_url,
    sort_order   = EXCLUDED.sort_order,
    status       = EXCLUDED.status;

COMMIT;
SQL

# --- 结果自证：逐条核对（失败即非 0 退出，别把「播了但不对」当成功） ----------
echo "核对夹具状态…"
run_sql <<'SQL'
WITH fixture AS (
    SELECT * FROM (VALUES
        ('u-perm-read@example.com',  2),
        ('u-perm-write@example.com', 3),
        ('u-perm-none@example.com',  1),
        ('u-perm-audit@example.com', 1)
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
   AND u.username IN ('u-perm-read@example.com', 'u-perm-write@example.com', 'u-perm-none@example.com', 'u-perm-audit@example.com')
 ORDER BY u.username;
SQL

# 强断言：四个用户各 **恰好 1 行**（不翻倍），权限点数目符合清单。
bad="$(printf '%s\n' \
  "SELECT count(*) FROM platform_iam_user WHERE tenant_id = 'tenant-a' AND username IN ('u-perm-read@example.com', 'u-perm-write@example.com', 'u-perm-none@example.com', 'u-perm-audit@example.com', 'u-perm-app@example.com', 'u-perm-oauth@example.com', 'e2e-user-oauth@example.com');" \
  "SELECT count(*) FROM platform_iam_user WHERE tenant_id = 'tenant-a' AND username = 'u-perm-read@example.com';" \
  "SELECT count(*) FROM platform_iam_user WHERE tenant_id = 'tenant-a' AND username = 'u-perm-write@example.com';" \
  "SELECT count(*) FROM platform_iam_user WHERE tenant_id = 'tenant-a' AND username = 'u-perm-none@example.com';" \
  "SELECT count(*) FROM platform_iam_user WHERE tenant_id = 'tenant-a' AND username = 'u-perm-audit@example.com';" \
  "SELECT count(*) FROM platform_iam_user WHERE tenant_id = 'tenant-a' AND username = 'u-perm-app@example.com';" \
  "SELECT count(*) FROM platform_iam_user WHERE tenant_id = 'tenant-a' AND username = 'u-perm-oauth@example.com';" \
  "SELECT count(*) FROM platform_iam_user WHERE tenant_id = 'tenant-a' AND username = 'e2e-user-oauth@example.com';" \
  "SELECT count(*) FROM platform_iam_user WHERE tenant_id = 'tenant-b' AND username = 'u-perm-other@example.com';" \
  | run_sql -tA | tr '\n' ' ')"
if [ "$bad" != '7 1 1 1 1 1 1 1 1 ' ]; then
  echo "✗ 夹具核对失败：期望 '7 1 1 1 1 1 1 1 1'，实际 '$bad'" >&2
  exit 1
fi

# 强断言 ②：**权限点数目**也要对（只数用户行数的话，授权漏了也 exit 0）。
#
# 为什么必须断言这一条：REQ-012 的 E2E 曾因为本脚本没授 `sandbox:demo:*` 而
# **6 条全部 skip、exit 0**（假绿）。D108 让 E2E 侧变红了，但**播种脚本自己也该自证** ——
# 否则「脚本成功」和「夹具可用」是两件事。
#
# 期望：read=2（rbac:role:read + sandbox:demo:read）
#       write=5（rbac:role:read/write + sandbox:demo:read/write/secret）
#       none=0
#       audit=1（audit:events:read）
grants="$(printf '%s\n' \
  "SELECT count(*) FROM platform_role_permission p JOIN platform_user_role r ON r.tenant_id = p.tenant_id AND r.role_id = p.role_id JOIN platform_iam_user u ON u.tenant_id = r.tenant_id AND u.id = r.user_id WHERE u.tenant_id = 'tenant-a' AND u.username = 'u-perm-read@example.com';" \
  "SELECT count(*) FROM platform_role_permission p JOIN platform_user_role r ON r.tenant_id = p.tenant_id AND r.role_id = p.role_id JOIN platform_iam_user u ON u.tenant_id = r.tenant_id AND u.id = r.user_id WHERE u.tenant_id = 'tenant-a' AND u.username = 'u-perm-write@example.com';" \
  "SELECT count(*) FROM platform_role_permission p JOIN platform_user_role r ON r.tenant_id = p.tenant_id AND r.role_id = p.role_id JOIN platform_iam_user u ON u.tenant_id = r.tenant_id AND u.id = r.user_id WHERE u.tenant_id = 'tenant-a' AND u.username = 'u-perm-none@example.com';" \
  "SELECT count(*) FROM platform_role_permission p JOIN platform_user_role r ON r.tenant_id = p.tenant_id AND r.role_id = p.role_id JOIN platform_iam_user u ON u.tenant_id = r.tenant_id AND u.id = r.user_id WHERE u.tenant_id = 'tenant-a' AND u.username = 'u-perm-audit@example.com';" \
  "SELECT count(*) FROM platform_role_permission p JOIN platform_user_role r ON r.tenant_id = p.tenant_id AND r.role_id = p.role_id JOIN platform_iam_user u ON u.tenant_id = r.tenant_id AND u.id = r.user_id WHERE u.tenant_id = 'tenant-a' AND u.username = 'u-perm-app@example.com';" \
  "SELECT count(*) FROM platform_role_permission p JOIN platform_user_role r ON r.tenant_id = p.tenant_id AND r.role_id = p.role_id JOIN platform_iam_user u ON u.tenant_id = r.tenant_id AND u.id = r.user_id WHERE u.tenant_id = 'tenant-a' AND u.username = 'u-perm-oauth@example.com';" \
  "SELECT count(*) FROM platform_role_permission p JOIN platform_user_role r ON r.tenant_id = p.tenant_id AND r.role_id = p.role_id JOIN platform_iam_user u ON u.tenant_id = r.tenant_id AND u.id = r.user_id WHERE u.tenant_id = 'tenant-a' AND u.username = 'e2e-user-oauth@example.com';" \
  "SELECT count(*) FROM platform_role_permission p JOIN platform_user_role r ON r.tenant_id = p.tenant_id AND r.role_id = p.role_id JOIN platform_iam_user u ON u.tenant_id = r.tenant_id AND u.id = r.user_id WHERE u.tenant_id = 'tenant-b' AND u.username = 'u-perm-other@example.com';" \
  | run_sql -tA | tr '\n' ' ')"
if [ "$grants" != '4 5 0 1 3 5 1 3 ' ]; then
  echo "✗ 夹具授权核对失败：期望 read=4 / write=5 / none=0 / audit=1 / app=3 / oauth=5 / oauth-user=1 / other=3，实际 '$grants'" >&2
  echo "  （REQ-012 的 E2E 需要 sandbox:demo:read/write/secret；REQ-014 的需要 audit:events:read；漏授会让用例 403。）" >&2
  exit 1
fi

# 强断言 ③：REQ-013 的 12 个 SSO 配置键**全部**已播种。
# 「脚本成功」不等于「夹具可用」—— 缺键时 providers 恒为空，E2E 才红。
sso_keys="$(printf '%s\n' \
  "SELECT count(*) FROM platform_tenant_config WHERE tenant_id = 'tenant-a' AND key IN ('sso.enforce','sso.jitProvisioning','sso.oidc.enabled','sso.oidc.issuer','sso.oidc.clientId','sso.oidc.scopes','sso.oidc.redirectUri','sso.ldap.enabled','sso.ldap.url','sso.ldap.baseDn','sso.ldap.userSearchFilter','sso.ldap.mailAttribute');" \
  | run_sql -tA | tr -d '[:space:]')"
if [ "$sso_keys" != '12' ]; then
  echo "✗ SSO 配置键核对失败：期望 12 个，实际 '$sso_keys'" >&2
  exit 1
fi

# REQ-018（AC-S-183 前置条件）：**两个租户各**播种 1 个文件 + 1 个任务 + 1 条错误回执
# + 1 行导入目标实体。
#
# ⚠️ 为什么必须两个租户都有：AC-S-183 的判据是「tenant-a 的列表里**不含** tenant-b 的行」——
# 只播一个租户的话，「隔离生效」和「压根没数据」是**同一个结果**，判据没有区分力。
#
# ⚠️ `platform_storage_probe_row` 也要播：它是 D333 新增的导入真实落点，
# 存的是**业务数据**（不是元数据），漏隔离最严重。
for storage_tenant in tenant-a tenant-b; do
  printf '%s\n' \
    "INSERT INTO platform_storage_object
       (id, tenant_id, object_key, size_bytes, sha256, content_type, scan_verdict, status)
     VALUES (gen_random_uuid(), '$storage_tenant', 'e2e/$storage_tenant.txt', 5,
             'aa', 'text/plain', 'skipped', 'available')
     ON CONFLICT (tenant_id, object_key) DO NOTHING;

     INSERT INTO platform_storage_upload
       (upload_id, tenant_id, object_key, total_parts, part_size, status, expires_at)
     VALUES (gen_random_uuid(), '$storage_tenant', 'e2e/$storage_tenant.bin', 1, 512,
             'pending', now() + interval '1 day');

     INSERT INTO platform_storage_job
       (id, tenant_id, kind, entity, status, total_rows, processed_rows, failed_rows)
     VALUES ('00000000-0000-4000-8000-00000000${storage_tenant#tenant-}0',
             '$storage_tenant', 'import', 'probe_row', 'completed', 1, 1, 0)
     ON CONFLICT (id) DO NOTHING;

     INSERT INTO platform_storage_job_error (job_id, tenant_id, row_number, \"column\", reason)
     SELECT id, '$storage_tenant', 2, 'amount', 'the value is not a valid integer'
     FROM platform_storage_job
     WHERE tenant_id = '$storage_tenant' LIMIT 1
     ON CONFLICT (job_id, row_number, \"column\") DO UPDATE SET reason = EXCLUDED.reason;

     INSERT INTO platform_storage_probe_row (id, tenant_id, job_id, email, amount, note)
     SELECT gen_random_uuid(), '$storage_tenant', id, 'e2e-$storage_tenant@example.com', 1, 'e2e'
     FROM platform_storage_job
     WHERE tenant_id = '$storage_tenant' LIMIT 1
     ON CONFLICT (tenant_id, job_id, email) DO NOTHING;" \
    | run_sql >/dev/null
done

# 强断言 ④：REQ-014 的审计夹具**至少 11 条**（AC-S-132 的翻页用例：
# 每页 10 条 → 第 2 页还有 1 条 → 响应带 nextCursor）。
#
# 少一条 → 翻页用例的「下一页」是禁用的 → 用例红。
# 这里断 **≥ 11** 而不是 **== 11**：登录等真实事件也会写审计表，
# 总数可以更多，但夹具行本身必须齐（用 object_type 精确取）。
audit_fixture="$(printf '%s\n' \
  "SELECT count(*) FROM platform_audit_log WHERE tenant_id = 'tenant-a' AND object_type = 'e2e_fixture';" \
  | run_sql -tA | tr -d '[:space:]')"
if [ "${audit_fixture:-0}" -lt 11 ]; then
  echo "✗ 审计夹具核对失败：期望至少 11 条，实际 '$audit_fixture'" >&2
  echo "  （AC-S-132 的翻页用例需要两页；每页 10 条 → 第 2 页必须还有记录。）" >&2
  exit 1
fi

# 强断言 ④b：REQ-018 的存储夹具**两个租户各 4 张表都有行**（AC-S-183 前置条件）。
#
# ⚠️ 断言的是**每张表在每个租户下都恰好 1 行** ——
# 少播一张表，AC-S-183 步骤 3 的「五张表各自只返回本租户的行」就有一张是空断言
# （空集合 trivially 只包含本租户的行 → 假绿）。
# ⚠️ **5 张表 × 2 个租户 = 恰好 10 行**（复核 R-04：原来漏播 `platform_storage_upload`）。
#
# 用 `-ne 10`（**精确相等**）而不是 `-lt 8`：
# AC-S-183 步骤 3 要对**每张表每个租户**验隔离，任何一张空表都会让
# 「只返回本租户的行」在空集合上 trivially 成立 = 假绿。
# 精确相等还能抓住「多播了」（脚本重复执行导致行数翻倍）。
storage_fixture="$(printf '%s\n' \
  "SELECT count(*) FROM (
     SELECT 'object' AS t, tenant_id FROM platform_storage_object WHERE object_key LIKE 'e2e/%'
     UNION ALL SELECT 'upload', tenant_id FROM platform_storage_upload WHERE object_key LIKE 'e2e/%'
     UNION ALL SELECT 'job', tenant_id FROM platform_storage_job WHERE entity = 'probe_row'
     UNION ALL SELECT 'job_error', tenant_id FROM platform_storage_job_error WHERE \"column\" = 'amount'
     UNION ALL SELECT 'probe_row', tenant_id FROM platform_storage_probe_row WHERE note = 'e2e'
   ) x WHERE tenant_id IN ('tenant-a','tenant-b');" \
  | run_sql -tA | tr -d '[:space:]')"
if [ "${storage_fixture:-0}" -ne 10 ]; then
  echo "✗ 存储夹具核对失败：期望恰好 10 行（5 表 × 2 租户），实际 '$storage_fixture'" >&2
  echo "  （AC-S-183 要对**每张表每个租户**验 RLS；空表会让隔离断言变成假绿。）" >&2
  exit 1
fi

# 强断言 ⑤：REQ-022 的 OAuth 夹具**存在且绑定正确**（D236 §3）。
#
# 「脚本成功」不等于「夹具可用」—— client 没建 / URI 写错 / secret 摘要口径不对
# → E2E 才红，而那时看的是「前端坏了」的错误码。
#
# ⚠️ 逐项断：
# - 三个 client 各**恰好 1 行**；
# - `client-A`：confidential + back-channel URI **就是** `$MOCK_PORT` 那个
#   + front-channel URI 非空 + 摘要**恰好 43 字符**（base64url 无填充）；
# - `client-B`：confidential + **没有** front-channel URI（AC-S-150）；
# - `public-client`：public + **没有** secret（`client_secret_hash IS NULL`）。
oauth_clients="$(printf '%s\n' \
  "SELECT count(*) FROM platform_oauth_client WHERE client_id IN ('client-A', 'client-B', 'public-client');" \
  "SELECT count(*) FROM platform_oauth_client WHERE client_id = 'client-A' AND client_type = 'confidential' AND backchannel_logout_uri = '$OAUTH_BACKCHANNEL_URI' AND frontchannel_logout_uri IS NOT NULL AND length(client_secret_hash) = 43;" \
  "SELECT count(*) FROM platform_oauth_client WHERE client_id = 'client-B' AND client_type = 'confidential' AND frontchannel_logout_uri IS NULL AND length(client_secret_hash) = 43;" \
  "SELECT count(*) FROM platform_oauth_client WHERE client_id = 'public-client' AND client_type = 'public' AND client_secret_hash IS NULL;" \
  | run_sql -tA | tr '\n' ' ')"
if [ "$oauth_clients" != '3 1 1 1 ' ]; then
  echo "✗ OAuth 夹具核对失败：期望 '3 1 1 1'（三个 client / client-A / client-B / public-client），实际 '$oauth_clients'" >&2
  echo "  （client-A 的 back-channel URI 应为 $OAUTH_BACKCHANNEL_URI —— 夹具顺序要求**先起 mock**再播种。）" >&2
  exit 1
fi

# 明文 secret：**打印到 stdout**（D236 §2）→ E2E 从 stdout / 夹具文件读。
#
# ⚠️ 同时写一份**夹具文件**（`target/` 下、**不入 Git**）：E2E 是**另一个进程**
# → 读不到本脚本的 stdout。
echo "✓ E2E 夹具就绪：u-perm-read / u-perm-write / u-perm-none / u-perm-audit / u-perm-oauth / e2e-user-oauth（tenant-a）+ u-perm-other（tenant-b）"
echo "  授权：read=4 / write=5 / none=0 / audit=1 / oauth=5 / oauth-user=1 / other=3 个权限点"
echo "  审计夹具：$audit_fixture 条（object_type = e2e_fixture）"
echo "  存储夹具：$storage_fixture 行（5 表 × 2 租户，REQ-018 / AC-S-183）"
echo "  OAuth client：client-A / client-B / public-client（back-channel → $OAUTH_BACKCHANNEL_URI）"
echo "  OAuth client 明文 secret（只此一次，库里只存摘要）："
echo "    client-A    = $OAUTH_CLIENT_A_SECRET"
echo "    client-B    = $OAUTH_CLIENT_B_SECRET"
echo "    public-client = （无 secret，public 类型）"

# 夹具文件：给 E2E 读（`target/` 已被 .gitignore 忽略 → 不入库）。
mkdir -p target
cat > target/e2e-oauth-clients.json <<JSON
{
  "backchannelLogoutUri": "$OAUTH_BACKCHANNEL_URI",
  "clients": [
    { "clientId": "client-A", "clientSecret": "$OAUTH_CLIENT_A_SECRET", "clientType": "confidential" },
    { "clientId": "client-B", "clientSecret": "$OAUTH_CLIENT_B_SECRET", "clientType": "confidential" },
    { "clientId": "public-client", "clientSecret": null, "clientType": "public" }
  ]
}
JSON
