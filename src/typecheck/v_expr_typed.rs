//! value level constructs with required type annotation, produced after type
//! checking/inference

use crate::typecheck::ty_expr::*;
use crate::typecheck::ty_var_name::*;
use crate::typecheck::v_expr::{RangeBound, VAtom, VConstructorExpr, VPatternLiteral, VVar};

#[derive(Clone, Debug)]
pub(crate) enum TypedVExpr {
    Abstraction(TypedVAbstrExpr),
    Application(TypedVAppExpr),
    Case(TypedVCaseExpr),
    Let(TypedVLetExpr),
    Atom(TypedVAtom),
    Constructor(TypedVConstructorExpr),
}

#[derive(Clone, Debug)]
pub(crate) struct TypedVAbstrExpr {
    pub name: VVar,
    pub params: Vec<TypedVAbstrParam>,
    pub body: Box<TypedVExpr>,
    pub ty: TyExpr,
}

#[derive(Clone, Debug)]
pub(crate) struct TypedVAbstrParam {
    // single argument binder
    //
    // when the pattern is not a variable, we will generate a simple binder for it
    //
    // for a plain variable pattern, binder and pattern refer to the same name
    pub binder: VVar,
    // pattern describing how the binder is matched/destructured
    pub pattern: TypedVPattern,
    pub ty: TyExpr,
}

#[derive(Clone, Debug)]
pub(crate) struct TypedVAppExpr {
    pub callable: Box<TypedVExpr>,
    pub args: Vec<TypedVExpr>,
    pub ty: TyExpr,
}

#[derive(Clone, Debug)]
pub(crate) struct TypedVCaseExpr {
    pub arg: Box<TypedVExpr>,
    pub clauses: Vec<TypedVCaseClause>,
    pub ty: TyExpr,
}

#[derive(Clone, Debug)]
pub(crate) struct TypedVCaseClause {
    pub pattern: TypedVPattern,
    pub guard: Option<TypedVExpr>,
    pub body: TypedVExpr,
}

#[derive(Clone, Debug)]
pub(crate) struct TypedVLetExpr {
    pub defs: Vec<(TypedVPattern, TypedVExpr)>,
    pub body: Box<TypedVExpr>,
    pub ty: TyExpr,
}

#[derive(Clone, Debug)]
pub(crate) struct TypedVAtom {
    pub atom: VAtom,
    pub ty: TyExpr,
    // explicit type args at this use site, for TyApp insertion
    // where order matches the binding's `ty_vars_schematic`
    // e.g. id @Int 3 where ty_args = [Int]
    pub ty_args: Vec<TyExpr>,
}

/// construct for product and record type
#[derive(Clone, Debug)]
pub(crate) struct TypedVConstructorExpr {
    pub constructor: VConstructorExpr,
    pub args: Vec<TypedVExpr>,
    // for record, this associates field name to linear indexing
    pub record_fields: Option<Vec<(String, usize)>>,
    pub ty: TyExpr,
    // explicit type args at this use site, ordered by the constructor's type parameters
    pub ty_args: Vec<TyExpr>,
}

#[derive(Clone, Debug)]
pub(crate) enum TypedVPattern {
    Wild {
        ty: TyExpr,
    },
    Variable {
        binder: VVar,
        ty: TyExpr,
        // this introduces schematic type vars for type abstraction, eg:
        // let id = \x -> x where `ty_vars_schematic` = [a] for forall a. a -> a
        //
        // note: order matters
        ty_vars_schematic: Vec<TyVarName>,
    },
    Literal {
        literal: VPatternLiteral,
        ty: TyExpr,
    },
    Range {
        start: RangeBound<VPatternLiteral>,
        end: RangeBound<VPatternLiteral>,
        ty: TyExpr,
    },
    Constructor {
        ty_name: Option<String>,
        constructor: String,
        args: Vec<TypedVPattern>,
        ty: TyExpr,
    },
    Record {
        ty_name: Option<String>,
        constructor: String,
        fields: Vec<(String, TypedVPattern)>,
        rest: bool, // `..` presence
        ty: TyExpr,
    },
}

impl TypedVExpr {
    pub(crate) fn ty(&self) -> &TyExpr {
        match self {
            TypedVExpr::Abstraction(ab) => &ab.ty,
            TypedVExpr::Application(app) => &app.ty,
            TypedVExpr::Case(case) => &case.ty,
            TypedVExpr::Let(let_expr) => &let_expr.ty,
            TypedVExpr::Atom(atom) => &atom.ty,
            TypedVExpr::Constructor(cons) => &cons.ty,
        }
    }
}

/// builds a typed variable atom expression.
pub(crate) fn mk_typed_vexpr_atom(var: &VVar, ty: &TyExpr) -> TypedVExpr {
    TypedVExpr::Atom(TypedVAtom {
        atom: VAtom::Variable(var.clone()),
        ty: ty.clone(),
        ty_args: Vec::new(),
    })
}

impl TypedVPattern {
    pub(crate) fn ty(&self) -> &TyExpr {
        match self {
            TypedVPattern::Wild { ty }
            | TypedVPattern::Variable { ty, .. }
            | TypedVPattern::Literal { ty, .. }
            | TypedVPattern::Range { ty, .. }
            | TypedVPattern::Constructor { ty, .. }
            | TypedVPattern::Record { ty, .. } => ty,
        }
    }
}
