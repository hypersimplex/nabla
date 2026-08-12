use crate::builtin::types::*;
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
        UnitExpr => (VExpr::Atom(VAtom::Unit), Some(mk_ty_unit())),
        IdentifierExpression(item) => v_expr_from_iden_expr(item, v_var_supply),
        LetExpression(item) => v_expr_from_let_expr(item, v_var_supply),
        NumericExpr(item) => v_expr_from_lit_num_expr(item, v_var_supply),
        StringExpr(item) => v_expr_from_lit_string_expr(item, v_var_supply),
        ConstructorExpression(item) => v_expr_from_constructor_expr(item, v_var_supply),
    }
}

fn v_expr_from_abstr_expr(
    a_abtr_expr: &abstr_structures::AbstractionExpr,
    v_var_supply: &mut VVarNameSupply,
) -> VExprAndTyAnnot {
    let abstr_structures::AbstractionExpr {
        name,
        pattern: _,
        param_patterns,
        expr,
        type_expr,
    } = a_abtr_expr;

    let name: VVar = match name {
        Some(x) => VVar::Named(VVarName {
            token: x.token.clone(),
            loc: Some(x.loc.clone()),
            builtin: None,
        }),
        _ => v_var_supply.generate(),
    };

    let params: Vec<VAbstrParam> = param_patterns
        .iter()
        .map(|pattern| {
            let vpattern = to_v_pattern(pattern);
            let binder = match &vpattern {
                VPattern::Variable(var) => var.clone(),
                _ => v_var_supply.generate(),
            };
            VAbstrParam {
                binder,
                pattern: vpattern,
                annotation: None,
            }
        })
        .collect();

    let body = Box::new(vexpr_and_ty_annot_from_aexpr(&expr.expr, v_var_supply));
    (
        VExpr::Abstraction(VAbstrExpr { name, params, body }),
        type_expr.as_ref().map(lower_type_annot_to_ty_expr),
    )
}

fn v_expr_from_app_expr(
    a_app_expr: &abstr_structures::AppExpr,
    v_var_supply: &mut VVarNameSupply,
) -> VExprAndTyAnnot {
    let abstr_structures::AppExpr { fun, arguments } = a_app_expr;

    let (vexpr_fun, texpr_fun_annot) = vexpr_and_ty_annot_from_aexpr(&fun.expr, v_var_supply);

    let callable = Box::new((
        vexpr_fun,
        fun.type_expr
            .as_ref()
            .map(lower_type_annot_to_ty_expr)
            .or(texpr_fun_annot),
    ));

    let args: Vec<_> = arguments
        .iter()
        .map(|abstr_structures::AExprAnnot { expr, type_expr }| {
            let (vexpr_arg, texpr_arg_annot) = vexpr_and_ty_annot_from_aexpr(expr, v_var_supply);
            (
                vexpr_arg,
                type_expr
                    .as_ref()
                    .map(lower_type_annot_to_ty_expr)
                    .or(texpr_arg_annot),
            )
        })
        .collect();

    if args.is_empty() {
        return (
            VExpr::Application(VAppExpr {
                callable,
                args: vec![],
            }),
            fun.type_expr.as_ref().map(lower_type_annot_to_ty_expr),
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

    (f, fun.type_expr.as_ref().map(lower_type_annot_to_ty_expr))
}

fn v_expr_from_block_expr(
    block_expr: &abstr_structures::BlockExpr,
    v_var_supply: &mut VVarNameSupply,
) -> VExprAndTyAnnot {
    assert!(block_expr.0.len() == 1); // currently expect only single expression
    let abstr_structures::AExprAnnot {
        ref expr,
        ref type_expr,
    } = block_expr.0[0];
    let (vexpr_body, texpr_body_annot) = vexpr_and_ty_annot_from_aexpr(expr, v_var_supply);
    (
        vexpr_body,
        type_expr
            .as_ref()
            .map(lower_type_annot_to_ty_expr)
            .or(texpr_body_annot),
    )
}

fn v_expr_from_case_expr(
    case_expr: &abstr_structures::CaseExpr,
    v_var_supply: &mut VVarNameSupply,
) -> VExprAndTyAnnot {
    let abstr_structures::CaseExpr {
        keyword,
        argument,
        clauses,
    } = case_expr;
    let (vexpr_arg, texpr_arg_annot) = vexpr_and_ty_annot_from_aexpr(&argument.expr, v_var_supply);

    let arg = Box::new((
        vexpr_arg,
        argument
            .type_expr
            .as_ref()
            .map(lower_type_annot_to_ty_expr)
            .or(texpr_arg_annot),
    ));

    let clauses = clauses
        .iter()
        .map(|clause| {
            let v_pattern = to_v_pattern(&clause.pattern);
            let guard = clause.guard.as_ref().map(|guard_expr| {
                let (vexpr_guard, texpr_guard_annot) =
                    vexpr_and_ty_annot_from_aexpr(&guard_expr.expr, v_var_supply);
                (
                    vexpr_guard,
                    guard_expr
                        .type_expr
                        .as_ref()
                        .map(lower_type_annot_to_ty_expr)
                        .or(texpr_guard_annot),
                )
            });

            let abstr_structures::AExprAnnot { expr, type_expr } = &*clause.body;
            let (vexpr_clause_body, texpr_clause_body_annot) =
                vexpr_and_ty_annot_from_aexpr(expr, v_var_supply);
            VCaseClause {
                pattern: v_pattern,
                guard,
                body: Box::new((
                    vexpr_clause_body,
                    type_expr
                        .as_ref()
                        .map(lower_type_annot_to_ty_expr)
                        .or(texpr_clause_body_annot),
                )),
            }
        })
        .collect();
    (
        VExpr::Case(VCaseExpr {
            keyword: keyword.clone(),
            arg,
            clauses,
        }),
        None,
    )
}

fn v_expr_from_iden_expr(
    iden_expr: &abstr_structures::IdenExpr,
    v_var_supply: &mut VVarNameSupply,
) -> VExprAndTyAnnot {
    let abstr_structures::IdenExpr { iden, builtin } = iden_expr;
    (
        VExpr::Atom(VAtom::Variable(VVar::Named(VVarName {
            token: iden.token.clone(),
            loc: Some(iden.loc.clone()),
            builtin: builtin.as_ref().map(|x| x.into()),
        }))),
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
        .map(
            |(
                pattern_expr,
                abstr_structures::AExprAnnot {
                    expr: expr_def,
                    type_expr,
                },
            )| {
                let v_pattern = to_v_pattern(pattern_expr);
                let (vexpr_def, texpr_def_annot) =
                    vexpr_and_ty_annot_from_aexpr(expr_def, v_var_supply);
                (
                    v_pattern,
                    vexpr_def,
                    type_expr
                        .as_ref()
                        .map(lower_type_annot_to_ty_expr)
                        .or(texpr_def_annot),
                )
            },
        )
        .collect();

    let (vexpr_body, texpr_body_annot) =
        vexpr_and_ty_annot_from_aexpr(&expr_and_ty_annot.expr, v_var_supply);

    let body = Box::new((vexpr_body, texpr_body_annot));
    (
        VExpr::Let(VLetExpr { defs, body }),
        expr_and_ty_annot
            .type_expr
            .as_ref()
            .map(lower_type_annot_to_ty_expr),
    )
}

fn v_expr_from_lit_num_expr(
    lit_num_expr: &abstr_structures::LiteralNumericExpr,
    v_var_supply: &mut VVarNameSupply,
) -> VExprAndTyAnnot {
    let abstr_structures::LiteralNumericExpr { literal } = lit_num_expr;
    let value = classify_numeric_literal(&literal.token);
    (
        VExpr::Atom(VAtom::Numeric(VLitNumeric {
            token: literal.token.clone(),
            loc: Some(literal.loc.clone()),
            value,
        })),
        None,
    )
}

fn v_expr_from_lit_string_expr(
    lit_string_expr: &abstr_structures::LiteralStringExpr,
    v_var_supply: &mut VVarNameSupply,
) -> VExprAndTyAnnot {
    let abstr_structures::LiteralStringExpr { literal } = lit_string_expr;
    (
        VExpr::Atom(VAtom::String(VLitString {
            token: literal.token.clone(),
            loc: Some(literal.loc.clone()),
        })),
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
        .map(|arg| vexpr_and_ty_annot_from_aexpr(&arg.expr, v_var_supply))
        .collect();
    let vrec: Option<Vec<(String, (VExpr, Option<TyExpr>))>> =
        record_fields.as_ref().map(|fields| {
            fields
                .iter()
                .map(|(name_tok, expr)| {
                    let name = identifier_from_token(name_tok, "record field name");
                    let (vx, tx) = vexpr_and_ty_annot_from_aexpr(&expr.expr, v_var_supply);
                    (
                        name,
                        (
                            vx,
                            expr.type_expr
                                .as_ref()
                                .map(lower_type_annot_to_ty_expr)
                                .or(tx),
                        ),
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

fn to_v_pattern(pattern: &abstr_structures::PatternExpr) -> VPattern {
    use crate::parse::abstr_structures::AExpr;
    use crate::parse::abstr_structures::PatternExpr;
    match pattern {
        PatternExpr::Wild => VPattern::Wild,
        PatternExpr::Variable(var) => VPattern::Variable(VVar::Named(VVarName {
            token: var.token.clone(),
            loc: Some(var.loc.clone()),
            builtin: None,
        })),
        PatternExpr::Literal(lit) => match &lit.expr {
            AExpr::NumericExpr(_) | AExpr::StringExpr(_) | AExpr::UnitExpr => {
                VPattern::Literal(literal_expr_to_vpattern(lit))
            }
            other => panic!("unsupported literal pattern expression: {:?}", other),
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
        AExpr::UnitExpr => VPatternLiteral::Unit,
        other => panic!("unsupported literal pattern expression: {:?}", other),
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
        other => panic!("expected identifier for {what}, got {:?}", other),
    }
}
