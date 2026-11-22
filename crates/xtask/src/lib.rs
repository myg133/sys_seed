//! 底座工程任务运行器。
//!
//! | 子命令 | 作用 |
//! |--------|------|
//! | `check-modules` | 校验 `modules/<name>/` 目录规范 |
//! | `check-deps` | 校验依赖方向：平台 crate 不得反向依赖业务模块 |
//! | `capabilities` | 生成 / 校验能力索引（`capabilities/`） |
//! | `create-module` | 生成业务模块骨架（开箱即过契约检查） |
//! | `tenant-policies` | 按 manifest 的 `tenantScoped` 段统一生成 / 校验 RLS 策略 |
//! | `audit` | 审计链校验 / 归档（REQ-014） |

pub mod audit;
pub mod capabilities;
pub mod catalog;
pub mod cli;
pub mod deps;
pub mod modules;
pub mod notify;
pub mod rustscan;
pub mod scaffold;
pub mod storage;
pub mod tenant_policies;

use std::path::{Path, PathBuf};

/// 工作区根目录（crates/xtask 的上两级）。
pub fn workspace_root() -> PathBuf {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .unwrap_or_else(|| manifest_dir.to_path_buf())
}
