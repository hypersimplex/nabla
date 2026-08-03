use crate::typecheck::texpr::TScheme;
use crate::typecheck::vexpr::VVariable;
use std::collections::BTreeMap;

/// maps value level variable to a type scheme (possibly with type parameters)
#[derive(Debug, Clone)]
pub(crate) struct EnvVVarToTyScheme(pub BTreeMap<VVar, TyScheme>);

impl EnvVVarToTyScheme {
    pub fn new() -> Self {
        Self(BTreeMap::new())
    }
    pub fn insert(&mut self, key: VVariable, value: TyScheme) -> Option<TyScheme> {
        self.0.insert(key, value)
    }
    pub fn get(&self, key: &VVariable) -> Option<&TyScheme> {
        self.0.get(key)
    }
}
