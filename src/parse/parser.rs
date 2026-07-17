use std::fmt;
use std::iter::Peekable;

use crate::parse::concrete_token::ConcreteToken;
use crate::parse::loc::ConcreteTokenAndLoc;

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
        }
    }
}

impl std::error::Error for ParseError {}

/// lightweight parser wrapper that exposes combinator-friendly helpers
#[derive(Clone)]
pub(crate) struct Parser<'a, I>
where
    I: Iterator<Item = &'a ConcreteTokenAndLoc> + Clone,
{
    tokens: Peekable<I>,
}

impl<'a, I> Parser<'a, I>
where
    I: Iterator<Item = &'a ConcreteTokenAndLoc> + Clone,
{
    pub(crate) fn new(iter: I) -> Self {
        Self {
            tokens: iter.peekable(),
        }
    }

    pub(crate) fn peek(&mut self) -> Option<&'a ConcreteTokenAndLoc> {
        self.tokens.peek().copied()
    }

    pub(crate) fn next(&mut self) -> Option<&'a ConcreteTokenAndLoc> {
        self.tokens.next()
    }

    pub(crate) fn fork(&self) -> Self {
        Self {
            tokens: self.tokens.clone(),
        }
    }

    pub(crate) fn expect(&mut self, expected: &ConcreteToken) -> ConcreteTokenAndLoc {
        self.expect_result(expected)
            .unwrap_or_else(|err| panic!("{err}"))
    }

    pub(crate) fn expect_result(
        &mut self,
        expected: &ConcreteToken,
    ) -> ParseResult<ConcreteTokenAndLoc> {
        TokenStreamExt::expect_token_result(self, expected)
    }

    pub(crate) fn satisfy<F>(&mut self, mut predicate: F) -> Option<ConcreteTokenAndLoc>
    where
        F: FnMut(&ConcreteToken) -> bool,
    {
        if let Some(tok) = self.peek() {
            if predicate(&tok.token) {
                return self.next().cloned();
            }
        }
        None
    }

    pub(crate) fn consume_trivial(&mut self) {
        TokenStreamExt::consume_trivial(self);
    }

    pub(crate) fn skip_spaces(&mut self) {
        TokenStreamExt::skip_spaces(self);
    }

    pub(crate) fn collect_balanced_until<F>(
        &mut self,
        should_stop: F,
    ) -> (Vec<ConcreteTokenAndLoc>, Option<ConcreteTokenAndLoc>)
    where
        F: FnMut(&ConcreteTokenAndLoc, &DelimiterTracker) -> bool,
    {
        self.collect_balanced_until_result(should_stop)
            .unwrap_or_else(|err| panic!("{err}"))
    }

    pub(crate) fn collect_balanced_until_result<F>(
        &mut self,
        should_stop: F,
    ) -> ParseResult<(Vec<ConcreteTokenAndLoc>, Option<ConcreteTokenAndLoc>)>
    where
        F: FnMut(&ConcreteTokenAndLoc, &DelimiterTracker) -> bool,
    {
        TokenStreamExt::collect_balanced_until_result(self, should_stop)
    }

    pub(crate) fn collect_until_token(
        &mut self,
        token: &ConcreteToken,
    ) -> Vec<ConcreteTokenAndLoc> {
        self.collect_until_token_result(token)
            .unwrap_or_else(|err| panic!("{err}"))
    }

    pub(crate) fn collect_until_token_result(
        &mut self,
        token: &ConcreteToken,
    ) -> ParseResult<Vec<ConcreteTokenAndLoc>> {
        TokenStreamExt::collect_until_token_result(self, token)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct DelimiterTracker {
    brace: usize,
    paren: usize,
    bracket: usize,
}

impl DelimiterTracker {
    pub(crate) fn observe_token(&mut self, token: &ConcreteTokenAndLoc) -> ParseResult<()> {
        match &token.token {
            ConcreteToken::BraceL => self.brace += 1,
            ConcreteToken::BraceR => {
                if self.brace == 0 {
                    return Err(ParseError::delimiter("}", token.clone()));
                }
                self.brace -= 1;
            }
            ConcreteToken::ParenL => self.paren += 1,
            ConcreteToken::ParenR => {
                if self.paren == 0 {
                    return Err(ParseError::delimiter(")", token.clone()));
                }
                self.paren -= 1;
            }
            ConcreteToken::BracketL => self.bracket += 1,
            ConcreteToken::BracketR => {
                if self.bracket == 0 {
                    return Err(ParseError::delimiter("]", token.clone()));
                }
                self.bracket -= 1;
            }
            _ => {}
        }
        Ok(())
    }

    pub(crate) fn at_top(&self) -> bool {
        self.brace == 0 && self.paren == 0 && self.bracket == 0
    }
}

/// compare tokens by variant, ignoring payload
pub(crate) fn matches_token(actual: &ConcreteToken, expected: &ConcreteToken) -> bool {
    use std::mem::discriminant;
    match (actual, expected) {
        (ConcreteToken::Iden(_), ConcreteToken::Iden(_)) => true,
        (ConcreteToken::LiteralNumeric(_), ConcreteToken::LiteralNumeric(_)) => true,
        (ConcreteToken::LiteralString(_), ConcreteToken::LiteralString(_)) => true,
        (ConcreteToken::Space(_), ConcreteToken::Space(_)) => true,
        (ConcreteToken::Comment(_), ConcreteToken::Comment(_)) => true,
        (a, b) if discriminant(a) == discriminant(b) => true,
        _ => false,
    }
}

pub(crate) trait TokenStreamExt<'a> {
    fn peek_token(&mut self) -> Option<&'a ConcreteTokenAndLoc>;
    fn next_token(&mut self) -> Option<ConcreteTokenAndLoc>;

    fn expect_token_result(&mut self, expected: &ConcreteToken) -> ParseResult<ConcreteTokenAndLoc>
    where
        Self: Sized,
    {
        match self.next_token() {
            Some(token) if matches_token(&token.token, expected) => Ok(token),
            Some(token) => Err(ParseError::unexpected_token(
                format!("{:?}", expected),
                Some(token),
            )),
            None => Err(ParseError::unexpected_eof("token")),
        }
    }

    fn consume_while<F>(&mut self, mut predicate: F)
    where
        F: FnMut(&ConcreteToken) -> bool,
        Self: Sized,
    {
        while let Some(next) = self.peek_token() {
            if predicate(&next.token) {
                self.next_token();
            } else {
                break;
            }
        }
    }

    fn consume_trivial(&mut self)
    where
        Self: Sized,
    {
        self.consume_while(|token| {
            matches!(
                token,
                ConcreteToken::LineDelimiter
                    | ConcreteToken::EndOfFile
                    | ConcreteToken::Space(_)
                    | ConcreteToken::CommentSlashes
                    | ConcreteToken::Comment(_)
            )
        });
    }

    fn skip_spaces(&mut self)
    where
        Self: Sized,
    {
        self.consume_while(|token| matches!(token, ConcreteToken::Space(_)));
    }

    fn peek_non_trivial(&mut self) -> Option<&'a ConcreteTokenAndLoc>
    where
        Self: Sized,
    {
        self.consume_trivial();
        self.peek_token()
    }

    fn next_non_trivial(&mut self) -> Option<ConcreteTokenAndLoc>
    where
        Self: Sized,
    {
        self.consume_trivial();
        self.next_token()
    }

    fn collect_balanced_until<F>(
        &mut self,
        mut should_stop: F,
    ) -> (Vec<ConcreteTokenAndLoc>, Option<ConcreteTokenAndLoc>)
    where
        F: FnMut(&ConcreteTokenAndLoc, &DelimiterTracker) -> bool,
        Self: Sized,
    {
        self.collect_balanced_until_result(should_stop)
            .unwrap_or_else(|err| panic!("{err}"))
    }

    fn collect_balanced_until_result<F>(
        &mut self,
        mut should_stop: F,
    ) -> ParseResult<(Vec<ConcreteTokenAndLoc>, Option<ConcreteTokenAndLoc>)>
    where
        F: FnMut(&ConcreteTokenAndLoc, &DelimiterTracker) -> bool,
        Self: Sized,
    {
        let mut collected = Vec::new();
        let mut balance = DelimiterTracker::default();

        loop {
            let Some(next) = self.peek_token().cloned() else {
                return Ok((collected, None));
            };

            if should_stop(&next, &balance) {
                let stop = self.next_token();
                return Ok((collected, stop));
            }

            let token = self
                .next_token()
                .ok_or_else(|| ParseError::unexpected_eof("collect_balanced_until"))?;
            if matches!(
                token.token,
                ConcreteToken::Space(_) | ConcreteToken::LineDelimiter | ConcreteToken::EndOfFile
            ) {
                continue;
            }

            balance.observe_token(&token)?;
            collected.push(token);
        }
    }

    fn collect_until_token(&mut self, token: &ConcreteToken) -> Vec<ConcreteTokenAndLoc>
    where
        Self: Sized,
    {
        self.collect_until_token_result(token)
            .unwrap_or_else(|err| panic!("{err}"))
    }

    fn collect_until_token_result(
        &mut self,
        token: &ConcreteToken,
    ) -> ParseResult<Vec<ConcreteTokenAndLoc>>
    where
        Self: Sized,
    {
        let (tokens, _) = self.collect_balanced_until_result(|next, balance| {
            balance.at_top() && matches_token(&next.token, token)
        })?;
        Ok(tokens)
    }
}

pub(crate) trait ForkableTokenStream<'a>: TokenStreamExt<'a> + Clone {
    fn fork(&self) -> Self;
}

pub(crate) fn collect_layout_items_result<'a, S, F, T>(
    stream: &mut S,
    base_indent: usize,
    break_on_equal: bool,
    mut parse_item: F,
) -> ParseResult<Vec<T>>
where
    S: TokenStreamExt<'a>,
    F: FnMut(&mut S) -> ParseResult<Option<T>>,
{
    let mut items = Vec::new();
    let mut started = false;

    loop {
        stream.consume_trivial();
        let Some(peeked) = stream.peek_token().cloned() else {
            return Ok(items);
        };

        let is_separator = matches!(peeked.token, ConcreteToken::VertBar);
        let column = peeked.loc.span_start.col;
        let should_break = if break_on_equal {
            column <= base_indent
        } else {
            column < base_indent
        };

        if started && !is_separator && should_break {
            return Ok(items);
        }

        match parse_item(stream)? {
            Some(item) => {
                started = true;
                items.push(item);
            }
            None => return Ok(items),
        }
    }
}

impl<'a, I> TokenStreamExt<'a> for Parser<'a, I>
where
    I: Iterator<Item = &'a ConcreteTokenAndLoc> + Clone,
{
    fn peek_token(&mut self) -> Option<&'a ConcreteTokenAndLoc> {
        self.tokens.peek().copied()
    }

    fn next_token(&mut self) -> Option<ConcreteTokenAndLoc> {
        self.tokens.next().cloned()
    }
}

impl<'a, I> ForkableTokenStream<'a> for Parser<'a, I>
where
    I: Iterator<Item = &'a ConcreteTokenAndLoc> + Clone,
{
    fn fork(&self) -> Self {
        self.clone()
    }
}

/// tokens that always terminate a layout-driven block (e.g. case clauses, let defs)
pub(crate) fn is_layout_block_terminator(token: &ConcreteToken) -> bool {
    matches!(
        token,
        ConcreteToken::ParenR
            | ConcreteToken::BraceR
            | ConcreteToken::BracketR
            | ConcreteToken::EndOfFile
    )
}

/// convenience helper for matching layout keywords without caring about payload
pub(crate) fn is_keyword_in(token: &ConcreteToken) -> bool {
    matches!(token, ConcreteToken::In)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::loc::Location;

    fn make_token(token: ConcreteToken) -> ConcreteTokenAndLoc {
        ConcreteTokenAndLoc {
            token,
            loc: Location::dummy(),
        }
    }

    #[test]
    fn peek_non_trivial_skips_trivia() {
        let tokens = vec![
            make_token(ConcreteToken::Space(1)),
            make_token(ConcreteToken::LineDelimiter),
            make_token(ConcreteToken::CommentSlashes),
            make_token(ConcreteToken::Comment(" foo".to_string())),
            make_token(ConcreteToken::Iden("value".to_string())),
        ];

        let mut parser = Parser::new(tokens.iter());
        let peeked = TokenStreamExt::peek_non_trivial(&mut parser)
            .expect("expected to find non-trivial token");
        assert!(matches!(peeked.token, ConcreteToken::Iden(_)));
    }

    #[test]
    fn next_non_trivial_yields_and_advances() {
        let tokens = vec![
            make_token(ConcreteToken::Space(1)),
            make_token(ConcreteToken::Iden("value".to_string())),
            make_token(ConcreteToken::Space(2)),
            make_token(ConcreteToken::Iden("next".to_string())),
        ];

        let mut parser = Parser::new(tokens.iter());
        let first = TokenStreamExt::next_non_trivial(&mut parser)
            .expect("expected to consume non-trivial token");
        assert!(matches!(first.token, ConcreteToken::Iden(_)));

        let second = TokenStreamExt::next_non_trivial(&mut parser)
            .expect("expected to consume second non-trivial token");
        match second.token {
            ConcreteToken::Iden(name) => assert_eq!(name, "next"),
            other => panic!("expected identifier, got {:?}", other),
        }
    }
}
