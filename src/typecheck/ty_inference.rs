use crate::parse::concrete_token::*;
use crate::typecheck::adt::*;
use crate::typecheck::algos::*;
use crate::typecheck::env_v_var_to_ty_scheme::*;
use crate::typecheck::subst::*;
use crate::typecheck::subst_persistent::*;
use crate::typecheck::ty_env::*;
use crate::typecheck::ty_err::*;
use crate::typecheck::ty_expr::*;
use crate::typecheck::ty_scheme::*;
use crate::typecheck::ty_var_name::*;
use crate::typecheck::ty_var_name_supply::*;
use crate::typecheck::v_expr::*;
use crate::typecheck::v_expr_typed::*;

use std::collections::*;

pub(crate) type Substitution = SubstPersistentIdent;

/// resolved constructor metadata instantiated with fresh type variables
struct ConstructorInstance<'a> {
    ty_name: String,
    ctor_def: &'a ConstructorDef,
    param_subst: Substitution,
    fresh_params: Vec<TyVarName>,
}

struct PatternConstructorInfo<'a> {
    instance: ConstructorInstance<'a>,
    field_types: Vec<TyExpr>,
}

/// compute free (excluding scheme-bound vars) type variables in a value-level
/// variable -> type scheme environment
pub(crate) fn free_tvns_in_ty_env(ty_scheme: &EnvVVarToTyScheme) -> Vec<TyVarName> {
    let mut out: BTreeSet<TyVarName> = BTreeSet::new();
    for scheme in ty_scheme.0.values() {
        let bound: BTreeSet<_> = scheme.ty_vars_schematic.iter().cloned().collect();
        for tvn in free_ty_vars(&scheme.ty_expr) {
            if !bound.contains(&tvn) {
                out.insert(tvn);
            }
        }
    }
    out.into_iter().collect()
}

pub(crate) fn free_ty_vars(ty_expr: &TyExpr) -> BTreeSet<TyVarName> {
    let mut vars: BTreeSet<TyVarName> = BTreeSet::new();
    let mut stack: Vec<&TyExpr> = vec![ty_expr];
    while let Some(current) = stack.pop() {
        match current {
            TyExpr::TyVar(var) if matches!(var, TyVarName::Auto(_) | TyVarName::UserDefined(_)) => {
                vars.insert(var.clone());
            }
            TyExpr::TyApp(app) => {
                stack.push(&app.ty_func);
                stack.push(&app.ty_arg);
            }
            _ => {}
        }
    }
    vars
}

// compute free type variables for the type expression, but ignore ADT type
// constructors; only true type variables should be generalized
pub(crate) fn free_ty_vars_excluding_adts(ty_expr: &TyExpr, ty_env: &TyEnv) -> BTreeSet<TyVarName> {
    free_ty_vars(ty_expr)
        .into_iter()
        .filter(|tvn: &TyVarName| !is_adt_type_var(ty_env, tvn))
        .collect()
}

/// finds a most general unifier, substitution `s`, such that s(t1) = s(t2)
///
/// notes:
///   - auto type variables act as variables in type equations, these can unify
///     with anything
///   - concrete types (Builtin, UserDefined) are rigid, they only unify with
///     same type
///   - TyApp unifies structurally: T1 A ~ T2 B iff T1 ~ T2 and A ~ B,
///     where `~` means `unifies with`
///   - prevents infinite types (handled in `extend`) by occurs check
///
/// type variable classification:
///   - Auto(n): Unification variables - can be substituted with any type
///   - Builtin(I64|String|...): Concrete primitive types - rigid
///   - UserDefined(name): Concrete ADT types - rigid, only unifies with same name
pub(crate) fn unify_ty_exprs(
    subst: &Substitution,
    ty_expr1: &TyExpr,
    ty_expr2: &TyExpr,
) -> Result<Substitution, TyError> {
    match (ty_expr1, ty_expr2) {
        (TyExpr::TyVar(tvn), other) => {
            // apply current substitution to see what the type variable resolves to
            let sub_ty_expr1 = subst.get(tvn).unwrap();
            match &sub_ty_expr1 {
                // if it's still a type variable after substitution
                TyExpr::TyVar(x) => {
                    // type constructors (builtin and user-defined) are rigid/concrete types:
                    //   unify with themselves or with Auto variables
                    match (x, other) {
                        // Builtin vs Builtin: must be the same type
                        (TyVarName::Builtin(b1), TyExpr::TyVar(TyVarName::Builtin(b2)))
                            if b1 != b2 =>
                        {
                            Err(TyError::TypeConflict(
                                format!(
                                    "Cannot unify incompatible builtin types: {:?} and {:?}",
                                    b1, b2
                                )
                                .to_string(),
                            ))
                        }

                        // UserDefined vs UserDefined: must have the same name
                        (TyVarName::UserDefined(u1), TyExpr::TyVar(TyVarName::UserDefined(u2)))
                            if u1.token != u2.token =>
                        {
                            Err(TyError::TypeConflict(
                                format!(
                                    "Cannot unify incompatible user-defined types: {:?} and {:?}",
                                    u1.token, u2.token
                                )
                                .to_string(),
                            ))
                        }

                        // Builtin vs UserDefined: incompatible
                        (TyVarName::Builtin(_), TyExpr::TyVar(TyVarName::UserDefined(_)))
                        | (TyVarName::UserDefined(_), TyExpr::TyVar(TyVarName::Builtin(_))) => {
                            Err(TyError::TypeConflict(
                                format!(
                                    "Cannot unify type {:?} with type {:?}",
                                    ty_expr1, ty_expr2
                                )
                                .to_string(),
                            ))
                        }

                        // Auto vs. Concrete type: substitute the Auto variable
                        (
                            TyVarName::Builtin(_) | TyVarName::UserDefined(_),
                            TyExpr::TyVar(TyVarName::Auto(_)),
                        ) => unify_ty_exprs(subst, ty_expr2, ty_expr1),
                        (
                            TyVarName::Auto(_),
                            TyExpr::TyVar(TyVarName::Builtin(_) | TyVarName::UserDefined(_)),
                        ) => extend(subst, x, other),

                        // same type variable or Auto vs Auto: proceed with standard unification
                        _ => extend(subst, x, other),
                    }
                }
                // type variable resolved to a compound type - recursively unify
                _ => {
                    let sub_ty_expr2 = subst_ty(subst, other);
                    unify_ty_exprs(subst, &sub_ty_expr1, &sub_ty_expr2)
                }
            }
        }
        // symmetry
        (TyExpr::TyApp(_), TyExpr::TyVar(_)) => unify_ty_exprs(subst, ty_expr2, ty_expr1),
        // structural unification for type applications, eg:
        //   T1<A1> ~ T2<A2> <=> T1 ~ T2 and A1 ~ A2
        (TyExpr::TyApp(ty_app1), TyExpr::TyApp(ty_app2)) => {
            // unify the type constructors (function parts)
            let subst_func = unify_ty_exprs(subst, &ty_app1.ty_func, &ty_app2.ty_func)?;
            // unify the type arguments with updated substitution
            unify_ty_exprs(&subst_func, &ty_app1.ty_arg, &ty_app2.ty_arg)
        }
    }
}

/// extend the current substitution subst if possible
fn extend(subst: &Substitution, tvn: &TyVarName, other: &TyExpr) -> Result<Substitution, TyError> {
    match other {
        TyExpr::TyVar(tvn_other) if tvn == tvn_other => Ok(subst.clone()), // success
        _ => {
            if free_ty_vars(other).contains(tvn) {
                return Err(TyError::TypeConflict(
                    format!(
                        "extend: infinite type detected: type variable {:?} present in other TyExpr {:?}",
                        tvn, other
                    )
                    .to_string(),
                ));
            }
            // compose {tvn -> other} in top of `subst`
            Ok(subst_compose(&subst_delta(tvn, other), subst))
        }
    }
}

/// delta substition:
///   tvn => ty_expr
///   o/w => identity
fn subst_delta(tvn: &TyVarName, ty_expr: &TyExpr) -> Substitution {
    let tvn_clone = tvn.clone();

    let ty_expr_clone = ty_expr.clone();

    // just apply logic on top of identity substitution
    subst_id().insert(tvn_clone, ty_expr_clone)
}

/// identity substition
pub(crate) fn subst_id() -> Substitution {
    SubstPersistentIdent::default()
}

/// create a substitution from a map
pub(crate) fn subst_from_map(map: &BTreeMap<TyVarName, TyExpr>) -> Substitution {
    let mut subst = SubstPersistentIdent::default();
    for (k, v) in map.iter() {
        subst = subst.insert(k.clone(), v.clone());
    }
    subst
}

/// composition of substitutions, applied from right to left:
///   subst_composed = (subst2 . subst1)
pub(crate) fn subst_compose(subst2: &Substitution, subst1: &Substitution) -> Substitution {
    Subst::compose(subst2, subst1)
}

/// apply provided substitution to the input typed expression and return the
/// resulting typed expression
pub(crate) fn apply_subst_typed_expr(subst: &Substitution, expr: TypedVExpr) -> TypedVExpr {
    match expr {
        TypedVExpr::Abstraction(abstr) => {
            let params = abstr
                .params
                .into_iter()
                .map(|param| TypedVAbstrParam {
                    binder: param.binder,
                    pattern: apply_subst_typed_pattern(subst, param.pattern),
                    ty: subst_ty(subst, &param.ty),
                })
                .collect();
            let body = Box::new(apply_subst_typed_expr(subst, *abstr.body));
            TypedVExpr::Abstraction(TypedVAbstrExpr {
                params,
                body,
                ty: subst_ty(subst, &abstr.ty),
            })
        }
        TypedVExpr::Application(app) => TypedVExpr::Application(TypedVAppExpr {
            callable: Box::new(apply_subst_typed_expr(subst, *app.callable)),
            args: app
                .args
                .into_iter()
                .map(|arg| apply_subst_typed_expr(subst, arg))
                .collect(),
            ty: subst_ty(subst, &app.ty),
        }),
        TypedVExpr::Case(case) => TypedVExpr::Case(TypedVCaseExpr {
            arg: Box::new(apply_subst_typed_expr(subst, *case.arg)),
            clauses: case
                .clauses
                .into_iter()
                .map(|clause| TypedVCaseClause {
                    pattern: apply_subst_typed_pattern(subst, clause.pattern),
                    guard: clause
                        .guard
                        .map(|guard| apply_subst_typed_expr(subst, guard)),
                    body: apply_subst_typed_expr(subst, clause.body),
                })
                .collect(),
            ty: subst_ty(subst, &case.ty),
        }),
        TypedVExpr::Let(let_expr) => TypedVExpr::Let(TypedVLetExpr {
            defs: let_expr
                .defs
                .into_iter()
                .map(|(pat, expr)| {
                    (
                        apply_subst_typed_pattern(subst, pat),
                        apply_subst_typed_expr(subst, expr),
                    )
                })
                .collect(),
            body: Box::new(apply_subst_typed_expr(subst, *let_expr.body)),
            ty: subst_ty(subst, &let_expr.ty),
        }),
        TypedVExpr::LitNumeric(x) => TypedVExpr::LitNumeric(TypedVLitNumeric {
            val: x.val.clone(),
            ty: x.ty.clone(),
        }),
        TypedVExpr::LitString(x) => TypedVExpr::LitString(TypedVLitString {
            val: x.val.clone(),
            ty: x.ty.clone(),
        }),
        TypedVExpr::Variable(x) => TypedVExpr::Variable(TypedVVariable {
            var: x.var.clone(),
            ty: subst_ty(subst, &x.ty),
            ty_args: x
                .ty_args
                .into_iter()
                .map(|arg| subst_ty(subst, &arg))
                .collect(),
            ty_schematic: subst_ty_scheme(subst, &x.ty_schematic),
        }),
        TypedVExpr::Constructor(constructor) => TypedVExpr::Constructor(TypedVConstructorExpr {
            ty_name: constructor.ty_name,
            constructor_name: constructor.constructor_name,
            args: constructor
                .args
                .into_iter()
                .map(|arg| apply_subst_typed_expr(subst, arg))
                .collect(),
            record_fields: constructor.record_fields,
            ty: subst_ty(subst, &constructor.ty),
            ty_args: constructor
                .ty_args
                .into_iter()
                .map(|arg| subst_ty(subst, &arg))
                .collect(),
        }),
    }
}

/// apply the provided substitution to the typed value-level pattern
pub(crate) fn apply_subst_typed_pattern(
    subst: &Substitution,
    pattern: TypedVPattern,
) -> TypedVPattern {
    match pattern {
        TypedVPattern::Wild { ty } => TypedVPattern::Wild {
            ty: subst_ty(subst, &ty),
        },
        TypedVPattern::Variable {
            binder,
            ty,
            ty_schematic,
        } => TypedVPattern::Variable {
            binder,
            ty: subst_ty(subst, &ty),
            ty_schematic: subst_ty_scheme(subst, &ty_schematic),
        },
        TypedVPattern::Literal { literal, ty } => TypedVPattern::Literal {
            literal,
            ty: subst_ty(subst, &ty),
        },
        TypedVPattern::Range { start, end, ty } => TypedVPattern::Range {
            start,
            end,
            ty: subst_ty(subst, &ty),
        },
        TypedVPattern::Constructor {
            ty_name,
            constructor,
            args,
            ty,
            ty_args,
        } => TypedVPattern::Constructor {
            ty_name,
            constructor,
            args: args
                .into_iter()
                .map(|arg| apply_subst_typed_pattern(subst, arg))
                .collect(),
            ty: subst_ty(subst, &ty),
            ty_args: ty_args.iter().map(|x| subst_ty(subst, x)).collect(),
        },
        TypedVPattern::Record {
            ty_name,
            constructor,
            fields,
            rest,
            ty,
            ty_args,
        } => TypedVPattern::Record {
            ty_name,
            constructor,
            fields: fields
                .into_iter()
                .map(|(name, pat)| (name, apply_subst_typed_pattern(subst, pat)))
                .collect(),
            rest,
            ty: subst_ty(subst, &ty),
            ty_args: ty_args.iter().map(|x| subst_ty(subst, x)).collect(),
        },
    }
}

/// type check value-level expression with type annotation
pub(crate) fn ty_check_vexpr_typed(
    env_var_to_ty_scheme: &mut EnvVVarToTyScheme,
    ty_env: &TyEnv,
    ty_var_ns: &mut TyVarNameSupply,
    vexpr: &VExpr,
) -> Result<(Substitution, TypedVExpr), TyError> {
    match vexpr {
        VExpr::Abstraction(expr) => {
            ty_check_abstraction_typed(env_var_to_ty_scheme, ty_env, ty_var_ns, expr)
        }
        VExpr::Application(expr) => {
            ty_check_application_typed(env_var_to_ty_scheme, ty_env, ty_var_ns, expr)
        }
        VExpr::Case(expr) => ty_check_case_typed(env_var_to_ty_scheme, ty_env, ty_var_ns, expr),
        VExpr::Let(expr) => ty_check_let_typed(env_var_to_ty_scheme, ty_env, ty_var_ns, expr),

        VExpr::LitNumeric(lit) => {
            let (subst, ty) = ty_check_lit_numeric(env_var_to_ty_scheme, ty_var_ns, lit)?;
            Ok((
                subst,
                TypedVExpr::LitNumeric(TypedVLitNumeric {
                    val: lit.clone(),
                    ty,
                }),
            ))
        }
        VExpr::LitString(lit) => {
            let (subst, ty) = ty_check_lit_string(env_var_to_ty_scheme, ty_var_ns, lit)?;
            Ok((
                subst,
                TypedVExpr::LitString(TypedVLitString {
                    val: lit.clone(),
                    ty,
                }),
            ))
        }
        VExpr::Variable(var) => {
            let (subst, ty, ty_args) = ty_check_variable(env_var_to_ty_scheme, ty_var_ns, var)?;
            Ok((
                subst,
                TypedVExpr::Variable(TypedVVariable {
                    var: var.clone(),
                    ty,
                    ty_args,
                    ty_schematic: env_var_to_ty_scheme.get(var).unwrap().clone(),
                }),
            ))
        }
        VExpr::Constructor(expr) => {
            ty_check_constructor_typed(env_var_to_ty_scheme, ty_env, ty_var_ns, expr)
        }
    }
}

/// type check lambda abstraction, \x.e
/// - assign fresh type variable a to parameter x
/// - type check body e in extended environment {x: a}
/// - return arrow type a -> t where t is the body's type
/// supports multiple parameters by threading pattern typing across them before
/// checking the body. the parser still emits nested abstractions when users
/// write curried lambdas, but this helper handles the general case
pub(crate) fn ty_check_abstraction_typed(
    env_var_to_ty_scheme: &mut EnvVVarToTyScheme,
    ty_env: &TyEnv,
    ty_var_ns: &mut TyVarNameSupply,
    v_abstr_expr: &VAbstrExpr,
) -> Result<(Substitution, TypedVExpr), TyError> {
    let mut env_lambda = env_var_to_ty_scheme.clone();
    let mut subst_params = subst_id();
    let mut typed_params: Vec<TypedVAbstrParam> = Vec::new();

    // process parameters
    for param in v_abstr_expr.params.iter() {
        env_lambda = env_lambda.apply_subst_to_env(&subst_params);

        let ty_binder = TyExpr::TyVar(ty_var_ns.generate());
        env_lambda.insert(
            param.binder.clone(),
            TyScheme {
                ty_vars_schematic: vec![],
                ty_expr: Box::new(ty_binder.clone()),
            },
        );

        let (pattern_subst, _bound_vars, typed_pattern_raw) =
            ty_check_pattern_typed(&mut env_lambda, ty_env, ty_var_ns, &param.pattern)?;
        subst_params = subst_compose(&pattern_subst, &subst_params);

        env_lambda = env_lambda.apply_subst_to_env(&subst_params);

        let ty_binder_resolved = subst_ty(&subst_params, &ty_binder);
        let pattern_resolved = subst_ty(&subst_params, typed_pattern_raw.ty());
        subst_params = unify_ty_exprs(&subst_params, &ty_binder_resolved, &pattern_resolved)?;

        env_lambda = env_lambda.apply_subst_to_env(&subst_params);

        if let Some(param_annot) = &param.annotation {
            let annot_resolved = subst_ty(&subst_params, param_annot);
            let ty_binder_substituted = subst_ty(&subst_params, &ty_binder);
            subst_params = unify_ty_exprs(&subst_params, &ty_binder_substituted, &annot_resolved)?;

            env_lambda = env_lambda.apply_subst_to_env(&subst_params);
        }

        typed_params.push(TypedVAbstrParam {
            binder: param.binder.clone(),
            pattern: apply_subst_typed_pattern(&subst_params, typed_pattern_raw),
            ty: subst_ty(&subst_params, &ty_binder),
        });
    }

    // process body

    env_lambda = env_lambda.apply_subst_to_env(&subst_params);

    let (body_vexpr, body_optional_texpr) = v_abstr_expr.body.as_ref();
    let (body_subst, typed_body_raw) =
        ty_check_vexpr_typed(&mut env_lambda, ty_env, ty_var_ns, body_vexpr)?;

    let mut phi = subst_compose(&body_subst, &subst_params);
    let mut typed_body = apply_subst_typed_expr(&phi, typed_body_raw);

    if let Some(ty_annot_body) = body_optional_texpr {
        let annot_resolved = subst_ty(&phi, ty_annot_body);
        match unify_ty_exprs(&phi, typed_body.ty(), &annot_resolved) {
            Ok(phi2) => {
                phi = phi2;
                typed_body = apply_subst_typed_expr(&phi, typed_body);
            }
            Err(_) => {
                return Err(TyError::TypeConflict(
                    format!(
                        "type checked body expression does not match type annotation: {:?} != {:?}",
                        typed_body.ty(),
                        ty_annot_body,
                    )
                    .to_string(),
                ));
            }
        }
    }

    // update params with new substitution
    let typed_params: Vec<TypedVAbstrParam> = typed_params
        .into_iter()
        .map(|param| TypedVAbstrParam {
            binder: param.binder,
            pattern: apply_subst_typed_pattern(&phi, param.pattern),
            ty: subst_ty(&phi, &param.ty),
        })
        .collect();

    // build type
    let param_types: Vec<TyExpr> = typed_params.iter().map(|p| p.ty.clone()).collect();
    let arrow_type = mk_ty_arrow_multi(param_types, typed_body.ty().clone());

    Ok((
        phi.clone(),
        TypedVExpr::Abstraction(TypedVAbstrExpr {
            params: typed_params,
            body: Box::new(typed_body),
            ty: arrow_type,
        }),
    ))
}

/// type check function application e_0 e_1 ... e_n
/// - type check callable e_0 to get type t_0
/// - for each argument e_i:
///   - type check e_i to get type t_i
///   - create fresh type variable b_i for result
///   - unify t_0 with (t_i -> b_i) to solve for b_i
///   - set t_0 = b_i for the next argument
/// - return the final result type
pub(crate) fn ty_check_application_typed(
    env_var_to_ty_scheme: &mut EnvVVarToTyScheme,
    ty_env: &TyEnv,
    ty_var_ns: &mut TyVarNameSupply,
    vexpr: &VAppExpr,
) -> Result<(Substitution, TypedVExpr), TyError> {
    let (subst_fun, typed_fun) =
        ty_check_vexpr_typed(env_var_to_ty_scheme, ty_env, ty_var_ns, &vexpr.callable.0)?;
    let mut ty_fun = typed_fun.ty().clone();
    let mut subst_acc = subst_fun.clone();
    let mut typed_vexpr_callable = apply_subst_typed_expr(&subst_fun, typed_fun);
    let mut typed_vexpr_args = Vec::new();

    for (arg_expr, _) in vexpr.args.iter() {
        let mut env_arg = env_var_to_ty_scheme.apply_subst_to_env(&subst_acc);

        let (subst_arg, typed_arg_raw) =
            ty_check_vexpr_typed(&mut env_arg, ty_env, ty_var_ns, arg_expr)?;
        let ty_arg = typed_arg_raw.ty().clone();
        subst_acc = subst_compose(&subst_arg, &subst_acc);
        ty_fun = subst_ty(&subst_arg, &ty_fun);

        let tyvar_ret = ty_var_ns.generate();
        let ty_rhs = mk_ty_arrow(ty_arg.clone(), TyExpr::TyVar(tyvar_ret.clone()));
        let phi = unify_ty_exprs(&subst_acc, &ty_fun, &ty_rhs)?;
        subst_acc = phi;

        let ty_ret = subst_acc.get(&tyvar_ret).unwrap();
        let ty_ret_norm = subst_ty(&subst_acc, &ty_ret);

        typed_vexpr_callable = apply_subst_typed_expr(&subst_acc, typed_vexpr_callable);
        let typed_arg = apply_subst_typed_expr(&subst_acc, typed_arg_raw);
        typed_vexpr_args.push(typed_arg);

        ty_fun = ty_ret_norm;
    }

    let ty_result = ty_fun;

    Ok((
        subst_acc.clone(),
        TypedVExpr::Application(TypedVAppExpr {
            callable: Box::new(typed_vexpr_callable),
            args: typed_vexpr_args,
            ty: ty_result,
        }),
    ))
}

/// type check case expression with pattern matching
///
/// case e of
///   p1 -> e1
///   p2 -> e2
///   ...
///
///   - type check scrutinee e to get type t_scrutinee
///   - for each clause i (pattern p_i, body e_i, optional guard g_i):
///     - type check pattern p_i, binding variables into the clause environment
///     - unify scrutinee type with pattern type
///     - if guard exists, type check g_i and unify with Bool
///     - type check body e_i in the environment enriched with bindings
///   - unify all body types (all branches return the same type)
///   - return substitution and the unified body type
///
pub(crate) fn ty_check_case_typed(
    env_var_to_ty_scheme: &mut EnvVVarToTyScheme,
    ty_env: &TyEnv,
    ty_var_ns: &mut TyVarNameSupply,
    vexpr: &VCaseExpr,
) -> Result<(Substitution, TypedVExpr), TyError> {
    // type check argument
    let (phi, typed_scrutinee_raw) =
        ty_check_vexpr_typed(env_var_to_ty_scheme, ty_env, ty_var_ns, &vexpr.arg.0)?;
    // carry scrutinee constraints forward
    // affect patterns and downstream typing
    let typed_scrutinee = apply_subst_typed_expr(&phi, typed_scrutinee_raw);

    let env_augmented = env_var_to_ty_scheme.apply_subst_to_env(&phi);

    struct ClauseInfo {
        body_expr: VExpr,
        env: EnvVVarToTyScheme,
        typed_pattern: TypedVPattern,
        typed_guard: Option<TypedVExpr>,
        subst: Substitution,
    }

    // for each clause, get: (body expr, env, typed pattern, typed guard, substitution)
    let mut clause_infos: Vec<ClauseInfo> = Vec::new();
    for clause in vexpr.clauses.iter() {
        let mut env_clause = env_augmented.clone();
        let (subst_pattern, _bound_vars, typed_pattern) =
            ty_check_pattern_typed(&mut env_clause, ty_env, ty_var_ns, &clause.pattern)?;

        // unify typed_scrutinee with clause's pattern
        let subst_pattern_unified =
            unify_ty_exprs(&subst_pattern, typed_scrutinee.ty(), typed_pattern.ty())?;

        env_clause = env_clause.apply_subst_to_env(&subst_pattern_unified);

        let (subst_clause, typed_guard) = match &clause.guard {
            Some((guard_expr, _)) => {
                let (subst_guard, typed_guard_raw) =
                    ty_check_vexpr_typed(&mut env_clause, ty_env, ty_var_ns, guard_expr)?;
                let mut subst_clause = subst_compose(&subst_guard, &subst_pattern_unified);
                let typed_guard_subst = apply_subst_typed_expr(&subst_clause, typed_guard_raw);
                let subst_guard_bool =
                    unify_ty_exprs(&subst_clause, typed_guard_subst.ty(), &mk_ty_bool())?;
                subst_clause = subst_guard_bool;

                env_clause = env_clause.apply_subst_to_env(&subst_clause);

                (subst_clause, Some(typed_guard_subst))
            }
            _ => (subst_pattern_unified, None),
        };

        let vexpr = clause.body.0.clone();
        clause_infos.push(ClauseInfo {
            body_expr: vexpr,
            env: env_clause,
            typed_pattern,
            typed_guard,
            subst: subst_clause,
        });
    }

    // compose substitutions
    let substs_pattern = clause_infos.iter().map(|x| &x.subst);
    let mut subst_patterns_unified =
        substs_pattern.fold(subst_id(), |acc, s| subst_compose(s, &acc));

    // apply substitution to scrutinee
    let mut ty_scrutinee_updated = subst_ty(&subst_patterns_unified, typed_scrutinee.ty());

    // enforce a single scrutinee type across all clause patterns
    // wildcard/variable patterns are typed as fresh auto type variables so they should be compatible
    for clause_info in clause_infos.iter() {
        let ty_pat = subst_ty(&subst_patterns_unified, clause_info.typed_pattern.ty());
        subst_patterns_unified =
            unify_ty_exprs(&subst_patterns_unified, &ty_scrutinee_updated, &ty_pat)?;
        ty_scrutinee_updated = subst_ty(&subst_patterns_unified, typed_scrutinee.ty());
    }

    // [todo] check case expression coverage over its matching patterns
    // if let Some(cov) = compute_constructor_coverage_for_case(ty_env, &ty_scrutinee_updated, vexpr) {
    //     for &idx in &cov.redundant {
    //         return Err(TyError::UnexpectedPattern(
    //             format!("warning: redundant pattern at clause {:?}", idx).to_string(),
    //         ));
    //     }
    //     for &idx in &cov.unreachable {
    //         return Err(TyError::UnexpectedPattern(
    //             format!("warning: unreachable pattern at clause {:?}", idx).to_string(),
    //         ));
    //     }
    // }

    // type check each clause's body
    let mut subst_bodies = subst_id();
    let mut typed_bodies = Vec::new();
    {
        for clause_info in clause_infos.iter() {
            let mut env_with_subst = clause_info.env.apply_subst_to_env(&subst_bodies);

            let (subst_body, typed_body) = ty_check_vexpr_typed(
                &mut env_with_subst,
                ty_env,
                ty_var_ns,
                &clause_info.body_expr,
            )?;
            subst_bodies = subst_compose(&subst_body, &subst_bodies);
            typed_bodies.push(typed_body);
        }
    }

    // unify types of bodies
    let first_body_ty = typed_bodies
        .first()
        .ok_or_else(|| TyError::UnexpectedExpr("case expr missing a clause body".to_string()))?
        .ty()
        .clone();
    for typed_body in typed_bodies.iter().skip(1) {
        subst_bodies = unify_ty_exprs(&subst_bodies, &first_body_ty, typed_body.ty())?;
    }

    // update substitution and apply it to arg, patterns, bodies, result
    let subst_updated = subst_compose(&subst_bodies, &subst_patterns_unified);
    // include scrutinee substitution
    // keep constraints from inside the scrutinee (e.g. applications)
    // don't drop them at the case boundary
    let subst_final = subst_compose(&subst_updated, &phi);
    let typed_arg = apply_subst_typed_expr(&subst_final, typed_scrutinee);
    let ty_result = subst_ty(&subst_final, &first_body_ty);
    if clause_infos.len() != typed_bodies.len() {
        return Err(TyError::UnexpectedExpr(
            format_args!(
                "case expr body count mismatch: clause_infos={:?} typed_bodies={:?}",
                clause_infos.len(),
                typed_bodies.len()
            )
            .to_string(),
        ));
    }
    let typed_clauses: Vec<TypedVCaseClause> = clause_infos
        .into_iter()
        .zip(typed_bodies)
        .map(|(info, body)| TypedVCaseClause {
            pattern: apply_subst_typed_pattern(&subst_final, info.typed_pattern),
            guard: info
                .typed_guard
                .map(|guard| apply_subst_typed_expr(&subst_final, guard)),
            body: apply_subst_typed_expr(&subst_final, body),
        })
        .collect();

    Ok((
        subst_final,
        TypedVExpr::Case(TypedVCaseExpr {
            arg: Box::new(typed_arg),
            clauses: typed_clauses,
            ty: ty_result,
        }),
    ))
}

#[derive(Clone, Debug)]
pub(crate) struct TypedBindingGroupDef {
    pub typed_pattern: TypedVPattern,
    pub typed_rhs: TypedVExpr,
    pub scheme: TyScheme,
}

/// shared binding-group type checking and inference engine used for both
/// top-level functions and local `let` binding definitions
///
/// returns (substitution, [{id -> (binding, def group)}])
/// where id is the index of input `defs`
/// and the SCC groups are ordered by the order of the returned vector
///
/// steps:
/// - binder seeding with monomorphic placeholders and uniqueness validation
/// - dependency graph construction and finding strongly connected components
/// - topological iteration over SCCs (where dependencies are ordered first)
/// - LHS pattern and RHS expression typechecking and unification
/// - optional signature (polymorphic or monomorphic) verification
/// - generalization `free(def) \ free(env)` per SCC
/// - SCC-wide type scheme variable substitution to keep mutually recursive bindings consistent
/// - `ty_args` backfilling for recursive uses
///
/// notes:
///   recursion policy:
///   - seed the env with monomorphic placeholder for definition associated with
///     each simple variable binder in preparation for type inference/check
///     (following the approach of look to the variables ref. SPJ 1987 S.9.5.2)
///   - no polymorphic recursion support
pub(crate) fn ty_check_binding_group(
    // environment accumulating info as type-checking progresses
    env_v_var_to_ty_scheme_binding_seed: &mut EnvVVarToTyScheme,
    env_outer: &mut EnvVVarToTyScheme,
    ty_env: &TyEnv,
    ty_var_ns: &mut TyVarNameSupply,
    defs: &[(VPattern, VExpr, Option<TyScheme>)],
) -> Result<(Substitution, Vec<BTreeMap<usize, TypedBindingGroupDef>>), TyError> {
    // collect bindings for function/definition; for each of these: insert a monomorphic type variable for the 1st pass
    let mut original_seeded_lhs_binders = BTreeSet::new();

    // map top level function binding to an integer id to for in generic graph algo
    let mut map_binding_to_def_group: BTreeMap<VVar, usize> = BTreeMap::new();

    for (idx, (pattern, _def_expr, _annot)) in defs.iter().enumerate() {
        let pattern_vars_collected = collect_all_pattern_variables(pattern)?;
        for pat_var in pattern_vars_collected {
            // each collected pattern variable is a binder
            //
            // keep a container to discriminate the original set of LHS binders with placeholder type variables
            // note: do not allow same name for binders across different definitions or within same definition group
            if original_seeded_lhs_binders.contains(&pat_var) {
                let first_def_idx =
                    map_binding_to_def_group
                        .get(&pat_var)
                        .copied()
                        .ok_or_else(|| {
                            TyError::InternalError(
                                format_args!(
                                    "binder={:?} cannot be found; duplicate_def_idx={:?}",
                                    pat_var, idx
                                )
                                .to_string(),
                            )
                        })?;
                return Err(TyError::PatBinderUniqueness(format!(
                    "duplicate binder `{:?}` in the same binding group (internal: binder seeding pass, first_def_idx={:?}, duplicate_def_idx={:?})",
                    pat_var, first_def_idx, idx
                ).to_string()));
            }

            original_seeded_lhs_binders.insert(pat_var.clone());

            // introduce monomorphic type variable (placeholder) for each binder
            // recursion policy
            // - seed the env with monomorphic placeholder for each function in
            //   preparation for type inference/check (following the approach of
            //   look to the variables ref. SPJ 1987 S.9.5.2)
            // - no polymorphic recursion support
            env_v_var_to_ty_scheme_binding_seed.insert(
                pat_var.clone(),
                TyScheme {
                    ty_vars_schematic: vec![],
                    ty_expr: Box::new(TyExpr::TyVar(ty_var_ns.generate())),
                },
            );
            // map each binder to its definition
            map_binding_to_def_group.insert(pat_var.clone(), idx);
        }
    }

    // compute mutually dependent definition groups
    let mut scc_groups: Vec<BTreeSet<usize>> = vec![];
    {
        let mut map_def_to_previsit = BTreeMap::new();
        let mut map_def_to_earliest = BTreeMap::new();
        let mut map_def_to_scc = BTreeMap::new();
        let mut stack = vec![];
        let mut generate_previsit = 0;

        // extract free value-level variables in each RHS and retain only binders from
        // this let group
        //
        // return definition indices that the current definition depends on
        //
        // definitions with no in-group recursive references yield an empty neighbor set
        let neighbours = |idx| -> Vec<usize> {
            let (_pat, def_expr, _ty_annot): &(VPattern, VExpr, Option<TyScheme>) = &defs[idx];
            let variables = def_expr.get_free_vars(&BTreeSet::new());
            // map free variables that are in this let group's binder map to definition indices
            let connected_def_ids: BTreeSet<usize> = variables
                .iter()
                .filter_map(|x| map_binding_to_def_group.get(x).cloned())
                .collect();
            connected_def_ids.into_iter().collect()
        };

        for (idx, (_pattern, _def_expr, _annot)) in defs.iter().enumerate() {
            scc(
                &mut map_def_to_previsit,
                &mut map_def_to_earliest,
                &mut map_def_to_scc,
                &mut scc_groups,
                &mut stack,
                &mut generate_previsit,
                &neighbours,
                idx,
            );
        }
    }

    // process in order of SCC dependency
    // ssc_groups should already be in order of dependency
    //   where current group i only possibly have dependencies on group(s) with index j < i
    //   and we process in ascending order of group index
    // as we process SCC groups, accumulate solved substitutions so that it's usable for next SCC group
    let mut subst_accum = subst_id();

    enum TypedLhsRhsPair {
        // single variable bound; generalize as one scheme for this binder
        SimpleVarBindingPair((TypedVPattern, TypedVExpr)),

        // pattern binding (may bind multiple vars); collect bound vars for per-var generalization
        NonSimpleVarBindingPair(
            (
                TypedVPattern,
                Vec<VVar>, /*bound vvars in pattern*/
                TypedVExpr,
            ),
        ),
    }
    let mut typed_binding_def_pairs: BTreeMap<usize, TypedLhsRhsPair> = BTreeMap::new();
    for scc in scc_groups.iter() {
        // per SCC:
        //  - type check patterns and RHSs against monomorphic placeholders, unifying as we go
        //  - collect typed (binding, def) pairs for generalization
        //  - generalize each binding as free(def) \ free(env)
        //  - system f prep: build an scc scheme map, apply it to typed nodes, stamp scheme order,
        //    then backfill recursive ty_args

        for idx in scc.iter() {
            // note: optional TyScheme subsumes monomorphic type
            let (pattern, def_expr, optional_annot): &(VPattern, VExpr, Option<TyScheme>) =
                &defs[*idx];

            *env_v_var_to_ty_scheme_binding_seed =
                env_v_var_to_ty_scheme_binding_seed.apply_subst_to_env(&subst_accum);

            let (subst, bound_vvars_in_pattern, typed_binding_expr) =
                ty_check_pattern_typed_with_seeded_binders(
                    env_v_var_to_ty_scheme_binding_seed,
                    &original_seeded_lhs_binders,
                    ty_env,
                    ty_var_ns,
                    pattern,
                )?;
            subst_accum = subst_compose(&subst_accum, &subst);

            let mut typed_binding_expr =
                apply_subst_typed_pattern(&subst_accum, typed_binding_expr);

            // typecheck RHS
            *env_v_var_to_ty_scheme_binding_seed =
                env_v_var_to_ty_scheme_binding_seed.apply_subst_to_env(&subst_accum);

            let (subst, typed_rhs_vexpr) = ty_check_vexpr_typed(
                env_v_var_to_ty_scheme_binding_seed,
                ty_env,
                ty_var_ns,
                def_expr,
            )?;
            subst_accum = subst_compose(&subst_accum, &subst);

            // unify LHS with RHS
            subst_accum =
                unify_ty_exprs(&subst_accum, typed_binding_expr.ty(), typed_rhs_vexpr.ty())?;

            // keep the typed binding synchronized with the latest substitution
            // without this, pattern ty metadata can lag behind rhs/scheme typing
            typed_binding_expr = apply_subst_typed_pattern(&subst_accum, typed_binding_expr);
            let mut typed_rhs_vexpr = apply_subst_typed_expr(&subst_accum, typed_rhs_vexpr);

            // if present, enforce optional type annotation/signature
            if let (VPattern::Variable(var), Some(sig_scheme)) = (pattern, optional_annot.as_ref())
            {
                // instantiate and then unify
                let mut subst_sig = subst_id();
                for tvn in sig_scheme.ty_vars_schematic.iter() {
                    subst_sig = subst_sig.insert(tvn.clone(), TyExpr::TyVar(ty_var_ns.generate()));
                }
                let sig_type_inst = subst_ty(
                    &subst_compose(&subst_sig, &subst_accum),
                    &sig_scheme.ty_expr,
                );
                subst_accum = unify_ty_exprs(&subst_accum, typed_rhs_vexpr.ty(), &sig_type_inst)
                    .map_err(|e| {
                        TyError::TypeConflict(format!(
                            "annotation/signature mismatch for binder `{:?}`: inferred: {:?}, annotation: {:?}, detail: {:?}",
                            var,
                            typed_rhs_vexpr.ty(),
                            sig_type_inst,
                            e
                        ).to_string())
                    })?;

                // keep typed nodes in sync after annotation unification
                typed_binding_expr = apply_subst_typed_pattern(&subst_accum, typed_binding_expr);
                typed_rhs_vexpr = apply_subst_typed_expr(&subst_accum, typed_rhs_vexpr);
            }

            match pattern {
                VPattern::Variable(_) => {
                    typed_binding_def_pairs.insert(
                        *idx,
                        TypedLhsRhsPair::SimpleVarBindingPair((
                            typed_binding_expr,
                            typed_rhs_vexpr,
                        )),
                    );
                }
                _ => {
                    typed_binding_def_pairs.insert(
                        *idx,
                        TypedLhsRhsPair::NonSimpleVarBindingPair((
                            typed_binding_expr,
                            bound_vvars_in_pattern,
                            typed_rhs_vexpr,
                        )),
                    );
                }
            }
        }

        // generalization (per SCC)
        // - compute free vars in each def and in the outer env
        // - scheme vars = free(def) \ free(env)
        // - record type scheme on simple binders for translating to core IR
        // - update monomorphic placeholders after generalization completes
        let free_ty_vars_in_env: BTreeSet<_> = {
            let mut env_outer_copy = env_outer.clone();
            env_outer_copy = env_outer_copy.apply_subst_to_env(&subst_accum);

            // `free(env)` for HM generalization:
            // type vars in scheme bodies that are not bound by the scheme
            free_tvns_in_ty_env(&env_outer_copy).into_iter().collect()
        };

        // scc-wide map from generalized unification vars to canonical scheme vars
        //
        // keeps mutually recursive binders in sync when they share a generalized var,
        // so one scc_subst can rewrite all RHSs/patterns and ty_args consistently
        let mut scc_scheme_var_map: BTreeMap<TyVarName, TyVarName> = BTreeMap::new();

        // first generalization pass: collect generalizable vars and build the scc-wide scheme map ---
        // per-binder ordering is recomputed later to avoid keeping a separate map
        for idx in scc.iter() {
            let entry = typed_binding_def_pairs
                .get(idx)
                .expect("SCC indices are drawn from binding definitions");
            match entry {
                TypedLhsRhsPair::SimpleVarBindingPair((_typed_binding_vexpr, typed_def_vexpr)) => {
                    // set subtract: free type variables in def texpr \ free type variables (schematic vars) from env

                    let free_ty_vars_in_def =
                        free_ty_vars_excluding_adts(typed_def_vexpr.ty(), ty_env);
                    let ty_vars_generalizable: Vec<_> = free_ty_vars_in_def
                        .into_iter()
                        .filter(|tvn| !free_ty_vars_in_env.contains(tvn))
                        .collect();
                    for tvn in &ty_vars_generalizable {
                        scc_scheme_var_map
                            .entry(tvn.clone())
                            // generate new type variables for schematic type variables to avoid name collision
                            .or_insert_with(|| ty_var_ns.generate());
                    }
                }

                // pattern binding policy
                // - pattern-bound variables stay monomorphic (MR-style); no scheme vars
                TypedLhsRhsPair::NonSimpleVarBindingPair(_) => {}
            }
        }

        let scc_subst_map: BTreeMap<_, _> = scc_scheme_var_map
            .iter()
            .map(|(tvn, scheme_tvn)| (tvn.clone(), TyExpr::TyVar(scheme_tvn.clone())))
            .collect();
        let scc_subst = subst_from_map(&scc_subst_map);

        // second generalization pass: apply scc substitution, finalize schemes, and stamp scheme vars
        for idx in scc.iter() {
            let entry = typed_binding_def_pairs
                .get_mut(idx)
                .expect("SCC indices are drawn from binding definitions");
            match entry {
                TypedLhsRhsPair::SimpleVarBindingPair((typed_binding_vexpr, typed_def_vexpr)) => {
                    let var = match &*typed_binding_vexpr {
                        TypedVPattern::Variable { binder, .. } => binder.clone(),
                        _ => unreachable!("simple pairs are created from variable patterns"),
                    };

                    // recompute per-binder generalized vars so the ordering stays local and fresh
                    let ty_vars_generalizable: Vec<_> =
                        free_ty_vars_excluding_adts(typed_def_vexpr.ty(), ty_env)
                            .into_iter()
                            .filter(|tvn| !free_ty_vars_in_env.contains(tvn))
                            .collect();
                    let scheme_ty_vars: Vec<_> = ty_vars_generalizable
                        .iter()
                        .map(|tvn| {
                            scc_scheme_var_map.get(tvn).cloned().ok_or_else(|| {
                                TyError::TypeConflict(
                                    format_args!("def_idx={:?} missing_tvn={:?}", idx, tvn)
                                        .to_string(),
                                )
                            })
                        })
                        .collect::<Result<Vec<_>, _>>()?;

                    // update for inferencing for remaining SCC groups
                    let ty_scheme_updated = TyScheme {
                        // mapped schematic type variables
                        ty_vars_schematic: scheme_ty_vars.clone(),
                        // apply substitution
                        ty_expr: Box::new(subst_ty(&scc_subst, typed_def_vexpr.ty())),
                    };

                    env_v_var_to_ty_scheme_binding_seed
                        .insert(var.clone(), ty_scheme_updated.clone());
                    env_outer.insert(var.clone(), ty_scheme_updated.clone());

                    // apply scc substitution and stamp scheme order
                    let binding_after_subst =
                        apply_subst_typed_pattern(&scc_subst, typed_binding_vexpr.clone());
                    let rhs_after_subst =
                        apply_subst_typed_expr(&scc_subst, typed_def_vexpr.clone());
                    *typed_def_vexpr = rhs_after_subst;

                    // record generalized type scheme on the binding for ease of
                    // translation to core IR later
                    *typed_binding_vexpr = set_scheme_ty_vars_for_binder_in_pattern(
                        binding_after_subst,
                        &var,
                        ty_scheme_updated.clone(),
                    );
                }
                TypedLhsRhsPair::NonSimpleVarBindingPair((
                    typed_pattern_vexpr,
                    bound_vvars_in_pattern,
                    typed_def_vexpr,
                )) => {
                    // pattern binding policy
                    // - pattern-bound variables are monomorphic; do not attach scheme vars
                    let binding_after_subst =
                        apply_subst_typed_pattern(&scc_subst, typed_pattern_vexpr.clone());
                    let rhs_after_subst =
                        apply_subst_typed_expr(&scc_subst, typed_def_vexpr.clone());
                    *typed_def_vexpr = rhs_after_subst;
                    let mut pat_var_types: BTreeMap<VVar, TyExpr> = BTreeMap::new();
                    collect_typed_pattern_var_types(&binding_after_subst, &mut pat_var_types);
                    for var in bound_vvars_in_pattern.iter() {
                        if let Some(ty_var) = pat_var_types.get(var) {
                            let scheme = TyScheme {
                                ty_vars_schematic: Vec::new(),
                                ty_expr: Box::new(ty_var.clone()),
                            };
                            env_v_var_to_ty_scheme_binding_seed.insert(var.clone(), scheme.clone());
                            env_outer.insert(var.clone(), scheme);
                        }
                    }
                    *typed_pattern_vexpr = binding_after_subst;
                }
            }
        }

        // fill recursive uses with explicit ty_args
        //
        // derive schemes from stamped binding patterns to keep a single source of truth
        let scheme_info_for_scc: BTreeMap<VVar, TyScheme> = typed_binding_def_pairs
            .values()
            .filter_map(|entry| match entry {
                TypedLhsRhsPair::SimpleVarBindingPair((typed_binding_vexpr, _typed_def_vexpr)) => {
                    match typed_binding_vexpr {
                        TypedVPattern::Variable {
                            binder,
                            ty_schematic,
                            ..
                        } => Some((binder.clone(), ty_schematic.clone())),
                        _ => None,
                    }
                }
                _ => None,
            })
            .collect();
        if !scheme_info_for_scc.is_empty() {
            for idx in scc.iter() {
                if let Some(entry) = typed_binding_def_pairs.get_mut(idx)
                    && let TypedLhsRhsPair::SimpleVarBindingPair((_, typed_def_vexpr)) = entry
                {
                    *typed_def_vexpr =
                        fill_missing_ty_args(typed_def_vexpr.clone(), &scheme_info_for_scc)?;
                }
            }
        }
    }

    let final_defs: BTreeMap<usize, TypedBindingGroupDef> = typed_binding_def_pairs
        .into_iter()
        .map(|(idx, pair)| {
            let (typed_pattern, typed_rhs, scheme) = match pair {
                TypedLhsRhsPair::SimpleVarBindingPair((pat, rhs)) => {
                    let scheme = match &pat {
                        TypedVPattern::Variable { ty_schematic, .. } => ty_schematic.clone(),
                        _ => TyScheme {
                            ty_vars_schematic: Vec::new(),
                            ty_expr: Box::new(rhs.ty().clone()),
                        },
                    };
                    (pat, rhs, scheme)
                }
                TypedLhsRhsPair::NonSimpleVarBindingPair((pat, _, rhs)) => {
                    let scheme = TyScheme {
                        ty_vars_schematic: Vec::new(),
                        ty_expr: Box::new(rhs.ty().clone()),
                    };
                    (pat, rhs, scheme)
                }
            };
            (
                idx,
                TypedBindingGroupDef {
                    typed_pattern,
                    typed_rhs,
                    scheme,
                },
            )
        })
        .collect();

    let ordered_mutually_recursive_groups = scc_groups
        .iter()
        .map(|scc_group| {
            let functions_in_group = scc_group
                .iter()
                .map(|idx| (*idx, final_defs.get(idx).unwrap().clone()))
                .collect();
            functions_in_group
        })
        .collect();

    Ok((subst_accum, ordered_mutually_recursive_groups))
}

/// type check let expression (implements let-polymorphism / generalization)
///
/// shape
///   let x1 = e1
///       x2 = e2
///       ...
///   in body
///
/// steps
/// - type check each `ei` to get types `ti`
/// - generalize `ti` by quantifying free vars (excluding those already in the outer env)
/// - extend the env with schemes (`x1: (\forall)a.t1`, `x2: (\forall)b.t2`, ...)
/// - type check the body in the extended env and return its type
///
/// policy
/// - let-bound variables are polymorphic (instantiable at multiple types)
/// - lambda parameters are monomorphic within their scope
/// - pattern-bound variables are monomorphic
pub(crate) fn ty_check_let_typed(
    env_var_to_ty_scheme: &mut EnvVVarToTyScheme,
    ty_env: &TyEnv,
    ty_var_ns: &mut TyVarNameSupply,
    vexpr: &VLetExpr,
) -> Result<(Substitution, TypedVExpr), TyError> {
    let defs: Vec<(VPattern, VExpr, Option<TyScheme>)> = vexpr
        .defs
        .iter()
        .map(|(pat, expr, annot)| {
            (
                pat.clone(),
                expr.clone(),
                // conversion to TyScheme for running the helper
                // `ty_check_binding_group`
                annot
                    .as_ref()
                    .map(|ty| build_scheme_from_ty_expr(ty, ty_env, ty_var_ns)),
            )
        })
        .collect();

    // used to compute free variables in environment
    // this is needed to determine the set of generalizable type variables which become schematic type variables in type schemes
    let mut env_outer = env_var_to_ty_scheme.clone();

    // environment accumulating info as type-checking progresses
    let mut env_working = env_var_to_ty_scheme.clone();

    let (mut subst_accum, mut groups_of_typed_binding_def_pairs) =
        ty_check_binding_group(&mut env_working, &mut env_outer, ty_env, ty_var_ns, &defs)?;

    // typecheck body of let expression using accumulated env and substitutions
    env_working = env_working.apply_subst_to_env(&subst_accum);

    let (subst_body, typed_body_expr) =
        ty_check_vexpr_typed(&mut env_working, ty_env, ty_var_ns, &vexpr.body.0)?;

    subst_accum = subst_compose(&subst_body, &subst_accum);
    let typed_body_expr = apply_subst_typed_expr(&subst_accum, typed_body_expr);
    let ty_body = typed_body_expr.ty().clone();

    let mut typed_defs = vec![];
    for typed_binding_def_pairs in groups_of_typed_binding_def_pairs.into_iter() {
        let defs: Vec<(TypedVPattern, TypedVExpr)> = typed_binding_def_pairs
            .into_values()
            .map(|def| {
                let typed_pat = apply_subst_typed_pattern(&subst_accum, def.typed_pattern);
                let typed_rhs = apply_subst_typed_expr(&subst_accum, def.typed_rhs);
                (typed_pat, typed_rhs)
            })
            .collect();

        typed_defs.extend(defs);
    }

    Ok((
        subst_accum,
        TypedVExpr::Let(TypedVLetExpr {
            defs: typed_defs,
            body: Box::new(typed_body_expr),
            ty: ty_body,
        }),
    ))
}

/// type check ADT constructor application C e1 e2 ... ek where C is a
/// constructor of ADT T a1 a2 ...
/// - resolve constructor C to its ADT definition and constructor signature
/// - instantiate ADT's type parameters with fresh type variables
/// - derive expected field types by applying instantiation's type parameters
/// - if record syntax is used, reorder args by field names and check missing
///   or unknown fields
/// - for each provided argument ei and expected field type ti:
///   - type check ei to get type t'i
///   - unify t'i ~ ti using accumulated substitution
///   - update the substitution and remaining expected field types
/// - resolve the ADT's fresh type parameters using accumulated substitution
/// - return ADT type T applied to resolved params, or an arrow type if
///   partially applied
///
/// eg: Some 42 :: Option I64
///   - fresh instantiation: Option a
///   - field type: a
///   - argument: 42 :: I64
///   - unify: a ~ I64
///   - result: Option I64
pub(crate) fn ty_check_constructor_typed(
    env_var_to_ty_scheme: &mut EnvVVarToTyScheme,
    ty_env: &TyEnv,
    ty_var_ns: &mut TyVarNameSupply,
    vexpr: &VConstructorExpr,
) -> Result<(Substitution, TypedVExpr), TyError> {
    let ctor_ref = match &vexpr.ty_name {
        Some(ty_name) => ConstructorRef::Qualified {
            ty_name: ty_name.clone(),
            constructor: vexpr.constructor.clone(),
        },
        _ => ConstructorRef::Unqualified(vexpr.constructor.clone()),
    };

    let ConstructorInstance {
        ty_name,
        ctor_def,
        param_subst,
        fresh_params,
        ..
    } = instantiate_constructor(ty_env, ty_var_ns, &ctor_ref)?;

    let expected_field_types: Vec<TyExpr> = ctor_def
        .field_types
        .iter()
        .map(|ft| subst_ty(&param_subst, ft))
        .collect();

    let mut qualified_ctor = vexpr.clone();
    qualified_ctor.ty_name = Some(ty_name.clone());

    if qualified_ctor.args.is_empty() && qualified_ctor.record_fields.is_none() {
        // case: not a record and no value arguments
        let result_params: Vec<TyExpr> = fresh_params
            .iter()
            .map(|tp| TyExpr::TyVar(tp.clone()))
            .collect();
        let ty_args = result_params.clone();
        let ty_result = build_adt_type_no_loc(&ty_name, &result_params);
        let ty_fun = if expected_field_types.is_empty() {
            // no additional arguments needed for the constructor
            ty_result
        } else {
            // additional arugments needed for the constructor, so make it a function like type
            mk_ty_arrow_multi(expected_field_types.clone(), ty_result)
        };
        return Ok((
            subst_id(),
            TypedVExpr::Constructor(TypedVConstructorExpr {
                // constructor: qualified_ctor,
                ty_name: qualified_ctor.ty_name.unwrap(),
                constructor_name: qualified_ctor.constructor,
                args: Vec::new(),
                record_fields: None,
                ty: ty_fun,
                ty_args,
            }),
        ));
    }

    let (positional_args, record_order) = if let Some(rec) = &qualified_ctor.record_fields {
        // case: a record => map fields to positions

        let expected_field_names = ctor_def.field_names.as_ref().ok_or_else(|| {
            TyError::UnexpectedSyntax(
                format!(
                    "Constructor {} is not a record, but record syntax was used",
                    qualified_ctor.constructor
                )
                .to_string(),
            )
        })?;
        use std::collections::HashMap;
        // collect field name -> (value expr, optional type expr)
        let mut vexpr_fieldname_mapping: HashMap<&String, &(VExpr, Option<TyExpr>)> =
            HashMap::new();
        for (field_name, vexpr_and_ty_annot) in rec.iter() {
            if vexpr_fieldname_mapping
                .insert(field_name, vexpr_and_ty_annot)
                .is_some()
            {
                return Err(TyError::UnexpectedField(
                    format!(
                        "Duplicate field {} in record constructor {}",
                        field_name, qualified_ctor.constructor
                    )
                    .to_string(),
                ));
            }
        }
        let mut ordered = Vec::new();
        // check against expected field names of the target record
        for fname in expected_field_names.iter() {
            let (vexpr, optional_ty_annot) =
                vexpr_fieldname_mapping.get(fname).ok_or_else(|| {
                    TyError::UnexpectedSyntax(
                        format!(
                            "Missing field {} in record constructor {}",
                            fname, qualified_ctor.constructor
                        )
                        .to_string(),
                    )
                })?;
            ordered.push((vexpr.clone(), optional_ty_annot.clone()));
        }
        if vexpr_fieldname_mapping.len() != expected_field_names.len() {
            return Err(TyError::UnexpectedField(
                format!(
                    "Unknown field in record constructor {}",
                    qualified_ctor.constructor
                )
                .to_string(),
            ));
        }
        (ordered, Some(expected_field_names.clone()))
    } else {
        // case: not a record => just collect positional args
        (qualified_ctor.args.clone(), None)
    };

    let mut subst = subst_id();
    let mut remaining_expected_field_types = expected_field_types;
    let mut typed_args = Vec::new();

    // type infer and check each positional argument
    for (arg_expr, _) in positional_args.iter() {
        if remaining_expected_field_types.is_empty() {
            return Err(TyError::UnexpectedPattern(
                format!(
                    "Constructor {} is over-applied (too many arguments)",
                    qualified_ctor.constructor
                )
                .to_string(),
            ));
        }

        let expected = remaining_expected_field_types.remove(0);

        let mut env_with_subst = env_var_to_ty_scheme.apply_subst_to_env(&subst);

        let (arg_subst, typed_arg_raw) =
            ty_check_vexpr_typed(&mut env_with_subst, ty_env, ty_var_ns, arg_expr)?;
        let ty_arg = typed_arg_raw.ty().clone();

        // update substitution: subst' = (arg_subst. subst)
        subst = subst_compose(&arg_subst, &subst);

        // type check
        subst = unify_ty_exprs(&subst, &ty_arg, &expected)?;

        // update remining type expr with updated substitution
        remaining_expected_field_types = remaining_expected_field_types
            .into_iter()
            .map(|t| subst_ty(&subst, &t))
            .collect();

        typed_args.push(apply_subst_typed_expr(&subst, typed_arg_raw));
    }

    // build the type of the result
    let result_params: Vec<TyExpr> = fresh_params
        .iter()
        .map(|tp| subst_ty(&subst, &TyExpr::TyVar(tp.clone())))
        .collect();
    let ty_args = result_params.clone();
    let ty_result = build_adt_type_no_loc(&ty_name, &result_params);
    let ty_ret = if remaining_expected_field_types.is_empty() {
        // no further arguments required
        ty_result.clone()
    } else {
        // more arguments required, so make a function like type expression
        mk_ty_arrow_multi(remaining_expected_field_types.clone(), ty_result.clone())
    };

    let typed_record_fields = record_order.map(|names| {
        names
            .into_iter()
            .enumerate()
            .map(|(idx, name)| (name, idx))
            .collect::<Vec<_>>()
    });

    Ok((
        subst.clone(),
        TypedVExpr::Constructor(TypedVConstructorExpr {
            // constructor: qualified_ctor,
            ty_name: qualified_ctor.ty_name.unwrap(),
            constructor_name: qualified_ctor.constructor,
            args: typed_args,
            record_fields: typed_record_fields, // record: not none, non-record: none
            ty: ty_ret,
            ty_args,
        }),
    ))
}

pub(crate) fn ty_check_lit_numeric(
    _env_var_to_ty_scheme: &mut EnvVVarToTyScheme,
    _ns: &mut TyVarNameSupply,
    vexpr: &VLitNumeric,
) -> Result<(Substitution, TyExpr), TyError> {
    match &vexpr.value {
        NumericLiteralValue::Int { parsed, raw } => {
            match parsed.to_owned().or_else(|| raw.parse::<i64>().ok()) {
                Some(_) => Ok((subst_id(), mk_ty_i64())),
                None => Err(TyError::TypeConflict(
                    format!("type checking literal (int) failed for {:?}", vexpr).to_string(),
                )),
            }
        }
        NumericLiteralValue::Float { parsed, raw } => {
            match parsed.to_owned().or_else(|| raw.parse::<f64>().ok()) {
                Some(_) => Ok((subst_id(), mk_ty_f64())),
                None => Err(TyError::TypeConflict(
                    format!("type checking literal (float) failed for {:?}", vexpr).to_string(),
                )),
            }
        }
    }
}

pub(crate) fn ty_check_lit_string(
    _env_var_to_ty_scheme: &mut EnvVVarToTyScheme,
    _ns: &mut TyVarNameSupply,
    vexpr: &VLitString,
) -> Result<(Substitution, TyExpr), TyError> {
    match &vexpr.token {
        ConcreteToken::LiteralString(_string) => Ok((subst_id(), mk_ty_string())),
        _ => Err(TyError::TypeConflict(
            format!("type checking literal (string) failed for {:?}", vexpr).to_string(),
        )),
    }
}

/// look up a variable and instantiate its type scheme with fresh type variables
/// - fetch the variable's type scheme from the environment
/// - create fresh type variables for each schematic type variable and
///   substitute them into the scheme type and return the instantiated type
/// - return explicit type arguments in scheme order
pub(crate) fn ty_check_variable(
    env_var_to_ty_scheme: &EnvVVarToTyScheme,
    ty_var_ns: &mut TyVarNameSupply,
    vexpr: &VVar,
) -> Result<(Substitution, TyExpr, Vec<TyExpr>), TyError> {
    // look up the variable to its type scheme
    let type_scheme = env_var_to_ty_scheme
        .0
        .get(vexpr)
        .ok_or_else(|| match vexpr {
            VVar::Renamed(named_uniqued) => {
                let name = format!("{}", named_uniqued.original.token);
                let loc = named_uniqued
                    .original
                    .loc
                    .as_ref()
                    .map(|l| format!("{:?}", l))
                    .unwrap_or_else(|| "<unknown location>".to_string());
                TyError::UnboundVariable(format!("unbound variable `{name}` at {loc}").to_string())
            }
            VVar::Anon(id) => TyError::UnboundVariable(
                format!("unbound anonymous variable anon_{id}").to_string(),
            ),
            VVar::Named(named) => TyError::InternalError(
                format!(
                    "encountered variable {:?} that is not renamed; ensure renamer pass is ran",
                    named
                )
                .to_string(),
            ),
        })?;
    // instantiate by generating unique schematic type variables to avoid collision, then apply substitution
    let mut substitution = subst_id();
    // record instantiation args in scheme order (for later TyApp insertion [todo])
    let mut ty_args = Vec::new();
    for schematic in &type_scheme.ty_vars_schematic {
        let fresh = ty_var_ns.generate();
        substitution = substitution.insert(schematic.clone(), TyExpr::TyVar(fresh.clone()));
        ty_args.push(TyExpr::TyVar(fresh));
    }
    let result_type_expr = subst_ty(&substitution, &type_scheme.ty_expr);
    Ok((subst_id(), result_type_expr, ty_args))
}

/// type check a pattern and return substitution, bound vars, and typed pattern
///
///   - constructor patterns:
///     - resolve constructor
///     - instantiate ADT type params with fresh vars
///     - apply the substitution to field types
///     - validate structure and unify subpatterns
///   - variable/wildcard patterns:
///     - allocate fresh monomorphic type variables
///
/// this uses fresh binders; callers with pre-seeded let binders
/// can use `ty_check_pattern_typed_with_seeded_binders` to reuse
/// placeholder types
///
/// this also augments the environment
pub(crate) fn ty_check_pattern_typed(
    env: &mut EnvVVarToTyScheme,
    ty_env: &TyEnv,
    ty_var_ns: &mut TyVarNameSupply,
    pattern: &VPattern,
) -> Result<(Substitution, Vec<VVar>, TypedVPattern), TyError> {
    let original_seeded_lhs_binders: BTreeSet<VVar> = BTreeSet::new();
    ty_check_pattern_typed_with_seeded_binders(
        env,
        &original_seeded_lhs_binders,
        ty_env,
        ty_var_ns,
        pattern,
    )
}

/// same as `ty_check_pattern_typed`, but reuses placeholder types for binders
/// that were seeded before typing (e.g. let-group LHS binders).
pub(crate) fn ty_check_pattern_typed_with_seeded_binders(
    env: &mut EnvVVarToTyScheme,
    original_seeded_lhs_binders: &BTreeSet<VVar>,
    ty_env: &TyEnv,
    ty_var_ns: &mut TyVarNameSupply,
    pattern: &VPattern,
) -> Result<(Substitution, Vec<VVar>, TypedVPattern), TyError> {
    match pattern {
        VPattern::Wild => {
            let ty = TyExpr::TyVar(ty_var_ns.generate());
            Ok((subst_id(), Vec::new(), TypedVPattern::Wild { ty }))
        }
        VPattern::Variable(var) => {
            // reuse the pre-seeded monomorphic placeholder type when available
            if original_seeded_lhs_binders.contains(var) {
                let ty_scheme = env.get(var).ok_or_else(|| {
                    TyError::InternalError(
                        format!(
                            "variable {:?} expected to be an original seeded LHS binder",
                            var,
                        )
                        .to_string(),
                    )
                })?;
                return Ok((
                    subst_id(),
                    vec![var.clone()],
                    TypedVPattern::Variable {
                        binder: var.clone(),
                        ty: (*ty_scheme.ty_expr).clone(),
                        ty_schematic: ty_scheme.clone(),
                    },
                ));
            }
            let ty = TyExpr::TyVar(ty_var_ns.generate());
            env.insert(
                var.clone(),
                TyScheme {
                    ty_vars_schematic: vec![],
                    ty_expr: Box::new(ty.clone()),
                },
            );
            Ok((
                subst_id(),
                vec![var.clone()],
                TypedVPattern::Variable {
                    binder: var.clone(),
                    ty,
                    ty_schematic: env.get(var).unwrap().clone(),
                },
            ))
        }
        VPattern::Literal(literal) => {
            let mut env_clone = env.clone();
            let vexpr = match literal {
                VPatternLiteral::Numeric(num) => VExpr::LitNumeric(num.clone()),
                VPatternLiteral::String(lit) => VExpr::LitString(lit.clone()),
            };
            let (subst, typed_expr) =
                ty_check_vexpr_typed(&mut env_clone, ty_env, ty_var_ns, &vexpr)?;
            let ty = typed_expr.ty().clone();
            Ok((
                subst.clone(),
                Vec::new(),
                TypedVPattern::Literal {
                    literal: literal.clone(),
                    ty,
                },
            ))
        }
        VPattern::Range { start, end } => {
            let mut env_clone = env.clone();
            let v_expr_start = match start {
                RangeBound::Inclusive(lit) | RangeBound::Exclusive(lit) => match lit {
                    VPatternLiteral::Numeric(num) => VExpr::LitNumeric(num.clone()),
                    VPatternLiteral::String(lit) => VExpr::LitString(lit.clone()),
                },
            };
            let v_expr_end = match end {
                RangeBound::Inclusive(lit) | RangeBound::Exclusive(lit) => match lit {
                    VPatternLiteral::Numeric(num) => VExpr::LitNumeric(num.clone()),
                    VPatternLiteral::String(lit) => VExpr::LitString(lit.clone()),
                },
            };
            let (subst_start, typed_start) =
                ty_check_vexpr_typed(&mut env_clone, ty_env, ty_var_ns, &v_expr_start)?;

            let mut env_clone_end = env_clone.apply_subst_to_env(&subst_start);

            let (subst_end, typed_end) =
                ty_check_vexpr_typed(&mut env_clone_end, ty_env, ty_var_ns, &v_expr_end)?;
            let mut subst = subst_compose(&subst_end, &subst_start);
            subst = unify_ty_exprs(&subst, typed_start.ty(), typed_end.ty())?;
            Ok((
                subst.clone(),
                Vec::new(),
                TypedVPattern::Range {
                    start: start.clone(),
                    end: end.clone(),
                    ty: subst_ty(&subst, typed_start.ty()),
                },
            ))
        }
        VPattern::Constructor {
            ty_name,
            constructor,
            args,
        } => {
            let PatternConstructorInfo {
                instance:
                    ConstructorInstance {
                        ty_name,
                        ctor_def,
                        param_subst: _,
                        fresh_params,
                    },
                field_types,
            } = instantiate_pattern_constructor(ty_env, ty_var_ns, ty_name, constructor)?;

            if args.len() != ctor_def.field_types.len() {
                return Err(TyError::ArityMismatch {
                    constructor: constructor.clone(),
                    expected: ctor_def.field_types.len(),
                    got: args.len(),
                });
            }

            let mut subst = subst_id();
            let mut bound_vars = Vec::new();
            let mut typed_args = Vec::new();

            // typecheck constructor args with fields in the type
            for (arg_pattern, ty_field) in args.iter().zip(field_types.iter()) {
                let (collected, typed_arg) = unify_nested_pattern_typed(
                    env,
                    original_seeded_lhs_binders,
                    ty_env,
                    ty_var_ns,
                    &mut subst,
                    arg_pattern,
                    ty_field,
                )?;
                bound_vars.extend(collected);
                typed_args.push(typed_arg);
            }

            let result_params: Vec<TyExpr> = fresh_params
                .iter()
                .map(|x| subst_ty(&subst, &TyExpr::TyVar(x.clone())))
                .collect();

            let ty_result = build_adt_type_no_loc(&ty_name, &result_params);

            Ok((
                subst.clone(),
                bound_vars,
                apply_subst_typed_pattern(
                    &subst,
                    TypedVPattern::Constructor {
                        ty_name: ty_name.clone(),
                        constructor: constructor.clone(),
                        args: typed_args,
                        ty: ty_result,
                        ty_args: result_params,
                    },
                ),
            ))
        }
        VPattern::Record {
            ty_name,
            constructor,
            fields,
            rest,
        } => {
            let PatternConstructorInfo {
                instance:
                    ConstructorInstance {
                        ty_name,
                        ctor_def,
                        param_subst: _,
                        fresh_params,
                    },
                field_types: expected_field_types,
            } = instantiate_pattern_constructor(ty_env, ty_var_ns, ty_name, constructor)?;

            let expected_field_names = ctor_def.field_names.as_ref().ok_or_else(|| {
                TyError::TypeConflict(
                    format!(
                        "Constructor {} is not a record, but record pattern syntax was used",
                        ctor_def.name
                    )
                    .to_string(),
                )
            })?;

            let expected_field_map: HashMap<&String, TyExpr> = expected_field_names
                .iter()
                .zip(expected_field_types)
                .collect();

            let mut seen = HashSet::new();
            let mut subst = subst_id();
            let mut bound_vars = Vec::new();
            let mut typed_fields = Vec::new();

            for (field_name, field_pattern) in fields {
                if seen.contains(field_name) {
                    return Err(TyError::UnexpectedField(
                        format!(
                            "Duplicate field {} in record pattern {}",
                            field_name, constructor
                        )
                        .to_string(),
                    ));
                }
                seen.insert(field_name);

                let expected_field_ty = expected_field_map.get(field_name).ok_or_else(|| {
                    TyError::UnexpectedField(
                        format!(
                            "Unknown field {} in pattern for constructor {}",
                            field_name, constructor
                        )
                        .to_string(),
                    )
                })?;

                let (collected, typed_field) = unify_nested_pattern_typed(
                    env,
                    original_seeded_lhs_binders,
                    ty_env,
                    ty_var_ns,
                    &mut subst,
                    field_pattern,
                    expected_field_ty,
                )?;
                bound_vars.extend(collected);
                typed_fields.push((field_name.clone(), typed_field));
            }

            if !*rest {
                // if no omission, check all fields are present
                let missing: Vec<&String> = expected_field_map
                    .keys()
                    .filter(|name| !seen.contains(*name))
                    .copied()
                    .collect();
                if !missing.is_empty() {
                    return Err(TyError::TypeConflict(
                        format!(
                            "Record pattern for constructor {} missing fields {:?}",
                            constructor, missing
                        )
                        .to_string(),
                    ));
                }
            }

            // apply accumulated substitution to type variables for the ADT
            let result_params: Vec<TyExpr> = fresh_params
                .iter()
                .map(|tp| subst_ty(&subst, &TyExpr::TyVar(tp.clone())))
                .collect();

            let ty_result = build_adt_type_no_loc(&ty_name, &result_params);

            Ok((
                subst.clone(),
                bound_vars,
                apply_subst_typed_pattern(
                    &subst,
                    TypedVPattern::Record {
                        ty_name: Some(ty_name.clone()),
                        constructor: constructor.clone(),
                        fields: typed_fields,
                        rest: *rest,
                        ty: ty_result,
                        ty_args: result_params,
                    },
                ),
            ))
        }
    }
}

fn is_adt_type_var(type_env: &TyEnv, tvn: &TyVarName) -> bool {
    match tvn {
        TyVarName::UserDefined(ud) => match &ud.token {
            ConcreteToken::Iden(name) => type_env.get_adt(name).is_ok(),
            _ => false,
        },
        _ => false,
    }
}

/// this is used to backfill recursive uses that were typed against monomorphic
/// placeholders, eg: fill in missing type arguments (ty_args) for recursive
/// polymorphic uses
///
/// `scheme_info_for_scc` maps generalized binders (eg: the current scc group)
/// to their schemes for backfilling recursive instantiations
///
/// we maintain which schematic type variables are visible in the current scope
///
/// we handle shadowing at each scope, by removing shadowed binders from the
/// `scheme_info_for_scc` map in order to avoid backfilling ty_args for locally
/// shadowed names
///
/// empty ty_args can mean:
/// - a monomorphic use, or
/// - a recursive placeholder use
///
/// backfill is applicable when the recursive placeholder variable has empty
/// ty_args and the variable is in `scheme_info_for_scc` map; recursive
/// placeholder uses in the map get explicit ty_args in scheme order
///
/// monomorphic uses not in the map stay empty and are already complete
///
/// handling of different constructs:
/// - abstraction:
///     - collect binders from parameter patterns, drop them from the
///       `scheme_info_for_scc` map to handle shadowing, recurse into body
/// - case:
///     - recurse into the scrutinee with the current `scheme_info_for_scc` map
///     - for each clause, drop binders from the clause pattern to handle
///       shadowing before recursing into guard and body
/// - let:
///     - collect binders from all defs, drop them from the `scheme_info_for_scc`
///       map to handle shadowing, recurse into RHS and body
/// - literal:
///     - no-op
/// - variable:
///     - if a variable has empty ty_args and appears in the map, synthesize
///       ty_args by matching the scheme to the call-site type in scheme order
/// - application / constructor:
///     - recurse into child expressions, preserving any existing ty_args
///
pub(crate) fn fill_missing_ty_args(
    expr: TypedVExpr,
    scheme_info_for_scc: &BTreeMap<VVar, TyScheme>,
) -> Result<TypedVExpr, TyError> {
    match expr {
        TypedVExpr::Abstraction(abstr) => {
            let mut shadowed_binders = Vec::new();
            for param in &abstr.params {
                collect_pattern_binders(&param.pattern, &mut shadowed_binders);
            }
            // remove shadowed binders so only recursive uses are backfilled
            let visible_scheme_info_for_scc =
                schematic_info_without_binders(scheme_info_for_scc, &shadowed_binders);
            let body = Box::new(fill_missing_ty_args(
                *abstr.body,
                &visible_scheme_info_for_scc,
            )?);
            // move other remaining fields from abstr to new struct
            Ok(TypedVExpr::Abstraction(TypedVAbstrExpr { body, ..abstr }))
        }
        TypedVExpr::Application(app) => Ok(TypedVExpr::Application(TypedVAppExpr {
            callable: Box::new(fill_missing_ty_args(*app.callable, scheme_info_for_scc)?),
            args: app
                .args
                .into_iter()
                .map(|arg| fill_missing_ty_args(arg, scheme_info_for_scc))
                .collect::<Result<Vec<_>, _>>()?,
            ty: app.ty,
        })),
        TypedVExpr::Case(case_expr) => {
            let arg = Box::new(fill_missing_ty_args(*case_expr.arg, scheme_info_for_scc)?);
            let clauses = case_expr
                .clauses
                .into_iter()
                .map(|clause| {
                    let mut shadowed_binders = Vec::new();
                    collect_pattern_binders(&clause.pattern, &mut shadowed_binders);
                    // filter shadowed binders before recursing into clause
                    let clause_scheme_info_for_scc =
                        schematic_info_without_binders(scheme_info_for_scc, &shadowed_binders);
                    Ok(TypedVCaseClause {
                        pattern: clause.pattern,
                        guard: clause
                            .guard
                            .map(|guard| fill_missing_ty_args(guard, &clause_scheme_info_for_scc))
                            .transpose()?,
                        body: fill_missing_ty_args(clause.body, &clause_scheme_info_for_scc)?,
                    })
                })
                .collect::<Result<Vec<_>, _>>()?;
            Ok(TypedVExpr::Case(TypedVCaseExpr {
                arg,
                clauses,
                ty: case_expr.ty,
            }))
        }
        TypedVExpr::Let(let_expr) => {
            let mut shadowed_binders = Vec::new();
            for (pat, _) in &let_expr.defs {
                collect_pattern_binders(pat, &mut shadowed_binders);
            }
            // let bindings shadow outer names; avoid filling ty_args for those
            let visible_scheme_info_for_scc =
                schematic_info_without_binders(scheme_info_for_scc, &shadowed_binders);
            let defs = let_expr
                .defs
                .into_iter()
                .map(|(pat, rhs)| {
                    Ok((
                        pat,
                        fill_missing_ty_args(rhs, &visible_scheme_info_for_scc)?,
                    ))
                })
                .collect::<Result<Vec<_>, _>>()?;
            let body = Box::new(fill_missing_ty_args(
                *let_expr.body,
                &visible_scheme_info_for_scc,
            )?);
            Ok(TypedVExpr::Let(TypedVLetExpr {
                defs,
                body,
                ty: let_expr.ty,
            }))
        }
        // base case of recursion
        TypedVExpr::LitNumeric(x) => Ok(TypedVExpr::LitNumeric(x)),
        // base case of recursion
        TypedVExpr::LitString(x) => Ok(TypedVExpr::LitString(x)),
        // base case of recursion
        TypedVExpr::Variable(TypedVVariable {
            var,
            ty,
            mut ty_args,
            mut ty_schematic,
        }) => {
            if ty_args.is_empty()
                && let Some(scheme) = scheme_info_for_scc.get(&var)
            {
                // perform back filling at use site

                // sanity check for matching the binder scheme against
                // this call-site type
                let instantiation_subst = unify_ty_exprs(&subst_id(), &scheme.ty_expr, &ty)
                            .map_err(|err| {
                                TyError::TypeConflict(
                                    format_args!(
                                        "unification error for binder=`{:?}` scheme_type={:?} use_type={:?} detail={:?}",
                                        var, scheme.ty_expr, ty, err
                                    )
                                    .to_string(),
                                )
                            })?;
                ty_args = scheme
                    .ty_vars_schematic
                    .iter()
                    .cloned()
                    .map(|tvn| subst_ty(&instantiation_subst, &TyExpr::TyVar(tvn)))
                    .collect();

                ty_schematic = scheme.clone();

                let used_ty_vars: BTreeSet<_> = ty_args.iter().flat_map(free_ty_vars).collect();
                let in_scope_ty_vars = free_ty_vars(&ty);
                if !used_ty_vars.is_subset(&in_scope_ty_vars) {
                    return Err(TyError::TypeConflict(
                        format_args!(
                            "recursive ty_args introduce new type vars for {:?}: {:?} not in {:?}",
                            var, used_ty_vars, in_scope_ty_vars
                        )
                        .to_string(),
                    ));
                }
            }
            Ok(TypedVExpr::Variable(TypedVVariable {
                var,
                ty,
                ty_args,
                ty_schematic,
            }))
        }
        TypedVExpr::Constructor(constructor) => {
            Ok(TypedVExpr::Constructor(TypedVConstructorExpr {
                // if zero-arg constructor, then this behaves like the base case of recursion
                ty_name: constructor.ty_name,
                constructor_name: constructor.constructor_name,
                args: constructor
                    .args
                    .into_iter()
                    .map(|arg| fill_missing_ty_args(arg, scheme_info_for_scc))
                    .collect::<Result<Vec<_>, _>>()?,
                record_fields: constructor.record_fields,
                ty: constructor.ty,
                ty_args: constructor.ty_args,
            }))
        }
    }
}

/// intent: associate a deterministic TyLam order with the binding site so downstream System F
/// elaboration can recover it
///
/// update `pattern` by attaching `scheme_ty_vars` to the binder matching `binder_target`
/// preserve the rest of the pattern structure
///
/// use cases:
/// - after generalizing a simple let binding, stamp `scheme_ty_vars` on `binder_target` so Core
///   elaboration can insert TyLam binders in the same order
/// - after generalizing a non-simple pattern binding, update `binder_target` inside `pattern`
///   for the same purpose
fn set_scheme_ty_vars_for_binder_in_pattern(
    pattern: TypedVPattern,
    binder_target: &VVar,
    ty_schematic: TyScheme,
) -> TypedVPattern {
    // attach scheme vars to the matching binder while preserving other pattern structure
    match pattern {
        TypedVPattern::Variable {
            binder: pat_binder,
            ty,
            ty_schematic: existing,
        } => {
            if &pat_binder == binder_target {
                TypedVPattern::Variable {
                    binder: pat_binder,
                    ty,
                    ty_schematic,
                }
            } else {
                TypedVPattern::Variable {
                    binder: pat_binder,
                    ty,
                    ty_schematic: existing,
                }
            }
        }
        TypedVPattern::Constructor {
            ty_name,
            constructor,
            args,
            ty,
            ty_args,
        } => TypedVPattern::Constructor {
            ty_name,
            constructor,
            args: args
                .into_iter()
                .map(|arg| {
                    set_scheme_ty_vars_for_binder_in_pattern(
                        arg,
                        binder_target,
                        ty_schematic.clone(),
                    )
                })
                .collect(),
            ty,
            ty_args,
        },
        TypedVPattern::Record {
            ty_name,
            constructor,
            fields,
            rest,
            ty,
            ty_args,
        } => TypedVPattern::Record {
            ty_name,
            constructor,
            fields: fields
                .into_iter()
                .map(|(name, pat)| {
                    (
                        name,
                        set_scheme_ty_vars_for_binder_in_pattern(
                            pat,
                            binder_target,
                            ty_schematic.clone(),
                        ),
                    )
                })
                .collect(),
            rest,
            ty,
            ty_args,
        },
        other => other,
    }
}

/// resolve a constructor and instantiate its type parameters with fresh type
/// variables
/// - look up constructor info in the type environment
/// - create fresh type variable for each ADT type parameter
/// - build a substitution from ADT params to fresh vars
/// - return the instantiated constructor info and substitution
fn instantiate_constructor<'a>(
    type_env: &'a TyEnv,
    ns: &mut TyVarNameSupply,
    ctor_ref: &ConstructorRef,
) -> Result<ConstructorInstance<'a>, TyError> {
    let resolved = type_env.resolve_constructor(ctor_ref)?;

    let fresh_params: Vec<TyVarName> = resolved
        .adt
        .ty_params
        .iter()
        .map(|_| ns.generate())
        .collect();

    let subst_map: BTreeMap<TyVarName, TyExpr> = resolved
        .adt
        .ty_params
        .iter()
        .zip(fresh_params.iter())
        .map(|(old, new)| (old.clone(), TyExpr::TyVar(new.clone())))
        .collect();

    let param_subst = subst_from_map(&subst_map);

    Ok(ConstructorInstance {
        ty_name: resolved.ty_name,
        ctor_def: resolved.ctor,
        param_subst,
        fresh_params,
    })
}

/// resolve a constructor reference for a pattern and instantiate its type
/// parameters
/// - resolve the constructor reference (qualified or unqualified)
/// - instantiate the constructor with fresh type variables
/// - apply the instantiation substitution to constructor field types
fn instantiate_pattern_constructor<'a>(
    type_env: &'a TyEnv,
    ns: &mut TyVarNameSupply,
    type_name: &Option<String>,
    constructor: &String,
) -> Result<PatternConstructorInfo<'a>, TyError> {
    let ctor_ref = match type_name {
        Some(type_name) => ConstructorRef::Qualified {
            ty_name: type_name.clone(),
            constructor: constructor.clone(),
        },
        None => ConstructorRef::Unqualified(constructor.clone()),
    };

    let instance = instantiate_constructor(type_env, ns, &ctor_ref)?;

    let field_types = instance
        .ctor_def
        .field_types
        .iter()
        .map(|ty| subst_ty(&instance.param_subst, ty))
        .collect();

    Ok(PatternConstructorInfo {
        instance,
        field_types,
    })
}

/// return bound vars and typed pattern with substitution (after unification
/// happens) applied
fn unify_nested_pattern_typed(
    env: &mut EnvVVarToTyScheme,
    original_seeded_lhs_binders: &BTreeSet<VVar>,
    type_env: &TyEnv,
    ns: &mut TyVarNameSupply,
    subst: &mut Substitution,
    pattern: &VPattern,
    ty_expected: &TyExpr,
) -> Result<(Vec<VVar>, TypedVPattern), TyError> {
    *env = env.apply_subst_to_env(subst);

    let (pattern_subst, bound_vars, typed_pattern_expr) =
        ty_check_pattern_typed_with_seeded_binders(
            env,
            original_seeded_lhs_binders,
            type_env,
            ns,
            pattern,
        )?;

    *subst = subst_compose(&pattern_subst, subst);

    *env = env.apply_subst_to_env(subst);

    let ty_pattern = typed_pattern_expr.ty().clone();
    let unified = unify_ty_exprs(subst, &ty_pattern, ty_expected)?;
    *subst = unified;

    *env = env.apply_subst_to_env(subst);

    Ok((
        bound_vars,
        apply_subst_typed_pattern(subst, typed_pattern_expr),
    ))
}

fn collect_all_pattern_variables_aux(
    ret: &mut BTreeSet<VVar>,
    pat: &VPattern,
) -> Result<(), TyError> {
    match pat {
        VPattern::Variable(var) => {
            // disallow duplicate binders within a single pattern tree
            if ret.contains(var) {
                return Err(TyError::PatBinderUniqueness(format!(
                    "duplicate pattern binder `{:?}` in let LHS pattern (internal: collect_all_pattern_variables_aux)",
                    var
                ).to_string()));
            }
            ret.insert(var.clone());
        }
        VPattern::Constructor { args, .. } => {
            for arg in args.iter() {
                collect_all_pattern_variables_aux(ret, arg)?;
            }
        }
        VPattern::Record { fields, .. } => {
            for (_field_name, field_pattern) in fields.iter() {
                collect_all_pattern_variables_aux(ret, field_pattern)?;
            }
        }
        VPattern::Wild | VPattern::Literal { .. } | VPattern::Range { .. } => {}
    }
    Ok(())
}

/// collects all binders reachable from one LHS pattern
fn collect_all_pattern_variables(pat: &VPattern) -> Result<BTreeSet<VVar>, TyError> {
    let mut collected: BTreeSet<VVar> = BTreeSet::new();
    collect_all_pattern_variables_aux(&mut collected, pat)?;
    Ok(collected)
}

// collect value-level variable -> type expression mappings from a typed value-level pattern
pub(crate) fn collect_typed_pattern_var_types(
    pat: &TypedVPattern,
    out: &mut BTreeMap<VVar, TyExpr>,
) {
    match pat {
        TypedVPattern::Variable { binder, ty, .. } => {
            out.insert(binder.clone(), ty.clone());
        }
        TypedVPattern::Range { .. } | TypedVPattern::Literal { .. } => {}
        TypedVPattern::Constructor { args, .. } => {
            for arg in args {
                collect_typed_pattern_var_types(arg, out);
            }
        }
        TypedVPattern::Record { fields, .. } => {
            for (_, field_pat) in fields {
                collect_typed_pattern_var_types(field_pat, out);
            }
        }
        TypedVPattern::Wild { .. } => {}
    }
}

/// collect all value-level binders introduced by a typed pattern
/// algorithm: walk the pattern tree and push each binder encountered in order
fn collect_pattern_binders(pat: &TypedVPattern, out: &mut Vec<VVar>) {
    match pat {
        TypedVPattern::Variable { binder, .. } => out.push(binder.clone()),
        TypedVPattern::Constructor { args, .. } => {
            for arg in args {
                collect_pattern_binders(arg, out);
            }
        }
        TypedVPattern::Record { fields, .. } => {
            for (_, pat) in fields {
                collect_pattern_binders(pat, out);
            }
        }
        TypedVPattern::Wild { .. }
        | TypedVPattern::Literal { .. }
        | TypedVPattern::Range { .. } => {}
    }
}

/// drop any binders from the `scheme_info_for_scc` map that are newly in scope
/// which are provided in `binders` and return the result as a new map
fn schematic_info_without_binders(
    schematic_info: &BTreeMap<VVar, TyScheme>,
    binders: &[VVar],
) -> BTreeMap<VVar, TyScheme> {
    if binders.is_empty() {
        return schematic_info.clone();
    }
    let mut next = schematic_info.clone();
    for binder in binders {
        next.remove(binder);
    }
    next
}

/// build a type scheme from a `ty_expr` type annotation
/// by generalizing free user-defined type variables that are not ADT names
/// in the type environment
pub(crate) fn build_scheme_from_ty_expr(
    ty_expr: &TyExpr,
    ty_env: &TyEnv,
    ns: &mut TyVarNameSupply,
) -> TyScheme {
    fn collect_user_vars<'a>(
        ty: &'a TyExpr,
        out: &mut BTreeSet<&'a TyVarNameUserDefined>,
        te: &TyEnv,
    ) {
        match ty {
            TyExpr::TyVar(TyVarName::UserDefined(u)) => {
                if let Err(TyError::UnknownType(_)) = te.get_adt(&format!("{}", u.token)) {
                    out.insert(u);
                }
            }
            TyExpr::TyVar(_) => {}
            TyExpr::TyApp(app) => {
                collect_user_vars(&app.ty_func, out, te);
                collect_user_vars(&app.ty_arg, out, te);
            }
        }
    }

    let mut ty_vars_schematic: Vec<TyVarName> = Vec::new();
    let mut subst = SubstPersistentIdent::default();

    let mut to_generalize = BTreeSet::new();
    collect_user_vars(ty_expr, &mut to_generalize, ty_env);

    for u in to_generalize {
        let fresh_ty_var_name = ns.generate();
        ty_vars_schematic.push(fresh_ty_var_name.clone());
        subst = subst.insert(
            TyVarName::UserDefined(u.clone()),
            TyExpr::TyVar(fresh_ty_var_name),
        );
    }

    let ty_expr_generalized = subst_ty(&subst, ty_expr);

    TyScheme {
        ty_vars_schematic,
        ty_expr: Box::new(ty_expr_generalized),
    }
}
