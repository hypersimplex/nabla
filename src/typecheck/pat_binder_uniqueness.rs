use std::collections::BTreeSet;

use crate::typecheck::ty_err::*;
use crate::typecheck::v_expr::{VAbstrParam, VExpr, VPattern, VVar};

fn collect_pattern_binders_unique(
    pattern: &VPattern,
    seen: &mut BTreeSet<VVar>,
) -> TyChkResult<()> {
    match pattern {
        VPattern::Variable(var) => {
            if !seen.insert(var.clone()) {
                return Err(TyError::PatBinderUniqueness(format!(
                    "duplicate pattern binder: {:?}",
                    var
                )));
            }
            Ok(())
        }
        VPattern::Constructor { args, .. } => {
            for arg in args {
                collect_pattern_binders_unique(arg, seen)?;
            }
            Ok(())
        }
        VPattern::Record { fields, .. } => {
            for (_, field_pattern) in fields {
                collect_pattern_binders_unique(field_pattern, seen)?;
            }
            Ok(())
        }
        VPattern::Wild | VPattern::Literal(_) | VPattern::Range { .. } => Ok(()),
    }
}

fn validate_pattern_binders_unique(pattern: &VPattern) -> TyChkResult<()> {
    let mut seen = BTreeSet::new();
    collect_pattern_binders_unique(pattern, &mut seen)
}

/// for simplicity, don't allow duplicate pattern binders
pub(crate) fn validate_pattern_binder_uniqueness(vexpr: &VExpr) -> TyChkResult<()> {
    match vexpr {
        VExpr::Abstraction(abstr) => {
            let mut seen = BTreeSet::new();
            for param in &abstr.params {
                collect_pattern_binders_unique(&param.pattern, &mut seen)?;
            }

            validate_pattern_binder_uniqueness(&abstr.body.0)
        }
        VExpr::Application(app) => {
            validate_pattern_binder_uniqueness(&app.callable.0)?;
            for (arg, _) in &app.args {
                validate_pattern_binder_uniqueness(arg)?;
            }
            Ok(())
        }
        VExpr::Case(case_expr) => {
            validate_pattern_binder_uniqueness(&case_expr.arg.0)?;
            for clause in &case_expr.clauses {
                validate_pattern_binders_unique(&clause.pattern)?;
                if let Some((guard_expr, _)) = &clause.guard {
                    validate_pattern_binder_uniqueness(guard_expr)?;
                }
                validate_pattern_binder_uniqueness(&clause.body.0)?;
            }
            Ok(())
        }
        VExpr::Let(let_expr) => {
            for (pattern, rhs, _) in &let_expr.defs {
                validate_pattern_binders_unique(pattern)?;
                validate_pattern_binder_uniqueness(rhs)?;
            }
            validate_pattern_binder_uniqueness(&let_expr.body.0)
        }
        VExpr::LitNumeric(_) | VExpr::LitString(_) | VExpr::Variable(_) => Ok(()),
        VExpr::Constructor(cons) => {
            for (arg, _) in &cons.args {
                validate_pattern_binder_uniqueness(arg)?;
            }
            if let Some(fields) = &cons.record_fields {
                for (_, (field_expr, _)) in fields {
                    validate_pattern_binder_uniqueness(field_expr)?;
                }
            }
            Ok(())
        }
    }
}
