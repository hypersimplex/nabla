//! types are made explicit in applications and lambda abstractions by making
//! them as explicit arguments and parameters (eg: System F)
//!
//! we will follow the convention of putting types in front of value-level
//! arguments and parameters

use crate::parse::concrete_token::*;
use crate::parse::loc::*;
use crate::typecheck::ty_expr::*;
use crate::typecheck::ty_var_name::*;
use crate::typecheck::v_expr::*;
use crate::typecheck::v_expr_typed::*;

use std::collections::BTreeMap;

/// this represents a top level function
#[derive(Clone, Debug)]
pub(crate) struct CoreTopLevelBinding {
    var_binder: CoreVar,

    expr: CoreAbstr,
}

/// mutually recursive functions in a SCC is grouped together here
#[derive(Clone, Debug)]
pub(crate) struct CoreTopLevelBindingGroup(pub Vec<CoreTopLevelBinding>);

#[derive(Clone, Debug)]
pub(crate) enum CoreExpr {
    // this uniformly treats value and type level abstraction
    Abstraction(CoreAbstr),

    // this uniformly treats value and type level application
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

    pub ty: CoreTy,
}

/// type expression, a type-level construct that contains type info
///
/// explicit forall is introduced for a polymorphic type
#[derive(Clone, Debug)]
pub(crate) enum CoreTy {
    // a placeholder type variable introduced by an outer `ForAll` construct
    Var(TyVarName),

    // concrete type/ADT would belong to this variant
    Constructor(CoreTyCon),

    // type application
    App(CoreTyApp),

    // constructor for introducing a polymorphic placeholder
    ForAll(CoreTyForAll),
}

#[derive(Clone, Debug)]
pub(crate) struct CoreTyCon {
    pub ty_name: String,
    pub constructor_name: String,
}

#[derive(Clone, Debug)]
pub(crate) struct CoreTyApp {
    pub ty_fun: Box<CoreTy>,
    pub ty_arg: Box<CoreTy>,
}

/// construct that introduces a parameteric/polymorphic type
///
/// this is basically a schematic type variable (in context of SPJ's literature)
///
/// note: multiple parameteric types are represented in nested form
#[derive(Clone, Debug)]
pub(crate) struct CoreTyForAll {
    // type variable introduced for the parameteric type
    ty_var: TyVarName,

    // remaining type expression that may reference the introduced parameteric type
    ty_expr: Box<CoreTy>,
}

/// this includes type level application
///
/// convention is to have all type level arguments in front before any value
/// level arguments
#[derive(Clone, Debug)]
pub(crate) struct CoreApp {
    pub callable: Box<CoreExpr>,

    // restricted to have only 1 argument
    //
    // multiple arguments is done via currying
    pub arg: Box<CoreExpr>,

    pub ty: CoreTy,
}

/// case construct in core IR introduces explicit variable binder for the result
/// of scrutinee evaluation
#[derive(Clone, Debug)]
pub(crate) struct CoreCase {
    pub scrutinee: Box<CoreExpr>,

    // binder to result of evaluating the scrutinee
    pub result: CoreVar,

    pub alts: Vec<CoreCaseAlt>,

    // type of case expression
    pub ty: CoreTy,
}

#[derive(Clone, Debug)]
pub(crate) struct CoreLet {
    pub defs: Vec<(CoreVar, CoreExpr)>,

    pub body: Box<CoreExpr>,

    pub ty: CoreTy,
}

#[derive(Clone, Debug)]
pub(crate) enum CoreLiteral {
    LitNumericIntegral(CoreLitNumericIntegral),

    LitNumericFloat(CoreLitNumericFloat),

    LitString(CoreLitString),
}

#[derive(Clone, Debug)]
pub(crate) struct CoreLitNumericIntegral {
    pub loc: Option<Location>,

    pub value: i64,
}

#[derive(Clone, Debug)]
pub(crate) struct CoreLitNumericFloat {
    pub loc: Option<Location>,

    pub value: f64,
}

#[derive(Clone, Debug)]
pub(crate) struct CoreLitString {
    pub loc: Option<Location>,

    pub value: String,
}

/// this is either a value-level variable or a type-level variable
#[derive(Clone, Debug)]
pub(crate) enum CoreVar {
    ValueVariable(CoreVVar),

    TypeVariable(CoreTyVar),
}

#[derive(Clone, Debug)]
pub(crate) struct CoreVVar {
    pub vvar: VVar,

    pub ty: CoreTy,
}

#[derive(Clone, Debug)]
pub(crate) struct CoreTyVar {
    pub ty_var: TyVarName,

    pub ty: CoreTy,
}

#[derive(Clone, Debug)]
pub(crate) struct CoreCaseAlt {
    pub pattern: CoreAltConPattern,

    pub expr: CoreExpr,
}

#[derive(Clone, Debug)]
pub(crate) enum CoreAltConPattern {
    Data(CoreTyCon),
    Literal(CoreLiteral),
}

/// transform from high level IR to the core IR
pub(crate) fn core_typed_top_level_function_group(
    group: &BTreeMap<usize, TypedTopLevelFunction>,
) -> CoreTopLevelBindingGroup {
    todo!("transform high level IR into core IR");
}

/// transform from high level IR to the core IR
pub(crate) fn core_expr_from_typed_v_expr(expr: &TypedVExpr) -> CoreExpr {
    todo!("transform high level IR into core IR");
    // use TypedVExpr::*;
    // match expr {
    //     Abstraction(x) => core_expr_from_abstraction(x),
    //     Application(x) => core_expr_from_application(x),
    //     Case(x) => core_expr_from_case(x),
    //     Let(x) => core_expr_from_let(x),
    //     LitNumeric(x) => core_expr_from_lit_num(x),
    //     LitString(x) => core_expr_from_lit_string(x),
    //     TypedVExpr::Variable(x) => core_expr_from_variable(x),
    //     Constructor(x) => core_expr_from_constructor(x),
    // }
}
