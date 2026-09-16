#[derive(Debug, Clone)]
pub(crate) enum CoreError {
    AdtError(String),
}

pub(crate) type CoreResult<T> = Result<T, CoreError>;
