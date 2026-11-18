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

echo "迁移（管理员连接）…"
DATABASE_URL="$ADMIN_URL" cargo run -q -p platform-server --bin base -- migrate up

echo "启动 base serve（同源，静态目录 ${BASE_STATIC_DIR}）…"
DATABASE_URL="$APP_URL" exec cargo run -q -p platform-server --bin base -- serve
