#!/usr/bin/env bash
# 停止本地容器环境。
set -euo pipefail

cd "$(dirname "$0")/.."

export XDG_RUNTIME_DIR="/run/user/$(id -u)"
podman-compose -f compose.yaml down
