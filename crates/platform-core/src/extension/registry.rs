//! 编译期注册：模块声明即被发现。
//!
//! # 为什么不用运行期动态插件
//!
//! ADR-0001 D5 已定：运行期插件带来安全面、调试难、回滚难。
//! 编译期注册有类型安全、性能可控、单测友好。
//!
//! # 注册表元素为什么是包装类型
//!
//! 设计摘要写的是 `inventory::collect!(PlatformModule)` + `inventory::iter::<dyn PlatformModule>`，
//! 但 `inventory` 0.3 的 `Collect` trait 带 `Sized` 约束，
//! `collect!(dyn PlatformModule)` **无法编译**（inventory 自带的 `tests/ui/collect-unsized.rs`
//! 就是这条的编译失败用例）。
//! 因此注册表存的是具体包装类型 [`ModuleRegistration`]，对外的
//! [`discover`] 仍然返回 `Vec<&'static dyn PlatformModule>` —— 对调用方没有区别。

use crate::extension::platform_module::PlatformModule;

/// 编译期注册条目：一个已登记的平台模块。
#[derive(Clone, Copy)]
pub struct ModuleRegistration {
    /// 被注册的模块实现。
    pub module: &'static dyn PlatformModule,
}

inventory::collect!(ModuleRegistration);

/// 发现所有编译期注册的模块（顺序不保证）。
pub fn discover() -> Vec<&'static dyn PlatformModule> {
    inventory::iter::<ModuleRegistration>
        .into_iter()
        .map(|registration| registration.module)
        .collect()
}

/// 发现所有已启用的模块（顺序不保证）。
///
/// 未启用的模块不加载 —— 对应 `manifest.json` 的 `enabled` 开关。
pub fn discover_enabled() -> Vec<&'static dyn PlatformModule> {
    discover()
        .into_iter()
        .filter(|module| module.enabled())
        .collect()
}

/// 登记一个平台模块（编译期生效）。
///
/// 必须在**模块作用域**调用，不能写在函数体内 —— `inventory::submit!`
/// 生成的是链接期构造器。
///
/// ```ignore
/// static SANDBOX_MODULE: SandboxModule = SandboxModule;
/// platform_core::register_platform_module!(SANDBOX_MODULE);
/// ```
#[macro_export]
macro_rules! register_platform_module {
    ($module:expr) => {
        $crate::extension::registry::inventory::submit! {
            $crate::extension::registry::ModuleRegistration {
                module: &$module,
            }
        }
    };
}

/// 让 `inventory` 宏在展开处可见：业务模块无需直接依赖 `inventory`。
pub use inventory;

#[cfg(test)]
mod tests {
    use super::*;

    struct Demo;

    impl PlatformModule for Demo {
        fn id(&self) -> &'static str {
            "demo"
        }
        fn version(&self) -> &'static str {
            "0.1.0"
        }
        fn enabled(&self) -> bool {
            true
        }
    }

    struct Disabled;

    impl PlatformModule for Disabled {
        fn id(&self) -> &'static str {
            "disabled"
        }
        fn version(&self) -> &'static str {
            "0.1.0"
        }
        fn enabled(&self) -> bool {
            false
        }
    }

    static DEMO: Demo = Demo;
    static DISABLED: Disabled = Disabled;

    crate::register_platform_module!(DEMO);
    crate::register_platform_module!(DISABLED);

    #[test]
    fn registered_modules_are_discovered() {
        let ids: Vec<&str> = discover().iter().map(|module| module.id()).collect();
        assert!(ids.contains(&"demo"));
        assert!(ids.contains(&"disabled"));
    }

    #[test]
    fn disabled_modules_are_filtered_out() {
        let ids: Vec<&str> = discover_enabled()
            .iter()
            .map(|module| module.id())
            .collect();
        assert!(ids.contains(&"demo"));
        assert!(!ids.contains(&"disabled"));
    }
}
