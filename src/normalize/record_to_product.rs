use crate::typecheck::adt::*;
use crate::typecheck::subst::*;
use crate::typecheck::subst_persistent::*;
use crate::typecheck::ty_con_env::*;
use crate::typecheck::ty_expr::*;
use crate::typecheck::v_expr_typed::*;
use std::collections::HashMap;

/// desugar typed expression by converting record patterns and record
/// constructor expressions to product patterns and product constructor
/// expressions
///
/// eg, given:
///
/// ```
/// data N {
///   y :: f64,
/// }
///
/// data T B {
///   v :: i64,
///   x :: B,
/// }
/// ```
///
/// this transforms a record pattern
///
/// ```
/// case y of
///   T { x: N { y: v }, .. } -> v
///     ```
///
/// to
///
/// ```
/// case y of
///   T _ (N v) -> v
/// ```
/// where:
/// - `_` is a wildcard pattern is introduced for omitted fields in the original
///   pattern `..`
///
/// - `(N v)` is the desugared positional pattern for `x = N { y: v }` in the
///   second argument position, matching the declared field order
///
/// and transforms a record constructor expression
///
/// ```
/// T { v = 0, x = 10 }
/// ```
///
/// to
///
/// ```
/// T 0 10
/// ```
///
/// positional arguments are already sorted according to declared field order
/// during type checking
///
/// setting record fields to none removes record metadata and yields a
/// positional product constructor expression
pub(crate) fn desugar_record_to_product(ty_env: &TyConEnv, expr: &TypedVExpr) -> TypedVExpr {
    match expr {
        TypedVExpr::Abstraction(ab) => {
            let abstraction = TypedVAbstrExpr {
                body: Box::new(desugar_record_to_product(ty_env, &ab.body)),
                ..ab.clone()
            };
            TypedVExpr::Abstraction(abstraction)
        }
        TypedVExpr::Application(app) => {
            let application = TypedVAppExpr {
                callable: Box::new(desugar_record_to_product(ty_env, &app.callable)),
                args: app
                    .args
                    .iter()
                    .map(|arg| desugar_record_to_product(ty_env, arg))
                    .collect(),
                ty: app.ty.clone(),
            };
            TypedVExpr::Application(application)
        }
        TypedVExpr::Case(case_expr) => {
            let arg = desugar_record_to_product(ty_env, &case_expr.arg);
            let clauses = case_expr
                .clauses
                .iter()
                .map(|clause| {
                    let pattern = desugar_pattern_records(ty_env, &clause.pattern);
                    let guard = clause
                        .guard
                        .as_ref()
                        .map(|g| desugar_record_to_product(ty_env, g));
                    let body = desugar_record_to_product(ty_env, &clause.body);

                    TypedVCaseClause {
                        pattern,
                        guard,
                        body,
                    }
                })
                .collect();

            TypedVExpr::Case(TypedVCaseExpr {
                arg: Box::new(arg),
                clauses,
                ty: case_expr.ty.clone(),
            })
        }
        TypedVExpr::Let(let_expr) => {
            let defs = let_expr
                .defs
                .iter()
                .map(|(pat, rhs)| {
                    let pat_desugared = desugar_pattern_records(ty_env, pat);
                    let rhs_desugared = desugar_record_to_product(ty_env, rhs);
                    (pat_desugared, rhs_desugared)
                })
                .collect();
            TypedVExpr::Let(TypedVLetExpr {
                defs,
                body: Box::new(desugar_record_to_product(ty_env, &let_expr.body)),
                ty: let_expr.ty.clone(),
            })
        }
        TypedVExpr::Constructor(ctor) => {
            // desugar record constructor expression to positional product constructor expression
            //
            // constructor arguments were already ordered to match the declared field order
            // during type checking
            //
            // erasing record fields to none removes record metadata and yields a uniform
            // positional product constructor expression
            let constructor = TypedVConstructorExpr {
                args: ctor
                    .args
                    .iter()
                    .map(|arg| desugar_record_to_product(ty_env, arg))
                    .collect(),
                record_fields: None,
                ..ctor.clone()
            };
            TypedVExpr::Constructor(constructor)
        }
        TypedVExpr::LitNumeric(_) | TypedVExpr::LitString(_) | TypedVExpr::Variable(_) => {
            expr.clone()
        }
    }
}

/// recursively convert all `TypedVPattern::Record` occurrences into positional
/// `TypedVPattern::Constructor` patterns matching field order
pub(crate) fn desugar_pattern_records(ty_env: &TyConEnv, pat: &TypedVPattern) -> TypedVPattern {
    match pat {
        TypedVPattern::Record {
            ty_name,
            constructor,
            fields,
            rest,
            ty,
            ty_args,
        } => desugar_record_pattern(ty_env, ty_name, constructor, fields, *rest, ty, ty_args),

        TypedVPattern::Constructor {
            ty_name,
            constructor,
            args,
            ty,
            ty_args,
        } => TypedVPattern::Constructor {
            ty_name: ty_name.clone(),
            constructor: constructor.clone(),
            args: args
                .iter()
                .map(|arg| desugar_pattern_records(ty_env, arg))
                .collect(),
            ty: ty.clone(),
            ty_args: ty_args.clone(),
        },

        other => other.clone(),
    }
}

/// convert a single record pattern to a positional constructor pattern
fn desugar_record_pattern(
    ty_env: &TyConEnv,
    ty_name: &Option<String>,
    constructor: &str,
    fields: &[(String, TypedVPattern)],
    rest: bool,
    ty: &TyExpr,
    ty_args: &[TyExpr],
) -> TypedVPattern {
    let ctor_ref = match ty_name {
        Some(t) => ConstructorRef::Qualified {
            ty_name: t.clone(),
            constructor: constructor.to_string(),
        },
        None => ConstructorRef::Unqualified(constructor.to_string()),
    };
    let resolved = ty_env
        .resolve_constructor(&ctor_ref)
        .expect("constructor should have been resolved during type checking");

    let expected_field_names = resolved
        .ctor
        .field_names
        .as_ref()
        .expect("record constructor must have field names");
    let expected_field_types = &resolved.ctor.field_types;
    let adt_ty_params = &resolved.adt.ty_params;

    assert_eq!(
        adt_ty_params.len(),
        ty_args.len(),
        "ADT type parameter count must match pattern type argument count for constructor {}",
        constructor
    );

    // omitted fields `..` synthesize `TypedVPattern::Wild` patterns
    //
    // substitute ADT formal type parameters with concrete pattern `ty_args`
    // so the wildcard types match this pattern's instantiation
    let mut subst = SubstPersistentIdent::default();
    for (param, arg) in adt_ty_params.iter().zip(ty_args.iter()) {
        subst = subst.insert(param.clone(), arg.clone());
    }

    // map user fields and recursively desugar any nested records
    let mut user_fields: HashMap<String, TypedVPattern> = fields
        .iter()
        .map(|(name, pat)| (name.clone(), desugar_pattern_records(ty_env, pat)))
        .collect();

    // order fields according to definition order, padding omitted fields (`..`) with wildcards
    let positional_args = expected_field_names
        .iter()
        .zip(expected_field_types.iter())
        .map(|(f_name, f_ty)| {
            if let Some(user_pat) = user_fields.remove(f_name) {
                user_pat
            } else {
                assert!(rest, "omitted field without rest in record pattern");
                let concrete_field_ty = subst_ty(&subst, f_ty);
                TypedVPattern::Wild {
                    ty: concrete_field_ty,
                }
            }
        })
        .collect();

    // sanity check
    assert!(
        user_fields.is_empty(),
        "unrecognized fields in record pattern for constructor {}: {:?}",
        constructor,
        user_fields.keys().collect::<Vec<_>>()
    );

    TypedVPattern::Constructor {
        ty_name: resolved.ty_name.clone(),
        constructor: resolved.constructor.clone(),
        args: positional_args,
        ty: ty.clone(),
        ty_args: ty_args.to_vec(),
    }
}
