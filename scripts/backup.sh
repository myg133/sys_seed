#!/usr/bin/env bash
# 数据库备份：pg_dump + gzip → backup/base-<时间戳>.sql.gz
#
# 用法：
#   DATABASE_URL=postgres://base:base@localhost:5432/base_dev scripts/backup.sh
#
# 环境变量：
#   BACKUP_DIR   备份目录（默认 backup）
#   BACKUP_KEEP  保留最近几份（默认 5）
#   PG_CONTAINER 数据库容器名（默认 base-postgres）
#
# 磁盘控制：目录内只保留最近 BACKUP_KEEP 份，更旧的自动删除。
set -euo pipefail

cd "$(dirname "$0")/.."
# shellcheck source=scripts/lib/pg.sh
source scripts/lib/pg.sh

BACKUP_DIR="${BACKUP_DIR:-backup}"
BACKUP_KEEP="${BACKUP_KEEP:-5}"

case "$BACKUP_KEEP" in
  '' | *[!0-9]*) echo "✗ BACKUP_KEEP 必须是正整数：$BACKUP_KEEP" >&2; exit 1 ;;
esac

pg_parse_url "${DATABASE_URL:-${MIGRATION_DATABASE_URL:-}}"
pg_detect_mode pg_dump

mkdir -p "$BACKUP_DIR"

timestamp=$(date +%Y%m%d-%H%M%S)
target="$BACKUP_DIR/base-${timestamp}.sql.gz"
tmp="${target}.tmp"

echo "▸ 备份数据库 ${PGDATABASE}（模式：${PG_MODE}）→ ${target}"
pg_dump_stream | gzip -9 >"$tmp"

# 落盘前自检：文件非空且 gzip 完整
if [ ! -s "$tmp" ]; then
  rm -f "$tmp"
  echo "✗ 备份为空，判定失败" >&2
  exit 1
fi
if ! gzip -t "$tmp"; then
  rm -f "$tmp"
  echo "✗ 备份文件 gzip 校验失败" >&2
  exit 1
fi

mv "$tmp" "$target"

# 轮转：只保留最近 BACKUP_KEEP 份（文件名按时间戳倒序）
mapfile -t backups < <(find "$BACKUP_DIR" -maxdepth 1 -type f -name 'base-*.sql.gz' | sort -r)
removed=0
for ((index = BACKUP_KEEP; index < ${#backups[@]}; index++)); do
  rm -f "${backups[index]}"
  removed=$((removed + 1))
done

size=$(du -h "$target" | cut -f1)
echo "✓ 备份完成：${target}（${size}）"
echo "  当前保留 ${#backups[@]} 份${removed:+，已清理 ${removed} 份}，保留上限 ${BACKUP_KEEP} 份"
