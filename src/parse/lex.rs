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

pub(crate) fn parse_content_to_concrete_tokens(
    file_path: &std::path::Path,
    file_content: &str,
) -> Result<LexedTokensAndLocs, ParseError> {
    let mut loc = Location::new(file_path);
    let mut cur = Cur::new(file_content.as_bytes());
    let mut out = vec![];
    lex_root(&mut cur, &mut loc, &mut out)?;
    let out = assign_line_start(out.as_slice());
    Ok(LexedTokensAndLocs(out))
}

fn assign_line_start(inputs: &[ConcreteTokenAndLoc]) -> Vec<ConcreteTokenAndLoc> {
    let mut ret = vec![];
    // state used to set the relevant field for each output token
    let mut start_next_line = true;
    for i in inputs {
        match &i.token {
            ConcreteToken::LineDelimiter => {
                // discard this token and set the relevant state for next token
                start_next_line = true;
            }
            ConcreteToken::Space(_) | ConcreteToken::CommentSlashes | ConcreteToken::Comment(_) => {
                // discard token and preserve current state
            }
            ConcreteToken::EndOfFile => {
                ret.push(ConcreteTokenAndLoc {
                    token: i.token.clone(),
                    loc: i.loc.clone(),
                    starts_a_line: false,
                });
            }
            other => {
                ret.push(ConcreteTokenAndLoc {
                    token: i.token.clone(),
                    loc: i.loc.clone(),
                    starts_a_line: start_next_line,
                });
                start_next_line = false;
            }
        }
    }
    ret
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

fn consume_string_literal(
    cur: &mut Cur,
    loc: &Location,
) -> Result<ConcreteTokenAndLoc, ParseError> {
    let span_start = Span::new(cur.pos_linear(), cur.row(), cur.col());
    if cur.forward() != Some('"') {
        return Err(ParseError::message(
            "expected string literal opening quote",
            None,
        ));
    }

    let mut literal = String::new();
    let mut escape_next = false;
    loop {
        let Some(ch) = cur.forward() else {
            return Err(ParseError::message(
                "unterminated string literal",
                Some(ConcreteTokenAndLoc {
                    token: ConcreteToken::LiteralString(literal),
                    loc: Location {
                        file: loc.file.clone(),
                        span_start,
                        span_end: Span::new(cur.pos_linear(), cur.row(), cur.col()),
                    },
                    starts_a_line: false,
                }),
            ));
        };

        if escape_next {
            literal.push(ch);
            escape_next = false;
            continue;
        }

        match ch {
            '\\' => {
                literal.push(ch);
                escape_next = true;
            }
            '"' => break,
            _ => literal.push(ch),
        }
    }

    Ok(ConcreteTokenAndLoc {
        token: ConcreteToken::LiteralString(literal),
        loc: Location {
            file: loc.file.clone(),
            span_start,
            span_end: Span::new(cur.pos_linear(), cur.row(), cur.col()),
        },
        starts_a_line: false,
    })
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
        starts_a_line: false,
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
        starts_a_line: false,
    });
}

fn consume_fixed_token(
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
        starts_a_line: false,
    });
}

fn classify_identifier_or_keyword(content: String) -> ConcreteToken {
    match content.as_str() {
        "_" => ConcreteToken::Underscore,
        "data" => ConcreteToken::Data,
        "case" => ConcreteToken::Case,
        "of" => ConcreteToken::Of,
        "where" => ConcreteToken::Where,
        "let" => ConcreteToken::Let,
        "in" => ConcreteToken::In,
        "mut" => ConcreteToken::Mut,
        _ => ConcreteToken::Iden(content),
    }
}

fn consume_identifier(cur: &mut Cur, loc: &mut Location, out: &mut Vec<ConcreteTokenAndLoc>) {
    let span_start = Span::new(cur.pos_linear(), cur.row(), cur.col());

    let mut content = String::new();

    content.push(cur.forward().unwrap());

    while let Some(x) = cur.peek_nth(1) {
        if !(x.is_alphanumeric() || x == '_') {
            break;
        }
        content.push(cur.forward().unwrap());
    }

    out.push(ConcreteTokenAndLoc {
        token: classify_identifier_or_keyword(content),
        loc: Location {
            file: loc.file.clone(),
            span_start,
            span_end: Span::new(cur.pos_linear(), cur.row(), cur.col()),
        },
        starts_a_line: false,
    });
}

fn consume_numeric(cur: &mut Cur, loc: &mut Location, out: &mut Vec<ConcreteTokenAndLoc>) {
    let span_start = Span::new(cur.pos_linear(), cur.row(), cur.col());

    let mut content = String::new();
    let mut has_decimal_point = false;

    while let Some(x) = cur.peek_nth(1) {
        match x {
            x if x.is_numeric() || x == '_' => {
                content.push(cur.forward().unwrap());
            }
            // - a dot belongs to this literal only when it starts its one decimal fraction
            // - otherwise leave it for fixed-token scanning, including the `..` range token
            '.' if !has_decimal_point && cur.peek_nth(2).is_some_and(|next| next.is_numeric()) => {
                has_decimal_point = true;
                content.push(cur.forward().unwrap());
            }
            _ => break,
        }
    }

    out.push(ConcreteTokenAndLoc {
        token: ConcreteToken::LiteralNumeric(content),
        loc: Location {
            file: loc.file.clone(),
            span_start,
            span_end: Span::new(cur.pos_linear(), cur.row(), cur.col()),
        },
        starts_a_line: false,
    });
}

fn try_match_fixed_token(
    cur: &Cur,
    fixed_tokens: &[(&str, ConcreteToken)],
) -> Option<(ConcreteToken, usize)> {
    for (spelling, token) in fixed_tokens {
        let len = spelling.chars().count();
        if cur.peek_n(len as i64).into_iter().eq(spelling.chars()) {
            return Some((token.clone(), len));
        }
    }
    None
}

fn lex_root(
    cur: &mut Cur,
    loc: &mut Location,
    out: &mut Vec<ConcreteTokenAndLoc>,
) -> Result<(), ParseError> {
    let fixed_tokens = [
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
        ("[", ConcreteToken::BracketL),
        ("]", ConcreteToken::BracketR),
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
        ("|", ConcreteToken::VertBar),
    ];

    loop {
        match try_match_fixed_token(cur, &fixed_tokens) {
            Some((token, len_chars)) => {
                if token == ConcreteToken::CommentSlashes {
                    // handle comments
                    consume_comments(cur, loc, out);
                } else if token == ConcreteToken::DoubleQuote {
                    out.push(consume_string_literal(cur, loc)?);
                } else {
                    consume_fixed_token(cur, loc, out, len_chars, token);
                }
            }
            None => match cur.peek_forward() {
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
                None => {
                    out.push(ConcreteTokenAndLoc {
                        token: ConcreteToken::EndOfFile,
                        loc: Location {
                            file: loc.file.clone(),
                            span_start: Span::new(cur.pos_linear(), cur.row(), cur.col()),
                            span_end: Span::new(cur.pos_linear(), cur.row(), cur.col()),
                        },
                        starts_a_line: false,
                    });
                    break;
                }
            },
        }
    }

    Ok(())
}

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

#[test]
fn test_string_literals() {
    let cases = [
        ("\"\"", ""),
        ("\"abc\"", "abc"),
        (r#""a\"b""#, r#"a\"b"#),
        (r#""a\\b""#, r#"a\\b"#),
    ];

    for (source, expected) in cases {
        let lexed = parse_content_to_concrete_tokens(std::path::Path::new("dummy_path"), source)
            .expect("lexing a terminated string literal should succeed");

        assert_eq!(lexed.0.len(), 2, "source: {source}");
        assert_eq!(
            lexed.0[0].token,
            ConcreteToken::LiteralString(expected.into()),
            "source: {source}"
        );
        assert_eq!(
            (
                lexed.0[0].loc.span_start.linear,
                lexed.0[0].loc.span_end.linear,
            ),
            (0, source.chars().count()),
            "source: {source}"
        );
        assert_eq!(lexed.0[1].token, ConcreteToken::EndOfFile);
    }
}

#[test]
fn test_unterminated_string_literals() {
    let cases = [("\"", ""), ("\"abc", "abc"), ("\"abc\\", "abc\\")];

    for (source, expected) in cases {
        let error = parse_content_to_concrete_tokens(std::path::Path::new("dummy_path"), source)
            .expect_err("lexing an unterminated string literal should fail");

        let ParseError::Message {
            message,
            token: Some(token),
        } = error
        else {
            panic!("unexpected error for {source:?}: {error:?}");
        };

        assert_eq!(message, "unterminated string literal");
        assert_eq!(
            token.token,
            ConcreteToken::LiteralString(expected.into()),
            "source: {source}"
        );
        assert_eq!(
            (token.loc.span_start.linear, token.loc.span_end.linear),
            (0, source.chars().count()),
            "source: {source}"
        );
    }
}

#[test]
fn test_keywords_are_classified() {
    let lexed_output = parse_content_to_concrete_tokens(
        std::path::Path::new("dummy_path"),
        "data dataValue case casey of offset where whereabouts let letter let2 let_value in input mut mutable Data _ _value",
    )
    .expect("lexing identifiers and keywords should succeed");

    let tokens: Vec<_> = lexed_output
        .0
        .into_iter()
        .map(|token_and_loc| token_and_loc.token)
        .collect();

    assert_eq!(
        tokens,
        vec![
            ConcreteToken::Data,
            ConcreteToken::Iden("dataValue".into()),
            ConcreteToken::Case,
            ConcreteToken::Iden("casey".into()),
            ConcreteToken::Of,
            ConcreteToken::Iden("offset".into()),
            ConcreteToken::Where,
            ConcreteToken::Iden("whereabouts".into()),
            ConcreteToken::Let,
            ConcreteToken::Iden("letter".into()),
            ConcreteToken::Iden("let2".into()),
            ConcreteToken::Iden("let_value".into()),
            ConcreteToken::In,
            ConcreteToken::Iden("input".into()),
            ConcreteToken::Mut,
            ConcreteToken::Iden("mutable".into()),
            ConcreteToken::Iden("Data".into()),
            ConcreteToken::Underscore,
            ConcreteToken::Iden("_value".into()),
            ConcreteToken::EndOfFile,
        ]
    );
}

#[test]
fn test_numeric_boundaries() {
    let lex_token_types = |source| {
        parse_content_to_concrete_tokens(std::path::Path::new("dummy_path"), source)
            .expect("lexing numeric and range boundaries should succeed")
            .0
            .into_iter()
            .map(|token_and_loc| token_and_loc.token)
            .collect::<Vec<_>>()
    };
    let numeric = |content: &str| ConcreteToken::LiteralNumeric(content.into());

    let cases = [
        (
            "1..2",
            vec![
                numeric("1"),
                ConcreteToken::Ellipse,
                numeric("2"),
                ConcreteToken::EndOfFile,
            ],
        ),
        (
            "1.5..2.5",
            vec![
                numeric("1.5"),
                ConcreteToken::Ellipse,
                numeric("2.5"),
                ConcreteToken::EndOfFile,
            ],
        ),
        (
            "1_000 0.000_001 _1000",
            vec![
                numeric("1_000"),
                numeric("0.000_001"),
                ConcreteToken::Iden("_1000".into()),
                ConcreteToken::EndOfFile,
            ],
        ),
    ];

    for (source, expected) in cases {
        assert_eq!(lex_token_types(source), expected, "source: {source}");
    }

    let range = parse_content_to_concrete_tokens(std::path::Path::new("dummy_path"), "1..2")
        .expect("lexing a numeric range should succeed");
    let spans: Vec<_> = range
        .0
        .iter()
        .take(3)
        .map(|token_and_loc| {
            (
                token_and_loc.loc.span_start.linear,
                token_and_loc.loc.span_end.linear,
            )
        })
        .collect();
    assert_eq!(spans, vec![(0, 1), (1, 3), (3, 4)]);
}

#[test]
fn test_brackets_are_lexed() {
    let tokens = parse_content_to_concrete_tokens(std::path::Path::new("dummy_path"), "[]")
        .expect("lexing bracket tokens should succeed")
        .0
        .into_iter()
        .map(|token_and_loc| token_and_loc.token)
        .collect::<Vec<_>>();

    assert_eq!(
        tokens,
        vec![
            ConcreteToken::BracketL,
            ConcreteToken::BracketR,
            ConcreteToken::EndOfFile,
        ]
    );
}
