use crate::typecheck::subst::*;
use crate::typecheck::ty_scheme::TyScheme;
use crate::typecheck::v_expr::VVar;

use std::collections::BTreeMap;

/// maps value level variable to a type scheme (possibly with type parameters)
#[derive(Debug, Clone)]
pub(crate) struct EnvVVarToTyScheme(pub BTreeMap<VVar, TyScheme>);

impl EnvVVarToTyScheme {
    pub fn new() -> Self {
        Self(BTreeMap::new())
    }
    pub fn insert(&mut self, key: VVar, value: TyScheme) -> &mut EnvVVarToTyScheme {
        self.0.insert(key, value);
        self
    }
    pub fn get(&self, key: &VVar) -> Option<&TyScheme> {
        self.0.get(key)
    }

    /// apply substitution recursively and return a new env
    pub fn apply_subst_to_env(&self, subst: &impl Subst) -> EnvVVarToTyScheme {
        EnvVVarToTyScheme(
            self.0
                .iter()
                .map(|(v_var, ty_scheme)| (v_var.clone(), subst_ty_scheme(subst, ty_scheme)))
                .collect(),
        )
    }

    /// apply substitution in-place and return the current env
    pub fn apply_subst_to_env_in_place(&mut self, subst: &impl Subst) -> &mut EnvVVarToTyScheme {
        for (_v_var, ty_scheme) in self.0.iter_mut() {
            *ty_scheme = subst_ty_scheme(subst, ty_scheme);
        }
        self
    }
}
