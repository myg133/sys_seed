#!/usr/bin/env bash
# 私有化部署前置校验：口令等敏感项必须显式注入，弱口令直接拒绝。
#
# 为什么要有这个脚本：
#   deploy/compose.prod.yaml 里 POSTGRES_PASSWORD **没有默认值**
#   （`${POSTGRES_PASSWORD:?...}`），compose 解析阶段就会因为「缺失」报错退出；
#   但 compose 只管「有没有」，管不了「是不是弱口令」——
#   有人把 `.env.example` 里的示例口令原样复制过来，compose 是不会拦的。
#   这个脚本补上那一层，并把错误说清楚。
#
# 用法（在放 .env 的目录里跑）：
#   cd deploy
#   ./scripts/prod-preflight.sh            # 读 ./.env（也可用 ENV_FILE 指定别的文件）
#   ENV_FILE=/etc/base/prod.env ./scripts/prod-preflight.sh
#
# 环境变量：
#   ENV_FILE         要校验的 env 文件（默认 .env；不存在则只看当前环境变量）
#   POSTGRES_PASSWORD 生产库口令（必须显式注入，无默认值）
#   POSTGRES_DB      生产库名（默认 base）
#   DATABASE_URL     应用连接串（必须显式注入）
#
# 退出码：0 = 通过；1 = 校验不通过（逐条打印原因）。
set -euo pipefail

ENV_FILE="${ENV_FILE:-.env}"

# 弱口令名单：常见默认值 / 示例口令一律拒绝。
WEAK_PASSWORDS=(
  base postgres password passwd changeme change_me admin root secret
  123456 12345678 123456789 qwerty letmein
)

fail() {
  echo "✗ $*" >&2
}

# ---------------------------------------------------------------------------
# 载入 env 文件（.env 的优先级低于真实环境变量，与 compose 一致）
# ---------------------------------------------------------------------------
load_env_file() {
  local file="$1"
  [ -f "$file" ] || return 0

  # 先记下真实环境里已有的变量，载入后原样恢复（真实环境优先）
  declare -A ORIG_ENV=()
  local name value
  while IFS='=' read -r name value; do
    case "$name" in
      [A-Za-z_]*) ORIG_ENV["$name"]="$value" ;;
    esac
  done < <(env)

  set -a
  # shellcheck disable=SC1090
  if ! . "$file"; then
    set +a
    fail "env 文件解析失败：$file"
    exit 1
  fi
  set +a

  for name in "${!ORIG_ENV[@]}"; do
    printf -v "$name" '%s' "${ORIG_ENV[$name]}"
    export "$name"
  done
}

# 从连接串里取库名（postgres://user:pass@host:5432/dbname?sslmode=require → dbname）
url_db_name() {
  local rest="$1"
  rest="${rest#*://}"
  rest="${rest#*@}"
  rest="${rest%%\?*}"
  rest="${rest#*/}"
  printf '%s' "$rest"
}

# 从连接串里取口令（没有 user:pass@ 形式则返回空）
url_password() {
  local url="$1" creds
  case "$url" in
    *@*) ;;
    *) printf ''; return 0 ;;
  esac
  creds="${url#*://}"
  creds="${creds%%@*}"
  case "$creds" in
    *:*) printf '%s' "${creds#*:}" ;;
    *) printf '' ;;
  esac
}

is_weak_password() {
  local candidate="$1" weak
  candidate="$(printf '%s' "$candidate" | tr '[:upper:]' '[:lower:]')"
  for weak in "${WEAK_PASSWORDS[@]}"; do
    [ "$candidate" = "$weak" ] && return 0
  done
  return 1
}

load_env_file "$ENV_FILE"

problems=0

# ---------------------------------------------------------------------------
# 1. 生产口令：必须显式注入
# ---------------------------------------------------------------------------
password="${POSTGRES_PASSWORD:-}"
if [ -z "$password" ]; then
  fail "缺少 POSTGRES_PASSWORD：生产口令必须由 .env / Docker secret 显式注入（compose 里没有默认值）。"
  fail "  修复：在 .env 里设置 POSTGRES_PASSWORD（生成：openssl rand -base64 24）"
  problems=$((problems + 1))
elif is_weak_password "$password"; then
  fail "POSTGRES_PASSWORD 是弱口令：生产库不接受默认/示例口令。"
  fail "  修复：换成至少 12 位的随机口令（openssl rand -base64 24）"
  problems=$((problems + 1))
elif [ "${#password}" -lt 12 ]; then
  fail "POSTGRES_PASSWORD 太短（${#password} 位，至少 12 位）。"
  fail "  修复：openssl rand -base64 24"
  problems=$((problems + 1))
fi

# ---------------------------------------------------------------------------
# 2. 应用连接串：必须显式注入
# ---------------------------------------------------------------------------
database_url="${DATABASE_URL:-}"
if [ -z "$database_url" ]; then
  fail "缺少 DATABASE_URL：应用连接串必须显式注入（compose 的 app 服务靠它连库）。"
  fail "  修复：在 .env 里设置 DATABASE_URL=postgres://base:<口令>@postgres:5432/<库名>"
  problems=$((problems + 1))
else
  url_pass="$(url_password "$database_url")"
  if [ -n "$url_pass" ] && is_weak_password "$url_pass"; then
    fail "DATABASE_URL 里的口令是弱口令（生产库不接受默认/示例口令）。"
    fail "  修复：口令改成随机值，并与 POSTGRES_PASSWORD 保持一致"
    problems=$((problems + 1))
  fi
fi

# ---------------------------------------------------------------------------
# 3. 库名一致：compose 建库用的 POSTGRES_DB 必须等于连接串里的库名
# ---------------------------------------------------------------------------
postgres_db="${POSTGRES_DB:-base}"
if [ -n "$database_url" ]; then
  url_db="$(url_db_name "$database_url")"
  if [ -n "$url_db" ] && [ "$url_db" != "$postgres_db" ]; then
    fail "库名不一致：DATABASE_URL 指向 '${url_db}'，但 POSTGRES_DB 是 '${postgres_db}' —— 应用会连到不存在的库。"
    fail "  修复：把 .env 里的 DATABASE_URL 库名改成 '${postgres_db}'，或显式设置 POSTGRES_DB"
    problems=$((problems + 1))
  fi
fi

if [ "$problems" -ne 0 ]; then
  echo "✗ 生产部署前置校验不通过（${problems} 项）—— 不要继续 up -d" >&2
  exit 1
fi

echo "✓ 生产部署前置校验通过：口令已显式注入且非弱口令，库名 '${postgres_db}' 与 DATABASE_URL 一致"
