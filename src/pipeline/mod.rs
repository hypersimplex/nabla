use crate::builtin::types::*;
use crate::builtin::values::*;
use crate::normalize::case_guard::*;
use crate::normalize::case_scrutinee::*;
use crate::normalize::pattern::*;
use crate::normalize::variable_renamer::*;
use crate::parse::abstr::*;
use crate::parse::abstr_structures::*;
use crate::parse::concrete_token::*;
use crate::parse::lex::*;
use crate::parse::parser::*;
use crate::typecheck::adt::*;
use crate::typecheck::convert_v_expr_from_a_expr::*;
use crate::typecheck::dependency_top_level::*;
use crate::typecheck::env_v_var_to_ty_scheme::*;
use crate::typecheck::pat_binder_uniqueness::*;
use crate::typecheck::subst::*;
use crate::typecheck::subst_persistent::*;
use crate::typecheck::ty_check_funcs::*;
use crate::typecheck::ty_env::*;
use crate::typecheck::ty_err::*;
use crate::typecheck::ty_expr::*;
use crate::typecheck::ty_scheme::*;
use crate::typecheck::ty_var_name::*;
use crate::typecheck::ty_var_name_supply::*;
use crate::typecheck::v_expr::*;
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

    let ty_env = register_adt_into_type_env(top_level.0.as_slice())?;

    let mut ty_var_ns = TyVarNameSupply::new();
    let mut env_v_var_to_ty_scheme = EnvVVarToTyScheme::new();

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

    let mut funcs: BTreeMap<usize, VExpr> = BTreeMap::new();

    for (idx, item) in top_level.0.iter().enumerate() {
        if let TopLevelItem::FunctionDefinition(def) = item {
            // convert to an abstraction expression
            let (vexpr, _annot) = vexpr_and_ty_annot_from_aexpr(
                &AExpr::AbstractionExpression(def.clone()),
                &mut v_var_ns,
            );
            validate_pattern_binder_uniqueness(&vexpr)?;
            assert!(matches!(&vexpr, VExpr::Abstraction(_)));
            funcs.insert(idx, vexpr);
        }
    }

    println!("rename user introduced local variables and patterns to uniquify..");
    // top level bindings stays constant (identity map)
    let vvars_outer_scope: BTreeMap<VVar, VVar> = env_v_var_to_ty_scheme
        .0
        .keys()
        .map(|x| (x.clone(), x.clone()))
        .collect();

    for (idx, expr) in funcs.iter_mut() {
        *expr = rename_var_unique(&mut v_var_ns, &vvars_outer_scope, expr);
    }

    // collect bindings for functions; for each of these: insert a monomorphic type variable for the 1st pass
    let mut original_seeded_lhs_binders: BTreeSet<VVar> = BTreeSet::new();
    // map top level function binding to an integer id to for in generic graph algo
    let mut map_binding_to_def_group: BTreeMap<VVar, usize> = BTreeMap::new();
    // environment accumulating info as type-checking progresses
    let mut env_v_var_to_ty_scheme_binding_seed: EnvVVarToTyScheme = env_v_var_to_ty_scheme.clone();

    for (idx, vexpr) in funcs.iter() {
        let v_var_binding_func = match vexpr {
            VExpr::Abstraction(VAbstrExpr {
                name: name @ VVar::Named(VVarName { .. }),
                ..
            }) => name,
            _ => unreachable!(),
        };

        if let Some(first_def_idx) = map_binding_to_def_group.get(&v_var_binding_func).copied() {
            return Err(TyError::PatBinderUniqueness(format!(
                "duplicate top-level binder `{:?}` (internal info: top-level binder seeding pass; def indices = {}, {})",
                v_var_binding_func, first_def_idx, idx
            )))?;
        }
        original_seeded_lhs_binders.insert(v_var_binding_func.clone());
        map_binding_to_def_group.insert(v_var_binding_func.clone(), *idx);
        // recursion policy (top-level)
        // - seed the env with monomorphic placeholder for each function in
        //   preparation for type inference/check (following the approach of
        //   look to the variables ref. SPJ 1987 S.9.5.2)
        // - signatures are checked after inference
        // - no polymorphic recursion support for now
        env_v_var_to_ty_scheme_binding_seed.insert(
            v_var_binding_func.clone(),
            TyScheme {
                ty_vars_schematic: vec![],
                ty_expr: Box::new(TyExpr::TyVar(ty_var_ns.generate())),
            },
        );
    }

    println!("analyzing dependency of top level functions into SCCs..");
    let scc_groups: Vec<BTreeSet<usize>> =
        compute_mutually_dependent_top_level_groups(&funcs, &map_binding_to_def_group);

    // this is used to compute free variables in environment; these are needed
    // to determine the set of generalizable type variables which become
    // schematic type variables in type schemes
    let mut env_outer: EnvVVarToTyScheme = env_v_var_to_ty_scheme.clone();

    let mut ty_check_results = ty_check_funcs(
        &ty_env,
        &mut ty_var_ns,
        &mut v_var_ns,
        &original_seeded_lhs_binders,
        &mut env_outer,
        &mut env_v_var_to_ty_scheme_binding_seed,
        &scc_groups,
        &funcs,
        &declared_function_type_schemes,
    )?;

    println!("type checked functions --->>");
    for (id, function_info) in ty_check_results.iter() {
        print!("{}", function_info.typed_expr.to_doc());
        println!();
    }
    println!("<<--- type checked functions");

    // type preserving passes --->>

    println!("desugar patterns to appear only in case clause pattern binders..");
    for (id, function_info) in ty_check_results.iter_mut() {
        function_info.typed_expr = desugar_pattern(&mut v_var_ns, &function_info.typed_expr);
    }

    println!("case scrutinee normalization to force having only simple variable..");
    for (id, function_info) in ty_check_results.iter_mut() {
        function_info.typed_expr =
            normalize_case_scrutinee(&mut v_var_ns, &function_info.typed_expr);
    }

    println!("case guard desugaring to case expressions without guard expressions..");
    for (id, function_info) in ty_check_results.iter_mut() {
        function_info.typed_expr = desugar_case_guard(&mut v_var_ns, &function_info.typed_expr);
    }

    // <<--- type preserving passes

    println!("normalized/desugared --->>");
    for (id, function_info) in ty_check_results.iter() {
        print!("{}", function_info.typed_expr.to_doc());
        println!();
    }
    println!("<<--- normalized/desugared");

    todo!("desugar and compile expressions to more basic forms")
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

    fn collect_user_vars<'a>(
        ty: &'a TyExpr,
        out: &mut BTreeSet<&'a TyVarNameUserDefined>,
        te: &TyEnv,
    ) {
        match ty {
            TyExpr::TyVar(TyVarName::UserDefined(u)) => {
                if let Err(TyError::UnknownType(_)) = te.get_adt(&format!("{}", u.token)) {
                    out.insert(u);
                }
            }
            TyExpr::TyVar(_) => {}
            TyExpr::TyApp(app) => {
                collect_user_vars(&app.ty_func, out, te);
                collect_user_vars(&app.ty_arg, out, te);
            }
        }
    }

    let mut ty_vars_schematic: Vec<TyVarName> = Vec::new();
    let mut subst = SubstPersistentIdent::default();

    let mut to_generalize = BTreeSet::new();
    collect_user_vars(&ty_expr, &mut to_generalize, ty_env);

    for u in to_generalize {
        let fresh_ty_var_name = ns.generate();
        ty_vars_schematic.push(fresh_ty_var_name.clone());
        subst = subst.insert(
            TyVarName::UserDefined(u.clone()),
            TyExpr::TyVar(fresh_ty_var_name),
        );
    }

    let ty_expr_generalized = subst_ty(&subst, &ty_expr);

    TyScheme {
        ty_vars_schematic,
        ty_expr: Box::new(ty_expr_generalized),
    }
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
