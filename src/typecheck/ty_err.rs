use crate::typecheck::ty_expr::TyExpr;
use crate::typecheck::ty_var_name::TyVarName;

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
    Unexpected(String),
    UnknownType(String),
    UnboundVariable(String),
    ArityMismatch {
        constructor: String,
        expected: usize,
        got: usize,
        msg: Option<String>,
    },
    TypeConflict {
        ty1: TyExpr,
        ty2: TyExpr,
        msg: Option<String>,
    },
    InfiniteType {
        var: TyVarName,
        ty: TyExpr,
        msg: Option<String>,
    },
    AdtError(String),
    PatBinderUniqueness(String), // TODO: move this else where
    InternalError(String),
}

pub(crate) type TyChkResult<T> = Result<T, TyError>;
