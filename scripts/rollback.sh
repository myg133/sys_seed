#!/usr/bin/env bash
# 迁移回滚：把数据库回滚到指定迁移版本。
#
# 用法：
#   scripts/rollback.sh <版本号>
#   scripts/rollback.sh <版本号> --dry-run
#
# 流程（设计摘要 §5.1）：
#   1. 先备份（回滚前留一份可回退的快照）
#   2. 执行 down 迁移，撤销版本号大于目标版本的迁移
#   3. 打印被撤销的版本
#
# 不可逆迁移（没有配套 down）会被拒绝回滚 —— 脚本会明确报错，不会做半截破坏。
#
# 环境变量：
#   DATABASE_URL  数据库连接串（必填）
#   MIGRATE_CMD   执行迁移的命令前缀，默认 `cargo run -p platform-server --bin base --`
#                 私有化部署示例：
#                 MIGRATE_CMD="docker compose -f deploy/compose.prod.yaml --profile migrate run --rm migrate" \
#                 scripts/rollback.sh 0
set -euo pipefail

cd "$(dirname "$0")/.."
# shellcheck source=scripts/lib/pg.sh
source scripts/lib/pg.sh

if [ $# -lt 1 ]; then
  echo "用法：$0 <版本号> [--dry-run]" >&2
  exit 2
fi

target_version="$1"
shift || true

dry_run=0
for arg in "$@"; do
  case "$arg" in
    --dry-run) dry_run=1 ;;
    *) echo "✗ 未知参数：$arg" >&2; exit 2 ;;
  esac
done

case "$target_version" in
  '' | *[!0-9]*) echo "✗ 版本号必须是非负整数：$target_version" >&2; exit 1 ;;
esac

pg_parse_url "${DATABASE_URL:-}"
pg_detect_mode psql

# 迁移命令前缀
if [ -n "${MIGRATE_CMD:-}" ]; then
  read -r -a migrate_cmd <<<"$MIGRATE_CMD"
elif command -v base >/dev/null 2>&1; then
  migrate_cmd=(base)
else
  migrate_cmd=(cargo run --quiet -p platform-server --bin base --)
fi

echo "▸ 当前迁移状态："
"${migrate_cmd[@]}" migrate status

if [ "$dry_run" -eq 1 ]; then
  echo "▸ --dry-run：不执行回滚。目标版本 ${target_version}。"
  exit 0
fi

# 回滚前先备份：回滚本身也可能出错，得留退路
echo "▸ 回滚前先备份…"
BACKUP_DIR="${BACKUP_DIR:-backup}" ./scripts/backup.sh

echo "▸ 回滚到版本 ${target_version}…"
"${migrate_cmd[@]}" migrate down --target "$target_version"

echo "✓ 回滚完成：目标版本 ${target_version}"
echo "  如需回退本次回滚，用 backup/ 下刚生成的备份恢复（见 deploy/RUNBOOK.md）"
