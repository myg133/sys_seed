//! 日志过滤器：**平台唯一一份**。
//!
//! # 为什么要抽到 `platform-core`
//!
//! 这个函数原本写在 `platform-server/src/main.rs` 里，结果是
//! **所有想验证「某条日志真的会输出」的测试都够不着它** ——
//! 它们只能用自定义 `EnvFilter` 近似，于是测过的东西与线上跑的东西**不是同一份代码**。
//!
//! REQ-010 的 QA 就是这样抓到一个 HIGH：
//! `platform_config` 的 WARN 在默认部署下被**静默丢弃**，
//! 而测试用 `EnvFilter::new("platform_config=warn")` 显式放行 → **一直假绿**。
//!
//! 把过滤器放进 `platform-core` 后，**测试与生产调的是同一个函数**，
//! 「测试放行但生产吞掉」在结构上不再可能。
//!
//! # 形态：全局默认级别 + 逐 target 降级
//!
//! ```text
//! {level},tower_http=info,hyper=info,sqlx=info
//! ```
//!
//! 第一条是**无 target 的全局默认** —— tracing 对任何没有更具体 directive 的 target 都用它。
//!
//! | 情况 | 结果 |
//! |---|---|
//! | 任何平台/业务 crate（`base` / `platform_server` / `platform_config` / 将来新增的） | ✅ 按 `{level}` 输出 |
//! | 已知噪音库（[`NOISY_TARGETS`]） | 压到 `info`，不淹没业务日志 |
//!
//! # 为什么不是「逐个 crate 列 target」
//!
//! 那个写法**新增一个 crate 就会静默丢日志**。本仓库已经犯过两次：
//! 先漏 `base`（bin crate 名与 lib crate 名不同），后漏 `platform_config`。
//! 全局默认级别让「漏配」不再是失败模式 —— 这是**根因修复**，不是补清单。

use tracing_subscriber::EnvFilter;

/// 已知噪音库：全局默认级别不适用于它们，单独压到 `info`。
///
/// 这些 crate 的 `debug` / `trace` 在全局默认是 `debug` 时会产生大量逐请求日志。
pub const NOISY_TARGETS: [&str; 3] = ["tower_http", "hyper", "sqlx"];

/// 生成日志过滤器：`RUST_LOG` 优先，否则按级别生成「全局默认 + 噪音库降级」。
///
/// `RUST_LOG` 存在时**完全接管**（标准 tracing 过滤语法），不再看 `level`。
pub fn log_filter(level: &str) -> EnvFilter {
    EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        let mut directives = vec![level.to_string()];
        directives.extend(NOISY_TARGETS.iter().map(|target| format!("{target}=info")));
        EnvFilter::new(directives.join(","))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // 测试全部落在 `platform-server/src/main.rs`（那里有 `capture_logs` 辅助函数）。
    // 这里只留一条：保证「新增 crate 无需改清单」这一根因判据在本 crate 内也可回归。

    #[test]
    fn filter_has_a_global_default_directive() {
        // 形态判据：第一条 directive 必须是**无 target 的级别**。
        // 若哪天有人改回「逐个列 target」，这里会失败。
        let filter = log_filter("info");
        let rendered = filter.to_string();
        assert!(
            rendered.split(',').any(|part| !part.contains('=')),
            "过滤器缺少全局默认级别（改回逐个列 target 了？）：{rendered}"
        );
    }

    #[test]
    fn noisy_targets_are_pinned_to_info() {
        let rendered = log_filter("debug").to_string();
        for target in NOISY_TARGETS {
            assert!(
                rendered.contains(&format!("{target}=info")),
                "噪音库 {target} 没被压到 info：{rendered}"
            );
        }
    }
}
