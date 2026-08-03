use crate::builtin::types::*;
use crate::builtin::values::*;
use crate::parse::abstr::*;
use crate::parse::abstr_structures::*;
use crate::parse::concrete_token::*;
use crate::parse::lex::*;
use crate::parse::loc::*;
use crate::parse::parser::*;
use crate::parse::printer::DocPrinter;
use crate::typecheck::env_vvar_to_ty_scheme::*;
use crate::typecheck::ty_err::*;
use crate::typecheck::ty_expr::*;
use crate::typecheck::ty_var_name_supply::*;

use std::path::Path;

pub(crate) enum CompileError {
    Parse(ParseError),
    Type(TyError),
    Other,
}

impl From<ParseError> for CompileError {
    fn from(e: ParseError) -> Self {
        Self::Parse(e)
    }
}

impl From<TyError> for CompileError {
    fn from(e: TyError) -> Self {
        Self::Type(e)
    }
}

type CompileResult = Result<(), CompileError>;

pub(crate) fn compile(content: &str) -> CompileResult {
    let lexed_output = parse_content_to_concrete_tokens(Path::new("dummy_path"), content)?;

    // for now, parser converts functions to lambda abstractions
    let top_level = parse_concrete_top_level(lexed_output)?;

    println!("{}", top_level.to_doc());

    let mut ty_var_ns = TyVarNameSupply::new();
    let mut env_vvar_to_ty_scheme = EnvVVarToTyScheme::new();

    init_builtin_values(&mut env_vvar_to_ty_scheme, &mut ty_var_ns);

    // // declared top-level type schemes indexed by function name
    // // used for post type inference signature checking
    // let mut declared_type_schemes_by_fn_name: BTreeMap<String, TyScheme> = BTreeMap::new();
    // insert_declared_signatures(
    //     &parsed_top_level,
    //     &type_env,
    //     &mut env_vvar_to_ty_scheme,
    //     &mut declared_type_schemes_by_fn_name,
    //     &mut ty_var_ns,
    // );

    // TODO:
    // collect bindings, insert monomorphic type variables for these
    // dependency analysis and sort them
    // typecheck each SCC group in dependency order

    todo!()
}

static TEST_PIPELINE_CONTENT_0: &str = r###"
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
"###;

// #[test]
// fn test_pipeline() {
//     compile(TEST_PIPELINE_CONTENT_0);
// }
