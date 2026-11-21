//! **受限变量替换**（REQ-015，D293）。
//!
//! # 只支持一种语法
//!
//! ```text
//! {{variable_name}}
//! ```
//!
//! 变量名白名单：`^[a-z][a-z0-9_]*$`（小写字母开头，后面是小写字母 / 数字 / 下划线）。
//!
//! # ⚠️ **不做**什么（D293，每一条都有理由）
//!
//! | 不做 | 为什么 |
//! |---|---|
//! | 条件 `{{#if x}}…{{/if}}` | 通用模板语言，要引入依赖 + **SSTI** 风险 |
//! | 循环 `{{#each xs}}…{{/each}}` | 同上 |
//! | 成员访问 `{{a.b}}` | 同上 |
//! | 部分引用 `{{> partial}}` | 同上 |
//! | 函数 / 表达式 `{{constructor.constructor('x')()}}` | **SSTI 的经典载荷** |
//!
//! **租户管理员能编辑模板** → 若支持表达式，等于给了他一个在服务端执行的入口。
//!
//! # ⚠️ 三条实现纪律（每条都有对应判据）
//!
//! 1. **未知变量显式失败**（`NOTIFY_TEMPLATE_MISSING_VARIABLE`，**点名**那个变量）——
//!    不静默留空（AC-S-167 步骤 7）；
//! 2. **单趟替换**：替换结果**不再二次扫描** ——
//!    否则 payload 值里的 `{{x}}` 会在第二轮被展开 → **模板注入**；
//! 3. **payload 里多余的键忽略**（不算错，但**不得**被代入，AC-S-167 步骤 8）。

use std::collections::BTreeMap;

use crate::error::NotifyError;

/// 占位符的开标记。
const OPEN: &str = "{{";
/// 占位符的闭标记。
const CLOSE: &str = "}}";

/// 变量名是否符合白名单 `^[a-z][a-z0-9_]*$`（D293）。
pub fn is_valid_variable_name(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(first) if first.is_ascii_lowercase() => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// 从模板里抽出**全部**变量名（按出现顺序，可能重复）。
///
/// 遇到不合法的占位符 → `Err(TemplateInvalidPlaceholder)`。
pub fn extract_variables(template: &str) -> Result<Vec<String>, NotifyError> {
    let mut names = Vec::new();
    let mut rest = template;
    while let Some(start) = rest.find(OPEN) {
        let after_open = &rest[start + OPEN.len()..];
        let Some(end) = after_open.find(CLOSE) else {
            // 有 `{{` 没有 `}}` —— 不完整，视为非法。
            return Err(NotifyError::TemplateInvalidPlaceholder {
                reason: "an opening delimiter has no matching closing delimiter",
            });
        };
        let raw = &after_open[..end];
        if !is_valid_variable_name(raw) {
            return Err(NotifyError::TemplateInvalidPlaceholder {
                reason: "the placeholder is not a valid variable name",
            });
        }
        names.push(raw.to_string());
        rest = &after_open[end + CLOSE.len()..];
    }
    Ok(names)
}

/// 检查模板里**没有未闭合的 `}}`**（防止 `a}}b` 这种畸形输入被静默接受）。
fn has_orphan_closing(template: &str) -> bool {
    // 去掉所有良构的 `{{…}}` 之后，看还有没有 `}}`。
    let mut rest = template;
    let mut cleaned = String::with_capacity(template.len());
    while let Some(start) = rest.find(OPEN) {
        cleaned.push_str(&rest[..start]);
        let after_open = &rest[start + OPEN.len()..];
        match after_open.find(CLOSE) {
            Some(end) => rest = &after_open[end + CLOSE.len()..],
            None => {
                // 未闭合的 `{{` —— 让 extract_variables 去报错。
                cleaned.push_str(rest);
                return false;
            }
        }
    }
    cleaned.push_str(rest);
    cleaned.contains(CLOSE)
}

/// 渲染一个模板。
///
/// # 语义
///
/// - 每个 `{{var}}` 必须在 `payload` 里**存在**（否则 `TemplateMissingVariable`，**点名**变量）；
/// - `payload` 里多余的键**忽略**（不报错、不代入）；
/// - **单趟替换**：替换进去的值**不会再被当作模板**扫描。
pub fn render(template: &str, payload: &BTreeMap<String, String>) -> Result<String, NotifyError> {
    if has_orphan_closing(template) {
        return Err(NotifyError::TemplateInvalidPlaceholder {
            reason: "the template contains a closing delimiter without an opening one",
        });
    }

    let mut output = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(start) = rest.find(OPEN) {
        // 占位符之前的**字面文本**原样输出（不扫描其中的 `}}`）。
        output.push_str(&rest[..start]);
        let after_open = &rest[start + OPEN.len()..];
        let Some(end) = after_open.find(CLOSE) else {
            return Err(NotifyError::TemplateInvalidPlaceholder {
                reason: "an opening delimiter has no matching closing delimiter",
            });
        };
        let raw = &after_open[..end];
        if !is_valid_variable_name(raw) {
            return Err(NotifyError::TemplateInvalidPlaceholder {
                reason: "the placeholder is not a valid variable name",
            });
        }
        let Some(value) = payload.get(raw) else {
            return Err(NotifyError::TemplateMissingVariable {
                variable: raw.to_string(),
            });
        };
        // ⚠️ 单趟：`value` 直接写入输出，**不再**对它做任何占位符解析。
        output.push_str(value);
        rest = &after_open[end + CLOSE.len()..];
    }
    // 尾部字面文本。
    output.push_str(rest);
    Ok(output)
}

/// 预览（与 [`render`] 等价 —— 「预览」就是把给定 payload 渲染一次）。
///
/// AC-S-167 步骤 4 的「模板预览」不需要第二套实现：
/// 预览面板拿到的是**同一份**渲染结果。
pub fn preview(template: &str, payload: &BTreeMap<String, String>) -> Result<String, NotifyError> {
    render(template, payload)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn payload(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect()
    }

    // -- 白名单 ------------------------------------------------------------

    /// 合法变量名的**正向边界**逐个成立（AC-S-167 步骤 5）。
    #[test]
    fn valid_variable_names_are_accepted() {
        for name in ["a", "a1", "a_1", "abc_def_9", "x9_y8"] {
            assert!(is_valid_variable_name(name), "应接受合法变量名：{name}");
        }
    }

    /// 非法变量名**逐个**被拒（AC-S-167 步骤 6）。
    #[test]
    fn invalid_variable_names_are_rejected() {
        for name in ["Name", "1a", "a-b", "", " a ", "a b", "A", "_a", "a.b"] {
            assert!(!is_valid_variable_name(name), "应拒绝非法变量名：{name:?}");
        }
    }

    // -- 渲染主路径 --------------------------------------------------------

    /// 主路径：两个变量**逐字**替换（AC-S-167 步骤 4）。
    #[test]
    fn the_main_path_renders_both_variables() {
        let out = render(
            "你好 {{name}}，订单 {{order_id}} 已发货",
            &payload(&[("name", "张三"), ("order_id", "A-1")]),
        )
        .expect("应渲染成功");
        assert_eq!(out, "你好 张三，订单 A-1 已发货");
    }

    /// 没有占位符的模板原样返回。
    #[test]
    fn a_template_without_placeholders_is_returned_verbatim() {
        let out = render("纯文本，没有变量", &payload(&[])).expect("应渲染成功");
        assert_eq!(out, "纯文本，没有变量");
    }

    /// 同一个变量出现多次，每处都替换。
    #[test]
    fn the_same_variable_is_replaced_everywhere() {
        let out = render("{{a}}-{{a}}-{{a}}", &payload(&[("a", "x")])).expect("应渲染成功");
        assert_eq!(out, "x-x-x");
    }

    /// **多余变量被忽略**（不算错，但不得被代入）（AC-S-167 步骤 8）。
    #[test]
    fn extra_payload_keys_are_ignored() {
        let out = render(
            "只用 {{name}}",
            &payload(&[("name", "张三"), ("extra", "不该出现")]),
        )
        .expect("多余变量不算错");
        assert_eq!(out, "只用 张三");
        assert!(!out.contains("不该出现"), "多余变量不得被代入：{out}");
    }

    // -- 失败路径 ----------------------------------------------------------

    /// **缺变量显式失败并点名**（AC-S-167 步骤 7）。
    #[test]
    fn a_missing_variable_fails_and_names_it() {
        let err = render(
            "你好 {{name}}，订单 {{order_id}} 已发货",
            &payload(&[("name", "张三")]),
        )
        .expect_err("缺变量必须失败");
        assert_eq!(err.code(), "NOTIFY_TEMPLATE_MISSING_VARIABLE");
        let detail = err.detail();
        assert!(detail.contains("order_id"), "必须点名缺失的变量：{detail}");
        assert!(
            !detail.contains("name") || detail.contains("order_id"),
            "点名的是缺失的那个"
        );
    }

    /// 缺变量**不静默留空**（这是本条最重要的区分力）。
    #[test]
    fn a_missing_variable_never_renders_as_empty() {
        let result = render("[{{missing}}]", &payload(&[]));
        assert!(result.is_err(), "缺变量不得静默留空：{result:?}");
    }

    /// 未闭合的 `{{` → 非法占位符（不是缺变量）。
    #[test]
    fn an_unclosed_opening_delimiter_is_invalid() {
        let err = render("你好 {{name", &payload(&[("name", "x")])).expect_err("必须失败");
        assert_eq!(err.code(), "NOTIFY_TEMPLATE_INVALID_PLACEHOLDER");
    }

    /// 孤立的 `}}` → 非法占位符。
    #[test]
    fn an_orphan_closing_delimiter_is_invalid() {
        let err = render("a}}b", &payload(&[])).expect_err("必须失败");
        assert_eq!(err.code(), "NOTIFY_TEMPLATE_INVALID_PLACEHOLDER");
    }

    // -- SSTI 载荷（AC-S-167 步骤 11）--------------------------------------

    /// 4 个 SSTI / 模板语言载荷**全部**被拒为非法占位符。
    #[test]
    fn every_ssti_payload_is_rejected_as_invalid_placeholder() {
        let cases = [
            "{{#if x}}y{{/if}}",
            "{{x.y}}",
            "{{> partial}}",
            "{{constructor.constructor('x')()}}",
        ];
        for case in cases {
            let result = render(case, &payload(&[("x", "1"), ("y", "2")]));
            let err = result.expect_err(&format!("SSTI 载荷必须被拒：{case}"));
            assert_eq!(
                err.code(),
                "NOTIFY_TEMPLATE_INVALID_PLACEHOLDER",
                "载荷 {case} 的错误码不对"
            );
        }
    }

    /// SSTI 载荷**不会 panic**（AC-S-167 步骤 11 的第 ① 条判据）。
    #[test]
    fn ssti_payloads_never_panic() {
        let cases = [
            "{{#if x}}y{{/if}}",
            "{{x.y}}",
            "{{> partial}}",
            "{{constructor.constructor('x')()}}",
            "{{}}",
            "{{{{}}}}",
            "{{a",
            "}}",
            "{{a}}}}",
            "{{__proto__}}",
            "{{a}}}}{{b}}",
        ];
        for case in cases {
            let outcome = std::panic::catch_unwind(|| {
                let _ = render(case, &payload(&[("a", "1"), ("b", "2")]));
            });
            assert!(outcome.is_ok(), "渲染 {case:?} 时 panic 了");
        }
    }

    // -- 单趟替换（防二次注入）---------------------------------------------

    /// **替换结果不再被当作模板**：payload 值里的 `{{x}}` 原样保留。
    ///
    /// ⚠️ 这是**最重要**的一条：若实现做了两趟扫描，
    /// 攻击者把 `{{secret}}` 放进自己的显示名，就能读到别人的变量。
    #[test]
    fn substituted_values_are_not_re_scanned() {
        let out = render(
            "你好 {{name}}",
            &payload(&[("name", "{{secret}}"), ("secret", "不该被展开")]),
        )
        .expect("应渲染成功");
        assert_eq!(
            out, "你好 {{secret}}",
            "替换结果不得被二次扫描（否则是模板注入）"
        );
    }

    /// 值里含 `}}` 也不会破坏解析（因为不再扫描）。
    #[test]
    fn substituted_values_with_delimiters_do_not_break_parsing() {
        let out = render("a{{v}}b", &payload(&[("v", "}}x{{")])).expect("应渲染成功");
        assert_eq!(out, "a}}x{{b");
    }

    // -- 抽取变量 ----------------------------------------------------------

    /// 抽取按出现顺序返回全部变量名（含重复）。
    #[test]
    fn extract_returns_names_in_order_with_duplicates() {
        let names = extract_variables("{{a}} {{b}} {{a}}").expect("应成功");
        assert_eq!(names, vec!["a", "b", "a"]);
    }

    /// 抽取也会拒绝非法占位符。
    #[test]
    fn extract_rejects_invalid_placeholders() {
        assert!(extract_variables("{{Name}}").is_err());
        assert!(extract_variables("{{a").is_err());
    }

    /// 预览与渲染**结果一致**（预览不是第二套实现）。
    #[test]
    fn preview_equals_render() {
        let tpl = "你好 {{name}}";
        let pl = payload(&[("name", "张三")]);
        assert_eq!(preview(tpl, &pl), render(tpl, &pl));
    }
}
