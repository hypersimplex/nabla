// Tests for parsing algebraic data type constructors and pattern matches
#[cfg(test)]
mod test_adt {
    use crate::parse::abstr::parse_concrete_top_level;
    use crate::parse::abstr_structures::*;
    use crate::parse::concrete_token::ConcreteToken;
    use crate::parse::lex::parse_content_to_concrete_tokens;
    use crate::parse::printer::*;
    use std::path::Path;

    const CONSTRUCTOR_FIXTURE: &str = r#"
makeSome x = Some x

makeQualified x = Option.Some x

makePair a b = Pair a b
"#;

    const PATTERN_FIXTURE: &str = r#"
unwrap opt = case opt of
        None -> 0
        Some x -> x
        _ -> -1
"#;

    const NESTED_FIXTURE: &str = r#"
nested x = case x of
        None -> 0
        Some None -> 1
        Some (Some y) -> y
"#;

    fn parse_top_level(content: &str) -> TopLevelItems {
        let lexed = parse_content_to_concrete_tokens(Path::new("<memory>"), content)
            .expect("lexing should succeed");
        parse_concrete_top_level(lexed).expect("parsing should succeed")
    }

    fn find_function<'a>(items: &'a [TopLevelItem], name: &str) -> &'a AbstractionExpr {
        items
            .iter()
            .find_map(|item| match item {
                TopLevelItem::FunctionDefinition(def)
                    if def.name.as_ref().map(|tok| &tok.token)
                        == Some(&ConcreteToken::Iden(name.to_string())) =>
                {
                    Some(def)
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("expected function `{name}`"))
    }

    fn only_expression<'a>(abstr: &'a AbstractionExpr) -> &'a AExpr {
        match &abstr.expr.expr {
            AExpr::BlockExpression(block) => {
                &block
                    .0
                    .first()
                    .expect("block should contain body expression")
                    .expr
            }
            other => other,
        }
    }

    #[test]
    fn some_constructor_application() {
        let items = parse_top_level(CONSTRUCTOR_FIXTURE);
        let make_some = find_function(&items.0, "makeSome");

        assert!(
            matches!(
                make_some.param_patterns.as_slice(),
                [PatternExpr::Variable(_)]
            ),
            "makeSome should have one variable parameter"
        );

        let application = match only_expression(make_some) {
            AExpr::ApplyExpression(app) => app,
            other => panic!("expected constructor application, got {other:?}"),
        };

        match &application.fun.expr {
            AExpr::ConstructorExpression(cons) => {
                assert!(cons.qualified.is_none(), "Some should be unqualified");
                if let ConcreteToken::Iden(name) = &cons.constructor.token {
                    assert_eq!(name, "Some");
                } else {
                    panic!("expected constructor identifier for Some");
                }
            }
            other => panic!("expected constructor callee, got {other:?}"),
        }

        assert_eq!(
            application.arguments.len(),
            1,
            "Some constructor should receive one argument"
        );
    }

    #[test]
    fn qualified_constructor_application() {
        let items = parse_top_level(CONSTRUCTOR_FIXTURE);
        let make_qualified = find_function(&items.0, "makeQualified");

        let application = match only_expression(make_qualified) {
            AExpr::ApplyExpression(app) => app,
            other => panic!("expected constructor application, got {other:?}"),
        };

        match &application.fun.expr {
            AExpr::ConstructorExpression(cons) => {
                let ty_name = cons
                    .qualified
                    .as_ref()
                    .map(|tok| tok.token.clone())
                    .expect("constructor should be qualified");
                assert_eq!(ty_name, ConcreteToken::Iden("Option".into()));
                if let ConcreteToken::Iden(name) = &cons.constructor.token {
                    assert_eq!(name, "Some");
                } else {
                    panic!("expected constructor identifier for Option.Some");
                }
            }
            other => panic!("expected qualified constructor callee, got {other:?}"),
        }

        assert_eq!(
            application.arguments.len(),
            1,
            "qualified constructor should have a single argument"
        );
    }

    #[test]
    fn pair_constructor_application() {
        let items = parse_top_level(CONSTRUCTOR_FIXTURE);
        let make_pair = find_function(&items.0, "makePair");

        assert_eq!(
            make_pair.param_patterns.len(),
            2,
            "makePair should have two parameters"
        );

        let application = match only_expression(make_pair) {
            AExpr::ApplyExpression(app) => app,
            other => panic!("expected constructor application, got {other:?}"),
        };

        assert_eq!(
            application.arguments.len(),
            2,
            "Pair constructor should be applied to two arguments"
        );
        match &application.fun.expr {
            AExpr::ConstructorExpression(cons) => {
                if let ConcreteToken::Iden(name) = &cons.constructor.token {
                    assert_eq!(name, "Pair");
                } else {
                    panic!("expected constructor identifier for Pair");
                }
            }
            other => panic!("expected constructor callee for Pair, got {other:?}"),
        }
    }

    #[test]
    fn case_clause_patterns() {
        let items = parse_top_level(PATTERN_FIXTURE);
        let unwrap_fn = find_function(&items.0, "unwrap");
        let case_expr = match only_expression(unwrap_fn) {
            AExpr::CaseExpression(case) => case,
            other => panic!("expected case expression, got {other:?}"),
        };

        assert_eq!(case_expr.clauses.len(), 3, "expected three case clauses");

        match &case_expr.clauses[0].pattern {
            PatternExpr::Constructor { constructor, .. } => {
                if let ConcreteToken::Iden(name) = &constructor.token {
                    assert_eq!(name, "None");
                } else {
                    panic!("expected constructor identifier for None");
                }
            }
            other => panic!("expected constructor pattern for None, got {other:?}"),
        }

        match &case_expr.clauses[1].pattern {
            PatternExpr::Constructor {
                constructor,
                args: PatternConstructorArgs::Positional(args),
                ..
            } => {
                if let ConcreteToken::Iden(name) = &constructor.token {
                    assert_eq!(name, "Some");
                } else {
                    panic!("expected constructor identifier for Some");
                }
                assert_eq!(args.len(), 1, "Some branch should bind one variable");
            }
            other => panic!("expected Some constructor pattern, got {other:?}"),
        }

        assert!(
            matches!(case_expr.clauses[2].pattern, PatternExpr::Wild),
            "final clause should be wildcard"
        );
    }

    #[test]
    fn nested_some_none_pattern() {
        let items = parse_top_level(NESTED_FIXTURE);
        let nested_fn = find_function(&items.0, "nested");
        let case_expr = match only_expression(nested_fn) {
            AExpr::CaseExpression(case) => case,
            other => panic!("expected case expression, got {other:?}"),
        };

        match &case_expr.clauses[1].pattern {
            PatternExpr::Constructor {
                constructor,
                args: PatternConstructorArgs::Positional(inner),
                ..
            } => {
                if let ConcreteToken::Iden(name) = &constructor.token {
                    assert_eq!(name, "Some");
                }
                assert_eq!(inner.len(), 1);
                match &inner[0] {
                    PatternExpr::Constructor { constructor, .. } => {
                        if let ConcreteToken::Iden(name) = &constructor.token {
                            assert_eq!(name, "None");
                        } else {
                            panic!("expected nested None constructor");
                        }
                    }
                    other => panic!("expected nested None constructor, got {other:?}"),
                }
            }
            other => panic!("expected Some None pattern, got {other:?}"),
        }
    }

    #[test]
    fn nested_some_some_pattern() {
        let items = parse_top_level(NESTED_FIXTURE);
        let nested_fn = find_function(&items.0, "nested");
        let case_expr = match only_expression(nested_fn) {
            AExpr::CaseExpression(case) => case,
            other => panic!("expected case expression, got {other:?}"),
        };

        match &case_expr.clauses[2].pattern {
            PatternExpr::Constructor {
                constructor,
                args: PatternConstructorArgs::Positional(arg),
                ..
            } => {
                if let ConcreteToken::Iden(name) = &constructor.token {
                    assert_eq!(name, "Some");
                }
                assert_eq!(arg.len(), 1);
                match &arg[0] {
                    PatternExpr::Constructor {
                        constructor,
                        args: PatternConstructorArgs::Positional(inner),
                        ..
                    } => {
                        if let ConcreteToken::Iden(name) = &constructor.token {
                            assert_eq!(name, "Some");
                        }
                        assert_eq!(inner.len(), 1);
                        assert!(
                            matches!(inner[0], PatternExpr::Variable(_)),
                            "expected inner variable pattern"
                        );
                    }
                    other => panic!("expected nested Some constructor, got {other:?}"),
                }
            }
            other => panic!("expected Some (Some _) pattern, got {other:?}"),
        }
    }
}
