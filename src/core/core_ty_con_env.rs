use std::collections::HashMap;

use crate::core::core_err::*;
use crate::core::core_ty_con::*;
use crate::typecheck::ty_con_env::*;

pub(crate) struct CoreTyConEnv {
    adts: HashMap<String, CoreADTDef>,
}

impl CoreTyConEnv {
    pub fn new() -> Self {
        CoreTyConEnv {
            adts: HashMap::new(),
        }
    }
    pub fn get_adt(&self, name: &str) -> Result<&CoreADTDef, CoreError> {
        self.adts
            .get(name)
            .ok_or_else(|| CoreError::AdtError(format!("unknown ADT: {}", name).to_string()))
    }
}

// conversion helper
impl<'a> From<&'a TyConEnv> for CoreTyConEnv {
    fn from(ty_con_env: &'a TyConEnv) -> Self {
        CoreTyConEnv {
            adts: ty_con_env
                .iter_adts()
                .map(|x| (x.name.clone(), x.into()))
                .collect(),
        }
    }
}
