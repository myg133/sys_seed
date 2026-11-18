#!/usr/bin/env bash
# 契约快照同步 / 校验。
#
# # 为什么要它
#
# 契约正本只有一份：在 BA 分支（`demand`）的 `contracts/<name>/<version>.md`。
# 代码仓库里另外放一份快照 `docs/contracts/<name>/<version>.md`，
# 是为了让「只拿到代码仓库的人」也能读到契约正文（AGENTS.md 的参考段指向它）。
#
# 快照由本脚本从正本生成、并由本脚本校验 —— 不存在两份各自演化的正本：
#   * 快照**永远**可以由本脚本从正本重新生成；
#   * 快照与正本不一致时，CI 会失败并指出同步命令。
#
# 用法：
#   scripts/sync-contracts.sh            # 校验（CI 用）：不一致则退出码 1
#   scripts/sync-contracts.sh --sync     # 同步：用 BA 正本覆盖仓库内快照
set -euo pipefail

cd "$(dirname "$0")/.."

MODE="check"
if [ "${1:-}" = "--sync" ]; then
  MODE="sync"
elif [ $# -gt 0 ]; then
  echo "用法：$0 [--sync]" >&2
  exit 2
fi

# --- 1. 定位 BA 分支引用（本地分支 → 远端跟踪分支） -------------------------
BA_REF=""
for ref in demand origin/demand refs/remotes/origin/demand; do
  if git rev-parse --verify --quiet "$ref" >/dev/null; then
    BA_REF="$ref"
    break
  fi
done

if [ -z "$BA_REF" ]; then
  echo "跳过：本地没有 BA 分支（demand / origin/demand），无法校验契约快照。"
  echo "      拉取后重试：git fetch origin demand"
  exit 0
fi

echo "契约正本分支：$BA_REF"

# --- 2. 契约清单来自 feature-meta/manifest.json 的 pin ------------------------
PINS=$(jq -r '.contracts | to_entries[] | "\(.key) \(.value)"' feature-meta/manifest.json)

status=0
while read -r name version; do
  [ -z "$name" ] && continue
  src="contracts/$name/$version.md"
  dst="docs/contracts/$name/$version.md"

  if ! git cat-file -e "$BA_REF:$src" 2>/dev/null; then
    echo "✗ $src 在 $BA_REF 上不存在（契约被删除或改名？）"
    status=1
    continue
  fi

  if [ "$MODE" = "sync" ]; then
    mkdir -p "$(dirname "$dst")"
    git show "$BA_REF:$src" >"$dst"
    echo "✓ 已同步 $dst"
  elif [ ! -f "$dst" ]; then
    echo "✗ 缺失快照：$dst"
    echo "    修复方式：运行 scripts/sync-contracts.sh --sync"
    status=1
  elif git show "$BA_REF:$src" | diff -u - "$dst" >/dev/null; then
    echo "✓ 快照一致：$dst"
  else
    echo "✗ 快照与 BA 正本不一致：$dst"
    echo "    修复方式：运行 scripts/sync-contracts.sh --sync"
    status=1
  fi
done <<<"$PINS"

exit "$status"
