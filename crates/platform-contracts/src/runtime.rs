//! 契约套件的异步运行时。
//!
//! 契约检查需要 `await`，但业务模块的测试 crate 不应该被迫依赖 tokio ——
//! 套件在内部用当前线程运行时驱动 future，模块侧只调用 [`block_on`]。

use std::future::Future;

/// 在当前线程运行时上把一个 future 驱动到完成。
pub fn block_on<F: Future>(future: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("failed to build the current-thread runtime")
        .block_on(future)
}

#[cfg(test)]
mod tests {
    use super::block_on;

    #[test]
    fn block_on_drives_a_future_to_completion() {
        let value = block_on(async { 1 + 1 });
        assert_eq!(value, 2);
    }
}
