//! 交付基线服务（REQ-005）。
//!
//! 三件事：
//!
//! 1. **配置外置**：同一镜像靠环境变量切分 dev / staging / prod，启动时校验、缺项 fail-fast。
//! 2. **健康检查**：`/health/live`（进程活着）与 `/health/ready`（数据库连通 + 迁移已应用）。
//! 3. **迁移独立化**：迁移是独立命令，不在服务启动流程里跑。

pub mod config;
pub mod db;
pub mod health;
pub mod migrate;
pub mod probe;
pub mod static_files;
