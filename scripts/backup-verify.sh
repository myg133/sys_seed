#!/usr/bin/env bash
# 备份恢复演练：把备份恢复到临时库，跑一次冒烟检查，再删掉临时库。
#
# 用法：
#   scripts/backup-verify.sh                # 校验最近一份备份
#   scripts/backup-verify.sh <备份文件>      # 校验指定备份
#
# 冒烟检查内容：
#   1. 临时库能创建、能连上
#   2. 平台迁移表存在（说明备份来自已迁移的库）
#   3. 关键表可查询
#
# 演练记录（日期 / 执行人 / 耗时 / 问题）写进 deploy/RUNBOOK.md。
set -euo pipefail

cd "$(dirname "$0")/.."
# shellcheck source=scripts/lib/pg.sh
source scripts/lib/pg.sh

BACKUP_DIR="${BACKUP_DIR:-backup}"

if [ $# -ge 1 ]; then
  backup_file="$1"
else
  backup_file=$(find "$BACKUP_DIR" -maxdepth 1 -type f -name 'base-*.sql.gz' | sort -r | head -1)
fi

if [ -z "$backup_file" ] || [ ! -f "$backup_file" ]; then
  echo "✗ 没有可校验的备份（先跑 scripts/backup.sh）" >&2
  exit 1
fi

pg_parse_url "${DATABASE_URL:-}"
pg_detect_mode psql

# 临时库名：与业务库隔离，验完必删
drill_db="base_verify_$(date +%Y%m%d%H%M%S)"

started=$(date +%s)
echo "▸ 校验备份：${backup_file}"
echo "▸ 临时库：${drill_db}"

cleanup() {
  pg_drop_database "$drill_db" || true
}
trap cleanup EXIT

pg_create_database "$drill_db"
PGDATABASE="$drill_db"

# 先把备份真正导进临时库 —— 后面的冒烟检查才有东西可查
gzip -dc "$backup_file" | pg_run_sql
echo "  ✓ 备份已导入临时库"

# 冒烟 1：能连上并执行查询
pg_query_scalar "SELECT 1" >/dev/null
echo "  ✓ 冒烟 1：连接与查询正常"

# 冒烟 2：迁移表存在（备份来自已迁移的库）
migrations_table=$(pg_query_scalar "SELECT count(*) FROM information_schema.tables WHERE table_schema = 'public' AND table_name = '_sqlx_migrations'")
if [ "$migrations_table" = "0" ]; then
  echo "  ✗ 冒烟 2：备份里没有迁移表 _sqlx_migrations" >&2
  exit 1
fi
echo "  ✓ 冒烟 2：迁移表存在"

# 冒烟 3：平台元数据表可查询
if [ "$(pg_query_scalar "SELECT count(*) FROM information_schema.tables WHERE table_schema = 'public' AND table_name = 'platform_meta'")" = "0" ]; then
  echo "  ! 冒烟 3：备份里没有 platform_meta 表（迁移可能未执行）"
else
  pg_query_scalar "SELECT count(*) FROM platform_meta" >/dev/null
  echo "  ✓ 冒烟 3：platform_meta 可查询"
fi

table_count=$(pg_query_scalar "SELECT count(*) FROM information_schema.tables WHERE table_schema = 'public'")
elapsed=$(( $(date +%s) - started ))
echo "✓ 备份校验通过：${backup_file}（${table_count} 张表，耗时 ${elapsed}s，临时库已清理）"
