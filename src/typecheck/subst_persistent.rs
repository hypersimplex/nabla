/// implementation to map ty_var_name -> ty_expr
use crate::typecheck::subst::*;
use crate::typecheck::ty_expr::*;
use crate::typecheck::ty_var_name::*;
use crate::util::persistent_map::*;

/// an implementation that does not apply identity function when key is not found
pub(crate) type SubstPersistent = PersistentMap<TyVarName, TyExpr>;

/// an implementation that applies identity function when key is not found
#[derive(Debug, Clone)]
pub(crate) struct SubstPersistentIdent(pub PersistentMap<TyVarName, TyExpr>);

impl Subst for SubstPersistent {
    fn new(key: TyVarName, val: TyExpr) -> Self {
        Self::new(key, val)
    }
    fn new_with(f: impl Fn() -> Self) -> Self
    where
        Self: Sized,
    {
        f()
    }
    fn get(&self, key: &TyVarName) -> Option<TyExpr> {
        self.get(key)
    }
    fn insert(&self, key: TyVarName, val: TyExpr) -> Self {
        Self::insert(self, key, val)
    }
    // composition of substitutions, applied from right to left:
    //   subst_composed = (subst2 . subst1)
    fn compose(subst2: &Self, subst1: &Self) -> Self {
        let mut subst_new = Self::default();
        // make substitution using subst2 for existing items in subst1
        for (tvn, tyexpr) in subst1.iter() {
            let tyexpr_new = subst_ty(subst2, &tyexpr);
            subst_new = subst_new.insert(tvn, tyexpr_new);
        }
        // copy remaning items in subst2 not in subst1
        for (tvn, tyexpr) in subst2.iter() {
            if subst1.get(&tvn).is_none() {
                subst_new = subst_new.insert(tvn, tyexpr);
            }
        }
        subst_new
    }
}

impl Default for SubstPersistent {
    fn default() -> Self {
        Self::default()
    }
}
impl Default for SubstPersistentIdent {
    fn default() -> Self {
        SubstPersistent::default().into()
    }
}

impl Subst for SubstPersistentIdent {
    fn new(key: TyVarName, val: TyExpr) -> Self {
        Self(SubstPersistent::new(key, val))
    }
    fn new_with(f: impl Fn() -> Self) -> Self
    where
        Self: Sized,
    {
        f()
    }
    fn get(&self, key: &TyVarName) -> Option<TyExpr> {
        match self.0.get(key) {
            Some(x) => Some(x),
            None => Some(TyExpr::TyVar(key.clone())),
        }
    }
    fn insert(&self, key: TyVarName, val: TyExpr) -> Self {
        Self(self.0.insert(key, val))
    }
    // composition of substitutions, applied from right to left:
    //   subst_composed = (subst2 . subst1)
    // note: equivalent to applying subst2 on top of subst1 using subst_ty helper
    fn compose(subst2: &Self, subst1: &Self) -> Self {
        let mut subst_new = Self::default();
        // make substitution using subst2 for existing items in subst1
        for (tvn, tyexpr) in subst1.iter() {
            let tyexpr_new = subst_ty(subst2, &tyexpr);
            subst_new = subst_new.insert(tvn, tyexpr_new);
        }
        // copy remaning items in subst2 not in subst1
        for (tvn, tyexpr) in subst2.iter() {
            if subst1.0.get(&tvn).is_none() {
                subst_new = subst_new.insert(tvn, tyexpr);
            }
        }
        subst_new
    }
}

impl SubstPersistentIdent {
    pub(crate) fn iter(&self) -> PersistentMapInnerIter<TyVarName, TyExpr> {
        self.0.iter()
    }
}

impl From<SubstPersistent> for SubstPersistentIdent {
    fn from(other: SubstPersistent) -> Self {
        Self(other)
    }
}

impl From<SubstPersistentIdent> for SubstPersistent {
    fn from(other: SubstPersistentIdent) -> Self {
        other.0
    }
}
