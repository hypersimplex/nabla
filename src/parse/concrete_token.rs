use crate::util::printer::*;

use std::collections::HashMap;
use std::fmt;

// reserved syntactic items in language
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum ConcreteToken {
    EndOfFile,
    Space(usize),   // number of blank spaces
    LineDelimiter,  // \n
    FwdSlash,       // /
    BackSlash,      // \ (for lambda or latex symbol) TODO
    BraceL,         // {
    BraceR,         // }
    BracketL,       // [
    BracketR,       // ]
    VertBar,        // |
    ParenL,         // (
    ParenR,         // )
    Comma,          // ,
    Colon,          // :
    AngleL,         // <
    AngleR,         // >
    DoubleQuote,    // "
    Dot,            // .
    Equal,          // =
    Star,           // *
    Plus,           // +
    Minus,          // -
    Exclamation,    // !
    And,            // &&
    Or,             // ||
    BinaryPlus,     // internally converted from Plus
    BinaryMinus,    // internally converted from Minus
    BinaryMul,      // internally converted from Star
    BinaryDiv,      // internally converted from FwdSlash
    BinaryAnd,      // internally converted from And
    BinaryOr,       // internally converted from Or
    UnaryNot,       // internally converted from Exclamation
    UnaryMinus,     // internally converted from Minus
    UnaryPlus,      // internally converted from Plus
    CommentSlashes, // //
    Comment(String),
    ArrowLeft,              // <-
    ArrowRight,             // ->
    IsType,                 // ::
    Ellipse,                // ..
    ImpliesRight,           // =>
    LessEqual,              // <=
    GreaterEqual,           // >=
    EqualEqual,             // ==
    Underscore,             // _ (wildcard pattern)
    Iden(String),           //[_a-zA-Z]+[_0-9a-zA-Z]*
    LiteralString(String),  //"asfasdf"
    LiteralNumeric(String), //9.1223
    //keywords:
    Data, // type constructor
    Case, // case
    Of,   // of
    Let,  // let
    In,   // in
    // TODOs
    Where, // where
    Mut,   // mut
           // TODO?:
           // StmDelimiter, // ;
           // Ampersand,   // &
           // Trait,   // trait
           // TyUnit, // ()
}

pub(crate) fn try_map_single_char_to_token(c: &char) -> Option<ConcreteToken> {
    let single_symbol_dict: HashMap<char, ConcreteToken> = [
        (' ', ConcreteToken::Space(1)),
        ('\n', ConcreteToken::LineDelimiter),
        ('/', ConcreteToken::FwdSlash),
        ('\\', ConcreteToken::BackSlash),
        ('{', ConcreteToken::BraceL),
        ('}', ConcreteToken::BraceR),
        ('[', ConcreteToken::BracketL),
        (']', ConcreteToken::BracketR),
        ('|', ConcreteToken::VertBar),
        ('(', ConcreteToken::ParenL),
        (')', ConcreteToken::ParenR),
        (',', ConcreteToken::Comma),
        (':', ConcreteToken::Colon),
        ('<', ConcreteToken::AngleL),
        ('>', ConcreteToken::AngleR),
        ('"', ConcreteToken::DoubleQuote),
        ('.', ConcreteToken::Dot),
        ('=', ConcreteToken::Equal),
        ('*', ConcreteToken::Star),
        ('+', ConcreteToken::Plus),
        ('-', ConcreteToken::Minus),
        ('!', ConcreteToken::Exclamation),
    ]
    .into_iter()
    .collect();

    single_symbol_dict.get(c).cloned()
}

fn map_token_to_chars(t: &ConcreteToken) -> String {
    let mapping: HashMap<ConcreteToken, &str> = [
        (ConcreteToken::EndOfFile, "EOF"),
        (ConcreteToken::Space(1), " "),
        (ConcreteToken::LineDelimiter, "\n"),
        (ConcreteToken::FwdSlash, "/"),
        (ConcreteToken::BackSlash, "\\"),
        (ConcreteToken::BraceL, "{"),
        (ConcreteToken::BraceR, "}"),
        (ConcreteToken::BracketL, "["),
        (ConcreteToken::BracketR, "]"),
        (ConcreteToken::VertBar, "|"),
        (ConcreteToken::ParenL, "("),
        (ConcreteToken::ParenR, ")"),
        (ConcreteToken::Comma, ","),
        (ConcreteToken::Colon, ":"),
        (ConcreteToken::AngleL, "<"),
        (ConcreteToken::AngleR, ">"),
        (ConcreteToken::DoubleQuote, "\""),
        (ConcreteToken::Dot, "."),
        (ConcreteToken::Equal, "="),
        (ConcreteToken::Star, "*"),
        (ConcreteToken::Plus, "+"),
        (ConcreteToken::Minus, "-"),
        (ConcreteToken::Exclamation, "!"),
        (ConcreteToken::And, "&&"),
        (ConcreteToken::Or, "||"),
        (ConcreteToken::BinaryPlus, "+"),
        (ConcreteToken::BinaryMinus, "-"),
        (ConcreteToken::BinaryMul, "*"),
        (ConcreteToken::BinaryDiv, "/"),
        (ConcreteToken::BinaryAnd, "&&"),
        (ConcreteToken::BinaryOr, "||"),
        (ConcreteToken::UnaryNot, "!"),
        (ConcreteToken::UnaryMinus, "-"),
        (ConcreteToken::UnaryPlus, "+"),
        (ConcreteToken::CommentSlashes, "//"),
        (ConcreteToken::ArrowLeft, "<-"),
        (ConcreteToken::ArrowRight, "->"),
        (ConcreteToken::IsType, "::"),
        (ConcreteToken::Ellipse, ".."),
        (ConcreteToken::ImpliesRight, "=>"),
        (ConcreteToken::LessEqual, "<="),
        (ConcreteToken::GreaterEqual, ">="),
        (ConcreteToken::EqualEqual, "=="),
        (ConcreteToken::Underscore, "_"),
        (ConcreteToken::Data, "data"),
        (ConcreteToken::Case, "case"),
        (ConcreteToken::Of, "of"),
        (ConcreteToken::Where, "where"),
        (ConcreteToken::Let, "let"),
        (ConcreteToken::In, "in"),
        (ConcreteToken::Mut, "mut"),
    ]
    .into_iter()
    .collect();

    match mapping.get(t) {
        Some(x) => x.to_string(),
        _ => match t {
            ConcreteToken::Comment(x) => x.clone(),
            ConcreteToken::Iden(x) => x.clone(),
            ConcreteToken::LiteralString(x) => x.clone(),
            ConcreteToken::LiteralNumeric(x) => x.clone(),
            _ => {
                panic! {"unexpected token: {:?}", t};
            }
        },
    }
}

impl fmt::Display for ConcreteToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = map_token_to_chars(self);
        write!(f, "{}", s)
    }
}

// helper impl. for doc printer trait --->>

impl DocPrinter for ConcreteToken {
    fn to_doc(&self) -> Box<Doc> {
        mk_lit(&format!("{}", self))
    }
}

// <<--- helper impl. for doc printer trait
