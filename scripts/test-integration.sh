#!/usr/bin/env bash
# 集成测试：连本机 base_test 库，不使用 testcontainers。
#
# 规则（低配开发基线）：
#   - 复用同一个 PostgreSQL 实例的独立库 base_test
#   - 测试前后容器数量必须不变
set -euo pipefail

cd "$(dirname "$0")/.."

export XDG_RUNTIME_DIR="/run/user/$(id -u)"

before=$(podman ps --format '{{.Names}}' 2>/dev/null | wc -l | tr -d ' ')

export DATABASE_URL="${DATABASE_URL:-postgres://base:base@127.0.0.1:5432/base_test}"
export SQLX_OFFLINE="${SQLX_OFFLINE:-true}"

cargo test --workspace --tests -- --test-threads=2
status=$?

after=$(podman ps --format '{{.Names}}' 2>/dev/null | wc -l | tr -d ' ')

if [ "$before" != "$after" ]; then
  echo "集成测试期间容器数量发生变化（$before → $after），不允许新建容器" >&2
  exit 1
fi

exit "$status"
