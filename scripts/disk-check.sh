#!/usr/bin/env bash
# 磁盘水位检查：文件系统水位 + Rust 产物 + 容器存储。
set -euo pipefail

cd "$(dirname "$0")/.."

echo "== 文件系统 =="
df -h /home | tail -1

echo
echo "== Rust 产物 =="
if [ -d target ]; then
  du -sh target
else
  echo "target/ 尚未生成"
fi

echo
echo "== 容器存储 =="
if command -v podman >/dev/null; then
  export XDG_RUNTIME_DIR="/run/user/$(id -u)"
  podman system df || echo "podman system df 执行失败"
else
  echo "未安装 podman，跳过"
fi

echo
echo "== node_modules =="
if [ -d node_modules ]; then
  du -sh node_modules
else
  echo "node_modules/ 尚未生成"
fi
