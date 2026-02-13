use std::fmt;

#[derive(Debug)]
pub enum SsukkaError {
    /// HTML rewriting error from lol_html
    Rewrite(String),
    /// CSS parsing/transform error
    Css(String),
    /// I/O error
    Io(std::io::Error),
    /// Configuration error
    Config(String),
}

impl fmt::Display for SsukkaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SsukkaError::Rewrite(msg) => write!(f, "HTML rewrite error: {msg}"),
            SsukkaError::Css(msg) => write!(f, "CSS error: {msg}"),
            SsukkaError::Io(err) => write!(f, "I/O error: {err}"),
            SsukkaError::Config(msg) => write!(f, "config error: {msg}"),
        }
    }
}

impl std::error::Error for SsukkaError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            SsukkaError::Io(err) => Some(err),
            _ => None,
        }
    }
}

impl From<std::io::Error> for SsukkaError {
    fn from(err: std::io::Error) -> Self {
        SsukkaError::Io(err)
    }
}

impl From<lol_html::errors::RewritingError> for SsukkaError {
    fn from(err: lol_html::errors::RewritingError) -> Self {
        SsukkaError::Rewrite(err.to_string())
    }
}

pub type Result<T> = std::result::Result<T, SsukkaError>;
