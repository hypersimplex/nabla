use crate::typecheck::ty_expr::*;
use crate::typecheck::v_expr::*;
use crate::typecheck::v_expr_typed::*;
use crate::typecheck::v_var_name_supply::*;

use std::collections::*;

pub(crate) fn desugar_case_guard(ns: &mut VVarNameSupply, expr: &TypedVExpr) -> TypedVExpr {
    match expr {
        TypedVExpr::Abstraction(ab) => {
            let abstraction = TypedVAbstrExpr {
                body: Box::new(desugar_case_guard(ns, &ab.body)),
                ..ab.clone()
            };
            TypedVExpr::Abstraction(abstraction)
        }
        TypedVExpr::Application(app) => {
            let application = TypedVAppExpr {
                callable: Box::new(desugar_case_guard(ns, &app.callable)),
                args: app
                    .args
                    .iter()
                    .map(|arg| desugar_case_guard(ns, arg))
                    .collect(),
                ty: app.ty.clone(),
            };
            TypedVExpr::Application(application)
        }
        TypedVExpr::Case(case) => {
            let scrutinee = desugar_case_guard(ns, &case.arg);
            let ty = &case.ty;

            let mut clauses_remaining = VecDeque::<TypedVCaseClause>::new();

            for TypedVCaseClause {
                pattern,
                guard,
                body,
            } in case.clauses.iter().rev()
            {
                let guard = guard.as_ref().map(|x| desugar_case_guard(ns, x));
                let body = desugar_case_guard(ns, body);
                match guard {
                    Some(g) => {
                        // transform current case clause from:
                        //
                        // pattern | g -> body
                        // remaining_clauses
                        //
                        // to:
                        //
                        // pattern -> case g of
                        //              True  -> body
                        //              False -> case scrutinee of
                        //                         remaining_clauses
                        let new_body = TypedVExpr::Case(TypedVCaseExpr {
                            arg: Box::new(g),
                            clauses: vec![
                                TypedVCaseClause {
                                    pattern: bool_pattern(true),
                                    guard: None,
                                    body,
                                },
                                TypedVCaseClause {
                                    pattern: bool_pattern(false),
                                    guard: None,
                                    body: TypedVExpr::Case(TypedVCaseExpr {
                                        arg: Box::new(scrutinee.clone()),
                                        clauses: clauses_remaining.iter().cloned().collect(),
                                        ty: ty.clone(),
                                    }),
                                },
                            ],
                            ty: ty.clone(),
                        });
                        clauses_remaining.push_front(TypedVCaseClause {
                            pattern: pattern.clone(),
                            guard: None,
                            body: new_body,
                        });
                    }
                    _ => {
                        // no guard so just collect clause
                        clauses_remaining.push_front(TypedVCaseClause {
                            pattern: pattern.clone(),
                            guard,
                            body,
                        });
                    }
                }
            }

            TypedVExpr::Case(TypedVCaseExpr {
                arg: Box::new(scrutinee),
                clauses: clauses_remaining.iter().cloned().collect(),
                ty: ty.clone(),
            })
        }
        TypedVExpr::Let(let_expr) => {
            let let_expr_new = TypedVLetExpr {
                defs: let_expr
                    .defs
                    .iter()
                    .map(|(pat, rhs)| (pat.clone(), desugar_case_guard(ns, rhs)))
                    .collect(),
                body: Box::new(desugar_case_guard(ns, &let_expr.body)),
                ty: let_expr.ty.clone(),
            };
            TypedVExpr::Let(let_expr_new)
        }
        other => other.clone(),
    }
}

// [todo] relocate this to common utility file
fn bool_pattern(is_true: bool) -> TypedVPattern {
    TypedVPattern::Constructor {
        ty_name: "Bool".to_string(),
        constructor: if is_true { "True" } else { "False" }.to_string(),
        args: vec![],
        ty: mk_ty_bool(),
    }
}
