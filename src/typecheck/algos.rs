use std::collections::*;

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
