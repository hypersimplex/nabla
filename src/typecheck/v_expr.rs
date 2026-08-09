use crate::parse::concrete_token;
use crate::parse::loc;
use crate::typecheck::ty_expr::*;
use crate::typecheck::v_var_name::*;

#[derive(Clone, Debug)]
pub enum VExpr {
    Abstraction(VAbstrExpr),
    Application(VAppExpr),
    Case(VCaseExpr),
    Let(VLetExpr),
    Atom(VAtom),
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

#[derive(Clone, Debug)]
pub(crate) enum VAtom {
    Numeric(VLitNumeric),
    String(VLitString),
    Unit,
    Variable(VVar),
}

/// ADT constructor expression
#[derive(Clone, Debug)]
pub(crate) struct VConstructorExpr {
    // type name (resolved during type checking)
    pub type_name: Option<String>,

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

    // unnamed lambda expression with auto generated id
    Anon(u64),
}

#[derive(Clone, Debug)]
pub(crate) struct VLitNumeric {
    pub token: concrete_token::ConcreteToken,

    // maybe None if compiler creates this internally from some optimization
    pub loc: Option<loc::Location>,

    pub value: NumericLiteralValue,
}

#[derive(Clone, Debug)]
pub(crate) struct VLitString {
    pub token: concrete_token::ConcreteToken,

    // maybe None if compiler creates this internally from some optimization
    pub loc: Option<loc::Location>,
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
        type_name: Option<String>,
        constructor: String,
        args: Vec<VPattern>,
    },
    Record {
        // filled during type checking
        type_name: Option<String>,

        constructor: String,

        // field name -> pattern bindings
        fields: Vec<(String, VPattern)>,

        // true if pattern has ".." wildcard
        rest: bool,
    },
}

#[derive(Clone, Debug)]
pub(crate) enum VPatternLiteral {
    Numeric(VLitNumeric),
    String(VLitString),
    Unit,
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
