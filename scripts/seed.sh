#!/usr/bin/env bash
# 灌种子数据到 base_dev 与 base_test（幂等）。
set -euo pipefail

cd "$(dirname "$0")/.."

export XDG_RUNTIME_DIR="/run/user/$(id -u)"

if ! podman ps --format '{{.Names}}' | grep -qx 'base-postgres'; then
  echo "数据库容器未运行，请先执行：bun run db:up" >&2
  exit 1
fi

for db in base_dev base_test; do
  podman exec -i base-postgres \
    psql -v ON_ERROR_STOP=1 -U base -d "$db" <deploy/seed/01-seed.sql
  echo "已灌入种子：$db"
done
