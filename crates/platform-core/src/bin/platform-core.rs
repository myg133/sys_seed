//! 平台核心最小服务。
//!
//! 只提供一个健康检查端点，用于验证「后端能跑起来」。
//! 业务端点在后续 Sprint 补齐；分组前缀遵循 `api-conventions v1` C1。

use axum::{Json, Router, routing::get};
use platform_core::{AppConfig, RequestContext};
use serde_json::json;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    init_tracing();

    let config = AppConfig::from_env();
    let addr = config.socket_addr();
    let app = Router::new().route("/api/health", get(health));

    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!(%addr, "platform-core 已启动");
    axum::serve(listener, app).await?;
    Ok(())
}

async fn health() -> Json<serde_json::Value> {
    let ctx = RequestContext::new();
    Json(json!({
        "status": "ok",
        "version": platform_core::VERSION,
        "traceId": ctx.trace_id(),
    }))
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt().with_env_filter(filter).init();
}
