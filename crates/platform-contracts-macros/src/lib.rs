//! `#[platform_module_contract]` —— 扩展点契约测试宏（简化版）。
//!
//! # 为什么是过程宏而不是 `macro_rules!`
//!
//! Rust 1.98 上 `macro_rules!` 的属性宏形态（`attr($item:item) => ...`）仍是
//! unstable 特性（rust-lang/rust#143547），稳定通道上拿不到被标注的 item。
//! 因此这里用过程宏实现 —— 只依赖标准库的 `proc_macro`，**不引入** syn / quote /
//! proc-macro2，第三方依赖仍然只有 `inventory` 一个。
//!
//! # 用法
//!
//! 被标注的 `mod` 里必须提供一个 `contract_subject()` 函数，返回
//! `platform_contracts::ContractSubject<你的夹具类型>`：
//!
//! ```ignore
//! #[platform_module_contract]
//! mod sandbox_contract {
//!     use platform_contracts::prelude::*;
//!
//!     fn contract_subject() -> ContractSubject<SandboxFixture> {
//!         ContractSubject {
//!             module: Box::new(sandbox::SandboxModule),
//!             fixture: SandboxFixture,
//!         }
//!     }
//! }
//! ```
//!
//! 宏会在该 `mod` 内追加 5 个 `#[test]`：C-1 / C-2 / C-3 / C-4 / C-5。

use proc_macro::{Delimiter, Group, Ident, Literal, TokenStream, TokenTree};

/// 生成的六个契约测试。
///
/// 用字符串写死再解析成 token 流：这段代码是固定的，字符串形式比手写 token 拼接
/// 可读得多，且不引入 quote / syn。
const GENERATED_TESTS: &str = r#"
    #[cfg(test)]
    #[test]
    fn contract_c1_manifest_matches_implementation() {
        let subject = contract_subject();
        let outcome = ::platform_contracts::checks::check_c1(&subject)
            .unwrap_or_else(|err| panic!("C-1 check could not run: {err}"));
        ::platform_contracts::checks::assert_compliant("C-1", &outcome);
    }

    #[cfg(test)]
    #[test]
    fn contract_c2_permission_points_are_declared() {
        let subject = contract_subject();
        let outcome = ::platform_contracts::checks::check_c2(&subject)
            .unwrap_or_else(|err| panic!("C-2 check could not run: {err}"));
        ::platform_contracts::checks::assert_compliant("C-2", &outcome);
    }

    #[cfg(test)]
    #[test]
    fn contract_c3_tenant_isolation_holds() {
        let subject = contract_subject();
        let outcome = ::platform_contracts::block_on(async {
            ::platform_contracts::checks::check_c3(&subject)
                .await
                .unwrap_or_else(|err| panic!("C-3 check could not run: {err}"))
        });
        ::platform_contracts::checks::assert_compliant("C-3", &outcome);
    }

    #[cfg(test)]
    #[test]
    fn contract_c4_transaction_boundary_holds() {
        let subject = contract_subject();
        let outcome = ::platform_contracts::block_on(async {
            ::platform_contracts::checks::check_c4(&subject)
                .await
                .unwrap_or_else(|err| panic!("C-4 check could not run: {err}"))
        });
        ::platform_contracts::checks::assert_compliant("C-4", &outcome);
    }

    #[cfg(test)]
    #[test]
    fn contract_c5_declared_permissions_have_authorization_call_sites() {
        let subject = contract_subject();
        let outcome = ::platform_contracts::checks::check_c5(&subject)
            .unwrap_or_else(|err| panic!("C-5 check could not run: {err}"));
        ::platform_contracts::checks::assert_compliant("C-5", &outcome);
    }

    #[cfg(test)]
    #[test]
    fn contract_c6_schema_projection_is_used() {
        let subject = contract_subject();
        let outcome = ::platform_contracts::checks::check_c6(&subject)
            .unwrap_or_else(|err| panic!("C-6 check could not run: {err}"));
        ::platform_contracts::checks::assert_compliant("C-6", &outcome);
    }
"#;

/// 被标注的 `mod` 解析结果。
struct ParsedModule {
    visibility: Option<TokenStream>,
    name: Ident,
    body: TokenStream,
}

/// 为业务模块生成 C-1 ~ C-5 五项契约测试。
///
/// 宏不接受参数：被标注的 `mod` 内需提供 `contract_subject()`。
#[proc_macro_attribute]
pub fn platform_module_contract(attr: TokenStream, item: TokenStream) -> TokenStream {
    let mut output = TokenStream::new();

    if !attr.is_empty() {
        output.extend(compile_error(
            "#[platform_module_contract] takes no arguments; define `fn contract_subject()` inside the module instead",
        ));
        return output;
    }

    match parse_module(item) {
        Ok(parsed) => {
            let mut body: Vec<TokenTree> = parsed.body.into_iter().collect();
            body.extend(
                GENERATED_TESTS
                    .parse::<TokenStream>()
                    .expect("generated test tokens must be valid Rust"),
            );

            if let Some(visibility) = parsed.visibility {
                output.extend(visibility);
            }
            output.extend(
                "mod"
                    .parse::<TokenStream>()
                    .expect("`mod` must be a valid token stream"),
            );
            output.extend([TokenTree::Ident(parsed.name)]);
            output.extend(TokenStream::from(TokenTree::Group(Group::new(
                Delimiter::Brace,
                body.into_iter().collect(),
            ))));
        }
        Err(message) => output.extend(compile_error(&message)),
    }

    output
}

/// 解析 `mod <name> { ... }`（允许 `pub` / `pub(...)` 可见性）。
fn parse_module(item: TokenStream) -> Result<ParsedModule, String> {
    let mut trees: Vec<TokenTree> = item.into_iter().collect();

    let mut visibility: Vec<TokenTree> = Vec::new();
    if let Some(TokenTree::Ident(first)) = trees.first() {
        if first.to_string() == "pub" {
            visibility.push(trees.remove(0));
            if let Some(TokenTree::Group(group)) = trees.first() {
                if group.delimiter() == Delimiter::Parenthesis {
                    visibility.push(trees.remove(0));
                }
            }
        }
    }

    let mut iter = trees.into_iter();
    match iter.next() {
        Some(TokenTree::Ident(keyword)) if keyword.to_string() == "mod" => {}
        _ => {
            return Err(
                "#[platform_module_contract] expects a `mod <name> { ... }` item".to_string(),
            );
        }
    }

    let name = match iter.next() {
        Some(TokenTree::Ident(name)) => name,
        _ => {
            return Err("#[platform_module_contract] expects a module name".to_string());
        }
    };

    let body = match iter.next() {
        Some(TokenTree::Group(group)) if group.delimiter() == Delimiter::Brace => group.stream(),
        _ => {
            return Err("#[platform_module_contract] expects a braced module body".to_string());
        }
    };

    Ok(ParsedModule {
        visibility: if visibility.is_empty() {
            None
        } else {
            Some(visibility.into_iter().collect())
        },
        name,
        body,
    })
}

/// 构造 `compile_error!("...")`。
fn compile_error(message: &str) -> TokenStream {
    let mut tokens = TokenStream::new();
    tokens.extend(
        "compile_error!"
            .parse::<TokenStream>()
            .expect("`compile_error!` must be a valid token stream"),
    );
    tokens.extend([TokenTree::Literal(Literal::string(message))]);
    tokens
}
