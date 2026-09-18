use crate::typecheck::ty_expr::TyExpr;
use crate::typecheck::ty_scheme::TyScheme;
use crate::typecheck::v_expr::VVar;
use crate::typecheck::v_expr_typed::*;
use crate::typecheck::v_var_name_supply::VVarNameSupply;

/// transforms wildcards in constructor patterns to fresh variable binders
///
/// this is mainly to prepare for conversion to Core IR and ensures that no
/// constructor pattern contains wildcards
pub(crate) fn normalize_case_constructor_wildcard(
    ns: &mut VVarNameSupply,
    expr: &TypedVExpr,
) -> TypedVExpr {
    match expr {
        TypedVExpr::Abstraction(abstraction) => TypedVExpr::Abstraction(TypedVAbstrExpr {
            body: Box::new(normalize_case_constructor_wildcard(ns, &abstraction.body)),
            ..abstraction.clone()
        }),
        TypedVExpr::Application(app) => TypedVExpr::Application(TypedVAppExpr {
            callable: Box::new(normalize_case_constructor_wildcard(ns, &app.callable)),
            args: app
                .args
                .iter()
                .map(|arg| normalize_case_constructor_wildcard(ns, arg))
                .collect(),
            ty: app.ty.clone(),
        }),
        TypedVExpr::Case(case_expr) => {
            let arg = normalize_case_constructor_wildcard(ns, &case_expr.arg);
            let alts = case_expr
                .alts
                .iter()
                .map(|alt| TypedVCaseAlt {
                    pattern: normalize_pattern(ns, &alt.pattern),
                    guard: alt
                        .guard
                        .as_ref()
                        .map(|g| normalize_case_constructor_wildcard(ns, g)),
                    body: normalize_case_constructor_wildcard(ns, &alt.body),
                })
                .collect();
            TypedVExpr::Case(TypedVCaseExpr {
                arg: Box::new(arg),
                alts,
                ty: case_expr.ty.clone(),
            })
        }
        TypedVExpr::Constructor(constructor) => TypedVExpr::Constructor(TypedVConstructorExpr {
            args: constructor
                .args
                .iter()
                .map(|arg| normalize_case_constructor_wildcard(ns, arg))
                .collect(),
            ..constructor.clone()
        }),
        TypedVExpr::Let(let_expr) => TypedVExpr::Let(TypedVLetExpr {
            defs: let_expr
                .defs
                .iter()
                .map(|(pat, rhs)| {
                    (
                        normalize_pattern(ns, pat),
                        normalize_case_constructor_wildcard(ns, rhs),
                    )
                })
                .collect(),
            body: Box::new(normalize_case_constructor_wildcard(ns, &let_expr.body)),
            ty: let_expr.ty.clone(),
        }),
        other => other.clone(),
    }
}

/// any wildcard argument is replaced with a fresh variable binder
fn normalize_pattern(ns: &mut VVarNameSupply, pat: &TypedVPattern) -> TypedVPattern {
    match pat {
        TypedVPattern::Constructor {
            ty_name,
            constructor,
            args,
            ty,
            ty_args,
        } => {
            let new_args = args
                .iter()
                .map(|arg| match arg {
                    TypedVPattern::Wild { ty } => {
                        let var = ns.generate();
                        mk_var_pat(&var, ty)
                    }
                    nested => normalize_pattern(ns, nested),
                })
                .collect();
            TypedVPattern::Constructor {
                ty_name: ty_name.clone(),
                constructor: constructor.clone(),
                args: new_args,
                ty: ty.clone(),
                ty_args: ty_args.clone(),
            }
        }
        other => other.clone(),
    }
}

fn mk_var_pat(var: &VVar, ty: &TyExpr) -> TypedVPattern {
    TypedVPattern::Variable {
        binder: var.clone(),
        ty: ty.clone(),
        ty_schematic: TyScheme {
            ty_vars_schematic: vec![],
            ty_expr: Box::new(ty.clone()),
        },
    }
}
