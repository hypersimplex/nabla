use crate::parse::concrete_token::*;
use crate::typecheck::env_v_var_to_ty_scheme::*;
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

use std::collections::*;

#[derive(Clone, Debug)]
pub(crate) struct ProgramFunctionArtifacts {
    pub name: VVar,
    pub vexpr: VExpr,
    pub ty_expr: TyExpr,
    // generalized scheme for top-level binder (TyLam order for Core/System F)
    // e.g. id : forall a. a -> a => wrap rhs with `TyLam a` in Core
    pub scheme: TyScheme,
    /// typed expression directly from type inference (pre-desugar/match compile)
    pub typed_expr: TypedVExpr,
}

/// type checking and inference over all top level functions, and do it for
/// bodies of these functions as well
///
/// process in order of SCC dependency:
///
/// `ssc_groups` should already be in order of dependency
/// where current group i only possibly have dependencies on group(s) with
/// index j < i and we process in ascending order of group index
pub(crate) fn ty_check_funcs(
    ty_env: &TyEnv,

    ty_var_ns: &mut TyVarNameSupply,

    v_var_supply: &mut VVarNameSupply,

    original_seeded_lhs_binders: &BTreeSet<VVar>,

    // used to compute free variables in environment
    // this is needed to determine the set of generalizable type variables which
    // become schematic type variables in type schemes
    env_outer: &mut EnvVVarToTyScheme,

    // this env accumulates info as typechecking progresses
    env_v_var_to_ty_scheme_binding_seed: &mut EnvVVarToTyScheme,

    scc_groups: &[BTreeSet<usize>],

    funcs: &BTreeMap<usize, VExpr>,

    declared_function_type_schemes: &BTreeMap<String, TyScheme>,
) -> TyChkResult<BTreeMap<usize, ProgramFunctionArtifacts>> {
    // as we process SCC groups, accumulate solved substitutions so that it's
    // usable for next SCC group
    let mut subst_accum = SubstPersistentIdent::default();

    struct TypedFuncBinding {
        typed_binding: TypedVPattern, // function name
        typed_lambda_abstraction: TypedVExpr,
    }

    let mut typed_binding_def_pairs: BTreeMap<usize, TypedFuncBinding> = BTreeMap::new();

    let mut results: BTreeMap<usize, ProgramFunctionArtifacts> = BTreeMap::new();

    for scc in scc_groups.iter() {
        // per SCC:
        //  - use previously created monomorphic type variables (placeholders)
        //    for each LHS binding in current SCC
        //  - type check RHS and unify with LHS binding for each (binding, def)
        //    pair in the SCC group
        //  - compose substitutions and normalize types as we go
        for idx in scc.iter() {
            let vexpr_abstr @ VExpr::Abstraction(VAbstrExpr {
                name:
                    binding_vvar @ VVar::Named(VVarName {
                        token: ConcreteToken::Iden(name),
                        ..
                    }),
                ..
            }) = funcs.get(idx).expect("mapped function idx does not exist")
            else {
                unreachable!();
            };

            // refresh the environment with the latest substitution so subsequent lookups see
            //  the updated monomorphic bindings
            *env_v_var_to_ty_scheme_binding_seed =
                env_v_var_to_ty_scheme_binding_seed.apply_subst_to_env(&subst_accum);

            // type check LHS (function name/binding)
            let (subst, _bound_vvars_in_pattern, mut typed_binding_expr) =
                ty_check_pattern_typed_with_seeded_binders(
                    env_v_var_to_ty_scheme_binding_seed,
                    original_seeded_lhs_binders,
                    ty_env,
                    ty_var_ns,
                    &VPattern::Variable(binding_vvar.clone()),
                )?;
            subst_accum = subst_compose(&subst_accum, &subst);
            typed_binding_expr = apply_subst_typed_pattern(&subst_accum, typed_binding_expr);

            *env_v_var_to_ty_scheme_binding_seed =
                env_v_var_to_ty_scheme_binding_seed.apply_subst_to_env(&subst_accum);

            // typecheck RHS (lambda abstraction)
            let (subst, mut typed_rhs_vexpr) = ty_check_vexpr_typed(
                env_v_var_to_ty_scheme_binding_seed,
                &ty_env,
                ty_var_ns,
                vexpr_abstr,
            )?;
            subst_accum = subst_compose(&subst_accum, &subst);

            // unify LHS with RHS
            subst_accum =
                unify_ty_exprs(&subst_accum, typed_binding_expr.ty(), typed_rhs_vexpr.ty())?;

            // when function has a declared signature in the source code:
            if let Some(sig_scheme) = declared_function_type_schemes.get(name) {
                // post-inference signature check (top-level only); does not affect recursion
                // instantiate schematic variables in signature to fresh variables before unifying
                let mut subst_sig = subst_id();
                for tvn in sig_scheme.ty_vars_schematic.iter() {
                    subst_sig = subst_sig.insert(tvn.clone(), TyExpr::TyVar(ty_var_ns.generate()));
                }
                let sig_type_inst = subst_ty(&subst_sig, &sig_scheme.ty_expr);
                subst_accum = unify_ty_exprs(&subst_accum, typed_rhs_vexpr.ty(), &sig_type_inst)?;
            }

            typed_rhs_vexpr = apply_subst_typed_expr(&subst_accum, typed_rhs_vexpr);
            typed_binding_expr = apply_subst_typed_pattern(&subst_accum, typed_binding_expr);

            assert!(matches!(typed_binding_expr, TypedVPattern::Variable { .. }));

            typed_binding_def_pairs.insert(
                *idx,
                TypedFuncBinding {
                    typed_binding: typed_binding_expr,
                    typed_lambda_abstraction: typed_rhs_vexpr,
                },
            );
        }

        // generalization (per SCC)
        // - compute free vars in each def and in the outer env
        // - schematic type variables = free(def) \ free(env)
        // - update monomorphic placeholders after generalization completes
        // - top-level function binders remain polymorphic; pattern binders stay
        //   monomorphic until selector desugaring moves polymorphism onto let
        //   binders (GHC-style)
        let free_ty_vars_in_env: BTreeSet<_> = {
            let mut env_outer_copy = env_outer.clone();
            env_outer_copy = env_outer_copy.apply_subst_to_env(&subst_accum);
            // `free(env)` for HM generalization
            // vars in scheme bodies that are not bound by the scheme
            free_tvns_in_ty_env(&env_outer_copy).into_iter().collect()
        };

        // build an scc-wide schematic substitution to keep mutually recursive
        // bindings in sync
        let mut scc_scheme_var_map: BTreeMap<TyVarName, TyVarName> = BTreeMap::new();

        for idx in scc.iter() {
            let TypedFuncBinding {
                typed_binding, // function name
                typed_lambda_abstraction,
            } = typed_binding_def_pairs.get(idx).unwrap();

            let typed_lambda_abstraction =
                apply_subst_typed_expr(&subst_accum, typed_lambda_abstraction.clone());

            // type variables that can be generalized (make generic parameters)
            // for the function
            //   = { free type variables in function definition }
            //     \ { free type variables in the environment }
            let free_ty_vars_in_def =
                free_ty_vars_excluding_adts(typed_lambda_abstraction.ty(), &ty_env);
            let ty_vars_generalizable: Vec<_> = free_ty_vars_in_def
                .into_iter()
                .filter(|tvn| !free_ty_vars_in_env.contains(tvn))
                .collect();

            for tvn in ty_vars_generalizable.iter() {
                scc_scheme_var_map
                    .entry(tvn.clone())
                    // generate new type variables for schematic type variables to avoid name collision
                    .or_insert_with(|| ty_var_ns.generate());
            }
        }

        let scc_subst_map: BTreeMap<_, _> = scc_scheme_var_map
            .iter()
            .map(|(tvn, scheme_tvn)| (tvn.clone(), TyExpr::TyVar(scheme_tvn.clone())))
            .collect();
        let scc_subst = subst_from_map(&scc_subst_map);

        // per-binder schematic order for TyLam/TyApp insertion
        let mut scheme_info_by_idx: BTreeMap<usize, Vec<TyVarName>> = BTreeMap::new();
        let mut scheme_info_for_group: BTreeMap<VVar, TyScheme> = BTreeMap::new();

        // todo: reduce redundant code

        for idx in scc.iter() {
            let TypedFuncBinding {
                typed_binding, // function name
                typed_lambda_abstraction,
            } = typed_binding_def_pairs.get(idx).unwrap();

            let typed_lambda_abstraction =
                apply_subst_typed_expr(&subst_accum, typed_lambda_abstraction.clone());

            // type variables that can be generalized (make generic parameters)
            // for the function
            //   = { free type variables in function definition }
            //     \ { free type variables in the environment }
            let free_ty_vars_in_def =
                free_ty_vars_excluding_adts(typed_lambda_abstraction.ty(), &ty_env);
            let ty_vars_generalizable: Vec<_> = free_ty_vars_in_def
                .into_iter()
                .filter(|tvn| !free_ty_vars_in_env.contains(tvn))
                .collect();

            let ty_vars_schematic: Vec<_> = ty_vars_generalizable
                .iter()
                .map(|tvn| scc_scheme_var_map.get(tvn).cloned().unwrap())
                .collect::<Vec<_>>();

            scheme_info_by_idx.insert(*idx, ty_vars_schematic);
        }

        for idx in scc.iter() {
            let TypedFuncBinding {
                typed_binding: TypedVPattern::Variable { binder, .. },
                typed_lambda_abstraction,
            } = typed_binding_def_pairs.get(idx).unwrap()
            else {
                unreachable!()
            };

            let ty_vars_schematic = scheme_info_by_idx.get(idx).unwrap();

            let typed_lambda_abstraction =
                apply_subst_typed_expr(&subst_accum, typed_lambda_abstraction.clone());
            let typed_lambda_abstraction =
                apply_subst_typed_expr(&scc_subst, typed_lambda_abstraction);
            scheme_info_for_group.insert(
                binder.clone(),
                TyScheme {
                    ty_vars_schematic: ty_vars_schematic.clone(),
                    ty_expr: Box::new(typed_lambda_abstraction.ty().clone()),
                },
            );
        }

        for idx in scc.iter() {
            let TypedFuncBinding {
                typed_binding, // function name
                typed_lambda_abstraction,
            } = typed_binding_def_pairs.get(idx).unwrap();

            let TypedVPattern::Variable { binder, .. } = typed_binding else {
                unreachable!();
            };

            let ty_vars_schematic = scheme_info_by_idx.get(idx).unwrap();

            // apply scc scheme substitution and fill explicit ty_args for recursive references
            let typed_lambda_abstraction =
                apply_subst_typed_expr(&subst_accum, typed_lambda_abstraction.clone());
            let typed_lambda_abstraction =
                apply_subst_typed_expr(&scc_subst, typed_lambda_abstraction);
            let typed_lambda_abstraction =
                fill_missing_ty_args(typed_lambda_abstraction, &scheme_info_for_group)?;

            // update for inferencing for remaining SCC groups
            let ty_scheme_updated = TyScheme {
                // mapped schematic type variables
                ty_vars_schematic: ty_vars_schematic.clone(),
                // apply substitution
                ty_expr: Box::new(typed_lambda_abstraction.ty().clone()),
            };

            // store function result and register its type in env
            results.insert(
                *idx,
                ProgramFunctionArtifacts {
                    name: binder.clone(),
                    vexpr: funcs.get(idx).unwrap().clone(),
                    ty_expr: typed_lambda_abstraction.ty().clone(),
                    typed_expr: typed_lambda_abstraction,
                    scheme: ty_scheme_updated.clone(),
                },
            );

            env_v_var_to_ty_scheme_binding_seed.insert(binder.clone(), ty_scheme_updated.clone());
            env_outer.insert(binder.clone(), ty_scheme_updated);
        }
    }

    Ok(results)
}
