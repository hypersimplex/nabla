use crate::builtin::types::*;
use crate::builtin::values::*;
use crate::core::core_err::*;
use crate::core::core_ir::*;
use crate::core::core_ty_con_env::*;
use crate::normalize::case_constructor_unnest::*;
use crate::normalize::case_constructor_wildcard::*;
use crate::normalize::case_default::*;
use crate::normalize::case_guard::*;
use crate::normalize::case_scrutinee::*;
use crate::normalize::literal_pattern::*;
use crate::normalize::literal_range_pattern::*;
use crate::normalize::pattern::*;
use crate::normalize::record_to_product::*;
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
use crate::typecheck::ty_con_env::*;
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
    Core(CoreError),
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

impl From<CoreError> for CompileError {
    fn from(e: CoreError) -> Self {
        Self::Core(e)
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

    // function name -> top-level declared signature type expressions
    let mut declared_function_signatures: BTreeMap<String, TyExpr> = BTreeMap::new();
    insert_declared_fun_signatures(
        top_level.0.as_slice(),
        &ty_env,
        &mut env_v_var_to_ty_scheme,
        &mut declared_function_signatures,
        &mut ty_var_ns,
    )?;

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

    // check declared function signature has a corresponding definition
    {
        let defined_fn_names: BTreeSet<&str> = seen_top_level_fn_names
            .iter()
            .filter_map(|vvar| match vvar {
                VVar::Named(VVarName {
                    token: ConcreteToken::Iden(name),
                    ..
                }) => Some(name.as_str()),
                _ => None,
            })
            .collect();

        for sig_name in declared_function_signatures.keys() {
            if !defined_fn_names.contains(sig_name.as_str()) {
                return Err(TyError::MissingFunctionDefinition(sig_name.clone()))?;
            }
        }
    }

    println!("rename user introduced local variables and patterns to uniquify..");
    // top level bindings stays constant (identity map)
    let vvars_outer_scope: BTreeMap<VVar, VVar> = env_v_var_to_ty_scheme
        .iter()
        .map(|(key, _val)| (key.clone(), key.clone()))
        .collect();

    for (idx, (_var_binder_for_fn, expr)) in funcs.iter_mut() {
        *expr = rename_var_unique(&mut v_var_ns, &vvars_outer_scope, expr);
    }

    // collect binding, definition, and optional signature for each top level
    // function
    let def_indices: Vec<usize> = funcs.keys().copied().collect();
    let defs: Vec<(VPattern, VExpr, Option<TyExpr>)> = def_indices
        .iter()
        .map(|idx| {
            let (var_binder_to_fn, vexpr) = funcs.get(idx).unwrap().clone();

            let opt_signature = match &var_binder_to_fn {
                VVar::Named(VVarName {
                    token: ConcreteToken::Iden(name),
                    ..
                }) => declared_function_signatures.get(name.as_str()).cloned(),
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
        &BTreeMap::new(),
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

    println!("desugar patterns to appear only in case alt pattern binders..");
    for group in ty_check_results.iter_mut() {
        for (id, top_lvl_fn) in group.iter_mut() {
            top_lvl_fn.typed_expr = desugar_pattern(&mut v_var_ns, &top_lvl_fn.typed_expr);
        }
    }

    println!("normalize case scrutinee to be simple variable..");
    for group in ty_check_results.iter_mut() {
        for (id, top_lvl_fn) in group.iter_mut() {
            top_lvl_fn.typed_expr = normalize_case_scrutinee(&mut v_var_ns, &top_lvl_fn.typed_expr);
        }
    }

    // desugar record patterns and record constructor expressions to positional product forms
    println!("desugar record to product type");
    for group in ty_check_results.iter_mut() {
        for (id, top_lvl_fn) in group.iter_mut() {
            top_lvl_fn.typed_expr = desugar_record_to_product(&ty_env, &top_lvl_fn.typed_expr);
        }
    }

    // [todo] compile pattern matching using decision tree for generating more
    // efficient code

    // [todo, optional] reduce code size by introducing join points; reference:
    // Compiling without continuations (2017)

    println!("normalize constructor wildcards to fresh variable binders..");
    for group in ty_check_results.iter_mut() {
        for (id, top_lvl_fn) in group.iter_mut() {
            top_lvl_fn.typed_expr =
                normalize_case_constructor_wildcard(&mut v_var_ns, &top_lvl_fn.typed_expr);
        }
    }

    println!("flatten case expressions' nested patterns in constructor arguments..");
    for group in ty_check_results.iter_mut() {
        for (id, top_lvl_fn) in group.iter_mut() {
            top_lvl_fn.typed_expr =
                normalize_case_constructor_unnest(&mut v_var_ns, &ty_env, &top_lvl_fn.typed_expr);
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

    println!("normalize case variable patterns to let and wildcards..");
    for group in ty_check_results.iter_mut() {
        for (id, top_lvl_fn) in group.iter_mut() {
            top_lvl_fn.typed_expr = normalize_case_default(&top_lvl_fn.typed_expr);
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

    println!("conversion to core --->>");

    let core_ty_con_env: CoreTyConEnv = (&ty_env).into();

    let mut core_top_level_groups: Vec<CoreTopLevelBindingGroup> = Vec::new();
    for group in ty_check_results.iter() {
        let core_top_level_group =
            core_typed_top_level_function_group(&core_ty_con_env, &mut v_var_ns, group);
        match core_top_level_group {
            Ok(x) => core_top_level_groups.push(x),
            Err(e) => return Err(e)?,
        }
    }

    for core_top_level_group in core_top_level_groups.iter() {
        for core_top_lvl_binding in core_top_level_group.0.iter() {
            print!("{}", core_top_lvl_binding.to_doc());
            println!();
        }
    }

    println!("<<--- conversion to core");

    // [WIP, todo]
    // - perform any transformations in core
    todo!("transform core IR to STG IR");
    todo!("codegen from STG IR");
    // - optionally invoke runtime

    Ok(())
}

fn insert_declared_fun_signatures(
    items: &[TopLevelItem],
    ty_env: &TyConEnv,
    env: &mut EnvVVarToTyScheme,
    declared_function_signatures: &mut BTreeMap<String, TyExpr>,
    ty_var_ns: &mut TyVarNameSupply,
) -> Result<(), TyError> {
    for i in items.iter() {
        if let TopLevelItem::FunctionSignature(sig) = i {
            if let ConcreteToken::Iden(name) = &sig.identifier.token {
                let ty_expr = lower_type_annot_to_ty_expr(&sig.ty);
                let ty_scheme = build_scheme_from_ty_expr(&ty_expr, ty_env, ty_var_ns)?;
                declared_function_signatures.insert(name.clone(), ty_expr);
                let v_var = VVar::Named(VVarName {
                    token: sig.identifier.token.clone(),
                    loc: Some(sig.identifier.loc.clone()),
                    builtin: None,
                });
                env.insert(v_var, ty_scheme);
            }
        }
    }
    Ok(())
}

static TEST_PIPELINE_CONTENT_SIMPLE: &str = r###"
f x y z = 9 / (-7*x+y+z)
"###;

static TEST_PIPELINE_CONTENT_LET_SIMPLE: &str = r###"
f_let a =
 let x :: Int64 = Int64 1
     y :: Int64 = Int64 2
     z :: Int64 = Int64 5
 in
   x + y + a
"###;

static TEST_PIPELINE_CONTENT_LET_NESTED: &str = r###"
f_let_1 a =
 let x :: Int64 = Int64 1
     y = Int64 2
     z = Int64 5
 in
   let b = Int64 10
       c = b * b
   in x + a + b * c
"###;

static TEST_PIPELINE_CONTENT_CASE: &str = r###"
ff x z =
 let y = (Int64 1) - (case z of
                       "a"         -> (Int64 0 * Int64 5)
                       "something" -> x
                       _           -> Int64 2
                     ) * Int64 7
 in y
"###;

static TEST_PIPELINE_CONTENT_CASE_NESTED: &str = r###"
fff x z =
 let y = (
          case z of
            "a"         -> (Int64 0 * Int64 5)
            "something" -> case x of
                             Int64 0 -> Int64 10
                             Int64 1 -> Int64 11
                             _       -> Int64 2 * x
            _           -> Int64 2
         ) * Int64 7
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
  | Node (Tree Tx) (Tree Tx)

f x = case x of
        Leaf num num_float -> num_float * 7.0
        _ -> 0.0
"###;

static TEST_PIPELINE_CONTENT_POLYMORPHIC_SUM_TYPE: &str = r###"
data Tree Tx =
  Leaf i64 Tx
  | Node (Tree Tx) (Tree Tx)

f x = case x of
        Leaf a b -> 1
        _ -> 2
"###;

static TEST_PIPELINE_CONTENT_FUNCTION_DEPENDENCE: &str = r###"
f4 yy zz = yy * (f5 zz)

f3 x =
  let a = f4
      b :: Int64 = Int64 7
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
f (Just x) True = x + 5
"###;

static TEST_PIPELINE_CONTENT_PATTERN_NORMALIZATION_LET_EXPR: &str = r###"
f x y = let Just(a) = x
            Just(b) = y
        in a + b
"###;

static TEST_PIPELINE_CONTENT_PATTERN_NORMALIZATION_CASE_SCRUTINEE: &str = r###"
f_get y = Just (Int64 5)
f x = case let Just(y) = x in y * (Int64 10) of
        _ -> Int64 0
"###;

static TEST_PIPELINE_CONTENT_PATTERN_NORMALIZATION_CONSTRUCTOR: &str = r###"
data Pair { x :: Int64, y :: Int64 }

f a = let Pair { x, y } = a
      in x * y
"###;

static TEST_PIPELINE_CONTENT_CASE_SCRUTINEE_NORMALIZATION: &str = r###"
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

// support for string ranges is yet to be determine, but one
// way is to be lexicographical ordering
static TEST_PIPELINE_CONTENT_DESUGAR_CASE_LITERAL_RANGE_STRING: &str = r###"
f x = case x of
        "a".."z" -> 100
        _        ->   0
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
           0..10         -> Int64 10
           120           -> Int64 20
           _             -> Int64 30
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

#[cfg(test)]
fn assert_compiles_or_stg_todo(content: &str) {
    let res = std::panic::catch_unwind(|| compile(content));
    assert!(
        res.as_ref().is_ok_and(|r| r.is_ok())
            || res.as_ref().is_err_and(|e| {
                e.downcast_ref::<&str>()
                    .map_or(false, |m| m.contains("transform core IR to STG IR"))
                    || e.downcast_ref::<String>()
                        .map_or(false, |m| m.contains("transform core IR to STG IR"))
            }),
        "compilation failed before reaching core: {:?}",
        res
    );
}

#[test]
fn test_pipeline_simple() {
    assert_compiles_or_stg_todo(TEST_PIPELINE_CONTENT_SIMPLE);
}

#[test]
fn test_pipeline_let_simple() {
    assert_compiles_or_stg_todo(TEST_PIPELINE_CONTENT_LET_SIMPLE);
}

#[test]
fn test_pipeline_let_nested() {
    assert_compiles_or_stg_todo(TEST_PIPELINE_CONTENT_LET_NESTED);
}

#[test]
fn test_pipeline_case() {
    assert_compiles_or_stg_todo(TEST_PIPELINE_CONTENT_CASE);
}

#[test]
fn test_pipeline_case_nested() {
    assert_compiles_or_stg_todo(TEST_PIPELINE_CONTENT_CASE_NESTED);
}

#[test]
fn test_pipeline_record() {
    assert_compiles_or_stg_todo(TEST_PIPELINE_CONTENT_RECORD);
}

#[test]
fn test_pipeline_pattern_sum_type() {
    assert_compiles_or_stg_todo(TEST_PIPELINE_CONTENT_PATTERN_SUM_TYPE);
}

#[test]
fn test_pipeline_polymorphic_sum_type() {
    assert_compiles_or_stg_todo(TEST_PIPELINE_CONTENT_POLYMORPHIC_SUM_TYPE);
}

#[test]
fn test_pipeline_function_dependence() {
    assert_compiles_or_stg_todo(TEST_PIPELINE_CONTENT_FUNCTION_DEPENDENCE);
}

#[test]
fn test_pipeline_function_recursion() {
    assert_compiles_or_stg_todo(TEST_PIPELINE_CONTENT_FUNCTION_RECURSION);
}

#[test]
fn test_pipeline_function_mutual_recursion() {
    assert_compiles_or_stg_todo(TEST_PIPELINE_CONTENT_FUNCTION_MUTUAL_RECURSION);
}

#[test]
fn test_pipeline_pattern_range() {
    assert_compiles_or_stg_todo(TEST_PIPELINE_CONTENT_PATTERN_RANGE);
}

#[test]
fn test_pipeline_constructor_record() {
    assert_compiles_or_stg_todo(TEST_PIPELINE_CONTENT_CONSTRUCTOR_RECORD);
}

#[test]
fn test_pipeline_pattern_record_type() {
    assert_compiles_or_stg_todo(TEST_PIPELINE_CONTENT_PATTERN_RECORD_TYPE);
}

#[test]
fn test_pipeline_pattern_record_type_nested() {
    assert_compiles_or_stg_todo(TEST_PIPELINE_CONTENT_PATTERN_RECORD_TYPE_NESTED);
}

#[test]
fn test_pipeline_function_signature() {
    assert_compiles_or_stg_todo(TEST_PIPELINE_CONTENT_FUNCTION_SIGNATURE);
}

#[test]
fn test_pipeline_pattern_normalization_func_param() {
    assert_compiles_or_stg_todo(TEST_PIPELINE_CONTENT_PATTERN_NORMALIZATION_FUNC_PARAM);
}

#[test]
fn test_pipeline_pattern_normalization_let_expr() {
    assert_compiles_or_stg_todo(TEST_PIPELINE_CONTENT_PATTERN_NORMALIZATION_LET_EXPR);
}

#[test]
fn test_pipeline_pattern_normalization_case_scrutinee() {
    assert_compiles_or_stg_todo(TEST_PIPELINE_CONTENT_PATTERN_NORMALIZATION_CASE_SCRUTINEE);
}

#[test]
fn test_pipeline_pattern_normalization_constructor() {
    assert_compiles_or_stg_todo(TEST_PIPELINE_CONTENT_PATTERN_NORMALIZATION_CONSTRUCTOR);
}

#[test]
fn test_pipeline_case_scrutinee_normalization() {
    assert_compiles_or_stg_todo(TEST_PIPELINE_CONTENT_CASE_SCRUTINEE_NORMALIZATION);
}

#[test]
fn test_pipeline_desugar_case_guard() {
    assert_compiles_or_stg_todo(TEST_PIPELINE_CONTENT_DESUGAR_CASE_GUARD);
}

#[test]
fn test_pipeline_desugar_case_literal_range_int() {
    assert_compiles_or_stg_todo(TEST_PIPELINE_CONTENT_DESUGAR_CASE_LITERAL_RANGE_INT);
}

#[test]
fn test_pipeline_desugar_case_literal_range_float() {
    assert_compiles_or_stg_todo(TEST_PIPELINE_CONTENT_DESUGAR_CASE_LITERAL_RANGE_FLOAT);
}

#[test]
fn test_pipeline_desugar_case_literal_range_string() {
    assert_compiles_or_stg_todo(TEST_PIPELINE_CONTENT_DESUGAR_CASE_LITERAL_RANGE_STRING);
}

#[test]
fn test_pipeline_desugar_case_literal_range_int_and_guard() {
    assert_compiles_or_stg_todo(TEST_PIPELINE_CONTENT_DESUGAR_CASE_LITERAL_RANGE_INT_AND_GUARD);
}

#[test]
fn test_pipeline_desugar_case_literal_pattern() {
    assert_compiles_or_stg_todo(TEST_PIPELINE_CONTENT_DESUGAR_CASE_LITERAL_PATTERN);
}

#[test]
fn test_pipeline_desugar_case_literals_in_let_defs() {
    assert_compiles_or_stg_todo(TEST_PIPELINE_CONTENT_DESUGAR_CASE_LITERALS_IN_LET_DEFS);
}

#[test]
fn test_pipeline_unit_type() {
    assert_compiles_or_stg_todo(TEST_PIPELINE_CONTENT_UNIT_TYPE);
}

#[test]
fn test_pipeline_case_unit_constructor_pattern() {
    assert_compiles_or_stg_todo(TEST_PIPELINE_CONTENT_CASE_UNIT_CONSTRUCTOR_PATTERN);
}

#[test]
fn test_pipeline_unit_type_signature() {
    assert_compiles_or_stg_todo(TEST_PIPELINE_CONTENT_UNIT_TYPE_SIGNATURE);
}

#[test]
fn test_pipeline_bool() {
    assert_compiles_or_stg_todo(TEST_PIPELINE_CONTENT_BOOL);
}

#[test]
fn test_pipeline_polymorphic_function() {
    assert_compiles_or_stg_todo(TEST_PIPELINE_CONTENT_POLYMORPHIC_FUNCTION);
}

#[test]
fn test_pipeline_local_polymorphic_function() {
    assert_compiles_or_stg_todo(TEST_PIPELINE_CONTENT_LOCAL_POLYMORPHIC_FUNCTION);
}

#[test]
fn test_pipeline_partially_applied_binding() {
    assert_compiles_or_stg_todo(TEST_PIPELINE_CONTENT_PARTIALLY_APPLIED_BINDING);
}

#[test]
fn test_pipeline_let_binding_type_annotation() {
    assert_compiles_or_stg_todo(TEST_PIPELINE_CONTENT_LET_BINDING_TYPE_ANNOTATION);
}

#[test]
fn test_pipeline_mutual_recursion_simple() {
    let content = r###"
f x = g (x + 1)
g y = f (y - 1) + 10
"###;
    assert_compiles_or_stg_todo(content);
}

#[test]
fn test_pipeline_mutual_recursion_rejects_unsound_polymorphism() {
    let content = r###"
f x = g (x + 1)
g y = f (y - 1) + 10
bad :: String
bad = f 0
"###;
    let res = compile(content);
    match res {
        Err(CompileError::Type(TyError::TypeConflict { ty1, ty2, .. })) => {
            let is_str = matches!(ty1, TyExpr::TyCon(TyCon::User(ref s)) if s == "String");
            let is_i64 = matches!(ty2, TyExpr::TyCon(TyCon::Builtin(TyConBuiltin::I64)));
            assert!(is_str && is_i64);
        }
        other => panic!(
            "expected TypeConflict between String and I64, got {:?}",
            other
        ),
    }
}

#[test]
fn test_pipeline_let_wildcard_pattern_annotation_type_conflict() {
    let mismatch = r###"
f x =
  let _ :: String = Int64 42
  in x + 1
"###;
    assert!(matches!(
        compile(mismatch),
        Err(CompileError::Type(TyError::TypeConflict { .. }))
    ));

    let matching = r###"
f x =
  let _ :: Int64 = Int64 42
  in x + 1
"###;
    assert_compiles_or_stg_todo(matching);
}

#[test]
fn test_pipeline_let_constructor_pattern_annotation_type_conflict() {
    let mismatch = r###"
f x =
  let Bool.True :: String = Bool.True
  in x
"###;
    assert!(matches!(
        compile(mismatch),
        Err(CompileError::Type(TyError::TypeConflict { .. }))
    ));

    let matching = r###"
f x =
  let Bool.True :: Bool = Bool.True
  in x
"###;
    assert_compiles_or_stg_todo(matching);
}

#[test]
fn test_pipeline_case_multiple_variables() {
    let content = r###"
data Tree = Leaf i64 | Node Tree Tree
f x = case x of
        Leaf a -> a
        y      -> 1
        z      -> 2
"###;
    // expected desugared output:
    //
    // first variable catch-all `y -> 1` desugars into `_ -> let y = x in 1`
    //
    // and other variable pattern is pruned
    //
    // f = \x ->
    //   case x of
    //     Tree.Leaf a -> a
    //     _ -> let y = x in 1
    assert_compiles_or_stg_todo(content);
}

#[test]
fn test_pipeline_case_wildcard_before_patterns() {
    let content = r###"
data Tree = Leaf i64 | Node Tree Tree
f x = case x of
        _      -> 0
        Leaf a -> a
        y      -> 1
"###;
    // expected desugared output:
    // Wildcard `_ -> 0` at the top prunes all other subsequent alternatives
    // alternatives
    //
    // single alternative simplification further transforms `case x of _ -> 0`
    // to `0`, so the function becomes `f = \x -> 0`
    assert_compiles_or_stg_todo(content);
}

#[test]
fn test_pipeline_case_variable_only() {
    let content = r###"
f x = case x of
        y -> y + 1
"###;
    // expected desugared output:
    // `y -> y + 1` desugars to `_ -> let y = x in y + 1`
    //
    // single alternative simplification transforms the function into:
    // `f = \x -> let y = x in y + 1`
    assert_compiles_or_stg_todo(content);
}

#[test]
fn test_pipeline_case_wildcard_only() {
    let content = r###"
f x = case x of
        _ -> 42
"###;
    // expected desugared output:
    // single alternative wildcard pattern transforms it into:
    //
    // `f = \x -> 42`
    assert_compiles_or_stg_todo(content);
}

#[test]
fn test_pipeline_pattern_constructor_wildcard() {
    let content = r###"
f m = case m of
        Just _  -> 1
        Nothing -> 0
"###;
    assert_compiles_or_stg_todo(content);
}

#[test]
fn test_pipeline_pattern_constructor_multiple_wildcards() {
    let content = r###"
data Triple A B C = Triple A B C

f t = case t of
        Triple _ _ _ -> 1
"###;
    assert_compiles_or_stg_todo(content);
}

#[test]
fn test_pipeline_pattern_constructor_mixed_wildcards() {
    let content = r###"
data Triple A B C = Triple A B C

f t = case t of
        Triple x _ z -> (+ x z)
"###;
    assert_compiles_or_stg_todo(content);
}

#[test]
fn test_pipeline_pattern_constructor_nested_wildcard() {
    let content = r###"
f m = case m of
        Just (Just _) -> 1
        _             -> 0
"###;
    assert_compiles_or_stg_todo(content);
}

#[test]
fn test_pipeline_pattern_nested_sum_type() {
    let content = r###"
f m = case m of
        Just (Just x) -> x
        Just Nothing  -> 0
        Nothing       -> 100
"###;
    assert_compiles_or_stg_todo(content);
}

#[test]
fn test_pipeline_pattern_nested_constructor_guard() {
    let content = r###"
f m = case m of
        Just (Just x) | x > 10 -> x
        Just (Just _)          -> 10
        Just Nothing           -> 0
        Nothing                -> 100
"###;
    assert_compiles_or_stg_todo(content);
}

#[test]
fn test_pipeline_pattern_nested_constructor_multiple_args_guard() {
    let content = r###"
data Pair A B = Pair A B

f p = case p of
        Pair (Just x) (Just y) | x > y -> x
        Pair (Just x) (Just _)          -> x
        _                               -> 0
"###;
    assert_compiles_or_stg_todo(content);
}

#[test]
fn test_pipeline_pattern_nested_constructor_deep_guard() {
    let content = r###"
data Box A = Box A

f b = case b of
        Box (Box (Box x)) | x > 0 -> x
        _                         -> 0
"###;
    assert_compiles_or_stg_todo(content);
}

#[test]
fn test_pipeline_pattern_nested_range_in_constructor_with_guard() {
    let content = r###"
f m = case m of
        Just (1..5) | True -> 1
        _                  -> 0
"###;
    assert_compiles_or_stg_todo(content);
}

#[test]
fn test_pipeline_pattern_nested_range_in_constructor() {
    let content = r###"
f m = case m of
        Just (1..5) -> 1
        _           -> 0
"###;
    assert_compiles_or_stg_todo(content);
}

#[test]
fn test_pipeline_pattern_nested_literal_in_constructor() {
    let content = r###"
f m = case m of
        Just 42 -> 1
        _       -> 0
"###;
    assert_compiles_or_stg_todo(content);
}

#[test]
fn test_pipeline_pattern_nested_range_in_record() {
    let content = r###"
data Item {
    val :: i64,
}

f item = case item of
    Item { val = 1..10 } -> 1
    _                    -> 0
"###;
    assert_compiles_or_stg_todo(content);
}

#[test]
fn test_pipeline_case_scrutinee_polymorphic_variable() {
    let content = r###"
none = Maybe.Nothing

f = case none of
      Maybe.Nothing -> 0
      _             -> 1
"###;
    assert_compiles_or_stg_todo(content);
}

#[test]
fn test_pipeline_case_scrutinee_compound_expression() {
    let content = r###"
g x = Maybe.Just x

f x = case g x of
        Maybe.Just v  -> v
        Maybe.Nothing -> 0
"###;
    assert_compiles_or_stg_todo(content);
}

#[test]
fn test_pipeline_case_guard_typing_error() {
    // expected failure in type inference f case expression with multiple
    // alternatives and guards
    //
    // `f` is expected to be rejected with a type conflict
    let content = r###"
f z = case 0 of
        _ | (z + 1) == 2 -> 10
        _ | z == (String "hello") -> 20
"###;

    let res = compile(content);
    match res {
        Err(CompileError::Type(TyError::TypeConflict { ty1, ty2, .. })) => {
            let is_i64 = |t: &TyExpr| matches!(t, TyExpr::TyCon(TyCon::Builtin(TyConBuiltin::I64)));
            let is_str = |t: &TyExpr| matches!(t, TyExpr::TyCon(TyCon::User(s)) if s == "String");
            let is_i64_and_str = (is_i64(&ty1) && is_str(&ty2)) || (is_str(&ty1) && is_i64(&ty2));
            assert!(
                is_i64_and_str,
                "expect guards to fail with type conflict between i64 and String"
            );
        }
        other => panic!(
            "expected type conflict between String and i64, got {:?}",
            other
        ),
    }
}

#[test]
fn test_pipeline_duplicate_adt_rejected() {
    let content_sum = r###"
data Foo = A
data Foo = B
f x = 0
"###;
    assert!(matches!(
        compile(content_sum),
        Err(CompileError::Type(TyError::AdtError(_)))
    ));

    let content_rec = r###"
data Bar { x :: i64 }
data Bar { y :: i64 }
f x = 0
"###;
    assert!(matches!(
        compile(content_rec),
        Err(CompileError::Type(TyError::AdtError(_)))
    ));
}

#[test]
fn test_pipeline_builtin_adt_shadowing_rejected() {
    let content_bool = r###"
data Bool = Yes | No
f x = 0
"###;
    assert!(matches!(
        compile(content_bool),
        Err(CompileError::Type(TyError::AdtError(_)))
    ));

    let content_unit = r###"
data Unit = CustomUnit
f x = 0
"###;
    assert!(matches!(
        compile(content_unit),
        Err(CompileError::Type(TyError::AdtError(_)))
    ));

    let content_maybe = r###"
data Maybe = CustomMaybe
f x = 0
"###;
    assert!(matches!(
        compile(content_maybe),
        Err(CompileError::Type(TyError::AdtError(_)))
    ));
}

#[test]
fn test_pipeline_builtin_type_parameter_shadowing_rejected() {
    for name in &[
        "i64", "f64", "Bytes", "String", "Int64", "Float64", "Bool", "Unit", "Maybe",
    ] {
        let content = format!("data Container {name} = Box {name}\nf x = 0\n");
        assert!(
            matches!(
                compile(&content),
                Err(CompileError::Type(TyError::AdtError(_)))
            ),
            "expected shadowing of builtin {name} as type parameter to be rejected"
        );
    }
}

#[test]
fn test_pipeline_duplicate_type_parameter_rejected() {
    let content = r###"
data Pair a a = Pair a a
f x = 0
"###;
    assert!(matches!(
        compile(content),
        Err(CompileError::Type(TyError::AdtError(_)))
    ));
}

// negative test
//
// declared polymorphic signature uses rigid type variables,
// preventing flexible unification with concrete types
#[test]
fn test_pipeline_polymorphic_signature_rejects_concrete_type() {
    let content = r###"
f :: a -> a
f x = 42
"###;
    assert!(matches!(
        compile(content),
        Err(CompileError::Type(TyError::TypeConflict { .. }))
    ));
}

// negative test
//
// distinct type variables in a polymorphic signature
// (e.g. `a` and `b`) must not unify with each other
#[test]
fn test_pipeline_polymorphic_signature_rejects_distinct_rigid_vars() {
    let content = r###"
f :: a -> b
f x = x
"###;
    assert!(matches!(
        compile(content),
        Err(CompileError::Type(TyError::TypeConflict { .. }))
    ));
}

// negative test for ensuring rigid type variables cannot escape their
// declaring scope
//
// `g` introduces rigid type variable `a`
//
// `g = x` constrains outer parameter `x` to have type `a`,
// then rigid type variable `a` leaks into `f`'s typing environment
#[test]
fn test_pipeline_polymorphic_signature_rejects_escaping_rigid_type() {
    let content = r###"
f x =
    let g :: a = x
    in 0
"###;
    assert!(matches!(
        compile(content),
        Err(CompileError::Type(TyError::TypeConflict {
            msg: Some(ref m),
            ..
        })) if m.contains("escapes")
    ));
}

// explicit polymorphic signature test
//
// repeated occurrences of the same type variable in an explicit type annotation
// (e.g. `a -> a`) are allowed and refer to the same type variable
#[test]
fn test_pipeline_polymorphic_signature_deduplication() {
    let content = r###"
id :: a -> a
id x = x
"###;
    assert_compiles_or_stg_todo(content);
}

// explicit polymorphic signature test
//
// multiple distinct type variables (e.g. `a -> b -> a`)
#[test]
fn test_pipeline_polymorphic_signature_multi_var() {
    let content = r###"
const :: a -> b -> a
const x y = x
"###;
    assert_compiles_or_stg_todo(content);
}

// explicit polymorphic signature test
//
// registered ADT constructors (e.g. `Maybe`) are preserved as
// concrete type constructors and not generalized as type variables
#[test]
fn test_pipeline_polymorphic_signature_with_adt() {
    let content = r###"
from_maybe :: a -> Maybe a -> a
from_maybe def m = case m of
  Just x -> x
  Nothing -> def
"###;
    assert_compiles_or_stg_todo(content);
}

// explicit polymorphic signature test
//
// nested arrow types (higher-order functions)
#[test]
fn test_pipeline_polymorphic_signature_higher_order() {
    let content = r###"
apply :: (a -> b) -> a -> b
apply f x = f x
"###;
    assert_compiles_or_stg_todo(content);
}

// explicit polymorphic signature test
//
// local let-bindings with polymorphic type annotations
// correctly generalize
#[test]
fn test_pipeline_let_binding_polymorphic_signature() {
    let content = r###"
f x =
    let id :: a -> a = \y -> y
    in id x
"###;
    assert_compiles_or_stg_todo(content);
}

#[test]
fn test_builtin_i64_from_f64() {
    let content = r###"
f = (i64_from_f64 3.14)
"###;
    assert_compiles_or_stg_todo(content);
}

#[test]
fn test_builtin_f64_from_i64() {
    let content = r###"
f = (f64_from_i64 42)
"###;
    assert_compiles_or_stg_todo(content);
}

#[test]
fn test_builtin_int64_from_float64() {
    let content = r###"
f = (int64_from_float64 (Float64 3.14))
"###;
    assert_compiles_or_stg_todo(content);
}

#[test]
fn test_builtin_float64_from_int64() {
    let content = r###"
f = (float64_from_int64 (Int64 42))
"###;
    assert_compiles_or_stg_todo(content);
}

#[test]
fn test_builtin_boxed_conversions_in_let() {
    let content = r###"
f x =
  let a = int64_from_float64 (Float64 3.14)
      b = float64_from_int64 (Int64 42)
  in a
"###;
    assert_compiles_or_stg_todo(content);
}

// negative test: int64_from_float64 rejects unboxed primitive float literal
#[test]
fn test_builtin_int64_from_float64_rejects_unboxed_literal() {
    let content = r###"
bad = int64_from_float64 3.14
"###;
    assert!(matches!(
        compile(content),
        Err(CompileError::Type(TyError::TypeConflict { .. }))
    ));
}

// negative test: float64_from_int64 rejects unboxed primitive int literal
#[test]
fn test_builtin_float64_from_int64_rejects_unboxed_literal() {
    let content = r###"
bad = float64_from_int64 42
"###;
    assert!(matches!(
        compile(content),
        Err(CompileError::Type(TyError::TypeConflict { .. }))
    ));
}

// negative test
//
// distinct user-defined types (e.g. `Foo` vs `Bar`) must not unify
#[test]
fn test_pipeline_user_defined_type_conflict() {
    let content = r###"
data Foo = Foo
data Bar = Bar

bad :: Foo -> Bar
bad x = x
"###;
    assert!(matches!(
        compile(content),
        Err(CompileError::Type(TyError::TypeConflict { .. }))
    ));
}

// negative test
//
// user-defined type and primitive type (e.g. `Foo` vs `i64`) must not unify
#[test]
fn test_pipeline_user_defined_vs_primitive_conflict() {
    let content = r###"
data Foo = Foo

bad :: Foo -> i64
bad x = x
"###;
    assert!(matches!(
        compile(content),
        Err(CompileError::Type(TyError::TypeConflict { .. }))
    ));
}

// user-defined types occurring at distinct source locations unify
#[test]
fn test_pipeline_user_defined_type_distinct_locations() {
    let content = r###"
data MyType = MyCon

id_my_type :: MyType -> MyType
id_my_type x = x
"###;
    assert_compiles_or_stg_todo(content);
}

// negative test
//
// referencing an undeclared/unbound variable must be reported as
// UnboundVariable with its name, rather than an internal error
#[test]
fn test_pipeline_unbound_variable_rejected() {
    let content = r###"
f x = unknown_var + 1
"###;
    assert!(matches!(
        compile(content),
        Err(CompileError::Type(TyError::UnboundVariable(_)))
    ));
}

// negative test: undeclared capitalized type in function signature must be reported as UnknownType
#[test]
fn test_pipeline_undeclared_type_in_signature_rejected() {
    let content = r###"
f :: Person -> i64
f x = 0
"###;
    assert!(matches!(
        compile(content),
        Err(CompileError::Type(TyError::UnknownType(_)))
    ));
}

// negative test: undeclared capitalized type nested as type parameter in signature
#[test]
fn test_pipeline_undeclared_type_nested_in_signature_rejected() {
    let content = r###"
f :: Maybe Person -> i64
f x = 0
"###;
    assert!(matches!(
        compile(content),
        Err(CompileError::Type(TyError::UnknownType(_)))
    ));
}

// negative test: undeclared capitalized type in let binding annotation must be reported as UnknownType
#[test]
fn test_pipeline_undeclared_type_in_let_annotation_rejected() {
    let content = r###"
f x =
  let y :: Person = Int64 1
  in y
"###;
    assert!(matches!(
        compile(content),
        Err(CompileError::Type(TyError::UnknownType(_)))
    ));
}

// negative test: function signature without accompanying definition
#[test]
fn test_pipeline_function_missing_definition() {
    let content = r###"
f :: i64 -> i64
"###;
    let res = compile(content);
    assert!(
        matches!(res, Err(CompileError::Type(TyError::MissingFunctionDefinition(ref name))) if name == "f"),
        "expected function to be rejected due to missing definition, but got {:?}",
        res
    );
}

// negative test: undeclared type in ADT constructor field
#[test]
fn test_pipeline_undeclared_type_in_adt_rejected() {
    let content = r###"
data Box = Box NoSuchType
f x = 0
"###;
    assert!(matches!(
        compile(content),
        Err(CompileError::Type(TyError::UnknownType(_)))
    ));
}

// negative test: unbound type variable in ADT constructor field
#[test]
fn test_pipeline_unbound_type_variable_in_adt_rejected() {
    let content = r###"
data Box = Box a
f x = 0
"###;
    assert!(matches!(
        compile(content),
        Err(CompileError::Type(TyError::UnknownType(_)))
    ));
}

// negative test: prefix operator in function application must be parenthesized
#[test]
fn test_pipeline_prefix_op_in_app_unparenthesized_rejected() {
    let content = r###"
f = g !x
"###;
    assert!(matches!(
        compile(content),
        Err(CompileError::Parse(ParseError::UnexpectedToken { .. }))
    ));
}

#[test]
fn test_pipeline_delimiter_closing_paren_at_column_zero() {
    let content = r###"
f x = (
  case x of
    0 -> 1
    _ -> 2
)
g y = y
"###;
    assert_compiles_or_stg_todo(content);
}

// negative test: mismatched delimiters
#[test]
fn test_pipeline_mismatched_delimiter_bracket_paren_rejected() {
    let content = r###"
f x = (
  1 + 2
]
"###;
    assert!(matches!(
        compile(content),
        Err(CompileError::Parse(ParseError::Message { .. }))
    ));
}

// negative test: unclosed delimiter
#[test]
fn test_pipeline_unclosed_delimiter_rejected() {
    let content = r###"
f x = (
  1 + 2
"###;
    assert!(matches!(
        compile(content),
        Err(CompileError::Parse(ParseError::Message { .. }))
    ));
}

// negative test: unparenthesized function header type annotation rejected by parser
#[test]
fn test_pipeline_unparenthesized_function_header_annotation_rejected() {
    let content = "f x :: i64 = x\n";
    assert!(matches!(
        compile(content),
        Err(CompileError::Parse(ParseError::UnexpectedToken { .. }))
    ));
}

// positive test: matching pattern and subexpression type annotations accepted
#[test]
fn test_pipeline_matching_type_annotations_accepted() {
    let content = r###"
f (x :: i64) = (x :: i64)
"###;
    assert_compiles_or_stg_todo(content);
}

// positive test: nullary constructor pattern does not greedily consume subsequent parameters
#[test]
fn test_pipeline_unparenthesized_constructor_pattern() {
    let content = r###"
f Nothing x = x
"###;
    assert_compiles_or_stg_todo(content);
}

// negative test: unparenthesized constructor taking one or more arguments should be rejected
#[test]
fn test_pipeline_unparenthesized_constructor_with_args_in_param_rejected() {
    let content = r###"
f Just x = x
"###;
    assert!(matches!(
        compile(content),
        Err(CompileError::Type(TyError::ArityMismatch {
            expected: 1,
            got: 0,
            ..
        }))
    ));
}

// negative test: subexpression type annotation mismatch rejected
#[test]
fn test_pipeline_call_argument_type_annotation_mismatch_rejected() {
    let content = r###"
f x = (\y -> y) (10 :: String)
"###;
    assert!(matches!(
        compile(content),
        Err(CompileError::Type(TyError::TypeConflict { .. }))
    ));
}

// negative test: type variable mismatch between parameter and body rejected
#[test]
fn test_pipeline_lambda_scoped_annotation_mismatch_rejected() {
    let content = r###"
f = (\ (x :: a) -> ("hello" :: a))
"###;
    assert!(matches!(
        compile(content),
        Err(CompileError::Type(TyError::TypeConflict { .. }))
    ));
}

// negative test: distinct type variables between parameter and body rejected
#[test]
fn test_pipeline_lambda_scoped_annotation_distinct_vars_rejected() {
    let content = r###"
f = (\ (x :: a) -> (x :: b))
"###;
    assert!(matches!(
        compile(content),
        Err(CompileError::Type(TyError::TypeConflict { .. }))
    ));
}

// positive test: type variable sharing accepted
#[test]
fn test_pipeline_lambda_scoped_annotation_matching_accepted() {
    let content = r###"
id_scoped = (\ (x :: a) -> (x :: a))
"###;
    assert_compiles_or_stg_todo(content);
}

// negative test: unannotated lambda parameter cannot unify with unscoped rigid expression type variable
#[test]
fn test_pipeline_lambda_unannotated_param_unscoped_body_annotation_rejected() {
    let content = r###"
f = (\x -> (x :: a))
"###;
    assert!(matches!(
        compile(content),
        Err(CompileError::Type(TyError::TypeConflict { .. }))
    ));
}

// positive test: let binding signature inside lambda references outer scoped type variable `a`
#[test]
fn test_pipeline_lambda_scoped_annotation_in_let_binding_sig_matching_accepted() {
    let content = r###"
f = (\ (x :: a) ->
    let y :: a = x
    in y)
"###;
    assert_compiles_or_stg_todo(content);
}

// positive test: top-level function signature scopes type variable `a` over its definition RHS
#[test]
fn test_pipeline_top_level_sig_scopes_over_fn_body_accepted() {
    let content = r###"
f :: a -> a
f x = (x :: a)
"###;
    assert_compiles_or_stg_todo(content);
}

// negative test: signature-scoped type variable `a` must not be generalized by an inner `let` binding
#[test]
fn test_pipeline_top_level_sig_scoped_var_not_generalized_by_inner_let_rejected() {
    let content = r###"
data Pair a b = Pair a b
f :: a -> Pair i64 String
f x =
  let g = \ (y :: a) -> y
  in Pair (g 1) (g (String "hello"))
"###;
    assert!(matches!(
        compile(content),
        Err(CompileError::Type(TyError::TypeConflict { .. }))
    ));
}

// negative test: primitive i64 takes 0 type arguments
#[test]
fn test_pipeline_primitive_i64_with_type_arg_rejected() {
    let content = r###"
f :: i64 a -> i64
f x = x
"###;
    assert!(matches!(
        compile(content),
        Err(CompileError::Type(TyError::ArityMismatch { ref constructor, expected: 0, got: 1, .. }))
        if constructor == "i64"
    ));
}

// negative test: nullary ADT Bool takes 0 type arguments
#[test]
fn test_pipeline_nullary_adt_with_type_arg_rejected() {
    let content = r###"
f :: Bool a -> Bool
f x = x
"###;
    assert!(matches!(
        compile(content),
        Err(CompileError::Type(TyError::ArityMismatch { ref constructor, expected: 0, got: 1, .. }))
        if constructor == "Bool"
    ));
}

// negative test: ADT field applying arguments to primitive i64 is rejected
#[test]
fn test_pipeline_adt_field_primitive_with_type_arg_rejected() {
    let content = r###"
data Bad = Bad (i64 a)
f x = x
"###;
    assert!(matches!(
        compile(content),
        Err(CompileError::Type(TyError::ArityMismatch { ref constructor, expected: 0, got: 1, .. }))
        if constructor == "i64"
    ));
}

// negative test: ADT Maybe takes 1 type argument, rejected when given 2
#[test]
fn test_pipeline_adt_arity_mismatch_rejected() {
    let content = r###"
f :: Maybe a b -> Maybe a
f x = x
"###;
    assert!(matches!(
        compile(content),
        Err(CompileError::Type(TyError::ArityMismatch { ref constructor, expected: 1, got: 2, .. }))
        if constructor == "Maybe"
    ));
}

// negative test: unknown type constructor in type signature is rejected
#[test]
fn test_pipeline_unknown_type_constructor_rejected() {
    let content = r###"
f :: NonExistentType -> i64
f x = 0
"###;
    assert!(matches!(
        compile(content),
        Err(CompileError::Type(TyError::UnknownType(ref name)))
        if name == "NonExistentType"
    ));
}

// negative test: user cannot shadow builtin primitive Bytes
#[test]
fn test_pipeline_bytes_redefinition_rejected() {
    let content = r###"
data Bytes = MyBytes
"###;
    assert!(matches!(
        compile(content),
        Err(CompileError::Type(TyError::AdtError(_)))
    ));
}

// negative test: user cannot shadow builtin ADT String
#[test]
fn test_pipeline_string_redefinition_rejected() {
    let content = r###"
data String = MyString
"###;
    assert!(matches!(
        compile(content),
        Err(CompileError::Type(TyError::AdtError(_)))
    ));
}

// negative test: user cannot shadow builtin ADT Int64
#[test]
fn test_pipeline_int64_redefinition_rejected() {
    let content = r###"
data Int64 = MyInt
"###;
    assert!(matches!(
        compile(content),
        Err(CompileError::Type(TyError::AdtError(_)))
    ));
}

// negative test: user cannot shadow builtin ADT Float64
#[test]
fn test_pipeline_float64_redefinition_rejected() {
    let content = r###"
data Float64 = MyFloat
"###;
    assert!(matches!(
        compile(content),
        Err(CompileError::Type(TyError::AdtError(_)))
    ));
}

// positive test: constructing String ADT from Bytes and pattern matching to deconstruct
#[test]
fn test_pipeline_string_adt_constructor_and_pattern_accepted() {
    let content = r###"
wrap :: Bytes -> String
wrap b = String b

unwrap :: String -> Bytes
unwrap s = case s of
             String b -> b

main = unwrap (wrap "hello world")
"###;
    assert_compiles_or_stg_todo(content);
}

// positive test: constructing Int64 and Float64 ADT and pattern matching to deconstruct
#[test]
fn test_pipeline_int64_and_float64_adt_constructor_and_pattern_accepted() {
    let content = r###"
wrapI :: i64 -> Int64
wrapI n = Int64 n

unwrapI :: Int64 -> i64
unwrapI x = case x of
              Int64 n -> n

wrapF :: f64 -> Float64
wrapF n = Float64 n

unwrapF :: Float64 -> f64
unwrapF x = case x of
              Float64 n -> n
"###;
    assert_compiles_or_stg_todo(content);
}

// negative test: unboxed integer cannot be bound in let expression
#[test]
fn test_pipeline_unboxed_let_binding_int_rejected() {
    let content = r###"
f x =
  let y = 42
  in y
"###;
    assert!(matches!(
        compile(content),
        Err(CompileError::Type(TyError::UnboxedLetBinding { .. }))
    ));
}

// negative test: unboxed float cannot be bound in let expression
#[test]
fn test_pipeline_unboxed_let_binding_float_rejected() {
    let content = r###"
f x =
  let y = 3.14
  in y
"###;
    assert!(matches!(
        compile(content),
        Err(CompileError::Type(TyError::UnboxedLetBinding { .. }))
    ));
}

// negative test: unboxed string literal (Bytes#) cannot be bound in let expression
#[test]
fn test_pipeline_unboxed_let_binding_bytes_rejected() {
    let content = r###"
f x =
  let y = "hello"
  in y
"###;
    assert!(matches!(
        compile(content),
        Err(CompileError::Type(TyError::UnboxedLetBinding { .. }))
    ));
}

// negative test: naked literal string ("hello") has type Bytes# and cannot be passed where String is expected
#[test]
fn test_pipeline_naked_string_literal_rejected_where_string_adt_expected() {
    let content = r###"
f :: String -> String
f s = s

bad = f "hello"
"###;
    let res = compile(content);
    match res {
        Err(CompileError::Type(TyError::TypeConflict { ty1, ty2, .. })) => {
            let is_bytes =
                |t: &TyExpr| matches!(t, TyExpr::TyCon(TyCon::Builtin(TyConBuiltin::Bytes)));
            let is_str = |t: &TyExpr| matches!(t, TyExpr::TyCon(TyCon::User(s)) if s == "String");
            assert!((is_str(&ty1) && is_bytes(&ty2)) || (is_bytes(&ty1) && is_str(&ty2)));
        }
        other => panic!(
            "expected TypeConflict between String and Bytes, got {:?}",
            other
        ),
    }
}

// negative test: record pattern binding unboxed primitive field in let expression is rejected
#[test]
fn test_pipeline_unboxed_let_binding_in_record_pattern_rejected() {
    let content = r###"
data Item { val :: i64 }

f a =
  let Item { val } = a
  in val
"###;
    assert!(matches!(
        compile(content),
        Err(CompileError::Type(TyError::UnboxedLetBinding { .. }))
    ));
}

// negative test: constructor pattern binding unboxed primitive field in let expression is rejected
#[test]
fn test_pipeline_unboxed_let_binding_in_constructor_pattern_rejected() {
    let content = r###"
f a =
  let Int64 n = a
  in n
"###;
    assert!(matches!(
        compile(content),
        Err(CompileError::Type(TyError::UnboxedLetBinding { .. }))
    ));
}
