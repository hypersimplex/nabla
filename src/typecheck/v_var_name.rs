use crate::parse::concrete_token::ConcreteToken;
use crate::parse::loc::Location;
use crate::typecheck::ty_expr::*;
use crate::util::printer::*;

#[derive(Clone, Debug, Eq)]
pub(crate) struct VVarName {
    pub token: ConcreteToken,

    // maybe None if compiler creates this
    pub loc: Option<Location>,

    // TODO: determine if this is the best place for it
    pub builtin: Option<FnBuiltin>,
}
impl PartialEq for VVarName {
    fn eq(&self, other: &Self) -> bool {
        // compare strictly by token identity so local binders (builtin: None)
        // properly shadow global builtins (builtin: Some)
        self.token.eq(&other.token)
    }
}

impl Ord for VVarName {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // order strictly by token identity, consistent with PartialEq
        self.token.cmp(&other.token)
    }
}

impl PartialOrd for VVarName {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

// helper impl. for doc printer trait --->>

impl DocPrinter for VVarName {
    fn to_doc(&self) -> Box<Doc> {
        self.token.to_doc()
    }
}

// <<--- helper impl. for doc printer trait
