#!/usr/bin/env bash
# E2E 的**同源**服务：`platform-server` 托管 `apps/shell/dist`，API 与页面同源。
#
# 为什么不跨源：本仓**没有 CORS 层**。页面在 `:4173`、API 在 `:8080` 时
# 浏览器会拦掉 `fetch('/api/config/branding')`，标题/主色永远不会变（R17）。
#
# 前置：
#   - `DATABASE_URL_TEST`：**管理员**连接串（库名必须恰好 `base_test`）；
#   - 迁移用管理员连接跑，serve 用普通角色 `base_app`（超管会绕过 RLS，
#     `base serve` 的启动自检会直接拒绝）。
#
# 夹具：起服务前会跑 `scripts/seed-e2e.sh`（显式、幂等，只写 `base_test`）——
# E2E 需要的三个账号不再依赖任何后端测试的副作用（D101）。
set -euo pipefail

cd "$(dirname "$0")/.."

ADMIN_URL="${DATABASE_URL_TEST:-}"
if [ -z "$ADMIN_URL" ]; then
  echo "✗ 缺少 DATABASE_URL_TEST（管理员连接串，库名必须是 base_test）。" >&2
  echo "  E2E 需要先跑迁移；示例：postgres://base:base@127.0.0.1:5432/base_test" >&2
  exit 2
fi
case "$ADMIN_URL" in
  */base_dev*)
    echo "✗ 拒绝在 base_dev 上跑 E2E —— 测试库只能是 base_test。" >&2
    exit 2
    ;;
esac

# 管理员 → 应用角色（开发环境的弱口令见 deploy/seed/02-app-role.sql）。
APP_URL="$(printf '%s' "$ADMIN_URL" | sed -E 's#://[^@]+@#://base_app:base@#')"

# 复用 worktree 的共享 target（工作区布局下在 ../code/target）。
if [ -z "${CARGO_TARGET_DIR:-}" ] && [ -d ../code/target ]; then
  CARGO_TARGET_DIR="$(cd ../code/target && pwd)"
  export CARGO_TARGET_DIR
fi

export BASE_STATIC_DIR="${BASE_STATIC_DIR:-apps/shell/dist}"
export BASE_MANIFEST_ROOT="${BASE_MANIFEST_ROOT:-.}"
export BASE_LOG_LEVEL="${BASE_LOG_LEVEL:-warn}"

# ---------------------------------------------------------------------------
# 认证密钥（**必须注入，否则服务起不来** —— 见下方说明）
# ---------------------------------------------------------------------------
#
# ⚠️ **这个缺口是 REQ-021 才发现的**：脚本此前**完全没有**注入 JWT 私钥与
# IAM 密钥 → 在**干净环境**里 `base serve` 会在启动自检处失败：
#
#   ✗ 认证与账号体系初始化失败：[INTERNAL_ERROR] ... the JWT signing key is
#     not a valid PKCS#8 EC key: InvalidKeyFormat
#
# → Playwright 的 webServer 拿不到服务 → 页面收到**空响应** →
#   SDK 报 `client.INTERNAL_ERROR`（"响应无法解析"）→
#   **看起来像前端坏了，其实是服务没起来**。
#
# 之前 REQ-011/012 的 E2E 能过，是因为**手工**起过带密钥的服务，
# Playwright 的 `reuseExistingServer: true` 复用了它 —— **掩盖了这个缺口**。
#
# 密钥是**每次运行现生成**的测试密钥（不落盘、不入库、不是生产密钥）：
# - JWT 私钥必须是 **PKCS#8 EC**（不是 RSA、不是 SEC1）
# - IAM 密钥必须是 **base64url**（含 `+/=` 会触发 `IAM_CONFIG_INVALID_VALUE`）

if [ -z "${BASE_JWT_PRIVATE_KEY:-}" ]; then
  echo "生成临时 JWT 私钥（PKCS#8 EC）…"
  BASE_JWT_PRIVATE_KEY="$(openssl ecparam -name prime256v1 -genkey -noout 2>/dev/null \
    | openssl pkcs8 -topk8 -nocrypt)"
  export BASE_JWT_PRIVATE_KEY
fi
export BASE_JWT_KID="${BASE_JWT_KID:-e2e-kid}"
export BASE_JWT_ISSUER="${BASE_JWT_ISSUER:-https://e2e.base}"
export BASE_JWT_AUDIENCE="${BASE_JWT_AUDIENCE:-e2e-aud}"

if [ -z "${BASE_IAM_SECRET_KEY:-}" ]; then
  echo "生成临时 IAM 密钥（base64url）…"
  BASE_IAM_SECRET_KEY="$(head -c 32 /dev/urandom | base64 | tr '+/' '-_' | tr -d '=')"
  export BASE_IAM_SECRET_KEY
fi

echo "迁移（管理员连接）…"
DATABASE_URL="$ADMIN_URL" cargo run -q -p platform-server --bin base -- migrate up

echo "播种 E2E 夹具（显式、幂等；D101）…"
DATABASE_URL_TEST="$ADMIN_URL" ./scripts/seed-e2e.sh

echo "启动 base serve（同源，静态目录 ${BASE_STATIC_DIR}）…"
DATABASE_URL="$APP_URL" exec cargo run -q -p platform-server --bin base -- serve
