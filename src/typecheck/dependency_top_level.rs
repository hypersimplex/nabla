use crate::typecheck::algos::*;
use crate::typecheck::v_expr::*;

use std::collections::*;

/// returns SCCs in terms of the IDs associated with top level functions kept in
/// `map_binding_to_def_group`
///
/// the output is in depending ordering of SCCs, eg:
/// current group with output index i only possibly have dependencies on
/// group(s) with index j < i
pub(crate) fn compute_mutually_dependent_top_level_groups(
    funcs: &BTreeMap<usize, VExpr>,
    map_binding_to_def_group: &BTreeMap<VVar, usize>,
) -> Vec<BTreeSet<usize>> {
    let mut scc_groups: Vec<BTreeSet<usize>> = vec![];

    let mut map_def_to_previsit: BTreeMap<usize, usize> = BTreeMap::new();
    let mut map_def_to_earliest: BTreeMap<usize, usize> = BTreeMap::new();
    let mut map_def_to_scc: BTreeMap<usize, usize> = BTreeMap::new();
    let mut stack: Vec<usize> = vec![];
    let mut generate_previsit: usize = 0;

    // extract free value-level variables in each definition and retain only
    // binders from this declaration group
    //
    // return definition indices that the current definition depends on
    //
    // definitions with no in-group recursive references yield an empty
    // neighbor set
    let neighbours = |idx: usize| -> Vec<usize> {
        let lambda_abstraction: &VExpr = funcs.get(&idx).expect("index not found");
        let variables = lambda_abstraction.get_free_vars(&BTreeSet::new());
        let connected_def_ids: BTreeSet<usize> = variables
            .iter()
            .filter_map(|x: &VVar| map_binding_to_def_group.get(x).cloned())
            .collect();
        connected_def_ids.into_iter().collect()
    };

    for (idx, _) in funcs.iter() {
        scc(
            &mut map_def_to_previsit,
            &mut map_def_to_earliest,
            &mut map_def_to_scc,
            &mut scc_groups,
            &mut stack,
            &mut generate_previsit,
            &neighbours,
            *idx,
        );
    }
    scc_groups
}
