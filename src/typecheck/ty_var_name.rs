use crate::parse::concrete_token;
use crate::parse::loc;

/// type-level variable used for inferencing types
///
/// when present in a type scheme's parameter list, this instantiates to a
/// fresh new type variable at use site, otherwise it is copied
#[derive(Clone, Debug, PartialOrd, Ord, PartialEq, Eq)]
pub(crate) enum TyVarName {
    UserDefined(TyVarNameUserDefined),
    Builtin(TyVarNameBuiltin), // basic type supported by the compiler
    Auto(u64),                 // auto-generated unique name
}

#[derive(Clone, Debug, PartialOrd, Ord, PartialEq, Eq)]
pub(crate) struct TyVarNameUserDefined {
    pub token: concrete_token::ConcreteToken,

    // maybe None if compiler creates this
    pub loc: Option<loc::Location>,
}

#[derive(Clone, Debug, PartialOrd, Ord, PartialEq, Eq)]
pub(crate) enum TyVarNameBuiltin {
    I64,
    F64,
    String,
    Bool,
    Unit,
    Arrow, // internally maps to a type scheme
}
