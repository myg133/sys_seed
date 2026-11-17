//! 扩展点 3：审计埋点声明。
//!
//! 平台据此自动埋点，业务模块**不手工写审计表**。
//! 审计表的固定字段与哈希链规则见 `api-conventions v1` C12。

/// 审计事件声明。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditEventSpec {
    /// 事件编码（模块内唯一）。
    pub code: String,
    /// 动作名（如 `create` / `approve`）。
    pub action: String,
    /// 审计对象类型（如 `demo_item`）。
    pub object_type: String,
    /// 面向开发者的英文描述。
    pub description: String,
}

impl AuditEventSpec {
    /// 声明一个审计事件。
    pub fn new(
        code: impl Into<String>,
        action: impl Into<String>,
        object_type: impl Into<String>,
    ) -> Self {
        Self {
            code: code.into(),
            action: action.into(),
            object_type: object_type.into(),
            description: String::new(),
        }
    }

    /// 补充描述。
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }
}

/// 审计埋点扩展点。
pub trait Auditable: Send + Sync {
    /// 声明该模块会产生的审计事件。
    fn audit_events(&self) -> &[AuditEventSpec];
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audit_event_spec_carries_code_action_and_object_type() {
        let spec = AuditEventSpec::new("demo.create", "create", "demo_item")
            .with_description("create demo record");
        assert_eq!(spec.code, "demo.create");
        assert_eq!(spec.action, "create");
        assert_eq!(spec.object_type, "demo_item");
        assert_eq!(spec.description, "create demo record");
    }

    #[test]
    fn auditable_exposes_declared_events() {
        struct Demo {
            events: Vec<AuditEventSpec>,
        }

        impl Auditable for Demo {
            fn audit_events(&self) -> &[AuditEventSpec] {
                &self.events
            }
        }

        let demo = Demo {
            events: vec![AuditEventSpec::new("demo.create", "create", "demo_item")],
        };
        assert_eq!(demo.audit_events().len(), 1);
        assert_eq!(demo.audit_events()[0].code, "demo.create");
    }
}
