use crate::parse::concrete_token;
use crate::parse::loc;
use crate::typecheck::ty_expr::*;
use crate::typecheck::v_var_name::*;
use crate::util::printer::*;

use std::collections::*;

#[derive(Clone, Debug)]
pub enum VExpr {
    Abstraction(VAbstrExpr),
    Application(VAppExpr),
    Case(VCaseExpr),
    Let(VLetExpr),
    LitNumeric(VLitNumeric),
    LitString(VLitString),
    Variable(VVar),
    Constructor(VConstructorExpr),
}

#[derive(Clone, Debug)]
pub(crate) struct VAbstrExpr {
    // name of abstraction or anonymous
    pub name: VVar,

    // parameter metadata with original pattern
    pub params: Vec<VAbstrParam>,

    // body expr and optional type annotation
    pub body: Box<(VExpr, Option<TyExpr>)>,
}

#[derive(Clone, Debug)]
pub(crate) struct VAbstrParam {
    // if `pattern` is a simple named variable, then `binder` is the same
    // else `binder` get assigned with an auto-generated variable name
    pub binder: VVar,
    pub pattern: VPattern,
    pub annotation: Option<TyExpr>,
}

#[derive(Clone, Debug)]
pub(crate) struct VAppExpr {
    pub callable: Box<(VExpr, Option<TyExpr>)>,
    pub args: Vec<(VExpr, Option<TyExpr>)>,
}

/// note: handles recursive let groups with pattern LHS bindings
#[derive(Clone, Debug)]
pub(crate) struct VLetExpr {
    // [(pattern, rhs, type annotation)]
    pub defs: Vec<(VPattern, VExpr, Option<TyExpr>)>,

    pub body: Box<(VExpr, Option<TyExpr>)>,
}

#[derive(Clone, Debug)]
pub(crate) struct VCaseExpr {
    pub keyword: loc::ConcreteTokenAndLoc,

    pub arg: Box<(VExpr, Option<TyExpr>)>,

    pub clauses: Vec<VCaseClause>,
}

#[derive(Clone, Debug)]
pub(crate) struct VCaseClause {
    pub pattern: VPattern,
    pub guard: Option<VExprAndTyAnnot>,
    pub body: Box<VExprAndTyAnnot>,
}

/// ADT constructor expression
#[derive(Clone, Debug)]
pub(crate) struct VConstructorExpr {
    // type name (resolved during type checking)
    pub ty_name: Option<String>,

    // constructor name
    pub constructor: String,

    // positional constructor arguments
    pub args: Vec<(VExpr, Option<TyExpr>)>,

    // record fields for record constructors
    pub record_fields: Option<Vec<(String, (VExpr, Option<TyExpr>))>>,
}

/// value level variable
#[derive(Clone, Debug, PartialOrd, Ord, PartialEq, Eq)]
pub(crate) enum VVar {
    Named(VVarName),

    // auto-generated, anonymous variable
    Anon(u64),

    // original variable + uniqued id
    Renamed(VVarNameUniqued),
}

#[derive(Clone, Debug, PartialOrd, Ord, PartialEq, Eq)]
pub(crate) struct VVarNameUniqued {
    pub original: VVarName,
    pub unique: u64,
}

#[derive(Clone, Debug)]
pub(crate) struct VLitNumeric {
    pub token: concrete_token::ConcreteToken,

    // maybe None if compiler creates this internally from some optimization
    pub loc: Option<loc::Location>,

    pub value: NumericLiteralValue,
}

impl VLitNumeric {
    pub(crate) fn ty(&self) -> TyExpr {
        match self.value {
            NumericLiteralValue::Int { .. } => mk_ty_i64(),
            NumericLiteralValue::Float { .. } => mk_ty_f64(),
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct VLitString {
    pub token: concrete_token::ConcreteToken,

    // maybe None if compiler creates this internally from some optimization
    pub loc: Option<loc::Location>,
}

impl VLitString {
    pub(crate) fn ty(&self) -> TyExpr {
        mk_ty_string()
    }
}

#[derive(Clone, Debug)]
pub(crate) enum NumericLiteralValue {
    Int { raw: String, parsed: Option<i64> },
    Float { raw: String, parsed: Option<f64> },
}

#[derive(Clone, Debug)]
pub(crate) enum RangeBound<T> {
    Inclusive(T),
    Exclusive(T),
}

/// pattern expressions used across case clauses, lambda parameters, and let bindings
#[derive(Clone, Debug)]
pub(crate) enum VPattern {
    // _ wildcard
    Wild,

    // variable bindings
    Variable(VVar),

    // literal patterns
    Literal(VPatternLiteral),

    Range {
        start: RangeBound<VPatternLiteral>,
        end: RangeBound<VPatternLiteral>,
    },

    Constructor {
        // filled during type checking
        ty_name: Option<String>,
        constructor: String,
        args: Vec<VPattern>,
    },
    Record {
        // filled during type checking
        ty_name: Option<String>,

        constructor: String,

        // field name -> pattern bindings
        fields: Vec<(String, VPattern)>,

        // true if pattern has ".." wildcard
        rest: bool,
    },
}

impl VPattern {
    pub(crate) fn get_bound_vars(&self, out: &mut BTreeSet<VVar>) {
        // collect all binders introduced by a pattern, including nested fields/args
        match self {
            VPattern::Wild => {}
            VPattern::Variable(v) => {
                out.insert(v.clone());
            }
            VPattern::Literal(_) => {}
            VPattern::Range { .. } => {}
            VPattern::Constructor { args, .. } => {
                for arg in args {
                    arg.get_bound_vars(out);
                }
            }
            VPattern::Record { fields, .. } => {
                for (_, field_pat) in fields {
                    field_pat.get_bound_vars(out);
                }
            }
        }
    }
    pub(crate) fn subst_vars(&mut self, substitution: &BTreeMap<VVar, VVar>) {
        match self {
            VPattern::Wild => {}
            VPattern::Variable(v) => if let Some(x) = substitution.get(v) { *v = x.clone() },
            VPattern::Literal(_) => {}
            VPattern::Range { .. } => {}
            VPattern::Constructor { args, .. } => {
                for arg in args.iter_mut() {
                    arg.subst_vars(substitution);
                }
            }
            VPattern::Record { fields, .. } => {
                for (_, field_pat) in fields.iter_mut() {
                    field_pat.subst_vars(substitution);
                }
            }
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) enum VPatternLiteral {
    Numeric(VLitNumeric),
    String(VLitString),
}

impl VPatternLiteral {
    pub(crate) fn ty(&self) -> TyExpr {
        use VPatternLiteral::*;
        match self {
            Numeric(x) => x.ty(),
            String(x) => x.ty(),
        }
    }
}

pub(crate) type VExprAndTyAnnot = (VExpr, Option<TyExpr>);

pub(crate) fn classify_numeric_literal(
    token: &concrete_token::ConcreteToken,
) -> NumericLiteralValue {
    let concrete_token::ConcreteToken::LiteralNumeric(raw) = token else {
        panic!(
            "classify_numeric_literal called on non-numeric token: {:?}",
            token
        );
    };
    // for simplicity, assume that parser checked validity of these
    if raw.contains('.') || raw.contains('e') || raw.contains('E') {
        NumericLiteralValue::Float {
            raw: raw.clone(),
            parsed: raw.parse::<f64>().ok(), // parse now to avoid repeated parsing downstream
        }
    } else {
        NumericLiteralValue::Int {
            raw: raw.clone(),
            parsed: raw.parse::<i64>().ok(),
        }
    }
}

// utility functions for VExpr ---

impl VExpr {
    pub(crate) fn get_free_vars(&self, bound: &BTreeSet<VVar>) -> BTreeSet<VVar> {
        match self {
            VExpr::Abstraction(abstr_expr) => abstr_expr.get_free_vars(bound),
            VExpr::Application(app_expr) => app_expr.get_free_vars(bound),
            VExpr::Case(case_expr) => case_expr.get_free_vars(bound),
            VExpr::Let(let_expr) => let_expr.get_free_vars(bound),
            VExpr::LitNumeric(x) => x.get_free_vars(bound),
            VExpr::LitString(x) => x.get_free_vars(bound),
            VExpr::Variable(x) => x.get_free_vars(bound),
            VExpr::Constructor(constructor_expr) => constructor_expr.get_free_vars(bound),
        }
    }
}

impl VAbstrExpr {
    pub(crate) fn get_free_vars(&self, bound: &BTreeSet<VVar>) -> BTreeSet<VVar> {
        // abstraction parameters bind within the abstraction body
        let mut bound_inner = bound.clone();
        for param in &self.params {
            bound_inner.insert(param.binder.clone());
            param.pattern.get_bound_vars(&mut bound_inner);
        }
        self.body.0.get_free_vars(&bound_inner)
    }
}

impl VAppExpr {
    pub(crate) fn get_free_vars(&self, bound: &BTreeSet<VVar>) -> BTreeSet<VVar> {
        let mut out = self.callable.0.get_free_vars(bound);
        for (arg, _) in &self.args {
            out.extend(arg.get_free_vars(bound));
        }
        out
    }
}

impl VCaseExpr {
    pub(crate) fn get_free_vars(&self, bound: &BTreeSet<VVar>) -> BTreeSet<VVar> {
        let mut out = self.arg.0.get_free_vars(bound);
        for clause in &self.clauses {
            out.extend(clause.get_free_vars(bound));
        }
        out
    }
}

impl VCaseClause {
    pub(crate) fn get_free_vars(&self, bound: &BTreeSet<VVar>) -> BTreeSet<VVar> {
        let mut out = BTreeSet::new();
        // case clause pattern binders scope over its guard and body only
        let mut bound_inner = bound.clone();
        self.pattern.get_bound_vars(&mut bound_inner);
        if let Some((guard, _)) = &self.guard {
            out.extend(guard.get_free_vars(&bound_inner));
        }
        out.extend(self.body.0.get_free_vars(&bound_inner));
        out
    }
}

impl VLetExpr {
    pub(crate) fn get_free_vars(&self, bound: &BTreeSet<VVar>) -> BTreeSet<VVar> {
        let mut out = BTreeSet::new();
        // let groups are recursive here: binders scope over all rhs defs and body
        let mut bound_inner = bound.clone();
        for (pat, _, _) in &self.defs {
            pat.get_bound_vars(&mut bound_inner);
        }
        for (_, rhs, _) in &self.defs {
            out.extend(rhs.get_free_vars(&bound_inner));
        }
        out.extend(self.body.0.get_free_vars(&bound_inner));
        out
    }
}

impl VLitNumeric {
    pub(crate) fn get_free_vars(&self, _bound: &BTreeSet<VVar>) -> BTreeSet<VVar> {
        BTreeSet::new()
    }
}

impl VLitString {
    pub(crate) fn get_free_vars(&self, _bound: &BTreeSet<VVar>) -> BTreeSet<VVar> {
        BTreeSet::new()
    }
}

impl VVar {
    pub(crate) fn get_free_vars(&self, bound: &BTreeSet<VVar>) -> BTreeSet<VVar> {
        let mut out = BTreeSet::new();
        // a variable atom is free iff it is not in scope
        if !bound.contains(self) {
            out.insert(self.clone());
        }
        out
    }
}

impl VConstructorExpr {
    pub(crate) fn get_free_vars(&self, bound: &BTreeSet<VVar>) -> BTreeSet<VVar> {
        let mut out = BTreeSet::new();
        for (arg, _) in &self.args {
            out.extend(arg.get_free_vars(bound));
        }
        if let Some(fields) = &self.record_fields {
            for (_, (expr, _)) in fields {
                out.extend(expr.get_free_vars(bound));
            }
        }
        out
    }
}

// helper impl. for doc printer trait --->>

impl DocPrinter for VLitNumeric {
    fn to_doc(&self) -> Box<Doc> {
        self.value.to_doc()
    }
}

impl DocPrinter for NumericLiteralValue {
    fn to_doc(&self) -> Box<Doc> {
        use NumericLiteralValue::*;
        match self {
            Int { parsed, .. } => mk_lit(&format!("{}", parsed.as_ref().unwrap())),
            Float { parsed, .. } => mk_lit(&format!("{}", parsed.as_ref().unwrap())),
        }
    }
}

impl DocPrinter for VLitString {
    fn to_doc(&self) -> Box<Doc> {
        mk_cat(mk_cat(mk_lit("\""), self.token.to_doc()), mk_lit("\""))
    }
}

impl DocPrinter for VVar {
    fn to_doc(&self) -> Box<Doc> {
        use VVar::*;
        match self {
            Named(vvar_name) => vvar_name.to_doc(),
            Anon(anon_id) => mk_lit(&format!("VAuto({})", anon_id)),
            Renamed(vvar_name_uniqued) => vvar_name_uniqued.to_doc(),
        }
    }
}

impl DocPrinter for VPatternLiteral {
    fn to_doc(&self) -> Box<Doc> {
        use VPatternLiteral::*;
        match self {
            Numeric(vlit_num) => vlit_num.to_doc(),
            String(vlit_string) => vlit_string.to_doc(),
        }
    }
}

impl<T> DocPrinter for RangeBound<T>
where
    T: DocPrinter,
{
    fn to_doc(&self) -> Box<Doc> {
        use RangeBound::*;
        match self {
            Inclusive(x) => mk_cat(mk_cat(mk_lit("Inclusive("), x.to_doc()), mk_lit(")")),
            Exclusive(x) => mk_cat(mk_cat(mk_lit("Exclusive("), x.to_doc()), mk_lit(")")),
        }
    }
}

impl DocPrinter for VVarNameUniqued {
    fn to_doc(&self) -> Box<Doc> {
        mk_cat(
            mk_cat(self.original.token.to_doc(), mk_lit("_")),
            mk_lit(&format!("{}", self.unique)),
        )
    }
}

// <<--- helper impl. for doc printer trait
