/// printer using Wadler/Lindig's approach
use std::collections::*;
use std::fmt;

#[derive(Debug, Clone)]
pub(crate) enum Doc {
    Nil,                     // identity element
    Text(String),            // literal
    Line,                    // space or linebreak
    LineForce,               // linebreak
    Nest(Indent, Box<Doc>),  //indent
    Cat(Box<Doc>, Box<Doc>), //concat from left to right
    Group(Box<Doc>),         // flexible lookahead for flat or break
}

// Implement the Display trait for your struct
impl fmt::Display for Box<Doc> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let width_limit = 100;
        let pos = 0;
        let mut stack = vec![StackItem {
            indent: 0,
            mode: Mode::Flat,
            doc: self.clone(),
        }];
        write!(f, "{}", pretty_print(width_limit, pos, &mut stack))
    }
}

#[derive(Debug, Clone, Copy)]
struct Indent(pub usize);

#[derive(Debug, Clone, Copy)]
enum Mode {
    Flat,
    Break,
}

#[derive(Debug, Clone)]
struct StackItem {
    indent: usize,
    mode: Mode,
    doc: Box<Doc>,
}

pub(crate) trait DocPrinter {
    fn to_doc(&self) -> Box<Doc>;
}

pub(crate) fn cat_space(doc1: Box<Doc>, doc2: Box<Doc>) -> Box<Doc> {
    use Doc::*;
    let s = Box::new(Cat(doc1, Box::new(Text(" ".to_string()))));
    Box::new(Cat(s, doc2))
}

pub(crate) fn mk_lit(s: &str) -> Box<Doc> {
    Box::new(Doc::Text(s.into()))
}

pub(crate) fn mk_line() -> Box<Doc> {
    Box::new(Doc::Line)
}
pub(crate) fn mk_line_force() -> Box<Doc> {
    Box::new(Doc::LineForce)
}

pub(crate) fn mk_cat(doc1: Box<Doc>, doc2: Box<Doc>) -> Box<Doc> {
    Box::new(Doc::Cat(doc1, doc2))
}

pub(crate) fn mk_nil() -> Box<Doc> {
    Box::new(Doc::Nil)
}

pub(crate) fn mk_nest(indent: usize, doc: Box<Doc>) -> Box<Doc> {
    Box::new(Doc::Nest(Indent(indent), doc))
}

pub(crate) fn mk_group(doc: Box<Doc>) -> Box<Doc> {
    Box::new(Doc::Group(doc))
}

pub(crate) fn pretty_print(
    width_limit: usize,
    mut pos: usize,
    stack: &mut Vec<StackItem>,
) -> String {
    let mut ret = String::new();

    while let Some(StackItem { indent, mode, doc }) = stack.pop() {
        match *doc {
            Doc::Nil => {
                // continue
            }
            Doc::Text(s) => {
                let l = s.len();
                ret.push_str(s.as_str());
                pos += l;
            }
            Doc::Line => match mode {
                Mode::Flat => {
                    ret.push_str(" ");
                    pos += 1;
                }
                _ => {
                    let spaces = format!("{:width$}", " ", width = indent);
                    ret.push_str("\n");
                    ret.push_str(&spaces);
                    pos = indent;
                }
            },
            Doc::LineForce => {
                let spaces = format!("{:width$}", " ", width = indent);
                ret.push_str("\n");
                ret.push_str(&spaces);
                pos = indent;
            }
            Doc::Nest(Indent(i), doc) => {
                stack.push(StackItem {
                    indent: indent + i,
                    mode,
                    doc,
                });
            }
            Doc::Cat(doc1, doc2) => {
                stack.push(StackItem {
                    indent,
                    mode,
                    doc: doc2,
                });
                stack.push(StackItem {
                    indent,
                    mode,
                    doc: doc1,
                });
            }
            Doc::Group(doc) => {
                let s = stack.clone();
                // perform lookahead
                if can_fit(width_limit as i32, width_limit as i32 - pos as i32, s) {
                    // choose flat
                    stack.push(StackItem {
                        indent,
                        mode: Mode::Flat,
                        doc,
                    });
                } else {
                    // choose break
                    stack.push(StackItem {
                        indent,
                        mode: Mode::Break,
                        doc,
                    });
                }
            }
        }
    }
    ret
}

fn can_fit(width_max: i32, width_remain: i32, mut stack: Vec<StackItem>) -> bool {
    if width_remain < 0 {
        return false; // terminal case
    }
    let StackItem { indent, mode, doc } = match stack.pop() {
        None => return true, // termial case
        Some(x) => x,
    };
    match *doc {
        Doc::Nil => can_fit(width_max, width_remain, stack),
        Doc::Text(s) => can_fit(width_max, width_remain - s.len() as i32, stack),
        Doc::Line => {
            match mode {
                Mode::Flat => {
                    // take up 1 space
                    can_fit(width_max, width_remain - 1i32, stack)
                }
                Mode::Break => {
                    // new line
                    // take account of indent
                    can_fit(width_max, width_max - indent as i32, stack)
                }
            }
        }
        Doc::LineForce => {
            // new line
            // take account of indent
            can_fit(width_max, width_max - indent as i32, stack)
        }
        Doc::Nest(Indent(i), doc) => {
            stack.push(StackItem {
                indent: indent + i, // add relative indent
                mode,
                doc,
            });
            can_fit(width_max, width_remain, stack)
        }
        Doc::Cat(doc1, doc2) => {
            stack.push(StackItem {
                indent,
                mode,
                doc: doc2,
            });
            stack.push(StackItem {
                indent,
                mode,
                doc: doc1,
            });
            can_fit(width_max, width_remain, stack)
        }
        Doc::Group(doc) => {
            // unconditionally use flat
            stack.push(StackItem {
                indent,
                mode: Mode::Flat,
                doc,
            });
            can_fit(width_max, width_remain, stack)
        }
    }
}

static TEST_CONTENT_FOR_PRINTER: &str = r###"
f_let a = a * 7

f b = case b of
        2 -> 2
        _ -> 100

f_let a =
 let x :: u32 = 1
     y :: u32 = 2
     z :: u32 = 5
 in
   let b = 10
       c = case b of
             b | b>2 -> 2*x
             _ -> 100
   in x + y + z + a + b * c

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

f_test_lambda :: u32 -> u32
f_test_lambda a b =
 (\x y -> x * y) a b

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

ff x =
 let y = 1-(case z of
             "a"         -> (0 * 5)
             "something" -> x
             _           -> 2
           )*7
 in y
"###;

#[cfg(test)]
mod tests {
    use crate::parse::abstr::parse_concrete_top_level;
    use crate::parse::abstr_structures::*;
    use crate::parse::lex::parse_content_to_concrete_tokens;
    use std::path::Path;

    use crate::parse::printer::*;

    #[test]
    fn test_printer() {
        parse_and_print(super::TEST_CONTENT_FOR_PRINTER);
    }

    fn parse_and_print(input: &str) {
        let lexed_output = parse_content_to_concrete_tokens(Path::new("dummy_path"), input)
            .expect("lexing abstract parser fixture");

        let top_level_items = parse_concrete_top_level(lexed_output)
            .expect("abstract parser should succeed for test fixture");

        assert!(
            top_level_items.0.len() > 0,
            "expected at least one top-level item"
        );

        println!("{}", top_level_items.to_doc());
    }
}
