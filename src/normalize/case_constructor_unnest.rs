use crate::typecheck::ty_con_env::TyConEnv;
use crate::typecheck::ty_expr::TyExpr;
use crate::typecheck::ty_scheme::TyScheme;
use crate::typecheck::v_expr::VVar;
use crate::typecheck::v_expr_typed::*;
use crate::typecheck::v_var_name_supply::VVarNameSupply;

use std::collections::VecDeque;

/// transforms constructor patterns in case alternatives so that:
/// - nested non-variable patterns in constructor arguments are flattened out by
///   converting into nested case expressions and the original nested patterns
///   are replaced with simple variable patterns
///
/// eg:
/// ```text
/// case y of
///   Just(Just x) -> 0
///   Nothing -> 1
/// ```
/// converts to:
/// ```text
/// case y of
///   Just(var) -> case var of
///                  Just x -> 0
///                  _      -> case y of
///                              Nothing -> 1
///   Nothing -> 1
/// ```
pub(crate) fn normalize_case_constructor_unnest(
    ns: &mut VVarNameSupply,
    ty_env: &TyConEnv,
    expr: &TypedVExpr,
) -> TypedVExpr {
    match expr {
        TypedVExpr::Abstraction(abstraction) => TypedVExpr::Abstraction(TypedVAbstrExpr {
            body: Box::new(normalize_case_constructor_unnest(
                ns,
                ty_env,
                &abstraction.body,
            )),
            ..abstraction.clone()
        }),
        TypedVExpr::Application(app) => TypedVExpr::Application(TypedVAppExpr {
            callable: Box::new(normalize_case_constructor_unnest(ns, ty_env, &app.callable)),
            args: app
                .args
                .iter()
                .map(|arg| normalize_case_constructor_unnest(ns, ty_env, arg))
                .collect(),
            ty: app.ty.clone(),
        }),
        TypedVExpr::Case(case_expr) => normalize_case_expr(ns, ty_env, case_expr),
        TypedVExpr::Constructor(constructor) => TypedVExpr::Constructor(TypedVConstructorExpr {
            args: constructor
                .args
                .iter()
                .map(|arg| normalize_case_constructor_unnest(ns, ty_env, arg))
                .collect(),
            ..constructor.clone()
        }),
        TypedVExpr::Let(let_expr) => TypedVExpr::Let(TypedVLetExpr {
            defs: let_expr
                .defs
                .iter()
                .map(|(pat, rhs)| {
                    (
                        pat.clone(),
                        normalize_case_constructor_unnest(ns, ty_env, rhs),
                    )
                })
                .collect(),
            body: Box::new(normalize_case_constructor_unnest(
                ns,
                ty_env,
                &let_expr.body,
            )),
            ty: let_expr.ty.clone(),
        }),
        other => other.clone(),
    }
}

fn normalize_case_expr(
    ns: &mut VVarNameSupply,
    ty_env: &TyConEnv,
    case_expr: &TypedVCaseExpr,
) -> TypedVExpr {
    let scrutinee = normalize_case_constructor_unnest(ns, ty_env, &case_expr.arg);
    let ty = &case_expr.ty;

    // accumulate from reverse direction
    let mut tail_alts: VecDeque<TypedVCaseAlt> = VecDeque::new();

    // note: reversed direction of iteration
    for alt in case_expr.alts.iter().rev() {
        let body_normalized = normalize_case_constructor_unnest(ns, ty_env, &alt.body);

        let guard_normalized = alt
            .guard
            .as_ref()
            .map(|g| normalize_case_constructor_unnest(ns, ty_env, g));

        let fallback_alternatives: Vec<_> = tail_alts.iter().cloned().collect();

        let norm_alt = normalize_case_alt(
            ns,
            ty_env,
            &alt.pattern,
            guard_normalized,
            body_normalized,
            &scrutinee,
            &fallback_alternatives,
            ty,
        );

        tail_alts.push_front(norm_alt);
    }

    TypedVExpr::Case(TypedVCaseExpr {
        arg: Box::new(scrutinee),
        alts: tail_alts.into_iter().collect(),
        ty: ty.clone(),
    })
}

/// normalize a single case alternative:
/// - every constructor argument that is a nested pattern is replaced with a
///   fresh variable binder and introduces a new case expression with:
///   1. the newly introduced fresh variable binder as its scrutinee, and
///   2. the original nested pattern put into an alternative along with the
///      original body, and
///   3. if the pattern in (2) is refutable or a guard is present, then
///      introduce a catch-all alternative to the case expression, where the
///      body contains an inner case expression with `alts_fallback`
fn normalize_case_alt(
    ns: &mut VVarNameSupply,
    ty_env: &TyConEnv,
    pattern: &TypedVPattern,
    guard_normalized: Option<TypedVExpr>,
    body_normalized: TypedVExpr,
    scrutinee: &TypedVExpr,
    alts_fallback: &[TypedVCaseAlt],
    outer_ty: &TyExpr,
) -> TypedVCaseAlt {
    match pattern {
        TypedVPattern::Constructor {
            ty_name,
            constructor,
            args,
            ty: pat_ty,
            ty_args,
        } => {
            // if there is no nested non-variable pattern, then we don't need to
            // do anything
            if args
                .iter()
                .all(|arg| matches!(arg, TypedVPattern::Variable { .. }))
            {
                return TypedVCaseAlt {
                    pattern: pattern.clone(),
                    guard: guard_normalized,
                    body: body_normalized,
                };
            }

            // there exists at least one nested non-variable pattern

            let mut new_args: Vec<TypedVPattern> = Vec::new();
            let mut nested_unpackings: Vec<(VVar, TyExpr, TypedVPattern)> = Vec::new();

            for arg in args.iter() {
                match arg {
                    TypedVPattern::Variable { .. } => {
                        new_args.push(arg.clone());
                    }
                    nested_pat => {
                        // generate a new variable binder for non-variable pattern

                        let var = ns.generate();
                        let ty = nested_pat.ty();
                        new_args.push(mk_var_pat(&var, ty));
                        nested_unpackings.push((var, ty.clone(), nested_pat.clone()));
                    }
                }
            }

            assert!(!nested_unpackings.is_empty());

            // alternative guard must be attached to the innermost unpacking alternative,
            // where all variable binders from outer and nested patterns are in scope
            let mut guard_for_innermost = guard_normalized;
            let mut current_body = body_normalized;

            // note: reverse iteration order (eg: innermost is processed first) along with guard
            for (var_binder_to_pat, ty_var_binder_to_pat, pat) in
                nested_unpackings.into_iter().rev()
            {
                let guard = guard_for_innermost.take();

                // introduce case expression for the nested non-variable pattern `pat` and
                // its associated `var_binder_to_pat`
                current_body = unpack_nested_argument(
                    ns,
                    ty_env,
                    &var_binder_to_pat,
                    &ty_var_binder_to_pat,
                    &pat,
                    guard,
                    current_body,
                    scrutinee,
                    alts_fallback,
                    outer_ty,
                );
            }

            TypedVCaseAlt {
                pattern: TypedVPattern::Constructor {
                    ty_name: ty_name.clone(),
                    constructor: constructor.clone(),
                    args: new_args,
                    ty: pat_ty.clone(),
                    ty_args: ty_args.clone(),
                },
                guard: None,
                body: current_body,
            }
        }

        // other variants: no change
        _ => TypedVCaseAlt {
            pattern: pattern.clone(),
            guard: guard_normalized,
            body: body_normalized,
        },
    }
}

/// unpacks a single nested argument pattern `pat` associated with
/// variable binder `var_binder_to_pat` by introducing a case expression
///
/// if the alternative associated with `pat` can possibly fail, introduce
/// a catch-all alternative and an inner case expression along with the
/// remaining non-empty alternatives `alts_fallback` for the inner case
/// expression
///
/// wraps `body` in an inner `case` expression:
/// ```text
/// case var_binder_to_pat of
///   pat | g -> body
/// // catch-all alternative below if: pat is refutable or guard exists,
/// // and fallback alt(s) exist
///   _       -> case scrutinee of
///                alts_fallback
/// ```
fn unpack_nested_argument(
    ns: &mut VVarNameSupply,
    ty_env: &TyConEnv,
    var_binder_to_pat: &VVar,
    ty_var_binder_to_pat: &TyExpr,
    pat: &TypedVPattern,
    guard: Option<TypedVExpr>,
    body: TypedVExpr,
    scrutinee: &TypedVExpr,
    alts_fallback: &[TypedVCaseAlt],
    ty_outer: &TyExpr, // type of expression in each body of case expression's alternatives
) -> TypedVExpr {
    // here we do recursion if the nested pattern is a constructor and thus may
    // contain nested pattern in its arguments
    //
    // otherwise, wrap the leaf pattern and guard directly
    let alt_normalized = match pat {
        TypedVPattern::Constructor { .. } => normalize_case_alt(
            ns,
            ty_env,
            pat,
            guard,
            body,
            scrutinee,
            alts_fallback,
            ty_outer,
        ),
        _ => TypedVCaseAlt {
            pattern: pat.clone(),
            guard,
            body,
        },
    };

    let has_guard = alt_normalized.guard.is_some();
    let mut inner_alts = vec![alt_normalized];

    // if the pattern is refutable or a guard is present, and remaining
    // alternatives is not empty, then we add fallback (wildcard/DEFAULT)
    // branch and its associated nested case expression, original scrutinee
    // (for re-evaluation), plus the remaining alternatives to be put in the
    // the nested case expression
    //
    // otherwise, we don't need to consider the remaining alternatives since
    // they are not reachable
    if (has_guard || is_pattern_refutable(ty_env, pat)) && !alts_fallback.is_empty() {
        inner_alts.push(TypedVCaseAlt {
            pattern: TypedVPattern::Wild {
                ty: ty_var_binder_to_pat.clone(),
            },
            guard: None,
            body: TypedVExpr::Case(TypedVCaseExpr {
                arg: Box::new(scrutinee.clone()),
                alts: alts_fallback.to_vec(),
                ty: ty_outer.clone(),
            }),
        });
    }

    // finally return the case expression
    TypedVExpr::Case(TypedVCaseExpr {
        arg: Box::new(mk_var_expr(var_binder_to_pat, ty_var_binder_to_pat)),
        alts: inner_alts,
        ty: ty_outer.clone(),
    })
}

// other helpers --->>

/// determine if a pattern match ever fails at runtime without other knowledge:
/// - sum types (with more than 1 constructor), literals, and ranges are refutable
/// - single constructor product types, variables, and wildcards are irrefutable
fn is_pattern_refutable(ty_env: &TyConEnv, pat: &TypedVPattern) -> bool {
    match pat {
        TypedVPattern::Constructor { ty_name, .. } => ty_env
            .get_adt(ty_name)
            .map_or(true, |adt| adt.constructors.len() > 1),
        TypedVPattern::Literal { .. } | TypedVPattern::Range { .. } => true,
        TypedVPattern::Variable { .. } | TypedVPattern::Wild { .. } => false,
        TypedVPattern::Record { .. } => false,
    }
}

fn mk_var_pat(var: &VVar, ty: &TyExpr) -> TypedVPattern {
    TypedVPattern::Variable {
        binder: var.clone(),
        ty: ty.clone(),
        ty_schematic: TyScheme {
            ty_vars_schematic: Vec::new(),
            ty_expr: Box::new(ty.clone()),
        },
    }
}

fn mk_var_expr(var: &VVar, ty: &TyExpr) -> TypedVExpr {
    TypedVExpr::Variable(TypedVVariable {
        var: var.clone(),
        ty: ty.clone(),
        ty_args: Vec::new(),
        ty_schematic: TyScheme {
            ty_vars_schematic: Vec::new(),
            ty_expr: Box::new(ty.clone()),
        },
    })
}

// <<--- helpers
