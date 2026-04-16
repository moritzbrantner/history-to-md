use std::error::Error as StdError;
use std::fmt;
use std::io;
use std::num::ParseIntError;
use std::string::FromUtf8Error;

pub type AppResult<T> = Result<T, AppError>;

#[derive(Debug)]
pub enum AppError {
    Message(String),
    Io {
        context: String,
        source: io::Error,
    },
    Json {
        context: String,
        source: serde_json::Error,
    },
    Utf8 {
        context: String,
        source: FromUtf8Error,
    },
    ParseInt {
        context: String,
        source: ParseIntError,
    },
}

impl AppError {
    pub fn message(message: impl Into<String>) -> Self {
        Self::Message(message.into())
    }

    pub fn io(context: impl Into<String>, source: io::Error) -> Self {
        Self::Io {
            context: context.into(),
            source,
        }
    }

    pub fn json(context: impl Into<String>, source: serde_json::Error) -> Self {
        Self::Json {
            context: context.into(),
            source,
        }
    }

    pub fn utf8(context: impl Into<String>, source: FromUtf8Error) -> Self {
        Self::Utf8 {
            context: context.into(),
            source,
        }
    }

    pub fn parse_int(context: impl Into<String>, source: ParseIntError) -> Self {
        Self::ParseInt {
            context: context.into(),
            source,
        }
    }
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Message(message) => f.write_str(message),
            Self::Io { context, source } => write!(f, "{context}: {source}"),
            Self::Json { context, source } => write!(f, "{context}: {source}"),
            Self::Utf8 { context, source } => write!(f, "{context}: {source}"),
            Self::ParseInt { context, source } => write!(f, "{context}: {source}"),
        }
    }
}

impl StdError for AppError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Self::Message(_) => None,
            Self::Io { source, .. } => Some(source),
            Self::Json { source, .. } => Some(source),
            Self::Utf8 { source, .. } => Some(source),
            Self::ParseInt { source, .. } => Some(source),
        }
    }
}
