# syntax=docker/dockerfile:1.7
#
# 底座镜像：多阶段构建（builder / frontend / runtime）。
#
# 设计依据：BA/demands/REQ-005-delivery/design-summary.md §2
#
# - builder 用 `rust:lld`（自带 mold/lld 链接器）：官方 `rust` 镜像的 GNU ld
#   链接内存是本机问题的放大版，低配机与 CI runner 都吃紧。
# - runtime 用 `debian:12-slim`：只带一个二进制 + 静态文件 + 迁移脚本，目标 < 100MB。
# - 依赖层单独缓存：cargo-chef 先按 recipe 把依赖编好，源码改动不会重编依赖。
#
# ⚠️ 本机（低配）不构建本镜像：release + LTO 的链接内存峰值远超本机余量。
#    镜像构建交给 CI（见 .github/workflows/ci.yml 的 build-image job）。

ARG RUST_IMAGE=rust:lld
ARG NODE_IMAGE=node:22-bookworm-slim
ARG RUNTIME_IMAGE=debian:12-slim

# ---------- 1. builder：Rust 工具链 + 源码 → 二进制 ----------
FROM ${RUST_IMAGE} AS builder

# release 构建的并行度：CI runner 7G 内存，并行度换不来速度，只抬峰值内存
ARG CARGO_BUILD_JOBS=2
ARG CARGO_CHEF_VERSION=0.1.71

WORKDIR /app
ENV CARGO_TERM_COLOR=always \
    CARGO_INCREMENTAL=0

# cargo-chef：把「依赖编译」与「源码编译」拆成两层，依赖层单独缓存
RUN cargo install cargo-chef --version "${CARGO_CHEF_VERSION}" --locked

COPY . .

# recipe 只描述依赖图；cook 先把依赖编进缓存层
RUN cargo chef prepare --recipe-path recipe.json
RUN cargo chef cook --release --locked --jobs "${CARGO_BUILD_JOBS}" --recipe-path recipe.json

# 只编交付用的二进制：base（服务 + 迁移命令）
RUN cargo build --release --locked --jobs "${CARGO_BUILD_JOBS}" -p platform-server --bin base

# ---------- 2. frontend：Node/pnpm → 前端产物 ----------
FROM ${NODE_IMAGE} AS frontend

# npm 源：CI 在境外用官方源；国内自建可用 --build-arg NPM_REGISTRY=... 覆盖
ARG NPM_REGISTRY=https://registry.npmjs.org/
ARG PNPM_VERSION=12.19.0

WORKDIR /app
ENV CI=true

RUN corepack enable && corepack prepare "pnpm@${PNPM_VERSION}" --activate

# 先只拷清单：依赖层可缓存
COPY package.json pnpm-lock.yaml pnpm-workspace.yaml .npmrc ./
COPY apps/ apps/
COPY packages/ packages/

RUN pnpm install --frozen-lockfile --registry="${NPM_REGISTRY}"
RUN pnpm -r build

# ---------- 3. runtime：最小运行镜像 ----------
FROM ${RUNTIME_IMAGE} AS runtime

# 只装 CA 证书（未来接 TLS / 出网调用时需要）；时区固定 UTC（契约 C13）
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# 非 root 运行
RUN useradd --system --uid 10001 --user-group --create-home --home-dir /home/base base

WORKDIR /app

COPY --from=builder /app/target/release/base /usr/local/bin/base
COPY --from=builder /app/migrations/ /app/migrations/
COPY --from=frontend /app/apps/shell/dist/ /app/static/

ENV BASE_MIGRATIONS_DIR=/app/migrations \
    BASE_STATIC_DIR=/app/static \
    HOST=0.0.0.0 \
    SERVER_PORT=8080 \
    RUST_LOG=info

USER base
EXPOSE 8080

# 镜像自带的健康检查：就绪探针（数据库连通 + 迁移已应用）
#
# --timeout 必须**大于**连接池的 acquire_timeout（当前 2s，见
# crates/platform-server/src/db.rs 的 ACQUIRE_TIMEOUT）：数据库不可达时
# /health/ready 要等满 acquire_timeout 才返回 503，超时若等于或小于它，
# 容器会被判定为 unhealthy 而不是拿到那个语义清晰的 503。
HEALTHCHECK --interval=15s --timeout=5s --start-period=10s --retries=3 \
    CMD ["/usr/local/bin/base", "healthcheck"]

ENTRYPOINT ["/usr/local/bin/base"]
CMD ["serve"]
