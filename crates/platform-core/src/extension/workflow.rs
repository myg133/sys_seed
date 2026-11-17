//! 扩展点 5：工作流接入。
//!
//! 本期只定义 trait 与配套类型；工作流的实际消费方在 REQ-017。

use std::collections::BTreeMap;
use std::future::Future;

use anyhow::Result;

use crate::context::RequestContext;

/// 状态流转事件。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TransitionEvent {
    /// 流程定义 key，与 [`WorkflowHandler::workflow_key`] 对应。
    pub workflow_key: String,
    /// 流程实例 ID。
    pub instance_id: String,
    /// 流转前状态；首次进入流程时为空。
    pub from_state: Option<String>,
    /// 流转后状态。
    pub to_state: String,
    /// 业务载荷。
    pub payload: BTreeMap<String, String>,
}

impl TransitionEvent {
    /// 构造一次状态流转事件。
    pub fn new(
        workflow_key: impl Into<String>,
        instance_id: impl Into<String>,
        from_state: Option<String>,
        to_state: impl Into<String>,
    ) -> Self {
        Self {
            workflow_key: workflow_key.into(),
            instance_id: instance_id.into(),
            from_state,
            to_state: to_state.into(),
            payload: BTreeMap::new(),
        }
    }

    /// 添加业务载荷。
    pub fn with_payload(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.payload.insert(key.into(), value.into());
        self
    }
}

/// 工作流接入扩展点。
pub trait WorkflowHandler: Send + Sync {
    /// 与流程定义 key 对应。
    fn workflow_key(&self) -> &'static str;

    /// 处理一次状态流转。
    fn on_transition(
        &self,
        ctx: &RequestContext,
        ev: &TransitionEvent,
    ) -> impl Future<Output = Result<()>> + Send;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transition_event_keeps_from_and_to_state() {
        let ev = TransitionEvent::new("demo-approval", "inst-1", Some("draft".into()), "approved")
            .with_payload("demo_id", "d-1");
        assert_eq!(ev.workflow_key, "demo-approval");
        assert_eq!(ev.from_state.as_deref(), Some("draft"));
        assert_eq!(ev.to_state, "approved");
        assert_eq!(ev.payload.get("demo_id").map(String::as_str), Some("d-1"));
    }

    #[test]
    fn workflow_handler_reports_its_key() {
        struct Demo;
        impl WorkflowHandler for Demo {
            fn workflow_key(&self) -> &'static str {
                "demo-approval"
            }
            async fn on_transition(
                &self,
                _ctx: &RequestContext,
                _ev: &TransitionEvent,
            ) -> Result<()> {
                Ok(())
            }
        }

        assert_eq!(Demo.workflow_key(), "demo-approval");
    }
}
