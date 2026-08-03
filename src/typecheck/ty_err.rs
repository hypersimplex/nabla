#[derive(Debug, Clone)]
pub(crate) enum TyError {
    UnknownConstructor {
        ty_name: Option<String>,
        constructor: String,
    },
    AmbiguousConstructor {
        constructor: String,
        candidates: Vec<String>,
        hint: String,
    },
    UnknownType(String),
    ArityMismatch {
        constructor: String,
        expected: usize,
        got: usize,
    },
    TypeMismatch {
        expected: String,
        got: String,
    },
    TypeConflict(String),
    AdtError(String),
    InternalError(String),
}

pub(crate) type TyChkResult<T> = Result<T, TyError>;
