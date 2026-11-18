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
#   scripts/sync-contracts.sh --fetch    # 先 git fetch origin demand 再校验（CI 用）
#
# ⚠️ 拿不到 BA 正本时**必须失败**（非 0 退出），不允许静默跳过：
#    静默跳过会让 CI 门禁空转 —— 快照与正本漂移了也没人知道。
set -euo pipefail

cd "$(dirname "$0")/.."

MODE="check"
FETCH=0
for arg in "$@"; do
  case "$arg" in
    --sync) MODE="sync" ;;
    --fetch) FETCH=1 ;;
    *)
      echo "用法：$0 [--sync] [--fetch]" >&2
      exit 2
      ;;
  esac
done

# --- 1. 定位 BA 分支引用（本地分支 → 远端跟踪分支） -------------------------
BA_REF=""
for ref in demand origin/demand refs/remotes/origin/demand; do
  if git rev-parse --verify --quiet "$ref" >/dev/null; then
    BA_REF="$ref"
    break
  fi
done

# 明确要求时先拉取（CI 里 origin 一定存在）
if [ -z "$BA_REF" ] && [ "$FETCH" -eq 1 ]; then
  echo "拉取 BA 分支 demand…"
  git fetch --quiet origin demand || true
  for ref in origin/demand refs/remotes/origin/demand; do
    if git rev-parse --verify --quiet "$ref" >/dev/null; then
      BA_REF="$ref"
      break
    fi
  done
fi

if [ -z "$BA_REF" ]; then
  echo "✗ 找不到 BA 正本分支（demand / origin/demand），无法校验契约快照。" >&2
  echo "  原因：没有正本可比对，快照可能已经与正本漂移。" >&2
  echo "  修复：git fetch origin demand（远程仓库可用时），或用 --fetch 参数重跑本脚本。" >&2
  echo "  本脚本不会静默跳过：门禁必须以失败暴露问题。" >&2
  exit 2
fi

echo "契约正本分支：$BA_REF"

# --- 2. 契约清单来自 feature-meta/manifest.json 的 pin ------------------------
if ! jq -e '.contracts | type == "object" and length > 0' feature-meta/manifest.json >/dev/null 2>&1; then
  echo "✗ feature-meta/manifest.json 缺少非空的 contracts pin。" >&2
  echo "  契约要求每个需求 pin 消费的契约版本，例如：" >&2
  echo '    "contracts": { "api-conventions": "v1", "extension-points": "v1" }' >&2
  echo "  没有 pin 就没有比对清单，校验会空转 —— 因此这里直接失败。" >&2
  exit 2
fi

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
