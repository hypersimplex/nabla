use crate::typecheck::v_expr::*;
use crate::typecheck::v_var_name::*;

/// supplies auto generated type variable names
pub(crate) struct VVarNameSupply {
    id: u64,
}

impl VVarNameSupply {
    pub fn new() -> Self {
        Self { id: 0 }
    }
    pub fn generate(&mut self) -> VVar {
        let ret: u64 = self.id;
        self.id += 1;
        VVar::Anon(ret)
    }
    pub fn uniqify(&mut self, original: &VVar) -> VVar {
        use VVar::*;
        match original {
            Named(x) => {
                let ret: u64 = self.id;
                self.id += 1;
                VVar::Renamed(VVarNameUniqued {
                    original: x.clone(),
                    unique: ret,
                })
            }
            other => other.clone(),
        }
    }
}
