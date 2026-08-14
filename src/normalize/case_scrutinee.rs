use crate::typecheck::v_expr::*;
use crate::typecheck::v_expr_typed::*;
use crate::typecheck::v_var_name_supply::*;

/// recursively normalize case scrutinee to be simple variable so that
/// scrutinees are atomic and we avoid re-evaluating expressions
///
/// this would transform
/// ```
/// case non-variable-expr of
///   ...
/// ```
///
/// to
///
/// ```
/// let
///   simple_variable = non-variable-expr
/// in
///   case simple_variable of
///     ...
/// ```
pub(crate) fn normalize_case_scrutinee(ns: &mut VVarNameSupply, expr: &TypedVExpr) -> TypedVExpr {
    match expr {
        TypedVExpr::Abstraction(ab) => {
            let abstraction = TypedVAbstrExpr {
                body: Box::new(normalize_case_scrutinee(ns, &ab.body)),
                ..ab.clone()
            };
            TypedVExpr::Abstraction(abstraction)
        }
        TypedVExpr::Application(app) => {
            let application = TypedVAppExpr {
                callable: Box::new(normalize_case_scrutinee(ns, &app.callable)),
                args: app
                    .args
                    .iter()
                    .map(|arg| normalize_case_scrutinee(ns, arg))
                    .collect(),
                ty: app.ty.clone(),
            };
            TypedVExpr::Application(application)
        }
        TypedVExpr::Case(case) => {
            let arg_norm = normalize_case_scrutinee(ns, &case.arg);
            let clauses_norm: Vec<_> = case
                .clauses
                .iter()
                .map(|clause| TypedVCaseClause {
                    pattern: clause.pattern.clone(),
                    guard: clause
                        .guard
                        .as_ref()
                        .map(|guard| normalize_case_scrutinee(ns, guard)),
                    body: normalize_case_scrutinee(ns, &clause.body),
                })
                .collect();

            let ty = case.ty.clone();
            let mut rebuilt = TypedVExpr::Case(TypedVCaseExpr {
                arg: Box::new(arg_norm.clone()),
                clauses: clauses_norm,
                ty: ty.clone(),
            });

            if matches!(arg_norm, TypedVExpr::Atom(_)) {
                // scrutinee is already a simple variable
                return rebuilt;
            }

            // introduce a simple variable to bind to original scrutinee of case
            // expression using a let expression
            let binder = ns.generate();
            let ty_binder = arg_norm.ty().clone();
            let binder_pat = TypedVPattern::Variable {
                binder: binder.clone(),
                ty: ty_binder.clone(),
                ty_vars_schematic: Vec::new(),
            };

            // use the new simple variable as the scrutinee instead
            let binder_expr = TypedVExpr::Atom(TypedVAtom {
                atom: VAtom::Variable(binder.clone()),
                ty: ty_binder,
                ty_args: Vec::new(),
            });
            rebuilt = match rebuilt {
                TypedVExpr::Case(mut c) => {
                    c.arg = Box::new(binder_expr);
                    TypedVExpr::Case(c)
                }
                _ => unreachable!(),
            };

            // nest the case expression in the introduced let expression
            TypedVExpr::Let(TypedVLetExpr {
                defs: vec![(binder_pat, arg_norm)],
                body: Box::new(rebuilt),
                ty,
            })
        }
        TypedVExpr::Let(let_expr) => {
            let let_expr_new = TypedVLetExpr {
                defs: let_expr
                    .defs
                    .iter()
                    .map(|(pat, rhs)| (pat.clone(), normalize_case_scrutinee(ns, rhs)))
                    .collect(),
                body: Box::new(normalize_case_scrutinee(ns, &let_expr.body)),
                ty: let_expr.ty.clone(),
            };
            TypedVExpr::Let(let_expr_new)
        }
        other => other.clone(),
    }
}
