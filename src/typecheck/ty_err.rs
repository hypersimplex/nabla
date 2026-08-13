#[derive(Debug, Clone)]
pub(crate) enum TyError {
    UnknownConstructor {
        ty_name: Option<String>,
        constructor: String,
    },
    AmbiguousConstructor {
        constructor: String,
        candidates: Vec<String>,
    },
    UnexpectedSyntax(String),
    UnexpectedPattern(String),
    UnexpectedField(String),
    UnexpectedExpr(String),
    UnknownType(String),
    UnboundVariable(String),
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
    PatBinderUniqueness(String), // TODO: move this else where
    InternalError(String),
}

pub(crate) type TyChkResult<T> = Result<T, TyError>;
