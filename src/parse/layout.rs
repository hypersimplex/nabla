use std::collections::*;

use crate::parse::concrete_token::ConcreteToken;
use crate::parse::loc::ConcreteTokenAndLoc;

use super::concrete_token::*;
use super::cur::*;
use super::loc::*;
use super::parser::{ParseError, ParseResult};

/// identifier for an implicit layout block
///
/// used in various parser helpers to check that a virtual marker belongs to the
/// block the parser helpers is processing/manipulating
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct LayoutId(u64);

/// construct used to represent layout indent/dedent
#[derive(Clone, Debug)]
pub(crate) enum LayoutCxt {
    // implicitly activated from an anchor/opener token
    ImplicitStart {
        id: LayoutId,
        column: usize,
        loc_anchor: Location,
    },

    // activated from an explicit token
    ExplicitStart {
        loc_start: Location,
    },
}

/// aggregate of token types:
/// - concrete token, and
/// - virtual tokens that are additionally injected by the layout abstraction
///   during parsing
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ParserTokenType {
    // regular token
    Concrete(ConcreteToken),

    // starts a new layout block/level
    LayoutStart(LayoutId),

    // acts as a delimiter that continues at the same layout block/level
    LayoutSeparator(LayoutId),

    // ends current layout block/level
    LayoutEnd(LayoutId),
}

#[derive(Clone, Debug)]
pub(crate) struct ParserToken {
    pub ty: ParserTokenType,
    pub loc: Location,
}

// some helper functions
impl ParserToken {
    pub(crate) fn is_concrete(&self) -> bool {
        match &self.ty {
            ParserTokenType::Concrete(_) => true,
            _ => false,
        }
    }
    pub(crate) fn is_layout(&self) -> bool {
        !self.is_concrete()
    }
    pub(crate) fn tok_concrete_and_loc(&self) -> Option<ConcreteTokenAndLoc> {
        match &self.ty {
            ParserTokenType::Concrete(x) => Some(ConcreteTokenAndLoc {
                token: x.clone(),
                loc: self.loc.clone(),
                starts_a_line: false, // factor this field out
            }),
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct LayoutStream {
    // concrete tokens
    inputs: LexedTokensAndLocs,

    // position of the next concrete token to be processed
    lex_pos: usize,

    // keep track of layout context
    layout_stack: Vec<LayoutCxt>,

    // not yet consumed by parser; virtual tokens are inserted
    in_progress: VecDeque<ParserToken>,

    // next owner ID assigned to an implicit layout request
    next_layout_id: u64,

    // state related to the token at inputs[lex_pos]
    // to prevent applying layout marker rule more than once to a same
    // input token, in the case the input token is not advanced and we will
    // try process it again later; a layout marker just means some encoding
    // for the position of the token
    // this is not for a token already stored in the queue `in_progress`
    next_input_layout_marker_processed: bool,
}

impl LayoutStream {
    pub(crate) fn new(input_tokens: LexedTokensAndLocs) -> Self {
        Self {
            inputs: input_tokens,
            lex_pos: 0,
            layout_stack: Vec::default(),
            in_progress: VecDeque::new(),
            next_layout_id: 0,
            next_input_layout_marker_processed: false,
        }
    }
    /// peek the next parse token (this includes virtual token)
    pub(crate) fn peek(&mut self) -> ParseResult<Option<&ParserToken>> {
        self.populate_in_progress_queue()?;
        Ok(self.in_progress.front())
    }
    /// get the next parse token (this includes virtual token)
    ///
    /// internally:
    /// - this may start a new layout block if we encounter explicit layout
    //    start token
    /// - we may also remove existing layout block if we encounter explicit
    ///   layout end token
    pub(crate) fn next(&mut self) -> ParseResult<Option<ParserToken>> {
        self.populate_in_progress_queue()?;
        match self.in_progress.front() {
            None => {
                return Ok(None);
            }
            Some(ParserToken {
                ty: ParserTokenType::Concrete(ConcreteToken::BraceL),
                loc,
            }) => {
                // explict delimiter for starting a new layout block
                self.layout_stack.push(LayoutCxt::ExplicitStart {
                    loc_start: loc.clone(),
                });
                Ok(self.in_progress.pop_front())
            }
            Some(
                parser_tok @ ParserToken {
                    ty: ParserTokenType::Concrete(ConcreteToken::BraceR),
                    loc,
                },
            ) => {
                // encountered an explict delimiter for ending a layout block
                //
                // check that the last item on the layout stack is an explicit start
                //
                // remove existing layout block
                match self.layout_stack.last() {
                    Some(LayoutCxt::ExplicitStart { .. }) => {} // expect to get this
                    _ => {
                        return Err(ParseError::message(
                            "expect matching explicit layout start",
                            parser_tok.tok_concrete_and_loc(),
                        ));
                    }
                }
                self.layout_stack.pop();
                Ok(self.in_progress.pop_front())
            }
            Some(_) => Ok(self.in_progress.pop_front()),
        }
    }

    fn allocate_layout_id(&mut self) -> ParseResult<LayoutId> {
        let id = LayoutId(self.next_layout_id);
        self.next_layout_id = self.next_layout_id.checked_add(1).ok_or_else(|| {
            ParseError::message(
                "exhausted implicit layout identifiers.. really on a u64 type? :(",
                None,
            )
        })?;
        Ok(id)
    }

    /// helper to try populate `in_progress` queue
    ///
    /// this may insert virtual tokens due to layout delimiters signaling
    /// start of a new layout block
    ///
    /// this may end implicit layout blocks by popping layout blocks from the layout stack
    ///
    /// if there is nothing to populate, this method also succeeds
    fn populate_in_progress_queue(&mut self) -> ParseResult<()> {
        // no-op conditions:
        if !self.in_progress.is_empty() {
            // there already exists an item in the queue
            return Ok(());
        }
        if self.lex_pos >= self.inputs.0.len() {
            // reached end of inputs, so nothing to populate
            return Ok(());
        }

        // a physical right brace closes implicit contexts nested inside the
        // matching explicit context before the brace itself is exposed
        //
        // this is structural layout behavior
        if let Some(ConcreteTokenAndLoc {
            token: ConcreteToken::BraceR,
            loc,
            ..
        }) = self.inputs.0.get(self.lex_pos)
        {
            let location = loc.to_zero_width_start();
            let mut closed_implicit = false;
            while let Some(LayoutCxt::ImplicitStart { id, .. }) = self.layout_stack.last() {
                let layout_id = *id;
                self.layout_stack.pop();
                self.in_progress.push_back(ParserToken {
                    ty: ParserTokenType::LayoutEnd(layout_id),
                    loc: location.clone(),
                });
                closed_implicit = true;
            }
            if closed_implicit {
                return Ok(());
            }
        }

        match self.inputs.0.get(self.lex_pos).unwrap() {
            ConcreteTokenAndLoc {
                token: ConcreteToken::EndOfFile,
                loc,
                ..
            } => {
                // we reached end of file, so pop an item off layout stack
                //
                // also enqueue a virtual layout token signaling end of a layout block
                //
                // early return without consuming input token
                match self.layout_stack.last() {
                    None => {} // nothing to pop
                    Some(LayoutCxt::ExplicitStart { loc_start }) => {
                        return Err(ParseError::message(
                            format!(
                                "no matching explicit layout end found for explicit layout start at {:?}",
                                loc_start,
                            ),
                            None,
                        ));
                    }
                    Some(LayoutCxt::ImplicitStart { id, .. }) => {
                        let layout_id = *id;
                        self.layout_stack.pop();
                        let mut loc = loc.clone();
                        loc.span_end = loc.span_start; // make it zero-width
                        self.in_progress.push_back(ParserToken {
                            ty: ParserTokenType::LayoutEnd(layout_id),
                            loc,
                        });
                        return Ok(());
                    }
                }
            }
            // handling for the case of a concrete input token that
            // starts a line
            //
            // may or may not early return without consuming input token
            // depending on whether the line start rule enqueues a virtual token or not
            ConcreteTokenAndLoc {
                loc,
                starts_a_line: true,
                ..
            } if !self.next_input_layout_marker_processed => {
                // avoid double processing in the case that input token is not consumed
                // and we will look at it again next time
                self.next_input_layout_marker_processed = true;
                self.apply_layout_marker(loc.span_start.col, &loc.clone());
                if !self.in_progress.is_empty() {
                    // early return
                    return Ok(());
                }
                // fall through
            }
            _ => {} // fall through
        }

        // at this point, we will consume an input token ---

        // doing this again due to the borrow checker :(
        let ConcreteTokenAndLoc { token, loc, .. } =
            self.inputs.0.get(self.lex_pos).unwrap().clone();
        self.in_progress.push_back(ParserToken {
            ty: ParserTokenType::Concrete(token),
            loc: loc,
        });
        self.lex_pos += 1;
        // reset when input is consumed and advanced
        self.next_input_layout_marker_processed = false;
        Ok(())
    }

    /// apply layout marker rule against the active contexts, potentially:
    /// - inserting LayoutSeparator into queue, and/or
    /// - removing layout block from the layout stack while inserting LayoutEnd
    ///   into the queue
    /// - closing implicit contexts nested inside an explicit `{ ... }` before
    ///   exposing its physical `}`
    ///
    /// conceptually the layout marker is (indentation column, location) which
    /// are the parameters to this function
    ///
    /// when this is called:
    /// - concrete token beginning a new line
    /// - token for a newly requested implicit block (if that block fails to open, then
    ///   the marker is applied to the enclosing block
    ///
    /// this does:
    /// loop {
    ///   one of:
    ///   - [0] continues current item of the implicit layout block by
    ///         terminating with no-op, or
    ///   - [1] starts a new item of the current implicit layout block by
    ///         inserting LayoutSeparator and terminate, or
    ///   - [2] ends current implicit layout block by inserting LayoutEnd and
    ///         popping the layout stack and recurse, or
    ///   - [3] terminate if an explicit layout block or None is reached
    /// }
    fn apply_layout_marker(&mut self, current_column: usize, location: &Location) {
        let location = location.to_zero_width_start();
        loop {
            match self.layout_stack.last() {
                Some(LayoutCxt::ImplicitStart {
                    id,
                    column: layout_column,
                    ..
                }) => {
                    let layout_id = *id;
                    let layout_column = *layout_column;
                    // case [0]: current line continues the current layout item
                    if current_column > layout_column {
                        return;
                    }
                    // case [1]: current line begins another item in the layout block
                    if current_column == layout_column {
                        self.in_progress.push_back(ParserToken {
                            ty: ParserTokenType::LayoutSeparator(layout_id),
                            loc: location,
                        });
                        return;
                    }
                    // case [2]: current line is outside this layout block (current_column < layout_column)
                    //
                    // continue the loop because one line start may close multiple nested blocks
                    self.layout_stack.pop();
                    self.in_progress.push_back(ParserToken {
                        ty: ParserTokenType::LayoutEnd(layout_id),
                        loc: location.clone(),
                    });
                }
                // case [3]:
                //
                // newline has no layout effect inside physical braces (ExplicitStart)
                //
                // also nothing to update when no enclosing context remains (None)
                Some(LayoutCxt::ExplicitStart { .. }) | None => return,
            }
        }
    }

    /// request to start a layout block and don't consume input
    ///
    /// `anchor` is the location that triggers the new layout to be requested
    ///
    /// buffered token remains unconsumed
    ///
    /// for a non-empty layout block:
    ///   buffered token is placed after `LayoutStart`
    /// for an empty block:
    ///   buffered token is placed after `LayoutStart` and `LayoutEnd`
    ///
    /// returns the owner ID attached to every virtual marker for this opening
    pub(super) fn open_implicit_layout(&mut self, anchor: &Location) -> ParseResult<LayoutId> {
        // get concrete token buffered
        let buffered_current = match self.in_progress.len() {
            0 => None,
            1 if self
                .in_progress
                .front()
                .is_some_and(ParserToken::is_concrete) =>
            {
                // Cache without mutating so fallible validation and ID
                // allocation leave the queue unchanged on error.
                self.in_progress.front().cloned()
            }
            _ => {
                return Err(ParseError::message(
                    "invariant: implicit layout cannot be opened before pending layout tokens",
                    None,
                ));
            }
        };
        // sanity checks
        let (loc, is_eof) = match &buffered_current {
            Some(ParserToken {
                ty: ParserTokenType::Concrete(token),
                loc,
            }) => (
                loc.to_zero_width_start(),
                matches!(token, ConcreteToken::EndOfFile),
            ),
            Some(_) => unreachable!("buffered_current was checked to be concrete"),
            None => match self.inputs.0.get(self.lex_pos) {
                Some(item) => (
                    item.loc.to_zero_width_start(),
                    matches!(item.token, ConcreteToken::EndOfFile),
                ),
                None => {
                    return Err(ParseError::message("next input cannot be empty", None));
                }
            },
        };

        let layout_id = self.allocate_layout_id()?;
        // no error encountered earlier, so commit pop
        if buffered_current.is_some() {
            self.in_progress.pop_front();
        }

        // unconditionally enqueue LayoutStart
        self.in_progress.push_back(ParserToken {
            ty: ParserTokenType::LayoutStart(layout_id),
            loc: loc.clone(),
        });

        if is_eof {
            // early exit
            //
            // enqueue virtual layoutEnd token before the original buffered token
            //
            // since enqueue [LayoutStart, LayoutEnd] then no new layout block needs
            // to be created on the layout stack
            self.in_progress.push_back(ParserToken {
                ty: ParserTokenType::LayoutEnd(layout_id),
                loc: loc.clone(),
            });
            // enqueue again the buffered token
            if let Some(buffered_current) = buffered_current {
                self.in_progress.push_back(buffered_current);
            }
            return Ok(layout_id);
        }

        // next token is not end of file ---
        let col = loc.span_start.col;

        let can_open = match self.layout_stack.last() {
            None => true,
            Some(LayoutCxt::ExplicitStart { .. }) => true,
            Some(LayoutCxt::ImplicitStart { column, .. }) if col > *column => true,
            _ => false,
        };
        if can_open {
            // create a new layout block on the layout stack
            self.layout_stack.push(LayoutCxt::ImplicitStart {
                id: layout_id,
                column: col,
                loc_anchor: anchor.clone(),
            });
        } else {
            // reject opening with an empty layout block
            self.in_progress.push_back(ParserToken {
                ty: ParserTokenType::LayoutEnd(layout_id),
                loc: loc.clone(),
            });
            // after rejecting with an empty layout block, unchanged enclosing
            // contexts so we apply marker/indentation position to current
            // layout stack and emit any necessary layout tokens
            self.apply_layout_marker(col, &loc);
        }

        if let Some(buffered_current) = buffered_current {
            // enqueue again the buffered token
            self.in_progress.push_back(buffered_current);
        } else {
            // token used to open the layout is still at inputs[lex_pos]
            // so the token will be inspected again
            //
            // avoid double processing for the current token which is not
            // consumed, but we have applied indentation rules for it by
            // either:
            // - opening a layout block, or
            // - rejecting opening a new block and the token's marker is applied
            //   to enclosing contexts
            // so the next time this token is processed, we won't introduce
            // an incorrect extra LayoutSeparator due to the token starting
            // at the same column as the existing layout
            self.next_input_layout_marker_processed = true;
        }
        Ok(layout_id)
    }

    /// closes `expected_id` before the buffered concrete token
    ///
    /// does checks to ensure that we do not pop a different implicit owner or
    //  an explicit context
    pub(super) fn close_implicit_layout_before_current(
        &mut self,
        expected_id: LayoutId,
    ) -> ParseResult<()> {
        let current_token_and_loc = match self.peek()? {
            None => {
                return Err(ParseError::message("expect input token", None));
            }
            Some(x) => match x.tok_concrete_and_loc() {
                Some(x) => x,
                None => {
                    return Err(ParseError::message(
                        "expect input token to not be a layout token",
                        None,
                    ));
                }
            },
        };
        // check layout stack for layout block to pop
        match self.layout_stack.last() {
            Some(LayoutCxt::ExplicitStart { .. }) => {
                return Err(ParseError::message(
                    "expect to close an implicit layout but encountered an explicit layout",
                    Some(current_token_and_loc),
                ));
            }
            Some(LayoutCxt::ImplicitStart { id, .. }) if *id == expected_id => {}
            Some(LayoutCxt::ImplicitStart { id, .. }) => {
                return Err(ParseError::message(
                    format!(
                        "expect to close implicit layout {:?}, but top implicit layout is {:?}",
                        expected_id, id
                    ),
                    Some(current_token_and_loc),
                ));
            }
            _ => {
                return Err(ParseError::message(
                    format!(
                        "expect implicit layout {:?} on top of the layout stack",
                        expected_id
                    ),
                    Some(current_token_and_loc),
                ));
            }
        }
        // pop 1 implicit context
        self.layout_stack.pop();

        // insert LayoutEnd into queue before current token
        self.in_progress.push_front(ParserToken {
            ty: ParserTokenType::LayoutEnd(expected_id),
            loc: current_token_and_loc.loc.to_zero_width_start(), // 0-width
        });

        // leave current concrete token unconsumed

        Ok(())
    }
}
