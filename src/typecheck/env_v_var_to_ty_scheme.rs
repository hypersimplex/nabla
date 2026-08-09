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
    pub fn insert(&mut self, key: VVar, value: TyScheme) -> Option<TyScheme> {
        self.0.insert(key, value)
    }
    pub fn get(&self, key: &VVar) -> Option<&TyScheme> {
        self.0.get(key)
    }
}
