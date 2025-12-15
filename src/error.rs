use std::fmt;

/// Custom error type for the print spooler application
#[derive(Debug)]
#[allow(dead_code)]
pub enum SpoolerError {
    /// HTTP/API communication errors
    Api(String),
    /// Network/request errors
    Network(reqwest::Error),
    /// JSON serialization/deserialization errors
    Json(serde_json::Error),
    /// File I/O errors
    Io(std::io::Error),
    /// Print-related errors
    Print(String),
    /// Configuration errors
    Config(String),
}

impl fmt::Display for SpoolerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SpoolerError::Api(msg) => write!(f, "API error: {}", msg),
            SpoolerError::Network(e) => write!(f, "Network error: {}", e),
            SpoolerError::Json(e) => write!(f, "JSON error: {}", e),
            SpoolerError::Io(e) => write!(f, "I/O error: {}", e),
            SpoolerError::Print(msg) => write!(f, "Print error: {}", msg),
            SpoolerError::Config(msg) => write!(f, "Configuration error: {}", msg),
        }
    }
}

impl std::error::Error for SpoolerError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            SpoolerError::Network(e) => Some(e),
            SpoolerError::Json(e) => Some(e),
            SpoolerError::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<reqwest::Error> for SpoolerError {
    fn from(err: reqwest::Error) -> Self {
        SpoolerError::Network(err)
    }
}

impl From<serde_json::Error> for SpoolerError {
    fn from(err: serde_json::Error) -> Self {
        SpoolerError::Json(err)
    }
}

impl From<std::io::Error> for SpoolerError {
    fn from(err: std::io::Error) -> Self {
        SpoolerError::Io(err)
    }
}

impl From<String> for SpoolerError {
    fn from(msg: String) -> Self {
        SpoolerError::Api(msg)
    }
}

impl From<&str> for SpoolerError {
    fn from(msg: &str) -> Self {
        SpoolerError::Api(msg.to_string())
    }
}

/// Result type alias for spooler operations
pub type SpoolerResult<T> = Result<T, SpoolerError>;
