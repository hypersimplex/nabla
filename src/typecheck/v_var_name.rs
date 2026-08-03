use crate::parse::concrete_token::ConcreteToken;
use crate::parse::loc::Location;
use crate::typecheck::ty_expr::*;

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
        match (self.builtin, other.builtin) {
            (Some(x), Some(y)) => return x.eq(&y),
            (Some(x), None) => return false,
            (None, Some(y)) => return false,
            _ => {}
        }
        self.token.eq(&other.token)
    }
}

impl Ord for VVarName {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        match (self.builtin, other.builtin) {
            (Some(x), Some(y)) => return x.cmp(&y),
            (Some(x), None) => return std::cmp::Ordering::Greater,
            (None, Some(y)) => return std::cmp::Ordering::Less,
            _ => {}
        }
        self.token.cmp(&other.token)
    }
}

impl PartialOrd for VVarName {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
