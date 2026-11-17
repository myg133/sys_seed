//! 统一错误类型。
//!
//! 错误码遵循 `api-conventions v1` C6：稳定的机器可读错误码（大写 + 下划线），
//! 面向用户的中文文案由前端 i18n 渲染，后端只返回错误码与参数。

use thiserror::Error;

/// 平台核心统一错误类型。
#[derive(Debug, Error)]
pub enum CoreError {
    /// 配置键不符合命名约定（层级 `.` 分隔，每段 lowerCamelCase）。
    #[error("invalid config key: {key}")]
    InvalidConfigKey { key: String },

    /// 标识符非法（空值或含空白字符）。
    #[error("invalid identifier: {value}")]
    InvalidIdentifier { value: String },

    /// 文件系统或网络读写失败。
    #[error("io error: {source}")]
    Io {
        #[from]
        source: std::io::Error,
    },
}

impl CoreError {
    /// 稳定的机器可读错误码，供前端按错误码分支处理。
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidConfigKey { .. } => "INVALID_CONFIG_KEY",
            Self::InvalidIdentifier { .. } => "INVALID_IDENTIFIER",
            Self::Io { .. } => "IO_ERROR",
        }
    }
}

/// 平台核心统一的返回类型。
pub type CoreResult<T> = Result<T, CoreError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_codes_are_stable_and_upper_snake() {
        let key_error = CoreError::InvalidConfigKey {
            key: "Bad-Key".to_string(),
        };
        let id_error = CoreError::InvalidIdentifier {
            value: " ".to_string(),
        };
        let io_error = CoreError::Io {
            source: std::io::Error::other("boom"),
        };

        assert_eq!(key_error.code(), "INVALID_CONFIG_KEY");
        assert_eq!(id_error.code(), "INVALID_IDENTIFIER");
        assert_eq!(io_error.code(), "IO_ERROR");

        for code in [key_error.code(), id_error.code(), io_error.code()] {
            assert!(
                code.chars().all(|c| c.is_ascii_uppercase() || c == '_'),
                "错误码必须是大写 + 下划线：{code}"
            );
        }
    }

    #[test]
    fn io_error_converts_from_std_io_error() {
        let source = std::io::Error::new(std::io::ErrorKind::NotFound, "missing");
        let error: CoreError = source.into();
        assert_eq!(error.code(), "IO_ERROR");
        assert!(error.to_string().contains("missing"));
    }
}
