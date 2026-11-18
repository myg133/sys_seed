#!/usr/bin/env bash
# 从备份文件恢复数据库。
#
# 用法：
#   scripts/restore.sh <备份文件> [目标数据库] [--yes]
#
#   - 指定目标数据库：恢复式演练（目标库会被删掉重建），验完请自行清理。
#   - 不指定目标数据库：恢复到 DATABASE_URL 指向的库 —— **会删库重建**，
#     必须显式加 --yes 确认（防止误操作把生产库清空）。
#
# 环境变量：
#   DATABASE_URL  数据库连接串（必填；缺省时用 MIGRATION_DATABASE_URL）
#                 备份 / 恢复是**管理操作**，要用超级用户连接（普通角色没有 DDL 权限）
#   PG_CONTAINER  数据库容器名（默认 base-postgres）
set -euo pipefail

cd "$(dirname "$0")/.."
# shellcheck source=scripts/lib/pg.sh
source scripts/lib/pg.sh

confirm=0
positional=()
for arg in "$@"; do
  case "$arg" in
    --yes | -y) confirm=1 ;;
    -*) echo "✗ 未知参数：$arg" >&2; exit 2 ;;
    *) positional+=("$arg") ;;
  esac
done

if [ ${#positional[@]} -lt 1 ]; then
  echo "用法：$0 <备份文件> [目标数据库] [--yes]" >&2
  exit 2
fi

backup_file="${positional[0]}"
target_db="${positional[1]:-}"

if [ ! -f "$backup_file" ]; then
  echo "✗ 备份文件不存在：$backup_file" >&2
  exit 1
fi
if ! gzip -t "$backup_file"; then
  echo "✗ 备份文件不是有效的 gzip：$backup_file" >&2
  exit 1
fi

pg_parse_url "${DATABASE_URL:-${MIGRATION_DATABASE_URL:-}}"
pg_detect_mode psql

if [ -n "$target_db" ]; then
  PGDATABASE="$target_db"
else
  if [ "$confirm" -ne 1 ]; then
    echo "✗ 该操作会**删除并重建**数据库 ${PGDATABASE}，请加 --yes 确认" >&2
    echo "  演练建议指定临时库：$0 ${backup_file} base_restore_drill" >&2
    exit 1
  fi
fi

echo "▸ 目标数据库：${PGDATABASE}（删除后重建 + 导入）"

# 覆盖式恢复：先删后建，避免旧对象与备份内容冲突
pg_drop_database "$PGDATABASE"
pg_create_database "$PGDATABASE"

echo "▸ 导入 ${backup_file}…"
gzip -dc "$backup_file" | pg_run_sql

echo "✓ 恢复完成：${PGDATABASE} ← ${backup_file}"
