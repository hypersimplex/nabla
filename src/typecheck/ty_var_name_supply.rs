use crate::typecheck::ty_var_name::*;

/// supplies auto generated type variable names
pub(crate) struct TyVarNameSupply {
    id: u64,
}

impl TyVarNameSupply {
    pub fn new() -> Self {
        Self { id: 0 }
    }
    /// generate a new flexible type variable
    pub fn generate(&mut self) -> TyVarName {
        let ret: u64 = self.id;
        self.id += 1;
        TyVarName::Auto(ret)
    }
    /// generate a new rigid type variable
    pub fn generate_rigid(&mut self) -> TyVarName {
        let ret: u64 = self.id;
        self.id += 1;
        TyVarName::Rigid(ret)
    }
}
