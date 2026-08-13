//! value level constructs with required type annotation, produced after type
//! checking/inference

use crate::typecheck::ty_expr::*;
use crate::typecheck::ty_var_name::*;
use crate::typecheck::v_expr::{RangeBound, VAtom, VConstructorExpr, VPatternLiteral, VVar};
use crate::util::printer::*;

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
    // explicit type args at this use site
    //
    // [todo]: for TyApp insertion, order shoudl match the binding's
    // `ty_vars_schematic`
    //
    // e.g.: id @Int 3 where ty_args = [Int]
    pub ty_args: Vec<TyExpr>,
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
        ty_name: String,
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

// helper impl. for doc printer trait --->>

impl DocPrinter for TypedVExpr {
    fn to_doc(&self) -> Box<Doc> {
        use TypedVExpr::*;
        match self {
            Abstraction(x) => x.to_doc(),
            Application(x) => x.to_doc(),
            Case(x) => x.to_doc(),
            Let(x) => x.to_doc(),
            Atom(x) => x.to_doc(),
            Constructor(x) => x.to_doc(),
        }
    }
}

impl DocPrinter for TypedVAbstrExpr {
    fn to_doc(&self) -> Box<Doc> {
        let mut doc_abstr = {
            let mut doc_name = self.name.to_doc();
            doc_name = mk_cat(mk_lit("("), doc_name);
            doc_name = mk_cat(doc_name, mk_cat(mk_lit(" :: "), self.ty.to_doc()));
            doc_name = mk_cat(doc_name, mk_lit(")"));
            cat_space(doc_name, mk_lit("= "))
        };
        let mut rhs = mk_nil();
        rhs = mk_cat(rhs, mk_lit("\\"));
        for (idx, param) in self.params.iter().enumerate() {
            if idx != 0 {
                rhs = cat_space(rhs, param.to_doc());
            } else {
                rhs = mk_cat(rhs, param.to_doc());
            }
        }
        rhs = cat_space(rhs, mk_lit("->"));
        let doc_body = self.body.to_doc();

        rhs = cat_space(rhs, mk_nest(4, doc_body));
        mk_cat(doc_abstr, mk_group(rhs))
    }
}

impl DocPrinter for TypedVAbstrParam {
    fn to_doc(&self) -> Box<Doc> {
        mk_cat(
            mk_cat(
                mk_lit("("),
                cat_space(
                    cat_space(self.binder.to_doc(), mk_lit("::")),
                    self.ty.to_doc(),
                ),
            ),
            mk_lit(")"),
        )
    }
}

impl DocPrinter for TypedVAppExpr {
    fn to_doc(&self) -> Box<Doc> {
        let mut doc_app = self.callable.to_doc();
        let mut doc_args = mk_nil();
        for (idx, arg) in self.args.iter().enumerate() {
            if idx != 0 {
                doc_args = cat_space(doc_args, arg.to_doc());
            } else {
                doc_args = mk_cat(doc_args, arg.to_doc());
            }
        }

        mk_cat(
            cat_space(mk_cat(mk_lit("("), doc_app), mk_nest(4, doc_args)),
            mk_lit(")"),
        )
    }
}

impl DocPrinter for TypedVCaseExpr {
    fn to_doc(&self) -> Box<Doc> {
        let mut header = cat_space(mk_lit("case"), self.arg.to_doc());
        header = cat_space(header, mk_lit("of"));

        let mut body = mk_nil();
        for clause in self.clauses.iter() {
            body = mk_cat(body, mk_line_force());
            body = mk_cat(body, clause.to_doc());
        }

        let ret = mk_cat(mk_line_force(), mk_cat(header, mk_nest(4, body)));
        mk_cat(ret, mk_line_force())
    }
}

impl DocPrinter for TypedVCaseClause {
    fn to_doc(&self) -> Box<Doc> {
        let mut doc_pat_and_guard = self.pattern.to_doc();
        if let Some(g) = &self.guard {
            let doc_guard = g.to_doc();
            doc_pat_and_guard = cat_space(doc_pat_and_guard, mk_lit("|"));
            doc_pat_and_guard = cat_space(doc_pat_and_guard, doc_guard);
        }
        let doc_clause_lhs = cat_space(doc_pat_and_guard, mk_lit("->"));
        cat_space(doc_clause_lhs, mk_nest(4, self.body.to_doc()))
    }
}

impl DocPrinter for TypedVLetExpr {
    fn to_doc(&self) -> Box<Doc> {
        let mut doc_defs = mk_nil();

        for (idx, (lhs, rhs)) in self.defs.iter().enumerate() {
            let mut doc_def = cat_space(
                cat_space(lhs.to_doc(), mk_lit("=")),
                mk_nest(4, rhs.to_doc()),
            );
            if idx != 0 {
                doc_def = mk_cat(mk_line_force(), doc_def);
            }
            doc_defs = mk_cat(doc_defs, doc_def);
        }
        let mut doc = cat_space(mk_lit("let"), mk_nest(4, doc_defs));
        doc = mk_cat(doc, mk_cat(mk_line_force(), mk_lit("in")));
        doc = mk_cat(doc, mk_nest(4, mk_cat(mk_line_force(), self.body.to_doc())));
        mk_cat(mk_line_force(), doc)
    }
}

impl DocPrinter for TypedVAtom {
    fn to_doc(&self) -> Box<Doc> {
        mk_cat(
            mk_cat(
                mk_lit("("),
                cat_space(
                    cat_space(self.atom.to_doc(), mk_lit("::")),
                    self.ty.to_doc(),
                ),
            ),
            mk_lit(")"),
        )
    }
}

impl DocPrinter for TypedVConstructorExpr {
    fn to_doc(&self) -> Box<Doc> {
        // println!("printing for TypedVConstructorExpr: {:?}", self);
        let mut doc = mk_nil();
        doc = mk_cat(
            doc,
            mk_lit(&format!("{}.{}", self.ty_name, self.constructor_name)),
        );
        if let Some(x) = &self.record_fields {
            doc = cat_space(doc, mk_lit("{"));
            for (field, linear_index) in x.iter() {
                doc = cat_space(doc, mk_lit(&format!("{}:", field)));
                doc = cat_space(doc, self.args[*linear_index].to_doc());
                doc = cat_space(doc, mk_lit(","));
            }
            doc = mk_cat(doc, mk_lit("}"));
        } else {
            for i in self.args.iter() {
                doc = cat_space(doc, i.to_doc());
            }
        }
        doc = mk_cat(mk_lit("("), doc);
        doc = cat_space(doc, mk_lit("::"));
        doc = cat_space(doc, self.ty.to_doc());
        doc = mk_cat(doc, mk_lit(")"));
        doc
    }
}

impl DocPrinter for TypedVPattern {
    fn to_doc(&self) -> Box<Doc> {
        use TypedVPattern::*;
        match self {
            Wild { ty } => mk_lit("_"),
            Variable {
                binder,
                ty,
                ty_vars_schematic,
            } => mk_cat(
                mk_cat(
                    mk_cat(mk_lit("("), cat_space(binder.to_doc(), mk_lit("::"))),
                    ty.to_doc(),
                ),
                mk_lit(")"),
            ),
            Literal { literal, ty } => literal.to_doc(),
            Range { start, end, ty } => mk_cat(mk_cat(start.to_doc(), mk_lit("..")), end.to_doc()),
            Constructor {
                ty_name,
                constructor,
                args,
                ty,
            } => {
                let mut doc = mk_nil();
                doc = mk_cat(doc, mk_lit(&format!("{}.", ty_name)));
                doc = mk_cat(doc, mk_lit(&format!("{}", constructor)));
                for i in args.iter() {
                    doc = cat_space(doc, i.to_doc());
                }
                doc
            }
            Record {
                ty_name,
                constructor,
                fields,
                rest,
                ty,
            } => {
                let mut doc = mk_nil();
                if let Some(qualified_type) = ty_name {
                    doc = mk_cat(doc, mk_lit(&format!("{}.", qualified_type)));
                }
                doc = mk_cat(doc, mk_lit(&format!("{} {{", constructor)));

                let mut doc_fields = mk_nil();
                for (field, pat) in fields.iter() {
                    doc_fields = mk_cat(
                        doc_fields,
                        mk_cat(
                            mk_line_force(),
                            mk_cat(
                                mk_cat(mk_lit(&format!("{}: ", field)), pat.to_doc()),
                                mk_lit(","),
                            ),
                        ),
                    );
                }
                if *rest {
                    doc_fields = mk_cat(doc_fields, mk_cat(mk_line_force(), mk_lit("..")));
                }
                doc = mk_cat(doc, mk_nest(4, mk_group(doc_fields)));
                doc = mk_cat(doc, mk_lit(&format!("}}")));
                doc
            }
        }
    }
}

// <<--- helper impl. for doc printer trait
