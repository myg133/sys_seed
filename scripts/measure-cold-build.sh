#!/usr/bin/env bash
# 冷编译实测：耗时 + 峰值内存（VmHWM 采样求和）。
#
# 口径与 REQ-006 的报告一致：本机**没有** /usr/bin/time（time 是 bash 内建）
# → 用 VmHWM 采样（cargo + 其 rustc 子进程的高水位求和，偏保守的上界）。
set -euo pipefail

cd "$(dirname "$0")/.."

sample_peak() {
  python3 - "$1" <<'PY'
import os, subprocess, sys, time

pid = int(sys.argv[1])
peak = 0
while True:
    try:
        # 找 cargo 及其全部后代
        out = subprocess.run(["ps", "-eo", "pid,ppid"], capture_output=True, text=True).stdout
        children = {}
        for line in out.splitlines()[1:]:
            parts = line.split()
            if len(parts) == 2:
                children.setdefault(int(parts[1]), []).append(int(parts[0]))
        seen = set()
        stack = [pid]
        while stack:
            cur = stack.pop()
            if cur in seen:
                continue
            seen.add(cur)
            stack.extend(children.get(cur, []))
        total = 0
        for p in seen:
            try:
                with open(f"/proc/{p}/status") as fh:
                    for line in fh:
                        if line.startswith("VmHWM:"):
                            total += int(line.split()[1])
                            break
            except OSError:
                pass
        peak = max(peak, total)
    except Exception:
        pass
    try:
        os.kill(pid, 0)
    except OSError:
        break
    time.sleep(0.2)
print(peak)
PY
}

run_measured() {
  local label="$1"
  shift
  local start end
  start=$(date +%s.%N)
  "$@" &
  local pid=$!
  local peak
  peak=$(sample_peak "$pid")
  wait "$pid"
  end=$(date +%s.%N)
  printf '%s\t耗时 %.1fs\t峰值 %d MB\n' "$label" "$(echo "$end - $start" | bc)" "$((peak / 1024))"
}

echo "=== 冷编译实测（$(date '+%F %T')）==="
cargo clean
run_measured "cargo check --workspace --all-targets" cargo check --workspace --all-targets
run_measured "cargo test --workspace --no-run" cargo test --workspace --no-run
