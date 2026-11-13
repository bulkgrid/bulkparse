use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("unsupported or ambiguous format: {0}")]
    Unsupported(String),
    #[error("format requires the '{0}' Cargo feature")]
    FeatureDisabled(&'static str),
    #[error("malformed document: {0}")]
    Malformed(String),
    #[error("resource limit exceeded: {0}")]
    Limit(&'static str),
    #[error("conversion cancelled")]
    Cancelled,
    #[error("encrypted documents are not supported")]
    Encrypted,
    #[error("strict conversion rejected incomplete content ({0} diagnostics)")]
    Incomplete(usize),
    #[error("I/O: {0}")]
    Io(#[from] std::io::Error),
}

impl Error {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Unsupported(_) => "unsupported",
            Self::FeatureDisabled(_) => "feature_disabled",
            Self::Malformed(_) => "malformed",
            Self::Limit(_) => "resource_limit",
            Self::Cancelled => "cancelled",
            Self::Encrypted => "encrypted",
            Self::Incomplete(_) => "incomplete",
            Self::Io(_) => "io",
        }
    }
}
