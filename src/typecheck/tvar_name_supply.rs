/// supplies auto generated type variable names
pub(crate) struct TVarNameSupply {
    id: u64,
}

impl TVarNameSupply {
    pub fn new() -> Self {
        Self { id: 0 }
    }
    pub fn gen_tvar_name(&mut self) -> TVariableName {
        let ret: u64 = self.id;
        self.id += 1;
        TVariableName::Auto(ret)
    }
}
