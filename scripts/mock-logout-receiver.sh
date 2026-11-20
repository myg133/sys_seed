#!/usr/bin/env bash
# back-channel 登出的 **mock 接收端**（REQ-022，D237）。
#
# # 为什么必须是**独立进程**（D237 §「为什么必须独立进程」）
#
# AC-S-149 的全部价值在于「出站 HTTP **真的发出去了**」——
# 在 E2E 进程里 mock HTTP（nock / MSW / `page.route`）等于**把被测逻辑本身
# mock 掉**（dev-guide §9.13）→ 判据恒真。
#
# → 本脚本是一个**真的 HTTP 监听器**：平台进程真的往 `127.0.0.1:$MOCK_PORT`
# 发一次 POST → 本进程真的收到 → 真的把 `logout_token` 落盘。
#
# # 落盘文件
#
# `target/mock-logout-received.jsonl` —— **每行一条 JSON**：
#
# ```json
# {"at":"2026-01-01T00:00:00.000Z","method":"POST","url":"/logout","logout_token":"<JWT>"}
# ```
#
# E2E 读**最后一行**（AC-S-149 步骤 2 的判据来源）。
#
# # `MOCK_MODE`
#
# | 值 | 行为 | 用途 |
# |---|---|---|
# | `ok`（默认） | 200 + `{}` | 正常投递 |
# | `timeout` | **永不响应** | 出站 5 秒超时 → 登出**不得**因此失败（D211） |
# | `500` | 500 | 接收端报错 → 登出**不得**因此失败（D211） |
#
# # 夹具顺序（**钉死**，D237 §2）
#
# **先起 mock → 再播种**：`platform_oauth_client.backchannel_logout_uri` 里
# 含 `$MOCK_PORT` → 播种时端口必须已经定下来。
# `scripts/seed-e2e.sh` 只**读** `$MOCK_PORT`（不负责起 mock）。
#
# # 用法
#
# ```bash
# MOCK_PORT=19999 ./scripts/mock-logout-receiver.sh          # 前台
# MOCK_PORT=19999 MOCK_MODE=500 ./scripts/mock-logout-receiver.sh &
# ```
set -euo pipefail

cd "$(dirname "$0")/.."

MOCK_PORT="${MOCK_PORT:-19999}"
MOCK_MODE="${MOCK_MODE:-ok}"
MOCK_LOG_FILE="${MOCK_LOG_FILE:-target/mock-logout-received.jsonl}"
MOCK_PID_FILE="${MOCK_PID_FILE:-target/mock-logout-receiver.pid}"

case "$MOCK_MODE" in
  ok | timeout | 500) ;;
  *)
    echo "✗ MOCK_MODE 只能是 ok | timeout | 500（当前 '$MOCK_MODE'）。" >&2
    exit 2
    ;;
esac

if ! command -v node >/dev/null 2>&1; then
  echo "✗ 找不到 node —— 本脚本用 node 起一个最小 HTTP 监听器。" >&2
  exit 1
fi

mkdir -p "$(dirname "$MOCK_LOG_FILE")"
# 每次启动**清空**落盘文件：E2E 读的是「最后一行」→ 上一轮的残留会误判。
: > "$MOCK_LOG_FILE"

# 用 node 起监听器（仓库的 E2E 栈本来就是 node → 不引入新依赖）。
node - "$MOCK_PORT" "$MOCK_MODE" "$MOCK_LOG_FILE" "$MOCK_PID_FILE" <<'NODE'
const http = require('node:http');
const fs = require('node:fs');

const port = Number(process.argv[2]);
const mode = process.argv[3];
const logFile = process.argv[4];
const pidFile = process.argv[5];

const server = http.createServer((req, res) => {
  let body = '';
  req.on('data', (chunk) => {
    body += chunk;
  });
  req.on('end', () => {
    let token = '';
    try {
      token = new URLSearchParams(body).get('logout_token') ?? '';
    } catch {
      token = '';
    }
    // ⚠️ **只记 `/logout`**：就绪探针会打 `/` → 记进去的话，E2E 读「最后一行」
    // 读到的是探针而不是真的登出通知 → 判据失真。
    if (req.url && req.url.endsWith('/logout')) {
      // **先落盘再响应**：E2E 可能在响应到达前就去读文件。
      fs.appendFileSync(
        logFile,
        `${JSON.stringify({
          at: new Date().toISOString(),
          method: req.method,
          url: req.url,
          logout_token: token,
        })}\n`,
      );
    }
    if (mode === 'timeout') {
      // 永不响应 → 平台的出站超时（5 秒）兜底。
      return;
    }
    if (mode === '500') {
      res.writeHead(500, { 'content-type': 'text/plain' });
      res.end('mock logout receiver failure');
      return;
    }
    res.writeHead(200, { 'content-type': 'application/json' });
    res.end('{}');
  });
});

server.on('error', (error) => {
  console.error(`mock logout receiver 启动失败：${error.message}`);
  process.exit(1);
});

server.listen(port, '127.0.0.1', () => {
  fs.writeFileSync(pidFile, `${process.pid}\n`, { mode: 0o600 });
  console.log(`mock logout receiver: http://127.0.0.1:${port}/logout（mode=${mode}）`);
});

const cleanup = () => {
  try {
    fs.rmSync(pidFile, { force: true });
  } catch {
    // 清理失败不影响退出码。
  }
  process.exit(0);
};
process.on('SIGTERM', cleanup);
process.on('SIGINT', cleanup);
NODE
