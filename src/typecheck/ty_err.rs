#[derive(Clone, Debug)]
pub(crate) enum TyError {
    ArityMismatch,
    TypeMismatch,
    UnknownConstructor,
    UnknownType,
}

pub(crate) type TyChkResult<T> = Result<T, TyError>;
