use crate::typecheck::ty_var_name::*;

/// supplies auto generated type variable names
pub(crate) struct TyVarNameSupply {
    id: u64,
}

impl TyVarNameSupply {
    pub fn new() -> Self {
        Self { id: 0 }
    }
    pub fn generate(&mut self) -> TyVarName {
        let ret: u64 = self.id;
        self.id += 1;
        TyVarName::Auto(ret)
    }
}
