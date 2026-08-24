use crate::builtin::values::*;
use crate::typecheck::env_v_var_to_ty_scheme::*;
use crate::typecheck::ty_expr::*;
use crate::typecheck::ty_scheme::*;
use crate::typecheck::v_expr::*;
use crate::typecheck::v_expr_typed::*;
use crate::typecheck::v_var_name::*;
use crate::typecheck::v_var_name_supply::*;

use std::collections::*;
use std::ops::Deref;

/// desugar typed expression by eliminating literal range patterns in case expressions
pub(crate) fn desugar_literal_range_pattern(
    ns: &mut VVarNameSupply,
    env_v_var_to_ty_scheme: &mut EnvVVarToTyScheme,
    expr: &TypedVExpr,
) -> TypedVExpr {
    match expr {
        TypedVExpr::Abstraction(abstr) => {
            desugar_literal_range_pattern_abstr(ns, env_v_var_to_ty_scheme, abstr)
        }
        TypedVExpr::Application(app) => {
            desugar_literal_range_pattern_app(ns, env_v_var_to_ty_scheme, app)
        }
        TypedVExpr::Case(case_expr) => {
            desugar_literal_range_pattern_case(ns, env_v_var_to_ty_scheme, case_expr)
        }
        TypedVExpr::Let(let_expr) => {
            desugar_literal_range_pattern_let_expr(ns, env_v_var_to_ty_scheme, let_expr)
        }
        TypedVExpr::LitNumeric(_) | TypedVExpr::LitString(_) | TypedVExpr::Variable(_) => {
            expr.clone()
        }
        TypedVExpr::Constructor(constructor) => {
            desugar_literal_range_pattern_constructor(ns, env_v_var_to_ty_scheme, constructor)
        }
    }
}

// note: don't support literal range pattern in parameter pattern
pub(crate) fn desugar_literal_range_pattern_abstr(
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
        body: Box::new(desugar_literal_range_pattern(
            ns,
            env_v_var_to_ty_scheme,
            body,
        )),
        ty: ty.clone(),
    })
}

pub(crate) fn desugar_literal_range_pattern_app(
    ns: &mut VVarNameSupply,
    env_v_var_to_ty_scheme: &mut EnvVVarToTyScheme,
    expr: &TypedVAppExpr,
) -> TypedVExpr {
    let TypedVAppExpr { callable, args, ty } = expr;
    TypedVExpr::Application(TypedVAppExpr {
        callable: Box::new(desugar_literal_range_pattern(
            ns,
            env_v_var_to_ty_scheme,
            callable,
        )),
        args: args
            .iter()
            .map(|x| desugar_literal_range_pattern(ns, env_v_var_to_ty_scheme, x))
            .collect(),
        ty: ty.clone(),
    })
}

pub(crate) fn desugar_literal_range_pattern_case(
    ns: &mut VVarNameSupply,
    env_v_var_to_ty_scheme: &mut EnvVVarToTyScheme,
    expr: &TypedVCaseExpr,
) -> TypedVExpr {
    let TypedVCaseExpr { arg, clauses, ty } = expr;

    let arg = Box::new(desugar_literal_range_pattern(
        ns,
        env_v_var_to_ty_scheme,
        arg,
    ));

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
                    TypedVPattern::Range { start, end, ty } => {
                        let simple_binder_var = {
                            let vvar_gen = ns.generate();
                            ns.uniqify(&vvar_gen)
                        };
                        let pattern = TypedVPattern::Variable {
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
                                ty_expr: Box::new(ty.clone()),
                            },
                        });

                        let expr_cmp_start = match start {
                            RangeBound::Inclusive(VPatternLiteral::Numeric(s)) => {
                                mk_builtin_typed_vexpr_ge(
                                    env_v_var_to_ty_scheme,
                                    &binder_typed_expr,
                                    &mk_typed_vexpr_from_v_lit_numeric(s),
                                )
                            }
                            RangeBound::Exclusive(VPatternLiteral::Numeric(s)) => {
                                mk_builtin_typed_vexpr_gt(
                                    env_v_var_to_ty_scheme,
                                    &binder_typed_expr,
                                    &mk_typed_vexpr_from_v_lit_numeric(s),
                                )
                            }
                            _ => todo!("literal range pattern only works for numerical types"),
                        };
                        let expr_cmp_end = match &end {
                            RangeBound::Inclusive(VPatternLiteral::Numeric(s)) => {
                                mk_builtin_typed_vexpr_le(
                                    env_v_var_to_ty_scheme,
                                    &binder_typed_expr,
                                    &mk_typed_vexpr_from_v_lit_numeric(s),
                                )
                            }
                            RangeBound::Exclusive(VPatternLiteral::Numeric(s)) => {
                                mk_builtin_typed_vexpr_lt(
                                    env_v_var_to_ty_scheme,
                                    &binder_typed_expr,
                                    &mk_typed_vexpr_from_v_lit_numeric(s),
                                )
                            }
                            _ => todo!("literal range pattern only works for numerical types"),
                        };

                        let expr_predicate = mk_builtin_typed_vexpr_logical_and(
                            env_v_var_to_ty_scheme,
                            &expr_cmp_start,
                            &expr_cmp_end,
                        );

                        let guard = match guard {
                            Some(g) => Some(mk_builtin_typed_vexpr_logical_and(
                                env_v_var_to_ty_scheme,
                                &g,
                                &expr_predicate,
                            )),
                            _ => Some(expr_predicate),
                        };
                        (pattern, guard)
                    }
                    _ => (pattern.clone(), guard.clone()),
                };

                let body = desugar_literal_range_pattern(ns, env_v_var_to_ty_scheme, body);
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

// note: don't support literal range patterns in LHS of let defs, but
// recursively desugar for RHS of defs
pub(crate) fn desugar_literal_range_pattern_let_expr(
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
                    desugar_literal_range_pattern(ns, env_v_var_to_ty_scheme, rhs_expr),
                )
            })
            .collect(),

        body: Box::new(desugar_literal_range_pattern(
            ns,
            env_v_var_to_ty_scheme,
            body,
        )),
        ty: ty.clone(),
    })
}

pub(crate) fn desugar_literal_range_pattern_constructor(
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
            .map(|x| desugar_literal_range_pattern(ns, env_v_var_to_ty_scheme, x))
            .collect(),
        record_fields: record_fields.clone(),
        ty: ty.clone(),
        ty_args: ty_args.clone(),
    })
}
