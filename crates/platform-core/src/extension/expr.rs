//! 受限表达式 DSL：自定义过滤条件与属性级校验（RBAC + ABAC 混合）共用一套语法。
//!
//! # 为什么不是任意 SQL
//!
//! 让业务方写 SQL 等于开 SQL 注入面。自定义过滤是**声明式条件**，不是查询语言。
//! 因此本模块有**三条硬约束**（design-summary §5.2）：
//!
//! | 约束 | 落地方式 |
//! |------|----------|
//! | **字段必须白名单** | 不在白名单里的属性 → **解析失败**（fail closed） |
//! | **值必须参数化** | 生成的 SQL 里只有占位符，字面值一律走绑定 |
//! | **函数调用不允许** | `now()` / `substring()` 之类一律**解析失败** |
//!
//! # 语法
//!
//! ```text
//! expr    := or
//! or      := and ( "or" and )*
//! and     := not ( "and" not )*
//! not     := "not" not | primary
//! primary := "(" expr ")" | comparison
//! comparison := operand op operand
//! op      := "==" | "!=" | ">" | ">=" | "<" | "<=" | "in" | "contains"
//! operand := literal | property
//! property:= "$" scope "." name        （scope = record / user / ctx）
//! literal := 字符串 / 数字 / 布尔 / 数组
//! ```
//!
//! # 判定链
//!
//! ```text
//! 解析 → 白名单校验 → 编译成「SQL 片段 + 绑定值」
//! ```
//!
//! 编译产物（[`CompiledCondition`]）交给 `platform-db` 的
//! `TenantQuery::with_condition` —— **业务模块拿不到 SQL 字符串拼接的能力**。

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};

/// 一个可参与绑定的字面值。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind", content = "value")]
pub enum ConditionValue {
    /// 文本。
    Text(String),
    /// 64 位整数。
    Int(i64),
    /// 布尔。
    Bool(bool),
    /// 双精度浮点。
    Float(f64),
}

impl ConditionValue {
    /// 字面量的类型。
    pub fn kind(&self) -> ValueKind {
        match self {
            Self::Text(_) => ValueKind::Text,
            Self::Int(_) | Self::Float(_) => ValueKind::Number,
            Self::Bool(_) => ValueKind::Bool,
        }
    }
}

/// 字面量的类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueKind {
    /// 文本。
    Text,
    /// 数值。
    Number,
    /// 布尔。
    Bool,
}

/// 字段的绑定方式。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldBinding {
    /// 业务表的一列（列名必须是安全 SQL 标识符）。
    Column {
        /// 列名。
        name: String,
        /// 列的类型。
        kind: ValueKind,
    },
    /// 当前用户 ID（值走参数绑定，不进 SQL 文本）。
    UserId,
    /// 当前租户 ID（值走参数绑定，不进 SQL 文本）。
    TenantId,
}

/// 字段白名单：属性路径 → 绑定方式。
///
/// **不在白名单里的属性一律拒绝解析** —— 这是「越界写法」的第一道闸。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FieldWhitelist {
    entries: BTreeMap<String, FieldBinding>,
}

impl FieldWhitelist {
    /// 空白名单（什么都不允许）。
    pub fn empty() -> Self {
        Self::default()
    }

    /// 加入一个绑定到业务表列的字段。
    pub fn with_column(mut self, path: &str, column: &str, kind: ValueKind) -> Self {
        self.entries.insert(
            path.to_string(),
            FieldBinding::Column {
                name: column.to_string(),
                kind,
            },
        );
        self
    }

    /// 加入一个绑定到当前用户 ID 的字段。
    pub fn with_user_id(mut self, path: &str) -> Self {
        self.entries.insert(path.to_string(), FieldBinding::UserId);
        self
    }

    /// 加入一个绑定到当前租户 ID 的字段。
    pub fn with_tenant_id(mut self, path: &str) -> Self {
        self.entries
            .insert(path.to_string(), FieldBinding::TenantId);
        self
    }

    /// 查一个属性的绑定方式。
    pub fn get(&self, path: &str) -> Option<&FieldBinding> {
        self.entries.get(path)
    }

    /// 白名单里的属性路径（已排序）。
    pub fn paths(&self) -> Vec<&str> {
        self.entries.keys().map(String::as_str).collect()
    }

    /// 是否允许该属性。
    pub fn allows(&self, path: &str) -> bool {
        self.entries.contains_key(path)
    }
}

/// 表达式求值 / 编译出错。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExprError {
    /// 出错原因（面向开发者的**英文 ASCII** 描述）。
    pub reason: String,
}

impl ExprError {
    /// 构造一条错误。
    pub fn new(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
        }
    }
}

impl fmt::Display for ExprError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.reason)
    }
}

impl std::error::Error for ExprError {}

/// 表达式的一棵语法树。
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    /// 比较。
    Comparison {
        /// 左操作数。
        left: Operand,
        /// 运算符。
        op: Op,
        /// 右操作数。
        right: Operand,
    },
    /// 与。
    And(Box<Expr>, Box<Expr>),
    /// 或。
    Or(Box<Expr>, Box<Expr>),
    /// 非。
    Not(Box<Expr>),
}

/// 一个操作数。
#[derive(Debug, Clone, PartialEq)]
pub enum Operand {
    /// 属性引用（如 `$record.amount`）。
    Property {
        /// 作用域（`record` / `user` / `ctx`）。
        scope: String,
        /// 字段名。
        field: String,
    },
    /// 字面值。
    Literal(ConditionValue),
    /// 字面值数组（只用于 `in`）。
    Array(Vec<ConditionValue>),
}

impl Operand {
    /// 属性的完整路径（如 `record.amount`）。
    pub fn property_path(&self) -> Option<String> {
        match self {
            Self::Property { scope, field } => Some(format!("{scope}.{field}")),
            _ => None,
        }
    }
}

/// 比较运算符。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// 相等。
    Eq,
    /// 不等。
    Ne,
    /// 大于。
    Gt,
    /// 大于等于。
    Ge,
    /// 小于。
    Lt,
    /// 小于等于。
    Le,
    /// 属于集合。
    In,
    /// 包含子串。
    Contains,
}

impl Op {
    /// 运算符的 SQL 文本。
    pub fn sql(&self) -> &'static str {
        match self {
            Self::Eq => "=",
            Self::Ne => "<>",
            Self::Gt => ">",
            Self::Ge => ">=",
            Self::Lt => "<",
            Self::Le => "<=",
            Self::In => "IN",
            Self::Contains => "LIKE",
        }
    }

    /// 是否是顺序比较（只允许数值）。
    fn is_ordering(&self) -> bool {
        matches!(self, Self::Gt | Self::Ge | Self::Lt | Self::Le)
    }
}

/// 编译产物：**SQL 片段 + 绑定值**。
///
/// SQL 片段里**只有占位符 `?`** —— 字面值一律在 `values` 里，由
/// `platform-db` 的 `TenantQuery` 改写成 `$n` 并绑定。
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledCondition {
    /// SQL 片段（占位符是 `?`）。
    pub sql: String,
    /// 绑定值（与 `sql` 里的 `?` 一一对应，顺序一致）。
    pub values: Vec<ConditionValue>,
}

impl CompiledCondition {
    /// 占位符个数。
    pub fn placeholder_count(&self) -> usize {
        self.sql.matches('?').count()
    }
}

/// 解析上下文：白名单 + 当前用户 / 租户。
#[derive(Debug, Clone)]
pub struct EvalContext<'a> {
    /// 字段白名单。
    pub whitelist: &'a FieldWhitelist,
    /// 当前用户 ID（`$user.id` 用）。
    pub user_id: Option<&'a str>,
    /// 当前租户 ID（`$ctx.tenant_id` 用）。
    pub tenant_id: Option<&'a str>,
}

/// 解析一段表达式（**不**编译）。
///
/// 解析阶段就拒绝越界写法：白名单外的字段、函数调用、多语句。
pub fn parse(source: &str) -> Result<Expr, ExprError> {
    let tokens = tokenize(source)?;
    let mut parser = Parser { tokens, pos: 0 };
    let expr = parser.parse_expr()?;
    parser.expect_end()?;
    Ok(expr)
}

/// 解析并编译成可执行条件。
pub fn compile(source: &str, ctx: &EvalContext<'_>) -> Result<CompiledCondition, ExprError> {
    let expr = parse(source)?;
    compile_expr(&expr, ctx)
}

/// 编译一棵语法树。
fn compile_expr(expr: &Expr, ctx: &EvalContext<'_>) -> Result<CompiledCondition, ExprError> {
    match expr {
        Expr::Comparison { left, op, right } => compile_comparison(left, *op, right, ctx),
        Expr::And(left, right) => {
            let left = compile_expr(left, ctx)?;
            let right = compile_expr(right, ctx)?;
            Ok(CompiledCondition {
                sql: format!("({} AND {})", left.sql, right.sql),
                values: [left.values, right.values].concat(),
            })
        }
        Expr::Or(left, right) => {
            let left = compile_expr(left, ctx)?;
            let right = compile_expr(right, ctx)?;
            Ok(CompiledCondition {
                sql: format!("({} OR {})", left.sql, right.sql),
                values: [left.values, right.values].concat(),
            })
        }
        Expr::Not(inner) => {
            let inner = compile_expr(inner, ctx)?;
            Ok(CompiledCondition {
                sql: format!("NOT ({})", inner.sql),
                values: inner.values,
            })
        }
    }
}

/// 编译一个比较式。
fn compile_comparison(
    left: &Operand,
    op: Op,
    right: &Operand,
    ctx: &EvalContext<'_>,
) -> Result<CompiledCondition, ExprError> {
    // `in` 的右操作数必须是数组。
    if matches!(op, Op::In) && !matches!(right, Operand::Array(_)) {
        return Err(ExprError::new(
            "the `in` operator requires an array on the right-hand side",
        ));
    }

    let left_sql = operand_sql(left, ctx)?;
    let right_sql = operand_sql(right, ctx)?;

    // 顺序比较只允许数值 —— `text > text` 在 PostgreSQL 里没有定义。
    if op.is_ordering() {
        let left_kind = operand_kind(left, ctx)?;
        let right_kind = operand_kind(right, ctx)?;
        if left_kind != ValueKind::Number || right_kind != ValueKind::Number {
            return Err(ExprError::new(
                "the ordering operators require numeric operands on both sides",
            ));
        }
    }

    let mut values = operand_values(left, ctx)?;
    values.extend(operand_values(right, ctx)?);

    Ok(CompiledCondition {
        sql: format!("{left_sql} {} {right_sql}", op.sql()),
        values,
    })
}

/// 操作数的 SQL 片段。
fn operand_sql(operand: &Operand, ctx: &EvalContext<'_>) -> Result<String, ExprError> {
    match operand {
        Operand::Property { .. } => {
            let path = operand.property_path().expect("属性操作数一定有路径");
            let binding = ctx.whitelist.get(&path).ok_or_else(|| {
                ExprError::new(format!(
                    "the attribute `{path}` is not in the whitelist (allowed: {})",
                    ctx.whitelist.paths().join(", ")
                ))
            })?;
            match binding {
                FieldBinding::Column { name, .. } => Ok(column_fragment(name)?),
                FieldBinding::UserId | FieldBinding::TenantId => Ok("?".to_string()),
            }
        }
        Operand::Literal(_) => Ok("?".to_string()),
        Operand::Array(items) => {
            if items.is_empty() {
                return Err(ExprError::new(
                    "the `in` operator requires a non-empty array",
                ));
            }
            Ok(format!("({})", vec!["?"; items.len()].join(", ")))
        }
    }
}

/// 操作数的绑定值。
fn operand_values(
    operand: &Operand,
    ctx: &EvalContext<'_>,
) -> Result<Vec<ConditionValue>, ExprError> {
    match operand {
        Operand::Property { scope, field } => {
            let path = format!("{scope}.{field}");
            let binding = ctx.whitelist.get(&path).ok_or_else(|| {
                ExprError::new(format!("the attribute `{path}` is not in the whitelist"))
            })?;
            match binding {
                FieldBinding::Column { .. } => Ok(Vec::new()),
                FieldBinding::UserId => Ok(vec![ConditionValue::Text(
                    ctx.user_id
                        .ok_or_else(|| {
                            ExprError::new("the attribute `user.id` requires an authenticated user")
                        })?
                        .to_string(),
                )]),
                FieldBinding::TenantId => Ok(vec![ConditionValue::Text(
                    ctx.tenant_id
                        .ok_or_else(|| {
                            ExprError::new(
                                "the attribute `ctx.tenant_id` requires a tenant context",
                            )
                        })?
                        .to_string(),
                )]),
            }
        }
        Operand::Literal(value) => Ok(vec![value.clone()]),
        Operand::Array(items) => Ok(items.clone()),
    }
}

/// 操作数的类型。
fn operand_kind(operand: &Operand, ctx: &EvalContext<'_>) -> Result<ValueKind, ExprError> {
    match operand {
        Operand::Literal(value) => Ok(value.kind()),
        Operand::Array(_) => Err(ExprError::new(
            "an array cannot be used with an ordering operator",
        )),
        Operand::Property { scope, field } => {
            let path = format!("{scope}.{field}");
            let binding = ctx.whitelist.get(&path).ok_or_else(|| {
                ExprError::new(format!("the attribute `{path}` is not in the whitelist"))
            })?;
            match binding {
                FieldBinding::Column { kind, .. } => Ok(*kind),
                FieldBinding::UserId | FieldBinding::TenantId => Ok(ValueKind::Text),
            }
        }
    }
}

/// 校验一个列名是安全的 SQL 标识符（**不合法就拒绝**，不拼进 SQL）。
fn column_fragment(name: &str) -> Result<String, ExprError> {
    let mut chars = name.chars();
    let valid = match chars.next() {
        Some(first) if first.is_ascii_lowercase() => {
            chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        }
        _ => false,
    };
    if !valid {
        return Err(ExprError::new(format!(
            "the column name `{name}` is not a safe SQL identifier"
        )));
    }
    Ok(name.to_string())
}

// ---------------------------------------------------------------------------
// 词法
// ---------------------------------------------------------------------------

/// 一个词法单元。
#[derive(Debug, Clone, PartialEq)]
enum Token {
    /// 标识符（`and` / `or` / `not` / `in` / `contains` / 字段名）。
    Ident(String),
    /// 属性引用（`$record.amount`）。
    Property(String),
    /// 整数。
    Int(i64),
    /// 浮点数。
    Float(f64),
    /// 字符串。
    Str(String),
    /// 布尔。
    Bool(bool),
    /// 运算符。
    Op(Op),
    /// 左括号。
    LParen,
    /// 右括号。
    RParen,
    /// 左方括号。
    LBracket,
    /// 右方括号。
    RBracket,
    /// 逗号。
    Comma,
}

/// 词法单元的位置（出错时定位用）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Span {
    start: usize,
    end: usize,
}

/// 带位置的词法单元。
#[derive(Debug, Clone, PartialEq)]
struct SpannedToken {
    token: Token,
    span: Span,
}

/// 词法分析。
fn tokenize(source: &str) -> Result<Vec<SpannedToken>, ExprError> {
    let chars: Vec<char> = source.chars().collect();
    let mut tokens = Vec::new();
    let mut index = 0usize;

    while index < chars.len() {
        let ch = chars[index];
        if ch.is_whitespace() {
            index += 1;
            continue;
        }
        let start = index;

        // 多语句：`;` 一律拒绝。
        if ch == ';' {
            return Err(ExprError::new(
                "multiple statements are not allowed in a condition expression",
            ));
        }

        match ch {
            '(' => {
                tokens.push(SpannedToken {
                    token: Token::LParen,
                    span: Span {
                        start,
                        end: index + 1,
                    },
                });
                index += 1;
            }
            ')' => {
                tokens.push(SpannedToken {
                    token: Token::RParen,
                    span: Span {
                        start,
                        end: index + 1,
                    },
                });
                index += 1;
            }
            '[' => {
                tokens.push(SpannedToken {
                    token: Token::LBracket,
                    span: Span {
                        start,
                        end: index + 1,
                    },
                });
                index += 1;
            }
            ']' => {
                tokens.push(SpannedToken {
                    token: Token::RBracket,
                    span: Span {
                        start,
                        end: index + 1,
                    },
                });
                index += 1;
            }
            ',' => {
                tokens.push(SpannedToken {
                    token: Token::Comma,
                    span: Span {
                        start,
                        end: index + 1,
                    },
                });
                index += 1;
            }
            '=' | '!' | '<' | '>' => {
                if chars.get(index + 1) == Some(&'=') {
                    let op = match ch {
                        '=' => Op::Eq,
                        '!' => Op::Ne,
                        '<' => Op::Le,
                        '>' => Op::Ge,
                        _ => unreachable!(),
                    };
                    tokens.push(SpannedToken {
                        token: Token::Op(op),
                        span: Span {
                            start,
                            end: index + 2,
                        },
                    });
                    index += 2;
                } else {
                    let op = match ch {
                        '=' => {
                            return Err(ExprError::new(
                                "the `=` operator is not supported; use `==`",
                            ));
                        }
                        '!' => Op::Ne,
                        '<' => Op::Lt,
                        '>' => Op::Gt,
                        _ => unreachable!(),
                    };
                    tokens.push(SpannedToken {
                        token: Token::Op(op),
                        span: Span {
                            start,
                            end: index + 1,
                        },
                    });
                    index += 1;
                }
            }
            '"' | '\'' => {
                let quote = ch;
                index += 1;
                let mut literal = String::new();
                let mut closed = false;
                while index < chars.len() {
                    let inner = chars[index];
                    if inner == '\\' {
                        if let Some(escaped) = chars.get(index + 1) {
                            literal.push(*escaped);
                            index += 2;
                            continue;
                        }
                    }
                    if inner == quote {
                        closed = true;
                        index += 1;
                        break;
                    }
                    literal.push(inner);
                    index += 1;
                }
                if !closed {
                    return Err(ExprError::new("the string literal is not terminated"));
                }
                tokens.push(SpannedToken {
                    token: Token::Str(literal),
                    span: Span { start, end: index },
                });
            }
            '$' => {
                index += 1;
                let scope = read_ident(&chars, &mut index);
                if scope.is_empty() {
                    return Err(ExprError::new(
                        "an attribute reference must be written as `$scope.field`",
                    ));
                }
                if chars.get(index) != Some(&'.') {
                    return Err(ExprError::new(
                        "an attribute reference must be written as `$scope.field`",
                    ));
                }
                index += 1;
                let field = read_ident(&chars, &mut index);
                if field.is_empty() {
                    return Err(ExprError::new(
                        "an attribute reference must be written as `$scope.field`",
                    ));
                }
                tokens.push(SpannedToken {
                    token: Token::Property(format!("{scope}.{field}")),
                    span: Span { start, end: index },
                });
            }
            '0'..='9' => {
                let (value, is_float) = read_number(&chars, &mut index);
                tokens.push(SpannedToken {
                    token: if is_float {
                        Token::Float(value)
                    } else {
                        Token::Int(value as i64)
                    },
                    span: Span { start, end: index },
                });
            }
            _ if ch.is_alphabetic() || ch == '_' => {
                let ident = read_ident(&chars, &mut index);
                // 函数调用：`name(` —— 一律拒绝。
                if chars.get(index) == Some(&'(') {
                    return Err(ExprError::new(format!(
                        "function calls are not allowed in a condition expression (`{ident}`)"
                    )));
                }
                match ident.as_str() {
                    "and" | "or" | "not" | "in" | "contains" => {
                        tokens.push(SpannedToken {
                            token: Token::Ident(ident),
                            span: Span { start, end: index },
                        });
                    }
                    "true" => tokens.push(SpannedToken {
                        token: Token::Bool(true),
                        span: Span { start, end: index },
                    }),
                    "false" => tokens.push(SpannedToken {
                        token: Token::Bool(false),
                        span: Span { start, end: index },
                    }),
                    _ => {
                        return Err(ExprError::new(format!(
                            "unexpected identifier `{ident}`; attribute references must be written as `$scope.field`"
                        )));
                    }
                }
            }
            _ => {
                return Err(ExprError::new(format!(
                    "unexpected character `{ch}` in the condition expression"
                )));
            }
        }
    }

    Ok(tokens)
}

/// 读一个标识符。
fn read_ident(chars: &[char], index: &mut usize) -> String {
    let start = *index;
    while *index < chars.len() {
        let ch = chars[*index];
        if ch.is_alphanumeric() || ch == '_' || ch == '-' {
            *index += 1;
        } else {
            break;
        }
    }
    chars[start..*index].iter().collect()
}

/// 读一个数字。
fn read_number(chars: &[char], index: &mut usize) -> (f64, bool) {
    let start = *index;
    let mut is_float = false;
    while *index < chars.len() {
        let ch = chars[*index];
        if ch.is_ascii_digit() {
            *index += 1;
        } else if ch == '.' && !is_float {
            is_float = true;
            *index += 1;
        } else {
            break;
        }
    }
    let text: String = chars[start..*index].iter().collect();
    (text.parse::<f64>().unwrap_or_default(), is_float)
}

// ---------------------------------------------------------------------------
// 语法
// ---------------------------------------------------------------------------

/// 递归下降解析器。
struct Parser {
    tokens: Vec<SpannedToken>,
    pos: usize,
}

impl Parser {
    /// 解析整个表达式（`or` 层）。
    fn parse_expr(&mut self) -> Result<Expr, ExprError> {
        self.parse_or()
    }

    /// `or` 层。
    fn parse_or(&mut self) -> Result<Expr, ExprError> {
        let mut left = self.parse_and()?;
        while self.peek_keyword("or") {
            self.advance();
            let right = self.parse_and()?;
            left = Expr::Or(Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    /// `and` 层。
    fn parse_and(&mut self) -> Result<Expr, ExprError> {
        let mut left = self.parse_not()?;
        while self.peek_keyword("and") {
            self.advance();
            let right = self.parse_not()?;
            left = Expr::And(Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    /// `not` 层。
    fn parse_not(&mut self) -> Result<Expr, ExprError> {
        if self.peek_keyword("not") {
            self.advance();
            let inner = self.parse_not()?;
            return Ok(Expr::Not(Box::new(inner)));
        }
        self.parse_primary()
    }

    /// 基本项：括号或比较式。
    fn parse_primary(&mut self) -> Result<Expr, ExprError> {
        if self.peek(&Token::LParen) {
            self.advance();
            let inner = self.parse_expr()?;
            self.expect(&Token::RParen, "expected `)`")?;
            return Ok(inner);
        }
        let left = self.parse_operand()?;
        let op = self.parse_op()?;
        let right = self.parse_operand()?;
        Ok(Expr::Comparison { left, op, right })
    }

    /// 解析运算符。
    fn parse_op(&mut self) -> Result<Op, ExprError> {
        let Some(token) = self.tokens.get(self.pos) else {
            return Err(ExprError::new("the condition expression is incomplete"));
        };
        match &token.token {
            Token::Op(op) => {
                let op = *op;
                self.advance();
                Ok(op)
            }
            Token::Ident(name) if name == "in" => {
                self.advance();
                Ok(Op::In)
            }
            Token::Ident(name) if name == "contains" => {
                self.advance();
                Ok(Op::Contains)
            }
            _ => Err(ExprError::new(
                "expected a comparison operator (`==`, `!=`, `>`, `>=`, `<`, `<=`, `in`, `contains`)",
            )),
        }
    }

    /// 解析操作数。
    fn parse_operand(&mut self) -> Result<Operand, ExprError> {
        let Some(token) = self.tokens.get(self.pos) else {
            return Err(ExprError::new("the condition expression is incomplete"));
        };
        let operand = match &token.token {
            Token::Property(path) => {
                let (scope, field) = path.split_once('.').ok_or_else(|| {
                    ExprError::new("an attribute reference must be `$scope.field`")
                })?;
                Operand::Property {
                    scope: scope.to_string(),
                    field: field.to_string(),
                }
            }
            Token::Str(text) => Operand::Literal(ConditionValue::Text(text.clone())),
            Token::Int(value) => Operand::Literal(ConditionValue::Int(*value)),
            Token::Float(value) => Operand::Literal(ConditionValue::Float(*value)),
            Token::Bool(value) => Operand::Literal(ConditionValue::Bool(*value)),
            Token::LBracket => {
                self.advance();
                let mut items = Vec::new();
                if !self.peek(&Token::RBracket) {
                    loop {
                        match self.parse_operand()? {
                            Operand::Literal(value) => items.push(value),
                            _ => {
                                return Err(ExprError::new(
                                    "an array may only contain literal values",
                                ));
                            }
                        }
                        if self.peek(&Token::Comma) {
                            self.advance();
                            continue;
                        }
                        break;
                    }
                }
                self.expect(&Token::RBracket, "expected `]`")?;
                Operand::Array(items)
            }
            _ => {
                return Err(ExprError::new(
                    "expected an attribute reference (`$scope.field`) or a literal value",
                ));
            }
        };
        self.advance();
        Ok(operand)
    }

    /// 下一个词法单元是不是指定的关键字。
    fn peek_keyword(&self, keyword: &str) -> bool {
        matches!(self.tokens.get(self.pos), Some(token) if token.token == Token::Ident(keyword.to_string()))
    }

    /// 下一个词法单元是不是指定的符号。
    fn peek(&self, expected: &Token) -> bool {
        matches!(self.tokens.get(self.pos), Some(token) if &token.token == expected)
    }

    /// 消费当前词法单元。
    fn advance(&mut self) {
        self.pos += 1;
    }

    /// 期望一个符号。
    fn expect(&mut self, expected: &Token, message: &str) -> Result<(), ExprError> {
        if self.peek(expected) {
            self.advance();
            Ok(())
        } else {
            Err(ExprError::new(message.to_string()))
        }
    }

    /// 表达式必须到此为止（多语句在这里被拒）。
    fn expect_end(&self) -> Result<(), ExprError> {
        if self.pos < self.tokens.len() {
            return Err(ExprError::new(
                "unexpected trailing tokens in the condition expression",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn whitelist() -> FieldWhitelist {
        FieldWhitelist::empty()
            .with_column("record.amount", "amount", ValueKind::Number)
            .with_column("record.dept_id", "dept_id", ValueKind::Text)
            .with_column("record.secret", "secret", ValueKind::Text)
            .with_user_id("user.id")
            .with_tenant_id("ctx.tenant_id")
    }

    fn ctx<'a>(wl: &'a FieldWhitelist) -> EvalContext<'a> {
        EvalContext {
            whitelist: wl,
            user_id: Some("u-1001"),
            tenant_id: Some("tenant-a"),
        }
    }

    #[test]
    fn a_valid_comparison_compiles_to_a_placeholder() {
        let wl = whitelist();
        let compiled = compile("$record.amount <= 10000", &ctx(&wl)).expect("应当能编译");
        assert_eq!(compiled.sql, "amount <= ?");
        assert_eq!(compiled.values, vec![ConditionValue::Int(10000)]);
        assert_eq!(compiled.placeholder_count(), 1);
        // **字面值不得出现在 SQL 文本里**。
        assert!(!compiled.sql.contains("10000"));
    }

    #[test]
    fn a_field_outside_the_whitelist_is_rejected() {
        let wl = FieldWhitelist::empty().with_column("record.amount", "amount", ValueKind::Number);
        let error = compile("$record.secret == \"x\"", &ctx(&wl)).expect_err("白名单外必须拒绝");
        assert!(
            error.reason.contains("not in the whitelist"),
            "错误原因要说清是白名单问题：{}",
            error.reason
        );
    }

    #[test]
    fn function_calls_are_rejected() {
        let wl = whitelist();
        let error = compile("now() < $record.amount", &ctx(&wl)).expect_err("函数调用必须拒绝");
        assert!(
            error.reason.contains("function calls are not allowed"),
            "错误原因要说清是函数调用：{}",
            error.reason
        );
    }

    #[test]
    fn multiple_statements_are_rejected() {
        let wl = whitelist();
        let error = compile("$record.amount <= 10000; DROP TABLE t", &ctx(&wl))
            .expect_err("多语句必须拒绝");
        assert!(
            error.reason.contains("multiple statements"),
            "错误原因要说清是多语句：{}",
            error.reason
        );
    }

    #[test]
    fn an_injection_string_is_compiled_as_a_plain_value() {
        let wl = whitelist();
        let compiled = compile("$record.dept_id == \"a' OR '1'='1\"", &ctx(&wl))
            .expect("注入串应当当作普通字符串值编译");
        assert_eq!(compiled.sql, "dept_id = ?");
        assert!(
            !compiled.sql.contains("OR"),
            "SQL 文本里不得出现注入串：{}",
            compiled.sql
        );
        assert_eq!(
            compiled.values,
            vec![ConditionValue::Text("a' OR '1'='1".to_string())]
        );
    }

    #[test]
    fn ordering_operators_reject_text_operands() {
        let wl = whitelist();
        let error = compile("$record.dept_id > 10", &ctx(&wl)).expect_err("文本与数值比较必须拒绝");
        assert!(
            error.reason.contains("numeric operands"),
            "错误原因要说清是类型问题：{}",
            error.reason
        );
    }

    #[test]
    fn boolean_composition_is_preserved() {
        let wl = whitelist();
        let compiled = compile(
            "$record.amount <= 10000 and $record.dept_id == \"dept-a\"",
            &ctx(&wl),
        )
        .expect("应当能编译");
        assert_eq!(compiled.sql, "(amount <= ? AND dept_id = ?)");
        assert_eq!(compiled.values.len(), 2);
    }

    #[test]
    fn user_and_tenant_properties_are_bound_not_inlined() {
        let wl = whitelist();
        let compiled = compile("$user.id == $ctx.tenant_id", &ctx(&wl)).expect("应当能编译");
        assert_eq!(compiled.sql, "? = ?");
        assert_eq!(
            compiled.values,
            vec![
                ConditionValue::Text("u-1001".to_string()),
                ConditionValue::Text("tenant-a".to_string())
            ]
        );
        assert!(!compiled.sql.contains("u-1001"));
        assert!(!compiled.sql.contains("tenant-a"));
    }

    #[test]
    fn the_in_operator_expands_to_a_placeholder_list() {
        let wl = whitelist();
        let compiled =
            compile("$record.dept_id in [\"dept-a\", \"dept-b\"]", &ctx(&wl)).expect("应当能编译");
        assert_eq!(compiled.sql, "dept_id IN (?, ?)");
        assert_eq!(compiled.values.len(), 2);
    }

    #[test]
    fn an_empty_array_is_rejected() {
        let wl = whitelist();
        let error = compile("$record.dept_id in []", &ctx(&wl)).expect_err("空数组必须拒绝");
        assert!(error.reason.contains("non-empty array"));
    }

    #[test]
    fn a_bare_identifier_is_rejected() {
        let wl = whitelist();
        let error = compile("amount <= 10000", &ctx(&wl)).expect_err("裸字段名必须拒绝");
        assert!(error.reason.contains("unexpected identifier"));
    }

    #[test]
    fn an_unterminated_string_is_rejected() {
        let wl = whitelist();
        let error =
            compile("$record.dept_id == \"abc", &ctx(&wl)).expect_err("未闭合字符串必须拒绝");
        assert!(error.reason.contains("not terminated"));
    }

    #[test]
    fn an_unsafe_column_name_is_rejected() {
        let wl =
            FieldWhitelist::empty().with_column("record.x", "x; DROP TABLE t", ValueKind::Text);
        let error = compile("$record.x == \"a\"", &ctx(&wl)).expect_err("非法列名必须拒绝");
        assert!(
            error.reason.contains("not a safe SQL identifier"),
            "错误原因要说清是标识符问题：{}",
            error.reason
        );
    }
}
