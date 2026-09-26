use crate::parse::concrete_token;
use crate::parse::loc;
use crate::util::printer::*;

/// type-level variable used for inferencing types
///
/// when present in a type scheme's parameter list, this instantiates to a
/// fresh new type variable at use site, otherwise it is copied
#[derive(Clone, Debug, PartialOrd, Ord, PartialEq, Eq)]
pub(crate) enum TyVarName {
    UserDefined(TyVarNameUserDefined),

    // basic type supported by the compiler
    Builtin(TyVarNameBuiltin),

    // auto-generated unique name
    Auto(u64),

    // rigid type for signature checking;
    // these do not mutate with / adapt to surrounding type context
    Rigid(u64),
}

#[derive(Clone, Debug, Eq)]
pub(crate) struct TyVarNameUserDefined {
    pub token: concrete_token::ConcreteToken,

    // maybe None if compiler creates this
    pub loc: Option<loc::Location>,
}

impl PartialEq for TyVarNameUserDefined {
    fn eq(&self, other: &Self) -> bool {
        self.token == other.token
    }
}

impl Ord for TyVarNameUserDefined {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.token.cmp(&other.token)
    }
}

impl PartialOrd for TyVarNameUserDefined {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Clone, Debug, PartialOrd, Ord, PartialEq, Eq)]
pub(crate) enum TyVarNameBuiltin {
    I64,
    F64,
    String,
    Arrow, // internally maps to a type scheme
}

// helpers ---

pub(crate) fn mk_ty_var_name_userdef(name: &str) -> TyVarName {
    TyVarName::UserDefined(TyVarNameUserDefined {
        token: concrete_token::ConcreteToken::Iden(name.to_string()),
        loc: None,
    })
}

// helper impl. for doc printer trait --->>

impl DocPrinter for TyVarName {
    fn to_doc(&self) -> Box<Doc> {
        use TyVarName::*;
        match self {
            UserDefined(ty_var_name_user_defined) => ty_var_name_user_defined.to_doc(),
            Builtin(ty_var_name_builtin) => ty_var_name_builtin.to_doc(),
            Auto(auto_id) => Doc::lit(&format!("TyAuto({})", auto_id)),
            Rigid(rigid_id) => Doc::lit(&format!("TyRigid({})", rigid_id)),
        }
    }
}

impl DocPrinter for TyVarNameUserDefined {
    fn to_doc(&self) -> Box<Doc> {
        match &self.token {
            iden @ concrete_token::ConcreteToken::Iden(_) => iden.to_doc(),
            _ => {
                unreachable!()
            }
        }
    }
}

impl DocPrinter for TyVarNameBuiltin {
    fn to_doc(&self) -> Box<Doc> {
        use TyVarNameBuiltin::*;
        match self {
            I64 => Doc::lit("i64"),
            F64 => Doc::lit("f64"),
            String => Doc::lit("String"),
            Arrow => Doc::lit("->"),
        }
    }
}

// <<--- helper impl. for doc printer trait
