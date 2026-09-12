//! value level constructs with required type annotation, produced after type
//! checking/inference

use crate::typecheck::ty_expr::*;
use crate::typecheck::ty_scheme::*;
use crate::typecheck::v_expr::*;
use crate::util::printer::*;

#[derive(Clone, Debug)]
pub(crate) struct TypedTopLevelFunction {
    // analogous to LHS binding of let expression
    pub name: VVar,

    pub scheme: TyScheme,

    // this is guaranteed to be a lambda abstraction
    pub typed_expr: TypedVExpr,
}

#[derive(Clone, Debug)]
pub(crate) enum TypedVExpr {
    Abstraction(TypedVAbstrExpr),
    Application(TypedVAppExpr),
    Case(TypedVCaseExpr),
    Let(TypedVLetExpr),
    LitNumeric(TypedVLitNumeric),
    LitString(TypedVLitString),
    Variable(TypedVVariable),
    Constructor(TypedVConstructorExpr),
}

// [todo]
#[derive(Clone, Debug)]
pub(crate) struct TypedVLitNumeric {
    pub val: VLitNumeric,

    pub ty: TyExpr,
}

// [todo]
#[derive(Clone, Debug)]
pub(crate) struct TypedVLitString {
    pub val: VLitString,

    pub ty: TyExpr,
}

/// represents callsite/reference of a variable
///
/// note: definition of variable is done in let expression / top level
/// definition
#[derive(Clone, Debug)]
pub(crate) struct TypedVVariable {
    pub var: VVar,

    // this is the type that is the result of applying substitutions during type
    // inference
    pub ty: TyExpr,

    // explicit type args at this use site
    //
    // [todo]: for TyApp insertion, order should match the binding's
    // `ty_vars_schematic`
    //
    // e.g.: id @Int 3 where ty_args = [Int]
    pub ty_args: Vec<TyExpr>,

    // note: this keep the original scheme of a function/let definition
    // that is binded to the variable
    pub ty_schematic: TyScheme,
}

#[derive(Clone, Debug)]
pub(crate) struct TypedVAbstrExpr {
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

/// construct for product and record type
#[derive(Clone, Debug)]
pub(crate) struct TypedVConstructorExpr {
    // type name (resolved during type checking)
    pub ty_name: String,

    pub constructor_name: String,

    pub args: Vec<TypedVExpr>,

    // for record, this associates field name to linear indexing
    pub record_fields: Option<Vec<(String, usize)>>,

    pub ty: TyExpr,

    // explicit type args at this use site, ordered by the constructor's type
    // parameters
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
        ty_schematic: TyScheme,
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
        ty_name: String,
        constructor: String,
        args: Vec<TypedVPattern>,
        ty: TyExpr,

        // possibly type arguments applied to the ADT
        ty_args: Vec<TyExpr>,
    },
    Record {
        ty_name: Option<String>,
        constructor: String,
        fields: Vec<(String, TypedVPattern)>,
        rest: bool, // `..` presence
        ty: TyExpr,

        // possibly type arguments applied to the ADT
        ty_args: Vec<TyExpr>,
    },
}

impl TypedVExpr {
    pub(crate) fn ty(&self) -> &TyExpr {
        match self {
            TypedVExpr::Abstraction(ab) => &ab.ty,
            TypedVExpr::Application(app) => &app.ty,
            TypedVExpr::Case(case) => &case.ty,
            TypedVExpr::Let(let_expr) => &let_expr.ty,
            TypedVExpr::LitNumeric(x) => &x.ty,
            TypedVExpr::LitString(x) => &x.ty,
            TypedVExpr::Variable(x) => &x.ty,
            TypedVExpr::Constructor(cons) => &cons.ty,
        }
    }
}

pub(crate) fn mk_typed_vexpr_from_v_lit_numeric(lit: &VLitNumeric) -> TypedVExpr {
    TypedVExpr::LitNumeric(TypedVLitNumeric {
        val: lit.clone(),
        ty: lit.ty(),
    })
}

pub(crate) fn mk_typed_vexpr_from_v_lit_string(lit: &VLitString) -> TypedVExpr {
    TypedVExpr::LitString(TypedVLitString {
        val: lit.clone(),
        ty: lit.ty(),
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

// helper impl. for doc printer trait --->>

impl DocPrinter for TypedTopLevelFunction {
    fn to_doc(&self) -> Box<Doc> {
        let doc_name = Doc::lit("(")
            .cat(self.name.to_doc())
            .cat(Doc::lit(" :: ").cat(self.scheme.to_doc()))
            .cat_lit(")");
        doc_name
            .cat_space_lit("=")
            .cat_space(self.typed_expr.to_doc().nest(4))
    }
}

impl DocPrinter for TypedVExpr {
    fn to_doc(&self) -> Box<Doc> {
        use TypedVExpr::*;
        match self {
            Abstraction(x) => x.to_doc(),
            Application(x) => x.to_doc(),
            Case(x) => x.to_doc(),
            Let(x) => x.to_doc(),
            LitNumeric(x) => x.to_doc(),
            LitString(x) => x.to_doc(),
            Variable(x) => x.to_doc(),
            Constructor(x) => x.to_doc(),
        }
    }
}

impl DocPrinter for TypedVAbstrExpr {
    fn to_doc(&self) -> Box<Doc> {
        let mut rhs = Doc::lit("\\");
        for param in self.params.iter() {
            rhs = rhs.cat_space(param.to_doc());
        }
        rhs = rhs.cat_space_lit("->");
        let doc_body = self.body.to_doc();

        rhs = rhs.cat_space(doc_body.nest(4));
        rhs.group()
    }
}

impl DocPrinter for TypedVAbstrParam {
    fn to_doc(&self) -> Box<Doc> {
        Doc::lit("(")
            .cat(
                self.binder
                    .to_doc()
                    .cat_space_lit("::")
                    .cat_space(self.ty.to_doc()),
            )
            .cat_lit(")")
    }
}

impl DocPrinter for TypedVAppExpr {
    fn to_doc(&self) -> Box<Doc> {
        let doc_app = self.callable.to_doc();
        let mut doc_args = Doc::nil();
        for (idx, arg) in self.args.iter().enumerate() {
            if idx != 0 {
                doc_args = doc_args.cat_space(arg.to_doc());
            } else {
                doc_args = doc_args.cat(arg.to_doc());
            }
        }

        Doc::lit("(")
            .cat(doc_app)
            .cat_space(doc_args.nest(4))
            .cat_lit(")")
    }
}

impl DocPrinter for TypedVCaseExpr {
    fn to_doc(&self) -> Box<Doc> {
        let header = Doc::lit("case")
            .cat_space(self.arg.to_doc())
            .cat_space_lit("of");

        let mut body = Doc::nil();
        for clause in self.clauses.iter() {
            body = body.cat_line_force().cat(clause.to_doc());
        }

        Doc::line_force()
            .cat(header)
            .cat(body.nest(4))
            .cat_line_force()
    }
}

impl DocPrinter for TypedVCaseClause {
    fn to_doc(&self) -> Box<Doc> {
        let mut doc_pat_and_guard = self.pattern.to_doc();
        if let Some(g) = &self.guard {
            let doc_guard = g.to_doc();
            doc_pat_and_guard = doc_pat_and_guard.cat_space_lit("|").cat_space(doc_guard);
        }
        let doc_clause_lhs = doc_pat_and_guard.cat_space_lit("->");
        doc_clause_lhs.cat_space(self.body.to_doc().nest(4))
    }
}

impl DocPrinter for TypedVLetExpr {
    fn to_doc(&self) -> Box<Doc> {
        let mut doc_defs = Doc::nil();

        for (idx, (lhs, rhs)) in self.defs.iter().enumerate() {
            let mut doc_def = lhs
                .to_doc()
                .cat_space_lit("=")
                .cat_space(rhs.to_doc().nest(4));
            if idx != 0 {
                doc_def = Doc::line_force().cat(doc_def);
            }
            doc_defs = doc_defs.cat(doc_def);
        }
        let doc = Doc::lit("let")
            .cat_space(doc_defs.nest(4))
            .cat(Doc::line_force().cat_lit("in"))
            .cat(Doc::line_force().cat(self.body.to_doc()).nest(4));
        Doc::line_force().cat(doc)
    }
}

impl DocPrinter for TypedVConstructorExpr {
    fn to_doc(&self) -> Box<Doc> {
        let mut doc = Doc::lit(&format!("{}.{}", self.ty_name, self.constructor_name));
        if let Some(x) = &self.record_fields {
            doc = doc.cat_space_lit("{");
            for (field, linear_index) in x.iter() {
                doc = doc
                    .cat_space(Doc::lit(&format!("{}:", field)))
                    .cat_space(self.args[*linear_index].to_doc())
                    .cat_space_lit(",");
            }
            doc = doc.cat_lit("}");
        } else {
            for i in self.args.iter() {
                doc = doc.cat_space(i.to_doc());
            }
        }
        Doc::lit("(")
            .cat(doc)
            .cat_space_lit("::")
            .cat_space(self.ty.to_doc())
            .cat_lit(")")
    }
}

impl DocPrinter for TypedVPattern {
    fn to_doc(&self) -> Box<Doc> {
        use TypedVPattern::*;
        match self {
            Wild { ty: _ } => Doc::lit("_"),
            Variable {
                binder,
                ty,
                ty_schematic,
            } => Doc::lit("(")
                .cat(binder.to_doc().cat_space_lit("::"))
                .cat(ty.to_doc())
                .cat(Doc::lit("/").cat(ty_schematic.to_doc()))
                .cat_lit(")"),
            Literal { literal, ty: _ } => literal.to_doc(),
            Range { start, end, ty: _ } => start.to_doc().cat_lit("..").cat(end.to_doc()),
            Constructor {
                ty_name,
                constructor,
                args,
                ty: _,
                ty_args,
            } => {
                let mut doc = Doc::lit(&format!("{}.{}", ty_name, constructor));

                if !ty_args.is_empty() {
                    doc = doc.cat_lit("<");
                    for i in ty_args {
                        doc = doc.cat(i.to_doc()).cat_lit(",");
                    }
                    doc = doc.cat_lit(">");
                }

                for i in args.iter() {
                    doc = doc.cat_space(i.to_doc());
                }
                doc
            }
            Record {
                ty_name,
                constructor,
                fields,
                rest,
                ty: _,
                ty_args,
            } => {
                let mut doc = Doc::nil();
                if let Some(qualified_type) = ty_name {
                    doc = doc.cat(Doc::lit(&format!("{}.", qualified_type)));
                }
                doc = doc.cat(Doc::lit(&format!("{} {{", constructor)));

                if !ty_args.is_empty() {
                    doc = doc.cat_lit("<");
                    for i in ty_args {
                        doc = doc.cat(i.to_doc()).cat_lit(",");
                    }
                    doc = doc.cat_lit(">");
                }

                let mut doc_fields = Doc::nil();
                for (field, pat) in fields.iter() {
                    doc_fields = doc_fields.cat_line_force().cat(
                        Doc::lit(&format!("{}: ", field))
                            .cat(pat.to_doc())
                            .cat_lit(","),
                    );
                }
                if *rest {
                    doc_fields = doc_fields.cat(Doc::line_force().cat_lit(".."));
                }
                doc.cat(doc_fields.group().nest(4)).cat_lit("}")
            }
        }
    }
}

impl DocPrinter for TypedVLitNumeric {
    fn to_doc(&self) -> Box<Doc> {
        Doc::lit("(")
            .cat(
                self.val
                    .to_doc()
                    .cat_space_lit("::")
                    .cat_space(self.ty.to_doc()),
            )
            .cat_lit(")")
    }
}

impl DocPrinter for TypedVLitString {
    fn to_doc(&self) -> Box<Doc> {
        Doc::lit("(")
            .cat(
                self.val
                    .to_doc()
                    .cat_space_lit("::")
                    .cat_space(self.ty.to_doc()),
            )
            .cat_lit(")")
    }
}

impl DocPrinter for TypedVVariable {
    fn to_doc(&self) -> Box<Doc> {
        let mut doc_ty_args = Doc::nil();
        if !self.ty_args.is_empty() {
            doc_ty_args = Doc::lit("<");
            for i in self.ty_args.iter() {
                doc_ty_args = doc_ty_args.cat(i.to_doc()).cat_lit(",");
            }
            doc_ty_args = doc_ty_args.cat_lit(">");
        }
        Doc::lit("(")
            .cat(
                self.var
                    .to_doc()
                    .cat(doc_ty_args)
                    .cat_space_lit("::")
                    .cat_space(self.ty.to_doc()),
            )
            .cat(Doc::lit("/").cat(self.ty_schematic.to_doc()))
            .cat_lit(")")
    }
}

// <<--- helper impl. for doc printer trait
