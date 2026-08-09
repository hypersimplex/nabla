use crate::builtin::types::*;
use crate::builtin::values::*;
use crate::parse::abstr::*;
use crate::parse::abstr_structures::*;
use crate::parse::concrete_token::*;
use crate::parse::lex::*;
use crate::parse::loc::*;
use crate::parse::parser::*;
use crate::parse::printer::DocPrinter;
use crate::typecheck::adt::*;
use crate::typecheck::convert_v_expr_from_a_expr::*;
use crate::typecheck::env_v_var_to_ty_scheme::*;
use crate::typecheck::pat_binder_uniqueness::*;
use crate::typecheck::subst::*;
use crate::typecheck::subst_persistent::*;
use crate::typecheck::ty_env::*;
use crate::typecheck::ty_err::*;
use crate::typecheck::ty_expr::*;
use crate::typecheck::ty_scheme::*;
use crate::typecheck::ty_var_name::*;
use crate::typecheck::ty_var_name_supply::*;
use crate::typecheck::v_expr::*;
use crate::typecheck::v_var_name::*;
use crate::typecheck::v_var_name_supply::*;

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

    // for now, parser converts functions to lambda abstractions
    let top_level = parse_concrete_top_level(lexed_output)?;

    println!("{}", top_level.to_doc());

    let ty_env = register_adt_into_type_env(top_level.0.as_slice())?;

    let mut ty_var_ns = TyVarNameSupply::new();
    let mut env_v_var_to_ty_scheme = EnvVVarToTyScheme::new();

    init_builtin_values(&mut env_v_var_to_ty_scheme, &mut ty_var_ns);

    // function name -> top-level type schemes
    let mut declared_function_type_schemes: BTreeMap<String, TyScheme> = BTreeMap::new();
    insert_declared_signatures(
        top_level.0.as_slice(),
        &ty_env,
        &mut env_v_var_to_ty_scheme,
        &mut declared_function_type_schemes,
        &mut ty_var_ns,
    );

    let mut v_var_supply: VVarNameSupply = VVarNameSupply::new();

    let mut funcs: BTreeMap<usize, VExpr> = BTreeMap::new();

    for (idx, item) in top_level.0.iter().enumerate() {
        let TopLevelItem::FunctionDefinition(def) = item else {
            continue;
        };
        // convert to an abstraction expression
        let (vexpr, _annot) = vexpr_and_ty_annot_from_aexpr(
            &AExpr::AbstractionExpression(def.clone()),
            &mut v_var_supply,
        );
        // reject duplicate pattern binders before type inference
        validate_pattern_binder_uniqueness(&vexpr)?;
        match &vexpr {
            VExpr::Abstraction(_) => {
                funcs.insert(idx, vexpr);
            }
            _ => {
                unreachable!("function definitions expected to be in a lambda abstraction");
            }
        }
    }

    // collect bindings for functions, insert monomorphic type variables for these
    let mut original_seeded_lhs_binders: BTreeSet<VVar> = BTreeSet::new();
    let mut map_binding_to_def_group: BTreeMap<VVar, usize> = BTreeMap::new();

    // environment accumulating info as typechecking progresses
    let mut env_v_var_to_ty_scheme_binding_seed: EnvVVarToTyScheme = env_v_var_to_ty_scheme.clone();

    for (idx, vexpr) in funcs.iter() {
        match vexpr {
            VExpr::Abstraction(VAbstrExpr {
                name: v_var_binding_func,
                ..
            }) => {
                match v_var_binding_func {
                    VVar::Named(VVarName { .. }) => {}
                    _ => {
                        unreachable!();
                    }
                }
                if let Some(first_def_idx) =
                    map_binding_to_def_group.get(v_var_binding_func).copied()
                {
                    return Err(TyError::PatBinderUniqueness(format!(
                        "duplicate top-level binder `{:?}` (internal info: top-level binder seeding pass; def indices = {}, {})",
                        v_var_binding_func, first_def_idx, idx
                    )))?;
                }
                original_seeded_lhs_binders.insert(v_var_binding_func.clone());
                map_binding_to_def_group.insert(v_var_binding_func.clone(), *idx);
                // recursion policy (top-level)
                // - seed the env with monomorphic placeholder for each function in preparation for type inference/check
                // - signatures are checked after inference; note: no polymorphic recursion support
                env_v_var_to_ty_scheme_binding_seed.insert(
                    v_var_binding_func.clone(),
                    TyScheme {
                        ty_vars_schematic: vec![],
                        ty_expr: Box::new(TyExpr::TyVar(ty_var_ns.generate())),
                    },
                );
            }
            _ => {
                unreachable!();
            }
        }
    }

    todo!("analyze dependency and sort each SCC; then typecheck each SCC group in dependency order")
}

fn insert_declared_signatures(
    items: &[TopLevelItem],
    ty_env: &TyEnv,
    env: &mut EnvVVarToTyScheme,
    declared_function_type_schemes: &mut BTreeMap<String, TyScheme>,
    ns: &mut TyVarNameSupply,
) {
    for i in items.iter() {
        if let TopLevelItem::FunctionSignature(sig) = i {
            if let ConcreteToken::Iden(name) = &sig.identifier.token {
                let scheme = build_scheme_from_signature(sig, ty_env, ns);
                declared_function_type_schemes.insert(name.clone(), scheme.clone());
                let v_var = VVar::Named(VVarName {
                    token: sig.identifier.token.clone(),
                    loc: Some(sig.identifier.loc.clone()),
                    builtin: None,
                });
                env.insert(v_var, scheme);
            }
        }
    }
}

/// build a type scheme from a declared function signature
/// by generalizing free user-defined type variables that are not ADT names
/// in the type environment
fn build_scheme_from_signature(sig: &FnSig, ty_env: &TyEnv, ns: &mut TyVarNameSupply) -> TyScheme {
    let ty_expr = lower_type_annot_to_ty_expr(&sig.ty);

    fn collect_user_vars<'a>(ty: &'a TyExpr, out: &mut Vec<&'a TyVarNameUserDefined>, te: &TyEnv) {
        match ty {
            TyExpr::TyVar(TyVarName::UserDefined(u)) => {
                if let Err(TyError::UnknownType(_)) = te.get_adt(&format!("{}", u.token)) {
                    out.push(u);
                }
            }
            TyExpr::TyVar(_) => {}
            TyExpr::TyApp(app) => {
                collect_user_vars(&app.ty_func, out, te);
                collect_user_vars(&app.ty_arg, out, te);
            }
        }
    }

    let mut schematic_ty_vars: Vec<TyVarName> = Vec::new();
    let mut subst = SubstPersistentIdent::default();

    let mut to_generalize: Vec<&TyVarNameUserDefined> = Vec::new();
    collect_user_vars(&ty_expr, &mut to_generalize, ty_env);
    to_generalize.sort();
    to_generalize.dedup();

    for u in to_generalize {
        let fresh_ty_var_name = ns.generate();
        schematic_ty_vars.push(fresh_ty_var_name.clone());
        subst = subst.insert(
            TyVarName::UserDefined(u.clone()),
            TyExpr::TyVar(fresh_ty_var_name),
        );
    }

    let ty_expr_generalized = subst_ty(&subst, &ty_expr);

    TyScheme {
        ty_vars_schematic: schematic_ty_vars,
        ty_expr: Box::new(ty_expr_generalized),
    }
}

static TEST_PIPELINE_CONTENT_0: &str = r###"
f x y z = 9 / (-7*x+y+z)

f_let_0 a =
 let x :: u32 = 1
     y :: u32 = 2
     z :: u32 = 5
 in
   x + (y :: u32) + z + a

f_let_1 a =
 let x :: u32 = 1
     y :: u32 = 2
     z :: u32 = 5
 in
   let b = 10
       c = b * b
   in x + y + z + a + b * c

f_let_2 a =
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

#[test]
fn test_pipeline() {
    match compile(TEST_PIPELINE_CONTENT_0) {
        Err(e) => {
            println!("{:?}", e);
        }
        _ => {}
    }
}
