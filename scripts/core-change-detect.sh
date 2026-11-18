#!/usr/bin/env bash
# 列出本次 PR 改动的平台核心路径（空输出 = 没改核心）。
#
# 判定口径（REQ-005 修复 QA 登记的偏差）：
#   base 用 **merge-base**(base 分支最新提交, PR head)，
#   不用 `pull_request.base.sha` ——
#   base.sha 是 PR 打开那一刻的 base 提交，**不会随 base 分支前进**，
#   用它做 diff 会把「base 上已经合入的核心改动」也算成本次 PR 的改动（偏严，
#   不会漏放，但会让正常的 PR 反复触发核心评审）。
#
# 核心路径清单覆盖（REQ-005 确认）：
#   crates/platform-core/            平台核心
#   crates/platform-contracts/       扩展点契约套件
#   crates/platform-contracts-macros/ 契约测试宏（REQ-003 新增，同样算核心）
#
# 判定标准：**改动是否影响所有业务模块**（CODEOWNERS 的口径）。
#   平台服务与工程工具不满足这条 —— 业务模块不依赖它们，
#   因此列为显式豁免，而不是悄悄漏掉。
#
# 用法：
#   BASE_REF=origin/develop HEAD_SHA=<sha> scripts/core-change-detect.sh
#   scripts/core-change-detect.sh --self-test
set -euo pipefail

cd "$(dirname "$0")/.."

# 自检用的临时目录（trap 在退出时清理）
TEMP_DIR=""

# 平台核心路径清单
CORE_PATHS=(
  crates/platform-core
  crates/platform-contracts
  crates/platform-contracts-macros
)

# 不属于核心的平台 crate（显式豁免，附理由）
NON_CORE_CRATES=(
  # 交付基线服务：业务模块不依赖它（业务数据访问层待 REQ-006/007）
  crates/platform-server
  # 工程任务运行器：CI 规则工具，不影响业务模块
  crates/xtask
)

# 自检：crates/ 下的每个平台 crate 都必须在核心清单里，
# 否则「新增了平台 crate 却忘了加进评审门禁」会静默漏放。
check_core_coverage() {
  local missing=0
  local dir name
  for dir in crates/*/; do
    name="${dir%/}"
    local covered=0
    local core
    for core in "${CORE_PATHS[@]}"; do
      [ "$core" = "$name" ] && covered=1 && break
    done
    [ "$covered" -eq 1 ] && continue
    local exempt=0
    for core in "${NON_CORE_CRATES[@]:-}"; do
      [ "$core" = "$name" ] && exempt=1 && break
    done
    [ "$exempt" -eq 1 ] && continue
    echo "✗ 平台 crate ${name} 不在核心路径清单里：改动它不会触发核心评审。" >&2
    echo "  修复：把它加进本脚本的 CORE_PATHS，或在 NON_CORE_CRATES 里显式豁免。" >&2
    missing=1
  done
  return "$missing"
}

# 自检：用临时仓库验证判定口径（merge-base + 路径覆盖）
self_test() {
  local temp
  TEMP_DIR=$(mktemp -d)
  temp="$TEMP_DIR"
  trap 'rm -rf "$TEMP_DIR"' EXIT

  git -C "$temp" init -q -b base
  git -C "$temp" config user.email t@t.local
  git -C "$temp" config user.name t
  mkdir -p "$temp/crates/platform-core" "$temp/crates/other"
  echo a >"$temp/crates/platform-core/a.txt"
  echo a >"$temp/crates/other/b.txt"
  git -C "$temp" add -A
  git -C "$temp" commit -qm init

  # base 分支上合入一个核心改动
  echo b >>"$temp/crates/platform-core/a.txt"
  git -C "$temp" commit -qam "core change on base"
  base_tip=$(git -C "$temp" rev-parse HEAD)

  # PR 分支：只改非核心路径
  git -C "$temp" checkout -qb feature
  echo b >"$temp/crates/other/b.txt"
  git -C "$temp" commit -qam "non core change"

  local out
  out=$(git -C "$temp" diff --name-only "$(git -C "$temp" merge-base "$base_tip" HEAD)" HEAD -- \
    crates/platform-core crates/platform-contracts crates/platform-contracts-macros)
  if [ -n "$out" ]; then
    echo "✗ 自检失败：base 上的核心改动被算成了 PR 的改动（输出：$out）" >&2
    return 1
  fi

  # PR 分支上改核心路径 → 必须被检出
  echo c >>"$temp/crates/platform-core/a.txt"
  git -C "$temp" commit -qam "core change on feature"
  out=$(git -C "$temp" diff --name-only "$(git -C "$temp" merge-base "$base_tip" HEAD)" HEAD -- \
    crates/platform-core crates/platform-contracts crates/platform-contracts-macros)
  if [ "$out" != "crates/platform-core/a.txt" ]; then
    echo "✗ 自检失败：PR 上的核心改动没被检出（输出：$out）" >&2
    return 1
  fi

  echo "✓ 自检通过：merge-base 口径不误算 base 侧改动，PR 侧核心改动能检出"
}

if [ "${1:-}" = "--self-test" ]; then
  self_test
  exit $?
fi

base_ref="${BASE_REF:-}"
head_sha="${HEAD_SHA:-HEAD}"

if [ -z "$base_ref" ]; then
  echo "用法：BASE_REF=origin/<base> HEAD_SHA=<sha> $0" >&2
  exit 2
fi

if ! git rev-parse --verify --quiet "$base_ref" >/dev/null; then
  echo "✗ 找不到 base 引用：${base_ref}（先 git fetch origin <base>）" >&2
  exit 2
fi

check_core_coverage

merge_base="$(git merge-base "$base_ref" "$head_sha")"
git diff --name-only "$merge_base" "$head_sha" -- "${CORE_PATHS[@]}"
