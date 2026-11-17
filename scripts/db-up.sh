#!/usr/bin/env bash
# 启动本地容器环境（PostgreSQL + Redis）。
set -euo pipefail

cd "$(dirname "$0")/.."

export XDG_RUNTIME_DIR="/run/user/$(id -u)"
command -v podman-compose >/dev/null || {
  echo "未找到 podman-compose，请先安装 podman 与 podman-compose" >&2
  exit 1
}

podman-compose -f compose.yaml up -d
echo "等待容器健康…"
podman-compose -f compose.yaml ps
