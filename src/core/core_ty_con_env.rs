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
    pub fn has_adt(&self, name: &str) -> bool {
        self.adts.contains_key(name)
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
        let mut core_env = CoreTyConEnv {
            adts: HashMap::new(),
        };

        for adt in ty_con_env.iter_adts() {
            let core_constructors: Vec<CoreConDef> = adt
                .constructors
                .iter()
                .map(CoreConDef::from_constructor_def)
                .collect();

            core_env.adts.insert(
                adt.name.clone(),
                CoreADTDef {
                    name: adt.name.clone(),
                    ty_params: adt.ty_params.clone(),
                    constructors: core_constructors,
                },
            );
        }

        core_env
    }
}
