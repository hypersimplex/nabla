use std::vec::Vec;

use super::concrete_token::*;
use super::cur::*;
use super::loc::*;
use super::parser::ParseError;

static TEST_CONTENT_1: &str = r###"
data T B { // constructor with record
  v :: u32,
  x :: B,
}

// sum constructor
data T2 A = T20 T A
              | T21 u32 
              | T22 u32 i32
              | T23 A A
              | T23 T3

// sum constructor (singleton) == product constructor
data T3 = Blah u32 u32 i32

add_and_square :: T -> T2 -> T

add_and_square x y =
// test comments
  let w :: u32 = 7
  
  let z :: String = "some test string here!@#!"

  let ret = case z of
             "a"         -> { 0 }
             "something" -> 1
             _           -> 2
  return ret
  7

f2::Num->Num
f2 x y = x + y

// main :: ()
// main =
//   let a = T { v: 10 };
//   let b = T23 { v: a };
//   let c = add_and_square a b
//
"###;

#[test]
fn test_lex_tokens() {
    let lexed_output =
        parse_content_to_concrete_tokens(std::path::Path::new("dummy_path"), TEST_CONTENT_1)
            .expect("lexing test fixture should succeed");
    // basic sanity checks instead of dumping the entire token stream to stdout
    assert!(lexed_output.0.len() > 0, "lexer should produce tokens");
    assert!(
        matches!(
            lexed_output.0.last().map(|t| &t.token),
            Some(ConcreteToken::EndOfFile)
        ),
        "lexer should terminate with EOF token"
    );
}

pub(crate) fn parse_content_to_concrete_tokens(
    file_path: &std::path::Path,
    file_content: &str,
) -> Result<LexedTokensAndLocs, ParseError> {
    let mut loc = Location::new(file_path);
    let mut cur = Cur::new(file_content.as_bytes());
    let mut out = vec![];
    lex_root(&mut cur, &mut loc, &mut out)?;
    Ok(LexedTokensAndLocs(out))
}

fn try_match_single_token(c: Option<char>, token: ConcreteToken) -> bool {
    match c {
        Some(x) => match try_map_single_char_to_token(&x) {
            Some(y) => y == token,
            _ => false,
        },
        _ => false,
    }
}

fn consume_string_literal(cur: &mut Cur, loc: &mut Location, out: &mut Vec<ConcreteTokenAndLoc>) {
    let span_start_starting_quotes = Span::new(cur.pos_linear(), cur.row(), cur.col());
    //consume "
    let t = try_map_single_char_to_token(&cur.forward().unwrap()).unwrap();
    assert_eq!(t, ConcreteToken::DoubleQuote);

    // ommit quote
    // out.push(ConcreteTokenAndLoc {
    //     token: t,
    //     loc: Location {
    //         file: loc.file.clone(),
    //         span_start: span_start_starting_quotes,
    //         span_end: Span::new(cur.pos_linear(), cur.row(), cur.col()),
    //     },
    // });

    let span_literal_start = Span::new(cur.pos_linear(), cur.row(), cur.col());
    //consume rest of string literal
    let mut literal = vec![];
    let mut escape_next = false;
    while let Some(x) = cur.peek_nth(1) {
        if !escape_next {
            if try_match_single_token(Some(x), ConcreteToken::BackSlash) {
                escape_next = true;
            }
            if try_match_single_token(Some(x), ConcreteToken::DoubleQuote) {
                break;
            }
            literal.push(cur.forward().unwrap());
        } else {
            escape_next = false;
            literal.push(cur.forward().unwrap());
        }
    }

    let span_literal_end = Span::new(cur.pos_linear(), cur.row(), cur.col());

    out.push(ConcreteTokenAndLoc {
        token: ConcreteToken::LiteralString(literal.into_iter().collect()),
        loc: Location {
            file: loc.file.clone(),
            span_start: span_literal_start,
            span_end: span_literal_end,
        },
    });

    // consume ending literal
    // let _span_start_ending_quotes = Span::new(cur.pos_linear(), cur.row(), cur.col());
    let t = try_map_single_char_to_token(&cur.forward().unwrap()).unwrap();
    assert_eq!(t, ConcreteToken::DoubleQuote);
    let span_end_ending_quotes = Span::new(cur.pos_linear(), cur.row(), cur.col());
    // ommit quote
    // out.push(ConcreteTokenAndLoc {
    //     token: t,
    //     loc: Location {
    //         file: loc.file.clone(),
    //         span_start: span_start_ending_quotes,
    //         span_end: span_end_ending_quotes,
    //     },
    // });

    //adjust literal string span indices to include double quotes, later used for space formatting checks
    out.last_mut().unwrap().loc.span_start = span_start_starting_quotes;
    out.last_mut().unwrap().loc.span_end = span_end_ending_quotes;
}

fn consume_comments(cur: &mut Cur, loc: &mut Location, out: &mut Vec<ConcreteTokenAndLoc>) {
    let span_start = Span::new(cur.pos_linear(), cur.row(), cur.col());
    //consume forward slashes
    cur.step(2);
    out.push(ConcreteTokenAndLoc {
        token: ConcreteToken::CommentSlashes,
        loc: Location {
            file: loc.file.clone(),
            span_start,
            span_end: Span::new(cur.pos_linear(), cur.row(), cur.col()),
        },
    });

    let span_comment_start = Span::new(cur.pos_linear(), cur.row(), cur.col());

    //consume rest of the line, not including line break
    let mut comments = vec![];
    while let Some(x) = cur.peek_nth(1) {
        if try_match_single_token(Some(x), ConcreteToken::LineDelimiter) {
            break;
        }
        comments.push(cur.forward().unwrap());
    }

    let span_comment_end = Span::new(cur.pos_linear(), cur.row(), cur.col());

    out.push(ConcreteTokenAndLoc {
        token: ConcreteToken::Comment(comments.into_iter().collect()),
        loc: Location {
            file: loc.file.clone(),
            span_start: span_comment_start,
            span_end: span_comment_end,
        },
    });
}

fn consume_and_map_token(
    cur: &mut Cur,
    loc: &mut Location,
    out: &mut Vec<ConcreteTokenAndLoc>,
    count_chars: usize,
    token: ConcreteToken,
) {
    let span_start = Span::new(cur.pos_linear(), cur.row(), cur.col());
    assert_eq!(count_chars as i64, cur.step(count_chars as i64));
    out.push(ConcreteTokenAndLoc {
        token,
        loc: Location {
            file: loc.file.clone(),
            span_start,
            span_end: Span::new(cur.pos_linear(), cur.row(), cur.col()),
        },
    });
}

fn consume_identifier(cur: &mut Cur, loc: &mut Location, out: &mut Vec<ConcreteTokenAndLoc>) {
    let span_start = Span::new(cur.pos_linear(), cur.row(), cur.col());

    let mut content = vec![];

    content.push(cur.forward().unwrap());

    // Special case: single underscore is a wildcard pattern
    if content[0] == '_'
        && cur
            .peek_nth(1)
            .map_or(true, |x| !x.is_alphanumeric() && x != '_')
    {
        out.push(ConcreteTokenAndLoc {
            token: ConcreteToken::Underscore,
            loc: Location {
                file: loc.file.clone(),
                span_start,
                span_end: Span::new(cur.pos_linear(), cur.row(), cur.col()),
            },
        });
        return;
    }

    while let Some(x) = cur.peek_nth(1) {
        if !(x.is_alphanumeric() || x == '_') {
            break;
        }
        content.push(cur.forward().unwrap());
    }

    out.push(ConcreteTokenAndLoc {
        token: ConcreteToken::Iden(content.into_iter().collect()),
        loc: Location {
            file: loc.file.clone(),
            span_start,
            span_end: Span::new(cur.pos_linear(), cur.row(), cur.col()),
        },
    });
}

fn consume_numeric(cur: &mut Cur, loc: &mut Location, out: &mut Vec<ConcreteTokenAndLoc>) {
    let span_start = Span::new(cur.pos_linear(), cur.row(), cur.col());

    let mut content = vec![];

    //defer checking validity of this numeric string later
    while let Some(x) = cur.peek_nth(1) {
        if !(x.is_numeric() || x == '_' || x == '.') {
            break;
        }
        content.push(cur.forward().unwrap());
    }

    out.push(ConcreteTokenAndLoc {
        token: ConcreteToken::LiteralNumeric(content.into_iter().collect()),
        loc: Location {
            file: loc.file.clone(),
            span_start,
            span_end: Span::new(cur.pos_linear(), cur.row(), cur.col()),
        },
    });
}

fn try_match_keywords<'a>(
    cur: &mut Cur,
    dict: impl Iterator<Item = (&'a str, ConcreteToken)>,
) -> Option<(ConcreteToken, usize)> {
    for (k, v) in dict {
        let l = k.chars().count();
        if cur
            .peek_n(l as i64)
            .into_iter()
            .collect::<String>()
            .as_str()
            == k
        {
            return Some((v, l));
        }
    }
    None
}

fn lex_root(
    cur: &mut Cur,
    loc: &mut Location,
    out: &mut Vec<ConcreteTokenAndLoc>,
) -> Result<(), ParseError> {
    let dict_keywords = [
        ("data", ConcreteToken::Data),
        ("case", ConcreteToken::Case),
        ("of", ConcreteToken::Of),
        ("where", ConcreteToken::Where),
        ("let", ConcreteToken::Let),
        ("in", ConcreteToken::In),
        ("mut", ConcreteToken::Mut),
        ("//", ConcreteToken::CommentSlashes),
        ("<-", ConcreteToken::ArrowLeft),
        ("->", ConcreteToken::ArrowRight),
        ("::", ConcreteToken::IsType),
        ("..", ConcreteToken::Ellipse),
        ("=>", ConcreteToken::ImpliesRight),
        ("<=", ConcreteToken::LessEqual),
        (">=", ConcreteToken::GreaterEqual),
        ("==", ConcreteToken::EqualEqual),
        (" ", ConcreteToken::Space(1)),
        ("\n", ConcreteToken::LineDelimiter),
        ("/", ConcreteToken::FwdSlash),
        ("\\", ConcreteToken::BackSlash),
        ("{", ConcreteToken::BraceL),
        ("}", ConcreteToken::BraceR),
        ("|", ConcreteToken::VertBar),
        ("(", ConcreteToken::ParenL),
        (")", ConcreteToken::ParenR),
        (",", ConcreteToken::Comma),
        (":", ConcreteToken::Colon),
        ("<", ConcreteToken::AngleL),
        (">", ConcreteToken::AngleR),
        ("\"", ConcreteToken::DoubleQuote),
        (".", ConcreteToken::Dot),
        ("=", ConcreteToken::Equal),
        ("*", ConcreteToken::Star),
        ("+", ConcreteToken::Plus),
        ("-", ConcreteToken::Minus),
        ("!", ConcreteToken::Exclamation),
        ("&&", ConcreteToken::And),
        ("||", ConcreteToken::Or),
    ]
    .into_iter();

    loop {
        // println!("lex_root");
        match try_match_keywords(cur, dict_keywords.clone()) {
            Some((token, len_chars)) => {
                if token == ConcreteToken::CommentSlashes {
                    // handle comments
                    consume_comments(cur, loc, out);
                } else if token == ConcreteToken::DoubleQuote {
                    consume_string_literal(cur, loc, out);
                } else {
                    consume_and_map_token(cur, loc, out, len_chars, token);
                }
            }
            _ => match cur.peek_forward() {
                Some(x) => {
                    if x.is_alphabetic() || x == '_' {
                        consume_identifier(cur, loc, out);
                    } else if x.is_numeric() {
                        consume_numeric(cur, loc, out);
                    } else {
                        return Err(ParseError::message(
                            format!(
                                "unexpected token '{}' at row {}, col {}",
                                x,
                                cur.row(),
                                cur.col()
                            ),
                            None,
                        ));
                    }
                }
                _ => {
                    out.push(ConcreteTokenAndLoc {
                        token: ConcreteToken::EndOfFile,
                        loc: Location {
                            file: loc.file.clone(),
                            span_start: Span::new(cur.pos_linear(), cur.row(), cur.col()),
                            span_end: Span::new(cur.pos_linear(), cur.row(), cur.col() + 1),
                        },
                    });
                    break;
                }
            },
        }
    }

    Ok(())
}
