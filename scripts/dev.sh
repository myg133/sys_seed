#!/usr/bin/env bash
# 一条命令起全栈：数据库检查 → 种子数据 → 后端 + 前端。
#
# 低配机注意：不要同时开 Storybook / svelte-check / Playwright。
set -euo pipefail

cd "$(dirname "$0")/.."

backend_pid=""
frontend_pid=""

cleanup() {
  [ -n "$backend_pid" ] && kill "$backend_pid" 2>/dev/null || true
  [ -n "$frontend_pid" ] && kill "$frontend_pid" 2>/dev/null || true
}
trap cleanup EXIT INT TERM

# 1. 数据库检查：容器没起只提示，不擅自拉镜像（镜像可能还在下载）
if podman ps --format '{{.Names}}' 2>/dev/null | grep -qx 'base-postgres'; then
  echo "[dev] 数据库容器运行中，灌入种子数据…"
  ./scripts/seed.sh
else
  echo "[dev] 未检测到数据库容器；需要数据库时执行：bun run db:up" >&2
fi

# 2. 后端
echo "[dev] 启动后端 platform-core…"
cargo run -p platform-core --bin platform-core &
backend_pid=$!

# 3. 前端
echo "[dev] 启动前端 @base/shell…"
pnpm --filter @base/shell dev &
frontend_pid=$!

echo "[dev] 后端 http://127.0.0.1:8080/api/health ，前端 http://127.0.0.1:5173"
wait -n "$backend_pid" "$frontend_pid"
