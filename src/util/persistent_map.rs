/// persistent map
use std::sync::Arc;

#[cfg(test)]
use std::collections::*;

#[derive(Debug, Clone)]
pub(crate) struct PersistentMap<K, V>(Option<PersistentMapInner<K, V>>);

impl<K: Clone + Ord, V: Clone> PersistentMap<K, V> {
    pub(crate) fn default() -> Self {
        Self(None)
    }
    pub(crate) fn new(key: K, value: V) -> Self {
        Self(Some(PersistentMapInner::new(key, value)))
    }
    pub(crate) fn insert(&self, key: K, value: V) -> PersistentMap<K, V> {
        match &self.0 {
            None => Self::new(key, value),
            Some(x) => Self(Some(x.insert(key, value))),
        }
    }
    pub(crate) fn get(&self, key: &K) -> Option<V>
    where
        V: Clone,
    {
        match &self.0 {
            None => None,
            Some(x) => x.get(key),
        }
    }
    pub(crate) fn in_order(&self) -> Vec<(K, V)> {
        match &self.0 {
            None => Vec::new(),
            Some(x) => x.in_order(),
        }
    }
    pub(crate) fn len(&self) -> usize {
        match &self.0 {
            None => 0,
            Some(x) => x.len(),
        }
    }
    pub(crate) fn iter(&self) -> PersistentMapInnerIter<K, V> {
        match &self.0 {
            None => PersistentMapInnerIter {
                stack_traversal: Vec::new(),
                idx: 0,
            },
            Some(x) => x.iter(),
        }
    }
}

#[derive(Debug, Clone)]
struct PersistentMapInner<K, V> {
    key: K,
    value: V,
    left: Option<Arc<PersistentMapInner<K, V>>>,
    right: Option<Arc<PersistentMapInner<K, V>>>,
}

impl<K: PartialOrd + Ord, V> PartialOrd for PersistentMapInner<K, V> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.key.cmp(&other.key))
    }
}

impl<K: Ord, V> Ord for PersistentMapInner<K, V> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.key.cmp(&other.key)
    }
}

impl<K: PartialEq, V> PartialEq for PersistentMapInner<K, V> {
    fn eq(&self, other: &Self) -> bool {
        self.key.eq(&other.key)
    }
}

impl<K: Eq, V> Eq for PersistentMapInner<K, V> {}

impl<K: Clone + Ord, V: Clone> PersistentMapInner<K, V> {
    pub(crate) fn new(key: K, value: V) -> Self {
        Self {
            key,
            value,
            left: None,
            right: None,
        }
    }

    pub(crate) fn insert(&self, key: K, value: V) -> PersistentMapInner<K, V> {
        match key.cmp(&self.key) {
            std::cmp::Ordering::Less => {
                let new_left = match &self.left {
                    Some(l) => Arc::new(l.insert(key, value)),
                    None => Arc::new(PersistentMapInner::new(key, value)),
                };
                PersistentMapInner {
                    key: self.key.clone(),
                    value: self.value.clone(),
                    left: Some(new_left),
                    right: self.right.clone(),
                }
            }
            std::cmp::Ordering::Greater => {
                let new_right = match &self.right {
                    Some(r) => Arc::new(r.insert(key, value)),
                    None => Arc::new(PersistentMapInner::new(key, value)),
                };
                PersistentMapInner {
                    key: self.key.clone(),
                    value: self.value.clone(),
                    left: self.left.clone(),
                    right: Some(new_right),
                }
            }
            std::cmp::Ordering::Equal => PersistentMapInner {
                key,
                value,
                left: self.left.clone(),
                right: self.right.clone(),
            },
        }
    }

    pub(crate) fn get(&self, key: &K) -> Option<V>
    where
        V: Clone,
    {
        match key.cmp(&self.key) {
            std::cmp::Ordering::Less => self.left.as_ref().and_then(|x| x.get(key)),
            std::cmp::Ordering::Greater => self.right.as_ref().and_then(|x| x.get(key)),
            std::cmp::Ordering::Equal => Some(self.value.clone()),
        }
    }

    pub(crate) fn in_order(&self) -> Vec<(K, V)> {
        let mut items = Vec::new();
        if let Some(branch) = self.left.as_ref() {
            items.append(&mut branch.in_order());
        }
        items.push((self.key.clone(), self.value.clone()));
        if let Some(branch) = self.right.as_ref() {
            items.append(&mut branch.in_order());
        }
        items
    }

    pub(crate) fn len(&self) -> usize {
        let mut count = 0;
        if let Some(branch) = self.left.as_ref() {
            count += branch.len();
        }
        count += 1;
        if let Some(branch) = self.right.as_ref() {
            count += branch.len();
        }
        count
    }

    pub(crate) fn iter(&self) -> PersistentMapInnerIter<K, V> {
        PersistentMapInnerIter {
            stack_traversal: self.in_order(),
            idx: 0,
        }
    }
}

pub(crate) struct PersistentMapInnerIter<K, V> {
    stack_traversal: Vec<(K, V)>,
    idx: usize,
}

impl<K: Clone, V: Clone> Iterator for PersistentMapInnerIter<K, V> {
    type Item = (K, V);
    fn next(&mut self) -> Option<Self::Item> {
        if self.idx >= self.stack_traversal.len() {
            None
        } else {
            let item = self.stack_traversal[self.idx].clone();
            self.idx += 1;
            Some(item)
        }
    }
}

#[test]
fn test_persist_map_insert_and_get() {
    type PersistentMapInnerInt = PersistentMapInner<i32, i32>;
    let n = PersistentMapInnerInt::new(0, 100);
    let mut ns = vec![n];
    for i in 1..10 {
        let recent = ns.last().unwrap();
        ns.push(recent.insert(i, i * 20));
    }
    for i in 1..10 {
        for j in 1..=i {
            assert_eq!(ns[i].get(&(j as i32)), Some(j as i32 * 20));
        }
    }
}

#[test]
fn test_persist_map_overwrite() {
    type PersistentMapInnerInt = PersistentMapInner<i32, i32>;
    let n = PersistentMapInnerInt::new(0, 100);
    let nn = n.insert(0, 50);
    assert_eq!(n.get(&0), Some(100));
    assert_eq!(nn.get(&0), Some(50));
}

#[test]
fn test_persist_map_iter() {
    type PersistentMapInnerInt = PersistentMapInner<i32, i32>;
    let n = PersistentMapInnerInt::new(0, 0);
    let mut ns = vec![n];
    for i in 1..10 {
        let recent = ns.last().unwrap();
        ns.push(recent.insert(i, i * 20));
    }
    for (idx, (k, v)) in ns.last().unwrap().iter().enumerate() {
        assert_eq!(v, idx as i32 * 20);
        assert_eq!(idx as i32, k);
    }
}

#[test]
fn test_persist_map_get_none() {
    type PersistentMapInnerInt = PersistentMapInner<i32, i32>;
    let mut n = PersistentMapInnerInt::new(0, 100);
    n = n.insert(0, 50);
    n = n.insert(10, 100);
    n = n.insert(-10, 20);
    assert_eq!(n.get(&11), None);
}

#[test]
fn test_persist_map_size() {
    type PersistentMapInnerInt = PersistentMapInner<i32, i32>;
    let mut n = PersistentMapInnerInt::new(0, 100);
    n = n.insert(0, 50);
    n = n.insert(10, 100);
    n = n.insert(-10, 20);
    let keys: Vec<_> = n.iter().map(|x| x.0).collect();
    assert_eq!(keys, vec![-10, 0, 10]);
    assert_eq!(n.len(), 3);
}

#[test]
fn test_persist_map_branching_immutability() {
    type PersistentMapInnerInt = PersistentMapInner<i32, i32>;
    let root_v0 = PersistentMapInnerInt::new(10, 100);
    let root_v1 = root_v0.insert(5, 50);
    let root_branch_a = root_v1.insert(2, 20);
    let root_branch_b = root_v1.insert(7, 70);

    // root_v0 must not see subsequent insertions
    assert_eq!(root_v0.get(&5), None);
    assert_eq!(root_v0.get(&2), None);
    assert_eq!(root_v0.get(&7), None);
    assert_eq!(root_v0.len(), 1);

    // root_v1 must not see insertions from diverged branches
    assert_eq!(root_v1.get(&2), None);
    assert_eq!(root_v1.get(&7), None);
    assert_eq!(root_v1.len(), 2);

    // root_branch_a must see 2 but not 7
    assert_eq!(root_branch_a.get(&2), Some(20));
    assert_eq!(root_branch_a.get(&7), None);
    assert_eq!(root_branch_a.len(), 3);

    // root_branch_b must see 7 but not 2
    assert_eq!(root_branch_b.get(&7), Some(70));
    assert_eq!(root_branch_b.get(&2), None);
    assert_eq!(root_branch_b.len(), 3);
}
