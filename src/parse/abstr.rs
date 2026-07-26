/// -----------------------------------------------------------------------------
///
/// intended abstract grammar
///
/// program := layout(top_level_item*)
///         := [layoutStart]
///            top_level_item
///            [layoutDelimiter]
///            ...
///            [layoutEnd]
///
///   where layout tokens serves as implicit delimiters for repeated items
///
///   constructs that uses implicit layout list are:
///     - top-level items
///     - `let` bindings
///     - `case` clauses
///
///   other constructs,
///     function `=`
///     lambda/clause `->`
///     binding `=`
///     `in`
///   , are followed by 1 expression and do not open an implicit layout block
///
///   a parser for one layout item can consume nested layouts, but must leave its owning separator/
///   end unconsumed
///
/// ---
///
/// top level
///
///   top_level_item       := function_signature
///                         | function_definition
///                         | data_declaration
///   function_signature   := identifier `::` type_expr
///   function_definition  := identifier parameter* `=` expr
///   parameter            := pattern
///
/// ---
///
/// expressions
///
///   expr                 := pratt_expr (`::` type_expr)?
///   prefix_expr          := literal
///                         | variable
///                         | constructor_expr
///                         | `()`
///                         | `(` expr `)`
///                         | lambda_expr
///                         | let_expr
///                         | case_expr
///                         | prefix_operator expr
///   application_argument := literal
///                         | variable
///                         | constructor_expr
///                         | `()`
///                         | `(` expr `)`
///
///   lambda_expr          := `\` parameter+ `->` expr
///   let_expr             := `let` layout(binding+) `in` expr
///                        := `let` [layoutStart]
///                                 binding
///                                 [layoutSeparator]
///                                 ...
///                                 [layoutEnd]
///                           `in` expr
///   binding              := pattern (`::` type_expr)? `=` expr
///                         | identifier parameter+ (`::` type_expr)? `=` expr
///
///   case_expr            := `case` expr `of` layout(case_clause+)
///                        := `case` expr `of`
///                              [layoutStart]
///                              case_clause
///                              [layoutSeparator]
///                              ...
///                              [layoutEnd]
///
///   case_clause          := pattern (`|` expr)? `->` expr
///
/// ---
///
/// patterns
///
///   pattern              := pattern_atom
///                         | pattern_atom `..` pattern_atom
///
///   pattern_atom         := `_`
///                         | variable
///                         | literal
///                         | `(` pattern `)`
///                         | `()`
///                         | constructor pattern_atom*
///                         | qualified_constructor pattern_atom*
///                         | constructor record_pattern
///                         | qualified_constructor record_pattern
///
///   record_pattern       := `{`
///                              (record_pattern_field
///                               (`,` record_pattern_field)* `,`?)?
///                            `}`
///   record_pattern_field := identifier (`=` pattern)? | `..`
///
///   see `abstr_pattern.rs` for the validation rules
///
///   note: range pattern, x..y, currently only supports numeric literal
/// ---
///
/// types
///
///   type_expr            := type_application (`->` type_expr)?
///   type_application     := type_atom+
///   type_atom            := identifier | `()` | `(` type_expr `)`
///
/// ---
///
/// data declarations
///
///   data_declaration     := `data` constructor type_parameter* data_body
///   data_body            := record_data_body | sum_data_body
///   record_data_body     := `{`
///                              (record_field (`,` record_field)* `,`?)?
///                            `}`
///   record_field         := identifier `::` type_expr
///   sum_data_body        := `=` data_constructor (`|` data_constructor)*
///   data_constructor     := constructor type_atom*
///
/// ---
///
/// operator precedence (highest to lowest)
///   - application
///   - unary + - !
///   - * /
///   - + -
///   - < <= > >= ==
///   - &&
///   - ||
///   - expression annotation `::`
///
/// ---
///
///  other notes
///     - function body expression currently do not support multi-line block expressions
///       but may be extended in the future to support constructs such as multi-line do notation
///     - application left-associative and binds more tightly than infix operators
///     - type annotation applies to the complete preceding expression and ends it
///     - parentheses are required before applying or operating on an annotated expression
///     - variables start with lower-case identifier
///     - constructor start with upper-case identifier and may be qualified: `Option.Some`
///     - record construction uses explicit braces and comma delimiters: `MyStruct { x = 1, y = 2 }`
///     - these annotation positions are distinct:
///       x :: T = rhs       // binding annotation
///       x = rhs :: T       // RHS expression annotation
///     - product type is expressed as sum type with only 1 constructor variant
///     - type application associates left; `->` associates right; eg:
///       `T A B` <=> `(T A) B`
///       `A -> B -> C` <=> `A -> (B -> C)`
///
/// -----------------------------------------------------------------------------
use super::abstr_pattern::{PatternTokenStream, parse_pattern_source};
use super::abstr_structures::*;
use super::concrete_token::*;
use super::layout::*;
use super::loc::*;
use super::parser::{
    ConcreteTokenSource, LayoutFeedback, LayoutGrammarSource, LayoutItemParser, ParseError,
    ParseResult, Parser,
};
use super::printer::*;

// top-level grammar ---

pub(crate) fn parse_concrete_top_level(input: LexedTokensAndLocs) -> ParseResult<TopLevelItems> {
    let mut result = vec![];
    let mut parser = Parser::new(input);
    match parser.peek()? {
        Some(x) if matches!(x.ty, ParserTokenType::Concrete(ConcreteToken::EndOfFile)) => {}
        Some(x) => {
            if x.loc.span_start.col != 0 {
                return Err(ParseError::message(
                    "expect top level item to be aligned to column 0",
                    None,
                ));
            }
            let anchor = x.loc.clone();
            let allow_empty = true;
            let parser_fn = parse_top_level_item;
            result.extend(parser.parse_layout_block(
                anchor,
                allow_empty,
                parser_fn,
                LayoutFeedback::None,
            )?);
        }
        _ => {}
    }
    parser.expect_concrete(&ConcreteToken::EndOfFile)?;
    Ok(TopLevelItems(result))
}

fn parse_top_level_item(item: &mut LayoutItemParser<'_>) -> ParseResult<TopLevelItem> {
    // expect one of:
    //  - data type definition
    //  - a function definition or signature
    match item.next_concrete()? {
        ConcreteTokenAndLoc {
            token: ConcreteToken::Data,
            ..
        } => parse_data(item),
        item_token @ ConcreteTokenAndLoc {
            token: ConcreteToken::Iden(_),
            ..
        } => parse_top_level_identifier(item, item_token),
        _ => Err(ParseError::message("expect data record or function", None)),
    }
}

fn parse_top_level_identifier(
    item: &mut LayoutItemParser<'_>,
    identifier: ConcreteTokenAndLoc,
) -> ParseResult<TopLevelItem> {
    match item.peek_concrete()? {
        Some(ConcreteTokenAndLoc {
            token: ConcreteToken::IsType,
            ..
        }) => parse_function_signature(item, identifier),
        Some(ConcreteTokenAndLoc { token, .. })
            if is_pattern_token(&token) || matches!(token, ConcreteToken::Equal) =>
        {
            parse_function_definition(item, identifier)
        }
        _ => Err(ParseError::message(
            "expect function signature or functio definition",
            None,
        )),
    }
}

// data declarations ---

fn parse_data(parser: &mut LayoutItemParser<'_>) -> ParseResult<TopLevelItem> {
    let identifier = parser.next_concrete()?;
    if !matches!(identifier.token, ConcreteToken::Iden(_)) {
        return Err(ParseError::unexpected_token(
            "data type identifier",
            Some(identifier),
        ));
    }
    let params = parse_data_type_params(parser)?;
    match parser.peek_concrete()? {
        // record type
        Some(ConcreteTokenAndLoc {
            token: ConcreteToken::BraceL,
            ..
        }) => {
            let components = parse_data_record(parser)?;
            Ok(TopLevelItem::DataRecord(DataRecord {
                identifier,
                params,
                components,
            }))
        }
        // ADT sum type
        Some(ConcreteTokenAndLoc {
            token: ConcreteToken::Equal,
            ..
        }) => {
            let variants = parse_data_sum(parser)?;
            Ok(TopLevelItem::DataSum(DataSum {
                identifier,
                params,
                variants,
            }))
        }
        _ => Err(ParseError::message(
            "expected `{` or `=` after data type",
            None,
        )),
    }
}

fn parse_data_type_params(parser: &mut LayoutItemParser<'_>) -> ParseResult<Vec<ATypeExprComplex>> {
    let mut params = Vec::new();
    while let Some(token) = parser.peek_concrete()? {
        if !matches!(token.token, ConcreteToken::Iden(_)) {
            break;
        }
        params.push(parse_type_atom_source(parser)?);
    }
    Ok(params)
}

fn parse_data_record(
    parser: &mut LayoutItemParser<'_>,
) -> ParseResult<Vec<(ConcreteTokenAndLoc, ATypeExprComplex)>> {
    parser.expect_concrete(&ConcreteToken::BraceL)?;
    let mut fields = Vec::new();
    loop {
        // closing `}` for record
        if matches!(
            parser.peek_concrete()?,
            Some(ConcreteTokenAndLoc {
                token: ConcreteToken::BraceR,
                ..
            })
        ) {
            parser.next_concrete()?;
            return Ok(fields);
        }
        let field = parser.next_concrete()?;
        if !matches!(field.token, ConcreteToken::Iden(_)) {
            return Err(ParseError::unexpected_token(
                "record field identifier",
                Some(field),
            ));
        }
        parser.expect_concrete(&ConcreteToken::IsType)?;
        let ty = parse_type_expr_source(parser)?;
        fields.push((field, ty));
        match parser.peek_concrete()? {
            Some(ConcreteTokenAndLoc {
                token: ConcreteToken::Comma,
                ..
            }) => {
                parser.next_concrete()?;
            }
            // closing `}` for record
            Some(ConcreteTokenAndLoc {
                token: ConcreteToken::BraceR,
                ..
            }) => {}
            _ => {
                return Err(ParseError::message(
                    "expected comma or `}` in data record",
                    None,
                ));
            }
        }
    }
}

fn parse_data_sum(
    parser: &mut LayoutItemParser<'_>,
) -> ParseResult<Vec<(ConcreteTokenAndLoc, Vec<ATypeExprComplex>)>> {
    parser.expect_concrete(&ConcreteToken::Equal)?;
    let mut variants = Vec::new();
    loop {
        let ctor = parser.next_concrete()?;
        if !matches!(ctor.token, ConcreteToken::Iden(_)) {
            return Err(ParseError::unexpected_token("data constructor", Some(ctor)));
        }
        let mut args = Vec::new();
        while let Some(next) = parser.peek_concrete()? {
            if matches!(next.token, ConcreteToken::VertBar) {
                break;
            }
            if !matches!(next.token, ConcreteToken::Iden(_) | ConcreteToken::ParenL) {
                break;
            }
            args.push(parse_type_atom_source(parser)?);
        }
        variants.push((ctor, args));
        if matches!(
            parser.peek_concrete()?,
            Some(ConcreteTokenAndLoc {
                token: ConcreteToken::VertBar,
                ..
            })
        ) {
            parser.next_concrete()?;
        } else {
            break;
        }
    }
    Ok(variants)
}

// function and type grammar ---

fn parse_function_signature(
    parser: &mut LayoutItemParser<'_>,
    identifier: ConcreteTokenAndLoc,
) -> ParseResult<TopLevelItem> {
    parser.expect_concrete(&ConcreteToken::IsType)?;
    let ty = parse_type_expr_source(parser)?;
    Ok(TopLevelItem::FunctionSignature(FnSig { identifier, ty }))
}

fn parse_function_definition(
    parser: &mut LayoutItemParser<'_>,
    identifier: ConcreteTokenAndLoc,
) -> ParseResult<TopLevelItem> {
    let mut patterns = Vec::new();
    let mut param_patterns = Vec::new();
    // collect function parameters/patterns
    while let Some(next) = parser.peek_concrete()? {
        if matches!(next.token, ConcreteToken::Equal | ConcreteToken::IsType) {
            break;
        }
        let pattern = parse_pattern_source(parser)
            .map_err(|e| ParseError::message(format!("invalid function parameter: {e:?}"), None))?;
        patterns.push(pattern_token(&pattern));
        param_patterns.push(pattern);
    }
    // optionally parse type annotation
    let type_expr = if matches!(
        parser.peek_concrete()?,
        Some(ConcreteTokenAndLoc {
            token: ConcreteToken::IsType,
            ..
        })
    ) {
        parser.next_concrete()?; // consume `::`
        Some(parse_type_expr_source(parser)?)
    } else {
        None
    };
    parser.expect_concrete(&ConcreteToken::Equal)?;
    // parse function body expr
    let expr =
        parse_expr_source(parser, 0)?.ok_or_else(|| ParseError::unexpected_eof("function body"))?;
    Ok(TopLevelItem::FunctionDefinition(AbstractionExpr {
        name: Some(identifier),
        pattern: patterns,
        param_patterns,
        expr: Box::new(expr),
        type_expr,
    }))
}

fn parse_function_body(parser: &mut Parser) -> ParseResult<BlockExpr> {
    // for now, we expect only 1 expression
    // but we can use a block expression to support
    // constructs like do notation in the future
    Ok(BlockExpr(vec![parse_expr_source(parser, 0)?.ok_or_else(
        || ParseError::unexpected_eof("function body"),
    )?]))
}

/// parses a type expression, eg:
///   A -> B
///   C
///   (A -> B) C
///   A B C
///   (A B) C
///   A (B C)
///   A (B -> C)
fn parse_type_expr(parser: &mut Parser) -> ParseResult<ATypeExprComplex> {
    parse_type_expr_source(parser)
}

/// parses an atomic type expression, eg:
///   A
///   (type_expr)
///   ()
fn parse_type_atom(parser: &mut Parser) -> ParseResult<ATypeExprComplex> {
    parse_type_atom_source(parser)
}

fn parse_required_type_annotation(
    parser: &mut Parser,
    context: &'static str,
) -> ParseResult<ATypeExprComplex> {
    parser.expect_concrete(&ConcreteToken::IsType)?;
    parse_type_expr(parser)
        .map_err(|_| ParseError::message(format!("expected type annotation for {context}"), None))
}

/// parses a type expression, eg:
///   A -> B
///   C
///   (A -> B) C
///   A B C
///   (A B) C
///   A (B C)
///   A (B -> C)
fn parse_type_expr_source(source: &mut impl ConcreteTokenSource) -> ParseResult<ATypeExprComplex> {
    let mut head = parse_type_atom_source(source)?;
    while let Some(next) = source.peek_concrete()? {
        if !matches!(next.token, ConcreteToken::Iden(_) | ConcreteToken::ParenL) {
            break;
        }
        // at this point, we have an identifier or '('

        // let parse_type_atom_source consume closing `)`
        let arg = parse_type_atom_source(source)?;

        match &mut head {
            ATypeExprComplex::Iden(id) => id.type_parameters.push(arg), // do a type application
            ATypeExprComplex::Fun(_) => {
                // A B->C is not valid
                return Err(ParseError::message(
                    "function type cannot be applied without parentheses",
                    None,
                ));
            }
        }
    }
    if matches!(
        source.peek_concrete()?,
        Some(ConcreteTokenAndLoc {
            token: ConcreteToken::ArrowRight,
            ..
        })
    ) {
        // arrow type detected, represent it with a linked list
        source.next_concrete()?;
        // recursion works since arrow type is right associative
        let tail = parse_type_expr_source(source)?;
        Ok(ATypeExprComplex::Fun(ATypeExprFun {
            head: std::sync::Arc::new(std::sync::Mutex::new(head)),
            tail: Some(std::sync::Arc::new(std::sync::Mutex::new(tail))),
        }))
    } else {
        Ok(head)
    }
}

/// parses an atomic type expression, eg:
///   A
///   (type_expr)
///   ()
fn parse_type_atom_source(source: &mut impl ConcreteTokenSource) -> ParseResult<ATypeExprComplex> {
    let token = source
        .next_concrete()?
        .ok_or_else(|| ParseError::unexpected_eof("type atom"))?;
    match token.token {
        // terminal case: type identifier
        ConcreteToken::Iden(_) => Ok(ATypeExprComplex::Iden(ATypeExprIden {
            identifier: token,
            type_parameters: Vec::new(),
        })),
        ConcreteToken::ParenL => {
            if matches!(
                source.peek_concrete()?,
                Some(ConcreteTokenAndLoc {
                    token: ConcreteToken::ParenR,
                    ..
                })
            ) {
                // terminal case:
                // () type
                // TODO: convert to a special unit type?
                source.next_concrete()?;
                return Ok(ATypeExprComplex::Iden(ATypeExprIden {
                    identifier: ConcreteTokenAndLoc {
                        token: ConcreteToken::Iden("()".into()),
                        loc: token.loc,
                        starts_a_line: false,
                    },
                    type_parameters: Vec::new(),
                }));
            }
            // recursion
            let ty = parse_type_expr_source(source)?;
            // consume closing ')'
            source.expect_concrete(&ConcreteToken::ParenR)?;
            Ok(ty)
        }
        other => Err(ParseError::unexpected_token(
            "type atom",
            Some(ConcreteTokenAndLoc {
                token: other,
                ..token
            }),
        )),
    }
}

// expression grammar ---

pub(crate) fn parse_expr(
    parser: &mut Parser,
    min_binding_power: usize,
) -> ParseResult<Option<AExprAnnot>> {
    parse_expr_source(parser, min_binding_power)
}

/// parse an expression using pratt parsing
fn parse_expr_source<S>(source: &mut S, min_binding_power: usize) -> ParseResult<Option<AExprAnnot>>
where
    S: ConcreteTokenSource + LayoutGrammarSource + PatternTokenStream,
{
    let Some(mut lhs) = parse_prefix_expr(source)? else {
        return Ok(None);
    };

    loop {
        let Some(next) = source.peek_concrete()? else {
            break;
        };

        if let Some(info) = infix_op_info(&next.token) {
            if info.lbp < min_binding_power {
                // infix op has less bind power than minimum required, so don't
                // apply this infix op
                break;
            }
            let operator = source.next_concrete()?.expect("peeked infix operator");
            // recursion
            let rhs = parse_expr_source(source, info.rbp)?.ok_or_else(|| {
                ParseError::unexpected_token("expression after infix operator", Some(next.clone()))
            })?;
            let builtin = AExprAnnot {
                expr: AExpr::IdentifierExpression(IdenExpr {
                    iden: ConcreteTokenAndLoc {
                        token: info.builtin_token,
                        loc: operator.loc.clone(),
                        starts_a_line: false,
                    },
                    builtin: Some(info.expr_type),
                }),
                type_expr: None,
            };
            // build the infix op with operands
            lhs = AExprAnnot {
                expr: AExpr::ApplyExpression(AppExpr {
                    fun: Box::new(builtin),
                    arguments: vec![lhs, rhs],
                }),
                type_expr: None,
            };
            continue;
        }

        // at this point, infix op failed to bind

        if is_expression_atom_start(&next.token) && APPLICATION_BINDING_POWER >= min_binding_power {
            // an application
            let argument = parse_prefix_expr(source)?.ok_or_else(|| {
                ParseError::unexpected_token("application argument", Some(next.clone()))
            })?;
            lhs = match lhs {
                // there exists an application expression, so append argument
                AExprAnnot {
                    expr: AExpr::ApplyExpression(mut application),
                    type_expr: None,
                } => {
                    application.arguments.push(argument);
                    AExprAnnot {
                        expr: AExpr::ApplyExpression(application),
                        type_expr: None,
                    }
                }
                // not an application expression, so make one
                other => AExprAnnot {
                    expr: AExpr::ApplyExpression(AppExpr {
                        fun: Box::new(other),
                        arguments: vec![argument],
                    }),
                    type_expr: None,
                },
            };
            continue;
        }

        break;
    }

    if min_binding_power == 0
        && matches!(
            source.peek_concrete()?,
            Some(ConcreteTokenAndLoc {
                token: ConcreteToken::IsType,
                ..
            })
        )
    {
        source.next_concrete()?;
        lhs.type_expr = Some(parse_type_expr_source(source)?);
    }
    Ok(Some(lhs))
}

fn parse_prefix_expr<S>(source: &mut S) -> ParseResult<Option<AExprAnnot>>
where
    S: ConcreteTokenSource + LayoutGrammarSource + PatternTokenStream,
{
    let Some(next) = source.peek_concrete()? else {
        return Ok(None);
    };

    match next.token {
        ConcreteToken::Iden(name) => {
            let token = source.next_concrete()?.expect("peeked identifier");
            if name.chars().next().is_some_and(char::is_uppercase) {
                // constructor (qualified or not)
                let mut qualified = None;
                let mut constructor = token;
                if matches!(
                    source.peek_concrete()?,
                    Some(ConcreteTokenAndLoc {
                        token: ConcreteToken::Dot,
                        ..
                    })
                ) {
                    source.next_concrete()?;
                    qualified = Some(constructor);
                    constructor = source
                        .next_concrete()?
                        .ok_or_else(|| ParseError::unexpected_eof("qualified constructor"))?;
                }
                let record_fields = if matches!(
                    source.peek_concrete()?,
                    Some(ConcreteTokenAndLoc {
                        token: ConcreteToken::BraceL,
                        ..
                    })
                ) {
                    Some(parse_record_constructor_fields_source(source)?)
                } else {
                    None
                };
                Ok(Some(AExprAnnot {
                    expr: AExpr::ConstructorExpression(ConstructorExpr {
                        qualified,
                        constructor,
                        args: Vec::new(),
                        record_fields,
                    }),
                    type_expr: None,
                }))
            } else {
                Ok(Some(AExprAnnot {
                    expr: AExpr::IdentifierExpression(IdenExpr {
                        iden: token,
                        builtin: None,
                    }),
                    type_expr: None,
                }))
            }
        }
        ConcreteToken::LiteralNumeric(_) => {
            let token = source.next_concrete()?.expect("peeked numeric literal");
            Ok(Some(AExprAnnot {
                expr: AExpr::NumericExpr(LiteralNumericExpr { literal: token }),
                type_expr: None,
            }))
        }
        ConcreteToken::LiteralString(_) => {
            let token = source.next_concrete()?.expect("peeked string literal");
            Ok(Some(AExprAnnot {
                expr: AExpr::StringExpr(LiteralStringExpr { literal: token }),
                type_expr: None,
            }))
        }
        ConcreteToken::ParenL => {
            source.next_concrete()?;
            if matches!(
                source.peek_concrete()?,
                Some(ConcreteTokenAndLoc {
                    token: ConcreteToken::ParenR,
                    ..
                })
            ) {
                // unit type
                source.next_concrete()?;
                return Ok(Some(AExprAnnot {
                    expr: AExpr::UnitExpr,
                    type_expr: None,
                }));
            }
            let expr = parse_expr_source(source, 0)?.ok_or_else(|| {
                ParseError::unexpected_token("expression inside parentheses", Some(next.clone()))
            })?;
            source.expect_concrete(&ConcreteToken::ParenR)?;
            Ok(Some(expr))
        }
        ConcreteToken::Case => {
            let keyword = source.next_concrete()?.expect("peeked case keyword");
            Ok(Some(parse_case_expr_source(source, keyword)?))
        }
        ConcreteToken::Let => {
            source.next_concrete()?;
            Ok(Some(parse_let_expr_source(source)?))
        }
        ConcreteToken::BackSlash => {
            source.next_concrete()?;
            Ok(Some(parse_lambda_expr_source(source)?))
        }
        ConcreteToken::Plus | ConcreteToken::Minus | ConcreteToken::Exclamation => {
            let info = prefix_op_info(&next.token).expect("matched prefix operator");
            let operator = source.next_concrete()?.expect("peeked prefix operator");
            // get the unary argument
            //
            // pass in the right binding power for the op as the minimal binding
            // power for parse_expr
            let operand = parse_expr_source(source, info.rbp)?.ok_or_else(|| {
                ParseError::unexpected_token(
                    "expression after prefix operator",
                    Some(operator.clone()),
                )
            })?;
            let builtin = AExprAnnot {
                expr: AExpr::IdentifierExpression(IdenExpr {
                    iden: ConcreteTokenAndLoc {
                        token: info.builtin_token,
                        loc: operator.loc,
                        starts_a_line: false,
                    },
                    builtin: Some(info.expr_type),
                }),
                type_expr: None,
            };
            Ok(Some(AExprAnnot {
                expr: AExpr::ApplyExpression(AppExpr {
                    fun: Box::new(builtin),
                    arguments: vec![operand],
                }),
                type_expr: None,
            }))
        }
        _ => Ok(None),
    }
}

fn is_expression_atom_start(token: &ConcreteToken) -> bool {
    matches!(
        token,
        ConcreteToken::Iden(_)
            | ConcreteToken::LiteralNumeric(_)
            | ConcreteToken::LiteralString(_)
            | ConcreteToken::ParenL
            | ConcreteToken::Case
            | ConcreteToken::Let
            | ConcreteToken::BackSlash
    )
}

fn parse_expression_block(parser: &mut Parser) -> ParseResult<BlockExpr> {
    let expr = parse_expr_source(parser, 0)?
        .ok_or_else(|| ParseError::unexpected_eof("expression block"))?;
    Ok(BlockExpr(vec![expr]))
}

fn parse_record_constructor_fields(
    parser: &mut Parser,
) -> ParseResult<Vec<(ConcreteTokenAndLoc, AExprAnnot)>> {
    parse_record_constructor_fields_source(parser)
}

fn parse_record_constructor_fields_source<S>(
    source: &mut S,
) -> ParseResult<Vec<(ConcreteTokenAndLoc, AExprAnnot)>>
where
    S: ConcreteTokenSource + LayoutGrammarSource + PatternTokenStream,
{
    source.expect_concrete(&ConcreteToken::BraceL)?;
    let mut fields = Vec::new();
    loop {
        // closing `}` for record
        if matches!(
            source.peek_concrete()?,
            Some(ConcreteTokenAndLoc {
                token: ConcreteToken::BraceR,
                ..
            })
        ) {
            source.next_concrete()?;
            return Ok(fields);
        }
        let field = source
            .next_concrete()?
            .ok_or_else(|| ParseError::unexpected_eof("record field"))?;
        if !matches!(field.token, ConcreteToken::Iden(_)) {
            return Err(ParseError::unexpected_token("record field", Some(field)));
        }
        source.expect_concrete(&ConcreteToken::Equal)?;
        let value = parse_expr_source(source, 0)?
            .ok_or_else(|| ParseError::unexpected_eof("record field value"))?;
        fields.push((field, value));
        match source.peek_concrete()? {
            Some(ConcreteTokenAndLoc {
                token: ConcreteToken::Comma,
                ..
            }) => {
                source.next_concrete()?;
            }
            Some(ConcreteTokenAndLoc {
                token: ConcreteToken::BraceR,
                ..
            }) => {}
            _ => {
                return Err(ParseError::message(
                    "expected comma or `}` in record expression",
                    None,
                ));
            }
        }
    }
}

fn parse_case_expr(parser: &mut Parser) -> ParseResult<AExprAnnot> {
    let keyword = parser.expect_concrete(&ConcreteToken::Case)?;
    parse_case_expr_source(parser, keyword)
}

fn parse_case_expr_source<S>(
    source: &mut S,
    keyword: ConcreteTokenAndLoc,
) -> ParseResult<AExprAnnot>
where
    S: ConcreteTokenSource + LayoutGrammarSource + PatternTokenStream,
{
    // clause expressions stop at their owning layout separator/end
    //
    // the enclosing layout helper consumes those markers after the callback returns
    let argument = parse_expr_source(source, 0)?
        .ok_or_else(|| ParseError::unexpected_token("case scrutinee", Some(keyword.clone())))?;
    source.expect_concrete(&ConcreteToken::Of)?;

    let first_clause = source
        .peek_concrete()?
        .ok_or_else(|| ParseError::unexpected_eof("case clause"))?;
    let clauses = source.parse_layout_items(
        first_clause.loc.clone(),
        false,
        parse_case_clause,
        LayoutFeedback::None,
    )?;

    Ok(AExprAnnot {
        expr: AExpr::CaseExpression(CaseExpr {
            keyword,
            argument: Box::new(argument),
            clauses,
        }),
        type_expr: None,
    })
}

fn parse_case_clause(item: &mut LayoutItemParser<'_>) -> ParseResult<CaseClause> {
    let pattern = parse_pattern_source(item)
        .map_err(|error| ParseError::message(format!("invalid case pattern: {error:?}"), None))?;

    let guard = if matches!(
        item.peek_concrete()?,
        Some(ConcreteTokenAndLoc {
            token: ConcreteToken::VertBar,
            ..
        })
    ) {
        item.next_concrete()?;
        Some(
            parse_expr_source(item, 0)?
                .ok_or_else(|| ParseError::unexpected_token("case guard", None))?,
        )
    } else {
        None
    };

    item.expect_concrete(&ConcreteToken::ArrowRight)?;
    let body = parse_expr_source(item, 0)?
        .ok_or_else(|| ParseError::unexpected_token("case clause body", None))?;

    Ok(CaseClause {
        pattern,
        guard,
        body: Box::new(body),
    })
}

fn parse_let_expr(parser: &mut Parser) -> ParseResult<AExprAnnot> {
    parse_let_expr_source(parser)
}

fn parse_let_def(parser: &mut Parser) -> ParseResult<(PatternExpr, AExprAnnot)> {
    let pattern = parse_pattern_source(parser)
        .map_err(|e| ParseError::message(format!("invalid let pattern: {e:?}"), None))?;
    let annotation = if matches!(
        parser.peek_concrete()?,
        Some(ConcreteTokenAndLoc {
            token: ConcreteToken::IsType,
            ..
        })
    ) {
        parser.next_concrete()?;
        Some(parse_type_expr_source(parser)?)
    } else {
        None
    };
    parser.expect_concrete(&ConcreteToken::Equal)?;
    let mut rhs = parse_expr_source(parser, 0)?
        .ok_or_else(|| ParseError::unexpected_eof("let binding RHS"))?;
    if let Some(ty) = annotation {
        rhs.type_expr = Some(ty);
    }
    Ok((pattern, rhs))
}

fn parse_lambda_expr(parser: &mut Parser) -> ParseResult<AExprAnnot> {
    parse_lambda_expr_source(parser)
}

fn parse_let_expr_source<S>(source: &mut S) -> ParseResult<AExprAnnot>
where
    S: ConcreteTokenSource + LayoutGrammarSource + PatternTokenStream,
{
    let first = source
        .peek_concrete()?
        .ok_or_else(|| ParseError::unexpected_eof("let binding"))?;
    let defs = source.parse_layout_items(
        first.loc.clone(),
        false,
        parse_let_def_item,
        LayoutFeedback::BeforeIn,
    )?;
    source.expect_concrete(&ConcreteToken::In)?;
    let expr =
        parse_expr_source(source, 0)?.ok_or_else(|| ParseError::unexpected_eof("let body"))?;
    Ok(AExprAnnot {
        expr: AExpr::LetExpression(LetExpr {
            defs,
            expr: Box::new(expr),
        }),
        type_expr: None,
    })
}

fn parse_let_def_item(item: &mut LayoutItemParser<'_>) -> ParseResult<(PatternExpr, AExprAnnot)> {
    // LHS pattern
    let pattern = parse_pattern_source(item)
        .map_err(|e| ParseError::message(format!("invalid let pattern: {:?}", e), None))?;
    // additional parameters on the LHS
    let mut parameters = Vec::new();
    while let Some(next) = item.peek_concrete()? {
        if matches!(next.token, ConcreteToken::IsType | ConcreteToken::Equal) {
            break;
        }
        let parameter = parse_pattern_source(item)
            .map_err(|e| ParseError::message(format!("invalid let parameter: {e:?}"), None))?;
        parameters.push(parameter);
    }
    let annotation = if matches!(
        item.peek_concrete()?,
        Some(ConcreteTokenAndLoc {
            token: ConcreteToken::IsType,
            ..
        })
    ) {
        item.next_concrete()?;
        Some(parse_type_expr_source(item)?)
    } else {
        None
    };
    item.expect_concrete(&ConcreteToken::Equal)?;
    // RHS
    let mut rhs =
        parse_expr_source(item, 0)?.ok_or_else(|| ParseError::unexpected_eof("let binding RHS"))?;
    if let Some(ty) = annotation {
        rhs.type_expr = Some(ty);
    }
    // for now, convert to a lambda if there are parameters on the LHS
    if !parameters.is_empty() {
        let param_patterns = parameters;
        let pattern_tokens = param_patterns
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let mut token = pattern_token(p);
                token.token = ConcreteToken::Iden(format!("__lambda_param_{i}"));
                token
            })
            .collect();
        rhs = AExprAnnot {
            expr: AExpr::AbstractionExpression(AbstractionExpr {
                name: None,
                pattern: pattern_tokens,
                param_patterns,
                expr: Box::new(rhs),
                type_expr: None,
            }),
            type_expr: None,
        };
    }
    Ok((pattern, rhs))
}

fn parse_lambda_expr_source<S>(source: &mut S) -> ParseResult<AExprAnnot>
where
    S: ConcreteTokenSource + LayoutGrammarSource + PatternTokenStream,
{
    let mut patterns = Vec::new();
    let mut param_patterns = Vec::new();
    loop {
        let next = source
            .peek_concrete()?
            .ok_or_else(|| ParseError::unexpected_eof("lambda parameter"))?;
        if matches!(next.token, ConcreteToken::ArrowRight) {
            break;
        }
        let pattern = parse_pattern_source(source)
            .map_err(|e| ParseError::message(format!("invalid lambda parameter: {e:?}"), None))?;
        patterns.push(pattern_token(&pattern));
        param_patterns.push(pattern);
    }
    if patterns.is_empty() {
        return Err(ParseError::message("lambda requires a parameter", None));
    }
    source.expect_concrete(&ConcreteToken::ArrowRight)?;
    let body =
        parse_expr_source(source, 0)?.ok_or_else(|| ParseError::unexpected_eof("lambda body"))?;
    Ok(AExprAnnot {
        expr: AExpr::AbstractionExpression(AbstractionExpr {
            name: None,
            pattern: patterns,
            param_patterns,
            expr: Box::new(body),
            type_expr: None,
        }),
        type_expr: None,
    })
}

fn pattern_token(pattern: &PatternExpr) -> ConcreteTokenAndLoc {
    let token = match pattern {
        PatternExpr::Wild => ConcreteToken::Underscore,
        PatternExpr::Variable(t) => t.token.clone(),
        PatternExpr::Literal(a) => match &a.expr {
            AExpr::NumericExpr(x) => x.literal.token.clone(),
            AExpr::StringExpr(x) => x.literal.token.clone(),
            _ => ConcreteToken::Iden("_pattern".into()),
        },
        PatternExpr::Range { .. } => ConcreteToken::Iden("_range".into()),
        PatternExpr::Constructor { constructor, .. } => constructor.token.clone(),
    };
    ConcreteTokenAndLoc {
        token,
        loc: Location::dummy(),
        starts_a_line: false,
    }
}

fn is_pattern_token(token: &ConcreteToken) -> bool {
    match token {
        ConcreteToken::Underscore
        | ConcreteToken::Iden(_)
        | ConcreteToken::LiteralNumeric(_)
        | ConcreteToken::LiteralString(_)
        | ConcreteToken::ParenL => true,
        _ => false,
    }
}

static TEST_CONTENT_ABSTRACT_PARSER: &str = r###"
// f x = 9 as f32 / ((-7*x as i32) as f32)
f x y z = 9 / (-7*x+y+z)

f_let a =
 let x :: u32 = 1
     y :: u32 = 2
     z :: u32 = 5
 in
   x + (y :: u32) + z + a

f_let a =
 let x :: u32 = 1
     y :: u32 = 2
     z :: u32 = 5
 in
   let b = 10
       c = b * b
   in x + y + z + a + b * c

f_let2 a =
 let x :: u32 = 1 + a
     y :: u32 = 2
     z :: u32 = 5
 in
   x + y + z

f_let_nest a =
 let x :: u32 = 1
     y :: u32 = 2
 in
   let z = x + y
   in z + a

ff x =
 let y = 1-(case z of
             "a"         -> (0 * 5)
             "something" -> x
             _           -> 2
           )*7
 in y

fff x =
 let y = (
          case z of
            "a"         -> (0 * 5)
            "something" -> case x of
                             0 -> 10
                             1 -> 11
                             _ -> 2 * x
            _           -> 2
         ) * 7
 in y

data T B { // constructor with record
  v :: u32,
  x :: B,
}

// sum constructor
data T2 A = T20 T A
              | T21 u32 
              | T22 u32 i32
              | T23 A A
              | T24 T3

data T3 = Blah u32 u32 i32

data Tree =
  Leaf u32
  | Node Tree Tree

//test type expressions in function signature and let expressions
f4 :: T (A -> B) -> T2 -> T3

f5 :: (T (A -> B) -> T2) -> T3 -> T4

fg :: T A B C (D AA
             ) -> T2 
                      E
                      F
                      (G H I)

f3 x =
  let a :: T u32 -> u32 = ff
      b :: f32 = 7
  in 
  x a b

add_and_square :: T -> T2 -> T3 T2
add_and_square x y =
//blah comments
  //more comments
  let z :: T2 = y * y
      ret = case z of
             "a"         -> 0 * 5
             "something" -> 1
             _           -> 2
  in
    x + z

simple :: u32
simple = 77

f3 :: T -> T2
f3 a =
  let b = 7.0 + a * 4.0
      c = b*a+1
  in
    let zz = 88 * f a b (c+e) (7 + 9 * 7)
        ret = case z of
               "a"         -> ((0 :: u32) * 5)
               "something" -> f2 1
               _           -> 2
    in 
      (f2 4 5) * 77
"###;

static TEST_CONTENT_ABSTRACT_PARSER_LETEXPR: &str = r###"
f_let a =
 let x :: u32 -> u32 = \bb -> (1 + bb)
     y :: u32 = 2 + 7
     z :: u32 = 5
     f arg :: T A -> T A = arg * a
 in
   x + y + z + a

f_let a =
 let x :: u32 = 1
     y :: u32 = 2
     z :: u32 = 5
 in
   let b = 10
       c = case b of 
             2 -> 2*x
             _ -> 100
   in x + y + z + a + b * c

f_let2 a =
 let x :: u32 = 1 + a
     y :: u32 = 2
     z :: u32 = 5
 in
   x + y + z
"###;

static TEST_CONTENT_ABSTRACT_PARSER_LETEXPR_SIMPLE: &str = r###"
f_let a =
 let x :: u32 = 1
 in x + a

f_let_2 a =
 let x = 1 :: u32
 in x + a
"###;

static TEST_CONTENT_ABSTRACT_PARSER_LET_RECORD_PARAMS: &str = r###"
let unpack Point { x = xv, y = yv } = xv
in unpack
"###;

static TEST_CONTENT_ABSTRACT_PARSER_LET_PATTERN_PARAMS: &str = r###"
let pair (Some x) _ = x
    res = pair (Some x) 0
in res
"###;

static TEST_CONTENT_ABSTRACT_PARSER_2: &str = r###"
f4 :: T (A -> B) -> T2 -> T3
f3 x =
  let a :: T u32 -> u32 = ff
      b :: f32 = 7
  in
    x + a + b
fg :: T A B C (D AA
             ) -> T2 
                      E
                      F
                      (G H I)
"###;

static TEST_CONTENT_ABSTRACT_PARSER_3: &str = r###"
f1 :: T (A -> B -> C -> D)
f2 :: T ((A -> B) -> C)
f3 :: T (A -> (B -> C))
"###;

static TEST_CONTENT_ABSTRACT_PARSER_TYPE_ANNOTATION: &str = r###"
f a =
 let x :: u32 = 1
     y :: u32 = 2
     z :: u32 = 5
 in
   let b = 10
       c = case b of 
             2 -> (2 :: u32) * x
             _ -> (100 + z) :: u32
   in x + y + z + a + b * c
"###;

static TEST_CONTENT_ABSTRACT_PARSER_LAMBDA: &str = r###"
f :: u32 -> u32 -> u32
f a =
 let ff = \x y -> x * y + a * z
     z = 10
 in
   ff 10

f_test_lambda :: u32 -> u32
f_test_lambda a b =
 (\x y -> x * y) a b

f_test_named_lambda a =
 let ff :: u32 -> u32 -> u32 = \x y -> x * y + a * z
     z = 10
 in
   ff 10

f_test_named_lambda_2 a =
 let ff = \x y -> x * y + a * z
     z = 10
 in
   ff 10
"###;

static TEST_CONTENT_ABSTRACT_PARSER_FUNCTION_SIMPLE: &str = r###"
f :: u32 -> u32 -> u32
f x y = x + y
"###;

static TEST_CONTENT_ABSTRACT_PARSER_CASE_EXPR: &str = r###"
ff x =
 let y = case x of
           "something"  -> 0
           "sdf"        -> 2
           _            -> 7
 in y
"###;

static TEST_CONTENT_ABSTRACT_PARSER_CASE_EXPR_2: &str = r###"
ff x =
 let y = case x of
           "something"  -> let
                             b = 10
                             c = 11
                           in
                             b * c
           "sdf"        -> 2
           _            -> 7
 in y
"###;

static TEST_CONTENT_FUNCTION_PARAM_PATTERN: &str = r###"
ff 0 x = x
ff 1 x = 1
"###;

#[cfg(test)]
mod tests {
    use crate::parse::lex::parse_content_to_concrete_tokens;
    use std::path::Path;

    #[test]
    fn test_parse_function_params_patterns() {
        test_abstract_parser(super::TEST_CONTENT_FUNCTION_PARAM_PATTERN);
    }

    #[test]
    fn test_abstract_parser_letexpr_simple() {
        test_abstract_parser(super::TEST_CONTENT_ABSTRACT_PARSER_LETEXPR_SIMPLE);
    }

    #[test]
    fn test_abstract_parser_letexpr() {
        test_abstract_parser(super::TEST_CONTENT_ABSTRACT_PARSER_LETEXPR);
    }

    #[test]
    fn test_parse_let_record_parameter() {
        let lexed_output = parse_content_to_concrete_tokens(
            Path::new("dummy_path"),
            super::TEST_CONTENT_ABSTRACT_PARSER_LET_RECORD_PARAMS,
        )
        .expect("lexing let record params fixture");

        let mut parser = super::Parser::new(lexed_output);
        let expr = super::parse_expr(&mut parser, 0)
            .expect("parse_expr should succeed")
            .expect("expected let expression");

        match expr.expr {
            super::AExpr::LetExpression(super::LetExpr { defs, .. }) => {
                assert_eq!(defs.len(), 1);
                let (pattern_unpack, rhs_unpack) = &defs[0];
                match pattern_unpack {
                    super::PatternExpr::Variable(var) => match &var.token {
                        super::ConcreteToken::Iden(name) => assert_eq!(name, "unpack"),
                        other => panic!("expected identifier for unpack binding, got {:?}", other),
                    },
                    other => panic!(
                        "expected variable pattern for unpack binding, got {:?}",
                        other
                    ),
                }

                match &rhs_unpack.expr {
                    super::AExpr::AbstractionExpression(abstr) => {
                        assert_eq!(abstr.pattern.len(), 1);
                        let super::ConcreteToken::Iden(name) = &abstr.pattern[0].token else {
                            panic!(
                                "expected generated identifier token, got {:?}",
                                abstr.pattern[0].token
                            );
                        };
                        assert!(
                            name.starts_with("__lambda_param_"),
                            "expected generated parameter name, got {name}"
                        );
                        assert_eq!(abstr.param_patterns.len(), 1);
                        match &abstr.param_patterns[0] {
                            super::PatternExpr::Constructor {
                                constructor,
                                args: super::PatternConstructorArgs::Record { fields, .. },
                                ..
                            } => {
                                match &constructor.token {
                                    super::ConcreteToken::Iden(name) => assert_eq!(name, "Point"),
                                    other => panic!(
                                        "expected constructor identifier for record, got {:?}",
                                        other
                                    ),
                                }
                                assert_eq!(fields.len(), 2);
                                match &fields[0] {
                                    (field_name, super::PatternExpr::Variable(binding)) => {
                                        match (&field_name.token, &binding.token) {
                                            (
                                                super::ConcreteToken::Iden(field),
                                                super::ConcreteToken::Iden(var),
                                            ) => {
                                                assert_eq!(field, "x");
                                                assert_eq!(var, "xv");
                                            }
                                            other => panic!(
                                                "unexpected tokens for record binding: {:?}",
                                                other
                                            ),
                                        }
                                    }
                                    other => panic!(
                                        "expected variable binding for first field, got {:?}",
                                        other
                                    ),
                                }
                                match &fields[1] {
                                    (field_name, super::PatternExpr::Variable(binding)) => {
                                        match (&field_name.token, &binding.token) {
                                            (
                                                super::ConcreteToken::Iden(field),
                                                super::ConcreteToken::Iden(var),
                                            ) => {
                                                assert_eq!(field, "y");
                                                assert_eq!(var, "yv");
                                            }
                                            other => panic!(
                                                "unexpected tokens for second record binding: {:?}",
                                                other
                                            ),
                                        }
                                    }
                                    other => panic!(
                                        "expected variable binding for second field, got {:?}",
                                        other
                                    ),
                                }
                            }
                            other => panic!("expected record pattern parameter, got {:?}", other),
                        }
                        match &abstr.expr.expr {
                            super::AExpr::IdentifierExpression(id_expr) => {
                                match &id_expr.iden.token {
                                    super::ConcreteToken::Iden(name) => assert_eq!(name, "xv"),
                                    other => panic!(
                                        "expected identifier body referencing xv, got {:?}",
                                        other
                                    ),
                                }
                            }
                            other => panic!(
                                "expected identifier body for unpack binding, got {:?}",
                                other
                            ),
                        }
                    }
                    other => panic!("expected abstraction for unpack binding, got {:?}", other),
                }
            }
            other => panic!("expected let expression, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_let_pattern_parameters() {
        let lexed_output = parse_content_to_concrete_tokens(
            Path::new("dummy_path"),
            super::TEST_CONTENT_ABSTRACT_PARSER_LET_PATTERN_PARAMS,
        )
        .expect("lexing let pattern params fixture");

        let mut parser = super::Parser::new(lexed_output);
        let expr = super::parse_expr(&mut parser, 0)
            .expect("parse_expr should succeed")
            .expect("expected let expression");

        match expr.expr {
            super::AExpr::LetExpression(super::LetExpr { defs, .. }) => {
                assert_eq!(defs.len(), 2);
                let (pattern_pair, rhs_pair) = &defs[0];
                match pattern_pair {
                    super::PatternExpr::Variable(var) => match &var.token {
                        super::ConcreteToken::Iden(name) => assert_eq!(name, "pair"),
                        other => panic!("expected identifier for pair binding, got {:?}", other),
                    },
                    other => panic!(
                        "expected variable pattern for pair binding, got {:?}",
                        other
                    ),
                }

                match &rhs_pair.expr {
                    super::AExpr::AbstractionExpression(abstr) => {
                        assert_eq!(abstr.pattern.len(), 2);
                        for binder in &abstr.pattern {
                            if let super::ConcreteToken::Iden(name) = &binder.token {
                                assert!(
                                    name.starts_with("__lambda_param_"),
                                    "expected generated binder name, got {name}"
                                );
                            } else {
                                panic!(
                                    "expected generated identifier token, got {:?}",
                                    binder.token
                                );
                            }
                        }
                        assert_eq!(abstr.param_patterns.len(), 2);
                        match &abstr.param_patterns[0] {
                            super::PatternExpr::Constructor {
                                constructor,
                                args: super::PatternConstructorArgs::Positional(args),
                                ..
                            } => {
                                match &constructor.token {
                                    super::ConcreteToken::Iden(name) => assert_eq!(name, "Some"),
                                    other => panic!(
                                        "expected constructor identifier for Some pattern, got {:?}",
                                        other
                                    ),
                                }
                                assert_eq!(args.len(), 1);
                                match &args[0] {
                                    super::PatternExpr::Variable(var) => match &var.token {
                                        super::ConcreteToken::Iden(name) => assert_eq!(name, "x"),
                                        other => panic!(
                                            "expected variable binding inside Some pattern, got {:?}",
                                            other
                                        ),
                                    },
                                    other => panic!(
                                        "expected variable binding inside Some pattern, got {:?}",
                                        other
                                    ),
                                }
                            }
                            other => panic!(
                                "expected constructor pattern for first parameter, got {:?}",
                                other
                            ),
                        }
                        match &abstr.param_patterns[1] {
                            super::PatternExpr::Wild => {}
                            other => panic!(
                                "expected wildcard pattern for second parameter, got {:?}",
                                other
                            ),
                        }
                        match &abstr.expr.expr {
                            super::AExpr::IdentifierExpression(id_expr) => {
                                match &id_expr.iden.token {
                                    super::ConcreteToken::Iden(name) => assert_eq!(name, "x"),
                                    other => panic!(
                                        "expected identifier body referencing x, got {:?}",
                                        other
                                    ),
                                }
                            }
                            other => panic!(
                                "expected identifier expression in abstraction body, got {:?}",
                                other
                            ),
                        }
                    }
                    other => panic!(
                        "expected abstraction expression for pair binding, got {:?}",
                        other
                    ),
                }

                let (pattern_res, rhs_res) = &defs[1];
                match pattern_res {
                    super::PatternExpr::Variable(var) => match &var.token {
                        super::ConcreteToken::Iden(name) => assert_eq!(name, "res"),
                        other => panic!("expected identifier for res binding, got {:?}", other),
                    },
                    other => panic!("expected variable pattern for res binding, got {:?}", other),
                }

                match &rhs_res.expr {
                    super::AExpr::ApplyExpression(app) => {
                        match &app.fun.expr {
                            super::AExpr::IdentifierExpression(id_expr) => {
                                match &id_expr.iden.token {
                                    super::ConcreteToken::Iden(name) => assert_eq!(name, "pair"),
                                    other => panic!(
                                        "expected identifier expression calling pair, got {:?}",
                                        other
                                    ),
                                }
                            }
                            other => panic!(
                                "expected identifier function in application, got {:?}",
                                other
                            ),
                        }
                        assert_eq!(app.arguments.len(), 2);
                    }
                    other => panic!(
                        "expected application expression for res binding, got {:?}",
                        other
                    ),
                }
            }
            other => panic!("expected let expression, got {:?}", other),
        }
    }

    #[test]
    fn test_abstract_parser_long() {
        test_abstract_parser(super::TEST_CONTENT_ABSTRACT_PARSER);
    }

    #[test]
    fn test_abstract_parser_signatures_0() {
        test_abstract_parser(super::TEST_CONTENT_ABSTRACT_PARSER_2);
    }

    #[test]
    fn test_abstract_parser_signatures_1() {
        test_abstract_parser(super::TEST_CONTENT_ABSTRACT_PARSER_3);
    }

    #[test]
    fn test_abstract_parser_type_annotation() {
        test_abstract_parser(super::TEST_CONTENT_ABSTRACT_PARSER_TYPE_ANNOTATION);
    }

    #[test]
    fn test_abstract_parser_lambda() {
        test_abstract_parser(super::TEST_CONTENT_ABSTRACT_PARSER_LAMBDA);
    }

    #[test]
    fn test_abstract_parser_function_simple() {
        test_abstract_parser(super::TEST_CONTENT_ABSTRACT_PARSER_FUNCTION_SIMPLE);
    }

    #[test]
    fn test_abstract_parser_case_expr() {
        test_abstract_parser(super::TEST_CONTENT_ABSTRACT_PARSER_CASE_EXPR);
    }

    #[test]
    fn test_abstract_parser_case_expr_2() {
        test_abstract_parser(super::TEST_CONTENT_ABSTRACT_PARSER_CASE_EXPR_2);
    }

    #[test]
    fn test_parse_record_constructor_expr() {
        use std::path::Path;

        let input = "Point { x = 1, y = 2 }";
        let lexed = parse_content_to_concrete_tokens(Path::new("/test"), input)
            .expect("lexing should succeed");
        let mut parser = super::Parser::new(lexed);
        let expr = super::parse_expr(&mut parser, 0)
            .expect("parse_expr should succeed")
            .expect("expected an expression");

        match expr.expr {
            super::AExpr::ConstructorExpression(super::ConstructorExpr {
                qualified: None,
                constructor,
                record_fields: Some(fields),
                ..
            }) => {
                match constructor.token {
                    super::ConcreteToken::Iden(name) => assert_eq!(name, "Point"),
                    other => panic!("expected constructor identifier, got {:?}", other),
                }
                assert_eq!(fields.len(), 2);
                match &fields[0].0.token {
                    super::ConcreteToken::Iden(name) => assert_eq!(name, "x"),
                    other => panic!("expected field name token, got {:?}", other),
                }
                match &fields[1].0.token {
                    super::ConcreteToken::Iden(name) => assert_eq!(name, "y"),
                    other => panic!("expected field name token, got {:?}", other),
                }
            }
            other => panic!("expected record constructor expression, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_case_expression_consumes_owned_layout_boundaries() {
        let input = "case x of\n  A -> 1\n  B -> 2\n";
        let lexed = parse_content_to_concrete_tokens(Path::new("/test"), input)
            .expect("lexing case expression should succeed");
        let mut parser = super::Parser::new(lexed);

        let expression = super::parse_expr(&mut parser, 0)
            .expect("case expression should parse")
            .expect("case expression should produce an AST");

        let super::AExpr::CaseExpression(case_expr) = expression.expr else {
            panic!("expected case expression")
        };
        assert_eq!(case_expr.clauses.len(), 2);
        assert!(matches!(
            parser.peek().expect("peek after case"),
            Some(super::ParserToken {
                ty: super::ParserTokenType::Concrete(super::ConcreteToken::EndOfFile),
                ..
            })
        ));
    }

    #[test]
    fn test_case_expression_inside_explicit_braces_closes_before_brace() {
        let input = "{ case x of\n  A -> 1\n  B -> 2\n}";
        let lexed = parse_content_to_concrete_tokens(Path::new("/test"), input)
            .expect("lexing braced case expression should succeed");
        let mut parser = super::Parser::new(lexed);
        parser
            .expect_concrete(&super::ConcreteToken::BraceL)
            .expect("expected opening brace");

        let expression = super::parse_expr(&mut parser, 0)
            .expect("braced case expression should parse")
            .expect("braced case expression should produce an AST");
        assert!(matches!(expression.expr, super::AExpr::CaseExpression(_)));
        parser
            .expect_concrete(&super::ConcreteToken::BraceR)
            .expect("case layout should close before physical brace");
    }

    #[test]
    fn test_nested_case_expression_returns_to_outer_clause_boundary() {
        let input = "case x of\n  A -> case y of\n    B -> 1\n  C -> 2\n";
        let lexed = parse_content_to_concrete_tokens(Path::new("/test"), input)
            .expect("lexing nested case expression should succeed");
        let mut parser = super::Parser::new(lexed);

        let expression = super::parse_expr(&mut parser, 0)
            .expect("nested case expression should parse")
            .expect("nested case expression should produce an AST");
        let super::AExpr::CaseExpression(outer) = expression.expr else {
            panic!("expected outer case expression")
        };
        assert_eq!(outer.clauses.len(), 2);
        let super::AExpr::CaseExpression(inner) = &outer.clauses[0].body.expr else {
            panic!("expected nested case expression in first clause")
        };
        assert_eq!(inner.clauses.len(), 1);
    }

    #[test]
    fn test_application_left_associative_simple() {
        use super::ConcreteToken;
        use std::path::Path;

        let input = "f g x";
        let lexed = parse_content_to_concrete_tokens(Path::new("/test"), input)
            .expect("lexing should succeed");
        let mut parser = super::Parser::new(lexed);
        let expr = super::parse_expr(&mut parser, 0)
            .expect("parse_expr should succeed")
            .expect("expected an expression");

        match expr.expr {
            super::AExpr::ApplyExpression(app) => {
                // fun must be the identifier f
                match &app.fun.expr {
                    super::AExpr::IdentifierExpression(id) => match &id.iden.token {
                        ConcreteToken::Iden(name) => assert_eq!(name, "f"),
                        other => panic!("expected identifier token 'f', got {:?}", other),
                    },
                    other => panic!("expected identifier fun, got {:?}", other),
                }

                assert_eq!(app.arguments.len(), 2, "expected 2 arguments for 'f g x'");

                // first arg must be identifier 'g' (not an application)
                match &app.arguments[0].expr {
                    super::AExpr::IdentifierExpression(id) => match &id.iden.token {
                        ConcreteToken::Iden(name) => assert_eq!(name, "g"),
                        other => panic!("expected identifier token 'g', got {:?}", other),
                    },
                    other => panic!("expected first arg to be identifier 'g', got {:?}", other),
                }

                // second arg must be identifier 'x'
                match &app.arguments[1].expr {
                    super::AExpr::IdentifierExpression(id) => match &id.iden.token {
                        ConcreteToken::Iden(name) => assert_eq!(name, "x"),
                        other => panic!("expected identifier token 'x', got {:?}", other),
                    },
                    other => panic!("expected second arg to be identifier 'x', got {:?}", other),
                }
            }
            other => panic!("expected application expression, got {:?}", other),
        }
    }

    #[test]
    fn test_application_paren_argument_distinct() {
        use super::ConcreteToken;
        use std::path::Path;

        let input = "f (g x)";
        let lexed = parse_content_to_concrete_tokens(Path::new("/test"), input)
            .expect("lexing should succeed");
        let mut parser = super::Parser::new(lexed);
        let expr = super::parse_expr(&mut parser, 0)
            .expect("parse_expr should succeed")
            .expect("expected an expression");

        match expr.expr {
            super::AExpr::ApplyExpression(app) => {
                // fun must be the identifier f
                match &app.fun.expr {
                    super::AExpr::IdentifierExpression(id) => match &id.iden.token {
                        ConcreteToken::Iden(name) => assert_eq!(name, "f"),
                        other => panic!("expected identifier token 'f', got {:?}", other),
                    },
                    other => panic!("expected identifier fun, got {:?}", other),
                }

                // there should be exactly one argument: (g x)
                assert_eq!(app.arguments.len(), 1, "expected 1 argument for 'f (g x)'");

                match &app.arguments[0].expr {
                    super::AExpr::ApplyExpression(inner) => {
                        // inner fun must be identifier 'g'
                        match &inner.fun.expr {
                            super::AExpr::IdentifierExpression(id) => match &id.iden.token {
                                ConcreteToken::Iden(name) => assert_eq!(name, "g"),
                                other => panic!(
                                    "expected identifier token 'g' in inner app, got {:?}",
                                    other
                                ),
                            },
                            other => panic!(
                                "expected inner application fun to be identifier 'g', got {:?}",
                                other
                            ),
                        }

                        assert_eq!(inner.arguments.len(), 1);
                        match &inner.arguments[0].expr {
                            super::AExpr::IdentifierExpression(id) => match &id.iden.token {
                                ConcreteToken::Iden(name) => assert_eq!(name, "x"),
                                other => panic!(
                                    "expected identifier token 'x' in inner arg, got {:?}",
                                    other
                                ),
                            },
                            other => panic!(
                                "expected inner application arg to be identifier 'x', got {:?}",
                                other
                            ),
                        }
                    }
                    other => panic!(
                        "expected first argument to be an application '(g x)', got {:?}",
                        other
                    ),
                }
            }
            other => panic!("expected application expression, got {:?}", other),
        }
    }

    fn test_abstract_parser(input: &str) {
        let lexed_output = parse_content_to_concrete_tokens(Path::new("dummy_path"), input)
            .expect("lexing abstract parser fixture");

        let top_level_items = super::parse_concrete_top_level(lexed_output)
            .expect("abstract parser should succeed for test fixture");

        assert!(
            top_level_items.0.len() > 0,
            "expected at least one top-level item"
        );

        // use crate::parse::printer::DocPrinter;
        // println!("{}", top_level_items.to_doc());
    }
}
