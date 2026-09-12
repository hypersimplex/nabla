//! types are made explicit in applications and lambda abstractions by making
//! them as explicit arguments and parameters (eg: System F)
//!
//! we will follow the convention of putting types in front of value-level
//! arguments / parameters

use crate::parse::concrete_token::*;
use crate::parse::loc::*;
use crate::typecheck::subst::*;
use crate::typecheck::ty_expr::*;
use crate::typecheck::ty_inference::*;
use crate::typecheck::ty_scheme::*;
use crate::typecheck::ty_var_name::*;
use crate::typecheck::v_expr::*;
use crate::typecheck::v_expr_typed::*;
use crate::util::printer::*;

use std::collections::BTreeMap;

/// this represents a top level function
#[derive(Clone, Debug)]
pub(crate) struct CoreTopLevelBinding {
    var_binder: CoreVar,

    abstraction: CoreAbstr,
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

    Type(CoreTy),
}

impl CoreExpr {
    fn ty(&self) -> CoreTy {
        use CoreExpr::*;
        match self {
            Abstraction(x) => x.ty(),
            Application(x) => x.ty(),
            Case(x) => x.ty(),
            Let(x) => x.ty(),
            Literal(x) => x.ty(),
            Variable(x) => x.ty(),
            Type(x) => x.clone(),
        }
    }
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

impl CoreAbstr {
    fn ty(&self) -> CoreTy {
        self.ty.clone()
    }
}

/// type expression, a type-level construct that contains type info
///
/// explicit forall is introduced for a polymorphic type
#[derive(Clone, Debug)]
pub(crate) enum CoreTy {
    // a placeholder type variable introduced by an outer `ForAll` construct
    //
    // eg: a polymorphic type
    Var(TyVarName),

    // type constructor
    //
    // this also includes builtin type
    TyConstructor(CoreTyCon),

    // type application
    //
    // essentially a list like structure if there is more than one arguments
    App(CoreTyApp),

    // constructor for introducing a polymorphic placeholder
    //
    // esentially nests another inner CoreTy inside
    //
    // convention is to have all `ForAll` in the outer-most/front layer of a
    // CoreTy expression
    ForAll(CoreTyForAll),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum CoreTyCon {
    Builtin(CoreTyConBuiltin),
    User(CoreTyConUser),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum CoreTyConBuiltin {
    Int,
    Float,
    String,
    Arrow,
}

// [todo,fix]: convert to ty con to start with, and remove this conversion
impl<'a> From<&'a TyVarNameBuiltin> for CoreTyConBuiltin {
    /// temporary hack to convert builtin ty var to type constructor
    fn from(builtin: &'a TyVarNameBuiltin) -> Self {
        use TyVarNameBuiltin::*;
        match builtin {
            I64 => CoreTyConBuiltin::Int,
            F64 => CoreTyConBuiltin::Float,
            String => CoreTyConBuiltin::String,
            Arrow => CoreTyConBuiltin::Arrow,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CoreTyConUser {
    // type constructor name
    pub name: String,
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
    pub ty_var: TyVarName,

    // remaining type expression that may reference the introduced parameteric type
    pub ty_expr: Box<CoreTy>,
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

impl CoreApp {
    fn ty(&self) -> CoreTy {
        self.ty.clone()
    }
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

impl CoreCase {
    fn ty(&self) -> CoreTy {
        self.ty.clone()
    }
}

#[derive(Clone, Debug)]
pub(crate) struct CoreLet {
    pub defs: Vec<(CoreVar, CoreExpr)>,

    pub body: Box<CoreExpr>,

    pub ty: CoreTy,
}

impl CoreLet {
    fn ty(&self) -> CoreTy {
        self.ty.clone()
    }
}

#[derive(Clone, Debug)]
pub(crate) enum CoreLiteral {
    LitNumericIntegral(CoreLitNumericIntegral),

    LitNumericFloat(CoreLitNumericFloat),

    LitString(CoreLitString),
}

impl CoreLiteral {
    fn ty(&self) -> CoreTy {
        use CoreLiteral::*;
        match self {
            LitNumericIntegral(x) => x.ty(),
            LitNumericFloat(x) => x.ty(),
            LitString(x) => x.ty(),
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct CoreLitNumericIntegral {
    pub loc: Option<Location>,

    pub value: i64,
}

impl CoreLitNumericIntegral {
    fn ty(&self) -> CoreTy {
        CoreTy::TyConstructor(CoreTyCon::Builtin(CoreTyConBuiltin::Int))
    }
}

#[derive(Clone, Debug)]
pub(crate) struct CoreLitNumericFloat {
    pub loc: Option<Location>,

    pub value: f64,
}

impl CoreLitNumericFloat {
    fn ty(&self) -> CoreTy {
        CoreTy::TyConstructor(CoreTyCon::Builtin(CoreTyConBuiltin::Float))
    }
}

#[derive(Clone, Debug)]
pub(crate) struct CoreLitString {
    pub loc: Option<Location>,

    pub value: String,
}

impl CoreLitString {
    fn ty(&self) -> CoreTy {
        CoreTy::TyConstructor(CoreTyCon::Builtin(CoreTyConBuiltin::String))
    }
}

/// this is either a value-level variable or a type-level variable
#[derive(Clone, Debug)]
pub(crate) enum CoreVar {
    ValueVariable(CoreVVar),

    TypeVariable(CoreTyVar),
}

impl CoreVar {
    fn ty(&self) -> CoreTy {
        use CoreVar::*;
        match self {
            ValueVariable(x) => x.ty(),
            TypeVariable(x) => x.ty(),
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct CoreVVar {
    pub vvar: VVar,

    pub ty: CoreTy,
}

impl CoreVVar {
    fn ty(&self) -> CoreTy {
        self.ty.clone()
    }
}

#[derive(Clone, Debug)]
pub(crate) struct CoreTyVar {
    pub ty_var: TyVarName,

    pub ty: CoreTy,
}

impl CoreTyVar {
    fn ty(&self) -> CoreTy {
        self.ty.clone()
    }
}

#[derive(Clone, Debug)]
pub(crate) struct CoreCaseAlt {
    pub pattern: CoreAltConPattern,

    pub expr: CoreExpr,
}

/// note: after desugaring to core, supported patterns are quite restricted
#[derive(Clone, Debug)]
pub(crate) enum CoreAltConPattern {
    Data(CoreTyCon),
    Literal(CoreLiteral),
}

/// mechanical translation
///
/// [todo, fix]: convert builtin ty var to type constructor from the start
fn core_ty_from_ty_expr(ty_expr: &TyExpr) -> CoreTy {
    match ty_expr {
        TyExpr::TyVar(ty_var_name) => match ty_var_name {
            TyVarName::Builtin(builtin) => {
                CoreTy::TyConstructor(CoreTyCon::Builtin(builtin.into()))
            }
            _ => CoreTy::Var(ty_var_name.clone()),
        },
        TyExpr::TyApp(TyApplication { ty_func, ty_arg }) => CoreTy::App(CoreTyApp {
            ty_fun: Box::new(core_ty_from_ty_expr(&ty_func)),
            ty_arg: Box::new(core_ty_from_ty_expr(&ty_arg)),
        }),
    }
}

/// note: schematic type variables are translated to `ForAll`s and these
/// schematic type variables are wrapped outside by convention
fn core_ty_from_ty_scheme(ty_scheme: &TyScheme) -> CoreTy {
    let mut core_ty = core_ty_from_ty_expr(&ty_scheme.ty_expr);
    // note: right associative so process in reverse order
    for ty_var_name in ty_scheme.ty_vars_schematic.iter().rev() {
        core_ty = CoreTy::ForAll(CoreTyForAll {
            ty_var: ty_var_name.clone(),
            ty_expr: Box::new(core_ty),
        });
    }
    core_ty
}

/// transform from high level IR to the core IR
pub(crate) fn core_typed_top_level_function_group(
    group: &BTreeMap<usize, TypedTopLevelFunction>,
) -> CoreTopLevelBindingGroup {
    let mut ret = vec![];
    for (id, top_lvl_function) in group.iter() {
        ret.push(core_top_level_binding_from_typed_top_level_function(
            top_lvl_function,
        ));
    }
    CoreTopLevelBindingGroup(ret)
}

/// transform from high level IR to the core IR
///
/// note: take schematic type variables in the type scheme of the binder of
/// the function and construct explicit core IR type parameters
pub(crate) fn core_top_level_binding_from_typed_top_level_function(
    top_lvl_fn: &TypedTopLevelFunction,
) -> CoreTopLevelBinding {
    let core_expr = core_expr_from_typed_v_expr(&top_lvl_fn.typed_expr);

    let mut core_abstraction = match core_expr {
        CoreExpr::Abstraction(abstr) => abstr,
        _ => {
            unreachable!()
        }
    };

    // binder may contain schematic type variables,
    // which are translated to `ForAll`s, eg:
    //   ForAll a. ForAll b. a -> b -> i64
    // so we construct explicit type parameters for these `ForAll`s
    let top_level_fn_binder = CoreVar::ValueVariable(CoreVVar {
        vvar: top_lvl_fn.name.clone(),
        ty: core_ty_from_ty_scheme(&top_lvl_fn.scheme),
    });

    CoreTopLevelBinding {
        var_binder: top_level_fn_binder,
        abstraction: core_abstraction,
    }
}

/// transform from high level IR to the core IR
pub(crate) fn core_expr_from_typed_v_expr(expr: &TypedVExpr) -> CoreExpr {
    use TypedVExpr::*;
    match expr {
        Abstraction(x) => core_expr_from_abstraction(x),
        Application(x) => core_expr_from_application(x),
        Case(x) => core_expr_from_case(x),
        Let(x) => core_expr_from_let(x),
        LitNumeric(x) => core_expr_from_lit_num(x),
        LitString(x) => core_expr_from_lit_string(x),
        Variable(x) => core_expr_from_variable(x),
        Constructor(x) => core_expr_from_constructor(x),
    }
}

/// core lambda abstraction contains 1 parameter; original abstractions with
/// multiple parameters are converted to a nested form
pub(crate) fn core_expr_from_abstraction(expr: &TypedVAbstrExpr) -> CoreExpr {
    let TypedVAbstrExpr { params, body, ty } = expr;

    let mut ty_abstraction = body.ty().clone();
    let mut core_expr = core_expr_from_typed_v_expr(body);
    // note: process in reverse order since the arrow type is right associative
    for param in params.iter().rev() {
        ty_abstraction = mk_ty_arrow(param.ty.clone(), ty_abstraction);
        core_expr = CoreExpr::Abstraction(CoreAbstr {
            param: CoreVar::ValueVariable(CoreVVar {
                vvar: param.binder.clone(),
                ty: core_ty_from_ty_expr(&param.ty),
            }),
            body: Box::new(core_expr),
            ty: core_ty_from_ty_expr(&ty_abstraction),
        });
    }
    core_expr
}

/// perform value-level application
///
/// eg:
///
/// a -> b -> c
/// (->) a ((->) b) c)
/// (App (App (->) a) (App (App (->) b) c))
///
///      App
///      /  \
///   App    App
///  /  \    |  \
/// ->   a  App  c
///         / \
///        ->  b
///
/// apply a, then the result is `b->c`
/// which corresponds to taking the right subtree of the root
pub(crate) fn core_expr_from_application(expr: &TypedVAppExpr) -> CoreExpr {
    let mut core_expr = core_expr_from_typed_v_expr(&expr.callable);
    for arg in expr.args.iter() {
        // [todo]: maybe check type of argument against expected parameter type
        // of the callable
        let core_expr_arg = core_expr_from_typed_v_expr(arg);

        let core_ty = core_expr.ty().clone();
        // this is the resulting type after the application
        let core_ty_result = match core_ty {
            CoreTy::App(app) => {
                let CoreTyApp { ty_fun, ty_arg } = app;
                (*ty_arg).clone()
            }
            _ => {
                unreachable!("expected CoreTyApp, but got {:?}", &core_ty);
            }
        };

        core_expr = CoreExpr::Application(CoreApp {
            callable: Box::new(core_expr),
            arg: Box::new(core_expr_arg),
            ty: core_ty_result,
        });
    }
    core_expr
}

pub(crate) fn core_expr_from_case(expr: &TypedVCaseExpr) -> CoreExpr {
    // this is guaranteed to be a simple variable
    let core_expr_scrutinee = core_expr_from_typed_v_expr(&*expr.arg);

    let scrutinee_var = match &core_expr_scrutinee {
        CoreExpr::Variable(scrutinee_var) => scrutinee_var.clone(),
        _ => unreachable!(),
    };

    let core_expr = CoreCase {
        scrutinee: Box::new(core_expr_scrutinee),

        // binder to result of evaluating the scrutinee
        result: scrutinee_var,

        alts: expr
            .clauses
            .iter()
            .map(|x| core_case_alt_from_typed_v_case_alt(x))
            .collect(), // convert TypedVCaseClause to CoreCaseAlt

        ty: core_ty_from_ty_expr(&expr.ty),
    };
    CoreExpr::Case(core_expr)
}

/// [todo]: for each let definition, take the schematic type variables in the scheme
/// of the LHS variable binder and construct explicit core IR type parameters
pub(crate) fn core_expr_from_let(expr: &TypedVLetExpr) -> CoreExpr {
    let defs = expr
        .defs
        .iter()
        .map(|(lhs, rhs)| match lhs {
            TypedVPattern::Variable {
                binder,
                ty,
                ty_schematic,
            } => {
                // explicitly include generic/parameteric types in type expr
                let mut ty_expr = core_ty_from_ty_expr(ty);
                for ty_var_name in ty_schematic.ty_vars_schematic.iter().rev() {
                    ty_expr = CoreTy::ForAll(CoreTyForAll {
                        ty_var: ty_var_name.clone(),
                        ty_expr: Box::new(ty_expr),
                    });
                }

                let lhs_var = CoreVar::ValueVariable(CoreVVar {
                    vvar: binder.clone(),
                    ty: ty_expr,
                });
                (lhs_var, core_expr_from_typed_v_expr(rhs))
            }
            _ => {
                unreachable!();
            }
        })
        .collect();

    CoreExpr::Let(CoreLet {
        // defs: Vec<(CoreVar, CoreExpr)>,
        defs,
        body: Box::new(core_expr_from_typed_v_expr(&*expr.body)),
        ty: core_ty_from_ty_expr(&expr.ty),
    })
}

pub(crate) fn core_expr_from_lit_num(expr: &TypedVLitNumeric) -> CoreExpr {
    match &expr.val.value {
        NumericLiteralValue::Int { raw, parsed } => {
            CoreExpr::Literal(CoreLiteral::LitNumericIntegral(CoreLitNumericIntegral {
                loc: expr.val.loc.clone(),
                value: parsed.unwrap(),
            }))
        }
        NumericLiteralValue::Float { raw, parsed } => {
            CoreExpr::Literal(CoreLiteral::LitNumericFloat(CoreLitNumericFloat {
                loc: expr.val.loc.clone(),
                value: parsed.unwrap(),
            }))
        }
    }
}

pub(crate) fn core_expr_from_lit_string(expr: &TypedVLitString) -> CoreExpr {
    let value = match &expr.val.token {
        ConcreteToken::LiteralString(x) => x.clone(),
        _ => {
            unreachable!();
        }
    };
    CoreExpr::Literal(CoreLiteral::LitString(CoreLitString {
        loc: expr.val.loc.clone(),
        value,
    }))
}

/// note: a variable at a use site may have specific types applied
/// and we explicitly encode these as type arguments
fn core_expr_from_variable(expr: &TypedVVariable) -> CoreExpr {
    let TypedVVariable {
        var,
        ty_schematic, // original schematic type of the thing the variable is binded to
        ty_args,      // type arguments that need to be applied
        ..
    } = expr;
    let core_vvar = CoreExpr::Variable(CoreVar::ValueVariable(CoreVVar {
        vvar: var.clone(),
        ty: core_ty_from_ty_scheme(ty_schematic), // this may contain `ForAll`
    }));

    let mut core_expr = core_vvar;

    // if the variable is binded to a polymorphic type, and there are `ty_args`
    // then we perform type-level application using these `ty_args`

    // currying: accumulate updated schematic type as we apply type argument one by one
    let mut ty_schematic_substitued = ty_schematic.clone();
    for (ty_arg, ty_schematic_var) in ty_args.iter().zip(ty_schematic.ty_vars_schematic.iter()) {
        let core_ty_arg = core_ty_from_ty_expr(ty_arg);

        // do a reduction, ty_schematic[ty_arg\ty_schematic_var],
        // eg: beta reduction by type by substituting `ty_arg` for
        // `ty_schematic_var` and return the new `ty_schematic`;
        // this should have one less schematic type variable
        ty_schematic_substitued =
            apply_ty_scheme(ty_schematic_var, ty_arg, &ty_schematic_substitued);

        // result type after applying the current type argument `core_ty_arg` to
        // `core_expr`
        let core_ty = core_ty_from_ty_scheme(&ty_schematic_substitued);

        core_expr = CoreExpr::Application(CoreApp {
            callable: Box::new(core_expr),
            arg: Box::new(CoreExpr::Type(core_ty_arg)),
            ty: core_ty,
        });
    }

    core_expr
}

/// transform constructor expression to an application of constructor function
/// with constructor arguments
fn core_expr_from_constructor(expr: &TypedVConstructorExpr) -> CoreExpr {
    todo!("core_expr_from_constructor")
}

fn core_case_alt_from_typed_v_case_alt(expr: &TypedVCaseClause) -> CoreCaseAlt {
    todo!("core_case_alt_from_typed_v_case_alt")
}

// [todo]
// helper impl. for doc printer trait --->>

impl DocPrinter for CoreTopLevelBinding {
    fn to_doc(&self) -> Box<Doc> {
        todo!()
    }
}

impl DocPrinter for CoreExpr {
    fn to_doc(&self) -> Box<Doc> {
        use CoreExpr::*;
        match self {
            Abstraction(x) => x.to_doc(),
            Application(x) => x.to_doc(),
            Case(x) => x.to_doc(),
            Let(x) => x.to_doc(),
            Literal(x) => x.to_doc(),
            Variable(x) => x.to_doc(),
            Type(x) => x.to_doc(),
        }
    }
}

impl DocPrinter for CoreAbstr {
    fn to_doc(&self) -> Box<Doc> {
        todo!()
    }
}

impl DocPrinter for CoreApp {
    fn to_doc(&self) -> Box<Doc> {
        todo!()
    }
}

impl DocPrinter for CoreCase {
    fn to_doc(&self) -> Box<Doc> {
        todo!()
    }
}

impl DocPrinter for CoreLet {
    fn to_doc(&self) -> Box<Doc> {
        todo!()
    }
}

impl DocPrinter for CoreLiteral {
    fn to_doc(&self) -> Box<Doc> {
        todo!()
    }
}

impl DocPrinter for CoreVar {
    fn to_doc(&self) -> Box<Doc> {
        use CoreVar::*;
        match self {
            ValueVariable(x) => x.to_doc(),
            TypeVariable(x) => x.to_doc(),
        }
    }
}

impl DocPrinter for CoreVVar {
    fn to_doc(&self) -> Box<Doc> {
        todo!()
    }
}

impl DocPrinter for CoreTyVar {
    fn to_doc(&self) -> Box<Doc> {
        todo!()
    }
}

impl DocPrinter for CoreTy {
    fn to_doc(&self) -> Box<Doc> {
        use CoreTy::*;
        match self {
            Var(x) => x.to_doc(),
            TyConstructor(x) => x.to_doc(),
            App(x) => x.to_doc(),
            ForAll(x) => x.to_doc(),
        }
    }
}

impl DocPrinter for CoreTyCon {
    fn to_doc(&self) -> Box<Doc> {
        todo!()
    }
}

impl DocPrinter for CoreTyApp {
    fn to_doc(&self) -> Box<Doc> {
        Doc::lit("(")
            .cat(self.ty_fun.to_doc())
            .cat_space(self.ty_arg.to_doc())
            .cat_lit(")")
    }
}

impl DocPrinter for CoreTyForAll {
    fn to_doc(&self) -> Box<Doc> {
        Doc::lit("forall")
            .cat_space(self.ty_var.to_doc())
            .cat_space_lit(".")
            .cat_space(self.ty_expr.to_doc())
    }
}

// <<--- helper impl. for doc printer trait
