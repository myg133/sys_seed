//! 平台配置：三层覆盖（平台 / 租户 / 空间）、品牌聚合、Feature Flag 与确定性灰度。
//!
//! # 一句话（design-summary §0）
//!
//! 把「底座的视觉与行为」从**代码里的常量**变成**按作用域分层的数据**：
//! 平台默认 → 租户覆盖 → 空间覆盖，改完**下一个请求**就生效；
//! 开关按租户与灰度比例**确定性地**判定，而不是每次请求掷骰子。
//!
//! # 与其它 crate 的关系
//!
//! ```text
//! platform-core (ConfigResolver trait / ConfigSource / ConfigKey)
//!       ↑ 实现
//! platform-config (本 crate：DbConfigResolver / 注册表 / HTTP)
//!       ↑ 装配
//! platform-server (挂到 /api/config 与 /api/feature-flags)
//! ```
//!
//! 业务模块**只**通过 `platform_core::extension::config::ConfigResolver`
//! 读配置，不依赖本 crate 的内部。
//!
//! # 关键不变式（每一条都有测试守着）
//!
//! | 不变式 | 位置 |
//! |--------|------|
//! | 缓存失效 = **成对比较** `(platform_epoch, tenant_epoch)`，不取 `max` | [`resolver`] |
//! | 平台级表直接 DML 被 `REVOKE`，唯一写通道是 `SECURITY DEFINER` 函数 | `migrations/20261118001000_config_schema.up.sql` |
//! | flag 只能经 flag 接口改（通用写路由 → 400） | [`service`] |
//! | 灰度用 `sha256("<flag>:<user>") % 100`，**禁 `RandomState`** | [`flag`] |
//! | 无用户身份 → `enabled=false`（`p=100` 也不变） | [`resolver`] |
//! | 品牌公开接口**枚举白名单**、忽略空间级 | [`whitelist`] |
//! | 空间级写 = `scope: space` **且**在白名单内 | [`service`] |
//! | 伪租户 `"platform"` 是保留值，真实租户 id 取它 → 报错 | [`service`] |

pub mod error;
pub mod flag;
pub mod http;
pub mod registry;
pub mod resolver;
pub mod service;
pub mod store;
pub mod whitelist;

pub use error::ConfigError;
pub use registry::{ConfigRegistry, RegisteredKey};
pub use resolver::DbConfigResolver;
pub use service::{ConfigService, FlagWriteRequest, WriteEntry, WriteRequest, WriteRoute};
pub use whitelist::{BRANDING_WHITELIST, is_branding_key};
