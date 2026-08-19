//! types are made explicit in applications and lambda abstractions by making
//! them as explicit arguments and parameters (eg: System F)
//!
//! we will follow the convention of putting types in front of value-level
//! arguments and parameters

use crate::typecheck::ty_expr::*;
use crate::typecheck::ty_var_name::*;
use crate::typecheck::v_expr::*;
use crate::typecheck::v_expr_typed::*;

#[derive(Clone, Debug)]
pub(crate) enum CoreExpr {
    Abstraction(CoreAbstr),
    Application(CoreApp),
    Case(CoreCase),
    Let(CoreLet),
    Literal(CoreLiteral),
    Variable(CoreVar),
}

/// this includes type level abstraction
///
/// convention is to have all type level parameters in front before any value
/// level parameters
#[derive(Clone, Debug)]
pub(crate) struct CoreAbstr {
    // note: lambda is constrained to have only 1 parameter that is a simple
    // variable
    //
    // for a function that takes no input, use the unit type
    pub param: CoreVar,

    pub body: Box<CoreExpr>,

    pub ty: TyExpr,
}

/// this includes type level application
///
/// convention is to have all type level arguments in front before any value
/// level arguments
#[derive(Clone, Debug)]
pub(crate) struct CoreApp {
    // this can be arbitrary expr, but eventually these are converted/enforced
    // to be simple variables prior to backend code gen
    pub callable: Box<CoreExpr>,

    // note: application is restricted to have only 1 argument
    pub arg: Box<CoreExpr>,

    pub ty: TyExpr,
}

#[derive(Clone, Debug)]
pub(crate) struct CoreCase {
    pub scrutinee: Box<CoreExpr>,

    // binder to result of evaluating the scrutinee
    pub result: CoreVar,

    pub alts: Vec<CoreCaseAlt>,

    // type of case expression
    pub ty: TyExpr,
}

#[derive(Clone, Debug)]
pub(crate) struct CoreLet {
    pub defs: Vec<(CoreVar, CoreExpr)>,

    pub body: Box<CoreExpr>,

    pub ty: TyExpr,
}

#[derive(Clone, Debug)]
pub(crate) enum CoreLiteral {
    // note: borrowing existing structure
    LitNumeric(VLitNumeric),

    // note: borrowing existing structure
    LitString(VLitString),
}

/// either a value-level variable or a type-level variable
#[derive(Clone, Debug)]
pub(crate) enum CoreVar {
    ValueVariable(VVar),

    TypeVariable(TyVarName),
}

#[derive(Clone, Debug)]
pub(crate) struct CoreCaseAlt {
    pub pattern: CoreAltConPattern,

    pub expr: CoreExpr,
}

#[derive(Clone, Debug)]
pub(crate) enum CoreAltConPattern {
    Data(CoreData),
    Literal(CoreLiteral),
}

// [todo]: look up map containing data constructor info and add more info
// explicitly into this structure
#[derive(Clone, Debug)]
struct CoreData {
    pub ty_name: String,
    pub constructor_name: String,
}

/// transform from high level IR to the core IR
pub(crate) fn core_expr_from_typed_v_expr(expr: &TypedVExpr) -> CoreExpr {
    todo!("transform high level IR into core IR")
}
