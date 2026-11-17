//! 沙箱模块实体。
//!
//! 最小实体：一条记录只属于一个租户 —— 租户隔离（C-3）就是围绕它验证的。

/// 沙箱示例实体。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DemoItem {
    /// 主键。
    pub id: String,
    /// 归属租户。
    pub tenant_id: String,
    /// 标题。
    pub title: String,
    /// 版本号：用于判断一次写操作是否真的生效。
    pub revision: u32,
}
