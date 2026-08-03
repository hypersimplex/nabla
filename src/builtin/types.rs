use crate::typecheck::ty_expr::*;
use crate::typecheck::ty_var_name::*;

// resolve a builtin type name (as it appears in source) to a TyExpr
// Bool maps to the builtin ADT
pub(crate) fn resolve_builtin_type(name: &str) -> Option<TyExpr> {
    match name {
        "i64" => Some(TyExpr::TyVar(TyVarName::Builtin(TyVarNameBuiltin::I64))),
        "f64" => Some(TyExpr::TyVar(TyVarName::Builtin(TyVarNameBuiltin::F64))),
        "String" => Some(TyExpr::TyVar(TyVarName::Builtin(TyVarNameBuiltin::String))),
        "Bool" => Some(build_adt_type_no_loc("Bool", &[])),
        "()" | "Unit" => Some(TyExpr::TyVar(TyVarName::Builtin(TyVarNameBuiltin::Unit))),
        _ => None,
    }
}
