//! **病毒扫描钩子**（REQ-018，D326 / AC-S-178 步骤 3 与 4）。
//!
//! # ⚠️ 默认是 `Skipped`，**不是** `Clean`
//!
//! 「没扫」和「扫过且干净」是**两件事**。
//! 默认返回 `Clean` 会让「病毒扫描已启用」变成一句谎话
//! （与 D292 的「未配置 ≠ 可用」同款）。
//!
//! → 默认 [`NoopScanner`] 返回 [`ScanVerdict::Skipped`]，
//! 且这个值**如实出现在响应体**（`scanVerdict`）与数据库（`scan_verdict` 列）。

use std::future::Future;
use std::pin::Pin;

/// 扫描判定。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScanVerdict {
    /// 扫过且干净。
    Clean,
    /// 扫过且判定为感染。
    Infected {
        /// 命中的签名名（如 `EICAR-Test-File`）。
        signature: String,
    },
    /// **没扫**（不是「干净」）。
    Skipped,
}

impl ScanVerdict {
    /// 落库用的字符串（与 `model::scan_verdict` 一致）。
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Clean => crate::model::scan_verdict::CLEAN,
            Self::Infected { .. } => crate::model::scan_verdict::INFECTED,
            Self::Skipped => crate::model::scan_verdict::SKIPPED,
        }
    }

    /// 命中感染的签名（其余为 `None`）。
    pub fn signature(&self) -> Option<&str> {
        match self {
            Self::Infected { signature } => Some(signature),
            _ => None,
        }
    }
}

/// 装箱的 future。
pub type ScanFuture<'a> = Pin<Box<dyn Future<Output = Result<ScanVerdict, String>> + Send + 'a>>;

/// 病毒扫描的接缝。
///
/// ⚠️ 用 `ScanFuture` 而不是 `impl Future` —— 后者让 trait 不可对象化。
pub trait VirusScanner: Send + Sync {
    /// 扫一段字节。
    fn scan<'a>(&'a self, bytes: &'a [u8]) -> ScanFuture<'a>;
}

/// **默认实现：不扫**（返回 `Skipped`）。
///
/// ⚠️ 想接真 ClamAV 时实现本 trait 并在装配层替换 —— 见 **TD（真实扫描未做）**。
#[derive(Debug, Clone, Copy, Default)]
pub struct NoopScanner;

impl VirusScanner for NoopScanner {
    fn scan<'a>(&'a self, _bytes: &'a [u8]) -> ScanFuture<'a> {
        Box::pin(async { Ok(ScanVerdict::Skipped) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **默认扫描器返回 `Skipped` 而不是 `Clean`**（D326 的核心判据，TC-018-027）。
    #[tokio::test]
    async fn the_default_scanner_reports_skipped_not_clean() {
        let verdict = NoopScanner.scan(b"any bytes").await.expect("不应失败");
        assert_eq!(verdict, ScanVerdict::Skipped);
        assert_ne!(verdict, ScanVerdict::Clean, "默认**不得**声称干净");
        assert_eq!(verdict.as_str(), "skipped");
        assert_ne!(verdict.as_str(), "clean");
    }

    /// 三种判定的字符串与 `model::scan_verdict` 一致。
    #[test]
    fn the_verdict_strings_match_the_model_constants() {
        assert_eq!(
            ScanVerdict::Clean.as_str(),
            crate::model::scan_verdict::CLEAN
        );
        assert_eq!(
            ScanVerdict::Skipped.as_str(),
            crate::model::scan_verdict::SKIPPED
        );
        assert_eq!(
            ScanVerdict::Infected {
                signature: "X".into()
            }
            .as_str(),
            crate::model::scan_verdict::INFECTED
        );
    }

    /// 只有 `Infected` 有签名。
    #[test]
    fn only_infected_carries_a_signature() {
        assert_eq!(
            ScanVerdict::Infected {
                signature: "EICAR".into()
            }
            .signature(),
            Some("EICAR")
        );
        assert_eq!(ScanVerdict::Clean.signature(), None);
        assert_eq!(ScanVerdict::Skipped.signature(), None);
    }

    /// 判定可以被测试替身替换（证明接缝可注入）。
    #[tokio::test]
    async fn the_trait_is_substitutable() {
        struct Infected;
        impl VirusScanner for Infected {
            fn scan<'a>(&'a self, _bytes: &'a [u8]) -> ScanFuture<'a> {
                Box::pin(async {
                    Ok(ScanVerdict::Infected {
                        signature: "EICAR-Test-File".to_string(),
                    })
                })
            }
        }
        let scanner: &dyn VirusScanner = &Infected;
        let verdict = scanner.scan(b"x").await.expect("不应失败");
        assert_eq!(verdict.signature(), Some("EICAR-Test-File"));
    }
}
