// Streaming pattern parser that constructs `PatternExpr` values directly from token
// streams, avoiding repeated slicing or backtracking
use super::abstr_structures::*;
use super::concrete_token::*;
use super::layout::{ParserToken, ParserTokenType};
use super::loc::*;
use super::parser::{LayoutFeedback, LayoutItemParser, ParseError, Parser};

#[derive(Debug)]
pub enum PatternParseError {
    EmptyPattern,
    InvalidPattern(String),
    UnexpectedToken { expected: String, got: String },
}

/// constructor need to start with an uppercase
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

fn pattern_parser_error(error: ParseError) -> PatternParseError {
    PatternParseError::InvalidPattern(format!("token stream error: {error}"))
}

pub(crate) trait PatternTokenStream {
    fn peek_pattern_concrete(&mut self) -> Result<Option<ConcreteTokenAndLoc>, PatternParseError>;

    fn next_pattern_concrete(&mut self) -> Result<Option<ConcreteTokenAndLoc>, PatternParseError>;
}

fn peek_concrete(parser: &mut Parser) -> Result<Option<ConcreteTokenAndLoc>, PatternParseError> {
    match parser.peek().map_err(pattern_parser_error)? {
        Some(ParserToken {
            ty: ParserTokenType::Concrete(token),
            loc,
        }) => Ok(Some(ConcreteTokenAndLoc {
            token: token.clone(),
            loc: loc.clone(),
            starts_a_line: false,
        })),
        Some(ParserToken {
            ty: ParserTokenType::LayoutSeparator(_) | ParserTokenType::LayoutEnd(_),
            ..
        })
        | None => Ok(None),
        Some(ParserToken {
            ty: ParserTokenType::LayoutStart(_),
            ..
        }) => Err(PatternParseError::InvalidPattern(
            "unexpected layout start inside pattern".to_string(),
        )),
    }
}

fn next_concrete(parser: &mut Parser) -> Result<Option<ConcreteTokenAndLoc>, PatternParseError> {
    match parser.peek().map_err(pattern_parser_error)? {
        Some(ParserToken {
            ty: ParserTokenType::Concrete(_),
            ..
        }) => parser
            .next()
            .map_err(pattern_parser_error)
            .map(|token| token.and_then(|token| token.tok_concrete_and_loc())),
        Some(ParserToken {
            ty: ParserTokenType::LayoutSeparator(_) | ParserTokenType::LayoutEnd(_),
            ..
        })
        | None => Ok(None),
        Some(ParserToken {
            ty: ParserTokenType::LayoutStart(_),
            ..
        }) => Err(PatternParseError::InvalidPattern(
            "unexpected layout start inside pattern".to_string(),
        )),
    }
}

impl PatternTokenStream for Parser {
    fn peek_pattern_concrete(&mut self) -> Result<Option<ConcreteTokenAndLoc>, PatternParseError> {
        peek_concrete(self)
    }

    fn next_pattern_concrete(&mut self) -> Result<Option<ConcreteTokenAndLoc>, PatternParseError> {
        next_concrete(self)
    }
}

impl PatternTokenStream for LayoutItemParser<'_> {
    fn peek_pattern_concrete(&mut self) -> Result<Option<ConcreteTokenAndLoc>, PatternParseError> {
        self.peek_concrete().map_err(pattern_parser_error)
    }

    fn next_pattern_concrete(&mut self) -> Result<Option<ConcreteTokenAndLoc>, PatternParseError> {
        if self
            .peek_concrete()
            .map_err(pattern_parser_error)?
            .is_none()
        {
            return Ok(None);
        }
        self.next_concrete().map(Some).map_err(pattern_parser_error)
    }
}

pub fn parse_pattern(tokens: &[ConcreteTokenAndLoc]) -> Result<PatternExpr, PatternParseError> {
    let mut parser = Parser::new(LexedTokensAndLocs(tokens.to_vec()));
    let pattern = PatternParser::new(&mut parser).parse_pattern()?;

    if peek_concrete(&mut parser)?.is_some() {
        return Err(PatternParseError::InvalidPattern(
            "unexpected tokens after pattern".to_string(),
        ));
    }

    Ok(pattern)
}

pub fn parse_pattern_stream(stream: &mut Parser) -> Result<PatternExpr, PatternParseError> {
    parse_pattern_source(stream)
}

pub(crate) fn parse_pattern_item_stream(
    stream: &mut LayoutItemParser<'_>,
) -> Result<PatternExpr, PatternParseError> {
    parse_pattern_source(stream)
}

pub(crate) fn parse_pattern_source<S: PatternTokenStream + ?Sized>(
    stream: &mut S,
) -> Result<PatternExpr, PatternParseError> {
    PatternParser::new(stream).parse_pattern()
}

struct PatternParser<'stream, S: PatternTokenStream + ?Sized> {
    stream: &'stream mut S,
}

impl<'stream, S: PatternTokenStream + ?Sized> PatternParser<'stream, S> {
    fn new(stream: &'stream mut S) -> Self {
        Self { stream }
    }

    fn peek(&mut self) -> Result<Option<ConcreteTokenAndLoc>, PatternParseError> {
        self.stream.peek_pattern_concrete()
    }

    fn next(&mut self) -> Result<Option<ConcreteTokenAndLoc>, PatternParseError> {
        self.stream.next_pattern_concrete()
    }

    fn parse_pattern(&mut self) -> Result<PatternExpr, PatternParseError> {
        let head = self.parse_atom_pattern()?;

        // optional range suffix: <literal> .. <literal>
        if let Some(peek) = self.peek()? {
            if matches!(peek.token, ConcreteToken::Ellipse) {
                // consume '..'
                self.next()?;
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
        let head = match self.peek()? {
            Some(token) => token,
            None => return Err(PatternParseError::EmptyPattern),
        };

        match &head.token {
            ConcreteToken::Underscore => {
                self.next()?;
                Ok(PatternExpr::Wild)
            }
            ConcreteToken::LiteralNumeric(_) => {
                let token = self.next()?.expect("peeked literal must be available");
                Ok(PatternExpr::Literal(AExprAnnot {
                    expr: AExpr::NumericExpr(LiteralNumericExpr { literal: token }),
                    type_expr: None,
                }))
            }
            ConcreteToken::LiteralString(_) => {
                let token = self.next()?.expect("peeked literal must be available");
                Ok(PatternExpr::Literal(AExprAnnot {
                    expr: AExpr::StringExpr(LiteralStringExpr { literal: token }),
                    type_expr: None,
                }))
            }
            ConcreteToken::ParenL => self.parse_parenthesized(),
            ConcreteToken::Iden(name) => {
                let token = self.next()?.expect("peeked identifier must be available");
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
        self.next()?;

        // special-case unit pattern: () and early return
        if let Some(peek) = self.peek()? {
            if matches!(peek.token, ConcreteToken::ParenR) {
                // consume ')'
                self.next()?;
                return Ok(PatternExpr::Literal(AExprAnnot {
                    expr: AExpr::UnitExpr,
                    type_expr: None,
                }));
            }
        }

        let inner = self.parse_pattern()?;

        // expect closing `)`
        match self.next()? {
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

        if let Some(next) = self.peek()? {
            if matches!(next.token, ConcreteToken::Dot) {
                // qualified name detected
                self.next()?;
                let ctor_token = self.next()?.ok_or_else(|| {
                    PatternParseError::InvalidPattern(
                        "expected constructor name after qualification".to_string(),
                    )
                })?;

                // get constructor name
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

        if let Some(next) = self.peek()? {
            if matches!(next.token, ConcreteToken::BraceL) {
                // record detected
                return self.parse_record_pattern(qualified, constructor);
            }
        }

        // non-record constructor
        let args = self.parse_constructor_args()?;
        Ok(PatternExpr::Constructor {
            qualified,
            constructor,
            args: PatternConstructorArgs::Positional(args),
        })
    }

    fn parse_constructor_args(&mut self) -> Result<Vec<PatternExpr>, PatternParseError> {
        let mut args = Vec::new();

        while let Some(next) = self.peek()? {
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
        self.next()?;
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
            let next = self.peek()?.ok_or_else(|| {
                PatternParseError::InvalidPattern("unclosed record pattern".into())
            })?;

            match &next.token {
                // closing `}` for record
                ConcreteToken::BraceR => {
                    self.next()?;
                    break;
                }
                ConcreteToken::Comma => {
                    self.next()?;
                }
                ConcreteToken::Ellipse => {
                    if rest {
                        return Err(PatternParseError::InvalidPattern(
                            "duplicate '..' in record pattern".to_string(),
                        ));
                    }
                    rest = true;
                    self.next()?;

                    if let Some(after_rest) = self.peek()? {
                        if matches!(after_rest.token, ConcreteToken::Comma) {
                            self.next()?;
                        }
                    }

                    match self.next()? {
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
                    let field_token = self.next()?.expect("peeked identifier must be available");

                    let field_pattern = if let Some(eq) = self.peek()? {
                        if matches!(eq.token, ConcreteToken::Equal) {
                            self.next()?;
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
    use crate::parse::lex::parse_content_to_concrete_tokens;
    use std::path::Path;

    fn make_token(token: ConcreteToken) -> ConcreteTokenAndLoc {
        ConcreteTokenAndLoc {
            token,
            loc: Location::dummy(),
            starts_a_line: false,
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
    fn pattern_stream_leaves_layout_boundaries_for_block_parser() {
        let lexed = parse_content_to_concrete_tokens(Path::new("dummy.path"), "  x\n  y")
            .expect("lexing aligned patterns should succeed");
        let mut parser = Parser::new(lexed);

        let patterns = parser
            .parse_layout_block(
                Location::dummy(),
                false,
                |stream| {
                    parse_pattern_item_stream(stream)
                        .map_err(|error| ParseError::message(format!("{error:?}"), None))
                },
                LayoutFeedback::None,
            )
            .expect("layout block should retain ownership of pattern boundaries");

        assert_eq!(patterns.len(), 2);
        assert!(
            patterns
                .iter()
                .all(|pattern| matches!(pattern, PatternExpr::Variable(_)))
        );
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
