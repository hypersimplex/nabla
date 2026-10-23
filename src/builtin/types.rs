use crate::parse::abstr_structures;
use crate::parse::concrete_token;
use crate::parse::loc;
use crate::typecheck::ty_expr::*;
use crate::typecheck::ty_var_name::*;

/// resolve a builtin primitive type name to its type constructor
pub(crate) fn resolve_builtin_ty_con(name: &str) -> Option<TyConBuiltin> {
    match name {
        "Int64#" => Some(TyConBuiltin::Int64),
        "Float64#" => Some(TyConBuiltin::Float64),
        "Bytes#" => Some(TyConBuiltin::Bytes),
        "->" => Some(TyConBuiltin::Arrow),
        _ => None,
    }
}

/// resolve a builtin type name to a TyExpr
pub(crate) fn resolve_builtin_ty_expr(name: &str) -> Option<TyExpr> {
    resolve_builtin_ty_con(name).map(|b| TyExpr::TyCon(TyCon::Builtin(b)))
}

/// convert type annotation to internal TyExpr, resolving builtin type names
/// and converting function arrows to builtin Arrow
pub(crate) fn lower_type_annot_to_ty_expr(ty_expr: &abstr_structures::ATypeExprComplex) -> TyExpr {
    match ty_expr {
        abstr_structures::ATypeExprComplex::Iden(iden) => {
            let abstr_structures::ATypeExprIden {
                identifier: loc::ConcreteTokenAndLoc { token, loc, .. },
                type_parameters,
            } = &iden;
            let mut head = match token {
                concrete_token::ConcreteToken::Iden(name)
                    if let Some(builtin) = resolve_builtin_ty_expr(name) =>
                {
                    builtin
                }
                _ => TyExpr::TyVar(TyVarName::UserDefined(TyVarNameUserDefined {
                    token: token.clone(),
                    loc: Some(loc.clone()),
                })),
            };
            for param in type_parameters.iter() {
                let arg = lower_type_annot_to_ty_expr(param);
                head = ty_app(head, arg);
            }
            head
        }
        abstr_structures::ATypeExprComplex::Fun(fun) => {
            // convert function types to arrow applications using the builtin arrow
            let head = {
                let g = fun.head.lock().unwrap();
                lower_type_annot_to_ty_expr(&g)
            };
            let tail = match &fun.tail {
                Some(x) => lower_type_annot_to_ty_expr(&x.lock().unwrap()),
                None => unreachable!("parser guarantees function type expression has a tail"),
            };
            mk_ty_arrow(head, tail)
        }
    }
}
