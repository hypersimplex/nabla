use crate::typecheck::ty_expr::*;
use crate::typecheck::v_expr::*;
use crate::typecheck::v_expr_typed::*;
use crate::typecheck::v_var_name::*;
use crate::typecheck::v_var_name_supply::*;

use std::collections::*;

/// desugar typed expression by eliminating patterns except in case expressions
pub(crate) fn desugar_pattern(ns: &mut VVarNameSupply, expr: &TypedVExpr) -> TypedVExpr {
    match expr {
        TypedVExpr::Abstraction(abstr) => desugar_pattern_abstr(ns, abstr),
        TypedVExpr::Application(app) => desugar_pattern_app(ns, app),
        TypedVExpr::Case(case_expr) => desugar_pattern_case(ns, case_expr),
        TypedVExpr::Let(let_expr) => desugar_pattern_let_expr(ns, let_expr),
        TypedVExpr::Atom(atom) => TypedVExpr::Atom(atom.clone()),
        TypedVExpr::Constructor(constructor) => desugar_pattern_constructor(ns, constructor),
    }
}

/// recursively desugar callable and argument exprs of an application
fn desugar_pattern_app(ns: &mut VVarNameSupply, app: &TypedVAppExpr) -> TypedVExpr {
    let callable = desugar_pattern(ns, &app.callable);
    let args = app
        .args
        .iter()
        .map(|arg| desugar_pattern(ns, arg))
        .collect();
    TypedVExpr::Application(TypedVAppExpr {
        callable: Box::new(callable),
        args,
        ty: app.ty.clone(),
    })
}

/// recursively desugar scrutinee, guards, and clause bodies inside a case
fn desugar_pattern_case(ns: &mut VVarNameSupply, case_expr: &TypedVCaseExpr) -> TypedVExpr {
    let arg = desugar_pattern(ns, &case_expr.arg);
    let clauses = case_expr
        .clauses
        .iter()
        .map(|clause| TypedVCaseClause {
            pattern: clause.pattern.clone(), // leave these patterns intact
            guard: clause
                .guard
                .as_ref()
                .map(|guard| desugar_pattern(ns, guard)),
            body: desugar_pattern(ns, &clause.body),
        })
        .collect();
    TypedVExpr::Case(TypedVCaseExpr {
        arg: Box::new(arg),
        clauses,
        ty: case_expr.ty.clone(),
    })
}

/// normalize abstractions so patterns parameters become variable parameters
/// and recursive desugar pattern for the body as well
///
/// approach:
/// - keep parameter positions with fresh or existing binders
/// - add `case` expressions in the body for non-variable patterns
///
/// eg, this transforms
/// ```
/// f = \pat1 pat2 -> body
/// ```
/// to (intermediate)
/// ```
/// f = \pat1 binder2 ->
///       case binder2 of
///         pat2 -> desugar(body)
/// ```
/// to
/// ```
/// f = \binder1 binder2 ->
///       case binder1 of
///         pat1 ->
///           case binder2 of
///             pat2 -> desugar(body)
/// ```
/// where binder1/binder2 are compiler-managed binders for the original
/// parameter positions
fn desugar_pattern_abstr(ns: &mut VVarNameSupply, abstr: &TypedVAbstrExpr) -> TypedVExpr {
    let TypedVAbstrExpr {
        name,
        params,
        body,
        ty,
    } = abstr;

    let mut body_expr = desugar_pattern(ns, body);

    // iterate in reverse
    for param in params.iter().rev() {
        if !is_irrefutable_variable(param) {
            let ty_body = body_expr.ty().clone();
            body_expr = mk_case_with_single_clause(
                &param.binder,
                &param.ty,
                &param.pattern,
                &body_expr,
                &ty_body,
            );
        }
    }

    // simple variable bindings now
    let params_desugared = params
        .into_iter()
        .map(|TypedVAbstrParam { binder, ty, .. }| TypedVAbstrParam {
            pattern: mk_typed_vpattern_variable(binder, ty),
            binder: binder.clone(),
            ty: ty.clone(),
        })
        .collect();

    TypedVExpr::Abstraction(TypedVAbstrExpr {
        name: name.clone(),
        params: params_desugared,
        body: Box::new(body_expr),
        ty: ty.clone(),
    })
}

/// recursively desugar constructor arguments while preserving ctor metadata
fn desugar_pattern_constructor(
    ns: &mut VVarNameSupply,
    constructor: &TypedVConstructorExpr,
) -> TypedVExpr {
    let args = constructor
        .args
        .iter()
        .map(|arg| desugar_pattern(ns, arg))
        .collect();
    TypedVExpr::Constructor(TypedVConstructorExpr {
        ty_name: constructor.ty_name.clone(),
        constructor_name: constructor.constructor_name.clone(),
        args,
        record_fields: constructor.record_fields.clone(),
        ty: constructor.ty.clone(),
        ty_args: constructor.ty_args.clone(),
    })
}

/// normalize let expressions so patterns no longer appear on LHS definitions
/// and also recursively desugar the RHS
///
/// - introduce a single let-group scope for all definitions
/// - lower each non-variable lhs to:
///   - a fresh scrutinee temp binding (`tmp = rhs`)
///   - selector bindings for each pattern binder (`x = case tmp of pat -> x`)
///
/// this would transforms
/// ```
///   let pat1 = rhs1
///       pat2 = rhs2
///   in body
/// ```
/// to
/// ```
///   let tmp1 = desugar(rhs1)
///       // -- if pat1 binds a,b then:
///       a = case tmp1 of pat1 -> a
///       b = case tmp1 of pat1 -> b
///       tmp2 = desugar(rhs2)
///       // -- if pat2 binds c,d then:
///       c = case tmp2 of pat2 -> c
///       d = case tmp2 of pat2 -> d
///   in desugar(body)
/// ```
fn desugar_pattern_let_expr(ns: &mut VVarNameSupply, let_expr: &TypedVLetExpr) -> TypedVExpr {
    let TypedVLetExpr { defs, body, ty } = let_expr;

    // pattern forcing happens only through selector bindings below
    let body = desugar_pattern(ns, body);

    // lowered_defs corresponds to bindings belonging to a same let-group for peer scope visibility
    let mut lowered_defs: Vec<(TypedVPattern, TypedVExpr)> = Vec::new();

    // process definition in source order
    for (pattern, rhs) in defs {
        let rhs = desugar_pattern(ns, rhs);
        if matches!(pattern, TypedVPattern::Variable { .. }) {
            // already normalized
            lowered_defs.push((pattern.clone(), rhs));
            continue;
        }

        // needs normalization (non-variable LHS)

        // introduce a fresh temp scrutinee and bind to RHS
        let binder_scrutinee = ns.generate();
        let ty_scrutinee = pattern.ty().clone();
        lowered_defs.push((
            mk_typed_vpattern_variable(&binder_scrutinee, &ty_scrutinee),
            rhs,
        ));

        // expose each pattern binder as a peer let binding via a selector case;
        // this can increase number of defs compared to the original defs
        // - if the binder is never referenced, this case is never forced
        // - avoid inserting an extra `case tmp of pat -> body` wrapper around the let body
        for (binder, ty_binder) in collect_pattern_binders_nested_in_order(&pattern) {
            // construct a case binding,
            // eg: binder' = case temp_scrutinee of
            //                 pat -> binder
            let selector_rhs = mk_case_with_single_clause(
                &binder_scrutinee,
                &ty_scrutinee,
                pattern,
                &mk_typed_vexpr_atom(&binder, &ty_binder),
                &ty_binder,
            );
            lowered_defs.push((
                mk_typed_vpattern_variable(&binder, &ty_binder),
                selector_rhs,
            ));
        }
    }

    TypedVExpr::Let(TypedVLetExpr {
        defs: lowered_defs,
        body: Box::new(body),
        ty: ty.clone(),
    })
}

/// collect variable binders and their types from nested patterns in left-to-right DFS order
fn collect_pattern_binders_nested_in_order(pattern: &TypedVPattern) -> Vec<(VVar, TyExpr)> {
    let mut binders_typed = vec![];
    fn collect(
        pattern: &TypedVPattern,
        binders_typed: &mut Vec<(VVar, TyExpr)>,
        seen: &mut BTreeSet<VVar>,
    ) {
        match pattern {
            TypedVPattern::Variable { binder, ty, .. } => {
                // duplicate pattern binders should be rejected earlier in the compiler pipeline.
                assert!(
                    seen.insert(binder.clone()),
                    "duplicate pattern binder `{:?}` reached lhs-pattern desugaring",
                    binder
                );
                binders_typed.push((binder.clone(), ty.clone()));
            }
            TypedVPattern::Constructor { args, .. } => {
                for arg in args {
                    collect(arg, binders_typed, seen);
                }
            }
            TypedVPattern::Record { fields, .. } => {
                for (_, field_pattern) in fields {
                    collect(field_pattern, binders_typed, seen);
                }
            }
            TypedVPattern::Wild { .. }
            | TypedVPattern::Literal { .. }
            | TypedVPattern::Range { .. } => {}
        }
    }
    let mut seen = BTreeSet::new();
    collect(pattern, &mut binders_typed, &mut seen);
    binders_typed
}

/// check whether a parameter is already an irrefutable variable binder.
fn is_irrefutable_variable(param: &TypedVAbstrParam) -> bool {
    matches!(
        &param.pattern,
        TypedVPattern::Variable { binder, .. } if binder == &param.binder
    )
}

/// helper to create a typed case expression with 1 clause
fn mk_case_with_single_clause(
    binder_scrutinee: &VVar,
    ty_scrutinee: &TyExpr,
    pattern: &TypedVPattern,
    body: &TypedVExpr,
    ty: &TyExpr,
) -> TypedVExpr {
    TypedVExpr::Case(TypedVCaseExpr {
        arg: Box::new(mk_typed_vexpr_atom(binder_scrutinee, ty_scrutinee)),
        clauses: vec![TypedVCaseClause {
            pattern: pattern.clone(),
            guard: None,
            body: body.clone(),
        }],
        ty: ty.clone(),
    })
}

/// helper to build a simple typed variable
fn mk_typed_vexpr_atom(var: &VVar, ty: &TyExpr) -> TypedVExpr {
    TypedVExpr::Atom(TypedVAtom {
        atom: VAtom::Variable(var.clone()),
        ty: ty.clone(),
        ty_args: Vec::new(),
    })
}

/// helper to build a simple pattern variable
fn mk_typed_vpattern_variable(binder: &VVar, ty: &TyExpr) -> TypedVPattern {
    TypedVPattern::Variable {
        binder: binder.clone(),
        ty: ty.clone(),
        ty_vars_schematic: Vec::new(),
    }
}
