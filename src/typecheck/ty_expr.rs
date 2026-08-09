use crate::parse::concrete_token;
use crate::typecheck::ty_var_name::*;
use std::collections::*;
use std::fmt;
use std::iter::IntoIterator;

/// type expression, a type-level construct that contains type info
#[derive(Clone, Debug)]
pub enum TyExpr {
    // a type variable used for solving type equations
    TyVar(TyVarName),

    TyApp(TyApplication),
}

/// type expression application (analogous to value-level application) where
/// this corresponds to substitution for schematic type parameters in
/// a polymorphic type
///
/// this subsumes simple compound type
#[derive(Clone, Debug)]
pub(crate) struct TyApplication {
    pub ty_func: Box<TyExpr>,
    pub ty_arg: Box<TyExpr>,
}

// TODO: determine if these should be defined here
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum FnBuiltin {
    UnaryPlus,
    UnaryNegate,
    UnaryLogicalNot,
    BinaryAdd,
    BinarySub,
    BinaryMul,
    BinaryDiv,
    BinaryLess,
    BinaryLessEqual,
    BinaryGreater,
    BinaryGreaterEqual,
    BinaryEqual,
    LogicalAnd,
    LogicalOr,
    Arrow,
    String,
    MatchFail,
}

// utility helpers ---

/// type level application: (ty_func ty_arg)
pub(crate) fn ty_app(ty_func: TyExpr, ty_arg: TyExpr) -> TyExpr {
    TyExpr::TyApp(TyApplication {
        ty_func: Box::new(ty_func),
        ty_arg: Box::new(ty_arg),
    })
}

/// build an arrow type: a -> b (represented as (Arrow a) b)
/// note: both the syntax-level converter (TyExpr::from) and the builtin resolver
/// (builtins::resolver::lower_type_annot_to_texpr) use these helpers to
/// normalize function types and type application consistently
pub(crate) fn mk_ty_arrow(arg: TyExpr, ret: TyExpr) -> TyExpr {
    ty_app(
        ty_app(
            TyExpr::TyVar(TyVarName::Builtin(TyVarNameBuiltin::Arrow)),
            arg,
        ),
        ret,
    )
}

/// build an arrow type: a -> b -> c -> ... -> ret
/// - eg: mk_ty_arrow_multi([Int, Bool], String) <=> Int -> Bool -> String
/// - note: right associativity
pub(crate) fn mk_ty_arrow_multi(args: Vec<TyExpr>, ret: TyExpr) -> TyExpr {
    args.into_iter()
        .rev()
        .fold(ret, |acc, arg| mk_ty_arrow(arg, acc))
}

/// build the I64 type
pub(crate) fn mk_ty_i64() -> TyExpr {
    TyExpr::TyVar(TyVarName::Builtin(TyVarNameBuiltin::I64))
}

/// build the F64 type
pub(crate) fn mk_ty_f64() -> TyExpr {
    TyExpr::TyVar(TyVarName::Builtin(TyVarNameBuiltin::F64))
}

/// build the String type
pub(crate) fn mk_ty_string() -> TyExpr {
    TyExpr::TyVar(TyVarName::Builtin(TyVarNameBuiltin::String))
}

/// build the Bool type
pub(crate) fn mk_ty_bool() -> TyExpr {
    build_adt_type_no_loc("Bool", &[])
}

/// build the Unit type
pub(crate) fn mk_ty_unit() -> TyExpr {
    TyExpr::TyVar(TyVarName::Builtin(TyVarNameBuiltin::Unit))
}

/// collect the spine of a type application
/// eg: TyApp(TyApp(TyApp(Arrow, Int), Bool), String) returns (Arrow, [Int, Bool, String])
/// eg:
///       @
///      / \
///     @   Tz
///    / \
///   @   Ty
///  / \
/// Tf   Tx
///
/// returns
///
/// (Tf, [Tx, Ty, Tz])
pub(crate) fn collect_spine(ty: TyExpr) -> (TyExpr, Vec<TyExpr>) {
    let mut args = Vec::new();
    let mut current = ty;

    while let TyExpr::TyApp(TyApplication { ty_func, ty_arg }) = current {
        args.push(*ty_arg);
        current = *ty_func;
    }

    args.reverse();
    (current, args)
}

/// check if a type expression is an arrow type
/// returns Some((arg_types, ret_type)) if it's an arrow, None otherwise
pub(crate) fn match_arrow(ty: &TyExpr) -> Option<(Vec<TyExpr>, TyExpr)> {
    let (head, args) = collect_spine(ty.clone());

    match head {
        TyExpr::TyVar(TyVarName::Builtin(TyVarNameBuiltin::Arrow)) if args.len() >= 2 => {
            let ret = args.last().unwrap().clone();
            let arg_types = args[..args.len() - 1].to_vec();
            Some((arg_types, ret))
        }
        _ => None,
    }
}

/// check if a type expression is an builtin integer
pub(crate) fn match_builtin_type(ty: &TyExpr, builtin: &TyVarNameBuiltin) -> bool {
    match ty {
        TyExpr::TyVar(TyVarName::Builtin(x)) if x == builtin => true,
        _ => {
            println!("{:?} does not match expected {:?}", ty, builtin);
            false
        }
    }
}

/// check if a type expression is an auto-generated type variable
pub(crate) fn match_ty_var_auto(ty: &TyExpr) -> bool {
    match ty {
        TyExpr::TyVar(TyVarName::Auto(x)) => true,
        _ => {
            println!("{:?} does not match expected TyVarName::Auto(_)", ty);
            false
        }
    }
}

/// get the head constructor of a type expression
/// for TyApp chains, this traverses to the leftmost element
pub(crate) fn get_head(ty: &TyExpr) -> TyExpr {
    match ty {
        TyExpr::TyApp(app) => get_head(&app.ty_func),
        _ => ty.clone(),
    }
}

/// build ADT type with parameters, using type level application
/// eg: ("List", [I64]) -> TyApp(TyVar("List"), I64)
///
/// TODO: fill in source location
pub(crate) fn build_adt_type_no_loc(type_name: &str, params: &[TyExpr]) -> TyExpr {
    let mut ty = TyExpr::TyVar(TyVarName::UserDefined(TyVarNameUserDefined {
        token: concrete_token::ConcreteToken::Iden(type_name.to_string()),
        loc: None,
    }));

    //left associative application of type exprs
    for param in params {
        ty = ty_app(ty, param.clone());
    }

    ty
}

/// decompose the application spine into (head name, type args) when the head is a
/// user-defined identifier token; returns None for builtin heads or non-identifier
/// tokens; this does not consult the type environment, so the head may or may not be an
/// ADT, and args may be empty
pub(crate) fn decompose_adt_type(ty: &TyExpr) -> Option<(String, Vec<TyExpr>)> {
    let (head, args) = collect_spine(ty.clone());
    match head {
        TyExpr::TyVar(TyVarName::UserDefined(user_defined)) => {
            if let concrete_token::ConcreteToken::Iden(name) = &user_defined.token {
                Some((name.clone(), args))
            } else {
                None
            }
        }
        _ => None,
    }
}

// helpers for debugging ---

impl fmt::Display for TyExpr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt_texpr(self, f, 0)
    }
}

fn fmt_texpr(expr: &TyExpr, f: &mut fmt::Formatter<'_>, prec: u8) -> fmt::Result {
    if let Some((args, ret)) = match_arrow(expr) {
        //terminal case for -> type
        let arrow_prec: u8 = 0;
        let needs_paren: bool = prec > arrow_prec;
        if needs_paren {
            write!(f, "(")?;
        }
        let mut iter = args.into_iter();
        if let Some(first) = iter.next() {
            fmt_texpr(&first, f, arrow_prec + 1)?;
            for arg in iter {
                write!(f, " -> ")?;
                fmt_texpr(&arg, f, arrow_prec + 1)?;
            }
            write!(f, " -> ")?;
            fmt_texpr(&ret, f, arrow_prec)?;
        } else {
            // no args, just return type
            fmt_texpr(&ret, f, arrow_prec)?;
        }
        if needs_paren {
            write!(f, ")")?;
        }
        return Ok(());
    }

    match expr {
        TyExpr::TyVar(name) => write!(f, "{}", display_ty_var_name(name)),
        TyExpr::TyApp(app) => {
            let app_prec: u8 = 1;
            let needs_paren: bool = prec > app_prec;
            if needs_paren {
                write!(f, "(")?;
            }
            fmt_texpr(&app.ty_func, f, app_prec)?;
            write!(f, " ")?;
            fmt_texpr(&app.ty_arg, f, app_prec + 1)?;
            if needs_paren {
                write!(f, ")")?;
            }
            Ok(())
        }
    }
}

fn display_ty_var_name(name: &TyVarName) -> String {
    match name {
        TyVarName::Builtin(b) => match b {
            TyVarNameBuiltin::I64 => "i64".to_string(),
            TyVarNameBuiltin::F64 => "f64".to_string(),
            TyVarNameBuiltin::String => "String".to_string(),
            TyVarNameBuiltin::Bool => "Bool".to_string(),
            TyVarNameBuiltin::Unit => "()".to_string(),
            TyVarNameBuiltin::Arrow => "->".to_string(),
        },
        TyVarName::UserDefined(u) => format!("{}", u.token),
        TyVarName::Auto(id) => format!("t{}", id),
    }
}
