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
//! {level},sqlx=info
//! ```
//!
//! （噪音清单见 [`NOISY_TARGETS`] —— 它**只含核实过有噪音的库**，
//! 不写「将来可能会噪音」的库，见 [D90]。）
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
/// # 这个清单只放**真的会产生噪音**的库
///
/// 曾经这里写着 `["tower_http", "hyper", "sqlx"]`，但 QA 复核指出：
/// `tower_http` **根本不在本仓依赖树里**、`hyper` 未启用 `tracing` feature ——
/// **两条是空转**。空转的 directive 不会出错，但它会让人误以为「噪音已处理」，
/// 而真正的噪音源没被点名。
///
/// 保留的都是**核实过会在 `debug` 下大量输出**的：
///
/// | crate | 为什么压 |
/// |---|---|
/// | `sqlx` | 每次查询打 `query=` 与耗时；`debug` 下一屏全是 SQL |
///
/// 今后发现新的噪音源**加进来即可**。
///
/// ⚠️ **不要在注释里写「某库还没开 tracing、所以不用压」** —— 这正是本清单一度写错的地方：
/// `hyper_util` **当下就依赖 `tracing`**（`Cargo.toml` 的 `client-legacy` 含 `dep:tracing`，
/// `cargo tree -p hyper-util` 可见 `tracing v0.1.44`），它的 `debug!("connecting to …")`
/// 确实会输出。**没把它列进来是「出站请求低频、暂不压」这个取舍**，不是「它不会输出」。
///
/// （`hyper` 本身未启用 `tracing` feature —— 这一条是核过的。）
pub const NOISY_TARGETS: [&str; 1] = ["sqlx"];

/// 生成日志过滤器：`RUST_LOG` 优先，否则按级别生成「全局默认 + 噪音库降级」。
///
/// `RUST_LOG` 存在时**完全接管**（标准 tracing 过滤语法），不再看 `level`。
pub fn log_filter(level: &str) -> EnvFilter {
    match EnvFilter::try_from_default_env() {
        Ok(filter) => filter,
        Err(_) => log_filter_ignoring_env(level),
    }
}

/// 与 [`log_filter`] 同形态，但**完全不看 `RUST_LOG`**。
///
/// 存在的理由：`log_filter` 会优先读 `RUST_LOG`，这让**依赖它的测试在
/// `RUST_LOG` 被设置时行为漂移**（QA 复核折出：`RUST_LOG=base=info` 会让
/// 平台侧 5 条测试变红）。测试要断言的正是「没有 `RUST_LOG` 时的默认形态」，
/// 所以必须有一条不受环境影响的入口。
///
/// 生产**不使用**它 —— 生产必须尊重 `RUST_LOG`。
pub fn log_filter_ignoring_env(level: &str) -> EnvFilter {
    let mut directives = vec![level.to_string()];
    directives.extend(NOISY_TARGETS.iter().map(|target| format!("{target}=info")));
    EnvFilter::new(directives.join(","))
}

/// 构造日志 subscriber（**生产与测试共用的唯一一份**）。
///
/// # ⚠️ 为什么要有这个函数（REQ-019 挑刺 M4）
///
/// 原来 `init_tracing` 直接 `tracing_subscriber::fmt().init()` ——
/// **没有 writer 缝**。测试想捕获日志只能自建一个 subscriber，
/// 那证明的是「测试装的日志层会打 trace_id」，**证明不了生产那份**。
///
/// 这正是本文件开头记的教训：`测过的东西与线上跑的不是同一份代码 → 假绿`。
///
/// → 把「构造 subscriber」抽出来：生产传 `std::io::stdout`，
/// 测试传内存 writer，**两者走同一个函数**。
///
/// # 参数
///
/// - `level`：日志级别（`RUST_LOG` 优先，见 [`log_filter`]）
/// - `writer`：日志去向（`MakeWriter`）
///
/// # ⚠️ 调用方注意
///
/// 返回的 subscriber **不**自动全局安装 ——
/// 生产用 `.init()`，测试用 `tracing::subscriber::with_default(...)`
/// （⚠️ `with_default` 是**线程局部**的：必须在同一个线程里跑被测代码，
/// 跨 `.await` 换线程会丢 —— 测试要用 `current_thread` runtime）。
pub fn build_subscriber<W>(level: &str, writer: W) -> impl tracing::Subscriber + Send + Sync
where
    W: for<'a> tracing_subscriber::fmt::MakeWriter<'a> + Send + Sync + 'static,
{
    tracing_subscriber::fmt()
        .with_env_filter(log_filter(level))
        .with_ansi(false)
        .with_writer(writer)
        .finish()
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
        let filter = log_filter_ignoring_env("info");
        let rendered = filter.to_string();
        assert!(
            rendered.split(',').any(|part| !part.contains('=')),
            "过滤器缺少全局默认级别（改回逐个列 target 了？）：{rendered}"
        );
    }

    #[test]
    fn noisy_targets_are_pinned_to_info() {
        let rendered = log_filter_ignoring_env("debug").to_string();
        for target in NOISY_TARGETS {
            assert!(
                rendered.contains(&format!("{target}=info")),
                "噪音库 {target} 没被压到 info：{rendered}"
            );
        }
    }
}
