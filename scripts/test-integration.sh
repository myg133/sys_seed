#!/usr/bin/env bash
# 集成测试：连本机 base_test 库，不使用 testcontainers。
#
# 规则（低配开发基线）：
#   - 复用同一个 PostgreSQL 实例的独立库 base_test
#   - 测试前重置 base_test（白名单闸门：只允许重置 base_test，绝不碰其它库）
#   - 测试前后容器数量必须不变 —— 无论测试成功还是失败，这条比对都要执行
set -euo pipefail

cd "$(dirname "$0")/.."

export XDG_RUNTIME_DIR="/run/user/$(id -u)"

# 测试库地址。白名单闸门：只有库名恰好是 base_test 才允许重置（见 reset_test_db）。
export DATABASE_URL_TEST="${DATABASE_URL_TEST:-postgres://base:base@127.0.0.1:5432/base_test}"
export DATABASE_URL="${DATABASE_URL:-$DATABASE_URL_TEST}"
export SQLX_OFFLINE="${SQLX_OFFLINE:-true}"

# 主机有没有 psql 客户端
if command -v psql >/dev/null 2>&1; then
  HOST_HAS_PSQL=1
else
  HOST_HAS_PSQL=0
fi

# --- 容器数量快照 -------------------------------------------------------------
# 返回当前运行中的容器数量。podman 不可用时返回 -1，表示"无法比对"。
container_count() {
  local names
  if ! names="$(podman ps --format '{{.Names}}' 2>/dev/null)"; then
    printf '%s\n' '-1'
    return 0
  fi
  if [ -z "$names" ]; then
    printf '%s\n' '0'
  else
    printf '%s\n' "$names" | grep -c .
  fi
}

# --- 测试库 SQL 执行器 --------------------------------------------------------
# 把 stdin 的 SQL 送到测试库执行。
# 主机装了 psql 就直连；没装则退回 podman exec 进容器内的 postgres
# （与 scripts/seed.sh 同一套约定）。执行失败必须让脚本失败。
run_test_db_sql() {
  local db="$1"
  if [ "$HOST_HAS_PSQL" -eq 1 ]; then
    psql "$DATABASE_URL_TEST" -v ON_ERROR_STOP=1
  else
    podman exec -i base-postgres psql -v ON_ERROR_STOP=1 -U base -d "$db"
  fi
}

# --- 重置测试库 ---------------------------------------------------------------
# 用途：每轮集成测试从干净 schema 开始，避免用例之间互相污染。
# 边界：**白名单** —— 只有库名恰好是 base_test 才允许重置；其它任何库名一律拒绝并非 0 退出。
# 数据库没起时给出清晰提示并跳过 —— 本地开发不常开 DB，不能静默失败。
reset_test_db() {
  # 从连接串里拆出 authority 与库名（库名去掉 query string）
  local authority host_port db
  authority="$(printf '%s' "$DATABASE_URL_TEST" | sed -E 's#^[^/]*//([^/]*)/.*#\1#')"
  host_port="${authority##*@}"
  db="$(printf '%s' "$DATABASE_URL_TEST" | sed -E 's#^[^/]*//[^/]*/([^?]*).*#\1#')"

  # 安全闸（白名单）：只有库名恰好是 base_test 才允许执行重置。
  # 其它库名（base_dev / postgres / 别的应用的库…）一律拒绝并非 0 退出 ——
  # 不能静默跳过，否则「以为重置了其实没重置」会让用例之间互相污染。
  local allowed_db="base_test"
  if [ -z "$db" ]; then
    echo "错误：DATABASE_URL_TEST 里解析不出库名，无法确认是测试库 $allowed_db，拒绝重置（$DATABASE_URL_TEST）" >&2
    echo "  安全闸为白名单：只有库名恰好是 $allowed_db 才允许重置" >&2
    return 2
  fi
  if [ "$db" != "$allowed_db" ]; then
    if [ "$db" = 'base_dev' ]; then
      echo "错误：DATABASE_URL_TEST 指向开发库 base_dev，拒绝重置开发库（$DATABASE_URL_TEST）" >&2
    else
      echo "错误：DATABASE_URL_TEST 指向的库 '$db' 不是测试库 $allowed_db，拒绝重置（$DATABASE_URL_TEST）" >&2
    fi
    echo "  安全闸为白名单：只有库名恰好是 $allowed_db 才允许重置；请改 DATABASE_URL_TEST 或手动清库" >&2
    return 2
  fi

  # 路径 1：主机装了 psql —— 直连，最通用
  if [ "$HOST_HAS_PSQL" -eq 1 ]; then
    if ! psql "$DATABASE_URL_TEST" -v ON_ERROR_STOP=1 -c 'select 1' >/dev/null 2>&1; then
      echo "跳过测试库重置：测试库不可达（$DATABASE_URL_TEST）" >&2
      echo "  本地开发不常开数据库；需要集成测试时先执行 'bun run db:up'" >&2
      return 0
    fi
  else
    # 路径 2：主机没装 psql —— 走 podman exec。
    # podman exec 进的是容器内的 postgres，只能用于指向本机 5432 的连接串
    case "$host_port" in
      127.0.0.1:5432 | localhost:5432) ;;
      *)
        echo "跳过测试库重置：主机未安装 psql，且 DATABASE_URL_TEST 不指向本机 127.0.0.1:5432" >&2
        echo "  需要集成测试时先执行 'bun run db:up'，或安装 psql 客户端" >&2
        return 0
        ;;
    esac
    if ! podman ps --format '{{.Names}}' 2>/dev/null | grep -qx 'base-postgres'; then
      echo "跳过测试库重置：主机未安装 psql，且数据库容器未运行" >&2
      echo "  本地开发不常开数据库；需要集成测试时先执行 'bun run db:up'" >&2
      return 0
    fi
    if ! podman exec base-postgres pg_isready -U base -d "$db" >/dev/null 2>&1; then
      echo "跳过测试库重置：数据库容器未就绪（$db）" >&2
      echo "  需要集成测试时先执行 'bun run db:up'" >&2
      return 0
    fi
  fi

  echo "重置测试库：$DATABASE_URL_TEST（库 $db）"
  {
    # 重置语句：清空 public schema 后重建，等价于"这个库是干净的"
    cat <<'SQL'
DROP SCHEMA IF EXISTS public CASCADE;
CREATE SCHEMA public;
SQL
    # 重建基线表：复用 deploy/seed/00-init.sql 的 DDL（过滤掉 \connect，
    # 否则它会切到 base_test，与 DATABASE_URL_TEST 指向的库不一致）
    grep -v '^\\connect' deploy/seed/00-init.sql
  } | run_test_db_sql "$db"
}

before="$(container_count)"
reset_test_db

# 测试失败也必须继续执行下面的容器数量比对，所以这里不让 set -e 直接中断
status=0
cargo test --workspace --tests -- --test-threads=2 || status=$?

after="$(container_count)"

# 容器数量比对：无论测试成功还是失败都要执行
if [ "$before" = '-1' ] || [ "$after" = '-1' ]; then
  echo "警告：podman 不可用，容器数量比对已跳过" >&2
elif [ "$before" != "$after" ]; then
  echo "集成测试期间容器数量发生变化（$before → $after），不允许新建容器" >&2
  # 容器数量变化本身就是失败；但若测试已失败，保留测试结果的退出码
  if [ "$status" -eq 0 ]; then
    status=1
  fi
else
  echo "容器数量比对通过：$before → $after（未新建容器）"
fi

exit "$status"
