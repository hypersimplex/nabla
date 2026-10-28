use crate::builtin::types::*;
use crate::builtin::values;
use crate::parse::abstr_structures;
use crate::parse::concrete_token::ConcreteToken;
use crate::parse::loc::ConcreteTokenAndLoc;
use crate::typecheck::ty_expr::*;
use crate::typecheck::v_expr::*;
use crate::typecheck::v_var_name::*;
use crate::typecheck::v_var_name_supply::*;

pub(crate) fn vexpr_and_ty_annot_from_aexpr(
    a_expr: &abstr_structures::AExpr,
    v_var_supply: &mut VVarNameSupply,
) -> VExprAndTyAnnot {
    use abstr_structures::AExpr::*;
    match a_expr {
        AbstractionExpression(item) => v_expr_from_abstr_expr(item, v_var_supply),
        ApplyExpression(item) => v_expr_from_app_expr(item, v_var_supply),
        BlockExpression(item) => v_expr_from_block_expr(item, v_var_supply),
        CaseExpression(item) => v_expr_from_case_expr(item, v_var_supply),
        IdentifierExpression(item) => v_expr_from_iden_expr(item, v_var_supply),
        LetExpression(item) => v_expr_from_let_expr(item, v_var_supply),
        NumericExpr(item) => v_expr_from_lit_num_expr(item, v_var_supply),
        StringExpr(item) => v_expr_from_lit_string_expr(item, v_var_supply),
        ConstructorExpression(item) => v_expr_from_constructor_expr(item, v_var_supply),
    }
}

/// convert to vexpr and gather explicit type annotation if it exists
pub(crate) fn vexpr_and_ty_annot_from_aexpr_annot(
    annot: &abstr_structures::AExprAnnot,
    v_var_supply: &mut VVarNameSupply,
) -> VExprAndTyAnnot {
    let (vexpr, inner_annot) = vexpr_and_ty_annot_from_aexpr(&annot.expr, v_var_supply);
    let ty_annot = annot
        .type_expr
        .as_ref()
        .map(lower_type_annot_to_ty_expr)
        .or(inner_annot); // [todo]: if both type annotations exist, then we should check equivalence
    (vexpr, ty_annot)
}

fn v_expr_from_abstr_expr(
    a_abtr_expr: &abstr_structures::AbstractionExpr,
    v_var_supply: &mut VVarNameSupply,
) -> VExprAndTyAnnot {
    let abstr_structures::AbstractionExpr {
        pattern: _,
        param_patterns,
        expr,
        type_expr,
    } = a_abtr_expr;

    let params: Vec<VAbstrParam> = param_patterns
        .iter()
        .map(|pattern| {
            let (vpattern, annot) = to_v_pattern_and_annot(pattern);
            let binder = match vpattern.peel_typed() {
                VPattern::Variable(var) => var.clone(),
                _ => v_var_supply.generate(),
            };
            VAbstrParam {
                binder,
                pattern: vpattern,
                annotation: annot,
            }
        })
        .collect();

    let body = Box::new(vexpr_and_ty_annot_from_aexpr_annot(expr, v_var_supply));
    (
        VExpr::Abstraction(VAbstrExpr { params, body }),
        type_expr.as_ref().map(lower_type_annot_to_ty_expr),
    )
}

fn v_expr_from_app_expr(
    a_app_expr: &abstr_structures::AppExpr,
    v_var_supply: &mut VVarNameSupply,
) -> VExprAndTyAnnot {
    let abstr_structures::AppExpr { fun, arguments } = a_app_expr;

    let callable = Box::new(vexpr_and_ty_annot_from_aexpr_annot(fun, v_var_supply));

    let args: Vec<_> = arguments
        .iter()
        .map(|arg| vexpr_and_ty_annot_from_aexpr_annot(arg, v_var_supply))
        .collect();

    if args.is_empty() {
        return (
            VExpr::Application(VAppExpr {
                callable,
                args: Vec::new(),
            }),
            None, // this is the type of the result of this application, just leave it empty
        );
    }

    let mut it_args = args.into_iter();
    let arg_first = it_args.next().unwrap();
    let mut f = VExpr::Application(VAppExpr {
        callable,
        args: vec![arg_first],
    });
    for arg in it_args {
        f = VExpr::Application(VAppExpr {
            callable: Box::new((f, None)),
            args: vec![arg],
        });
    }

    (f, None)
}

fn v_expr_from_block_expr(
    block_expr: &abstr_structures::BlockExpr,
    v_var_supply: &mut VVarNameSupply,
) -> VExprAndTyAnnot {
    // currently expect only single expression, but may be used for sequencing (such as in do notation)
    assert!(block_expr.0.len() == 1);
    vexpr_and_ty_annot_from_aexpr_annot(&block_expr.0[0], v_var_supply)
}

fn v_expr_from_case_expr(
    case_expr: &abstr_structures::CaseExpr,
    v_var_supply: &mut VVarNameSupply,
) -> VExprAndTyAnnot {
    let abstr_structures::CaseExpr {
        keyword,
        argument,
        alts,
    } = case_expr;
    let arg = Box::new(vexpr_and_ty_annot_from_aexpr_annot(argument, v_var_supply));

    let alts =
        alts.iter()
            .map(|alt| {
                let v_pattern = to_v_pattern(&alt.pattern);
                let guard = alt.guard.as_ref().map(|guard_expr| {
                    vexpr_and_ty_annot_from_aexpr_annot(guard_expr, v_var_supply)
                });

                let body = Box::new(vexpr_and_ty_annot_from_aexpr_annot(&alt.body, v_var_supply));
                VCaseAlt {
                    pattern: v_pattern,
                    guard,
                    body,
                }
            })
            .collect();
    (
        VExpr::Case(VCaseExpr {
            keyword: keyword.clone(),
            arg,
            alts,
        }),
        None,
    )
}

fn v_expr_from_iden_expr(
    iden_expr: &abstr_structures::IdenExpr,
    _v_var_supply: &mut VVarNameSupply,
) -> VExprAndTyAnnot {
    let abstr_structures::IdenExpr { iden, builtin } = iden_expr;
    let builtin_resolved = builtin
        .as_ref()
        .map(|x| x.into())
        .or_else(|| match &iden.token {
            ConcreteToken::Iden(name) => values::resolve_builtin_fn(name),
            _ => None,
        });
    (
        VExpr::Variable(VVar::Named(VVarName {
            token: iden.token.clone(),
            loc: Some(iden.loc.clone()),
            builtin: builtin_resolved,
        })),
        None,
    )
}

fn v_expr_from_let_expr(
    let_expr: &abstr_structures::LetExpr,
    v_var_supply: &mut VVarNameSupply,
) -> VExprAndTyAnnot {
    let abstr_structures::LetExpr {
        defs,
        expr: expr_and_ty_annot,
    } = let_expr;

    let defs: Vec<(VPattern, VExpr, Option<TyExpr>)> = defs
        .iter()
        .map(|(pattern_expr, expr_annot)| {
            let v_pattern = to_v_pattern(pattern_expr);
            let (vexpr_def, texpr_def_annot) =
                vexpr_and_ty_annot_from_aexpr_annot(expr_annot, v_var_supply);
            (v_pattern, vexpr_def, texpr_def_annot)
        })
        .collect();

    let body = Box::new(vexpr_and_ty_annot_from_aexpr_annot(
        expr_and_ty_annot,
        v_var_supply,
    ));
    (VExpr::Let(VLetExpr { defs, body }), None)
}

fn v_expr_from_lit_num_expr(
    lit_num_expr: &abstr_structures::LiteralNumericExpr,
    _v_var_supply: &mut VVarNameSupply,
) -> VExprAndTyAnnot {
    let abstr_structures::LiteralNumericExpr { literal } = lit_num_expr;
    let value = classify_numeric_literal(&literal.token);
    (
        VExpr::LitNumeric(VLitNumeric {
            token: literal.token.clone(),
            loc: Some(literal.loc.clone()),
            value,
        }),
        None,
    )
}

fn v_expr_from_lit_string_expr(
    lit_string_expr: &abstr_structures::LiteralStringExpr,
    _v_var_supply: &mut VVarNameSupply,
) -> VExprAndTyAnnot {
    let abstr_structures::LiteralStringExpr { literal } = lit_string_expr;
    (
        VExpr::LitString(VLitString {
            token: literal.token.clone(),
            loc: Some(literal.loc.clone()),
        }),
        None,
    )
}

fn v_expr_from_constructor_expr(
    constructor_expr: &abstr_structures::ConstructorExpr,
    v_var_supply: &mut VVarNameSupply,
) -> VExprAndTyAnnot {
    let abstr_structures::ConstructorExpr {
        qualified,
        constructor,
        args,
        record_fields,
    } = constructor_expr;
    let (ty_name, constructor_name) =
        parse_constructor_ref(qualified, constructor, "constructor expression");

    let vargs: Vec<(VExpr, Option<TyExpr>)> = args
        .iter()
        .map(|arg| vexpr_and_ty_annot_from_aexpr_annot(arg, v_var_supply))
        .collect();
    let vrec: Option<Vec<(String, (VExpr, Option<TyExpr>))>> =
        record_fields.as_ref().map(|fields| {
            fields
                .iter()
                .map(|(name_tok, expr)| {
                    let name = identifier_from_token(name_tok, "record field name");
                    (
                        name,
                        vexpr_and_ty_annot_from_aexpr_annot(expr, v_var_supply),
                    )
                })
                .collect()
        });

    (
        VExpr::Constructor(VConstructorExpr {
            ty_name,
            constructor: constructor_name,
            args: vargs,
            record_fields: vrec,
        }),
        None,
    )
}

// helpers related to pattern

fn to_v_pattern_and_annot(pattern: &abstr_structures::PatternExpr) -> (VPattern, Option<TyExpr>) {
    match pattern {
        abstr_structures::PatternExpr::Typed { pattern: inner, ty } => {
            (to_v_pattern(pattern), Some(lower_type_annot_to_ty_expr(ty)))
        }
        other => (to_v_pattern(other), None),
    }
}

fn to_v_pattern(pattern: &abstr_structures::PatternExpr) -> VPattern {
    use crate::parse::abstr_structures::AExpr;
    use crate::parse::abstr_structures::PatternExpr;
    match pattern {
        PatternExpr::Wild => VPattern::Wild,
        PatternExpr::Typed { pattern, ty } => VPattern::Typed {
            pattern: Box::new(to_v_pattern(pattern)),
            ty: lower_type_annot_to_ty_expr(ty),
        },
        PatternExpr::Variable(var) => VPattern::Variable(VVar::Named(VVarName {
            token: var.token.clone(),
            loc: Some(var.loc.clone()),
            builtin: None,
        })),
        PatternExpr::Literal(lit) => match &lit.expr {
            AExpr::NumericExpr(_) | AExpr::StringExpr(_) => {
                VPattern::Literal(literal_expr_to_vpattern(lit))
            }
            other => unreachable!(
                "parser guarantees PatternExpr::Literal only contains numeric or string literals, got: {:?}",
                other
            ),
        },
        PatternExpr::Range { start, end } => {
            let start = match start {
                abstr_structures::PatternRangeBound::Inclusive(lit) => {
                    RangeBound::Inclusive(literal_expr_to_vpattern(lit))
                }
                abstr_structures::PatternRangeBound::Exclusive(lit) => {
                    RangeBound::Exclusive(literal_expr_to_vpattern(lit))
                }
            };
            let end = match end {
                abstr_structures::PatternRangeBound::Inclusive(lit) => {
                    RangeBound::Inclusive(literal_expr_to_vpattern(lit))
                }
                abstr_structures::PatternRangeBound::Exclusive(lit) => {
                    RangeBound::Exclusive(literal_expr_to_vpattern(lit))
                }
            };
            VPattern::Range { start, end }
        }
        PatternExpr::Constructor {
            qualified,
            constructor,
            args,
        } => match args {
            abstr_structures::PatternConstructorArgs::Positional(args) => {
                let (ty_name, constructor_name) =
                    parse_constructor_ref(qualified, constructor, "pattern");
                let vargs: Vec<VPattern> = args.iter().map(to_v_pattern).collect();
                VPattern::Constructor {
                    ty_name,
                    constructor: constructor_name,
                    args: vargs,
                }
            }
            abstr_structures::PatternConstructorArgs::Record { fields, rest } => {
                let (ty_name, constructor_name) =
                    parse_constructor_ref(qualified, constructor, "record pattern");

                let vfields: Vec<(String, VPattern)> = fields
                    .iter()
                    .map(|(field_name, field_pattern)| {
                        let name = identifier_from_token(field_name, "record field name");
                        (name, to_v_pattern(field_pattern))
                    })
                    .collect();

                VPattern::Record {
                    ty_name,
                    constructor: constructor_name,
                    fields: vfields,
                    rest: *rest,
                }
            }
        },
    }
}

fn literal_expr_to_vpattern(lit: &abstr_structures::AExprAnnot) -> VPatternLiteral {
    use crate::parse::abstr_structures::*;
    match &lit.expr {
        AExpr::NumericExpr(num) => {
            let value = classify_numeric_literal(&num.literal.token);
            VPatternLiteral::Numeric(VLitNumeric {
                token: num.literal.token.clone(),
                loc: Some(num.literal.loc.clone()),
                value,
            })
        }
        AExpr::StringExpr(string) => VPatternLiteral::String(VLitString {
            token: string.literal.token.clone(),
            loc: Some(string.literal.loc.clone()),
        }),
        other => unreachable!(
            "parser guarantees PatternExpr::Literal only contains numeric or string literals, got: {:?}",
            other
        ),
    }
}

pub(crate) fn parse_constructor_ref(
    qualified: &Option<ConcreteTokenAndLoc>,
    constructor: &ConcreteTokenAndLoc,
    context: &str,
) -> (Option<String>, String) {
    let ty_name = qualified
        .as_ref()
        .map(|q| identifier_from_token(q, &format!("qualified {context} type name")));
    let constructor_name =
        identifier_from_token(constructor, &format!("{context} constructor name"));
    (ty_name, constructor_name)
}

pub(crate) fn identifier_from_token(token: &ConcreteTokenAndLoc, what: &str) -> String {
    match &token.token {
        ConcreteToken::Iden(name) => name.clone(),
        other => unreachable!("parser guarantees identifier for {what}, got {:?}", other),
    }
}
