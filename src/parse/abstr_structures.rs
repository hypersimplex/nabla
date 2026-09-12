use std::sync::*;

// top level structures:
// data
// function signature
// function definition

use super::loc::*;

use super::concrete_token::*;

use crate::util::printer::*;

// eg: T A B, where T is the identifier and A, B are type parameters
#[derive(Clone, Debug)]
pub(crate) struct ATypeExprIden {
    pub identifier: ConcreteTokenAndLoc,
    pub type_parameters: Vec<ATypeExprComplex>,
}

impl DocPrinter for ATypeExprIden {
    fn to_doc(&self) -> Box<Doc> {
        let mut doc = self.identifier.to_doc();
        if self.type_parameters.is_empty() {
            return doc;
        }
        doc = Doc::lit("(").cat_space(doc);
        for i in self.type_parameters.iter() {
            doc = doc.cat_space(i.to_doc());
        }
        doc.cat_space_lit(")")
    }
}

// eg: T1 A -> T2 -> T3
// implemented as a linked list, where there is a link for each ->
#[derive(Clone, Debug)]
pub(crate) struct ATypeExprFun {
    pub head: Arc<Mutex<ATypeExprComplex>>,
    pub tail: Option<Arc<Mutex<ATypeExprComplex>>>,
}

impl DocPrinter for ATypeExprFun {
    fn to_doc(&self) -> Box<Doc> {
        let doc_head = {
            let guard_head = self.head.lock().unwrap();
            let content_head = &*guard_head;
            content_head.to_doc()
        };

        if let Some(x) = &self.tail {
            let guard = x.lock().unwrap();
            let content_tail = &*guard;
            let doc_tail = content_tail.to_doc();
            return Doc::lit("(")
                .cat_space(doc_head.cat_space_lit("->").cat_space(doc_tail))
                .cat_space_lit(")");
        }
        doc_head
    }
}

// either identifier type expr or function-like type expr
#[derive(Clone, Debug)]
pub(crate) enum ATypeExprComplex {
    Iden(ATypeExprIden),
    Fun(ATypeExprFun),
}

impl DocPrinter for ATypeExprComplex {
    fn to_doc(&self) -> Box<Doc> {
        use ATypeExprComplex::*;
        match self {
            Iden(x) => x.to_doc(),
            Fun(x) => x.to_doc(),
        }
    }
}

// function parameter type signature
// eg: add_and_square :: T -> T2 T3 -> T
// identifier: add_and_square
// ty: ATypeExprComplex::Fun(..)
#[derive(Clone, Debug)]
pub(crate) struct FnSig {
    pub identifier: ConcreteTokenAndLoc,
    pub ty: ATypeExprComplex,
}

impl DocPrinter for FnSig {
    fn to_doc(&self) -> Box<Doc> {
        self.identifier
            .to_doc()
            .cat_space(Doc::lit("::").cat_space(self.ty.to_doc()))
    }
}

// might be useful when adding support for imperative constructs / do notation
#[derive(Clone, Debug)]
pub(crate) struct BlockExpr(pub Vec<AExprAnnot>);

impl DocPrinter for BlockExpr {
    fn to_doc(&self) -> Box<Doc> {
        let mut doc = Doc::nil();
        let mut is_first = true;
        for i in self.0.iter() {
            if !is_first {
                doc = doc.cat_line();
            }
            is_first = false;
            doc = doc.cat(i.to_doc());
        }
        doc
    }
}

// record generic/schematic type
#[derive(Clone, Debug)]
pub(crate) struct DataRecord {
    pub identifier: ConcreteTokenAndLoc,
    pub params: Vec<ATypeExprComplex>,
    pub components: Vec<(ConcreteTokenAndLoc, ATypeExprComplex)>, //[(field name, type_expr)]
}

impl DocPrinter for DataRecord {
    fn to_doc(&self) -> Box<Doc> {
        let mut doc = Doc::lit("data").cat_space(self.identifier.to_doc());
        for i in self.params.iter() {
            doc = doc.cat_space(i.to_doc());
        }
        doc = doc.cat_space_lit("{");

        let mut doc_fields = Doc::nil();
        for (field_name, type_expr) in self.components.iter() {
            doc_fields = doc_fields
                .cat_line_force()
                .cat(field_name.to_doc())
                .cat_space_lit("::")
                .cat_space(type_expr.to_doc())
                .cat_lit(", ");
        }
        doc.cat(doc_fields.nest(4))
            .cat(Doc::line_force().cat_lit("}"))
    }
}

//sum of product generic/schematic type
#[derive(Clone, Debug)]
pub(crate) struct DataSum {
    pub identifier: ConcreteTokenAndLoc,
    pub params: Vec<ATypeExprComplex>,
    pub variants: Vec<(ConcreteTokenAndLoc, Vec<ATypeExprComplex>)>, //[(constructor name, [type_expr])]
}

impl DocPrinter for DataSum {
    fn to_doc(&self) -> Box<Doc> {
        let mut doc = Doc::lit("data").cat_space(self.identifier.to_doc());
        for i in self.params.iter() {
            doc = doc.cat_space(i.to_doc());
        }

        let mut doc_variants = Doc::nil();
        for (idx, (constructor_name, type_exprs)) in self.variants.iter().enumerate() {
            let mut doc_variant = if idx == 0 {
                Doc::lit("=")
            } else {
                Doc::lit("|")
            };
            doc_variant = doc_variant.cat_space(constructor_name.to_doc());
            for i in type_exprs.iter() {
                doc_variant = doc_variant.cat_space(i.to_doc());
            }
            doc_variant = Doc::line_force().cat(doc_variant);
            doc_variants = doc_variants.cat(doc_variant);
        }
        doc.cat(doc_variants.nest(4))
    }
}

// expressions start ---

// note: identifier / literal / expression inside parentheses is primary expression / atom

// non-atomic/primary expression types
#[derive(Clone, Copy, Debug)]
pub(crate) enum BuiltinExprType {
    //prefix:
    UnaryPlus,
    UnaryNegate,
    UnaryLogicalNot,
    //infix:
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
}

#[derive(Clone, Debug)]
pub(crate) struct PrefixOpInfo {
    pub expr_type: BuiltinExprType,
    pub rbp: usize,
    pub builtin_token: ConcreteToken,
}

#[derive(Clone, Debug)]
pub(crate) struct InfixOpInfo {
    pub expr_type: BuiltinExprType,
    pub lbp: usize,
    pub rbp: usize,
    pub builtin_token: ConcreteToken,
}

pub(crate) fn prefix_op_info(token: &ConcreteToken) -> Option<PrefixOpInfo> {
    match token {
        ConcreteToken::Exclamation => Some(PrefixOpInfo {
            expr_type: BuiltinExprType::UnaryLogicalNot,
            rbp: 54,
            builtin_token: ConcreteToken::UnaryNot,
        }),
        ConcreteToken::Minus => Some(PrefixOpInfo {
            expr_type: BuiltinExprType::UnaryNegate,
            rbp: 56,
            builtin_token: ConcreteToken::UnaryMinus,
        }),
        ConcreteToken::Plus => Some(PrefixOpInfo {
            expr_type: BuiltinExprType::UnaryPlus,
            rbp: 58,
            builtin_token: ConcreteToken::UnaryPlus,
        }),
        _ => None,
    }
}

pub(crate) fn infix_op_info(token: &ConcreteToken) -> Option<InfixOpInfo> {
    match token {
        ConcreteToken::Star => Some(InfixOpInfo {
            expr_type: BuiltinExprType::BinaryMul,
            lbp: 50,
            rbp: 51,
            builtin_token: ConcreteToken::BinaryMul,
        }),
        ConcreteToken::FwdSlash => Some(InfixOpInfo {
            expr_type: BuiltinExprType::BinaryDiv,
            lbp: 50,
            rbp: 51,
            builtin_token: ConcreteToken::BinaryDiv,
        }),
        ConcreteToken::Plus => Some(InfixOpInfo {
            expr_type: BuiltinExprType::BinaryAdd,
            lbp: 46,
            rbp: 47,
            builtin_token: ConcreteToken::BinaryPlus,
        }),
        ConcreteToken::Minus => Some(InfixOpInfo {
            expr_type: BuiltinExprType::BinarySub,
            lbp: 46,
            rbp: 47,
            builtin_token: ConcreteToken::BinaryMinus,
        }),
        ConcreteToken::AngleL => Some(InfixOpInfo {
            expr_type: BuiltinExprType::BinaryLess,
            lbp: 44,
            rbp: 45,
            builtin_token: ConcreteToken::AngleL,
        }),
        ConcreteToken::LessEqual => Some(InfixOpInfo {
            expr_type: BuiltinExprType::BinaryLessEqual,
            lbp: 44,
            rbp: 45,
            builtin_token: ConcreteToken::LessEqual,
        }),
        ConcreteToken::AngleR => Some(InfixOpInfo {
            expr_type: BuiltinExprType::BinaryGreater,
            lbp: 44,
            rbp: 45,
            builtin_token: ConcreteToken::AngleR,
        }),
        ConcreteToken::GreaterEqual => Some(InfixOpInfo {
            expr_type: BuiltinExprType::BinaryGreaterEqual,
            lbp: 44,
            rbp: 45,
            builtin_token: ConcreteToken::GreaterEqual,
        }),
        ConcreteToken::EqualEqual => Some(InfixOpInfo {
            expr_type: BuiltinExprType::BinaryEqual,
            lbp: 44,
            rbp: 45,
            builtin_token: ConcreteToken::EqualEqual,
        }),
        ConcreteToken::And => Some(InfixOpInfo {
            expr_type: BuiltinExprType::LogicalAnd,
            lbp: 40,
            rbp: 41,
            builtin_token: ConcreteToken::BinaryAnd,
        }),
        ConcreteToken::Or => Some(InfixOpInfo {
            expr_type: BuiltinExprType::LogicalOr,
            lbp: 38,
            rbp: 39,
            builtin_token: ConcreteToken::BinaryOr,
        }),
        _ => None,
    }
}

// returns left binding power
pub(crate) const APPLICATION_BINDING_POWER: usize = 100;

#[derive(Clone, Debug)]
pub(crate) struct LiteralNumericExpr {
    pub literal: ConcreteTokenAndLoc,
}

impl DocPrinter for LiteralNumericExpr {
    fn to_doc(&self) -> Box<Doc> {
        self.literal.to_doc()
    }
}

#[derive(Clone, Debug)]
pub(crate) struct LiteralStringExpr {
    pub literal: ConcreteTokenAndLoc,
}

impl DocPrinter for LiteralStringExpr {
    fn to_doc(&self) -> Box<Doc> {
        Doc::lit(&format!("\"{}\"", self.literal.token))
    }
}

#[derive(Clone, Debug)]
pub(crate) struct IdenExpr {
    pub iden: ConcreteTokenAndLoc,
    pub builtin: Option<BuiltinExprType>,
}

impl DocPrinter for IdenExpr {
    fn to_doc(&self) -> Box<Doc> {
        self.iden.to_doc()
    }
}

// this provides scope for definitions to be visible to the body of letexpr
//
// eg:
//  let a :: u32 = 6
//      b :: u32 = 7
//  in
//    a + b
//
// defs:
//   a = ..
//   b = ..
// expr: a + b
#[derive(Clone, Debug)]
pub(crate) struct LetExpr {
    pub defs: Vec<(PatternExpr, AExprAnnot)>, // [(pattern, rhs)]
    pub expr: Box<AExprAnnot>,                // body of letexpr
}

impl DocPrinter for LetExpr {
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
            .cat(Doc::line_force().cat(self.expr.to_doc()).nest(4));
        Doc::line_force().cat(doc)
    }
}

// eg:
// let a :: u32 = 6
// patterns: [a]
// type: u32
// expr: 6
//
// \x y z -> x + y + z
// patterns: [x, y, z]
// type: None
// expr: x + y + z
#[derive(Clone, Debug)]
pub(crate) struct AbstractionExpr {
    pub pattern: Vec<ConcreteTokenAndLoc>, // parameter tokens (verbatim); TODO: retire this?
    pub param_patterns: Vec<PatternExpr>,  // original parameter patterns
    pub expr: Box<AExprAnnot>,             // body of lambda
    pub type_expr: Option<ATypeExprComplex>, // optional type annotation
}

impl DocPrinter for AbstractionExpr {
    fn to_doc(&self) -> Box<Doc> {
        let mut doc_abstr = Doc::lit("\\");
        for (idx, pat_expr) in self.param_patterns.iter().enumerate() {
            if idx != 0 {
                doc_abstr = doc_abstr.cat_space(pat_expr.to_doc());
            } else {
                doc_abstr = doc_abstr.cat(pat_expr.to_doc());
            }
        }
        doc_abstr = doc_abstr.cat_space_lit("->");
        let doc_body = self.expr.to_doc();

        doc_abstr = doc_abstr.cat_space(doc_body.nest(4));
        Doc::lit("(").cat(doc_abstr).cat_lit(")")
    }
}

#[derive(Clone, Debug)]
pub(crate) struct TopLevelFunction {
    // top level function is always named
    pub name: ConcreteTokenAndLoc,

    pub abstraction: AbstractionExpr,
}

impl DocPrinter for TopLevelFunction {
    fn to_doc(&self) -> Box<Doc> {
        let mut doc_name = self.name.to_doc();
        if let Some(ty_expr) = &self.abstraction.type_expr {
            doc_name = doc_name.cat(Doc::lit(" :: ").cat(ty_expr.to_doc()));
        }
        doc_name
            .cat_space_lit("=")
            .cat_space(self.abstraction.to_doc())
    }
}

// pattern expressions for pattern matching
#[derive(Clone, Debug)]
pub(crate) enum PatternExpr {
    Wild,                          // _ (wildcard pattern)
    Variable(ConcreteTokenAndLoc), // x (variable binding)
    Literal(AExprAnnot),           // 42, "hello" (literal value)
    Range {
        start: PatternRangeBound,
        end: PatternRangeBound,
    },
    Constructor {
        qualified: Option<ConcreteTokenAndLoc>, // type name if qualified, else empty
        constructor: ConcreteTokenAndLoc,       // constructor name
        args: PatternConstructorArgs,           // positional args or record fields
    },
}

impl DocPrinter for PatternExpr {
    fn to_doc(&self) -> Box<Doc> {
        use PatternExpr::*;
        match self {
            Wild => Doc::lit("_"),
            Variable(x) => x.to_doc(),
            Literal(x) => x.to_doc(), // 42, "hello" (literal value)
            Range { start, end } => {
                match (start, end) {
                    // use shorthand syntax for inclusive start: start..end / start..=end
                    (PatternRangeBound::Inclusive(_), PatternRangeBound::Inclusive(_)) => start
                        .to_doc()
                        .cat_space_lit("..")
                        .cat_space(Doc::lit("=").cat(end.to_doc())),
                    (PatternRangeBound::Inclusive(_), PatternRangeBound::Exclusive(_)) => {
                        start.to_doc().cat_space_lit("..").cat_space(end.to_doc())
                    }
                    // TODO: support explicit syntax during parsing
                    (PatternRangeBound::Exclusive(_), PatternRangeBound::Inclusive(_)) => {
                        let doc_start = Doc::lit("Excluded(").cat(start.to_doc()).cat_lit(")");
                        let doc_end = Doc::lit("Included(").cat(end.to_doc()).cat_lit(")");
                        Doc::lit("(")
                            .cat(doc_start.cat_lit(", ").cat(doc_end))
                            .cat_lit(")")
                    }
                    (PatternRangeBound::Exclusive(_), PatternRangeBound::Exclusive(_)) => {
                        let doc_start = Doc::lit("Excluded(").cat(start.to_doc()).cat_lit(")");
                        let doc_end = Doc::lit("Excluded(").cat(end.to_doc()).cat_lit(")");
                        Doc::lit("(")
                            .cat(doc_start.cat_lit(", ").cat(doc_end))
                            .cat_lit(")")
                    }
                }
            }
            Constructor {
                qualified,
                constructor,
                args,
            } => {
                let mut doc = Doc::nil();
                if let Some(x) = &qualified {
                    doc = doc.cat(x.to_doc()).cat_lit(".");
                }
                doc.cat(constructor.to_doc()).cat(args.to_doc())
            }
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) enum PatternRangeBound {
    Inclusive(AExprAnnot),
    Exclusive(AExprAnnot),
}

impl DocPrinter for PatternRangeBound {
    fn to_doc(&self) -> Box<Doc> {
        use PatternRangeBound::*;
        match self {
            Inclusive(x) | Exclusive(x) => x.to_doc(),
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) enum PatternConstructorArgs {
    Positional(Vec<PatternExpr>),
    Record {
        fields: Vec<(ConcreteTokenAndLoc, PatternExpr)>,
        rest: bool,
    },
}

impl DocPrinter for PatternConstructorArgs {
    fn to_doc(&self) -> Box<Doc> {
        use PatternConstructorArgs::*;
        match self {
            Positional(x) => {
                if x.is_empty() {
                    Doc::nil()
                } else {
                    let mut doc = Doc::lit("(");
                    for (idx, pat_expr) in x.iter().enumerate() {
                        if idx > 0 {
                            doc = doc.cat_lit(" ");
                        }
                        doc = doc.cat(pat_expr.to_doc());
                    }
                    doc.cat_lit(")")
                }
            }
            Record { fields, rest } => {
                let mut doc = Doc::lit("{").cat_line();
                for (field, pat) in fields.iter() {
                    let entry = field
                        .to_doc()
                        .cat_lit(":")
                        .cat_space(pat.to_doc())
                        .cat_lit(",");
                    doc = doc.cat_line().cat(entry);
                }
                if *rest {
                    doc = doc.cat_lit(" ..");
                }
                doc.cat_line().cat_lit("}")
            }
        }
    }
}

// syntactically corresponds to a case clause:
//   pattern (| guard)? -> body
#[derive(Clone, Debug)]
pub(crate) struct CaseClause {
    pub pattern: PatternExpr,
    pub guard: Option<AExprAnnot>,
    pub body: Box<AExprAnnot>,
}

impl DocPrinter for CaseClause {
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

// case expr of
//   pattern (| guard)? -> expr
//   ..
#[derive(Clone, Debug)]
pub(crate) struct CaseExpr {
    pub keyword: ConcreteTokenAndLoc, // appearance of "case"
    pub argument: Box<AExprAnnot>,    // expr to be evaluated by case (eg: scrutinee)
    pub clauses: Vec<CaseClause>,     // [(test, optional guard, branch expression)]
}

impl DocPrinter for CaseExpr {
    fn to_doc(&self) -> Box<Doc> {
        let header = Doc::lit("case")
            .cat_space(self.argument.expr.to_doc())
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

// adt constructor expression (e.g., Some 42, Option.Some 42, Cons 1 Nil)
#[derive(Clone, Debug)]
pub(crate) struct ConstructorExpr {
    pub qualified: Option<ConcreteTokenAndLoc>, // type name if qualified (e.g., "Option" in Option.Some)
    pub constructor: ConcreteTokenAndLoc,       // constructor name (e.g., "Some")
    pub args: Vec<AExprAnnot>,                  // constructor positional arguments
    pub record_fields: Option<Vec<(ConcreteTokenAndLoc, AExprAnnot)>>, // record fields: { field = expr, ... }
}

impl DocPrinter for ConstructorExpr {
    fn to_doc(&self) -> Box<Doc> {
        let mut doc = Doc::nil();
        if let Some(x) = &self.qualified {
            doc = doc.cat(x.to_doc()).cat_lit(".");
        }
        doc = doc.cat(self.constructor.to_doc());

        if let Some(x) = &self.record_fields {
            doc = doc.cat_space_lit("{");
            for (field, field_expr) in x.iter() {
                doc = doc
                    .cat_space(field.to_doc())
                    .cat_lit(":")
                    .cat_space(field_expr.to_doc())
                    .cat_space_lit(",");
            }
            doc.cat_lit("}")
        } else {
            if self.args.is_empty() {
                return doc;
            }
            for i in self.args.iter() {
                doc = doc.cat_space(i.to_doc());
            }
            doc
        }
    }
}

// application expression
#[derive(Clone, Debug)]
pub(crate) struct AppExpr {
    pub fun: Box<AExprAnnot>,
    pub arguments: Vec<AExprAnnot>,
}

impl DocPrinter for AppExpr {
    fn to_doc(&self) -> Box<Doc> {
        let doc_app = self.fun.to_doc();
        let mut doc_args = Doc::nil();
        for (idx, arg) in self.arguments.iter().enumerate() {
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

// expressions end ---

pub(crate) struct TopLevelItems(pub Vec<TopLevelItem>);

impl DocPrinter for TopLevelItems {
    fn to_doc(&self) -> Box<Doc> {
        let mut doc = Doc::nil();
        for i in self.0.iter() {
            doc = doc.cat(Doc::line_force().cat(i.to_doc()));
        }
        doc
    }
}

#[derive(Clone, Debug)]
pub(crate) enum TopLevelItem {
    DataRecord(DataRecord),
    DataSum(DataSum),
    FunctionSignature(FnSig),
    FunctionDefinition(TopLevelFunction),
}

impl DocPrinter for TopLevelItem {
    fn to_doc(&self) -> Box<Doc> {
        use TopLevelItem::*;
        match self {
            DataRecord(x) => x.to_doc(),
            DataSum(x) => x.to_doc(),
            FunctionSignature(x) => x.to_doc(),
            FunctionDefinition(x) => x.to_doc(),
        }
    }
}

// expr along with (optional) type annotation
#[derive(Clone, Debug)]
pub(crate) struct AExprAnnot {
    pub expr: AExpr,
    pub type_expr: Option<ATypeExprComplex>,
}

impl DocPrinter for AExprAnnot {
    fn to_doc(&self) -> Box<Doc> {
        let doc_expr = self.expr.to_doc();
        if let Some(x) = &self.type_expr {
            return Doc::lit("(")
                .cat(doc_expr.cat_space_lit("::").cat_space(x.to_doc()))
                .cat_lit(")");
        }
        doc_expr
    }
}

#[derive(Clone, Debug)]
pub(crate) enum AExpr {
    StringExpr(LiteralStringExpr),
    NumericExpr(LiteralNumericExpr),
    IdentifierExpression(IdenExpr),
    LetExpression(LetExpr),
    AbstractionExpression(AbstractionExpr),
    CaseExpression(CaseExpr),
    ApplyExpression(AppExpr),
    BlockExpression(BlockExpr), // placeholder for possibly supporting multi-line constructs like do-notation
    ConstructorExpression(ConstructorExpr), // WIP: ADT constructor application
}

impl DocPrinter for AExpr {
    fn to_doc(&self) -> Box<Doc> {
        use AExpr::*;
        match self {
            StringExpr(x) => x.to_doc(),
            NumericExpr(x) => x.to_doc(),
            IdentifierExpression(x) => x.to_doc(),
            LetExpression(x) => x.to_doc(),
            AbstractionExpression(x) => x.to_doc(),
            CaseExpression(x) => x.to_doc(),
            ApplyExpression(x) => x.to_doc(),
            BlockExpression(x) => x.to_doc(),
            ConstructorExpression(x) => x.to_doc(),
        }
    }
}
