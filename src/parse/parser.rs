use std::fmt;

use crate::parse::concrete_token::ConcreteToken;
use crate::parse::layout::*;
use crate::parse::loc::{ConcreteTokenAndLoc, LexedTokensAndLocs, Location};

// parser helpers and errors

#[derive(Clone, Debug)]
pub(crate) enum ParseError {
    UnexpectedToken {
        expected: String,
        found: Option<ConcreteTokenAndLoc>,
    },
    UnexpectedEof {
        context: &'static str,
    },
    Indentation {
        message: String,
        token: Option<ConcreteTokenAndLoc>,
    },
    DelimiterMismatch {
        delimiter: &'static str,
        token: ConcreteTokenAndLoc,
    },
    Message {
        message: String,
        token: Option<ConcreteTokenAndLoc>,
    },
    LocatedMessage {
        message: String,
        location: Location,
    },
}

pub(crate) type ParseResult<T> = Result<T, ParseError>;

impl ParseError {
    pub(crate) fn unexpected_token<S: Into<String>>(
        expected: S,
        found: Option<ConcreteTokenAndLoc>,
    ) -> Self {
        Self::UnexpectedToken {
            expected: expected.into(),
            found,
        }
    }

    pub(crate) fn unexpected_eof(context: &'static str) -> Self {
        Self::UnexpectedEof { context }
    }

    pub(crate) fn indentation<S: Into<String>>(
        message: S,
        token: Option<ConcreteTokenAndLoc>,
    ) -> Self {
        Self::Indentation {
            message: message.into(),
            token,
        }
    }

    pub(crate) fn delimiter(delimiter: &'static str, token: ConcreteTokenAndLoc) -> Self {
        Self::DelimiterMismatch { delimiter, token }
    }

    pub(crate) fn message<S: Into<String>>(message: S, token: Option<ConcreteTokenAndLoc>) -> Self {
        Self::Message {
            message: message.into(),
            token,
        }
    }

    pub(crate) fn located_message<S: Into<String>>(message: S, location: Location) -> Self {
        Self::LocatedMessage {
            message: message.into(),
            location,
        }
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseError::UnexpectedToken { expected, found } => {
                if let Some(tok) = found {
                    write!(
                        f,
                        "unexpected token {:?}, expected {} at {:?}",
                        tok.token, expected, tok.loc
                    )
                } else {
                    write!(f, "unexpected token, expected {}", expected)
                }
            }
            ParseError::UnexpectedEof { context } => {
                write!(f, "unexpected end of input while parsing {context}")
            }
            ParseError::Indentation { message, token } => {
                if let Some(tok) = token {
                    write!(f, "indentation error: {message} at {:?}", tok.loc)
                } else {
                    write!(f, "indentation error: {message}")
                }
            }
            ParseError::DelimiterMismatch { delimiter, token } => {
                write!(f, "unmatched {delimiter} at {:?}", token.loc)
            }
            ParseError::Message { message, token } => {
                if let Some(tok) = token {
                    write!(f, "{message} at {:?}", tok.loc)
                } else {
                    write!(f, "{message}")
                }
            }
            ParseError::LocatedMessage { message, location } => {
                write!(f, "{message} at {location:?}")
            }
        }
    }
}

impl std::error::Error for ParseError {}

#[derive(Clone, Debug)]
pub(crate) struct Parser {
    layout_stream: LayoutStream,
    item_scope_owners: Vec<LayoutId>,
}

///
/// this provides a RAII style parser capability an item callback receives while
/// parsing one implicit layout block
///
/// parent layout helper remains responsible for consuming its own
/// separator/end markers
///
/// this capability only exposes concrete token methods, see
/// ConcreteTokenSource trait
pub(crate) struct LayoutItemParser<'a> {
    parser: &'a mut Parser,
    owner: LayoutId,
}

/// this allows guidance of an explicit closing of a layout block
///
/// note that physical `}` that closes layout is handled by
/// `LayoutStream` and cannot be requested through this
#[derive(Clone, Debug)]
pub(crate) enum LayoutFeedback {
    None,
    BeforeIn,
}

impl LayoutFeedback {
    fn matches(&self, token: &ConcreteToken) -> bool {
        match self {
            Self::None => false,
            Self::BeforeIn => matches!(token, ConcreteToken::In),
        }
    }
}

/// capability for accessing only concrete tokens
pub(crate) trait ConcreteTokenSource {
    fn peek_concrete(&mut self) -> ParseResult<Option<ConcreteTokenAndLoc>>;
    fn next_concrete(&mut self) -> ParseResult<Option<ConcreteTokenAndLoc>>;
    fn expect_concrete(&mut self, expected: &ConcreteToken) -> ParseResult<ConcreteTokenAndLoc>;
}

/// capability for initiating implicit layout block
pub(crate) trait LayoutGrammarSource: ConcreteTokenSource {
    fn parse_layout_items<T>(
        &mut self,
        anchor: Location,
        allow_empty: bool,
        parser_fn: impl FnMut(&mut LayoutItemParser<'_>) -> ParseResult<T>,
        feedback: LayoutFeedback,
    ) -> ParseResult<Vec<T>>;
}

impl Parser {
    pub(crate) fn new(tokens: LexedTokensAndLocs) -> Self {
        Self {
            layout_stream: LayoutStream::new(tokens),
            item_scope_owners: Vec::new(),
        }
    }
    // Return the next logical token without consuming it. The token may be
    // concrete or generated by layout processing. If the result is concrete,
    // `parse_layout_block` may subsequently open a block before that token.
    pub(crate) fn peek(&mut self) -> ParseResult<Option<&ParserToken>> {
        self.layout_stream.peek()
    }
    pub(crate) fn next(&mut self) -> ParseResult<Option<ParserToken>> {
        if let Some(owner) = self.item_scope_owners.first().copied() {
            let blocked = self
                .layout_stream
                .peek()?
                .and_then(|token| layout_marker_owner(&token.ty))
                == Some(owner);
            if blocked {
                let location = self
                    .layout_stream
                    .peek()?
                    .map(|token| token.loc.clone())
                    .expect("blocked marker was peeked");
                return Err(ParseError::located_message(
                    format!(
                        "item callback cannot consume parent layout marker {:?}",
                        owner
                    ),
                    location,
                ));
            }
        }
        self.layout_stream.next()
    }
    pub(crate) fn expect_concrete(
        &mut self,
        expected: &ConcreteToken,
    ) -> ParseResult<ConcreteTokenAndLoc> {
        let item = self
            .peek()?
            .ok_or_else(|| ParseError::unexpected_eof("concrete token"))?;

        // un-consuming check
        match &item.ty {
            ParserTokenType::Concrete(token) if same_concrete_variant(token, expected) => {}
            ParserTokenType::Concrete(token) => {
                return Err(ParseError::unexpected_token(
                    format!("{:?}", expected),
                    Some(ConcreteTokenAndLoc {
                        token: token.clone(),
                        loc: item.loc.clone(),
                        starts_a_line: false,
                    }),
                ));
            }
            layout_token => {
                return Err(ParseError::message(
                    format!(
                        "unexpected layout token {:?}, expected concrete token {:?} at {:?}",
                        layout_token, expected, item.loc
                    ),
                    None,
                ));
            }
        }

        // consume
        let ParserToken {
            ty: ParserTokenType::Concrete(token),
            loc,
        } = self.next()?.unwrap()
        else {
            unreachable!();
        };

        Ok(ConcreteTokenAndLoc {
            token,
            loc,
            starts_a_line: false,
        })
    }

    pub(crate) fn peek_concrete(&mut self) -> ParseResult<Option<ConcreteTokenAndLoc>> {
        match self.peek()? {
            Some(ParserToken {
                ty: ParserTokenType::Concrete(token),
                loc,
            }) => Ok(Some(ConcreteTokenAndLoc {
                token: token.clone(),
                loc: loc.clone(),
                starts_a_line: false,
            })),
            Some(ParserToken {
                ty: ParserTokenType::LayoutStart(_),
                loc,
            }) => Err(ParseError::located_message(
                "unexpected layout start while expecting a concrete token",
                loc.clone(),
            )),
            Some(ParserToken {
                ty: ParserTokenType::LayoutSeparator(_) | ParserTokenType::LayoutEnd(_),
                ..
            })
            | None => Ok(None),
        }
    }

    pub(crate) fn next_concrete(&mut self) -> ParseResult<Option<ConcreteTokenAndLoc>> {
        if self.peek_concrete()?.is_none() {
            return Ok(None);
        }
        let token = self
            .next()?
            .ok_or_else(|| ParseError::unexpected_eof("concrete token"))?;
        Ok(token.tok_concrete_and_loc())
    }

    fn consume_layout_start(&mut self, expected_id: LayoutId) -> ParseResult<()> {
        let token = self
            .peek()?
            .ok_or_else(|| ParseError::message("next returned empty", None))?;
        match &token.ty {
            ParserTokenType::LayoutStart(id) if *id == expected_id => {}
            ParserTokenType::LayoutStart(id) => {
                return Err(ParseError::located_message(
                    format!(
                        "expecting layout start {:?}, found layout start {:?}",
                        expected_id, id
                    ),
                    token.loc.clone(),
                ));
            }
            _ => {
                return Err(ParseError::message(
                    format!("expecting layout start {:?}", expected_id),
                    token.tok_concrete_and_loc(),
                ));
            }
        }
        self.next()?;
        Ok(())
    }
    fn consume_layout_end(&mut self, expected_id: LayoutId) -> ParseResult<()> {
        let token = self
            .peek()?
            .ok_or_else(|| ParseError::message("next returned empty", None))?;
        match &token.ty {
            ParserTokenType::LayoutEnd(id) if *id == expected_id => {}
            ParserTokenType::LayoutEnd(id) => {
                return Err(ParseError::located_message(
                    format!(
                        "expecting layout end {:?}, found layout end {:?}",
                        expected_id, id
                    ),
                    token.loc.clone(),
                ));
            }
            _ => {
                return Err(ParseError::message(
                    format!("expecting layout end {:?}", expected_id),
                    token.tok_concrete_and_loc(),
                ));
            }
        }
        self.next()?;
        Ok(())
    }
    fn consume_layout_separator(&mut self, expected_id: LayoutId) -> ParseResult<()> {
        let token = self
            .peek()?
            .ok_or_else(|| ParseError::message("next returned empty", None))?;
        match &token.ty {
            ParserTokenType::LayoutSeparator(id) if *id == expected_id => {}
            ParserTokenType::LayoutSeparator(id) => {
                return Err(ParseError::located_message(
                    format!(
                        "expecting layout separator {:?}, found layout separator {:?}",
                        expected_id, id
                    ),
                    token.loc.clone(),
                ));
            }
            _ => {
                return Err(ParseError::message(
                    format!("expecting layout separator {:?}", expected_id),
                    token.tok_concrete_and_loc(),
                ));
            }
        }
        self.next()?;
        Ok(())
    }

    /// user is expected to call this when there is a keyword/identifier that
    /// signals to start a new layout block
    ///
    /// a layout block is identified by an id (LayoutId) and boundaries
    /// of a layout block is identified by virtual delimiters:
    /// LayoutStart(id), LayoutSeprator(id), LayoutEnd(id)
    ///
    /// this function handles lifecycle of its own layout, by
    /// consuming LayoutStart, LayoutSeparator, LayoutEnd virtual tokens
    ///
    /// user is expected to provide `parser_fn`  to parse 1 item inside a layout block
    /// and must parse up to but not consume its own block's virtual token
    /// (LayoutSeparator/LayoutEnd)
    ///
    /// `parser_fn` must leave this block's separator/end unconsumed. It may parse
    /// nested layout blocks, but each nested helper must consume its own markers
    /// before `parser_fn` returns
    ///
    /// `anchor` is the location that triggers start of a layout block
    ///
    /// A caller may use `peek` to inspect the first concrete item and obtain its
    /// location before calling this method; opening a layout preserves that lookahead.
    ///
    /// `layout_feedback` allows user to say which follow token can be present that
    /// can trigger a layout block to end, in the case that column layout rules
    /// cannot be used to ending a layout block; physical `}` closure is normalized
    /// by LayoutStream and the user don't need to interact with that token
    pub(crate) fn parse_layout_block<T>(
        &mut self,
        anchor: Location,
        allow_empty: bool,
        mut parser_fn: impl FnMut(&mut LayoutItemParser<'_>) -> ParseResult<T>,
        layout_feedback: LayoutFeedback,
    ) -> ParseResult<Vec<T>> {
        let layout_id = self.layout_stream.open_implicit_layout(&anchor)?;
        self.consume_layout_start(layout_id)?;

        let mut items = vec![];

        let check_allow_empty = |items: &[T]| -> ParseResult<()> {
            if items.is_empty() && !allow_empty {
                return ParseResult::Err(ParseError::located_message(
                    "expect non-empty result after parsing",
                    anchor.clone(),
                ));
            }
            Ok(())
        };

        loop {
            // item or closure
            match self.peek()? {
                Some(ParserToken {
                    ty: ParserTokenType::LayoutEnd(id),
                    ..
                }) if *id == layout_id => {
                    self.consume_layout_end(layout_id)?;
                    check_allow_empty(&items)?;
                    return ParseResult::Ok(items);
                }
                Some(ParserToken {
                    ty: ParserTokenType::LayoutEnd(id),
                    loc,
                }) => {
                    return Err(ParseError::located_message(
                        format!(
                            "layout block {:?} encountered end owned by {:?}",
                            layout_id, id
                        ),
                        loc.clone(),
                    ));
                }
                Some(ParserToken {
                    ty: ParserTokenType::LayoutSeparator(id),
                    loc,
                }) => {
                    let message = if *id == layout_id {
                        format!(
                            "layout block {:?} encountered a separator while expecting an item",
                            layout_id
                        )
                    } else {
                        format!(
                            "layout block {:?} encountered separator owned by {:?}",
                            layout_id, id
                        )
                    };
                    return Err(ParseError::located_message(message, loc.clone()));
                }
                Some(ParserToken {
                    ty: ParserTokenType::Concrete(token),
                    ..
                }) if layout_feedback.matches(token) => {
                    // handle the case where it is expected that LayoutEnd can
                    // occur, but is not automatically handled by column layout
                    // rules by the layout abstraction layer due to inlining on
                    // the same line (which is still valid)
                    self.layout_stream
                        .close_implicit_layout_before_current(layout_id)?;
                    self.consume_layout_end(layout_id)?;
                    check_allow_empty(&items)?;
                    return ParseResult::Ok(items);
                }
                Some(token) if matches!(&token.ty, ParserTokenType::Concrete(_)) => {
                    let item = {
                        // use of an RAII structure LayoutItemParser to push layout id;
                        // once parsing is done, that layout id is popped
                        let mut item_parser = LayoutItemParser::new(self, layout_id);
                        parser_fn(&mut item_parser)?
                    };
                    items.push(item);
                }
                _ => {
                    return ParseResult::Err(ParseError::located_message(
                        "expect LayoutEnd / ConcreteToken",
                        anchor.clone(),
                    ));
                }
            }

            // expect boundary after item
            match self.peek()? {
                Some(ParserToken {
                    ty: ParserTokenType::LayoutSeparator(id),
                    ..
                }) if *id == layout_id => {
                    self.consume_layout_separator(layout_id)?;
                }
                Some(ParserToken {
                    ty: ParserTokenType::LayoutSeparator(id),
                    loc,
                }) => {
                    return Err(ParseError::located_message(
                        format!(
                            "layout block {:?} encountered separator owned by {:?}",
                            layout_id, id
                        ),
                        loc.clone(),
                    ));
                }
                Some(ParserToken {
                    ty: ParserTokenType::LayoutEnd(id),
                    ..
                }) if *id == layout_id => {
                    self.consume_layout_end(layout_id)?;
                    return ParseResult::Ok(items);
                }
                Some(ParserToken {
                    ty: ParserTokenType::LayoutEnd(id),
                    loc,
                }) => {
                    return Err(ParseError::located_message(
                        format!(
                            "layout block {:?} encountered end owned by {:?}",
                            layout_id, id
                        ),
                        loc.clone(),
                    ));
                }
                Some(ParserToken {
                    ty: ParserTokenType::Concrete(token),
                    ..
                }) if layout_feedback.matches(token) => {
                    // handle the case where it is expected that LayoutEnd can occur,
                    // but is not present due to inlining on the same line (which is still valid)
                    self.layout_stream
                        .close_implicit_layout_before_current(layout_id)?;
                    self.consume_layout_end(layout_id)?;
                    return ParseResult::Ok(items);
                }
                _ => {
                    return ParseResult::Err(ParseError::located_message(
                        "expect LayoutEnd / LayoutSeparator / ConcreteToken(layout can close before), but was not found",
                        anchor.clone(),
                    ));
                }
            }
        }
    }
}

impl LayoutItemParser<'_> {
    fn new(parser: &mut Parser, owner: LayoutId) -> LayoutItemParser<'_> {
        // RAII: mutates the underlying parser by pushing a layout owner/id
        parser.item_scope_owners.push(owner);
        LayoutItemParser { parser, owner }
    }

    /// returns the next concrete token, or `None` when this item's owning
    /// LayoutSeparator/LayoutEnd is next
    pub(crate) fn peek_concrete(&mut self) -> ParseResult<Option<ConcreteTokenAndLoc>> {
        match self.parser.peek()? {
            None => Ok(None),
            Some(ParserToken {
                ty: ParserTokenType::Concrete(token),
                loc,
            }) => Ok(Some(ConcreteTokenAndLoc {
                token: token.clone(),
                loc: loc.clone(),
                starts_a_line: false,
            })),
            Some(ParserToken {
                ty: ParserTokenType::LayoutSeparator(id) | ParserTokenType::LayoutEnd(id),
                loc,
            }) if *id == self.owner => Ok(None),
            Some(ParserToken {
                ty: ParserTokenType::LayoutStart(id),
                loc,
            })
            | Some(ParserToken {
                ty: ParserTokenType::LayoutSeparator(id) | ParserTokenType::LayoutEnd(id),
                loc,
            }) => Err(ParseError::located_message(
                format!(
                    "item for layout {:?} encountered layout marker {:?} that is not owned by the current context",
                    self.owner, id
                ),
                loc.clone(),
            )),
        }
    }

    /// get next conrete token that is owned within current context, else error out
    pub(crate) fn next_concrete(&mut self) -> ParseResult<ConcreteTokenAndLoc> {
        self.peek_concrete()?.ok_or_else(|| {
            ParseError::message(
                format!("layout item {:?} reached its boundary", self.owner),
                None,
            )
        })?;

        let token = self
            .parser
            .next()?
            .ok_or_else(|| ParseError::unexpected_eof("concrete item token"))?;
        token.tok_concrete_and_loc().ok_or_else(|| {
            ParseError::message(
                format!(
                    "layout item {:?} encountered a non-concrete token",
                    self.owner
                ),
                None,
            )
        })
    }

    // `next_concrete` plus a check against provided concrete token
    // and return that token
    pub(crate) fn expect_concrete(
        &mut self,
        expected: &ConcreteToken,
    ) -> ParseResult<ConcreteTokenAndLoc> {
        let token = self.next_concrete()?;
        if same_concrete_variant(&token.token, expected) {
            Ok(token)
        } else {
            Err(ParseError::unexpected_token(
                format!("{:?}", expected),
                Some(token),
            ))
        }
    }

    pub(crate) fn at_boundary(&mut self) -> ParseResult<bool> {
        Ok(self.peek_concrete()?.is_none())
    }

    pub(crate) fn parse_nested_layout<T>(
        &mut self,
        anchor: Location,
        allow_empty: bool,
        parser_fn: impl FnMut(&mut LayoutItemParser<'_>) -> ParseResult<T>,
        layout_feedback: LayoutFeedback,
    ) -> ParseResult<Vec<T>> {
        self.parser
            .parse_layout_block(anchor, allow_empty, parser_fn, layout_feedback)
    }
}

impl Drop for LayoutItemParser<'_> {
    fn drop(&mut self) {
        // RAII: mutates the underlying parser by popping layout owner/id
        let popped = self.parser.item_scope_owners.pop();
        debug_assert_eq!(popped, Some(self.owner));
    }
}

impl ConcreteTokenSource for Parser {
    fn peek_concrete(&mut self) -> ParseResult<Option<ConcreteTokenAndLoc>> {
        Parser::peek_concrete(self)
    }

    fn next_concrete(&mut self) -> ParseResult<Option<ConcreteTokenAndLoc>> {
        Parser::next_concrete(self)
    }

    fn expect_concrete(&mut self, expected: &ConcreteToken) -> ParseResult<ConcreteTokenAndLoc> {
        Parser::expect_concrete(self, expected)
    }
}

impl ConcreteTokenSource for LayoutItemParser<'_> {
    fn peek_concrete(&mut self) -> ParseResult<Option<ConcreteTokenAndLoc>> {
        LayoutItemParser::peek_concrete(self)
    }

    fn next_concrete(&mut self) -> ParseResult<Option<ConcreteTokenAndLoc>> {
        if LayoutItemParser::peek_concrete(self)?.is_none() {
            return Ok(None);
        }
        LayoutItemParser::next_concrete(self).map(Some)
    }

    fn expect_concrete(&mut self, expected: &ConcreteToken) -> ParseResult<ConcreteTokenAndLoc> {
        LayoutItemParser::expect_concrete(self, expected)
    }
}

impl LayoutGrammarSource for Parser {
    fn parse_layout_items<T>(
        &mut self,
        anchor: Location,
        allow_empty: bool,
        parser_fn: impl FnMut(&mut LayoutItemParser<'_>) -> ParseResult<T>,
        feedback: LayoutFeedback,
    ) -> ParseResult<Vec<T>> {
        self.parse_layout_block(anchor, allow_empty, parser_fn, feedback)
    }
}

impl LayoutGrammarSource for LayoutItemParser<'_> {
    fn parse_layout_items<T>(
        &mut self,
        anchor: Location,
        allow_empty: bool,
        parser_fn: impl FnMut(&mut LayoutItemParser<'_>) -> ParseResult<T>,
        feedback: LayoutFeedback,
    ) -> ParseResult<Vec<T>> {
        self.parse_nested_layout(anchor, allow_empty, parser_fn, feedback)
    }
}

/// Compare concrete tokens by variant, ignoring payload values.
fn same_concrete_variant(actual: &ConcreteToken, expected: &ConcreteToken) -> bool {
    use std::mem::discriminant;
    discriminant(actual) == discriminant(expected)
}

fn layout_marker_owner(token: &ParserTokenType) -> Option<LayoutId> {
    match token {
        ParserTokenType::LayoutStart(id)
        | ParserTokenType::LayoutSeparator(id)
        | ParserTokenType::LayoutEnd(id) => Some(*id),
        ParserTokenType::Concrete(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::loc::{LexedTokensAndLocs, Location, Span};

    fn location(line: usize, col: usize, width: usize) -> Location {
        let mut loc = Location::dummy();
        let linear = line * 100 + col;
        loc.span_start = Span::new(linear, line, col);
        loc.span_end = Span::new(linear + width, line, col + width);
        loc
    }

    fn token(
        token: ConcreteToken,
        line: usize,
        col: usize,
        starts_a_line: bool,
    ) -> ConcreteTokenAndLoc {
        ConcreteTokenAndLoc {
            token,
            loc: location(line, col, 1),
            starts_a_line,
        }
    }

    fn anchor() -> Location {
        location(0, 0, 1)
    }

    fn parser(tokens: Vec<ConcreteTokenAndLoc>) -> Parser {
        Parser::new(LexedTokensAndLocs(tokens))
    }

    fn parse_identifier(parser: &mut Parser) -> ParseResult<String> {
        let parsed = parser.expect_concrete(&ConcreteToken::Iden(String::new()))?;
        let ConcreteToken::Iden(identifier) = parsed.token else {
            unreachable!("expect_concrete checked the token variant")
        };
        Ok(identifier)
    }

    fn parse_identifier_item(item: &mut LayoutItemParser<'_>) -> ParseResult<String> {
        let parsed = item.expect_concrete(&ConcreteToken::Iden(String::new()))?;
        let ConcreteToken::Iden(identifier) = parsed.token else {
            unreachable!("expect_concrete checked the token variant")
        };
        Ok(identifier)
    }

    fn closes_before_in() -> LayoutFeedback {
        LayoutFeedback::BeforeIn
    }

    fn assert_current_type(parser: &mut Parser, expected: &ParserTokenType) {
        assert_eq!(
            parser.peek().unwrap().map(|token| &token.ty),
            Some(expected)
        );
    }

    #[test]
    fn layout_block_parses_items_separated_by_alignment() {
        let mut parser = parser(vec![
            token(ConcreteToken::Iden("a".into()), 1, 2, true),
            token(ConcreteToken::Iden("b".into()), 2, 2, true),
            token(ConcreteToken::EndOfFile, 3, 0, true),
        ]);

        let items = parser
            .parse_layout_block(anchor(), false, parse_identifier_item, closes_before_in())
            .unwrap();

        assert_eq!(items, vec![String::from("a"), String::from("b")]);
        assert_current_type(
            &mut parser,
            &ParserTokenType::Concrete(ConcreteToken::EndOfFile),
        );
    }

    #[test]
    fn layout_block_can_open_after_peeking_first_item() {
        let mut parser = parser(vec![
            token(ConcreteToken::Iden("a".into()), 0, 0, true),
            token(ConcreteToken::Iden("b".into()), 1, 0, true),
            token(ConcreteToken::EndOfFile, 2, 0, true),
        ]);
        let first_item_location = parser
            .peek()
            .unwrap()
            .expect("expected first item")
            .loc
            .clone();

        let items = parser
            .parse_layout_block(
                first_item_location,
                false,
                parse_identifier_item,
                closes_before_in(),
            )
            .unwrap();

        assert_eq!(items, vec![String::from("a"), String::from("b")]);
        assert_current_type(
            &mut parser,
            &ParserTokenType::Concrete(ConcreteToken::EndOfFile),
        );
    }

    #[test]
    fn layout_block_stops_at_natural_dedent() {
        let mut parser = parser(vec![
            token(ConcreteToken::Iden("a".into()), 1, 2, true),
            token(ConcreteToken::In, 2, 0, true),
        ]);

        let items = parser
            .parse_layout_block(anchor(), false, parse_identifier_item, closes_before_in())
            .unwrap();

        assert_eq!(items, vec![String::from("a")]);
        assert_current_type(&mut parser, &ParserTokenType::Concrete(ConcreteToken::In));
    }

    #[test]
    fn layout_block_allows_immediate_empty_block_at_eof() {
        let mut parser = parser(vec![token(ConcreteToken::EndOfFile, 1, 0, true)]);
        assert_current_type(
            &mut parser,
            &ParserTokenType::Concrete(ConcreteToken::EndOfFile),
        );

        let items = parser
            .parse_layout_block(anchor(), true, parse_identifier_item, closes_before_in())
            .unwrap();

        assert!(items.is_empty());
        assert_current_type(
            &mut parser,
            &ParserTokenType::Concrete(ConcreteToken::EndOfFile),
        );
    }

    #[test]
    fn nonempty_layout_block_reports_anchor_for_immediate_empty_block() {
        let anchor = anchor();
        let mut parser = parser(vec![token(ConcreteToken::EndOfFile, 1, 0, true)]);

        let error = parser
            .parse_layout_block(
                anchor.clone(),
                false,
                parse_identifier_item,
                closes_before_in(),
            )
            .expect_err("nonempty block should reject immediate LayoutEnd");

        let ParseError::LocatedMessage {
            location: error_anchor,
            ..
        } = error
        else {
            panic!("empty block should produce an anchored message")
        };
        assert_eq!(error_anchor, anchor);
        assert_current_type(
            &mut parser,
            &ParserTokenType::Concrete(ConcreteToken::EndOfFile),
        );
    }

    #[test]
    fn layout_block_feedback_closes_before_first_item() {
        let mut parser = parser(vec![token(ConcreteToken::In, 0, 3, false)]);

        let items = parser
            .parse_layout_block(anchor(), true, parse_identifier_item, closes_before_in())
            .unwrap();

        assert!(items.is_empty());
        assert_current_type(&mut parser, &ParserTokenType::Concrete(ConcreteToken::In));
    }

    #[test]
    fn nonempty_layout_block_rejects_feedback_close_before_first_item() {
        let mut parser = parser(vec![token(ConcreteToken::In, 0, 3, false)]);

        let error = parser
            .parse_layout_block(anchor(), false, parse_identifier_item, closes_before_in())
            .expect_err("nonempty block should reject feedback close before an item");

        assert!(matches!(error, ParseError::LocatedMessage { .. }));
        assert_current_type(&mut parser, &ParserTokenType::Concrete(ConcreteToken::In));
    }

    #[test]
    fn layout_block_feedback_closes_after_item_on_same_line() {
        let mut parser = parser(vec![
            token(ConcreteToken::Iden("a".into()), 0, 2, true),
            token(ConcreteToken::In, 0, 4, false),
        ]);

        let items = parser
            .parse_layout_block(anchor(), false, parse_identifier_item, closes_before_in())
            .unwrap();

        assert_eq!(items, vec![String::from("a")]);
        assert_current_type(&mut parser, &ParserTokenType::Concrete(ConcreteToken::In));
    }

    #[test]
    fn layout_block_uses_structural_physical_brace_closure() {
        let mut parser = parser(vec![
            token(ConcreteToken::BraceL, 0, 0, true),
            token(ConcreteToken::Iden("a".into()), 0, 2, false),
            token(ConcreteToken::BraceR, 0, 4, false),
            token(ConcreteToken::EndOfFile, 0, 5, false),
        ]);
        parser.expect_concrete(&ConcreteToken::BraceL).unwrap();

        let items = parser
            .parse_layout_block(anchor(), false, parse_identifier_item, LayoutFeedback::None)
            .unwrap();

        assert_eq!(items, vec![String::from("a")]);
        assert_current_type(
            &mut parser,
            &ParserTokenType::Concrete(ConcreteToken::BraceR),
        );
        parser.expect_concrete(&ConcreteToken::BraceR).unwrap();
        assert_current_type(
            &mut parser,
            &ParserTokenType::Concrete(ConcreteToken::EndOfFile),
        );
    }

    #[test]
    fn layout_block_feedback_closes_after_separator() {
        let mut parser = parser(vec![
            token(ConcreteToken::Iden("a".into()), 1, 2, true),
            token(ConcreteToken::In, 2, 2, true),
        ]);

        let items = parser
            .parse_layout_block(anchor(), false, parse_identifier_item, closes_before_in())
            .unwrap();

        assert_eq!(items, vec![String::from("a")]);
        assert_current_type(&mut parser, &ParserTokenType::Concrete(ConcreteToken::In));
    }

    #[test]
    fn rejected_inner_block_leaves_enclosing_separator_for_its_owner() {
        let tokens = vec![
            token(ConcreteToken::Iden("outer".into()), 1, 2, true),
            token(ConcreteToken::Iden("next".into()), 2, 2, true),
        ];
        let mut parser = parser(tokens);
        let outer_id = parser
            .layout_stream
            .open_implicit_layout(&anchor())
            .unwrap();
        parser.consume_layout_start(outer_id).unwrap();
        parse_identifier(&mut parser).unwrap();

        let items = parser
            .parse_layout_block(
                token(ConcreteToken::Of, 1, 7, false).loc,
                true,
                parse_identifier_item,
                closes_before_in(),
            )
            .unwrap();

        assert!(items.is_empty());
        assert_current_type(&mut parser, &ParserTokenType::LayoutSeparator(outer_id));
    }

    #[test]
    fn layout_block_does_not_open_across_peeked_separator() {
        let tokens = vec![
            token(ConcreteToken::Iden("a".into()), 1, 2, true),
            token(ConcreteToken::Iden("b".into()), 2, 2, true),
        ];
        let mut parser = parser(tokens);
        let outer_id = parser
            .layout_stream
            .open_implicit_layout(&anchor())
            .unwrap();
        parser.consume_layout_start(outer_id).unwrap();
        parse_identifier(&mut parser).unwrap();
        assert_current_type(&mut parser, &ParserTokenType::LayoutSeparator(outer_id));

        let error = parser
            .parse_layout_block(
                token(ConcreteToken::Of, 1, 7, false).loc,
                true,
                parse_identifier_item,
                closes_before_in(),
            )
            .expect_err("a pending separator belongs to the enclosing layout block");

        assert!(matches!(error, ParseError::Message { .. }));
        assert_current_type(&mut parser, &ParserTokenType::LayoutSeparator(outer_id));
    }

    #[test]
    fn nested_layout_block_returns_control_at_enclosing_separator() {
        let mut parser = parser(vec![
            token(ConcreteToken::Iden("outer".into()), 1, 2, true),
            token(ConcreteToken::Iden("inner_a".into()), 2, 4, true),
            token(ConcreteToken::Iden("inner_b".into()), 3, 4, true),
            token(ConcreteToken::Iden("next_outer".into()), 4, 2, true),
            token(ConcreteToken::EndOfFile, 5, 0, true),
        ]);

        let items = parser
            .parse_layout_block(
                anchor(),
                false,
                |item| {
                    let item_anchor = item.expect_concrete(&ConcreteToken::Iden(String::new()))?;
                    let ConcreteToken::Iden(identifier) = item_anchor.token.clone() else {
                        unreachable!("expect_concrete checked the token variant")
                    };
                    let nested = if identifier == "outer" {
                        assert!(matches!(
                            item.peek_concrete()?.map(|token| token.token),
                            Some(ConcreteToken::Iden(_))
                        ));
                        item.parse_nested_layout(
                            item_anchor.loc.clone(),
                            false,
                            parse_identifier_item,
                            closes_before_in(),
                        )?
                    } else {
                        vec![]
                    };
                    Ok((identifier, nested))
                },
                closes_before_in(),
            )
            .unwrap();

        assert_eq!(
            items,
            vec![
                (
                    String::from("outer"),
                    vec![String::from("inner_a"), String::from("inner_b")],
                ),
                (String::from("next_outer"), vec![]),
            ]
        );
        assert_current_type(
            &mut parser,
            &ParserTokenType::Concrete(ConcreteToken::EndOfFile),
        );
    }

    #[test]
    fn layout_block_propagates_item_parser_error() {
        let mut parser = parser(vec![token(ConcreteToken::Let, 1, 2, true)]);

        let error = parser
            .parse_layout_block(anchor(), false, parse_identifier_item, closes_before_in())
            .expect_err("item parser error should propagate");

        assert!(matches!(error, ParseError::UnexpectedToken { .. }));
    }

    #[test]
    fn layout_block_rejects_item_parser_that_does_not_consume_input() {
        let mut parser = parser(vec![token(ConcreteToken::Iden("a".into()), 1, 2, true)]);

        let error = parser
            .parse_layout_block(anchor(), false, |_| Ok(()), closes_before_in())
            .expect_err("each successful item parser must advance to a boundary");

        assert!(matches!(error, ParseError::LocatedMessage { .. }));
        assert_current_type(
            &mut parser,
            &ParserTokenType::Concrete(ConcreteToken::Iden("a".into())),
        );
    }

    #[test]
    fn item_callback_cannot_consume_parent_boundary() {
        let mut parser = parser(vec![
            token(ConcreteToken::Iden("a".into()), 1, 2, true),
            token(ConcreteToken::Iden("b".into()), 2, 2, true),
        ]);

        let error = parser
            .parse_layout_block(
                anchor(),
                false,
                |item| {
                    item.next_concrete()?;
                    item.parser
                        .next()?
                        .ok_or_else(|| ParseError::unexpected_eof("parent boundary"))?;
                    Ok(())
                },
                LayoutFeedback::None,
            )
            .expect_err("callback must not consume its parent separator");

        let ParseError::LocatedMessage { message, .. } = error else {
            panic!("parent-boundary consumption should produce a located error")
        };
        assert!(message.contains("parent layout marker"));
    }

    #[test]
    fn cloned_parser_has_independent_layout_context_buffer_and_position() {
        let mut original = parser(vec![
            token(ConcreteToken::Iden("a".into()), 0, 2, true),
            token(ConcreteToken::In, 0, 4, false),
        ]);
        let layout_id = original
            .layout_stream
            .open_implicit_layout(&anchor())
            .unwrap();
        original.consume_layout_start(layout_id).unwrap();
        parse_identifier(&mut original).unwrap();
        original.peek().unwrap();
        let mut cloned = original.clone();

        original
            .layout_stream
            .close_implicit_layout_before_current(layout_id)
            .unwrap();
        original.consume_layout_end(layout_id).unwrap();
        assert_current_type(&mut original, &ParserTokenType::Concrete(ConcreteToken::In));

        assert_current_type(&mut cloned, &ParserTokenType::Concrete(ConcreteToken::In));
        cloned
            .layout_stream
            .close_implicit_layout_before_current(layout_id)
            .expect("original feedback close must not mutate cloned layout context");
        cloned.consume_layout_end(layout_id).unwrap();
        assert_current_type(&mut cloned, &ParserTokenType::Concrete(ConcreteToken::In));
    }
}
