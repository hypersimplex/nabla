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

    // extract free value-level variables in each definition and retain only binders
    // from this declaration group
    // return definition indices that the current definition depends on
    // definitions with no in-group recursive references yield an empty neighbor set
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

/// return earliest previsit number reachable for current node in the current traversal
pub(crate) fn scc<F>(
    map_def_to_previsit: &mut BTreeMap<usize, usize>,
    map_def_to_earliest: &mut BTreeMap<usize, usize>,
    map_def_to_scc: &mut BTreeMap<usize, usize>,
    ssc_groups: &mut Vec<BTreeSet<usize>>,
    stack: &mut Vec<usize>,
    generate_previsit: &mut usize,
    neighbours: &F,
    current: usize,
) -> Option<usize>
where
    F: Fn(usize) -> Vec<usize>,
{
    // terminal conditions ---
    if map_def_to_scc.contains_key(&current) {
        // already processed, so skip
        return None;
    }
    if let Some(x) = map_def_to_previsit.get(&current) {
        // cycle detected, return current node's previsit number
        return Some(*x);
    }
    // --- terminal conditions

    // current node not visited yet, so generate previst number for it
    let previsit = *generate_previsit;
    *generate_previsit += 1;
    map_def_to_previsit.insert(current, previsit);
    map_def_to_earliest.insert(current, previsit);
    stack.push(current);

    for i in neighbours(current) {
        if let Some(previsit_num) = scc(
            map_def_to_previsit,
            map_def_to_earliest,
            map_def_to_scc,
            ssc_groups,
            stack,
            generate_previsit,
            neighbours,
            i,
        ) {
            // save earliest previsit number reachable from current node
            let earliest = map_def_to_earliest.get_mut(&current).unwrap();
            *earliest = (*earliest).min(previsit_num);
        }
    }

    let earliest = *map_def_to_earliest.get(&current).expect("earliest link");
    if previsit == earliest {
        // collect SCC
        let id_scc = ssc_groups.len();
        let mut ssc = BTreeSet::new();
        while let Some(node) = stack.pop() {
            ssc.insert(node);
            map_def_to_scc.insert(node, id_scc);
            if node == current {
                break;
            }
        }
        ssc_groups.push(ssc);
    }

    Some(earliest)
}
