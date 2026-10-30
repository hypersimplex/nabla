use crate::builtin::types::resolve_builtin_ty_con;
use crate::parse::abstr_pattern::is_type_constructor_name;
use crate::parse::concrete_token::*;
use crate::typecheck::adt::*;
use crate::typecheck::algos::*;
use crate::typecheck::env_v_var_to_ty_scheme::*;
use crate::typecheck::subst::*;
use crate::typecheck::subst_persistent::*;
use crate::typecheck::ty_con_env::*;
use crate::typecheck::ty_err::*;
use crate::typecheck::ty_expr::*;
use crate::typecheck::ty_scheme::*;
use crate::typecheck::ty_var_name::*;
use crate::typecheck::ty_var_name_supply::*;
use crate::typecheck::v_expr::*;
use crate::typecheck::v_expr_typed::*;
use crate::util::printer::DocPrinter;

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
    for (_vvar, scheme) in ty_scheme {
        let bound: BTreeSet<_> = scheme.ty_vars_schematic.iter().cloned().collect();
        for tvn in free_ty_vars(&scheme.ty_expr) {
            if !bound.contains(&tvn) {
                out.insert(tvn);
            }
        }
    }
    out.into_iter().collect()
}

// compute free type variables for the type expression
pub(crate) fn free_ty_vars(ty_expr: &TyExpr) -> BTreeSet<TyVarName> {
    let mut vars: BTreeSet<TyVarName> = BTreeSet::new();
    let mut stack: Vec<&TyExpr> = vec![ty_expr];
    while let Some(current) = stack.pop() {
        match current {
            TyExpr::TyVar(var) => {
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

/// finds a most general unifier, substitution `s`, such that s(t1) = s(t2)
///
/// notes:
///   - concrete types are rigid, they only unify with the same type
///   - TyApp unifies structurally: T1 A ~ T2 B iff T1 ~ T2 and A ~ B,
///     where `~` means `unifies with`
///   - prevents infinite types (handled in `extend`) by occurs check
///
///   - type variable classification:
///     - Auto:
///         - acts as variable in type equations, these can unify with anything
///     - Rigid:
///       - represents a universal quantified generic variable (cannot assume
///         anything about it)
///       - cannot unify with concrete types
///     - UserDefined:
///       - source-level identifer; acts as a brige to be able to unify with a concrete type
pub(crate) fn unify_ty_exprs(
    subst: &Substitution,
    ty_expr1: &TyExpr,
    ty_expr2: &TyExpr,
) -> Result<Substitution, TyError> {
    match (ty_expr1, ty_expr2) {
        // TyCon vs TyCon: identical constructors unify
        (TyExpr::TyCon(c1), TyExpr::TyCon(c2)) => {
            if c1 == c2 {
                Ok(subst.clone())
            } else {
                Err(TyError::TypeConflict {
                    ty1: ty_expr1.clone(),
                    ty2: ty_expr2.clone(),
                    msg: Some(format!(
                        "incompatible type constructors: `{}` vs `{}`",
                        c1.to_doc(),
                        c2.to_doc()
                    )),
                })
            }
        }

        // TyCon vs TyApp: incompatible
        (TyExpr::TyCon(_), TyExpr::TyApp(_)) | (TyExpr::TyApp(_), TyExpr::TyCon(_)) => {
            Err(TyError::TypeConflict {
                ty1: ty_expr1.clone(),
                ty2: ty_expr2.clone(),
                msg: Some(
                    "cannot unify concrete type constructor with type application".to_string(),
                ),
            })
        }

        (TyExpr::TyVar(tvn), other) => {
            // apply current substitution to see what the type variable resolves to
            let sub_ty_expr1 = subst.get(tvn).unwrap();
            match &sub_ty_expr1 {
                // if it's still a type variable after substitution
                TyExpr::TyVar(x) => {
                    // type constructors (builtin and user-defined) are rigid/concrete types:
                    //   unify with themselves or with Auto variables
                    match (x, other) {
                        // Rigid vs TyCon: cannot bind rigid to concrete constructor
                        (TyVarName::Rigid(_), TyExpr::TyCon(_)) => Err(TyError::TypeConflict {
                            ty1: ty_expr1.clone(),
                            ty2: ty_expr2.clone(),
                            msg: Some(
                                "cannot unify rigid type variable with concrete type constructor"
                                    .to_string(),
                            ),
                        }),

                        // Auto vs TyCon: substitute the Auto variable `x`
                        (TyVarName::Auto(_), TyExpr::TyCon(_)) => extend(subst, x, other),

                        // UserDefined vs UserDefined: must have the same name
                        (TyVarName::UserDefined(u1), TyExpr::TyVar(TyVarName::UserDefined(u2))) => {
                            if u1 == u2 {
                                Ok(subst.clone())
                            } else {
                                Err(TyError::TypeConflict {
                                    ty1: ty_expr1.clone(),
                                    ty2: ty_expr2.clone(),
                                    msg: Some("incompatible user-defined types".to_string()),
                                })
                            }
                        }

                        // UserDefined vs TyCon
                        (TyVarName::UserDefined(u), TyExpr::TyCon(con)) => {
                            if let TyCon::User(name) = con {
                                if let ConcreteToken::Iden(uname) = &u.token {
                                    if uname == name {
                                        return Ok(subst.clone());
                                    }
                                }
                            }
                            Err(TyError::TypeConflict {
                                ty1: ty_expr1.clone(),
                                ty2: ty_expr2.clone(),
                                msg: match con {
                                    TyCon::User(_) => {
                                        Some("incompatible user-defined types".to_string())
                                    }
                                    TyCon::Builtin(_) => Some(
                                        "cannot unify builtin type with user-defined type"
                                            .to_string(),
                                    ),
                                },
                            })
                        }

                        // Rigid vs Rigid
                        (TyVarName::Rigid(r1), TyExpr::TyVar(TyVarName::Rigid(r2))) => {
                            if r1 == r2 {
                                Ok(subst.clone())
                            } else {
                                Err(TyError::TypeConflict {
                                    ty1: ty_expr1.clone(),
                                    ty2: ty_expr2.clone(),
                                    msg: Some(
                                        "cannot unify different rigid type variables".to_string(),
                                    ),
                                })
                            }
                        }

                        // Rigid vs Auto: bind the flexible Auto variable to Rigid
                        (TyVarName::Rigid(_), TyExpr::TyVar(TyVarName::Auto(_))) => {
                            // symmetry
                            unify_ty_exprs(subst, ty_expr2, ty_expr1)
                        }
                        (TyVarName::Auto(_), TyExpr::TyVar(TyVarName::Rigid(_))) => {
                            extend(subst, x, other)
                        }

                        // Rigid vs Concrete type produces error
                        (TyVarName::Rigid(_), TyExpr::TyVar(TyVarName::UserDefined(_)))
                        | (TyVarName::UserDefined(_), TyExpr::TyVar(TyVarName::Rigid(_))) => {
                            Err(TyError::TypeConflict {
                                ty1: ty_expr1.clone(),
                                ty2: ty_expr2.clone(),
                                msg: Some(
                                    "cannot unify rigid type variable with concrete type"
                                        .to_string(),
                                ),
                            })
                        }

                        // Rigid vs type application produces error
                        (TyVarName::Rigid(_), TyExpr::TyApp(_)) => Err(TyError::TypeConflict {
                            ty1: ty_expr1.clone(),
                            ty2: ty_expr2.clone(),
                            msg: Some(
                                "cannot unify rigid type variable with type application"
                                    .to_string(),
                            ),
                        }),

                        // Auto vs. Concrete type: substitute the Auto variable
                        (TyVarName::UserDefined(_), TyExpr::TyVar(TyVarName::Auto(_))) => {
                            // mirror
                            unify_ty_exprs(subst, ty_expr2, ty_expr1)
                        }

                        (TyVarName::Auto(_), TyExpr::TyVar(TyVarName::UserDefined(_))) => {
                            extend(subst, x, other)
                        }

                        // concrete type vs type application: incompatible
                        (TyVarName::UserDefined(_), TyExpr::TyApp(_)) => {
                            Err(TyError::TypeConflict {
                                ty1: ty_expr1.clone(),
                                ty2: ty_expr2.clone(),
                                msg: Some(
                                    "cannot unify concrete type constructor with type application"
                                        .to_string(),
                                ),
                            })
                        }

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
        (TyExpr::TyCon(_), TyExpr::TyVar(_)) => unify_ty_exprs(subst, ty_expr2, ty_expr1),
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
    let other_resolved = subst_ty(subst, other);
    match &other_resolved {
        TyExpr::TyVar(tvn_other) if tvn == tvn_other => Ok(subst.clone()), // success
        _ => {
            if free_ty_vars(&other_resolved).contains(tvn) {
                return Err(TyError::InfiniteType {
                    var: tvn.clone(),
                    ty: other_resolved,
                    msg: Some("occurs check failed in substitution extension".to_string()),
                });
            }
            // compose {tvn -> other_resolved} on top of `subst`
            Ok(subst_compose(&subst_delta(tvn, &other_resolved), subst))
        }
    }
}

/// delta substition:
///   tvn => ty_expr
///   o/w => identity
pub(crate) fn subst_delta(tvn: &TyVarName, ty_expr: &TyExpr) -> Substitution {
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

/// unify two substitutions by unifying their mappings for common type variables
pub(crate) fn unify_substs(
    subst1: &Substitution,
    subst2: &Substitution,
) -> Result<Substitution, TyError> {
    let mut subst_combined = subst1.clone();
    for (tvn, ty2) in subst2.iter() {
        let ty1_after_subst = subst_ty(&subst_combined, &TyExpr::TyVar(tvn.clone()));
        let ty2_after_subst = subst_ty(&subst_combined, &ty2);
        subst_combined = unify_ty_exprs(&subst_combined, &ty1_after_subst, &ty2_after_subst)?;
    }
    Ok(subst_combined)
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
            alts: case
                .alts
                .into_iter()
                .map(|alt| TypedVCaseAlt {
                    pattern: apply_subst_typed_pattern(subst, alt.pattern),
                    guard: alt.guard.map(|guard| apply_subst_typed_expr(subst, guard)),
                    body: apply_subst_typed_expr(subst, alt.body),
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

fn check_no_escaped_rigid_vars(
    env: &EnvVVarToTyScheme,
    subst: &Substitution,
    outer_ty_vars: &BTreeMap<String, TyVarName>,
    inner_ty_vars: &BTreeMap<String, TyVarName>,
) -> Result<(), TyError> {
    // new_rigid_vars = inner_ty_vars \ outer_vars
    let newly_introduced_rigid_vars: BTreeSet<&TyVarName> = inner_ty_vars
        .iter()
        .filter(|(name, _)| !outer_ty_vars.contains_key(*name))
        .map(|(_, tvn)| tvn)
        .collect();
    // check if rigid variables escape to outside (if they coincide with free
    // type variables present in the type environment)
    if !newly_introduced_rigid_vars.is_empty() {
        let env_after = env.apply_subst_to_env(subst);
        for tvn in free_tvns_in_ty_env(&env_after) {
            if newly_introduced_rigid_vars.contains(&tvn) {
                return Err(TyError::TypeConflict {
                    ty1: TyExpr::TyVar(tvn.clone()),
                    ty2: TyExpr::TyVar(tvn.clone()), // [todo] fix / use another error variant
                    msg: Some(format!("rigid type variable {:?} escapes", tvn)),
                });
            }
        }
    }
    Ok(())
}

/// type check value-level expression with optional type annotation
///
/// we thread `local_ty_vars` through various ty_check_* functions where
/// `local_ty_vars` tracks user-written type variable names in lexical scope
/// (mapped to `Rigid` type variables only by construction) so we can support
/// scoped type variables, detect escaping local rigid variables [1],
/// and prevent inner `let` bindings from generalizing outer scoped type
/// variables [2]
///
/// where:
///   [1]: `f = \x -> (x :: a)` is not allowed since `x` has an outer Auto type
///        inference variable and the inner subexpression is not allowed to
///        specify Rigid type variable and bind to an outer inference variable
///   [2]: an inner binding is not allowed to steal an outer Rigid type variable
///        and make it generalizable (polymorphic)
pub(crate) fn ty_check_vexpr_typed(
    env_var_to_ty_scheme: &mut EnvVVarToTyScheme,
    ty_env: &TyConEnv,
    ty_var_ns: &mut TyVarNameSupply,
    vexpr_and_annot: &VExprAndTyAnnot,
    local_ty_vars: &BTreeMap<String, TyVarName>,
) -> Result<(Substitution, TypedVExpr), TyError> {
    let (vexpr, maybe_ty_expr) = vexpr_and_annot;

    let mut expr_ty_vars = local_ty_vars.clone();
    let maybe_annot_inst = match maybe_ty_expr {
        Some(ty_annot) => Some(instantiate_rigid_and_subst_ty_expr(
            ty_annot,
            ty_env,
            ty_var_ns,
            &mut expr_ty_vars,
        )?),
        None => None,
    };

    let (subst, typed_expr_raw) = match vexpr {
        VExpr::Abstraction(expr) => ty_check_abstraction_typed(
            env_var_to_ty_scheme,
            ty_env,
            ty_var_ns,
            expr,
            &expr_ty_vars,
        )?,
        VExpr::Application(expr) => ty_check_application_typed(
            env_var_to_ty_scheme,
            ty_env,
            ty_var_ns,
            expr,
            &expr_ty_vars,
        )?,
        VExpr::Case(expr) => {
            ty_check_case_typed(env_var_to_ty_scheme, ty_env, ty_var_ns, expr, &expr_ty_vars)?
        }
        VExpr::Let(expr) => {
            ty_check_let_typed(env_var_to_ty_scheme, ty_env, ty_var_ns, expr, &expr_ty_vars)?
        }

        VExpr::LitNumeric(lit) => {
            let (subst, ty) = ty_check_lit_numeric(env_var_to_ty_scheme, ty_var_ns, lit)?;
            (
                subst,
                TypedVExpr::LitNumeric(TypedVLitNumeric {
                    val: lit.clone(),
                    ty,
                }),
            )
        }
        VExpr::LitString(lit) => {
            let (subst, ty) = ty_check_lit_string(env_var_to_ty_scheme, ty_var_ns, lit)?;
            (
                subst,
                TypedVExpr::LitString(TypedVLitString {
                    val: lit.clone(),
                    ty,
                }),
            )
        }
        VExpr::Variable(var) => {
            let (subst, ty, ty_args) = ty_check_variable(env_var_to_ty_scheme, ty_var_ns, var)?;
            (
                subst,
                TypedVExpr::Variable(TypedVVariable {
                    var: var.clone(),
                    ty,
                    ty_args,
                    ty_schematic: env_var_to_ty_scheme.get(var).unwrap().clone(),
                }),
            )
        }
        VExpr::Constructor(expr) => ty_check_constructor_typed(
            env_var_to_ty_scheme,
            ty_env,
            ty_var_ns,
            expr,
            &expr_ty_vars,
        )?,
    };

    match maybe_annot_inst {
        Some(annot_inst) => {
            // if explicit type annotation is present, then check consistency
            // between the annotation and the inferred type
            let annot_resolved = subst_ty(&subst, &annot_inst);
            let subst_unified = unify_ty_exprs(&subst, &annot_resolved, typed_expr_raw.ty())?;

            check_no_escaped_rigid_vars(
                env_var_to_ty_scheme,
                &subst_unified,
                local_ty_vars,
                &expr_ty_vars,
            )?;

            let typed_expr = apply_subst_typed_expr(&subst_unified, typed_expr_raw);
            Ok((subst_unified, typed_expr))
        }
        None => Ok((subst, typed_expr_raw)),
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
    ty_env: &TyConEnv,
    ty_var_ns: &mut TyVarNameSupply,
    v_abstr_expr: &VAbstrExpr,
    local_ty_vars: &BTreeMap<String, TyVarName>,
) -> Result<(Substitution, TypedVExpr), TyError> {
    let mut env_lambda = env_var_to_ty_scheme.clone();
    let mut scoped_ty_vars = local_ty_vars.clone();
    let mut subst_params = subst_id();
    let mut typed_params: Vec<TypedVAbstrParam> = Vec::new();

    // process parameters
    for param in v_abstr_expr.params.iter() {
        env_lambda.apply_subst_to_env_in_place(&subst_params);

        let ty_binder = TyExpr::TyVar(ty_var_ns.generate());
        env_lambda.insert(
            param.binder.clone(),
            TyScheme {
                ty_vars_schematic: Vec::new(),
                ty_expr: Box::new(ty_binder.clone()),
            },
        );

        let (pattern_subst, _bound_vars, typed_pattern_raw) = ty_check_pattern_typed(
            &mut env_lambda,
            ty_env,
            ty_var_ns,
            &param.pattern,
            &mut scoped_ty_vars,
        )?;
        subst_params = subst_compose(&pattern_subst, &subst_params);

        env_lambda.apply_subst_to_env_in_place(&subst_params);

        let ty_binder_resolved = subst_ty(&subst_params, &ty_binder);
        let pattern_resolved = subst_ty(&subst_params, typed_pattern_raw.ty());
        subst_params = unify_ty_exprs(&subst_params, &ty_binder_resolved, &pattern_resolved)?;

        env_lambda.apply_subst_to_env_in_place(&subst_params);

        typed_params.push(TypedVAbstrParam {
            binder: param.binder.clone(),
            pattern: apply_subst_typed_pattern(&subst_params, typed_pattern_raw),
            ty: subst_ty(&subst_params, &ty_binder),
        });
    }

    // process body

    env_lambda.apply_subst_to_env_in_place(&subst_params);

    let (body_subst, typed_body_raw) = ty_check_vexpr_typed(
        &mut env_lambda,
        ty_env,
        ty_var_ns,
        &v_abstr_expr.body,
        &scoped_ty_vars,
    )?;

    let phi = subst_compose(&body_subst, &subst_params);
    check_no_escaped_rigid_vars(env_var_to_ty_scheme, &phi, local_ty_vars, &scoped_ty_vars)?;
    let typed_body = apply_subst_typed_expr(&phi, typed_body_raw);

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
    ty_env: &TyConEnv,
    ty_var_ns: &mut TyVarNameSupply,
    vexpr: &VAppExpr,
    local_ty_vars: &BTreeMap<String, TyVarName>,
) -> Result<(Substitution, TypedVExpr), TyError> {
    let (subst_fun, typed_fun) = ty_check_vexpr_typed(
        env_var_to_ty_scheme,
        ty_env,
        ty_var_ns,
        &vexpr.callable,
        local_ty_vars,
    )?;
    let mut ty_fun = typed_fun.ty().clone();
    let mut subst_acc = subst_fun.clone();
    let mut typed_vexpr_callable = apply_subst_typed_expr(&subst_fun, typed_fun);
    let mut typed_vexpr_args = Vec::new();

    for arg_and_annot in vexpr.args.iter() {
        let mut env_arg = env_var_to_ty_scheme.apply_subst_to_env(&subst_acc);

        let (subst_arg, typed_arg_raw) = ty_check_vexpr_typed(
            &mut env_arg,
            ty_env,
            ty_var_ns,
            arg_and_annot,
            local_ty_vars,
        )?;
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

/// helper struct used in `ty_check_case_typed`
struct AltInfo<'a> {
    body: &'a VExprAndTyAnnot,
    env: EnvVVarToTyScheme,
    typed_pattern: TypedVPattern,
    typed_guard: Option<TypedVExpr>,
    subst: Substitution,

    // for type variable sharing between pattern, guard, body of the alternative
    // and also detecting escaping rigid variable
    alt_ty_vars: BTreeMap<String, TyVarName>,
}

/// type check case expression with pattern matching
///
/// case e of
///   p1 -> e1
///   p2 -> e2
///   ...
///
///   - type check scrutinee e to get type t_scrutinee
///   - for each alt i (pattern p_i, body e_i, optional guard g_i):
///     - type check pattern p_i, binding variables into the alt environment
///     - unify scrutinee type with pattern type
///     - if guard exists, type check g_i and unify with Bool
///     - type check body e_i in the environment enriched with bindings
///   - unify all body types (all branches return the same type)
///   - return substitution and the unified body type
///
pub(crate) fn ty_check_case_typed(
    env_var_to_ty_scheme: &mut EnvVVarToTyScheme,
    ty_env: &TyConEnv,
    ty_var_ns: &mut TyVarNameSupply,
    vexpr: &VCaseExpr,
    local_ty_vars: &BTreeMap<String, TyVarName>,
) -> Result<(Substitution, TypedVExpr), TyError> {
    // type check argument/scrutinee
    let (phi, typed_scrutinee_raw) = ty_check_vexpr_typed(
        env_var_to_ty_scheme,
        ty_env,
        ty_var_ns,
        &vexpr.arg,
        local_ty_vars,
    )?;
    // carry scrutinee constraints forward
    // affect patterns and downstream typing
    let mut typed_scrutinee = apply_subst_typed_expr(&phi, typed_scrutinee_raw);

    let mut env_augmented = env_var_to_ty_scheme.apply_subst_to_env(&phi);

    // for each alt, get: (body expr, env, typed pattern, typed guard, substitution)
    let mut alt_infos: Vec<AltInfo<'_>> = Vec::new();
    for alt in vexpr.alts.iter() {
        let mut env_alt = env_augmented.clone();
        let mut alt_ty_vars = local_ty_vars.clone();
        let (subst_pattern, _bound_vars, typed_pattern) = ty_check_pattern_typed(
            &mut env_alt,
            ty_env,
            ty_var_ns,
            &alt.pattern,
            &mut alt_ty_vars,
        )?;

        // unify typed_scrutinee with alt's pattern
        let subst_pattern_unified =
            unify_ty_exprs(&subst_pattern, typed_scrutinee.ty(), typed_pattern.ty())?;

        env_alt.apply_subst_to_env_in_place(&subst_pattern_unified);

        let (subst_alt, typed_guard) = match &alt.guard {
            Some(guard_expr_and_annot) => {
                let (subst_guard, typed_guard_raw) = ty_check_vexpr_typed(
                    &mut env_alt,
                    ty_env,
                    ty_var_ns,
                    guard_expr_and_annot,
                    &alt_ty_vars,
                )?;
                let mut subst_alt = subst_compose(&subst_guard, &subst_pattern_unified);
                let typed_guard_subst = apply_subst_typed_expr(&subst_alt, typed_guard_raw);
                let subst_guard_bool =
                    unify_ty_exprs(&subst_alt, typed_guard_subst.ty(), &mk_ty_bool())?;
                subst_alt = subst_guard_bool;

                env_alt.apply_subst_to_env_in_place(&subst_alt);

                (subst_alt, Some(typed_guard_subst))
            }
            _ => (subst_pattern_unified, None),
        };

        // propagate case alternative's substitution to env_augmented and
        // typed_scrutinee so that subsequent alternatives observe the type
        // constraints
        env_augmented.apply_subst_to_env_in_place(&subst_alt);
        typed_scrutinee = apply_subst_typed_expr(&subst_alt, typed_scrutinee);

        alt_infos.push(AltInfo {
            body: &alt.body,
            env: env_alt,
            typed_pattern,
            typed_guard,
            subst: subst_alt,
            alt_ty_vars,
        });
    }

    // pairwise unify case alternative substitutions using `unify_substs` and
    // produce the combined substitution
    let mut subst_patterns_unified = subst_id();
    for alt_info in alt_infos.iter() {
        subst_patterns_unified = unify_substs(&subst_patterns_unified, &alt_info.subst)?;
    }

    // apply substitution to scrutinee
    let mut ty_scrutinee_updated = subst_ty(&subst_patterns_unified, typed_scrutinee.ty());

    // enforce a single scrutinee type across all alt patterns
    // wildcard/variable patterns are typed as fresh auto type variables so they should be compatible
    for alt_info in alt_infos.iter() {
        let ty_pat = subst_ty(&subst_patterns_unified, alt_info.typed_pattern.ty());
        subst_patterns_unified =
            unify_ty_exprs(&subst_patterns_unified, &ty_scrutinee_updated, &ty_pat)?;
        ty_scrutinee_updated = subst_ty(&subst_patterns_unified, typed_scrutinee.ty());
    }

    // [todo] check case expression coverage over its matching patterns

    // type check each alt's body
    let mut subst_bodies = subst_id();
    let mut typed_bodies = Vec::new();
    {
        for alt_info in alt_infos.iter_mut() {
            // propagate unified substitutions, related to pattern and guard, into the case
            // alternative environment
            alt_info
                .env
                .apply_subst_to_env_in_place(&subst_patterns_unified);

            alt_info.env.apply_subst_to_env_in_place(&subst_bodies);

            let (subst_body, typed_body) = ty_check_vexpr_typed(
                &mut alt_info.env,
                ty_env,
                ty_var_ns,
                alt_info.body,
                &alt_info.alt_ty_vars,
            )?;
            subst_bodies = subst_compose(&subst_body, &subst_bodies);
            typed_bodies.push(typed_body);
        }
    }

    // unify types of bodies
    let first_body_ty = typed_bodies
        .first()
        .ok_or_else(|| TyError::Unexpected("case expr missing an alt body".to_string()))?
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
    for alt_info in alt_infos.iter() {
        check_no_escaped_rigid_vars(
            env_var_to_ty_scheme,
            &subst_final,
            local_ty_vars,
            &alt_info.alt_ty_vars,
        )?;
    }
    let typed_arg = apply_subst_typed_expr(&subst_final, typed_scrutinee);
    let ty_result = subst_ty(&subst_final, &first_body_ty);
    if alt_infos.len() != typed_bodies.len() {
        return Err(TyError::Unexpected(
            format_args!(
                "case expr body count mismatch: alt_infos={:?} typed_bodies={:?}",
                alt_infos.len(),
                typed_bodies.len()
            )
            .to_string(),
        ));
    }
    let typed_alts: Vec<TypedVCaseAlt> = alt_infos
        .into_iter()
        .zip(typed_bodies)
        .map(|(info, body)| TypedVCaseAlt {
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
            alts: typed_alts,
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
    ty_env: &TyConEnv,
    ty_var_ns: &mut TyVarNameSupply,
    // signatures are kept as `Option<TyExpr>` (rather than pre-generalized `TyScheme`)
    // so user-written type variable names remain available for lexical scoping over each RHS
    defs: &[(VPattern, VExpr, Option<TyExpr>)],
    local_ty_vars: &BTreeMap<String, TyVarName>,
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
            // - `_annot` is intentionally ignored here: recursive call sites start with
            //   `ty_args = []` against this monomorphic `Auto` placeholder and are backfilled
            //   after SCC generalization
            env_v_var_to_ty_scheme_binding_seed.insert(
                pat_var.clone(),
                TyScheme {
                    ty_vars_schematic: Vec::new(),
                    ty_expr: Box::new(TyExpr::TyVar(ty_var_ns.generate())),
                },
            );
            // map each binder to its definition
            map_binding_to_def_group.insert(pat_var.clone(), idx);
        }
    }

    // compute mutually dependent definition groups
    let mut scc_groups: Vec<BTreeSet<usize>> = Vec::new();
    {
        let mut map_def_to_previsit = BTreeMap::new();
        let mut map_def_to_earliest = BTreeMap::new();
        let mut map_def_to_scc = BTreeMap::new();
        let mut stack = Vec::new();
        let mut generate_previsit = 0;

        // extract free value-level variables in each RHS and retain only binders from
        // this let group
        //
        // return definition indices that the current definition depends on
        //
        // definitions with no in-group recursive references yield an empty neighbor set
        let neighbours = |idx| -> Vec<usize> {
            let (_pat, def_expr, _ty_annot): &(VPattern, VExpr, Option<TyExpr>) = &defs[idx];
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
            let (pattern, def_expr, optional_annot): &(VPattern, VExpr, Option<TyExpr>) =
                &defs[*idx];

            env_v_var_to_ty_scheme_binding_seed.apply_subst_to_env_in_place(&subst_accum);

            // scoped type variables:
            //   - instantiate free user-defined type vars in pattern
            //     annotations and `optional_annot` as `Rigid` vars in
            //     `def_ty_vars` before checking `def_expr`
            //   - this allows type annotations inside `def_expr` to reference
            //     and share the same rigid vars
            // monomorphic recursion:
            //   - in-group recursive calls inside `def_expr` look up binders in
            //     `env_v_var_to_ty_scheme_binding_seed`, unifying against
            //     their shared `TyAuto` placeholders rather than a generalized
            //     polymorphic scheme
            let mut def_ty_vars = local_ty_vars.clone();

            let (subst, bound_vvars_in_pattern, typed_binding_expr) =
                ty_check_pattern_typed_with_seeded_binders(
                    env_v_var_to_ty_scheme_binding_seed,
                    &original_seeded_lhs_binders,
                    ty_env,
                    ty_var_ns,
                    pattern,
                    &mut def_ty_vars,
                )?;
            subst_accum = subst_compose(&subst, &subst_accum);

            let mut typed_binding_expr =
                apply_subst_typed_pattern(&subst_accum, typed_binding_expr);

            // typecheck RHS (enforcing explicit signature annotation if present)
            env_v_var_to_ty_scheme_binding_seed.apply_subst_to_env_in_place(&subst_accum);

            let maybe_annot_inst = match optional_annot {
                Some(ty_annot) => Some(instantiate_rigid_and_subst_ty_expr(
                    ty_annot,
                    ty_env,
                    ty_var_ns,
                    &mut def_ty_vars,
                )?),
                None => None,
            };

            let (subst, typed_rhs_vexpr) = ty_check_vexpr_typed(
                env_v_var_to_ty_scheme_binding_seed,
                ty_env,
                ty_var_ns,
                &(def_expr.clone(), None),
                &def_ty_vars,
            )?;
            subst_accum = subst_compose(&subst, &subst_accum);

            if let Some(annot_inst) = maybe_annot_inst {
                let annot_resolved = subst_ty(&subst_accum, &annot_inst);
                subst_accum = unify_ty_exprs(&subst_accum, &annot_resolved, typed_rhs_vexpr.ty())?;
            }

            // unify LHS with RHS
            subst_accum =
                unify_ty_exprs(&subst_accum, typed_binding_expr.ty(), typed_rhs_vexpr.ty())?;

            // check escape against `env_outer` (not `env_v_var_to_ty_scheme_binding_seed`)
            // because in-group seeded placeholders in `env_v_var_to_ty_scheme_binding_seed`
            // are expected to unify with signature `Rigid` vars during recursive SCC checking
            check_no_escaped_rigid_vars(env_outer, &subst_accum, local_ty_vars, &def_ty_vars)?;

            // keep the typed binding synchronized with the latest substitution
            // without this, pattern ty metadata can lag behind rhs/scheme typing
            typed_binding_expr = apply_subst_typed_pattern(&subst_accum, typed_binding_expr);
            let typed_rhs_vexpr = apply_subst_typed_expr(&subst_accum, typed_rhs_vexpr);

            match pattern.peel_typed() {
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

        // synchronize all typed pairs in this SCC with the final accumulated substitution
        for idx in scc.iter() {
            if let Some(entry) = typed_binding_def_pairs.get_mut(idx) {
                match entry {
                    TypedLhsRhsPair::SimpleVarBindingPair((pat, rhs)) => {
                        *pat = apply_subst_typed_pattern(&subst_accum, pat.clone());
                        *rhs = apply_subst_typed_expr(&subst_accum, rhs.clone());
                    }
                    TypedLhsRhsPair::NonSimpleVarBindingPair((pat, _, rhs)) => {
                        *pat = apply_subst_typed_pattern(&subst_accum, pat.clone());
                        *rhs = apply_subst_typed_expr(&subst_accum, rhs.clone());
                    }
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
            env_outer_copy.apply_subst_to_env_in_place(&subst_accum);

            // `free(env)` for HM generalization:
            // type vars in scheme bodies that are not bound by the scheme,
            // plus any rigid type variables in lexical scope (`local_ty_vars`)
            free_tvns_in_ty_env(&env_outer_copy)
                .into_iter()
                .chain(local_ty_vars.values().cloned())
                .collect()
        };

        // scc-wide map from generalized unification vars (both `Auto` and signature-introduced
        // `Rigid` vars not in `free_ty_vars_in_env`) to canonical `Auto` scheme vars
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

                    let free_ty_vars_in_def = free_ty_vars(typed_def_vexpr.ty());
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
                    let ty_vars_generalizable: Vec<_> = free_ty_vars(typed_def_vexpr.ty())
                        .into_iter()
                        .filter(|tvn| !free_ty_vars_in_env.contains(tvn))
                        .collect();
                    let scheme_ty_vars: Vec<_> = ty_vars_generalizable
                        .iter()
                        .map(|tvn| {
                            scc_scheme_var_map.get(tvn).cloned().ok_or_else(|| {
                                TyError::InternalError(
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
    ty_env: &TyConEnv,
    ty_var_ns: &mut TyVarNameSupply,
    vexpr: &VLetExpr,
    local_ty_vars: &BTreeMap<String, TyVarName>,
) -> Result<(Substitution, TypedVExpr), TyError> {
    // used to compute free variables in environment
    // this is needed to determine the set of generalizable type variables which become schematic type variables in type schemes
    let mut env_outer = env_var_to_ty_scheme.clone();

    // environment accumulating info as type-checking progresses
    let mut env_working = env_var_to_ty_scheme.clone();

    let (mut subst_accum, groups_of_typed_binding_def_pairs) = ty_check_binding_group(
        &mut env_working,
        &mut env_outer,
        ty_env,
        ty_var_ns,
        &vexpr.defs,
        local_ty_vars,
    )?;

    // typecheck body of let expression using accumulated env and substitutions
    env_working.apply_subst_to_env_in_place(&subst_accum);

    let (subst_body, typed_body_expr) = ty_check_vexpr_typed(
        &mut env_working,
        ty_env,
        ty_var_ns,
        &vexpr.body,
        local_ty_vars,
    )?;

    subst_accum = subst_compose(&subst_body, &subst_accum);
    let typed_body_expr = apply_subst_typed_expr(&subst_accum, typed_body_expr);
    let ty_body = typed_body_expr.ty().clone();

    let mut typed_defs = Vec::new();
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

    for (typed_pat, typed_rhs) in &typed_defs {
        validate_no_unboxed_let_binding(typed_pat, typed_rhs)?;
        validate_pattern_no_unboxed_vars(typed_pat)?;
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

// we follow the convention that let binding are always on the heap
fn validate_no_unboxed_let_binding(pat: &TypedVPattern, rhs: &TypedVExpr) -> Result<(), TyError> {
    if rhs.ty().is_unboxed_primitive() {
        return Err(TyError::UnboxedLetBinding {
            ty: rhs.ty().clone(),
            msg: Some(format!(
                "unboxed primitive type `{}` cannot be bound in let expression; use boxed ADT instead",
                rhs.ty().to_doc().to_string()
            )),
        });
    }
    Ok(())
}

// [todo]: check if this is necesary
fn validate_pattern_no_unboxed_vars(pat: &TypedVPattern) -> Result<(), TyError> {
    match pat {
        TypedVPattern::Variable { binder, ty, .. } => {
            if ty.is_unboxed_primitive() {
                return Err(TyError::UnboxedLetBinding {
                    ty: ty.clone(),
                    msg: Some(format!(
                        "cannot bind unboxed primitive type `{}` to variable `{}` in let expression; use boxed ADT instead",
                        ty.to_doc().to_string(),
                        binder.to_doc().to_string()
                    )),
                });
            }
        }
        // non-variable patterns bind no variables into the environment or lazy
        // thunks, so matching unboxed fields inside boxed constructors is ok
        TypedVPattern::Wild { .. }
        | TypedVPattern::Literal { .. }
        | TypedVPattern::Range { .. } => {}
        TypedVPattern::Constructor { args, .. } => {
            for arg in args {
                validate_pattern_no_unboxed_vars(arg)?;
            }
        }
        TypedVPattern::Record { fields, .. } => {
            for (_, field_pat) in fields {
                validate_pattern_no_unboxed_vars(field_pat)?;
            }
        }
    }
    Ok(())
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
    ty_env: &TyConEnv,
    ty_var_ns: &mut TyVarNameSupply,
    vexpr: &VConstructorExpr,
    local_ty_vars: &BTreeMap<String, TyVarName>,
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

    let (positional_args, record_order): (Vec<&VExprAndTyAnnot>, Option<Vec<String>>) =
        if let Some(rec) = &qualified_ctor.record_fields {
            // case: a record => map fields to positions

            let expected_field_names = ctor_def.field_names.as_ref().ok_or_else(|| {
                TyError::Unexpected(
                    format!(
                        "Constructor {} is not a record, but record syntax was used",
                        qualified_ctor.constructor
                    )
                    .to_string(),
                )
            })?;
            use std::collections::HashMap;
            // collect field name -> (value expr, optional type expr)
            let mut vexpr_fieldname_mapping: HashMap<&String, &VExprAndTyAnnot> = HashMap::new();
            for (field_name, vexpr_and_ty_annot) in rec.iter() {
                if vexpr_fieldname_mapping
                    .insert(field_name, vexpr_and_ty_annot)
                    .is_some()
                {
                    return Err(TyError::Unexpected(
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
                let annot = vexpr_fieldname_mapping.get(fname).ok_or_else(|| {
                    TyError::Unexpected(
                        format!(
                            "Missing field {} in record constructor {}",
                            fname, qualified_ctor.constructor
                        )
                        .to_string(),
                    )
                })?;
                ordered.push(*annot);
            }
            if vexpr_fieldname_mapping.len() != expected_field_names.len() {
                return Err(TyError::Unexpected(
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
            (qualified_ctor.args.iter().collect(), None)
        };

    let mut subst = subst_id();
    let expected_field_len = expected_field_types.len();
    let mut remaining_expected_field_types = expected_field_types;
    let mut typed_args = Vec::new();

    // type infer and check each positional argument
    for arg_and_annot in positional_args.iter() {
        if remaining_expected_field_types.is_empty() {
            return Err(TyError::ArityMismatch {
                constructor: qualified_ctor.constructor.clone(),
                expected: expected_field_len,
                got: positional_args.len(),
                msg: Some("over-applied constructor expression".to_string()),
            });
        }

        let expected = remaining_expected_field_types.remove(0);

        let mut env_with_subst = env_var_to_ty_scheme.apply_subst_to_env(&subst);

        let (arg_subst, typed_arg_raw) = ty_check_vexpr_typed(
            &mut env_with_subst,
            ty_env,
            ty_var_ns,
            *arg_and_annot,
            local_ty_vars,
        )?;
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
                Some(_) => Ok((subst_id(), mk_ty_unboxed_int64())),
                None => Err(TyError::Unexpected(
                    format!("type checking literal (int) failed for {:?}", vexpr).to_string(),
                )),
            }
        }
        NumericLiteralValue::Float { parsed, raw } => {
            match parsed.to_owned().or_else(|| raw.parse::<f64>().ok()) {
                Some(_) => Ok((subst_id(), mk_ty_unboxed_float64())),
                None => Err(TyError::Unexpected(
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
        ConcreteToken::LiteralString(_string) => Ok((subst_id(), mk_ty_bytes())),
        _ => Err(TyError::Unexpected(
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
    let type_scheme = env_var_to_ty_scheme.get(vexpr).ok_or_else(|| {
        let (token, loc) = match vexpr {
            VVar::Named(named) => (&named.token, &named.loc),
            VVar::Renamed(named_uniqued) => {
                (&named_uniqued.original.token, &named_uniqued.original.loc)
            }
            VVar::Anon(id) => return TyError::UnboundVariable(format!("anon_{id}")),
        };
        match loc {
            Some(loc) => TyError::UnboundVariable(format!("`{token}` at {loc:?}")),
            None => TyError::UnboundVariable(format!("`{token}`")),
        }
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
    ty_env: &TyConEnv,
    ty_var_ns: &mut TyVarNameSupply,
    pattern: &VPattern,
    // thread this parameter through due to use of it by some inner functions
    // such as `instantiate_rigid_and_subst_ty_expr`
    scoped_ty_vars: &mut BTreeMap<String, TyVarName>,
) -> Result<(Substitution, Vec<VVar>, TypedVPattern), TyError> {
    let original_seeded_lhs_binders: BTreeSet<VVar> = BTreeSet::new();
    ty_check_pattern_typed_with_seeded_binders(
        env,
        &original_seeded_lhs_binders,
        ty_env,
        ty_var_ns,
        pattern,
        scoped_ty_vars,
    )
}

/// same as `ty_check_pattern_typed`, but reuses placeholder types for binders
/// that were seeded before typing (e.g. let-group LHS binders).
pub(crate) fn ty_check_pattern_typed_with_seeded_binders(
    env: &mut EnvVVarToTyScheme,
    original_seeded_lhs_binders: &BTreeSet<VVar>,
    ty_env: &TyConEnv,
    ty_var_ns: &mut TyVarNameSupply,
    pattern: &VPattern,
    scoped_ty_vars: &mut BTreeMap<String, TyVarName>,
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
                    ty_vars_schematic: Vec::new(),
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
            let (subst, typed_expr) = ty_check_vexpr_typed(
                &mut env_clone,
                ty_env,
                ty_var_ns,
                &(vexpr, None),
                &BTreeMap::new(),
            )?;
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
            let (subst_start, typed_start) = ty_check_vexpr_typed(
                &mut env_clone,
                ty_env,
                ty_var_ns,
                &(v_expr_start, None),
                &BTreeMap::new(),
            )?;

            env_clone.apply_subst_to_env_in_place(&subst_start);

            let (subst_end, typed_end) = ty_check_vexpr_typed(
                &mut env_clone,
                ty_env,
                ty_var_ns,
                &(v_expr_end, None),
                &BTreeMap::new(),
            )?;
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
                    msg: Some("pattern constructor arity mismatch".to_string()),
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
                    scoped_ty_vars,
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
                TyError::Unexpected(
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
                    return Err(TyError::Unexpected(
                        format!(
                            "Duplicate field {} in record pattern {}",
                            field_name, constructor
                        )
                        .to_string(),
                    ));
                }
                seen.insert(field_name);

                let expected_field_ty = expected_field_map.get(field_name).ok_or_else(|| {
                    TyError::Unexpected(
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
                    scoped_ty_vars,
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
                    return Err(TyError::Unexpected(
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
        VPattern::Typed { pattern, ty } => {
            // recursive call to wrapped pattern
            let (subst_pat, bound_vars, typed_pat_raw) =
                ty_check_pattern_typed_with_seeded_binders(
                    env,
                    original_seeded_lhs_binders,
                    ty_env,
                    ty_var_ns,
                    pattern,
                    scoped_ty_vars,
                )?;
            env.apply_subst_to_env_in_place(&subst_pat);

            let annot_inst =
                instantiate_rigid_and_subst_ty_expr(ty, ty_env, ty_var_ns, scoped_ty_vars)?;
            let annot_resolved = subst_ty(&subst_pat, &annot_inst);
            let pat_ty_resolved = subst_ty(&subst_pat, typed_pat_raw.ty());
            let subst_unified = unify_ty_exprs(&subst_pat, &pat_ty_resolved, &annot_resolved)?;

            env.apply_subst_to_env_in_place(&subst_unified);

            Ok((
                subst_unified.clone(),
                bound_vars,
                apply_subst_typed_pattern(&subst_unified, typed_pat_raw),
            ))
        }
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
///     - for each alt, drop binders from the alt pattern to handle
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
            let alts = case_expr
                .alts
                .into_iter()
                .map(|alt| {
                    let mut shadowed_binders = Vec::new();
                    collect_pattern_binders(&alt.pattern, &mut shadowed_binders);
                    // filter shadowed binders before recursing into alt
                    let alt_scheme_info_for_scc =
                        schematic_info_without_binders(scheme_info_for_scc, &shadowed_binders);
                    Ok(TypedVCaseAlt {
                        pattern: alt.pattern,
                        guard: alt
                            .guard
                            .map(|guard| fill_missing_ty_args(guard, &alt_scheme_info_for_scc))
                            .transpose()?,
                        body: fill_missing_ty_args(alt.body, &alt_scheme_info_for_scc)?,
                    })
                })
                .collect::<Result<Vec<_>, _>>()?;
            Ok(TypedVExpr::Case(TypedVCaseExpr {
                arg,
                alts,
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
                //
                // note: `scheme` comes from the SCC generalization pass where `scc_subst` has
                // already rewritten any generalized `Rigid` signature vars into canonical `Auto`
                // scheme vars on both `scheme.ty_expr` and this call-site `ty`

                // sanity check for matching the binder scheme against
                // this call-site type
                let instantiation_subst = unify_ty_exprs(&subst_id(), &scheme.ty_expr, &ty)
                    .map_err(|_| TyError::TypeConflict {
                        ty1: (*scheme.ty_expr).clone(),
                        ty2: ty.clone(),
                        msg: Some(format!("unification error for binder `{:?}`", var)),
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
                    return Err(TyError::Unexpected(
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
    type_env: &'a TyConEnv,
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
    type_env: &'a TyConEnv,
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
    type_env: &TyConEnv,
    ns: &mut TyVarNameSupply,
    subst: &mut Substitution,
    pattern: &VPattern,
    ty_expected: &TyExpr,
    scoped_ty_vars: &mut BTreeMap<String, TyVarName>,
) -> Result<(Vec<VVar>, TypedVPattern), TyError> {
    env.apply_subst_to_env_in_place(subst);

    let (pattern_subst, bound_vars, typed_pattern_expr) =
        ty_check_pattern_typed_with_seeded_binders(
            env,
            original_seeded_lhs_binders,
            type_env,
            ns,
            pattern,
            scoped_ty_vars,
        )?;

    *subst = subst_compose(&pattern_subst, subst);

    env.apply_subst_to_env_in_place(subst);

    let ty_pattern = typed_pattern_expr.ty().clone();
    let unified = unify_ty_exprs(subst, &ty_pattern, ty_expected)?;
    *subst = unified;

    env.apply_subst_to_env_in_place(subst);

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
        VPattern::Typed { pattern, .. } => {
            // apply method to wrapped pattern
            collect_all_pattern_variables_aux(ret, pattern)?;
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

/// collect unique `TyVarName::UserDefined` type variable names (excluding
/// registered ADTs in TyConEnv) in left-to-right appearance order
///
/// we also validate that all type constructors and builtin types are applied
/// with the correct arity
///
/// expects `ty` to be canonicalized via `canonicalize_ty_cons`
pub(crate) fn collect_user_defined_ty_var_names(
    ty: &TyExpr,
    te: &TyConEnv,
) -> Result<Vec<String>, TyError> {
    fn collect(
        ty: &TyExpr,
        te: &TyConEnv,
        seen: &mut BTreeSet<String>,
        ordered: &mut Vec<String>,
    ) -> Result<(), TyError> {
        let (head, args) = collect_spine(ty.clone());
        let num_args = args.len();
        match head {
            TyExpr::TyCon(con) => {
                te.validate_and_resolve_ty_con(con.name(), num_args)?;
            }
            TyExpr::TyVar(TyVarName::UserDefined(u)) => {
                if let ConcreteToken::Iden(name) = &u.token {
                    if is_type_constructor_name(name) {
                        return Err(TyError::UnknownType(name.clone()));
                    }
                    if num_args > 0 {
                        return Err(TyError::ArityMismatch {
                            constructor: name.clone(),
                            expected: 0,
                            got: num_args,
                            msg: Some(format!("type variable `{name}` cannot take arguments")),
                        });
                    }
                    if !seen.contains(name) {
                        seen.insert(name.clone());
                        ordered.push(name.clone());
                    }
                }
            }
            TyExpr::TyVar(_) => {
                if num_args > 0 {
                    return Err(TyError::ArityMismatch {
                        constructor: head.to_doc().to_string(),
                        expected: 0,
                        got: num_args,
                        msg: Some("type variable cannot take arguments".to_string()),
                    });
                }
            }
            TyExpr::TyApp(_) => unreachable!("collect_spine strips all TyApp"),
        }

        for arg in args {
            collect(&arg, te, seen, ordered)?;
        }
        Ok(())
    }

    let mut seen = BTreeSet::new();
    let mut ordered = Vec::new();
    collect(ty, te, &mut seen, &mut ordered)?;
    Ok(ordered)
}

/// substitute user-defined type variables whose token names exist in `var_map`
/// with the corresponding fresh type variable mapped by `var_map`
pub(crate) fn subst_user_defined_ty_vars(
    ty: &TyExpr,
    var_map: &BTreeMap<String, TyVarName>,
) -> TyExpr {
    if var_map.is_empty() {
        return ty.clone();
    }
    match ty {
        TyExpr::TyVar(TyVarName::UserDefined(u)) => {
            if let ConcreteToken::Iden(name) = &u.token {
                if let Some(target) = var_map.get(name) {
                    return TyExpr::TyVar(target.clone());
                }
            }
            ty.clone()
        }
        TyExpr::TyVar(_) => ty.clone(),
        TyExpr::TyCon(_) => ty.clone(),
        TyExpr::TyApp(app) => ty_app(
            subst_user_defined_ty_vars(&app.ty_func, var_map),
            subst_user_defined_ty_vars(&app.ty_arg, var_map),
        ),
    }
}

/// canonicalize any user-defined type variable identifiers that name
/// registered ADTs or builtin types into first-class `TyCon` constructors
pub(crate) fn canonicalize_ty_cons(te: &TyConEnv, ty: &TyExpr) -> TyExpr {
    match ty {
        TyExpr::TyCon(_) => ty.clone(),
        TyExpr::TyVar(TyVarName::UserDefined(u)) => {
            if let ConcreteToken::Iden(name) = &u.token {
                if let Some(b) = resolve_builtin_ty_con(name) {
                    return TyExpr::TyCon(TyCon::Builtin(b));
                }
                if te.has_adt(name) {
                    return TyExpr::TyCon(TyCon::User(name.clone()));
                }
            }
            ty.clone()
        }
        TyExpr::TyVar(_) => ty.clone(),
        TyExpr::TyApp(app) => ty_app(
            canonicalize_ty_cons(te, &app.ty_func),
            canonicalize_ty_cons(te, &app.ty_arg),
        ),
    }
}

/// instantiate user-defined type variables in `ty` with fresh rigid type variables and
/// return the resulting substituted `TyExpr`
///
/// this internally canonicalizes `ty`
///
/// updates `var_map` with fresh rigid type variables for newly encountered names while
/// reusing existing entries, allowing callers to scope identical names across
/// multiple annotations (e.g. in function signatures, let bindings, and lambda abstractions).
///
/// note: rigid type variable acts like a universal generic type
pub(crate) fn instantiate_rigid_and_subst_ty_expr(
    ty: &TyExpr,
    te: &TyConEnv,
    ns: &mut TyVarNameSupply,
    var_map: &mut BTreeMap<String, TyVarName>,
) -> Result<TyExpr, TyError> {
    let canonical = canonicalize_ty_cons(te, ty);
    for name in collect_user_defined_ty_var_names(&canonical, te)? {
        // use `Rigid` rather than flexible `Auto` vars so user-annotated
        // type variables cannot unify with concrete types or other distinct
        // rigid vars
        var_map.entry(name).or_insert_with(|| ns.generate_rigid());
    }
    Ok(subst_user_defined_ty_vars(&canonical, var_map))
}

/// build a type scheme from a `ty_expr` type annotation by generalizing free
/// user-defined type variables that are not ADT names in the type environment
///
/// this internally canonicalizes `ty_expr`
pub(crate) fn build_scheme_from_ty_expr(
    ty_expr: &TyExpr,
    ty_env: &TyConEnv,
    ns: &mut TyVarNameSupply,
) -> Result<TyScheme, TyError> {
    let canonical = canonicalize_ty_cons(ty_env, ty_expr);
    let var_names = collect_user_defined_ty_var_names(&canonical, ty_env)?;
    let mut var_map = BTreeMap::new();
    let mut ty_vars_schematic = Vec::new();

    for name in var_names {
        let fresh = ns.generate();
        ty_vars_schematic.push(fresh.clone());
        var_map.insert(name, fresh);
    }

    let ty_expr_generalized = subst_user_defined_ty_vars(&canonical, &var_map);

    Ok(TyScheme {
        ty_vars_schematic,
        ty_expr: Box::new(ty_expr_generalized),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- test helpers ---

    fn mk_lit_int(n: i64) -> VExpr {
        VExpr::LitNumeric(VLitNumeric {
            token: ConcreteToken::LiteralNumeric(n.to_string()),
            loc: None,
            value: NumericLiteralValue::Int {
                raw: n.to_string(),
                parsed: Some(n),
            },
        })
    }

    fn check_let(
        pat: VPattern,
        rhs: VExpr,
        annot: Option<TyExpr>,
    ) -> Result<(Substitution, TypedVExpr), TyError> {
        let mut env = EnvVVarToTyScheme::new();
        let ty_env = TyConEnv::new_with_prelude();
        let mut ns = TyVarNameSupply::new();
        let let_expr = VLetExpr {
            defs: vec![(pat, rhs, annot)],
            body: Box::new((mk_lit_int(0), None)),
        };
        ty_check_let_typed(&mut env, &ty_env, &mut ns, &let_expr, &BTreeMap::new())
    }

    // --- unification tests ---

    #[test]
    fn test_unify_type_conflicts_and_bridging() {
        let subst = subst_id();
        let ty_int = mk_ty_unboxed_int64();
        let ty_bytes = mk_ty_bytes();
        let ty_user = mk_ty_user("Foo");
        let ty_user_foo = TyExpr::TyCon(TyCon::User("Foo".to_string()));
        let ty_user_bar = TyExpr::TyCon(TyCon::User("Bar".to_string()));

        // incompatible builtins
        assert!(matches!(
            unify_ty_exprs(&subst, &ty_int, &ty_bytes),
            Err(TyError::TypeConflict { .. })
        ));

        // user-defined bridging to matching TyCon::User succeeds
        assert!(unify_ty_exprs(&subst, &ty_user, &ty_user_foo).is_ok());

        // user-defined vs distinct TyCon::User fails
        assert!(matches!(
            unify_ty_exprs(&subst, &ty_user, &ty_user_bar),
            Err(TyError::TypeConflict { .. })
        ));

        // user-defined vs TyCon::Builtin fails (tested symmetrically)
        assert!(matches!(
            unify_ty_exprs(&subst, &ty_user, &ty_int),
            Err(TyError::TypeConflict { .. })
        ));
        assert!(matches!(
            unify_ty_exprs(&subst, &ty_int, &ty_user),
            Err(TyError::TypeConflict { .. })
        ));
    }

    #[test]
    fn test_unify_concrete_vs_type_app_yields_type_conflict() {
        let subst = subst_id();
        let ty_int = mk_ty_unboxed_int64();
        let ty_app = mk_ty_arrow(mk_ty_unboxed_int64(), mk_ty_unboxed_int64());

        assert!(matches!(
            unify_ty_exprs(&subst, &ty_int, &ty_app),
            Err(TyError::TypeConflict { .. })
        ));
        assert!(matches!(
            unify_ty_exprs(&subst, &ty_app, &ty_int),
            Err(TyError::TypeConflict { .. })
        ));
    }

    #[test]
    fn test_unify_occurs_check_yields_infinite_type() {
        let subst = subst_id();
        let var_a = TyVarName::Auto(100);
        let var_b = TyVarName::Auto(101);
        let ty_a = TyExpr::TyVar(var_a.clone());
        let ty_b = TyExpr::TyVar(var_b.clone());

        // direct cycle: a ~ (a -> Int64#)
        assert!(matches!(
            unify_ty_exprs(&subst, &ty_a, &mk_ty_arrow(ty_a.clone(), mk_ty_unboxed_int64())),
            Err(TyError::InfiniteType { var, .. }) if var == var_a
        ));

        // indirect cycle: b ~ (a -> Int64#), then a ~ b
        let subst1 = unify_ty_exprs(
            &subst,
            &ty_b,
            &mk_ty_arrow(ty_a.clone(), mk_ty_unboxed_int64()),
        )
        .unwrap();
        assert!(matches!(
            unify_ty_exprs(&subst1, &ty_a, &ty_b),
            Err(TyError::InfiniteType { var, .. }) if var == var_a
        ));
    }

    #[test]
    fn test_unify_substs() {
        let var_a = TyVarName::Auto(100);
        let var_b = TyVarName::Auto(101);

        // compatible: a -> b and b -> Int64#
        let s_comp1 = subst_delta(&var_a, &TyExpr::TyVar(var_b.clone()));
        let s_comp2 = subst_delta(&var_b, &mk_ty_unboxed_int64());
        let res = unify_substs(&s_comp1, &s_comp2).unwrap();
        let is_int64 = |t: &TyExpr| matches!(t, TyExpr::TyCon(TyCon::Builtin(TyConBuiltin::Int64)));
        assert!(is_int64(&subst_ty(&res, &TyExpr::TyVar(var_a.clone()))));
        assert!(is_int64(&subst_ty(&res, &TyExpr::TyVar(var_b))));

        // conflict: a -> Int64# vs a -> Bytes#
        let s_conf1 = subst_delta(&var_a, &mk_ty_unboxed_int64());
        let s_conf2 = subst_delta(&var_a, &mk_ty_bytes());
        assert!(matches!(
            unify_substs(&s_conf1, &s_conf2),
            Err(TyError::TypeConflict { .. })
        ));
    }

    // --- let-binding unboxed pattern rejection tests ---

    #[test]
    fn test_unboxed_let_binding_patterns_rejected() {
        // wildcard pattern: let _ :: Int64# = 42
        assert!(matches!(
            check_let(VPattern::Wild, mk_lit_int(42), Some(mk_ty_unboxed_int64())),
            Err(TyError::UnboxedLetBinding { .. })
        ));

        // literal pattern: let 10 :: Int64# = 10
        let pat_lit = VPattern::Literal(VPatternLiteral::Numeric(VLitNumeric {
            token: ConcreteToken::LiteralNumeric("10".to_string()),
            loc: None,
            value: NumericLiteralValue::Int {
                raw: "10".to_string(),
                parsed: Some(10),
            },
        }));
        assert!(matches!(
            check_let(pat_lit, mk_lit_int(10), Some(mk_ty_unboxed_int64())),
            Err(TyError::UnboxedLetBinding { .. })
        ));
    }

    // --- user-defined type variable handling tests ---

    #[cfg(test)]
    fn mk_ty_user(name: &str) -> TyExpr {
        TyExpr::TyVar(TyVarName::UserDefined(TyVarNameUserDefined {
            token: ConcreteToken::Iden(name.to_string()),
            loc: None,
        }))
    }

    // explicit polymorphic type scheme construction test
    //
    // verify scheme construction from type expression:
    // - deduplicate repeated occurrences of type variables (e.g. `a -> a`)
    // - preserve appearance order across multiple variables (e.g. `a -> b -> a`)
    // - preserve registered ADT constructors without generalizing them as type variables
    // - monomorphic types yield an empty quantifier list
    #[test]
    fn test_build_scheme_from_ty_expr() {
        let mut te = TyConEnv::empty();
        te.add_adt_skeleton("Maybe".to_string(), vec![TyVarName::Auto(0)]);
        let mut ns = TyVarNameSupply::new();

        // deduplication: a -> a yields 1 schematic variable
        let ty_id = mk_ty_arrow(mk_ty_user("a"), mk_ty_user("a"));
        let scheme = build_scheme_from_ty_expr(&ty_id, &te, &mut ns).unwrap();
        assert_eq!(scheme.ty_vars_schematic.len(), 1);

        // multiple variables: a -> b -> a yields 2 schematic variables
        let ty_const = mk_ty_arrow(
            mk_ty_user("a"),
            mk_ty_arrow(mk_ty_user("b"), mk_ty_user("a")),
        );
        let scheme_const = build_scheme_from_ty_expr(&ty_const, &te, &mut ns).unwrap();
        assert_eq!(scheme_const.ty_vars_schematic.len(), 2);

        // adt preservation: a -> Maybe a -> Int64# preserves Maybe and yields 1 schematic variable
        let maybe_a = ty_app(mk_ty_user("Maybe"), mk_ty_user("a"));
        let ty_adt = mk_ty_arrow(mk_ty_user("a"), mk_ty_arrow(maybe_a, mk_ty_unboxed_int64()));
        let scheme_adt = build_scheme_from_ty_expr(&ty_adt, &te, &mut ns).unwrap();
        assert_eq!(scheme_adt.ty_vars_schematic.len(), 1);

        // monomorphic: Int64# -> Int64# yields 0 schematic variables
        let ty_mono = mk_ty_arrow(mk_ty_unboxed_int64(), mk_ty_unboxed_int64());
        let scheme_mono = build_scheme_from_ty_expr(&ty_mono, &te, &mut ns).unwrap();
        assert!(scheme_mono.ty_vars_schematic.is_empty());

        // undeclared capitalized type name yields UnknownType error
        let ty_nested_unknown = ty_app(mk_ty_user("Maybe"), mk_ty_user("Item"));
        assert!(matches!(
            build_scheme_from_ty_expr(&ty_nested_unknown, &te, &mut ns),
            Err(TyError::UnknownType(_))
        ));

        // arity validation: under-applied ADT and applied type variable
        assert!(matches!(
            build_scheme_from_ty_expr(&mk_ty_user("Maybe"), &te, &mut ns),
            Err(TyError::ArityMismatch {
                expected: 1,
                got: 0,
                ..
            })
        ));

        let var_applied = ty_app(mk_ty_user("a"), mk_ty_user("b"));
        assert!(matches!(
            build_scheme_from_ty_expr(&var_applied, &te, &mut ns),
            Err(TyError::ArityMismatch {
                expected: 0,
                got: 1,
                ..
            })
        ));
    }
}
