#[derive(Debug, Clone)]
pub(crate) enum CoreError {
    ArityMismatch {
        constructor: String,
        expected: usize,
        got: usize,
    },
    TypeConflict(String),
    AdtError(String),
    InternalError(String),
}

pub(crate) type CoreResult<T> = Result<T, CoreError>;
