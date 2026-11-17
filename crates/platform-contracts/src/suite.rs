//! 契约套件框架。

use thiserror::Error;

/// 一处契约违例：哪个检查项、违反了什么。
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("contract violation at {check_id}: {message}")]
pub struct ContractViolation {
    /// 违反的检查项 ID。
    pub check_id: String,
    /// 面向开发者的英文描述。
    pub message: String,
}

impl ContractViolation {
    /// 构造一条违例。
    pub fn new(check_id: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            check_id: check_id.into(),
            message: message.into(),
        }
    }
}

/// 一项契约检查。
///
/// 业务模块为每个扩展点实现检查逻辑，平台分发同一套测试；
/// 检查失败返回 `ContractViolation`，由套件统一收集。
pub trait ContractCheck {
    /// 检查项 ID，全局唯一。
    fn id(&self) -> &str;

    /// 执行检查；通过返回 `Ok(())`。
    fn run(&self) -> Result<(), ContractViolation>;
}

/// 契约套件：收集若干检查项并汇总结果。
#[derive(Default)]
pub struct ContractSuite {
    checks: Vec<Box<dyn ContractCheck>>,
}

/// 契约执行报告。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SuiteReport {
    /// 检查项总数。
    pub total: usize,
    /// 通过的检查项数。
    pub passed: usize,
    /// 收集到的违例。
    pub violations: Vec<ContractViolation>,
}

impl SuiteReport {
    /// 失败（未通过）的检查项数。
    pub fn failed(&self) -> usize {
        self.violations.len()
    }

    /// 是否完全合规。
    pub fn is_compliant(&self) -> bool {
        self.violations.is_empty()
    }
}

impl ContractSuite {
    /// 空套件。
    pub fn new() -> Self {
        Self::default()
    }

    /// 登记一个检查项。
    pub fn register(&mut self, check: Box<dyn ContractCheck>) {
        self.checks.push(check);
    }

    /// 已登记的检查项数。
    pub fn len(&self) -> usize {
        self.checks.len()
    }

    /// 套件是否为空。
    pub fn is_empty(&self) -> bool {
        self.checks.is_empty()
    }

    /// 逐项执行检查并汇总报告。
    ///
    /// 单项失败不会中断整轮执行：所有违例一次性收集，便于一次看全。
    pub fn run(&self) -> SuiteReport {
        let mut report = SuiteReport {
            total: self.checks.len(),
            ..SuiteReport::default()
        };
        for check in &self.checks {
            match check.run() {
                Ok(()) => report.passed += 1,
                Err(violation) => report.violations.push(violation),
            }
        }
        report
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct AlwaysPass;

    impl ContractCheck for AlwaysPass {
        fn id(&self) -> &str {
            "always-pass"
        }

        fn run(&self) -> Result<(), ContractViolation> {
            Ok(())
        }
    }

    struct AlwaysFail;

    impl ContractCheck for AlwaysFail {
        fn id(&self) -> &str {
            "always-fail"
        }

        fn run(&self) -> Result<(), ContractViolation> {
            Err(ContractViolation::new("always-fail", "must reject"))
        }
    }

    #[test]
    fn empty_suite_is_compliant() {
        let suite = ContractSuite::new();
        assert!(suite.is_empty());
        let report = suite.run();
        assert!(report.is_compliant());
        assert_eq!(report.total, 0);
    }

    #[test]
    fn passing_check_is_counted() {
        let mut suite = ContractSuite::new();
        suite.register(Box::new(AlwaysPass));
        let report = suite.run();
        assert_eq!(report.total, 1);
        assert_eq!(report.passed, 1);
        assert_eq!(report.failed(), 0);
        assert!(report.is_compliant());
    }

    #[test]
    fn violations_are_collected_without_stopping() {
        let mut suite = ContractSuite::new();
        suite.register(Box::new(AlwaysFail));
        suite.register(Box::new(AlwaysPass));
        suite.register(Box::new(AlwaysFail));

        let report = suite.run();
        assert_eq!(report.total, 3);
        assert_eq!(report.passed, 1);
        assert_eq!(report.failed(), 2);
        assert!(!report.is_compliant());
        assert_eq!(report.violations[0].check_id, "always-fail");
        assert_eq!(report.violations[0].message, "must reject");
    }
}
