use crate::typecheck::v_expr::*;
use crate::typecheck::v_var_name_supply::*;

use std::collections::*;
use std::ops::Deref;

/// when we encounter introduction of binders, we attempt to uniquify all
/// these that are not at the top-level, and apply substitution to these binders
/// and occurences of these binder variables in expressions
pub(crate) fn rename_var_unique(
    ns: &mut VVarNameSupply,
    vvar_outer_scope: &BTreeMap<VVar, VVar>,
    expr: &VExpr,
) -> VExpr {
    rename_var_unique_aux(ns, vvar_outer_scope, 0, expr)
}

// inner helper
fn rename_var_unique_aux(
    ns: &mut VVarNameSupply,
    vvar_outer_scope: &BTreeMap<VVar, VVar>,
    lvl: u32, // 0 for top-level scope
    expr: &VExpr,
) -> VExpr {
    use VExpr::*;
    match expr {
        Abstraction(x) => rename_var_unique_abstraction(ns, vvar_outer_scope, lvl, x),
        Application(x) => rename_var_unique_application(ns, vvar_outer_scope, lvl, x),
        Case(x) => rename_var_unique_case(ns, vvar_outer_scope, lvl, x),
        Let(x) => rename_var_unique_let(ns, vvar_outer_scope, lvl, x),
        LitNumeric(x) => expr.clone(),
        LitString(x) => expr.clone(),
        Variable(x) => rename_var_unique_vvar(ns, vvar_outer_scope, lvl, x),
        Constructor(x) => rename_var_unique_constructor(ns, vvar_outer_scope, lvl, x),
    }
}

fn rename_var_unique_abstraction(
    ns: &mut VVarNameSupply,
    vvar_outer_scope: &BTreeMap<VVar, VVar>,
    lvl: u32, // 0 for top-level scope
    expr: &VAbstrExpr,
) -> VExpr {
    let VAbstrExpr { params, body } = expr;

    let mut vvar_unique_map = vvar_outer_scope.clone();

    let params = params
        .iter()
        .map(
            |VAbstrParam {
                 binder,
                 pattern,
                 annotation,
             }| {
                let mut pattern_vars = BTreeSet::new();
                pattern_vars.insert(binder.clone());
                pattern.get_bound_vars(&mut pattern_vars);

                for i in pattern_vars {
                    vvar_unique_map.insert(i.clone(), ns.uniqify(&i));
                }

                let binder = vvar_unique_map.get(binder).unwrap().clone();

                let mut pattern = pattern.clone();
                pattern.subst_vars(&vvar_unique_map);

                VAbstrParam {
                    binder,
                    pattern,
                    annotation: annotation.clone(),
                }
            },
        )
        .collect();

    let (body_v_expr, body_ty_annot) = Deref::deref(body);
    let body_v_expr = rename_var_unique_aux(ns, &vvar_unique_map, lvl + 1, body_v_expr);
    let body = Box::new((body_v_expr, body_ty_annot.clone()));

    VExpr::Abstraction(VAbstrExpr { params, body })
}

fn rename_var_unique_application(
    ns: &mut VVarNameSupply,
    vvar_outer_scope: &BTreeMap<VVar, VVar>,
    lvl: u32, // 0 for top-level scope
    expr: &VAppExpr,
) -> VExpr {
    let VAppExpr { callable, args } = expr;

    // recursion

    let (callable_expr, callable_ty_annot) = Deref::deref(callable);
    let callable_expr = rename_var_unique_aux(ns, vvar_outer_scope, lvl + 1, callable_expr);
    let callable = Box::new((callable_expr, callable_ty_annot.clone()));

    let args = args
        .iter()
        .map(|(v_expr, ty_annot)| {
            (
                rename_var_unique_aux(ns, vvar_outer_scope, lvl + 1, v_expr),
                ty_annot.clone(),
            )
        })
        .collect();

    VExpr::Application(VAppExpr { callable, args })
}

fn rename_var_unique_case(
    ns: &mut VVarNameSupply,
    vvar_outer_scope: &BTreeMap<VVar, VVar>,
    lvl: u32, // 0 for top-level scope
    expr: &VCaseExpr,
) -> VExpr {
    let VCaseExpr {
        keyword,
        arg,
        clauses,
    } = expr;

    // note: local binders introduced in case scrutinee is not visible to
    // case clauses
    let (arg_expr, arg_ty_annot) = Deref::deref(arg);
    let arg_expr = rename_var_unique_aux(ns, vvar_outer_scope, lvl + 1, arg_expr);
    let arg = Box::new((arg_expr, arg_ty_annot.clone()));

    let clauses = clauses
        .iter()
        .map(|clause| {
            let VCaseClause {
                pattern,
                guard,
                body,
            } = clause;

            let mut vvar_unique_map = vvar_outer_scope.clone();
            {
                let mut pattern_vars = BTreeSet::new();
                pattern.get_bound_vars(&mut pattern_vars);
                for i in pattern_vars {
                    vvar_unique_map.insert(i.clone(), ns.uniqify(&i));
                }
            }
            // apply substitution to pattern
            let mut pattern = pattern.clone();
            pattern.subst_vars(&vvar_unique_map);

            // recursion
            let guard = guard.as_ref().map(|(g_v_expr, g_ty_annot)| {
                (
                    rename_var_unique_aux(ns, &vvar_unique_map, lvl + 1, &g_v_expr),
                    g_ty_annot.clone(),
                )
            });
            let (body_v_expr, body_ty_annot) = Deref::deref(body);
            let body = Box::new((
                rename_var_unique_aux(ns, &vvar_unique_map, lvl + 1, &body_v_expr),
                body_ty_annot.clone(),
            ));
            VCaseClause {
                pattern,
                guard,
                body,
            }
        })
        .collect();

    VExpr::Case(VCaseExpr {
        keyword: keyword.clone(),
        arg,
        clauses,
    })
}

fn rename_var_unique_let(
    ns: &mut VVarNameSupply,
    vvar_outer_scope: &BTreeMap<VVar, VVar>,
    lvl: u32, // 0 for top-level scope
    expr: &VLetExpr,
) -> VExpr {
    let VLetExpr { defs, body } = expr;

    let mut vvar_unique_map = vvar_outer_scope.clone();

    for (def_pat, def_v_expr, def_ty_annot) in defs.iter() {
        let mut bound_vars = BTreeSet::new();
        def_pat.get_bound_vars(&mut bound_vars);
        for i in bound_vars {
            vvar_unique_map.insert(i.clone(), ns.uniqify(&i));
        }
    }

    let defs = defs
        .iter()
        .cloned()
        .map(|(mut def_pat, def_v_expr, def_ty_annot)| {
            def_pat.subst_vars(&vvar_unique_map);
            let vvar_unique_map_per_def = vvar_unique_map.clone();
            let def_v_expr =
                rename_var_unique_aux(ns, &vvar_unique_map_per_def, lvl + 1, &def_v_expr);
            (def_pat, def_v_expr, def_ty_annot)
        })
        .collect();

    let (body_expr, body_ty_annot) = Deref::deref(body);
    let body_expr = rename_var_unique_aux(ns, &vvar_unique_map, lvl + 1, body_expr);
    let body = Box::new((body_expr, body_ty_annot.clone()));

    VExpr::Let(VLetExpr { defs, body })
}

fn rename_var_unique_vvar(
    ns: &mut VVarNameSupply,
    vvar_outer_scope: &BTreeMap<VVar, VVar>,
    lvl: u32, // 0 for top-level scope
    vvar: &VVar,
) -> VExpr {
    // apply var substitution
    match vvar_outer_scope.get(vvar) {
        Some(mapped_to_var) => VExpr::Variable(mapped_to_var.clone()),
        _ => VExpr::Variable(vvar.clone()),
    }
}

fn rename_var_unique_constructor(
    ns: &mut VVarNameSupply,
    vvar_outer_scope: &BTreeMap<VVar, VVar>,
    lvl: u32, // 0 for top-level scope
    expr: &VConstructorExpr,
) -> VExpr {
    let VConstructorExpr {
        ty_name,
        constructor,
        args,
        record_fields,
    } = expr;

    // recursion

    VExpr::Constructor(VConstructorExpr {
        ty_name: ty_name.clone(),
        constructor: constructor.clone(),
        args: args
            .iter()
            .map(|(v_expr, ty_annot)| {
                (
                    rename_var_unique_aux(ns, vvar_outer_scope, lvl + 1, v_expr),
                    ty_annot.clone(),
                )
            })
            .collect(),
        record_fields: record_fields.as_ref().map(|fields| {
            fields
                .iter()
                .map(|(field_name, (v_expr, ty_annot))| {
                    (
                        field_name.clone(),
                        (
                            rename_var_unique_aux(ns, vvar_outer_scope, lvl + 1, v_expr),
                            ty_annot.clone(),
                        ),
                    )
                })
                .collect()
        }),
    })
}
