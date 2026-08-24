use crate::builtin::values::*;
use crate::typecheck::env_v_var_to_ty_scheme::*;
use crate::typecheck::ty_expr::*;
use crate::typecheck::ty_scheme::*;
use crate::typecheck::v_expr::*;
use crate::typecheck::v_expr_typed::*;
use crate::typecheck::v_var_name::*;
use crate::typecheck::v_var_name_supply::*;

/// desugar typed expression by eliminating literal range patterns in case expressions
pub(crate) fn desugar_literal_pattern(
    ns: &mut VVarNameSupply,
    env_v_var_to_ty_scheme: &mut EnvVVarToTyScheme,
    expr: &TypedVExpr,
) -> TypedVExpr {
    match expr {
        TypedVExpr::Abstraction(abstr) => {
            desugar_literal_pattern_abstr(ns, env_v_var_to_ty_scheme, abstr)
        }
        TypedVExpr::Application(app) => {
            desugar_literal_pattern_app(ns, env_v_var_to_ty_scheme, app)
        }
        TypedVExpr::Case(case_expr) => {
            desugar_literal_pattern_case(ns, env_v_var_to_ty_scheme, case_expr)
        }
        TypedVExpr::Let(let_expr) => {
            desugar_literal_pattern_let_expr(ns, env_v_var_to_ty_scheme, let_expr)
        }
        TypedVExpr::LitNumeric(_) | TypedVExpr::LitString(_) | TypedVExpr::Variable(_) => {
            expr.clone()
        }
        TypedVExpr::Constructor(constructor) => {
            desugar_literal_pattern_constructor(ns, env_v_var_to_ty_scheme, constructor)
        }
    }
}

pub(crate) fn desugar_literal_pattern_abstr(
    ns: &mut VVarNameSupply,
    env_v_var_to_ty_scheme: &mut EnvVVarToTyScheme,
    expr: &TypedVAbstrExpr,
) -> TypedVExpr {
    let TypedVAbstrExpr {
        name,
        params,
        body,
        ty,
    } = expr;
    TypedVExpr::Abstraction(TypedVAbstrExpr {
        name: name.clone(),
        params: params.clone(),
        body: Box::new(desugar_literal_pattern(ns, env_v_var_to_ty_scheme, body)),
        ty: ty.clone(),
    })
}

pub(crate) fn desugar_literal_pattern_app(
    ns: &mut VVarNameSupply,
    env_v_var_to_ty_scheme: &mut EnvVVarToTyScheme,
    expr: &TypedVAppExpr,
) -> TypedVExpr {
    let TypedVAppExpr { callable, args, ty } = expr;
    TypedVExpr::Application(TypedVAppExpr {
        callable: Box::new(desugar_literal_pattern(
            ns,
            env_v_var_to_ty_scheme,
            callable,
        )),
        args: args
            .iter()
            .map(|x| desugar_literal_pattern(ns, env_v_var_to_ty_scheme, x))
            .collect(),
        ty: ty.clone(),
    })
}

pub(crate) fn desugar_literal_pattern_case(
    ns: &mut VVarNameSupply,
    env_v_var_to_ty_scheme: &mut EnvVVarToTyScheme,
    expr: &TypedVCaseExpr,
) -> TypedVExpr {
    let TypedVCaseExpr { arg, clauses, ty } = expr;

    let arg = Box::new(desugar_literal_pattern(ns, env_v_var_to_ty_scheme, arg));

    TypedVExpr::Case(TypedVCaseExpr {
        arg: arg.clone(),
        clauses: clauses
            .iter()
            .map(|x| {
                let TypedVCaseClause {
                    pattern,
                    guard,
                    body,
                } = x;

                let (pattern, guard) = match &pattern {
                    TypedVPattern::Literal { literal, ty } => {
                        let simple_binder_var = {
                            let vvar_gen = ns.generate();
                            ns.uniqify(&vvar_gen)
                        };
                        let pattern_new = TypedVPattern::Variable {
                            binder: simple_binder_var.clone(),
                            ty: ty.clone(),
                            ty_schematic: TyScheme {
                                ty_vars_schematic: vec![],
                                ty_expr: Box::new(ty.clone()),
                            },
                        };

                        let binder_typed_expr = TypedVExpr::Variable(TypedVVariable {
                            var: simple_binder_var,
                            ty: arg.ty().clone(),
                            ty_args: vec![],
                            ty_schematic: TyScheme {
                                ty_vars_schematic: vec![],
                                ty_expr: Box::new(arg.ty().clone()),
                            },
                        });

                        let expr_predicate = match literal {
                            VPatternLiteral::Numeric(s) => mk_builtin_typed_vexpr_eq(
                                env_v_var_to_ty_scheme,
                                &binder_typed_expr,
                                &mk_typed_vexpr_from_v_lit_numeric(&s),
                            ),
                            VPatternLiteral::String(s) => mk_builtin_typed_vexpr_eq(
                                env_v_var_to_ty_scheme,
                                &binder_typed_expr,
                                &mk_typed_vexpr_from_v_lit_string(&s),
                            ),
                        };

                        let guard_new = match guard {
                            Some(g) => Some(mk_builtin_typed_vexpr_logical_and(
                                env_v_var_to_ty_scheme,
                                &g,
                                &expr_predicate,
                            )),
                            _ => Some(expr_predicate),
                        };
                        (pattern_new, guard_new)
                    }
                    _ => (pattern.clone(), guard.clone()),
                };

                let body = desugar_literal_pattern(ns, env_v_var_to_ty_scheme, body);
                TypedVCaseClause {
                    pattern,
                    guard,
                    body,
                }
            })
            .collect(),
        ty: ty.clone(),
    })
}

pub(crate) fn desugar_literal_pattern_let_expr(
    ns: &mut VVarNameSupply,
    env_v_var_to_ty_scheme: &mut EnvVVarToTyScheme,
    expr: &TypedVLetExpr,
) -> TypedVExpr {
    let TypedVLetExpr { defs, body, ty } = expr;
    TypedVExpr::Let(TypedVLetExpr {
        defs: defs
            .iter()
            .map(|(lhs_pat, rhs_expr)| {
                (
                    lhs_pat.clone(),
                    desugar_literal_pattern(ns, env_v_var_to_ty_scheme, rhs_expr),
                )
            })
            .collect(),
        body: Box::new(desugar_literal_pattern(ns, env_v_var_to_ty_scheme, body)),
        ty: ty.clone(),
    })
}

pub(crate) fn desugar_literal_pattern_constructor(
    ns: &mut VVarNameSupply,
    env_v_var_to_ty_scheme: &mut EnvVVarToTyScheme,
    expr: &TypedVConstructorExpr,
) -> TypedVExpr {
    let TypedVConstructorExpr {
        ty_name,
        constructor_name,
        args,
        record_fields,
        ty,
        ty_args,
    } = expr;
    TypedVExpr::Constructor(TypedVConstructorExpr {
        ty_name: ty_name.clone(),
        constructor_name: constructor_name.clone(),
        args: args
            .iter()
            .map(|x| desugar_literal_pattern(ns, env_v_var_to_ty_scheme, x))
            .collect(),
        record_fields: record_fields.clone(),
        ty: ty.clone(),
        ty_args: ty_args.clone(),
    })
}
