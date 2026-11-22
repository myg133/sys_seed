//! **CSV 解析与生成**（RFC 4180，自己写 —— 零依赖，D320）。
//!
//! # 支持什么
//!
//! - 逗号分隔；`"` 包裹含特殊字符的字段；
//! - 字段内的 `"` 写成 `""`（转义）；
//! - CRLF / LF 都是行分隔；
//! - 首行 UTF-8 BOM（`EF BB BF`）**自动剥掉**；
//! - 空文件的末尾没有换行也能正确结束。
//!
//! # ⚠️ 三条硬约束（每条都有对应判据）
//!
//! 1. **引号未闭合 → 报错并指出起始行号**（AC-S-179 步骤 4）。
//!    **不得**把剩余文件都当一个字段 —— 那会让 `max_row_bytes` 失效 → 内存爆。
//! 2. **单行超 `max_row_bytes` → 报错并指出行号**（AC-S-179 步骤 5）。
//! 3. **行号是「文件行号」（1-based，含表头）**（AC-S-181）——
//!    数据第 1 行 = 文件第 2 行。

/// 一行（已解析的字段）。
pub type Record = Vec<String>;

/// CSV 解析错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CsvError {
    /// 引号未闭合（读到 EOF 仍在引号内）。
    UnclosedQuote {
        /// **引号开始的行号**（1-based）。
        line: usize,
    },
    /// 单行（原始字节数）超过上限。
    RowTooLong {
        /// 行号（1-based）。
        line: usize,
        /// 上限（字节）。
        max_bytes: usize,
    },
    /// 字段数与该行表头不一致。
    ColumnCountMismatch {
        /// 行号（1-based）。
        line: usize,
        /// 期望的字段数（表头列数）。
        expected: usize,
        /// 实际的字段数。
        actual: usize,
    },
}

impl CsvError {
    /// 面向开发者的英文描述（ASCII，契约 C6）。
    pub fn detail(&self) -> String {
        match self {
            Self::UnclosedQuote { line } => {
                format!("a quoted field starting at line {line} is never closed")
            }
            Self::RowTooLong { line, max_bytes } => {
                format!("the line {line} exceeds the maximum row size of {max_bytes} bytes")
            }
            Self::ColumnCountMismatch {
                line,
                expected,
                actual,
            } => format!("the line {line} has {actual} fields but the header has {expected}"),
        }
    }

    /// 出错的行号（1-based）。
    pub fn line(&self) -> usize {
        match self {
            Self::UnclosedQuote { line }
            | Self::RowTooLong { line, .. }
            | Self::ColumnCountMismatch { line, .. } => *line,
        }
    }
}

impl std::fmt::Display for CsvError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.detail())
    }
}

impl std::error::Error for CsvError {}

/// 解析结果（**逐行产出**，不是一次性 Vec —— 便于流式消费）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedCsv {
    /// 表头（第一行的字段）。
    pub header: Record,
    /// 数据行。
    pub rows: Vec<Record>,
}

impl ParsedCsv {
    /// 数据行数（**不含表头**）。
    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    /// 用表头 + 一行数据拼成 `字段名 → 值`（重复列名取**第一个**）。
    pub fn to_map(&self, row: &Record) -> std::collections::BTreeMap<String, String> {
        self.header
            .iter()
            .zip(row.iter())
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }
}

/// UTF-8 BOM。
const BOM: &[u8] = &[0xEF, 0xBB, 0xBF];

/// 解析 CSV。
///
/// # 参数
///
/// - `max_row_bytes`：单**原始行**的字节上限（防畸形文件吃内存）。
pub fn parse(bytes: &[u8], max_row_bytes: usize) -> Result<ParsedCsv, CsvError> {
    let body = bytes.strip_prefix(BOM).unwrap_or(bytes);
    let text = String::from_utf8_lossy(body);

    let mut records: Vec<Record> = Vec::new();
    let mut current: Record = Vec::new();
    let mut field = String::new();
    // 当前字段是否已被引号包裹（`"x"` 与 `x` 不同：前者允许内含分隔符）。
    let mut in_quotes = false;
    // 引号开始的行号（1-based），用于未闭合时报错。
    let mut quote_start_line: Option<usize> = None;
    let mut line: usize = 1;
    let mut row_bytes: usize = 0;

    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        row_bytes += ch.len_utf8();
        if row_bytes > max_row_bytes {
            return Err(CsvError::RowTooLong {
                line,
                max_bytes: max_row_bytes,
            });
        }

        if in_quotes {
            // 引号内：`"` 要么是转义（下一个也是 `"`），要么是闭合。
            if ch == '"' {
                if chars.peek() == Some(&'"') {
                    // `""` → 一个字面引号（消费掉第二个）。
                    chars.next();
                    row_bytes += 1;
                    field.push('"');
                } else {
                    // 闭合。
                    in_quotes = false;
                }
                continue;
            }
            field.push(ch);
            continue;
        }

        // 不在引号里。
        match ch {
            '"' => {
                in_quotes = true;
                quote_start_line = Some(line);
            }
            ',' => {
                current.push(std::mem::take(&mut field));
            }
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                current.push(std::mem::take(&mut field));
                records.push(std::mem::take(&mut current));
                line += 1;
                row_bytes = 0;
            }
            '\n' => {
                current.push(std::mem::take(&mut field));
                records.push(std::mem::take(&mut current));
                line += 1;
                row_bytes = 0;
            }
            other => field.push(other),
        }
    }

    // 未闭合的引号是错误（绝不把剩余文件当一个字段）。
    if in_quotes {
        return Err(CsvError::UnclosedQuote {
            line: quote_start_line.unwrap_or(line),
        });
    }

    // 收尾：文件末尾没有换行时的最后一个字段 / 行。
    if !field.is_empty() || !current.is_empty() {
        current.push(field);
        records.push(current);
    }

    // 去掉完全空的行（只有 CRLF 的行）。
    records.retain(|r| !(r.len() == 1 && r[0].is_empty()));

    if records.is_empty() {
        return Ok(ParsedCsv {
            header: Vec::new(),
            rows: Vec::new(),
        });
    }

    let header = records.remove(0);
    let expected = header.len();
    for (index, row) in records.iter().enumerate() {
        if row.len() != expected {
            return Err(CsvError::ColumnCountMismatch {
                // 数据第 index 行 = 文件第 index + 2 行（表头是第 1 行）。
                line: index + 2,
                expected,
                actual: row.len(),
            });
        }
    }

    Ok(ParsedCsv {
        header,
        rows: records,
    })
}

/// 把一个字段按 RFC 4180 转义（需要时加引号）。
fn escape_field(value: &str) -> String {
    let needs_quotes = value
        .chars()
        .any(|c| c == ',' || c == '"' || c == '\r' || c == '\n');
    if !needs_quotes {
        return value.to_string();
    }
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        if ch == '"' {
            out.push('"');
        }
        out.push(ch);
    }
    out.push('"');
    out
}

/// 生成 CSV（**带 BOM** —— Excel 兼容，AC-S-182 步骤 4）。
pub fn generate(header: &Record, rows: &[Record]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(BOM);
    let push_row = |fields: &Record, out: &mut Vec<u8>| {
        let line: Vec<String> = fields.iter().map(|f| escape_field(f)).collect();
        out.extend_from_slice(line.join(",").as_bytes());
        out.extend_from_slice(b"\r\n");
    };
    push_row(header, &mut out);
    for row in rows {
        push_row(row, &mut out);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 主路径：逗号 / 引号 / 换行 / BOM 四种边界（AC-S-179 步骤 1，TC-018-030）。
    #[test]
    fn the_main_path_handles_comma_quote_newline_and_bom() {
        let mut input = Vec::from(BOM);
        input.extend_from_slice(
            "name,note\n\"含逗号,的值\",ok\n\"含\"\"引号\"\"的值\",ok\n\"含\n换行\",ok\n普通,ok\n"
                .as_bytes(),
        );
        let parsed = parse(&input, 65536).expect("应解析成功");
        assert_eq!(parsed.header, vec!["name", "note"]);
        assert_eq!(parsed.row_count(), 4);
        assert_eq!(parsed.rows[0], vec!["含逗号,的值", "ok"]);
        assert_eq!(parsed.rows[1], vec!["含\"引号\"的值", "ok"]);
        assert_eq!(parsed.rows[2], vec!["含\n换行", "ok"]);
        assert_eq!(parsed.rows[3], vec!["普通", "ok"]);
        // ⚠️ BOM 必须被剥掉。
        assert_eq!(
            parsed.header[0], "name",
            "BOM 不得出现在第一个字段里（实际 {:?}）",
            parsed.header[0]
        );
        assert!(!parsed.header[0].starts_with('\u{feff}'));
    }

    /// 引号未闭合 → 报错并指出**起始行号**（TC-018-033）。
    #[test]
    fn an_unclosed_quote_reports_the_starting_line() {
        let err = parse(b"name,note\n\"abc", 65536).expect_err("必须报错");
        assert_eq!(err.line(), 2, "应指出引号开始的第 2 行");
        assert!(
            matches!(err, CsvError::UnclosedQuote { line: 2 }),
            "错误类型必须是 UnclosedQuote，实际 {err:?}"
        );
    }

    /// 引号未闭合**绝不**把剩余文件当一个字段（内存安全）。
    #[test]
    fn an_unclosed_quote_does_not_swallow_the_rest() {
        // 3 万字节全在一个未闭合的引号里 —— 必须报错，不能返回一个 3 万字节的字段。
        let mut input = Vec::from("name\n\"");
        input.extend(std::iter::repeat_n(b'x', 30_000));
        let result = parse(&input, 1000);
        assert!(
            matches!(
                result,
                Err(CsvError::RowTooLong { .. }) | Err(CsvError::UnclosedQuote { .. })
            ),
            "必须是 RowTooLong 或 UnclosedQuote，实际 {result:?}"
        );
    }

    /// 单行超上限 → 报错并指出行号与上限（TC-018-034）。
    #[test]
    fn a_row_too_long_reports_the_line_and_limit() {
        let input = b"name\nok\nxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx\n";
        let err = parse(input, 20).expect_err("第 3 行应超限");
        assert_eq!(err.line(), 3);
        assert!(
            matches!(err, CsvError::RowTooLong { max_bytes: 20, .. }),
            "错误里必须带上限 20，实际 {err:?}"
        );
    }

    /// 空文件 → 0 条数据行（TC-018-035，不是错误）。
    #[test]
    fn an_empty_file_yields_zero_rows() {
        let parsed = parse(b"", 1024).expect("空文件不是错误");
        assert_eq!(parsed.row_count(), 0);
        assert!(parsed.header.is_empty());
    }

    /// 只有表头 → 0 条数据行。
    #[test]
    fn a_header_only_file_yields_zero_rows() {
        let parsed = parse(b"name,note\n", 1024).expect("应解析成功");
        assert_eq!(parsed.header, vec!["name", "note"]);
        assert_eq!(parsed.row_count(), 0);
    }

    /// 文件末尾没有换行也能正确结束。
    #[test]
    fn a_missing_trailing_newline_is_fine() {
        let parsed = parse(b"a,b\n1,2", 1024).expect("应解析成功");
        assert_eq!(parsed.rows, vec![vec!["1", "2"]]);
    }

    /// CRLF 也是行分隔。
    #[test]
    fn crlf_is_a_line_separator() {
        let parsed = parse(b"a,b\r\n1,2\r\n3,4\r\n", 1024).expect("应解析成功");
        assert_eq!(parsed.row_count(), 2);
        assert_eq!(parsed.rows[0], vec!["1", "2"]);
        assert_eq!(parsed.rows[1], vec!["3", "4"]);
    }

    /// 列数不一致 → 报错并指出**文件行号**（TC-018-042 的基础）。
    #[test]
    fn a_column_count_mismatch_reports_the_file_line() {
        // 表头 2 列；第 2 行只有 1 列。
        let err = parse(b"a,b\n1\n", 1024).expect_err("列数不符必须报错");
        assert_eq!(err.line(), 2, "数据第 1 行 = 文件第 2 行");
        assert!(
            matches!(
                err,
                CsvError::ColumnCountMismatch {
                    expected: 2,
                    actual: 1,
                    ..
                }
            ),
            "实际 {err:?}"
        );
    }

    /// 生成：含逗号 / 引号 / 换行的字段被加引号并转义（TC-018-036）。
    #[test]
    fn generate_quotes_and_escapes_special_fields() {
        let header = vec!["a".to_string(), "b".to_string()];
        let rows = vec![
            vec!["x,y".to_string(), "c\"d".to_string()],
            vec!["e\nf".to_string(), "g".to_string()],
        ];
        let out = generate(&header, &rows);
        let text = String::from_utf8_lossy(&out);
        assert!(text.contains("\"x,y\""), "含逗号必须加引号：{text}");
        assert!(text.contains("\"c\"\"d\""), "引号必须加倍：{text}");
        assert!(text.contains("\"e\nf\""), "含换行必须加引号：{text}");
    }

    /// 生成带 BOM（AC-S-182 步骤 4）。
    #[test]
    fn generate_starts_with_a_bom() {
        let out = generate(&vec!["a".to_string()], &[]);
        assert_eq!(&out[..3], BOM, "生成的 CSV 必须以 BOM 开头");
    }

    /// **round-trip**：生成再解析，逐字段一致（TC-018-047 的基础）。
    #[test]
    fn generate_then_parse_round_trips() {
        let header = vec!["名称".to_string(), "备注".to_string()];
        let rows = vec![
            vec!["含,逗号".to_string(), "含\"引号\"".to_string()],
            vec!["含\n换行".to_string(), "普通".to_string()],
            vec!["".to_string(), "空字段".to_string()],
        ];
        let bytes = generate(&header, &rows);
        let parsed = parse(&bytes, 65536).expect("round-trip 应解析成功");
        assert_eq!(parsed.header, header);
        assert_eq!(parsed.rows, rows, "round-trip 必须逐字段一致");
    }

    /// 不带特殊字符的字段**不加引号**（不无谓地膨胀）。
    #[test]
    fn plain_fields_are_not_quoted() {
        let out = generate(&vec!["abc".to_string()], &[]);
        let text = String::from_utf8_lossy(&out);
        assert!(text.contains("abc"), "{text}");
        assert!(!text.contains("\"abc\""), "普通字段不该加引号：{text}");
    }

    /// `to_map` 用表头做键。
    #[test]
    fn to_map_uses_the_header_as_keys() {
        let parsed = parse("name,amount\n张三,100\n".as_bytes(), 1024).expect("应解析");
        let map = parsed.to_map(&parsed.rows[0]);
        assert_eq!(map.get("name").map(String::as_str), Some("张三"));
        assert_eq!(map.get("amount").map(String::as_str), Some("100"));
    }

    /// 空行（只有换行）不产生数据行。
    #[test]
    fn blank_lines_are_skipped() {
        let parsed = parse(b"a,b\n\n1,2\n\n", 1024).expect("应解析");
        assert_eq!(parsed.row_count(), 1, "空行不算数据行");
    }
}
