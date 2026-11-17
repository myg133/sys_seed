//! 底座工程任务运行器。
//!
//! 本期只提供目录规范校验（AC-S-001）。其余子命令在后续 Sprint 按需补齐。

pub mod cli;
pub mod modules;

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
