#!/usr/bin/env bash
# 数据库脚本共用逻辑：连接串解析 + 在数据库容器内执行 psql / pg_dump。
#
# 为什么走容器内执行：
#   本机没装 psql 客户端（见 docs/dev-environment.md），而且在容器里用本地
#   socket 连接（官方 postgres 镜像的 pg_hba 默认 `local all all trust`），
#   **口令既不落到命令行、也不落到进程列表**。
#
# 环境变量：
#   DATABASE_URL   必填，数据库连接串
#   PG_CONTAINER   数据库容器名（默认 base-postgres）
#   PG_ENGINE      容器运行时（默认自动探测 podman / docker）

set -euo pipefail

# 数据库容器名
: "${PG_CONTAINER:=base-postgres}"

# 容器运行时：自动探测
if [ -z "${PG_ENGINE:-}" ]; then
  if command -v podman >/dev/null 2>&1; then
    PG_ENGINE=podman
  elif command -v docker >/dev/null 2>&1; then
    PG_ENGINE=docker
  else
    echo "✗ 找不到 podman / docker，无法在容器内执行数据库命令" >&2
    exit 1
  fi
fi

# 执行模式：local（本地装了客户端）/ container（容器内执行）
PG_MODE=""

# 解析后的连接参数
PGHOST=""
PGPORT="5432"
PGUSER=""
PGPASSWORD=""
PGDATABASE=""

# 解析 DATABASE_URL。
#
# 支持 `postgres://user:password@host:port/db?params`；
# 口令不做 URL 解码 —— 容器内执行不需要口令，本地执行遇到特殊字符请自行转义。
pg_parse_url() {
  local url="${1:-}"
  if [ -z "$url" ]; then
    echo "✗ 缺少 DATABASE_URL" >&2
    exit 1
  fi

  local rest="${url#*://}"
  local creds=""
  if [[ "$rest" == *"@"* ]]; then
    creds="${rest%%@*}"
    rest="${rest#*@}"
  fi

  PGUSER="$creds"
  PGPASSWORD=""
  if [[ "$creds" == *":"* ]]; then
    PGUSER="${creds%%:*}"
    PGPASSWORD="${creds#*:}"
  fi

  local hostport="$rest"
  local path=""
  if [[ "$hostport" == *"/"* ]]; then
    path="${hostport#*/}"
    hostport="${hostport%%/*}"
  fi

  PGHOST="$hostport"
  if [[ "$hostport" == *":"* ]]; then
    PGHOST="${hostport%%:*}"
    PGPORT="${hostport##*:}"
  fi

  PGDATABASE="${path%%\?*}"
  if [ -z "$PGDATABASE" ]; then
    echo "✗ DATABASE_URL 里没有数据库名：${url}" >&2
    exit 1
  fi
}

# 容器是否在运行
pg_container_running() {
  "$PG_ENGINE" ps --format '{{.Names}}' 2>/dev/null | grep -qx "$PG_CONTAINER"
}

# 决定执行模式：本地有客户端就用本地，否则用容器。
pg_detect_mode() {
  local client="$1"
  if command -v "$client" >/dev/null 2>&1; then
    PG_MODE="local"
    return 0
  fi
  if pg_container_running; then
    PG_MODE="container"
    return 0
  fi
  echo "✗ 本机没有 ${client} 客户端，数据库容器 ${PG_CONTAINER} 也没运行" >&2
  echo "  装客户端：sudo pacman -S postgresql   或   起容器：bun run db:up" >&2
  exit 1
}

# 在目标数据库上执行 SQL（从 stdin 读），返回命令的退出码。
# 查询结果丢弃（-o /dev/null）：错误仍走 stderr，ON_ERROR_STOP 保证出错即失败。
pg_run_sql() {
  if [ "$PG_MODE" = "local" ]; then
    PGPASSWORD="$PGPASSWORD" psql -v ON_ERROR_STOP=1 -h "$PGHOST" -p "$PGPORT" \
      -U "$PGUSER" -d "$PGDATABASE" -q -o /dev/null "$@"
  else
    "$PG_ENGINE" exec -i "$PG_CONTAINER" psql -v ON_ERROR_STOP=1 -q -o /dev/null \
      -U "$PGUSER" -d "$PGDATABASE" "$@"
  fi
}

# 执行一条只返回结果的 SQL（-At：只取值，不带表头）。
pg_query_scalar() {
  if [ "$PG_MODE" = "local" ]; then
    PGPASSWORD="$PGPASSWORD" psql -At -v ON_ERROR_STOP=1 -h "$PGHOST" -p "$PGPORT" \
      -U "$PGUSER" -d "$PGDATABASE" -c "$1"
  else
    "$PG_ENGINE" exec -i "$PG_CONTAINER" psql -At -v ON_ERROR_STOP=1 -U "$PGUSER" -d "$PGDATABASE" -c "$1"
  fi
}

# 在指定库上执行一条只返回结果的 SQL（维护库查询用）。
pg_query_scalar_in() {
  local database="$1"
  local sql="$2"
  if [ "$PG_MODE" = "local" ]; then
    PGPASSWORD="$PGPASSWORD" psql -At -v ON_ERROR_STOP=1 -h "$PGHOST" -p "$PGPORT" \
      -U "$PGUSER" -d "$database" -c "$sql"
  else
    "$PG_ENGINE" exec -i "$PG_CONTAINER" psql -At -v ON_ERROR_STOP=1 -U "$PGUSER" -d "$database" -c "$sql"
  fi
}

# 导出数据库（纯 SQL 文本，输出到 stdout）。
pg_dump_stream() {
  if [ "$PG_MODE" = "local" ]; then
    PGPASSWORD="$PGPASSWORD" pg_dump -h "$PGHOST" -p "$PGPORT" -U "$PGUSER" -d "$PGDATABASE" \
      --no-owner --no-privileges
  else
    "$PG_ENGINE" exec -i "$PG_CONTAINER" pg_dump -U "$PGUSER" -d "$PGDATABASE" \
      --no-owner --no-privileges
  fi
}

# 数据库是否存在（在维护库上查，避免目标库不存在时报连接错误）
pg_database_exists() {
  local name="$1"
  local result
  result=$(pg_query_scalar_in "${PG_MAINTENANCE_DB:-postgres}" \
    "SELECT 1 FROM pg_database WHERE datname = '${name}'")
  [ "$result" = "1" ]
}

# 创建数据库（已存在则跳过）
pg_create_database() {
  local name="$1"
  if pg_database_exists "$name"; then
    return 0
  fi
  if [ "$PG_MODE" = "local" ]; then
    PGPASSWORD="$PGPASSWORD" createdb -h "$PGHOST" -p "$PGPORT" -U "$PGUSER" "$name"
  else
    "$PG_ENGINE" exec -i "$PG_CONTAINER" createdb -U "$PGUSER" "$name"
  fi
}

# 删除数据库（不存在则跳过）
pg_drop_database() {
  local name="$1"
  if ! pg_database_exists "$name"; then
    return 0
  fi
  if [ "$PG_MODE" = "local" ]; then
    PGPASSWORD="$PGPASSWORD" dropdb -h "$PGHOST" -p "$PGPORT" -U "$PGUSER" "$name"
  else
    "$PG_ENGINE" exec -i "$PG_CONTAINER" dropdb -U "$PGUSER" "$name"
  fi
}
