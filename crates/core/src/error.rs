#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Invalid(String),
    #[error("authentication required")]
    Unauthorized,
    #[error("invalid username or password")]
    BadCredentials,
    #[error("forbidden")]
    Forbidden,
    #[error("{0} not found")]
    NotFound(&'static str),
    #[error("{0}")]
    Conflict(&'static str),
    #[error("rate limited")]
    RateLimited { retry_after_secs: u64 },
    #[error("service unavailable")]
    Unavailable,
    #[error("internal error: {0}")]
    Internal(String),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
