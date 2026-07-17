// -----------------------------------------------------------------------------
//
// layout
//   - indentation-sensitive; top-level items, case branches, data clauses,
//     and let bindings must align on their starting column
//   - unexpected dedents or tokens at the wrong column yield immediate errors
//
// expressions
//   expr            := prefix postfix*
//   prefix          := literal | identifier | constructor | lambda | let | case | '(' expr ')'
//   postfix         := application_argument | infix_op expr | type_annotation
//
// application_argument := identifier
//                          | constructor
//                          | literal
//                          | '(' expr ')'
//
// precedence tracked via operator-info binding powers (Pratt parser)
//
// literals & identifiers
//   numeric literal := digits ('.' digits)?
//   string literal  := double-quoted sequence
//   identifier      := lower-case start ⇒ variable; upper-case start ⇒ constructor (unless qualified lookup fails)
//
// pattern matching
//   pattern := wildcard
//               | variable
//               | literal
//               | constructor pattern*
//               | record { field = pattern, ... }
//               | record { field, ..., .. }
//
//   - see `abstr_pattern.rs` for exact validation rules
//
// case expressions
//   ```
//   case expr of
//     pattern -> expr
//     pattern -> expr
//     ...
//   ```
//
//   - clauses are indentation-aligned; bodies may be multi-line blocks
//
// let expressions
//   ```
//   let pattern (:: type_expr)? = expr
//       pattern (:: type_expr)? = expr
//       ...
//   in expr
//   ```
//
//   - function-style lhs (`f x y = ...`) desugars to nested lambdas plus a generated case expression so pattern matching occurs during type checking
//
// lambdas & function definitions
//   lambda := \ pattern … pattern -> expr
//
//   - function definitions are stored as `AbstractionExpr` (name + parameter list)
//
// type expressions
//   type_expr      := type_atom ('->' type_expr)*
//   type_atom      := identifier type_arguments? | '(' type_expr ')'
//   type_arguments := type_atom*   // space-delimited application; parentheses group nested applications
//
// data declarations (ADTs)
//   - record form:  `data Name params { field :: Type, ... }`
//   - sum form:     `data Name params = Constructor Type* | Constructor Type* | ...`
//   - params are space-delimited identifiers (e.g., `data Pair A B = ...`)
//   - product type is treated as single-constructor sum type
//
// operator precedence (tightest → loosest)
//   - application
//   - unary + - !
//   - * /
//   - + -
//   - &&
//   - ||
// -----------------------------------------------------------------------------

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::vec::Vec;

use super::abstr_structures::*;
use super::concrete_token::*;
use super::loc::*;
use super::parser::{
    ForkableTokenStream, ParseError, ParseResult, Parser, TokenStreamExt,
    collect_layout_items_result, is_keyword_in, is_layout_block_terminator,
};

fn mk_identifier_expr(token: ConcreteTokenAndLoc) -> AExprAnnot {
    AExprAnnot {
        expr: AExpr::IdentifierExpression(IdenExpr {
            iden: token,
            builtin: None,
        }),
        type_expr: None,
    }
}

fn mk_numeric_expr(token: ConcreteTokenAndLoc) -> AExprAnnot {
    AExprAnnot {
        expr: AExpr::NumericExpr(LiteralNumericExpr { literal: token }),
        type_expr: None,
    }
}

fn mk_string_expr(token: ConcreteTokenAndLoc) -> AExprAnnot {
    AExprAnnot {
        expr: AExpr::StringExpr(LiteralStringExpr { literal: token }),
        type_expr: None,
    }
}

fn token_indent(token: &ConcreteTokenAndLoc) -> usize {
    token.loc.span_start.col
}

fn next_indent(indent_current: Indent) -> Indent {
    match indent_current {
        Indent::CurLvl(lvl) => Indent::PrevLvl(lvl),
        other => other,
    }
}

fn parse_braced_list<'a, S, F, T>(input: &mut S, mut parse_item: F) -> ParseResult<Vec<T>>
where
    S: TokenStreamExt<'a>,
    F: FnMut(&mut S) -> ParseResult<Option<T>>,
{
    let mut items = Vec::new();

    loop {
        input.consume_trivial();
        let Some(peeked) = input.peek_token().cloned() else {
            return Err(ParseError::unexpected_eof("braced list"));
        };

        match peeked.token {
            ConcreteToken::BraceR => {
                input.next_token();
                break;
            }
            ConcreteToken::Comma => {
                // skip separators between items
                input.next_token();
                continue;
            }
            ConcreteToken::EndOfFile => {
                return Err(ParseError::unexpected_eof("braced list"));
            }
            _ => {}
        }

        match parse_item(input)? {
            Some(item) => items.push(item),
            None => {
                return Err(ParseError::message(
                    "failed to parse item inside braced list",
                    Some(peeked),
                ));
            }
        }
    }

    Ok(items)
}

fn parse_record_constructor_fields<'a, S>(
    input: &mut S,
) -> ParseResult<Vec<(ConcreteTokenAndLoc, AExprAnnot)>>
where
    S: ForkableTokenStream<'a>,
{
    parse_braced_list(input, |stream| {
        stream.consume_trivial();
        let Some(name_tok) = stream.next_token() else {
            return Ok(None);
        };
        if matches!(name_tok.token, ConcreteToken::BraceR) {
            return Ok(None);
        }
        if !matches!(name_tok.token, ConcreteToken::Iden(_)) {
            return Err(ParseError::unexpected_token(
                "record field name",
                Some(name_tok.clone()),
            ));
        }
        stream.consume_trivial();
        match stream.next_token() {
            Some(eq) if matches!(eq.token, ConcreteToken::Equal) => {}
            Some(other) => return Err(ParseError::unexpected_token("'='", Some(other))),
            None => {
                return Err(ParseError::unexpected_eof(
                    "'=' in record constructor field",
                ));
            }
        }
        stream.consume_trivial();
        let Some(start) = stream.peek_token().cloned() else {
            return Err(ParseError::unexpected_eof(
                "expression in record constructor field",
            ));
        };
        let indent_expr = Indent::CurLvl(start.loc.span_start.col);
        let expr = parse_expr(stream, indent_expr, 0, false)?.ok_or_else(|| {
            ParseError::message(
                "expected expression in record constructor field",
                Some(start.clone()),
            )
        })?;
        Ok(Some((name_tok.clone(), expr)))
    })
}

static LAMBDA_PARAM_COUNTER: AtomicUsize = AtomicUsize::new(0);

fn fresh_param_token(source: &ConcreteTokenAndLoc) -> ConcreteTokenAndLoc {
    let idx = LAMBDA_PARAM_COUNTER.fetch_add(1, Ordering::Relaxed);
    ConcreteTokenAndLoc {
        token: ConcreteToken::Iden(format!("__lambda_param_{idx}")),
        loc: source.loc.clone(),
    }
}

fn collect_function_param_binders(
    params: &[PatternExpr],
    source: &ConcreteTokenAndLoc,
) -> Vec<ConcreteTokenAndLoc> {
    params
        .iter()
        .map(|pattern| match pattern {
            PatternExpr::Variable(var) => var.clone(),
            PatternExpr::Wild => fresh_param_token(source),
            _ => fresh_param_token(source),
        })
        .collect()
}

pub(crate) fn space_and_delimiter_and_comment_filter<'a>(
    input: impl Iterator<Item = &'a ConcreteTokenAndLoc>,
) -> impl Iterator<Item = &'a ConcreteTokenAndLoc> {
    input.filter(|x| {
        !matches!(x.token, ConcreteToken::Space(_))
            && !matches!(x.token, ConcreteToken::LineDelimiter)
            && !matches!(x.token, ConcreteToken::CommentSlashes)
            && !matches!(x.token, ConcreteToken::Comment(_))
    })
}

// ===================== ADT Parsing Functions =====================

use super::abstr_pattern::{
    is_constructor_name, is_pattern_start_token, parse_pattern, parse_pattern_stream,
};

fn is_constructor_arg_terminator(token: &ConcreteToken) -> bool {
    matches!(
        token,
        ConcreteToken::ArrowRight
            | ConcreteToken::Equal
            | ConcreteToken::Case
            | ConcreteToken::Of
            | ConcreteToken::Let
            | ConcreteToken::In
            | ConcreteToken::BraceR
            | ConcreteToken::ParenR
            | ConcreteToken::Comma
            | ConcreteToken::VertBar
            | ConcreteToken::Plus
            | ConcreteToken::Minus
            | ConcreteToken::Star
            | ConcreteToken::FwdSlash
            | ConcreteToken::AngleL
            | ConcreteToken::AngleR
            | ConcreteToken::LessEqual
            | ConcreteToken::GreaterEqual
            | ConcreteToken::EqualEqual
            | ConcreteToken::And
            | ConcreteToken::Or
    )
}

fn gather_exprs_until<'a, S, F>(
    input: &mut S,
    indent: Indent,
    mut should_stop: F,
) -> ParseResult<Vec<AExprAnnot>>
where
    S: ForkableTokenStream<'a>,
    F: FnMut(&ConcreteTokenAndLoc) -> bool,
{
    let mut exprs = Vec::new();

    loop {
        input.consume_trivial();
        let Some(next_token) = input.peek_token().cloned() else {
            break;
        };

        if should_stop(&next_token) {
            break;
        }

        if update_indent_enclosing_delimiter(indent, Indent::CurLvl(next_token.loc.span_start.col))
            .is_err()
        {
            break;
        }

        let expr_indent = Indent::CurLvl(next_token.loc.span_start.col);
        match parse_expr(input, expr_indent, 0, false)? {
            Some(expr) => exprs.push(expr),
            None => break,
        }
    }

    Ok(exprs)
}

// pattern parsing has been moved to abstr_pattern.rs for better organization

// ===================== End of ADT Parsing Functions =====================

pub(crate) fn parse_concrete_top_level(
    input: LexedTokensAndLocs,
) -> ParseResult<Vec<TopLevelItem>> {
    let mut parser = Parser::new(input.0.iter());
    // layout-driven walk at indent 0; cursor stops when token dedents or we hit EOF
    collect_layout_items_result(&mut parser, 0, false, |stream| {
        loop {
            stream.consume_trivial();
            let Some(next) = stream.peek_token().cloned() else {
                return Ok(None);
            };

            // any dedent ends the top-level block
            if token_indent(&next) != 0 {
                return Ok(None);
            }

            match &next.token {
                ConcreteToken::Iden(_) => {
                    let ident = stream
                        .next_token()
                        .ok_or_else(|| ParseError::unexpected_eof("top-level identifier"))?;
                    let item = parse_top_level_identifier(stream, ident)?;
                    return Ok(Some(item));
                }
                ConcreteToken::Data => {
                    let data_token = stream
                        .next_token()
                        .ok_or_else(|| ParseError::unexpected_eof("data keyword"))?
                        .clone();
                    let item = parse_data(stream, Indent::PrevLvl(data_token.loc.span_start.col))?;
                    return Ok(Some(item));
                }
                ConcreteToken::EndOfFile => return Ok(None),
                _ => {
                    // unsupported or trivia: consume and keep scanning
                    stream.next_token();
                }
            }
        }
    })
}

///process data record or sum type
fn parse_data<'a, S>(input: &mut S, indent: Indent) -> ParseResult<TopLevelItem>
where
    S: TokenStreamExt<'a>,
{
    input.consume_trivial();
    let data_identifier_info = input
        .next_token()
        .ok_or_else(|| ParseError::unexpected_eof("data declaration"))?;

    match &data_identifier_info.token {
        ConcreteToken::Iden(_) => {}
        _ => {
            return Err(ParseError::unexpected_token(
                "data identifier",
                Some(data_identifier_info),
            ));
        }
    }
    let data_identifier_info = data_identifier_info;

    update_indent(
        indent,
        Indent::CurLvl(data_identifier_info.loc.span_start.col),
    )
    .map_err(|e| ParseError::indentation(e, Some(data_identifier_info.clone())))?;

    let concrete_type_exprs = parse_data_type_params(input)?;

    input.consume_trivial();
    let next_token = input
        .next_token()
        .ok_or_else(|| ParseError::unexpected_eof("data body after identifier"))?;
    match &next_token.token {
        ConcreteToken::BraceL => {
            //let it take care of closing }
            let components = parse_data_record(input, indent)?;
            Ok(TopLevelItem::DataRecord(DataRecord {
                identifier: data_identifier_info,
                params: concrete_type_exprs,
                components,
            }))
        }
        ConcreteToken::Equal => {
            let variants = parse_data_sum(input, indent)?;
            Ok(TopLevelItem::DataSum(DataSum {
                identifier: data_identifier_info,
                params: concrete_type_exprs,
                variants,
            }))
        }
        _ => Err(ParseError::unexpected_token(
            "data record `{` or constructors `=`",
            Some(next_token.clone()),
        )),
    }
}

fn parse_data_type_params<'a, S>(input: &mut S) -> ParseResult<Vec<ATypeExprComplex>>
where
    S: TokenStreamExt<'a>,
{
    // Space-delimited parameter names (identifiers) after the data name
    let mut params = Vec::new();
    loop {
        input.consume_trivial();
        let Some(next) = input.peek_token().cloned() else {
            break;
        };

        // Stop before the body: record '{' or '=' starts immediately after params
        if matches!(next.token, ConcreteToken::BraceL | ConcreteToken::Equal) {
            break;
        }

        match next.token {
            ConcreteToken::Iden(_) => {
                // consume and store as a simple identifier type parameter
                let id = input.next_token().unwrap();
                params.push(ATypeExprComplex::Iden(ATypeExprIden {
                    identifier: id,
                    type_parameters: Vec::new(),
                }));
            }
            _ => break,
        }
    }
    Ok(params)
}

//data type with record
fn parse_data_record<'a, S>(
    input: &mut S,
    _indent: Indent,
) -> ParseResult<Vec<(ConcreteTokenAndLoc, ATypeExprComplex)>>
where
    S: TokenStreamExt<'a>,
{
    parse_braced_list(input, |stream| {
        let field = match stream.next_token() {
            Some(tok) if matches!(tok.token, ConcreteToken::Iden(_)) => tok.clone(),
            Some(other) => {
                return Err(ParseError::unexpected_token(
                    "record field identifier",
                    Some(other),
                ));
            }
            None => return Err(ParseError::unexpected_eof("data record field")),
        };

        stream.consume_trivial();
        let type_expr = parse_required_type_annotation(stream, "after record field in data")?;
        Ok(Some((field, type_expr)))
    })
}

//data type with sum constructor
fn parse_data_sum<'a, S>(
    input: &mut S,
    indent: Indent,
) -> ParseResult<Vec<(ConcreteTokenAndLoc, Vec<ATypeExprComplex>)>>
where
    S: TokenStreamExt<'a>,
{
    let base_indent = match indent {
        Indent::PrevLvl(col) => col,
        _ => 0,
    };

    collect_layout_items_result(input, base_indent, true, |stream| {
        stream.consume_trivial();
        stream.consume_while(|token| {
            matches!(
                token,
                ConcreteToken::VertBar | ConcreteToken::LineDelimiter | ConcreteToken::Space(_)
            )
        });

        let Some(peeked) = stream.peek_token().cloned() else {
            return Ok(None);
        };

        if !matches!(peeked.token, ConcreteToken::Iden(_)) {
            return Err(ParseError::unexpected_token(
                "constructor identifier",
                Some(peeked),
            ));
        }

        let ctor_token = stream
            .next_token()
            .ok_or_else(|| ParseError::unexpected_eof("constructor identifier"))?
            .clone();

        let mut constructor_types = Vec::new();
        loop {
            stream.consume_trivial();
            stream.consume_while(|tok| matches!(tok, ConcreteToken::Space(_)));

            let Some(next) = stream.peek_token().cloned() else {
                break;
            };

            if matches!(
                next.token,
                ConcreteToken::VertBar | ConcreteToken::EndOfFile
            ) {
                break;
            }

            if is_constructor_arg_terminator(&next.token)
                || (token_indent(&next) <= base_indent && !constructor_types.is_empty())
            {
                break;
            }

            let ty_indent = Indent::CurLvl(token_indent(&next));
            // parse a single constructor field type; do not consume
            // adjacent identifiers as type application here because
            // space separates distinct constructor fields
            match parse_single_field_type_expr(stream, ty_indent)? {
                Some(arg) => constructor_types.push(arg),
                None => break,
            }
        }

        Ok(Some((ctor_token, constructor_types)))
    })
}

fn tokens_to_patterns(tokens: &[ConcreteTokenAndLoc]) -> ParseResult<Vec<PatternExpr>> {
    let mut parser = Parser::new(tokens.iter());
    let mut patterns = Vec::new();

    while let Some(next) = TokenStreamExt::peek_non_trivial(&mut parser) {
        if !is_pattern_start_token(&next.token) {
            return Err(ParseError::message(
                format!("unsupported pattern token: {:?}", next.token),
                Some(next.clone()),
            ));
        }

        match parse_pattern_stream(&mut parser) {
            Ok(pattern) => patterns.push(pattern),
            Err(err) => {
                return Err(ParseError::message(
                    format!("pattern parsing error: {:?}", err),
                    Some(next.clone()),
                ));
            }
        }
    }

    Ok(patterns)
}

fn parse_top_level_identifier<'a, S>(
    input: &mut S,
    t: ConcreteTokenAndLoc,
) -> ParseResult<TopLevelItem>
where
    S: ForkableTokenStream<'a>,
{
    // skip any spaces after the identifier
    input.skip_spaces();

    // expect start of function signature or function definition for now
    match input.peek_token() {
        Some(x) => match &x.token {
            ConcreteToken::IsType => {
                let input_indent = x.loc.span_start.col;
                parse_function_signature(input, t, Indent::PrevLvl(input_indent))
            }
            ConcreteToken::Iden(_) | ConcreteToken::ParenL | ConcreteToken::Equal => {
                //a function definition with at least 1 argument or 0 argument
                parse_function_definition(input, t)
            }
            _ => Err(ParseError::unexpected_token(
                "function signature '::' or definition '='",
                Some(x.clone()),
            )),
        },
        None => Err(ParseError::unexpected_eof("function definition")),
    }
}

fn parse_function_signature<'a, S>(
    input: &mut S,
    function_identifier: ConcreteTokenAndLoc,
    indent: Indent,
) -> ParseResult<TopLevelItem>
where
    S: ForkableTokenStream<'a>,
{
    if !matches!(indent, Indent::PrevLvl(_)) {
        return Err(ParseError::message(
            "unexpected indentation for function signature",
            Some(function_identifier.clone()),
        ));
    }

    //consume ::
    input.expect_token_result(&ConcreteToken::IsType)?;

    let ty = parse_type_expr(input, indent)?.ok_or_else(|| {
        ParseError::message(
            "expected type expression in function signature",
            Some(function_identifier.clone()),
        )
    })?;

    Ok(TopLevelItem::FunctionSignature(FnSig {
        identifier: function_identifier,
        ty,
    }))
}

// parses a type expression with optional control over space-delimited application
fn parse_type_expr_internal<'a, S>(
    input: &mut S,
    indent: Indent,
    allow_space_application: bool,
) -> ParseResult<Option<ATypeExprComplex>>
where
    S: TokenStreamExt<'a>,
{
    let Some(next) = input.peek_token().cloned() else {
        return Ok(None);
    };

    match &next.token {
        ConcreteToken::ParenR => {
            update_indent_enclosing_delimiter(indent, Indent::CurLvl(next.loc.span_start.col))
                .map_err(|e| ParseError::indentation(e, Some(next.clone())))?;
            return Ok(None);
        }
        ConcreteToken::Iden(_) | ConcreteToken::ParenL => {
            update_indent(indent, Indent::CurLvl(next.loc.span_start.col))
                .map_err(|e| ParseError::indentation(e, Some(next.clone())))?;
        }
        _ => {
            return Err(ParseError::unexpected_token(
                "type expression",
                Some(next.clone()),
            ));
        }
    }

    let mut head = parse_type_atom(input, indent)?;

    if allow_space_application {
        // space-delimited type application: T A B
        loop {
            input.consume_trivial();
            let Some(peeked) = input.peek_token().cloned() else {
                break;
            };

            if update_indent(indent, Indent::CurLvl(peeked.loc.span_start.col)).is_err() {
                break;
            }
            let is_terminator = matches!(
                peeked.token,
                ConcreteToken::ArrowRight
                    | ConcreteToken::ParenR
                    | ConcreteToken::Comma
                    | ConcreteToken::BraceR
                    | ConcreteToken::BracketR
                    | ConcreteToken::VertBar
                    | ConcreteToken::Of
                    | ConcreteToken::Where
                    | ConcreteToken::Let
                    | ConcreteToken::In
                    | ConcreteToken::Equal
                    | ConcreteToken::EndOfFile
            );
            if is_terminator {
                break;
            }
            if !matches!(peeked.token, ConcreteToken::Iden(_) | ConcreteToken::ParenL) {
                break;
            }

            let param = parse_type_atom(input, indent)?;
            if let ATypeExprComplex::Iden(ref mut iden) = head {
                iden.type_parameters.push(param);
            } else {
                break;
            }
        }
    }

    let combined = parse_type_arrows(input, indent, head)?;
    Ok(Some(combined))
}

// public wrapper: allow space-delimited application by default
// parses:
//   Identifier Type* | Identifier
fn parse_type_expr<'a, S>(input: &mut S, indent: Indent) -> ParseResult<Option<ATypeExprComplex>>
where
    S: TokenStreamExt<'a>,
{
    parse_type_expr_internal(input, indent, true)
}

// parses a single constructor field type; disables space-delimited application
// so adjacent identifiers are treated as separate fields
fn parse_single_field_type_expr<'a, S>(
    input: &mut S,
    indent: Indent,
) -> ParseResult<Option<ATypeExprComplex>>
where
    S: TokenStreamExt<'a>,
{
    parse_type_expr_internal(input, indent, false)
}

fn parse_type_atom<'a, S>(input: &mut S, indent: Indent) -> ParseResult<ATypeExprComplex>
where
    S: TokenStreamExt<'a>,
{
    let token = input
        .next_token()
        .ok_or_else(|| ParseError::unexpected_eof("type atom"))?;
    let result = match &token.token {
        ConcreteToken::ParenL => {
            // unit type: ()
            input.consume_trivial();
            if let Some(peek) = input.peek_token().cloned() {
                if matches!(peek.token, ConcreteToken::ParenR) {
                    let _ = input.next_token(); // consume ')'
                    // synthesize an identifier token for "()" to flow through lowerers
                    let unit_tok = ConcreteTokenAndLoc {
                        token: ConcreteToken::Iden("()".to_string()),
                        loc: token.loc.clone(),
                    };
                    return Ok(ATypeExprComplex::Iden(ATypeExprIden {
                        identifier: unit_tok,
                        type_parameters: vec![],
                    }));
                }
            }
            let inner = parse_type_expr(input, Indent::PrevLvl(token.loc.span_start.col))?
                .ok_or_else(|| {
                    ParseError::message(
                        "expected type expression inside parentheses",
                        Some(token.clone()),
                    )
                })?;
            let closing = input
                .next_token()
                .ok_or_else(|| ParseError::unexpected_eof("')' to close type atom"))?;
            if !matches!(closing.token, ConcreteToken::ParenR) {
                return Err(ParseError::unexpected_token(")", Some(closing)));
            }
            inner
        }
        ConcreteToken::Iden(_) => parse_type_identifier(input, indent, token.clone())?,
        _ => {
            return Err(ParseError::unexpected_token(
                "type identifier or '('",
                Some(token.clone()),
            ));
        }
    };
    Ok(result)
}

fn parse_type_identifier<'a, S>(
    input: &mut S,
    indent: Indent,
    identifier: ConcreteTokenAndLoc,
) -> ParseResult<ATypeExprComplex>
where
    S: TokenStreamExt<'a>,
{
    let type_parameters = Vec::new();

    if let Some(next) = input.peek_token() {
        if update_indent(indent, Indent::CurLvl(next.loc.span_start.col)).is_err() {
            return Ok(ATypeExprComplex::Iden(ATypeExprIden {
                identifier,
                type_parameters,
            }));
        }
    }

    Ok(ATypeExprComplex::Iden(ATypeExprIden {
        identifier,
        type_parameters,
    }))
}

fn parse_type_arrows<'a, S>(
    input: &mut S,
    indent: Indent,
    head: ATypeExprComplex,
) -> ParseResult<ATypeExprComplex>
where
    S: TokenStreamExt<'a>,
{
    match input.peek_token().cloned() {
        Some(next)
            if update_indent(indent, Indent::CurLvl(next.loc.span_start.col)).is_ok()
                && matches!(next.token, ConcreteToken::ArrowRight) =>
        {
            let arrow = input
                .next_token()
                .ok_or_else(|| ParseError::unexpected_eof("arrow in type expression"))?;
            let tail_head = parse_type_expr(input, Indent::PrevLvl(arrow.loc.span_start.col))?
                .ok_or_else(|| {
                    ParseError::message(
                        "error parsing type on right side of ->",
                        Some(arrow.clone()),
                    )
                })?;
            let tail = parse_type_arrows(input, indent, tail_head)?;
            Ok(ATypeExprComplex::Fun(ATypeExprFun {
                head: Arc::new(Mutex::new(head)),
                tail: Some(Arc::new(Mutex::new(tail))),
            }))
        }
        _ => Ok(head),
    }
}

fn parse_function_definition<'a>(
    input: &mut impl ForkableTokenStream<'a>,
    function_identifier: ConcreteTokenAndLoc,
) -> ParseResult<TopLevelItem> {
    //parse input parameters
    let mut params = vec![];
    loop {
        input.consume_trivial();
        let Some(peeked) = input.peek_token().cloned() else {
            return Err(ParseError::unexpected_eof("function parameters before '='"));
        };
        match &peeked.token {
            ConcreteToken::Iden(_) => {
                let param = input.next_token().ok_or_else(|| {
                    ParseError::unexpected_eof("function parameter after identifier")
                })?;
                params.push(param);
            }
            ConcreteToken::ParenL => {
                // minimally support unit pattern parameter: foo () = ...
                let open = input.next_token().ok_or_else(|| {
                    ParseError::unexpected_eof("'(' starting function parameter pattern")
                })?;
                input.consume_trivial();
                let close = input.next_token().ok_or_else(|| {
                    ParseError::unexpected_eof("')' closing function parameter pattern")
                })?;
                if !matches!(close.token, ConcreteToken::ParenR) {
                    return Err(ParseError::unexpected_token(
                        "')' closing function parameter pattern",
                        Some(close.clone()),
                    ));
                }
                params.push(open.clone());
                params.push(close.clone());
            }
            ConcreteToken::Equal => {
                let _ = input.next_token();
                break;
            }
            _ => {
                return Err(ParseError::unexpected_token(
                    "function parameter or '='",
                    Some(peeked),
                ));
            }
        }
    }

    let body_expr = parse_function_body(
        input,
        Indent::PrevLvl(function_identifier.loc.span_start.col),
    )?;

    let param_patterns = tokens_to_patterns(&params)?;

    Ok(TopLevelItem::FunctionDefinition(AbstractionExpr {
        name: Some(function_identifier),
        pattern: params,
        param_patterns,
        type_expr: None,
        expr: Box::new(AExprAnnot {
            expr: AExpr::BlockExpression(body_expr),
            type_expr: None,
        }),
    }))
}

fn parse_function_body<'a>(
    input: &mut impl ForkableTokenStream<'a>,
    indent: Indent,
) -> ParseResult<BlockExpr> {
    // skip any spaces before body
    input.consume_trivial();

    let mut ret = Vec::new();
    let precedence = 0;
    let align_to_indent = false;

    while let Some(expr) = parse_expr(input, indent, precedence, align_to_indent)? {
        ret.push(expr);
        input.consume_trivial();
        match input.peek_token() {
            Some(tok) if tok.loc.span_start.col == 0 => break,
            Some(_) => continue,
            None => break,
        }
    }

    Ok(BlockExpr(ret))
}

fn apply_argument(base: AExprAnnot, arg: AExprAnnot) -> AExprAnnot {
    match base.expr {
        AExpr::ApplyExpression(mut app) => {
            app.arguments.push(arg);
            AExprAnnot {
                expr: AExpr::ApplyExpression(app),
                type_expr: base.type_expr,
            }
        }
        other => AExprAnnot {
            expr: AExpr::ApplyExpression(AppExpr {
                fun: Box::new(AExprAnnot {
                    expr: other,
                    type_expr: base.type_expr.clone(),
                }),
                arguments: vec![arg],
            }),
            type_expr: None,
        },
    }
}

#[derive(Clone, Copy)]
enum PrefixLiteralKind {
    Numeric,
    String,
}

impl PrefixLiteralKind {
    fn to_expr(self, token: ConcreteTokenAndLoc) -> AExprAnnot {
        match self {
            PrefixLiteralKind::Numeric => mk_numeric_expr(token),
            PrefixLiteralKind::String => mk_string_expr(token),
        }
    }
}

#[derive(Clone)]
enum PrefixRule {
    Literal(PrefixLiteralKind),
    Identifier,
    Lambda,
    Let,
    Case,
    Paren,
    PrefixOperator(PrefixOpInfo),
}

fn classify_prefix_rule(token: &ConcreteToken) -> Option<PrefixRule> {
    match token {
        ConcreteToken::LiteralNumeric(_) => Some(PrefixRule::Literal(PrefixLiteralKind::Numeric)),
        ConcreteToken::LiteralString(_) => Some(PrefixRule::Literal(PrefixLiteralKind::String)),
        ConcreteToken::Iden(_) => Some(PrefixRule::Identifier),
        ConcreteToken::BackSlash => Some(PrefixRule::Lambda),
        ConcreteToken::Let => Some(PrefixRule::Let),
        ConcreteToken::Case => Some(PrefixRule::Case),
        ConcreteToken::ParenL => Some(PrefixRule::Paren),
        ConcreteToken::Exclamation | ConcreteToken::Minus | ConcreteToken::Plus => {
            prefix_op_info(token).map(PrefixRule::PrefixOperator)
        }
        _ => None,
    }
}

impl PrefixRule {
    fn parse<'a, S>(
        self,
        input: &mut S,
        token: ConcreteTokenAndLoc,
        indent_current: Indent,
    ) -> ParseResult<PrefixOutcome>
    where
        S: ForkableTokenStream<'a>,
    {
        match self {
            PrefixRule::Literal(kind) => {
                let _ = input
                    .next_token()
                    .ok_or_else(|| ParseError::unexpected_eof("literal expression"))?;
                Ok(PrefixOutcome::Continue {
                    expr: kind.to_expr(token),
                    indent: next_indent(indent_current),
                })
            }
            PrefixRule::Identifier => {
                input
                    .next_token()
                    .ok_or_else(|| ParseError::unexpected_eof("identifier expression"))?;
                let ConcreteToken::Iden(name) = &token.token else {
                    return Err(ParseError::message(
                        "identifier rule applied to non-identifier token",
                        Some(token.clone()),
                    ));
                };

                if !is_constructor_name(name) {
                    return Ok(PrefixOutcome::Continue {
                        expr: mk_identifier_expr(token),
                        indent: next_indent(indent_current),
                    });
                }

                input.consume_trivial();
                let mut qualified = None;
                let mut constructor = token.clone();

                if let Some(dot_candidate) = input.peek_token().cloned() {
                    if matches!(dot_candidate.token, ConcreteToken::Dot) {
                        let mut lookahead = input.fork();
                        lookahead.next_token();
                        lookahead.consume_trivial();
                        match lookahead.peek_token() {
                            Some(next) if matches!(next.token, ConcreteToken::Iden(_)) => {
                                input.next_token();
                                input.consume_trivial();
                                constructor = input.next_token().ok_or_else(|| {
                                    ParseError::unexpected_eof(
                                        "constructor name after qualification",
                                    )
                                })?;
                                qualified = Some(token.clone());
                            }
                            _ => {
                                return Ok(PrefixOutcome::Continue {
                                    expr: mk_identifier_expr(token),
                                    indent: next_indent(indent_current),
                                });
                            }
                        }
                    }
                }

                // treat constructor head as an atom; if followed by record fields, parse them
                input.consume_trivial();
                let mut record_fields: Option<Vec<(ConcreteTokenAndLoc, AExprAnnot)>> = None;
                if let Some(peek_after) = input.peek_token().cloned() {
                    if matches!(peek_after.token, ConcreteToken::BraceL) {
                        input
                            .next_token()
                            .ok_or_else(|| ParseError::unexpected_eof("record constructor '{'"))?;
                        record_fields = Some(parse_record_constructor_fields(input)?);
                    }
                }

                Ok(PrefixOutcome::Continue {
                    expr: AExprAnnot {
                        expr: AExpr::ConstructorExpression(ConstructorExpr {
                            qualified,
                            constructor,
                            args: vec![],
                            record_fields,
                        }),
                        type_expr: None,
                    },
                    indent: next_indent(indent_current),
                })
            }
            PrefixRule::Lambda => {
                let expr = parse_lambda_expr(input, indent_current)?;
                Ok(PrefixOutcome::Continue {
                    expr,
                    indent: next_indent(indent_current),
                })
            }
            PrefixRule::Let => {
                let expr = parse_let_expr(input, indent_current)?;
                Ok(PrefixOutcome::Final(expr))
            }
            PrefixRule::Case => {
                let expr = parse_case_expr(input, indent_current)?;
                Ok(PrefixOutcome::Continue {
                    expr,
                    indent: next_indent(indent_current),
                })
            }
            PrefixRule::Paren => {
                input
                    .next_token()
                    .ok_or_else(|| ParseError::unexpected_eof("parenthesized expression"))?;
                // special-case unit literal: ()
                input.consume_trivial();
                match input.peek_token().cloned() {
                    Some(next) if matches!(next.token, ConcreteToken::ParenR) => {
                        let _ = input.next_token(); // consume ')'
                        Ok(PrefixOutcome::Continue {
                            expr: AExprAnnot {
                                expr: AExpr::UnitExpr,
                                type_expr: None,
                            },
                            indent: next_indent(indent_current),
                        })
                    }
                    _ => {
                        let indent_new = Indent::PrevLvl(token.loc.span_start.col);
                        let inner = parse_expr(input, indent_new, 0, false)?.ok_or_else(|| {
                            ParseError::message(
                                "expected expression inside parentheses",
                                Some(token.clone()),
                            )
                        })?;
                        let closing = input
                            .next_token()
                            .ok_or_else(|| ParseError::unexpected_eof("')' to close expression"))?;
                        if !matches!(closing.token, ConcreteToken::ParenR) {
                            return Err(ParseError::unexpected_token("')'", Some(closing)));
                        }
                        Ok(PrefixOutcome::Continue {
                            expr: inner,
                            indent: next_indent(indent_current),
                        })
                    }
                }
            }
            PrefixRule::PrefixOperator(PrefixOpInfo {
                expr_type,
                rbp,
                builtin_token,
            }) => {
                input
                    .next_token()
                    .ok_or_else(|| ParseError::unexpected_eof("prefix operator"))?;
                let arg = parse_expr(input, indent_current, rbp, false)?.ok_or_else(|| {
                    ParseError::message(
                        "expected expression after prefix operator",
                        Some(token.clone()),
                    )
                })?;
                Ok(PrefixOutcome::Continue {
                    expr: AExprAnnot {
                        expr: AExpr::ApplyExpression(AppExpr {
                            fun: Box::new(AExprAnnot {
                                expr: AExpr::IdentifierExpression(IdenExpr {
                                    iden: ConcreteTokenAndLoc {
                                        token: builtin_token,
                                        loc: token.loc.clone(),
                                    },
                                    builtin: Some(expr_type),
                                }),
                                type_expr: None,
                            }),
                            arguments: vec![arg],
                        }),
                        type_expr: None,
                    },
                    indent: next_indent(indent_current),
                })
            }
        }
    }
}

#[derive(Clone, Copy)]
enum PostfixApplicationKind {
    LiteralNumeric,
    LiteralString,
    Identifier,
    ParenArgument,
}

#[derive(Clone)]
enum PostfixRule {
    TypeAnnotation,
    Application(PostfixApplicationKind),
    InfixOperator(InfixOpInfo),
}

#[derive(Clone, Copy)]
enum PostfixBinding {
    Always,
    Lbp(usize),
}

impl PostfixBinding {
    fn can_bind_at(self, min_bp: usize) -> bool {
        match self {
            PostfixBinding::Always => true,
            PostfixBinding::Lbp(lbp) => lbp >= min_bp,
        }
    }
}

fn classify_postfix_rule(token: &ConcreteToken) -> Option<(PostfixRule, PostfixBinding)> {
    match token {
        ConcreteToken::IsType => Some((PostfixRule::TypeAnnotation, PostfixBinding::Always)),
        ConcreteToken::LiteralNumeric(_) => Some((
            PostfixRule::Application(PostfixApplicationKind::LiteralNumeric),
            PostfixBinding::Lbp(APPLICATION_BINDING_POWER),
        )),
        ConcreteToken::LiteralString(_) => Some((
            PostfixRule::Application(PostfixApplicationKind::LiteralString),
            PostfixBinding::Lbp(APPLICATION_BINDING_POWER),
        )),
        ConcreteToken::Iden(_) => Some((
            PostfixRule::Application(PostfixApplicationKind::Identifier),
            PostfixBinding::Lbp(APPLICATION_BINDING_POWER),
        )),
        ConcreteToken::ParenL => Some((
            PostfixRule::Application(PostfixApplicationKind::ParenArgument),
            PostfixBinding::Lbp(APPLICATION_BINDING_POWER),
        )),
        ConcreteToken::Plus
        | ConcreteToken::Minus
        | ConcreteToken::Star
        | ConcreteToken::FwdSlash
        | ConcreteToken::AngleL
        | ConcreteToken::AngleR
        | ConcreteToken::LessEqual
        | ConcreteToken::GreaterEqual
        | ConcreteToken::EqualEqual
        | ConcreteToken::And
        | ConcreteToken::Or => infix_op_info(token).map(|info @ InfixOpInfo { lbp, .. }| {
            (
                PostfixRule::InfixOperator(info.clone()),
                PostfixBinding::Lbp(lbp),
            )
        }),
        _ => None,
    }
}

impl PostfixApplicationKind {
    fn apply<'a, S>(
        self,
        input: &mut S,
        expr: AExprAnnot,
        peeked: ConcreteTokenAndLoc,
        indent_current: Indent,
    ) -> ParseResult<AExprAnnot>
    where
        S: ForkableTokenStream<'a>,
    {
        match self {
            PostfixApplicationKind::LiteralNumeric => {
                input
                    .next_token()
                    .ok_or_else(|| ParseError::unexpected_eof("numeric literal argument"))?;
                Ok(apply_argument(expr, mk_numeric_expr(peeked)))
            }
            PostfixApplicationKind::LiteralString => {
                input
                    .next_token()
                    .ok_or_else(|| ParseError::unexpected_eof("string literal argument"))?;
                Ok(apply_argument(expr, mk_string_expr(peeked)))
            }
            PostfixApplicationKind::Identifier => {
                // parse exactly one application atom to ensure left-associative chaining:
                //   - lower-case identifier → single identifier atom
                //   - upper-case (constructor) or qualified Type.Ctor → constructor expr with its args
                //   - note: parenthesized and literal arguments are handled by their own kinds
                let arg = parse_application_atom(input, indent_current, &peeked)?;
                Ok(apply_argument(expr, arg))
            }
            PostfixApplicationKind::ParenArgument => {
                input
                    .next_token()
                    .ok_or_else(|| ParseError::unexpected_eof("argument start '('"))?;
                let indent_new = Indent::PrevLvl(peeked.loc.span_start.col);
                let arg = parse_expr(input, indent_new, 0, false)?.ok_or_else(|| {
                    ParseError::message(
                        "expected expression inside parentheses",
                        Some(peeked.clone()),
                    )
                })?;
                match input.next_token() {
                    Some(tok) if matches!(tok.token, ConcreteToken::ParenR) => {}
                    Some(tok) => {
                        return Err(ParseError::unexpected_token("')'", Some(tok)));
                    }
                    None => {
                        return Err(ParseError::unexpected_eof("')' to close argument list"));
                    }
                }
                Ok(apply_argument(expr, arg))
            }
        }
    }
}

fn parse_application_atom<'a, S>(
    input: &mut S,
    _indent_current: Indent,
    peeked: &ConcreteTokenAndLoc,
) -> ParseResult<AExprAnnot>
where
    S: ForkableTokenStream<'a>,
{
    match &peeked.token {
        ConcreteToken::Iden(name) => {
            // consume the identifier token
            let token = input
                .next_token()
                .ok_or_else(|| ParseError::unexpected_eof("identifier argument"))?;

            if !is_constructor_name(name) {
                return Ok(mk_identifier_expr(token));
            }

            // possible qualified constructor head as atom: Type.Ctor
            let mut qualified = None;
            let mut constructor = token.clone();

            if let Some(dot_candidate) = input.peek_token().cloned() {
                if matches!(dot_candidate.token, ConcreteToken::Dot) {
                    let mut lookahead = input.fork();
                    lookahead.next_token();
                    lookahead.consume_trivial();
                    match lookahead.peek_token() {
                        Some(next) if matches!(next.token, ConcreteToken::Iden(_)) => {
                            // consume '.' and the ctor identifier
                            input.next_token();
                            input.consume_trivial();
                            constructor = input.next_token().ok_or_else(|| {
                                ParseError::unexpected_eof("constructor name after qualification")
                            })?;
                            qualified = Some(token.clone());
                        }
                        _ => {}
                    }
                }
            }

            Ok(AExprAnnot {
                expr: AExpr::ConstructorExpression(ConstructorExpr {
                    qualified,
                    constructor,
                    args: vec![],
                    record_fields: None,
                }),
                type_expr: None,
            })
        }
        _other => Err(ParseError::unexpected_token(
            "identifier argument",
            Some(peeked.clone()),
        )),
    }
}

impl PostfixRule {
    fn apply<'a, S>(
        self,
        input: &mut S,
        expr: AExprAnnot,
        peeked: ConcreteTokenAndLoc,
        indent_current: Indent,
    ) -> ParseResult<PostfixOutcome>
    where
        S: ForkableTokenStream<'a>,
    {
        match self {
            PostfixRule::TypeAnnotation => {
                input
                    .next_token()
                    .ok_or_else(|| ParseError::unexpected_eof("type annotation '::'"))?;
                let type_expr = parse_type_expr(input, Indent::PrevLvl(peeked.loc.span_start.col))?
                    .ok_or_else(|| {
                        ParseError::message(
                            "unable to parse type expr in type annotation",
                            Some(peeked.clone()),
                        )
                    })?;
                let mut expr = expr;
                expr.type_expr = Some(type_expr);
                // stop further postfix after a type annotation; require parentheses to continue
                Ok(PostfixOutcome::Stop(expr))
            }
            PostfixRule::Application(kind) => {
                let expr = kind.apply(input, expr, peeked, indent_current)?;
                Ok(PostfixOutcome::Continue(expr))
            }
            PostfixRule::InfixOperator(info) => {
                let info = info.clone();
                input
                    .next_token()
                    .ok_or_else(|| ParseError::unexpected_eof("infix operator"))?;
                let rhs = parse_expr(input, indent_current, info.rbp, false)?.ok_or_else(|| {
                    ParseError::message(
                        "expected expression on right side of operator",
                        Some(peeked.clone()),
                    )
                })?;
                let lhs = expr;
                let applied = AExprAnnot {
                    expr: AExpr::ApplyExpression(AppExpr {
                        fun: Box::new(AExprAnnot {
                            expr: AExpr::IdentifierExpression(IdenExpr {
                                iden: ConcreteTokenAndLoc {
                                    token: info.builtin_token.clone(),
                                    loc: peeked.loc.clone(),
                                },
                                builtin: Some(info.expr_type),
                            }),
                            type_expr: None,
                        }),
                        arguments: vec![lhs, rhs],
                    }),
                    type_expr: None,
                };
                Ok(PostfixOutcome::Continue(applied))
            }
        }
    }
}

enum PostfixOutcome {
    Continue(AExprAnnot),
    Stop(AExprAnnot),
}

// pratt parsing
pub(crate) fn parse_expr<'a, S>(
    input: &mut S,
    indent: Indent,
    min_bp: usize,
    align_to_indent: bool,
) -> ParseResult<Option<AExprAnnot>>
where
    S: ForkableTokenStream<'a>,
{
    input.consume_trivial();
    let prefix = match parse_prefix_expr(input, indent, align_to_indent)? {
        Some(prefix) => prefix,
        None => return Ok(None),
    };
    let (mut expr, mut indent_current) = match prefix {
        PrefixOutcome::Continue { expr, indent } => (expr, indent),
        PrefixOutcome::Final(expr) => return Ok(Some(expr)),
    };

    loop {
        input.consume_trivial();
        let Some(next_token) = input.peek_token().cloned() else {
            break;
        };

        let Some((rule, binding)) = classify_postfix_rule(&next_token.token) else {
            break;
        };

        if !binding.can_bind_at(min_bp) {
            break;
        }

        indent_current = match update_indent(
            indent_current,
            Indent::CurLvl(next_token.loc.span_start.col),
        ) {
            Ok(indent) => indent,
            Err(_) => break,
        };

        match rule.apply(input, expr, next_token, indent_current)? {
            PostfixOutcome::Continue(new_expr) => {
                expr = new_expr;
            }
            PostfixOutcome::Stop(new_expr) => {
                expr = new_expr;
                break;
            }
        }
    }

    Ok(Some(expr))
}

enum PrefixOutcome {
    Continue { expr: AExprAnnot, indent: Indent },
    Final(AExprAnnot),
}

fn parse_prefix_expr<'a, S>(
    input: &mut S,
    indent: Indent,
    align_to_indent: bool,
) -> ParseResult<Option<PrefixOutcome>>
where
    S: ForkableTokenStream<'a>,
{
    let Some(token) = input.peek_token().cloned() else {
        return Ok(None);
    };
    if token.token == ConcreteToken::EndOfFile {
        return Ok(None);
    }

    let indent_current = if align_to_indent {
        align_indent(indent, Indent::CurLvl(token.loc.span_start.col))
            .map_err(|e| ParseError::indentation(e, Some(token.clone())))?
    } else {
        update_indent(indent, Indent::CurLvl(token.loc.span_start.col))
            .map_err(|e| ParseError::indentation(e, Some(token.clone())))?
    };

    let Some(prefix_rule) = classify_prefix_rule(&token.token) else {
        return Err(ParseError::unexpected_token(
            "expression",
            Some(token.clone()),
        ));
    };

    Ok(Some(prefix_rule.parse(
        input,
        token.clone(),
        indent_current,
    )?))
}

fn parse_case_expr<'a>(
    input: &mut impl ForkableTokenStream<'a>,
    indent: Indent,
) -> ParseResult<AExprAnnot> {
    let keyword = input
        .next_token()
        .ok_or_else(|| ParseError::unexpected_eof("case expression"))?;
    if !matches!(keyword.token, ConcreteToken::Case) {
        return Err(ParseError::unexpected_token(
            "'case' keyword",
            Some(keyword),
        ));
    }

    let cur_indent = align_indent(indent, Indent::CurLvl(keyword.loc.span_start.col))
        .map_err(|e| ParseError::indentation(e, Some(keyword.clone())))?;

    let indent_case_lvl;

    let indent_case = match cur_indent {
        Indent::CurLvl(lvl) => {
            indent_case_lvl = lvl;
            Indent::PrevLvl(lvl)
        }
        _ => {
            return Err(ParseError::message(
                "expected current indentation for case expression",
                Some(keyword.clone()),
            ));
        }
    };

    let mut argument_tokens = Vec::new();
    loop {
        let token = input
            .next_token()
            .ok_or_else(|| ParseError::unexpected_eof("case scrutinee"))?;
        if matches!(token.token, ConcreteToken::Of) {
            break;
        }
        argument_tokens.push(token);
    }

    input.consume_trivial();
    let clause_base_indent = input
        .peek_token()
        .map(|tok| token_indent(tok))
        .unwrap_or(indent_case_lvl);

    let clauses = collect_layout_items_result(input, clause_base_indent, false, |stream| {
        stream.consume_trivial();
        let Some(next) = stream.peek_token().cloned() else {
            return Ok(None);
        };

        if is_keyword_in(&next.token) && token_indent(&next) <= indent_case_lvl {
            return Ok(None);
        }

        if is_layout_block_terminator(&next.token) {
            return Ok(None);
        }

        if token_indent(&next) < clause_base_indent {
            return Ok(None);
        }

        let clause_indent_val = token_indent(&next);
        let parsed_pattern = parse_pattern_stream(stream)
            .map_err(|e| ParseError::message(format!("pattern parsing error: {:?}", e), None))?;

        stream.consume_trivial();
        let mut guard_expr = None;
        if let Some(peek) = stream.peek_token().cloned() {
            if matches!(peek.token, ConcreteToken::VertBar) {
                let bar = stream
                    .next_token()
                    .ok_or_else(|| ParseError::unexpected_eof("guard '|'"))?;
                let indent_guard = Indent::PrevLvl(bar.loc.span_start.col);
                let guard = parse_expr(stream, indent_guard, 0, false)?.ok_or_else(|| {
                    ParseError::message("expected guard expression after '|'", Some(bar.clone()))
                })?;
                guard_expr = Some(guard);
                stream.consume_trivial();
            }
        }

        let arrow = stream
            .next_token()
            .ok_or_else(|| ParseError::unexpected_eof("case arrow"))?;
        if !matches!(arrow.token, ConcreteToken::ArrowRight) {
            return Err(ParseError::unexpected_token("->", Some(arrow)));
        }
        let indent_after_arrow = Indent::PrevLvl(arrow.loc.span_start.col);

        let clause_body = gather_exprs_until(stream, indent_after_arrow, |next| {
            if next.loc.span_start.col < clause_indent_val {
                return true;
            }

            if next.loc.span_start.col == clause_indent_val
                && (is_pattern_start_token(&next.token) || matches!(next.token, ConcreteToken::In))
            {
                return true;
            }

            matches!(
                next.token,
                ConcreteToken::ParenR | ConcreteToken::BraceR | ConcreteToken::BracketR
            )
        })?;

        Ok(Some(CaseClause {
            pattern: parsed_pattern,
            guard: guard_expr,
            body: Box::new(AExprAnnot {
                expr: AExpr::BlockExpression(BlockExpr(clause_body)),
                type_expr: None,
            }),
        }))
    })?;

    let precedence = 0;
    let mut argument_stream = Parser::new(argument_tokens.iter());
    let argument_expr = parse_expr(&mut argument_stream, indent_case, precedence, false)?
        .ok_or_else(|| {
            ParseError::message(
                "cannot parse argument expression of case",
                Some(keyword.clone()),
            )
        })?;

    Ok(AExprAnnot {
        expr: AExpr::CaseExpression(CaseExpr {
            keyword: keyword.clone(),
            argument: Box::new(argument_expr),
            clauses,
        }),
        type_expr: None,
    })
}

fn parse_function_binding_header(
    tokens: &[ConcreteTokenAndLoc],
) -> Option<(ConcreteTokenAndLoc, Vec<PatternExpr>)> {
    let mut parser = Parser::new(tokens.iter());
    let first = TokenStreamExt::next_non_trivial(&mut parser)?;
    let ConcreteToken::Iden(name) = &first.token else {
        return None;
    };
    if is_constructor_name(name) {
        return None;
    }

    let mut params = Vec::new();

    while let Some(next) = TokenStreamExt::peek_non_trivial(&mut parser) {
        if !is_pattern_start_token(&next.token) {
            return None;
        }
        let pattern = parse_pattern_stream(&mut parser).ok()?;
        params.push(pattern);
    }

    if params.is_empty() {
        return None;
    }

    Some((first, params))
}

// parse a definition; currently used in parsing let expression definitions
fn parse_let_def<'a>(
    input: &mut impl ForkableTokenStream<'a>,
    indent: Indent,
) -> ParseResult<Option<(PatternExpr, AExprAnnot)>> {
    let lvl = match indent {
        Indent::CurLvl(lvl) => lvl,
        _ => {
            return Err(ParseError::message(
                "invalid indentation while parsing let binding",
                None,
            ));
        }
    };

    input.consume_trivial();
    let mut dedent_error: Option<ConcreteTokenAndLoc> = None;
    let (pattern_tokens, stop_token) = input.collect_balanced_until_result(|next, balance| {
        if next.loc.span_start.col < lvl {
            dedent_error = Some(next.clone());
            return true;
        }

        balance.at_top() && matches!(next.token, ConcreteToken::IsType | ConcreteToken::Equal)
    })?;

    if let Some(tok) = dedent_error {
        return Err(ParseError::indentation(
            "unexpected dedent while parsing let pattern",
            Some(tok),
        ));
    }

    let stop_token =
        stop_token.ok_or_else(|| ParseError::unexpected_eof("let binding separator"))?;

    if pattern_tokens.is_empty() {
        return Ok(None);
    }

    let fn_header = parse_function_binding_header(&pattern_tokens);
    let pattern = match parse_pattern(&pattern_tokens) {
        Ok(pattern) => pattern,
        Err(err) => match fn_header.as_ref() {
            Some((name, _header_params)) => PatternExpr::Variable(name.clone()),
            None => {
                return Err(ParseError::message(
                    format!("invalid let-binding pattern: {:?}", err),
                    pattern_tokens.first().cloned(),
                ));
            }
        },
    };

    let mut parsed_type_annot = None;

    match &stop_token.token {
        ConcreteToken::IsType => {
            input.consume_trivial();
            let Some(type_start) = input.peek_token().cloned() else {
                return Err(ParseError::unexpected_eof("type expression in let binding"));
            };
            let ty_indent = Indent::CurLvl(type_start.loc.span_start.col);
            parsed_type_annot = Some(parse_type_expr(input, ty_indent)?.ok_or_else(|| {
                ParseError::message(
                    "unable to parse type expression in let binding",
                    Some(type_start.clone()),
                )
            })?);
            input.consume_trivial();
            match input.next_token() {
                Some(tok) if matches!(tok.token, ConcreteToken::Equal) => {}
                Some(tok) => {
                    return Err(ParseError::unexpected_token("=", Some(tok)));
                }
                None => {
                    return Err(ParseError::unexpected_eof("= in let binding"));
                }
            }
        }
        ConcreteToken::Equal => {}
        _ => {
            return Err(ParseError::unexpected_token(
                "type annotation or '=' in let binding",
                Some(stop_token.clone()),
            ));
        }
    }

    input.consume_trivial();
    let mut rhs_expr = parse_expr(input, Indent::PrevLvl(lvl), 0, false)?.ok_or_else(|| {
        ParseError::message(
            "expected expression in let binding",
            pattern_tokens.last().cloned(),
        )
    })?;

    if let Some(type_expr) = parsed_type_annot {
        rhs_expr.type_expr = Some(type_expr);
    }

    if let Some((fn_name_token, header_params)) = fn_header {
        let param_binders = collect_function_param_binders(&header_params, &fn_name_token);
        let fn_type_annotation = rhs_expr.type_expr.take();
        let body_expr = rhs_expr;
        rhs_expr = AExprAnnot {
            expr: AExpr::AbstractionExpression(AbstractionExpr {
                name: None,
                pattern: param_binders,
                param_patterns: header_params,
                expr: Box::new(body_expr),
                type_expr: fn_type_annotation,
            }),
            type_expr: None,
        };
    }

    Ok(Some((pattern, rhs_expr)))
}

fn parse_let_expr<'a>(
    input: &mut impl ForkableTokenStream<'a>,
    _indent: Indent,
) -> ParseResult<AExprAnnot> {
    let keyword = input
        .next_token()
        .ok_or_else(|| ParseError::unexpected_eof("let expression"))?;

    match keyword.token {
        ConcreteToken::Let => {}
        _ => {
            return Err(ParseError::unexpected_token("'let' keyword", Some(keyword)));
        }
    }

    let _indent = Indent::PrevLvl(keyword.loc.span_start.col);

    let base_indent = keyword.loc.span_start.col;
    let defs = collect_layout_items_result(
        input,
        base_indent,
        false,
        |stream| -> ParseResult<Option<(PatternExpr, AExprAnnot)>> {
            loop {
                stream.consume_trivial();
                let Some(next) = stream.peek_token().cloned() else {
                    return Ok(None);
                };

                // stop when we see 'in' at the let base indent
                if is_keyword_in(&next.token) && token_indent(&next) == base_indent {
                    return Ok(None);
                }

                // allow repeated 'let' keywords at the same base indent as separators
                if matches!(next.token, ConcreteToken::Let) && token_indent(&next) == base_indent {
                    let _ = stream.next_token();
                    continue;
                }

                if token_indent(&next) < base_indent {
                    return Ok(None);
                }

                let indent_defs = Indent::CurLvl(token_indent(&next));
                return parse_let_def(stream, indent_defs);
            }
        },
    )?;

    input.consume_trivial();
    let next = input
        .next_token()
        .ok_or_else(|| ParseError::unexpected_eof("'in' after let bindings"))?;
    if !is_keyword_in(&next.token) || token_indent(&next) != base_indent {
        return Err(ParseError::unexpected_token(
            "'in' after let bindings",
            Some(next),
        ));
    }
    let indent_token_in = next.loc.span_start.col;

    input.consume_trivial();
    let expr = parse_expr(input, Indent::PrevLvl(indent_token_in), 0, false)?
        .ok_or_else(|| ParseError::unexpected_eof("expression after 'in'"))?;

    Ok(AExprAnnot {
        expr: AExpr::LetExpression(LetExpr {
            defs,
            expr: Box::new(expr),
        }),
        type_expr: None,
    })
}

fn parse_lambda_expr<'a>(
    input: &mut impl ForkableTokenStream<'a>,
    _indent: Indent,
) -> ParseResult<AExprAnnot> {
    // parses expr of the form: \a b .. -> body

    let keyword = input
        .next_token()
        .ok_or_else(|| ParseError::unexpected_eof("lambda expression"))?;

    match keyword.token {
        ConcreteToken::BackSlash => {}
        _ => {
            return Err(ParseError::unexpected_token("'\\'", Some(keyword)));
        }
    }

    let _indent = Indent::PrevLvl(keyword.loc.span_start.col);

    let (param_pattern, arrow_token) = input.collect_balanced_until_result(|next, balance| {
        balance.at_top() && matches!(next.token, ConcreteToken::ArrowRight)
    })?;
    let arrow = arrow_token
        .ok_or_else(|| ParseError::message("expected '->' after lambda parameters", None))?;
    let indent_token_arrow_right = match &arrow.token {
        ConcreteToken::ArrowRight => arrow.loc.span_start.col,
        _ => {
            return Err(ParseError::unexpected_token(
                "'->' after lambda parameters",
                Some(arrow.clone()),
            ));
        }
    };
    input.consume_trivial();
    let expr = parse_expr(input, Indent::PrevLvl(indent_token_arrow_right), 0, false)?.ok_or_else(
        || ParseError::message("expected expression in lambda body", Some(arrow.clone())),
    )?;

    let param_patterns = tokens_to_patterns(&param_pattern)?;

    Ok(AExprAnnot {
        expr: AExpr::AbstractionExpression(AbstractionExpr {
            name: None,
            pattern: param_pattern,
            param_patterns,
            expr: Box::new(expr),
            type_expr: None,
        }),
        type_expr: None,
    })
}
fn parse_required_type_annotation<'a, S>(input: &mut S, ctx: &str) -> ParseResult<ATypeExprComplex>
where
    S: TokenStreamExt<'a>,
{
    let annot = input
        .next_token()
        .ok_or_else(|| ParseError::message(format!("expected '::' {ctx}"), None))?;
    if !matches!(annot.token, ConcreteToken::IsType) {
        return Err(ParseError::unexpected_token("'::'", Some(annot)));
    }

    input.consume_trivial();
    let Some(type_start) = input.peek_token().cloned() else {
        return Err(ParseError::message(
            format!("expected type expression {ctx}"),
            None,
        ));
    };
    let ty_indent = Indent::CurLvl(type_start.loc.span_start.col);
    let parsed = parse_type_expr(input, ty_indent)?.ok_or_else(|| {
        ParseError::message(
            format!("unable to parse type expression {ctx}"),
            Some(type_start.clone()),
        )
    })?;
    Ok(parsed)
}

static TEST_CONTENT_ABSTRACT_PARSER: &str = r###"
// f x = 9 as f32 / ((-7*x as i32) as f32)
f x y z = 9 / (-7*x+y+z)

f_let a =
 let x :: u32 = 1
     y :: u32 = 2
     z :: u32 = 5
 in
   x + y :: u32 + z + a

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
  v :: u32
  x :: B
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
  in a
  let b :: f32 = 7
  in b
  f5 7
  x
  (7*9)
  (77*2)

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

// f2 :: T -> T2
// f2 a =
//   a * 4.0
//   let ret =
//     a * 4.0
//   ret

simple :: u32
simple = 77

f3 :: T -> T2
f3 a =
  a * (7.0 + a)
  c * a
  let b = 7.0 + a * 4.0
      c = b*a+1
  in
    let zz = 88 * f a b (c+e) (7 + 9 * 7)
        ret = case z of
               "a"         -> ((0 :: u32) * 5)
                              77
               "something" -> f2 1
               _           -> 2
    in 
      (f2 4 5) * 77
  77 + 9 * 3.0 / 2
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
 7 + 9

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
 (7 :: f32)  + 9.0
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

#[cfg(test)]
mod tests {
    use crate::parse::lex::parse_content_to_concrete_tokens;
    use crate::parse::loc::LexedTokensAndLocs;
    use std::path::Path;

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

        let filtered = LexedTokensAndLocs(
            super::space_and_delimiter_and_comment_filter(lexed_output.0.iter())
                .cloned()
                .collect(),
        );

        let mut parser = super::Parser::new(filtered.0.iter());
        let expr = super::parse_expr(&mut parser, super::Indent::CurLvl(0), 0, false)
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

        let filtered = LexedTokensAndLocs(
            super::space_and_delimiter_and_comment_filter(lexed_output.0.iter())
                .cloned()
                .collect(),
        );

        let mut parser = super::Parser::new(filtered.0.iter());
        let expr = super::parse_expr(&mut parser, super::Indent::CurLvl(0), 0, false)
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
        let filtered = LexedTokensAndLocs(
            super::space_and_delimiter_and_comment_filter(lexed.0.iter())
                .cloned()
                .collect(),
        );

        let mut parser = super::Parser::new(filtered.0.iter());
        let expr = super::parse_expr(&mut parser, super::Indent::CurLvl(0), 0, false)
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
    fn test_application_left_associative_simple() {
        use super::ConcreteToken;
        use std::path::Path;

        let input = "f g x";
        let lexed = parse_content_to_concrete_tokens(Path::new("/test"), input)
            .expect("lexing should succeed");
        let filtered = LexedTokensAndLocs(
            super::space_and_delimiter_and_comment_filter(lexed.0.iter())
                .cloned()
                .collect(),
        );

        let mut parser = super::Parser::new(filtered.0.iter());
        let expr = super::parse_expr(&mut parser, super::Indent::CurLvl(0), 0, false)
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
        let filtered = LexedTokensAndLocs(
            super::space_and_delimiter_and_comment_filter(lexed.0.iter())
                .cloned()
                .collect(),
        );

        let mut parser = super::Parser::new(filtered.0.iter());
        let expr = super::parse_expr(&mut parser, super::Indent::CurLvl(0), 0, false)
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

        let lexed_output_filtered = LexedTokensAndLocs(
            super::space_and_delimiter_and_comment_filter(lexed_output.0.iter())
                .cloned()
                .collect(),
        );

        let top_level_items = super::parse_concrete_top_level(lexed_output_filtered)
            .expect("abstract parser should succeed for test fixture");

        // avoid printing during tests; ensure items parsed without panic
        assert!(
            top_level_items.len() > 0,
            "expected at least one top-level item"
        );
        // println!("{}", top_level_items[0]);
    }
}
