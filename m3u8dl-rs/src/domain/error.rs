use thiserror::Error;

/// Top-level download error. All public adapter / application fns return `Result<T, DownloadError>`.
///
/// HTTP layer maps variants to status codes:
/// - `Network`        → 502
/// - `Parse`          → 400
/// - `Decrypt`        → 422
/// - `Ffmpeg`         → 500
/// - `Io`             → 500
/// - `Unsupported`    → 422 (with link to docs/SCOPE.md)
/// - `Cancelled`      → 499
#[derive(Debug, Error)]
pub enum DownloadError {
    #[error("network: {0}")]
    Network(#[from] reqwest::Error),

    #[error("parse: {0}")]
    Parse(String),

    #[error("decrypt: {0}")]
    Decrypt(String),

    #[error("ffmpeg exit={code}: {stderr}")]
    Ffmpeg { code: i32, stderr: String },

    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    #[error("unsupported: {feature} — see docs/SCOPE.md")]
    Unsupported { feature: String },

    #[error("cancelled")]
    Cancelled,
}

pub type Result<T> = std::result::Result<T, DownloadError>;
