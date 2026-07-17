// Streaming pattern parser that constructs `PatternExpr` values directly from token
// streams, avoiding repeated slicing or backtracking
use std::marker::PhantomData;

use super::abstr_structures::*;
use super::concrete_token::*;
use super::loc::*;
use super::parser::{Parser, TokenStreamExt};

#[derive(Debug)]
pub enum PatternParseError {
    EmptyPattern,
    InvalidPattern(String),
    UnexpectedToken { expected: String, got: String },
}

pub fn is_constructor_name(name: &str) -> bool {
    name.chars().next().map_or(false, char::is_uppercase)
}

// check if it's starting with: wildcard / literal/ identifier (constructor/variable) / parenthesized pattern
pub(crate) fn is_pattern_start_token(token: &ConcreteToken) -> bool {
    matches!(
        token,
        ConcreteToken::Underscore
            | ConcreteToken::LiteralNumeric(_)
            | ConcreteToken::LiteralString(_)
            | ConcreteToken::Iden(_)
            | ConcreteToken::ParenL
    )
}

pub fn parse_pattern(tokens: &[ConcreteTokenAndLoc]) -> Result<PatternExpr, PatternParseError> {
    let mut parser = Parser::new(tokens.iter());
    let pattern = PatternParser::new(&mut parser).parse_pattern()?;

    if TokenStreamExt::peek_non_trivial(&mut parser).is_some() {
        return Err(PatternParseError::InvalidPattern(
            "unexpected tokens after pattern".to_string(),
        ));
    }

    Ok(pattern)
}

pub fn parse_pattern_stream<'a, S>(stream: &mut S) -> Result<PatternExpr, PatternParseError>
where
    S: TokenStreamExt<'a>,
{
    PatternParser::new(stream).parse_pattern()
}

struct PatternParser<'stream, 'a, S>
where
    S: TokenStreamExt<'a>,
{
    stream: &'stream mut S,
    _marker: PhantomData<&'a ()>,
}

impl<'stream, 'a, S> PatternParser<'stream, 'a, S>
where
    S: TokenStreamExt<'a>,
{
    fn new(stream: &'stream mut S) -> Self {
        Self {
            stream,
            _marker: PhantomData,
        }
    }

    fn parse_pattern(&mut self) -> Result<PatternExpr, PatternParseError> {
        let head = self.parse_atom_pattern()?;

        // optional range suffix: <literal> .. <literal>
        if let Some(peek) = TokenStreamExt::peek_non_trivial(self.stream) {
            if matches!(peek.token, ConcreteToken::Ellipse) {
                // consume '..'
                TokenStreamExt::next_non_trivial(self.stream);
                let tail = self.parse_atom_pattern()?;
                match (head, tail) {
                    (PatternExpr::Literal(start), PatternExpr::Literal(end)) => {
                        if matches!(start.expr, AExpr::StringExpr(_))
                            || matches!(end.expr, AExpr::StringExpr(_))
                        {
                            return Err(PatternParseError::InvalidPattern(
                                "string range patterns are not supported".to_string(),
                            ));
                        }
                        return Ok(PatternExpr::Range {
                            start: PatternRangeBound::Inclusive(start),
                            end: PatternRangeBound::Exclusive(end),
                        });
                    }
                    _ => {
                        return Err(PatternParseError::InvalidPattern(
                            "range bounds must be literal patterns".to_string(),
                        ));
                    }
                }
            }
        }

        Ok(head)
    }

    fn parse_atom_pattern(&mut self) -> Result<PatternExpr, PatternParseError> {
        let head = match TokenStreamExt::peek_non_trivial(self.stream) {
            Some(token) => token.clone(),
            None => return Err(PatternParseError::EmptyPattern),
        };

        match &head.token {
            ConcreteToken::Underscore => {
                TokenStreamExt::next_non_trivial(self.stream);
                Ok(PatternExpr::Wild)
            }
            ConcreteToken::LiteralNumeric(_) => {
                let token = TokenStreamExt::next_non_trivial(self.stream)
                    .expect("peeked literal must be available");
                Ok(PatternExpr::Literal(AExprAnnot {
                    expr: AExpr::NumericExpr(LiteralNumericExpr { literal: token }),
                    type_expr: None,
                }))
            }
            ConcreteToken::LiteralString(_) => {
                let token = TokenStreamExt::next_non_trivial(self.stream)
                    .expect("peeked literal must be available");
                Ok(PatternExpr::Literal(AExprAnnot {
                    expr: AExpr::StringExpr(LiteralStringExpr { literal: token }),
                    type_expr: None,
                }))
            }
            ConcreteToken::ParenL => self.parse_parenthesized(),
            ConcreteToken::Iden(name) => {
                let token = TokenStreamExt::next_non_trivial(self.stream)
                    .expect("peeked identifier must be available");
                if is_constructor_name(name) {
                    self.parse_constructor(token)
                } else {
                    Ok(PatternExpr::Variable(token))
                }
            }
            other => Err(PatternParseError::InvalidPattern(format!(
                "unexpected token in pattern: {:?}",
                other
            ))),
        }
    }

    fn parse_parenthesized(&mut self) -> Result<PatternExpr, PatternParseError> {
        // consume '('
        TokenStreamExt::next_non_trivial(self.stream);

        // special-case unit pattern: ()
        if let Some(peek) = TokenStreamExt::peek_non_trivial(self.stream) {
            if matches!(peek.token, ConcreteToken::ParenR) {
                // consume ')'
                TokenStreamExt::next_non_trivial(self.stream);
                return Ok(PatternExpr::Literal(AExprAnnot {
                    expr: AExpr::UnitExpr,
                    type_expr: None,
                }));
            }
        }

        let inner = self.parse_pattern()?;

        match TokenStreamExt::next_non_trivial(self.stream) {
            Some(token) if matches!(token.token, ConcreteToken::ParenR) => Ok(inner),
            Some(token) => Err(PatternParseError::UnexpectedToken {
                expected: ")".to_string(),
                got: format!("{:?}", token.token),
            }),
            None => Err(PatternParseError::InvalidPattern(
                "unclosed parenthesized pattern".to_string(),
            )),
        }
    }

    fn parse_constructor(
        &mut self,
        head: ConcreteTokenAndLoc,
    ) -> Result<PatternExpr, PatternParseError> {
        let mut qualified = None;
        let mut constructor = head.clone();

        if let Some(next) = TokenStreamExt::peek_non_trivial(self.stream) {
            if matches!(next.token, ConcreteToken::Dot) {
                TokenStreamExt::next_non_trivial(self.stream);
                let ctor_token =
                    TokenStreamExt::next_non_trivial(self.stream).ok_or_else(|| {
                        PatternParseError::InvalidPattern(
                            "expected constructor name after qualification".to_string(),
                        )
                    })?;

                match &ctor_token.token {
                    ConcreteToken::Iden(name) if is_constructor_name(name) => {
                        qualified = Some(head);
                        constructor = ctor_token;
                    }
                    ConcreteToken::Iden(name) => {
                        return Err(PatternParseError::InvalidPattern(format!(
                            "expected constructor name after '.', got {:?}",
                            name
                        )));
                    }
                    other => {
                        return Err(PatternParseError::InvalidPattern(format!(
                            "expected constructor name after '.', got {:?}",
                            other
                        )));
                    }
                }
            }
        }

        if let Some(next) = TokenStreamExt::peek_non_trivial(self.stream) {
            if matches!(next.token, ConcreteToken::BraceL) {
                return self.parse_record_pattern(qualified, constructor);
            }
        }

        let args = self.parse_constructor_args()?;
        Ok(PatternExpr::Constructor {
            qualified,
            constructor,
            args: PatternConstructorArgs::Positional(args),
        })
    }

    fn parse_constructor_args(&mut self) -> Result<Vec<PatternExpr>, PatternParseError> {
        let mut args = Vec::new();

        while let Some(next) = TokenStreamExt::peek_non_trivial(self.stream) {
            if !is_pattern_start_token(&next.token) {
                break;
            }

            let arg = self.parse_pattern()?;
            args.push(arg);
        }

        Ok(args)
    }

    fn parse_record_pattern(
        &mut self,
        qualified: Option<ConcreteTokenAndLoc>,
        constructor: ConcreteTokenAndLoc,
    ) -> Result<PatternExpr, PatternParseError> {
        // consume '{'
        TokenStreamExt::next_non_trivial(self.stream);
        let (fields, rest) = self.parse_record_fields()?;
        Ok(PatternExpr::Constructor {
            qualified,
            constructor,
            args: PatternConstructorArgs::Record { fields, rest },
        })
    }

    fn parse_record_fields(
        &mut self,
    ) -> Result<(Vec<(ConcreteTokenAndLoc, PatternExpr)>, bool), PatternParseError> {
        let mut fields = Vec::new();
        let mut rest = false;
        loop {
            let next = TokenStreamExt::peek_non_trivial(self.stream)
                .ok_or_else(|| PatternParseError::InvalidPattern("unclosed record pattern".into()))?
                .clone();

            match &next.token {
                ConcreteToken::BraceR => {
                    TokenStreamExt::next_non_trivial(self.stream);
                    break;
                }
                ConcreteToken::Comma => {
                    TokenStreamExt::next_non_trivial(self.stream);
                }
                ConcreteToken::Ellipse => {
                    if rest {
                        return Err(PatternParseError::InvalidPattern(
                            "duplicate '..' in record pattern".to_string(),
                        ));
                    }
                    rest = true;
                    TokenStreamExt::next_non_trivial(self.stream);

                    if let Some(after_rest) = TokenStreamExt::peek_non_trivial(self.stream) {
                        if matches!(after_rest.token, ConcreteToken::Comma) {
                            TokenStreamExt::next_non_trivial(self.stream);
                        }
                    }

                    match TokenStreamExt::next_non_trivial(self.stream) {
                        Some(token) if matches!(token.token, ConcreteToken::BraceR) => break,
                        Some(token) => {
                            return Err(PatternParseError::UnexpectedToken {
                                expected: "}".to_string(),
                                got: format!("{:?}", token.token),
                            });
                        }
                        None => {
                            return Err(PatternParseError::InvalidPattern(
                                "unclosed record pattern after '..'".to_string(),
                            ));
                        }
                    }
                }
                ConcreteToken::Iden(_) => {
                    let field_token = TokenStreamExt::next_non_trivial(self.stream)
                        .expect("peeked identifier must be available");

                    let field_pattern =
                        if let Some(eq) = TokenStreamExt::peek_non_trivial(self.stream) {
                            if matches!(eq.token, ConcreteToken::Equal) {
                                TokenStreamExt::next_non_trivial(self.stream);
                                self.parse_pattern()?
                            } else {
                                PatternExpr::Variable(field_token.clone())
                            }
                        } else {
                            PatternExpr::Variable(field_token.clone())
                        };

                    fields.push((field_token, field_pattern));
                }
                other => {
                    return Err(PatternParseError::InvalidPattern(format!(
                        "unexpected token in record pattern: {:?}",
                        other
                    )));
                }
            }
        }

        Ok((fields, rest))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_token(token: ConcreteToken) -> ConcreteTokenAndLoc {
        ConcreteTokenAndLoc {
            token,
            loc: Location::dummy(),
        }
    }

    #[test]
    fn parse_wildcard() {
        let tokens = vec![make_token(ConcreteToken::Underscore)];
        let pattern = parse_pattern(&tokens).unwrap();
        assert!(matches!(pattern, PatternExpr::Wild));
    }

    #[test]
    fn parse_variable() {
        let tokens = vec![make_token(ConcreteToken::Iden("x".to_string()))];
        let pattern = parse_pattern(&tokens).unwrap();
        assert!(matches!(pattern, PatternExpr::Variable(_)));
    }

    #[test]
    fn parse_constructor_no_args() {
        let tokens = vec![make_token(ConcreteToken::Iden("None".to_string()))];
        let pattern = parse_pattern(&tokens).unwrap();
        if let PatternExpr::Constructor {
            qualified,
            constructor,
            args: PatternConstructorArgs::Positional(args),
        } = pattern
        {
            assert!(qualified.is_none());
            assert!(args.is_empty());
            assert!(matches!(constructor.token, ConcreteToken::Iden(_)));
        } else {
            panic!("expected constructor pattern");
        }
    }

    #[test]
    fn parse_constructor_with_args() {
        let tokens = vec![
            make_token(ConcreteToken::Iden("Some".to_string())),
            make_token(ConcreteToken::Iden("x".to_string())),
        ];
        let pattern = parse_pattern(&tokens).unwrap();
        match pattern {
            PatternExpr::Constructor {
                args: PatternConstructorArgs::Positional(args),
                ..
            } => {
                assert_eq!(args.len(), 1);
                assert!(matches!(args[0], PatternExpr::Variable(_)));
            }
            other => panic!("expected constructor pattern, got {:?}", other),
        }
    }

    #[test]
    fn parse_parenthesized_pattern() {
        let tokens = vec![
            make_token(ConcreteToken::ParenL),
            make_token(ConcreteToken::Iden("x".to_string())),
            make_token(ConcreteToken::ParenR),
        ];
        let pattern = parse_pattern(&tokens).unwrap();
        assert!(matches!(pattern, PatternExpr::Variable(_)));
    }

    #[test]
    fn parse_unit_pattern() {
        let tokens = vec![
            make_token(ConcreteToken::ParenL),
            make_token(ConcreteToken::ParenR),
        ];
        let pattern = parse_pattern(&tokens).unwrap();
        match pattern {
            PatternExpr::Literal(AExprAnnot {
                expr: AExpr::UnitExpr,
                ..
            }) => {}
            other => panic!("expected unit literal pattern, got {:?}", other),
        }
    }

    #[test]
    fn parse_constructor_with_unit_arg_pattern() {
        let tokens = vec![
            make_token(ConcreteToken::Iden("Some".to_string())),
            make_token(ConcreteToken::ParenL),
            make_token(ConcreteToken::ParenR),
        ];
        let pattern = parse_pattern(&tokens).unwrap();
        match pattern {
            PatternExpr::Constructor {
                args: PatternConstructorArgs::Positional(args),
                ..
            } => {
                assert_eq!(args.len(), 1);
                assert!(matches!(
                    args[0],
                    PatternExpr::Literal(AExprAnnot {
                        expr: AExpr::UnitExpr,
                        ..
                    })
                ));
            }
            other => panic!(
                "expected constructor pattern with unit arg, got {:?}",
                other
            ),
        }
    }

    #[test]
    fn parse_record_pattern_with_unit_field() {
        let tokens = vec![
            make_token(ConcreteToken::Iden("Point".to_string())),
            make_token(ConcreteToken::BraceL),
            make_token(ConcreteToken::Iden("x".to_string())),
            make_token(ConcreteToken::Equal),
            make_token(ConcreteToken::ParenL),
            make_token(ConcreteToken::ParenR),
            make_token(ConcreteToken::BraceR),
        ];
        let pattern = parse_pattern(&tokens).unwrap();
        match pattern {
            PatternExpr::Constructor {
                args: PatternConstructorArgs::Record { fields, .. },
                ..
            } => {
                assert_eq!(fields.len(), 1);
                let (_name, field_pat) = &fields[0];
                assert!(matches!(
                    field_pat,
                    PatternExpr::Literal(AExprAnnot {
                        expr: AExpr::UnitExpr,
                        ..
                    })
                ));
            }
            other => panic!("expected record pattern with unit field, got {:?}", other),
        }
    }

    #[test]
    fn parse_qualified_constructor() {
        let tokens = vec![
            make_token(ConcreteToken::Iden("Option".to_string())),
            make_token(ConcreteToken::Dot),
            make_token(ConcreteToken::Iden("Some".to_string())),
            make_token(ConcreteToken::Iden("x".to_string())),
        ];
        let pattern = parse_pattern(&tokens).unwrap();
        match pattern {
            PatternExpr::Constructor {
                qualified,
                constructor,
                args: PatternConstructorArgs::Positional(args),
            } => {
                assert!(qualified.is_some());
                assert!(matches!(constructor.token, ConcreteToken::Iden(_)));
                assert_eq!(args.len(), 1);
            }
            other => panic!("expected qualified constructor, got {:?}", other),
        }
    }

    #[test]
    fn parse_record_pattern_with_fields() {
        let tokens = vec![
            make_token(ConcreteToken::Iden("Point".to_string())),
            make_token(ConcreteToken::BraceL),
            make_token(ConcreteToken::Iden("x".to_string())),
            make_token(ConcreteToken::Equal),
            make_token(ConcreteToken::Iden("a".to_string())),
            make_token(ConcreteToken::Comma),
            make_token(ConcreteToken::Iden("y".to_string())),
            make_token(ConcreteToken::Equal),
            make_token(ConcreteToken::Iden("b".to_string())),
            make_token(ConcreteToken::BraceR),
        ];
        let pattern = parse_pattern(&tokens).unwrap();
        match pattern {
            PatternExpr::Constructor {
                args: PatternConstructorArgs::Record { fields, rest },
                ..
            } => {
                assert_eq!(fields.len(), 2);
                assert!(!rest);
            }
            other => panic!("expected record pattern, got {:?}", other),
        }
    }

    #[test]
    fn parse_record_pattern_with_punning() {
        let tokens = vec![
            make_token(ConcreteToken::Iden("Point".to_string())),
            make_token(ConcreteToken::BraceL),
            make_token(ConcreteToken::Iden("x".to_string())),
            make_token(ConcreteToken::Comma),
            make_token(ConcreteToken::Iden("y".to_string())),
            make_token(ConcreteToken::BraceR),
        ];
        let pattern = parse_pattern(&tokens).unwrap();
        match pattern {
            PatternExpr::Constructor {
                args: PatternConstructorArgs::Record { fields, rest },
                ..
            } => {
                assert_eq!(fields.len(), 2);
                assert!(!rest);
                assert!(matches!(fields[0].1, PatternExpr::Variable(_)));
            }
            other => panic!("expected record pattern with punning, got {:?}", other),
        }
    }

    #[test]
    fn parse_record_pattern_with_rest() {
        let tokens = vec![
            make_token(ConcreteToken::Iden("Point".to_string())),
            make_token(ConcreteToken::BraceL),
            make_token(ConcreteToken::Iden("x".to_string())),
            make_token(ConcreteToken::Comma),
            make_token(ConcreteToken::Ellipse),
            make_token(ConcreteToken::BraceR),
        ];
        let pattern = parse_pattern(&tokens).unwrap();
        match pattern {
            PatternExpr::Constructor {
                args: PatternConstructorArgs::Record { fields, rest },
                ..
            } => {
                assert_eq!(fields.len(), 1);
                assert!(rest);
            }
            other => panic!("expected record pattern with rest, got {:?}", other),
        }
    }

    #[test]
    fn parse_numeric_range_pattern() {
        let tokens = vec![
            make_token(ConcreteToken::LiteralNumeric("0".to_string())),
            make_token(ConcreteToken::Ellipse),
            make_token(ConcreteToken::LiteralNumeric("3".to_string())),
        ];
        let pattern = parse_pattern(&tokens).unwrap();
        match pattern {
            PatternExpr::Range { start, end } => {
                if let PatternRangeBound::Inclusive(AExprAnnot {
                    expr: AExpr::NumericExpr(lit),
                    ..
                }) = start
                {
                    assert_eq!(lit.literal.token, ConcreteToken::LiteralNumeric("0".into()));
                } else {
                    panic!("expected inclusive numeric start bound");
                }
                if let PatternRangeBound::Exclusive(AExprAnnot {
                    expr: AExpr::NumericExpr(lit),
                    ..
                }) = end
                {
                    assert_eq!(lit.literal.token, ConcreteToken::LiteralNumeric("3".into()));
                } else {
                    panic!("expected exclusive numeric end bound");
                }
            }
            other => panic!("expected range pattern, got {:?}", other),
        }
    }

    #[test]
    fn parse_string_range_pattern_is_rejected() {
        let tokens = vec![
            make_token(ConcreteToken::LiteralString("a".to_string())),
            make_token(ConcreteToken::Ellipse),
            make_token(ConcreteToken::LiteralString("z".to_string())),
        ];
        let err = parse_pattern(&tokens).unwrap_err();
        match err {
            PatternParseError::InvalidPattern(msg) => {
                assert!(
                    msg.contains("string range patterns"),
                    "unexpected error: {msg}"
                );
            }
            other => panic!("expected invalid pattern error, got {:?}", other),
        }
    }

    #[test]
    fn test_nested_constructor_in_field() {
        // Test: Point { x = Some y }
        let tokens = vec![
            make_token(ConcreteToken::Iden("Point".to_string())),
            make_token(ConcreteToken::BraceL),
            make_token(ConcreteToken::Iden("x".to_string())),
            make_token(ConcreteToken::Equal),
            make_token(ConcreteToken::Iden("Some".to_string())),
            make_token(ConcreteToken::Iden("y".to_string())),
            make_token(ConcreteToken::BraceR),
        ];

        let pattern = parse_pattern(&tokens).unwrap();
        match pattern {
            PatternExpr::Constructor {
                args: PatternConstructorArgs::Record { fields, .. },
                ..
            } => {
                assert_eq!(fields.len(), 1);
                // Check that the field pattern is a constructor pattern
                match &fields[0].1 {
                    PatternExpr::Constructor {
                        constructor,
                        args: PatternConstructorArgs::Positional(args),
                        ..
                    } => {
                        if let ConcreteToken::Iden(name) = &constructor.token {
                            assert_eq!(name, "Some");
                        }
                        assert_eq!(args.len(), 1);
                        // Check that the argument is a variable pattern
                        assert!(matches!(&args[0], PatternExpr::Variable(_)));
                    }
                    _ => panic!("Expected constructor pattern in field"),
                }
            }
            _ => panic!("Expected record pattern"),
        }
    }

    #[test]
    fn test_deeply_nested_record_patterns() {
        // Test: Outer { a = Inner { b = Some c } }
        let tokens = vec![
            make_token(ConcreteToken::Iden("Outer".to_string())),
            make_token(ConcreteToken::BraceL),
            make_token(ConcreteToken::Iden("a".to_string())),
            make_token(ConcreteToken::Equal),
            make_token(ConcreteToken::Iden("Inner".to_string())),
            make_token(ConcreteToken::BraceL),
            make_token(ConcreteToken::Iden("b".to_string())),
            make_token(ConcreteToken::Equal),
            make_token(ConcreteToken::Iden("Some".to_string())),
            make_token(ConcreteToken::Iden("c".to_string())),
            make_token(ConcreteToken::BraceR),
            make_token(ConcreteToken::BraceR),
        ];

        let pattern = parse_pattern(&tokens).unwrap();
        match pattern {
            PatternExpr::Constructor {
                args: PatternConstructorArgs::Record { fields, .. },
                ..
            } => {
                assert_eq!(fields.len(), 1);
                // Check nested record pattern
                match &fields[0].1 {
                    PatternExpr::Constructor {
                        args:
                            PatternConstructorArgs::Record {
                                fields: inner_fields,
                                ..
                            },
                        ..
                    } => {
                        assert_eq!(inner_fields.len(), 1);
                        // Check constructor pattern inside inner record
                        match &inner_fields[0].1 {
                            PatternExpr::Constructor {
                                constructor,
                                args: PatternConstructorArgs::Positional(args),
                                ..
                            } => {
                                if let ConcreteToken::Iden(name) = &constructor.token {
                                    assert_eq!(name, "Some");
                                }
                                assert_eq!(args.len(), 1);
                            }
                            _ => panic!("Expected constructor pattern in nested field"),
                        }
                    }
                    _ => panic!("Expected nested record pattern"),
                }
            }
            _ => panic!("Expected record pattern"),
        }
    }

    #[test]
    fn test_wildcard_in_field() {
        // Test: Point { x = _, y = z }
        let tokens = vec![
            make_token(ConcreteToken::Iden("Point".to_string())),
            make_token(ConcreteToken::BraceL),
            make_token(ConcreteToken::Iden("x".to_string())),
            make_token(ConcreteToken::Equal),
            make_token(ConcreteToken::Underscore),
            make_token(ConcreteToken::Comma),
            make_token(ConcreteToken::Iden("y".to_string())),
            make_token(ConcreteToken::Equal),
            make_token(ConcreteToken::Iden("z".to_string())),
            make_token(ConcreteToken::BraceR),
        ];

        let pattern = parse_pattern(&tokens).unwrap();
        match pattern {
            PatternExpr::Constructor {
                args: PatternConstructorArgs::Record { fields, .. },
                ..
            } => {
                assert_eq!(fields.len(), 2);
                // Check first field has wildcard pattern
                assert!(matches!(&fields[0].1, PatternExpr::Wild));
                // Check second field has variable pattern
                assert!(matches!(&fields[1].1, PatternExpr::Variable(_)));
            }
            _ => panic!("Expected record pattern"),
        }
    }

    #[test]
    fn test_literal_in_field() {
        // Test: Config { port = 8080, host = "localhost" }
        let tokens = vec![
            make_token(ConcreteToken::Iden("Config".to_string())),
            make_token(ConcreteToken::BraceL),
            make_token(ConcreteToken::Iden("port".to_string())),
            make_token(ConcreteToken::Equal),
            make_token(ConcreteToken::LiteralNumeric("8080".to_string())),
            make_token(ConcreteToken::Comma),
            make_token(ConcreteToken::Iden("host".to_string())),
            make_token(ConcreteToken::Equal),
            make_token(ConcreteToken::LiteralString("localhost".to_string())),
            make_token(ConcreteToken::BraceR),
        ];

        let pattern = parse_pattern(&tokens).unwrap();
        match pattern {
            PatternExpr::Constructor {
                args: PatternConstructorArgs::Record { fields, .. },
                ..
            } => {
                assert_eq!(fields.len(), 2);
                // Check first field has literal pattern
                assert!(matches!(&fields[0].1, PatternExpr::Literal(_)));
                // Check second field has literal pattern
                assert!(matches!(&fields[1].1, PatternExpr::Literal(_)));
            }
            _ => panic!("Expected record pattern"),
        }
    }

    #[test]
    fn test_parenthesized_pattern_in_field() {
        // Test: Data { value = (Cons x xs) }
        let tokens = vec![
            make_token(ConcreteToken::Iden("Data".to_string())),
            make_token(ConcreteToken::BraceL),
            make_token(ConcreteToken::Iden("value".to_string())),
            make_token(ConcreteToken::Equal),
            make_token(ConcreteToken::ParenL),
            make_token(ConcreteToken::Iden("Cons".to_string())),
            make_token(ConcreteToken::Iden("x".to_string())),
            make_token(ConcreteToken::Iden("xs".to_string())),
            make_token(ConcreteToken::ParenR),
            make_token(ConcreteToken::BraceR),
        ];

        let pattern = parse_pattern(&tokens).unwrap();
        match pattern {
            PatternExpr::Constructor {
                args: PatternConstructorArgs::Record { fields, .. },
                ..
            } => {
                assert_eq!(fields.len(), 1);
                // Check that the field pattern is a constructor
                match &fields[0].1 {
                    PatternExpr::Constructor {
                        constructor,
                        args: PatternConstructorArgs::Positional(args),
                        ..
                    } => {
                        if let ConcreteToken::Iden(name) = &constructor.token {
                            assert_eq!(name, "Cons");
                        }
                        assert_eq!(args.len(), 2);
                    }
                    _ => panic!("Expected constructor pattern in parentheses"),
                }
            }
            _ => panic!("Expected record pattern"),
        }
    }
}
