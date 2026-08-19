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
    Builtin(TyVarNameBuiltin), // basic type supported by the compiler
    Auto(u64),                 // auto-generated unique name
}

#[derive(Clone, Debug, PartialOrd, Ord, PartialEq, Eq)]
pub(crate) struct TyVarNameUserDefined {
    pub token: concrete_token::ConcreteToken,

    // maybe None if compiler creates this
    pub loc: Option<loc::Location>,
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
            Auto(auto_id) => mk_lit(&format!("TyAuto({})", auto_id)),
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
            I64 => mk_lit("i64"),
            F64 => mk_lit("f64"),
            String => mk_lit("String"),
            Arrow => mk_lit("->"),
        }
    }
}

// <<--- helper impl. for doc printer trait
