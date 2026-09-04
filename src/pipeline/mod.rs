use crate::builtin::types::*;
use crate::builtin::values::*;
use crate::core::core_ir::*;
use crate::normalize::case_guard::*;
use crate::normalize::case_scrutinee::*;
use crate::normalize::literal_pattern::*;
use crate::normalize::literal_range_pattern::*;
use crate::normalize::pattern::*;
use crate::normalize::variable_renamer::*;
use crate::parse::abstr::*;
use crate::parse::abstr_structures::*;
use crate::parse::concrete_token::*;
use crate::parse::lex::*;
use crate::parse::loc::ConcreteTokenAndLoc;
use crate::parse::parser::*;
use crate::typecheck::adt::*;
use crate::typecheck::convert_v_expr_from_a_expr::*;
use crate::typecheck::env_v_var_to_ty_scheme::*;
use crate::typecheck::pat_binder_uniqueness::*;
use crate::typecheck::subst::*;
use crate::typecheck::subst_persistent::*;
use crate::typecheck::ty_env::*;
use crate::typecheck::ty_err::*;
use crate::typecheck::ty_expr::*;
use crate::typecheck::ty_inference::*;
use crate::typecheck::ty_scheme::*;
use crate::typecheck::ty_var_name::*;
use crate::typecheck::ty_var_name_supply::*;
use crate::typecheck::v_expr::*;
use crate::typecheck::v_expr_typed::*;
use crate::typecheck::v_var_name::*;
use crate::typecheck::v_var_name_supply::*;
use crate::util::printer::*;

use std::collections::*;
use std::path::Path;

#[derive(Clone, Debug)]
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

    // note: parser converts functions to lambda abstractions already
    let top_level = parse_concrete_top_level(lexed_output)?;

    println!("{}", top_level.to_doc());

    let mut ty_var_ns = TyVarNameSupply::new();
    let mut env_v_var_to_ty_scheme = EnvVVarToTyScheme::new();

    let ty_env = register_adt_into_type_env(&mut ty_var_ns, top_level.0.as_slice())?;

    // note: builtin name does not change
    init_builtin_values(&mut env_v_var_to_ty_scheme, &mut ty_var_ns);

    // function name -> top-level type schemes
    let mut declared_function_type_schemes: BTreeMap<String, TyScheme> = BTreeMap::new();
    insert_declared_fun_signatures(
        top_level.0.as_slice(),
        &ty_env,
        &mut env_v_var_to_ty_scheme,
        &mut declared_function_type_schemes,
        &mut ty_var_ns,
    );

    let mut v_var_ns: VVarNameSupply = VVarNameSupply::new();

    let mut funcs: BTreeMap<usize, (VVar, VExpr)> = BTreeMap::new();

    let mut seen_top_level_fn_names = BTreeSet::new();

    for (idx, item) in top_level.0.iter().enumerate() {
        if let TopLevelItem::FunctionDefinition(def) = item {
            let TopLevelFunction { name, abstraction } = def;

            // convert to an abstraction expression
            let (vexpr, _annot) = vexpr_and_ty_annot_from_aexpr(
                &AExpr::AbstractionExpression(abstraction.clone()),
                &mut v_var_ns,
            );
            validate_pattern_binder_uniqueness(&vexpr)?;
            assert!(matches!(&vexpr, VExpr::Abstraction(_)));

            let var_binder_for_fn = VVar::Named(VVarName {
                token: name.token.clone(),
                loc: Some(name.loc.clone()),
                builtin: None,
            });

            if seen_top_level_fn_names.contains(&var_binder_for_fn) {
                return Err(TyError::PatBinderUniqueness(format!(
                    "duplicate binder for top level function: {:?}",
                    &var_binder_for_fn
                )))?;
            }
            seen_top_level_fn_names.insert(var_binder_for_fn.clone());

            funcs.insert(idx, (var_binder_for_fn, vexpr));
        }
    }

    println!("rename user introduced local variables and patterns to uniquify..");
    // top level bindings stays constant (identity map)
    let vvars_outer_scope: BTreeMap<VVar, VVar> = env_v_var_to_ty_scheme
        .0
        .keys()
        .map(|x| (x.clone(), x.clone()))
        .collect();

    for (idx, (_var_binder_for_fn, expr)) in funcs.iter_mut() {
        *expr = rename_var_unique(&mut v_var_ns, &vvars_outer_scope, expr);
    }

    // collect binding, definition, and optional signature for each top level
    // function
    let def_indices: Vec<usize> = funcs.keys().copied().collect();
    let defs: Vec<(VPattern, VExpr, Option<TyScheme>)> = def_indices
        .iter()
        .map(|idx| {
            let (var_binder_to_fn, vexpr) = funcs.get(idx).unwrap().clone();

            let opt_signature = match &var_binder_to_fn {
                VVar::Named(VVarName {
                    token: ConcreteToken::Iden(name),
                    ..
                }) => declared_function_type_schemes.get(name.as_str()).cloned(),
                _ => None,
            };
            (VPattern::Variable(var_binder_to_fn), vexpr, opt_signature)
        })
        .collect();

    println!("analyzing dependency of top level functions and type checking..");

    // this is used to compute free variables in environment; these are needed
    // to determine the set of generalizable type variables which become
    // schematic type variables in type schemes
    let mut env_outer: EnvVVarToTyScheme = env_v_var_to_ty_scheme.clone();

    let mut env_v_var_to_ty_scheme_binding_seed: EnvVVarToTyScheme = env_v_var_to_ty_scheme.clone();

    let (_subst, ordered_mutually_recursive_groups) = ty_check_binding_group(
        &mut env_v_var_to_ty_scheme_binding_seed,
        &mut env_outer,
        &ty_env,
        &mut ty_var_ns,
        &defs,
    )?;

    // ordered in mutually dependent function groups
    let mut ty_check_results: Vec<BTreeMap<usize, TypedTopLevelFunction>> = Vec::new();

    for scc_group in ordered_mutually_recursive_groups.iter() {
        let mut group: BTreeMap<usize, TypedTopLevelFunction> = BTreeMap::new();
        for (idx, def) in scc_group.into_iter() {
            let orig_idx = def_indices[*idx];
            let (var_binder_to_fn, vexpr) = funcs.get(&orig_idx).unwrap().clone();
            let ty_expr = def.typed_rhs.ty().clone();
            group.insert(
                orig_idx,
                TypedTopLevelFunction {
                    name: var_binder_to_fn.clone(),
                    vexpr,
                    ty_expr,
                    scheme: def.scheme.clone(),
                    typed_expr: def.typed_rhs.clone(),
                },
            );
        }
        ty_check_results.push(group);
    }

    println!("type checked functions --->>");
    for group in ty_check_results.iter() {
        for (id, top_lvl_fn) in group.iter() {
            print!("{}", top_lvl_fn.to_doc());
            println!();
        }
    }
    println!("<<--- type checked functions");

    // type preserving passes --->>

    println!("desugar patterns to appear only in case clause pattern binders..");
    for group in ty_check_results.iter_mut() {
        for (id, top_lvl_fn) in group.iter_mut() {
            top_lvl_fn.typed_expr = desugar_pattern(&mut v_var_ns, &top_lvl_fn.typed_expr);
        }
    }

    println!("desugar case literal range pattern to case guard expression");
    for group in ty_check_results.iter_mut() {
        for (id, top_lvl_fn) in group.iter_mut() {
            top_lvl_fn.typed_expr = desugar_literal_range_pattern(
                &mut v_var_ns,
                &mut env_v_var_to_ty_scheme,
                &top_lvl_fn.typed_expr,
            );
        }
    }

    println!("desugar case literal pattern to case guard expression");
    for group in ty_check_results.iter_mut() {
        for (id, top_lvl_fn) in group.iter_mut() {
            top_lvl_fn.typed_expr = desugar_literal_pattern(
                &mut v_var_ns,
                &mut env_v_var_to_ty_scheme,
                &top_lvl_fn.typed_expr,
            );
        }
    }

    println!("normalize case scrutinee to be simple variable..");
    for group in ty_check_results.iter_mut() {
        for (id, top_lvl_fn) in group.iter_mut() {
            top_lvl_fn.typed_expr = normalize_case_scrutinee(&mut v_var_ns, &top_lvl_fn.typed_expr);
        }
    }

    println!("desugar case guard to case expressions without guard expressions..");
    for group in ty_check_results.iter_mut() {
        for (id, top_lvl_fn) in group.iter_mut() {
            top_lvl_fn.typed_expr = desugar_case_guard(&mut v_var_ns, &top_lvl_fn.typed_expr);
        }
    }

    println!("normalize case scrutinee to be simple variable again after case guard desugaring..");
    for group in ty_check_results.iter_mut() {
        for (id, top_lvl_fn) in group.iter_mut() {
            top_lvl_fn.typed_expr = normalize_case_scrutinee(&mut v_var_ns, &top_lvl_fn.typed_expr);
        }
    }

    // <<--- type preserving passes

    println!("normalized/desugared --->>");
    for group in ty_check_results.iter() {
        for (id, top_lvl_fn) in group.iter() {
            print!("{}", top_lvl_fn.to_doc());
            println!();
        }
    }
    println!("<<--- normalized/desugared");

    // [WIP]
    let core_top_level_groups: Vec<_> = ty_check_results
        .iter()
        .map(|group| {
            let core_top_level_group = core_typed_top_level_function_group(group);
            core_top_level_group
        })
        .collect();

    Ok(())
}

fn insert_declared_fun_signatures(
    items: &[TopLevelItem],
    ty_env: &TyEnv,
    env: &mut EnvVVarToTyScheme,
    declared_function_type_schemes: &mut BTreeMap<String, TyScheme>,
    ty_var_ns: &mut TyVarNameSupply,
) {
    for i in items.iter() {
        if let TopLevelItem::FunctionSignature(sig) = i {
            if let ConcreteToken::Iden(name) = &sig.identifier.token {
                let ty_scheme = build_scheme_from_signature(sig, ty_env, ty_var_ns);
                declared_function_type_schemes.insert(name.clone(), ty_scheme.clone());
                let v_var = VVar::Named(VVarName {
                    token: sig.identifier.token.clone(),
                    loc: Some(sig.identifier.loc.clone()),
                    builtin: None,
                });
                env.insert(v_var, ty_scheme);
            }
        }
    }
}

/// build a type scheme from a declared function signature
/// by generalizing free user-defined type variables that are not ADT names
/// in the type environment
fn build_scheme_from_signature(sig: &FnSig, ty_env: &TyEnv, ns: &mut TyVarNameSupply) -> TyScheme {
    let ty_expr = lower_type_annot_to_ty_expr(&sig.ty);
    build_scheme_from_ty_expr(&ty_expr, ty_env, ns)
}

static TEST_PIPELINE_CONTENT_SIMPLE: &str = r###"
f x y z = 9 / (-7*x+y+z)
"###;

static TEST_PIPELINE_CONTENT_LET_SIMPLE: &str = r###"
f_let a =
 let x :: i64 = 1
     y :: i64 = 2
     z :: i64 = 5
 in
   x + y + a
"###;

static TEST_PIPELINE_CONTENT_LET_NESTED: &str = r###"
f_let_1 a =
 let x :: i64 = 1
     y = 2
     z = 5
 in
   let b = 10
       c = b * b
   in x + a + b * c
"###;

static TEST_PIPELINE_CONTENT_CASE: &str = r###"
ff x z =
 let y = 1-(case z of
             "a"         -> (0 * 5)
             "something" -> x
             _           -> 2
           )*7
 in y
"###;

static TEST_PIPELINE_CONTENT_CASE_NESTED: &str = r###"
fff x z =
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
"###;

static TEST_PIPELINE_CONTENT_RECORD: &str = r###"
data T B { // record
  v :: i64,
  x :: B,
}
"###;

static TEST_PIPELINE_CONTENT_PATTERN_SUM_TYPE: &str = r###"
data Tree Tx =
  Leaf i64 Tx
  | Node Tree Tree

f x = case x of
        Leaf num num_float -> num_float * 7.0
        _ -> 0.0
"###;

static TEST_PIPELINE_CONTENT_POLYMORPHIC_SUM_TYPE: &str = r###"
data Tree Tx =
  Leaf i64 Tx
  | Node Tree Tree

f x = case x of
        Leaf a b -> 1
        _ -> 2
"###;

static TEST_PIPELINE_CONTENT_FUNCTION_DEPENDENCE: &str = r###"
f4 yy zz = yy * (f5 zz)

f3 x =
  let a = f4
      b :: i64 = 7
  in
    a b x

f5 j = j
"###;

static TEST_PIPELINE_CONTENT_FUNCTION_RECURSION: &str = r###"
fib x = case x of
         0 -> 0
         1 -> 1
         _ -> fib (x-1) + (fib x-2)
"###;

static TEST_PIPELINE_CONTENT_FUNCTION_MUTUAL_RECURSION: &str = r###"
// contrived example
f accum x = f2 (accum + x) (x-1)

f2 accum x = case x of
              0 -> accum
              _ -> f accum x
"###;

static TEST_PIPELINE_CONTENT_PATTERN_RANGE: &str = r###"
f x = case x of
        0..7 -> True
        _ -> False
"###;

static TEST_PIPELINE_CONTENT_CONSTRUCTOR_RECORD: &str = r###"
data T B { // record
  v :: i64,
  x :: B,
}

f x = case x of
        0..7 -> T {v= 0, x= 10}
        _ -> T {v= 0, x= 20}
"###;

static TEST_PIPELINE_CONTENT_PATTERN_RECORD_TYPE: &str = r###"
data T B { // record
  v :: i64,
  x :: B,
}

f y = case y of
        T {x=bound_var, ..} -> bound_var
        _ -> 100
"###;

static TEST_PIPELINE_CONTENT_PATTERN_RECORD_TYPE_NESTED: &str = r###"
data N {
  y :: f64,
}

data T B { // record
  v :: i64,
  x :: B,
}

// infer that y :: (T N)
f y = case y of
        T { x=N {y=bounded_var,}, ..} -> bounded_var
        _ -> 100.0
"###;

static TEST_PIPELINE_CONTENT_FUNCTION_SIGNATURE: &str = r###"
f :: i64 -> i64 -> i64
f x y = x * y + 5

g :: a -> a
g x = x
"###;

static TEST_PIPELINE_CONTENT_PATTERN_NORMALIZATION_FUNC_PARAM: &str = r###"
data Maybe T
  = Just T
  | Nothing

f (Just x) True = x + 5
"###;

static TEST_PIPELINE_CONTENT_PATTERN_NORMALIZATION_LET_EXPR: &str = r###"
data Maybe T
  = Just T
  | Nothing

f x y = let Just(a) = x
            Just(b) = y
        in a + b
"###;

static TEST_PIPELINE_CONTENT_PATTERN_NORMALIZATION_CASE_SCRUTINEE: &str = r###"
data Maybe T
  = Just T
  | Nothing

f_get y = Just 5
f x = case let Just(y) = x in y*10 of
        50 -> 1
        _ -> 0
"###;

static TEST_PIPELINE_CONTENT_PATTERN_NORMALIZATION_CONSTRUCTOR: &str = r###"
data Pair { x :: i64, y :: i64 }

f a = let Pair { x, y } = a
      in x * y
"###;

static TEST_PIPELINE_CONTENT_CASE_SCRUTINEE_NORMALIZATION: &str = r###"
data Maybe T
  = Just T
  | Nothing

f_get y = Just(y + 5)
f x = case f_get x of
        Just(val) -> 1
        _ -> 0
"###;

static TEST_PIPELINE_CONTENT_DESUGAR_CASE_GUARD: &str = r###"
f x = case x of
        y | y>10 -> 100
        10       ->  50
        _        ->   0
"###;

static TEST_PIPELINE_CONTENT_DESUGAR_CASE_LITERAL_RANGE_INT: &str = r###"
f x = case x of
        0..10 -> 100
        _     ->   0
"###;

static TEST_PIPELINE_CONTENT_DESUGAR_CASE_LITERAL_RANGE_FLOAT: &str = r###"
f x = case x of
        0.0 .. 10.5 -> 100
        _           ->   0
"###;

static TEST_PIPELINE_CONTENT_DESUGAR_CASE_LITERAL_RANGE_INT_AND_GUARD: &str = r###"
f x = case x of
        x | x > 15 -> 200
        0..10      -> 100
        _          ->   0
"###;

static TEST_PIPELINE_CONTENT_DESUGAR_CASE_LITERAL_PATTERN: &str = r###"
f x = case x of
        12         ->  50
        _          ->   0
"###;

static TEST_PIPELINE_CONTENT_DESUGAR_CASE_LITERALS_IN_LET_DEFS: &str = r###"
ff z =
 let y = case z of
           0..10         -> 10
           120           -> 20
           _             -> 30
 in y
"###;

static TEST_PIPELINE_CONTENT_UNIT_TYPE: &str = r###"
f_get Unit = Unit
f = f_get Unit
"###;

static TEST_PIPELINE_CONTENT_CASE_UNIT_CONSTRUCTOR_PATTERN: &str = r###"
f x = case x of
        Just(Unit) -> 7
        _ -> 10
"###;

static TEST_PIPELINE_CONTENT_UNIT_TYPE_SIGNATURE: &str = r###"
f_get :: Unit -> Unit
f_get Unit = Unit
"###;

static TEST_PIPELINE_CONTENT_BOOL: &str = r###"
f_get :: Bool -> Bool
f_get x = case x of
            True  -> False
            False -> True
"###;

static TEST_PIPELINE_CONTENT_POLYMORPHIC_FUNCTION: &str = r###"
id x = x

f x = case x of
       10  -> id 10
       _   -> 20

f2 x = case x of
         Maybe.Nothing  -> id x
         _              -> Maybe.Just(10)
"###;

static TEST_PIPELINE_CONTENT_LOCAL_POLYMORPHIC_FUNCTION: &str = r###"
f x = let id x = x
      in
        case x of
         Maybe.Nothing  -> id x
         _              -> Maybe.Just(10)
"###;

static TEST_PIPELINE_CONTENT_PARTIALLY_APPLIED_BINDING: &str = r###"
f x = let first a b = a
          partial = first x
      in
          partial
"###;

static TEST_PIPELINE_CONTENT_LET_BINDING_TYPE_ANNOTATION: &str = r###"
f x =
      // [todo] support type annotation of let definition
      // let id :: (a ->a)
      let id x = x
      in
          id x
"###;

#[test]
fn test_pipeline_simple() {
    match compile(TEST_PIPELINE_CONTENT_SIMPLE) {
        Err(e) => {
            println!("{:?}", e);
        }
        _ => {}
    }
}

#[test]
fn test_pipeline_let_simple() {
    match compile(TEST_PIPELINE_CONTENT_LET_SIMPLE) {
        Err(e) => {
            println!("{:?}", e);
        }
        _ => {}
    }
}

#[test]
fn test_pipeline_let_nested() {
    match compile(TEST_PIPELINE_CONTENT_LET_NESTED) {
        Err(e) => {
            println!("{:?}", e);
        }
        _ => {}
    }
}

#[test]
fn test_pipeline_case() {
    match compile(TEST_PIPELINE_CONTENT_CASE) {
        Err(e) => {
            println!("{:?}", e);
        }
        _ => {}
    }
}

#[test]
fn test_pipeline_case_nested() {
    match compile(TEST_PIPELINE_CONTENT_CASE_NESTED) {
        Err(e) => {
            println!("{:?}", e);
        }
        _ => {}
    }
}

#[test]
fn test_pipeline_record() {
    match compile(TEST_PIPELINE_CONTENT_RECORD) {
        Err(e) => {
            println!("{:?}", e);
        }
        _ => {}
    }
}

#[test]
fn test_pipeline_pattern_sum_type() {
    match compile(TEST_PIPELINE_CONTENT_PATTERN_SUM_TYPE) {
        Err(e) => {
            println!("{:?}", e);
        }
        _ => {}
    }
}

#[test]
fn test_pipeline_polymorphic_sum_type() {
    match compile(TEST_PIPELINE_CONTENT_POLYMORPHIC_SUM_TYPE) {
        Err(e) => {
            println!("{:?}", e);
        }
        _ => {}
    }
}

#[test]
fn test_pipeline_function_dependence() {
    match compile(TEST_PIPELINE_CONTENT_FUNCTION_DEPENDENCE) {
        Err(e) => {
            println!("{:?}", e);
        }
        _ => {}
    }
}

#[test]
fn test_pipeline_function_recursion() {
    match compile(TEST_PIPELINE_CONTENT_FUNCTION_RECURSION) {
        Err(e) => {
            println!("{:?}", e);
        }
        _ => {}
    }
}

#[test]
fn test_pipeline_function_mutual_recursion() {
    match compile(TEST_PIPELINE_CONTENT_FUNCTION_MUTUAL_RECURSION) {
        Err(e) => {
            println!("{:?}", e);
        }
        _ => {}
    }
}

#[test]
fn test_pipeline_pattern_range() {
    match compile(TEST_PIPELINE_CONTENT_PATTERN_RANGE) {
        Err(e) => {
            println!("{:?}", e);
        }
        _ => {}
    }
}

#[test]
fn test_pipeline_constructor_record() {
    match compile(TEST_PIPELINE_CONTENT_CONSTRUCTOR_RECORD) {
        Err(e) => {
            println!("{:?}", e);
        }
        _ => {}
    }
}

#[test]
fn test_pipeline_pattern_record_type() {
    match compile(TEST_PIPELINE_CONTENT_PATTERN_RECORD_TYPE) {
        Err(e) => {
            println!("{:?}", e);
        }
        _ => {}
    }
}

#[test]
fn test_pipeline_pattern_record_type_nested() {
    match compile(TEST_PIPELINE_CONTENT_PATTERN_RECORD_TYPE_NESTED) {
        Err(e) => {
            println!("{:?}", e);
        }
        _ => {}
    }
}

#[test]
fn test_pipeline_function_signature() {
    match compile(TEST_PIPELINE_CONTENT_FUNCTION_SIGNATURE) {
        Err(e) => {
            println!("{:?}", e);
        }
        _ => {}
    }
}

#[test]
fn test_pipeline_pattern_normalization_func_param() {
    match compile(TEST_PIPELINE_CONTENT_PATTERN_NORMALIZATION_FUNC_PARAM) {
        Err(e) => {
            println!("{:?}", e);
        }
        _ => {}
    }
}

#[test]
fn test_pipeline_pattern_normalization_let_expr() {
    match compile(TEST_PIPELINE_CONTENT_PATTERN_NORMALIZATION_LET_EXPR) {
        Err(e) => {
            println!("{:?}", e);
        }
        _ => {}
    }
}

#[test]
fn test_pipeline_pattern_normalization_case_scrutinee() {
    match compile(TEST_PIPELINE_CONTENT_PATTERN_NORMALIZATION_CASE_SCRUTINEE) {
        Err(e) => {
            println!("{:?}", e);
        }
        _ => {}
    }
}

#[test]
fn test_pipeline_pattern_normalization_constructor() {
    match compile(TEST_PIPELINE_CONTENT_PATTERN_NORMALIZATION_CONSTRUCTOR) {
        Err(e) => {
            println!("{:?}", e);
        }
        _ => {}
    }
}

#[test]
fn test_pipeline_case_scrutinee_normalization() {
    match compile(TEST_PIPELINE_CONTENT_CASE_SCRUTINEE_NORMALIZATION) {
        Err(e) => {
            println!("{:?}", e);
        }
        _ => {}
    }
}

#[test]
fn test_pipeline_desugar_case_guard() {
    match compile(TEST_PIPELINE_CONTENT_DESUGAR_CASE_GUARD) {
        Err(e) => {
            println!("{:?}", e);
        }
        _ => {}
    }
}

#[test]
fn test_pipeline_desugar_case_literal_range_int() {
    match compile(TEST_PIPELINE_CONTENT_DESUGAR_CASE_LITERAL_RANGE_INT) {
        Err(e) => {
            println!("{:?}", e);
        }
        _ => {}
    }
}

#[test]
fn test_pipeline_desugar_case_literal_range_float() {
    match compile(TEST_PIPELINE_CONTENT_DESUGAR_CASE_LITERAL_RANGE_FLOAT) {
        Err(e) => {
            println!("{:?}", e);
        }
        _ => {}
    }
}

#[test]
fn test_pipeline_desugar_case_literal_range_int_and_guard() {
    match compile(TEST_PIPELINE_CONTENT_DESUGAR_CASE_LITERAL_RANGE_INT_AND_GUARD) {
        Err(e) => {
            println!("{:?}", e);
        }
        _ => {}
    }
}

#[test]
fn test_pipeline_desugar_case_literal_pattern() {
    match compile(TEST_PIPELINE_CONTENT_DESUGAR_CASE_LITERAL_PATTERN) {
        Err(e) => {
            println!("{:?}", e);
        }
        _ => {}
    }
}

#[test]
fn test_pipeline_desugar_case_literals_in_let_defs() {
    match compile(TEST_PIPELINE_CONTENT_DESUGAR_CASE_LITERALS_IN_LET_DEFS) {
        Err(e) => {
            println!("{:?}", e);
        }
        _ => {}
    }
}

#[test]
fn test_pipeline_unit_type() {
    match compile(TEST_PIPELINE_CONTENT_UNIT_TYPE) {
        Err(e) => {
            println!("{:?}", e);
        }
        _ => {}
    }
}

#[test]
fn test_pipeline_case_unit_constructor_pattern() {
    match compile(TEST_PIPELINE_CONTENT_CASE_UNIT_CONSTRUCTOR_PATTERN) {
        Err(e) => {
            println!("{:?}", e);
        }
        _ => {}
    }
}

#[test]
fn test_pipeline_unit_type_signature() {
    match compile(TEST_PIPELINE_CONTENT_UNIT_TYPE_SIGNATURE) {
        Err(e) => {
            println!("{:?}", e);
        }
        _ => {}
    }
}

#[test]
fn test_pipeline_bool() {
    match compile(TEST_PIPELINE_CONTENT_BOOL) {
        Err(e) => {
            println!("{:?}", e);
        }
        _ => {}
    }
}

#[test]
fn test_pipeline_polymorphic_function() {
    match compile(TEST_PIPELINE_CONTENT_POLYMORPHIC_FUNCTION) {
        Err(e) => {
            println!("{:?}", e);
        }
        _ => {}
    }
}

#[test]
fn test_pipeline_local_polymorphic_function() {
    match compile(TEST_PIPELINE_CONTENT_LOCAL_POLYMORPHIC_FUNCTION) {
        Err(e) => {
            println!("{:?}", e);
        }
        _ => {}
    }
}

#[test]
fn test_pipeline_partially_applied_binding() {
    match compile(TEST_PIPELINE_CONTENT_PARTIALLY_APPLIED_BINDING) {
        Err(e) => {
            println!("{:?}", e);
        }
        _ => {}
    }
}

#[test]
fn test_pipeline_let_binding_type_annotation() {
    match compile(TEST_PIPELINE_CONTENT_LET_BINDING_TYPE_ANNOTATION) {
        Err(e) => {
            println!("{:?}", e);
        }
        _ => {}
    }
}
