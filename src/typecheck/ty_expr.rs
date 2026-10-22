use crate::parse::abstr_structures;
use crate::parse::concrete_token;
use crate::typecheck::ty_var_name::*;
use crate::util::printer::*;
use std::iter::IntoIterator;

/// type expression, a type-level construct that contains type info
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum TyExpr {
    // a type variable used for solving type equations
    TyVar(TyVarName),

    // a concrete type constructor (eg: i64, f64, ->, or any ADT)
    TyCon(TyCon),

    TyApp(TyApplication),
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum TyCon {
    Builtin(TyConBuiltin),

    // user defined
    User(String),
}

impl TyCon {
    pub(crate) fn name(&self) -> &str {
        match self {
            TyCon::Builtin(b) => b.name(),
            TyCon::User(s) => s.as_str(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum TyConBuiltin {
    I64,
    F64,
    Bytes,
    Arrow,
}

impl TyConBuiltin {
    pub(crate) fn arity(&self) -> usize {
        match self {
            TyConBuiltin::I64 => 0,
            TyConBuiltin::F64 => 0,
            TyConBuiltin::Bytes => 0,
            TyConBuiltin::Arrow => 2,
        }
    }

    pub(crate) fn name(&self) -> &'static str {
        match self {
            TyConBuiltin::I64 => "i64",
            TyConBuiltin::F64 => "f64",
            TyConBuiltin::Bytes => "Bytes",
            TyConBuiltin::Arrow => "->",
        }
    }

    pub(crate) fn is_unboxed(&self) -> bool {
        matches!(
            self,
            TyConBuiltin::I64 | TyConBuiltin::F64 | TyConBuiltin::Bytes
        )
    }
}

/// type expression application (analogous to value-level application) where
/// this corresponds to substitution for schematic type parameters in
/// a polymorphic type
///
/// this subsumes simple compound type
///
/// eg:
///
/// a -> b
///
/// equivalent to:
///
/// ((->) a) b
/// (App (App (->) a) b)
///
/// ```text
///      App
///      / \
///   App   b
///  /  \
/// ->   a
/// ```
///
#[derive(Clone, Debug, PartialEq, Eq)]
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
    MatchFail,
    ConvertI64FromF64,
    ConvertF64FromI64,
    ConvertInt64FromFloat64,
    ConvertFloat64FromInt64,
}

impl From<&abstr_structures::BuiltinExprType> for FnBuiltin {
    fn from(expr_type: &abstr_structures::BuiltinExprType) -> Self {
        use abstr_structures::BuiltinExprType::*;
        match expr_type {
            UnaryPlus => FnBuiltin::UnaryPlus,
            UnaryNegate => FnBuiltin::UnaryNegate,
            UnaryLogicalNot => FnBuiltin::UnaryLogicalNot,
            BinaryAdd => FnBuiltin::BinaryAdd,
            BinarySub => FnBuiltin::BinarySub,
            BinaryMul => FnBuiltin::BinaryMul,
            BinaryDiv => FnBuiltin::BinaryDiv,
            BinaryLess => FnBuiltin::BinaryLess,
            BinaryLessEqual => FnBuiltin::BinaryLessEqual,
            BinaryGreater => FnBuiltin::BinaryGreater,
            BinaryGreaterEqual => FnBuiltin::BinaryGreaterEqual,
            BinaryEqual => FnBuiltin::BinaryEqual,
            LogicalAnd => FnBuiltin::LogicalAnd,
            LogicalOr => FnBuiltin::LogicalOr,
        }
    }
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
///
/// this helpers is used to normalize function types and type application
/// consistently
pub(crate) fn mk_ty_arrow(arg: TyExpr, ret: TyExpr) -> TyExpr {
    ty_app(
        ty_app(TyExpr::TyCon(TyCon::Builtin(TyConBuiltin::Arrow)), arg),
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
    TyExpr::TyCon(TyCon::Builtin(TyConBuiltin::I64))
}

/// build the F64 type
pub(crate) fn mk_ty_f64() -> TyExpr {
    TyExpr::TyCon(TyCon::Builtin(TyConBuiltin::F64))
}

/// build the Bytes primitive type
pub(crate) fn mk_ty_bytes() -> TyExpr {
    TyExpr::TyCon(TyCon::Builtin(TyConBuiltin::Bytes))
}

/// build the String type
pub(crate) fn mk_ty_string() -> TyExpr {
    build_adt_type_no_loc("String", &[])
}

/// build the Int64 boxed type
pub(crate) fn mk_ty_int64() -> TyExpr {
    build_adt_type_no_loc("Int64", &[])
}

/// build the Float64 boxed type
pub(crate) fn mk_ty_float64() -> TyExpr {
    build_adt_type_no_loc("Float64", &[])
}

/// build the Bool type
pub(crate) fn mk_ty_bool() -> TyExpr {
    build_adt_type_no_loc("Bool", &[])
}

/// build the Unit type
#[allow(dead_code)]
pub(crate) fn mk_ty_unit() -> TyExpr {
    build_adt_type_no_loc("Unit", &[])
}

impl TyExpr {
    pub(crate) fn is_unboxed_primitive(&self) -> bool {
        match self {
            TyExpr::TyCon(TyCon::Builtin(b)) => b.is_unboxed(),
            _ => false,
        }
    }
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

/// build ADT type with type parameters, using type level application
/// eg: ("List", [I64]) -> TyApp(TyCon(User("List")), I64)
///
/// todo: fill in source location
pub(crate) fn build_adt_type_no_loc(type_name: &str, params: &[TyExpr]) -> TyExpr {
    let mut ty = TyExpr::TyCon(TyCon::User(type_name.to_string()));

    //left associative application of type exprs
    for param in params {
        ty = ty_app(ty, param.clone());
    }

    ty
}

// helper impl. for doc printer trait --->>

impl DocPrinter for TyExpr {
    fn to_doc(&self) -> Box<Doc> {
        use TyExpr::*;
        match self {
            TyVar(ty_var_name) => ty_var_name.to_doc(),
            TyCon(ty_con) => ty_con.to_doc(),
            TyApp(ty_app) => ty_app.to_doc(),
        }
    }
}

impl DocPrinter for TyCon {
    fn to_doc(&self) -> Box<Doc> {
        match self {
            TyCon::Builtin(b) => b.to_doc(),
            TyCon::User(name) => Doc::lit(name),
        }
    }
}

impl DocPrinter for TyConBuiltin {
    fn to_doc(&self) -> Box<Doc> {
        match self {
            TyConBuiltin::I64 => Doc::lit("i64#"),
            TyConBuiltin::F64 => Doc::lit("f64#"),
            TyConBuiltin::Bytes => Doc::lit("Bytes#"),
            TyConBuiltin::Arrow => Doc::lit("->"),
        }
    }
}

impl DocPrinter for TyApplication {
    fn to_doc(&self) -> Box<Doc> {
        Doc::lit("(")
            .cat(self.ty_func.to_doc().cat_space(self.ty_arg.to_doc()))
            .cat_lit(")")
    }
}

// <<--- helper impl. for doc printer trait
