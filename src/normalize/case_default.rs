use crate::typecheck::v_expr_typed::*;

/// transforms `case` expressions such that:
/// - variable patterns:
///     `case v of
///        ...
///        x -> body`
///
///   becomes
///
///   `case v of
///      ...
///      _ -> let x = v
///           in body`
///
/// - prune unreachable alternatives after an unconditional pattern (simple
///   variable pattern with no guard or wildcard pattern)
///
/// - simplify single unconditional wildcard: `case v of _ -> body` => `body`
///
/// after this tranformation, alternatives only contain:
/// constructor, literal, or wildcard (`DEFAULT`) patterns
///
/// this is mainly done to prepare for conversion to the Core IR where
/// we expect constructor, literal, or wildcard (`DEFAULT`) patterns only
pub(crate) fn normalize_case_default(expr: &TypedVExpr) -> TypedVExpr {
    match expr {
        TypedVExpr::Abstraction(ab) => TypedVExpr::Abstraction(TypedVAbstrExpr {
            body: Box::new(normalize_case_default(&ab.body)),
            ..ab.clone()
        }),
        TypedVExpr::Application(app) => TypedVExpr::Application(TypedVAppExpr {
            callable: Box::new(normalize_case_default(&app.callable)),
            args: app.args.iter().map(|x| normalize_case_default(x)).collect(),
            ty: app.ty.clone(),
        }),
        TypedVExpr::Constructor(c) => TypedVExpr::Constructor(TypedVConstructorExpr {
            args: c.args.iter().map(|x| normalize_case_default(x)).collect(),
            ..c.clone()
        }),
        TypedVExpr::Let(let_expr) => TypedVExpr::Let(TypedVLetExpr {
            defs: let_expr
                .defs
                .iter()
                .map(|(pat, rhs)| (pat.clone(), normalize_case_default(rhs)))
                .collect(),
            body: Box::new(normalize_case_default(&let_expr.body)),
            ty: let_expr.ty.clone(),
        }),
        TypedVExpr::Case(case) => {
            let arg_normalized = normalize_case_default(&case.arg);
            let mut new_alts = Vec::new();

            for alt in case.alts.iter() {
                let body_norm = normalize_case_default(&alt.body);

                // note: guard should have been desguared away by earlier
                // passes in the compilation pipeline, but we'll leave this
                // here for soundness
                let guard_normalized = alt.guard.as_ref().map(|x| normalize_case_default(x));

                let (pattern, body, is_catchall) = match &alt.pattern {
                    // rewrite alternative with variable pattern:
                    //   `x -> body`
                    // to
                    //   `_ -> let x = arg in body`
                    // or
                    //   `_ -> body` if `x` is same as `arg`
                    TypedVPattern::Variable { binder, ty, .. } => {
                        let is_same_var =
                            matches!(&arg_normalized, TypedVExpr::Variable(v) if v.var == *binder);
                        let body = if is_same_var {
                            body_norm
                        } else {
                            let body_ty = body_norm.ty().clone();
                            TypedVExpr::Let(TypedVLetExpr {
                                defs: vec![(alt.pattern.clone(), arg_normalized.clone())],
                                body: Box::new(body_norm),
                                ty: body_ty,
                            })
                        };
                        (TypedVPattern::Wild { ty: ty.clone() }, body, true)
                    }

                    TypedVPattern::Wild { .. } => (alt.pattern.clone(), body_norm, true),

                    // other (eg: Constructor / literal): keep as is
                    _ => (alt.pattern.clone(), body_norm, false),
                };

                new_alts.push(TypedVCaseAlt {
                    pattern,
                    guard: guard_normalized,
                    body,
                });

                // unconditional match (unguarded variable or wildcard
                // pattern) => prune trailing alternatives since they are
                // unreachable
                if is_catchall && alt.guard.is_none() {
                    break;
                }
            }

            // further simplification: if case expression has exactly one
            // unconditional wildcard alternative, `case v of _ -> body`,
            // the case wrapper is redundant and can be replaced by `body`
            //
            // note: if the original pattern is a variable `x`, `body` is
            // transformed into the form `let x = v in body` by our
            // transformation
            if let [
                TypedVCaseAlt {
                    pattern: TypedVPattern::Wild { .. },
                    guard: None,
                    ..
                },
            ] = &new_alts[..]
            {
                return new_alts.pop().unwrap().body;
            }

            TypedVExpr::Case(TypedVCaseExpr {
                arg: Box::new(arg_normalized),
                alts: new_alts,
                ty: case.ty.clone(),
            })
        }
        // literals, variables, etc. with no nested sub-expressions => no change
        other => other.clone(),
    }
}
