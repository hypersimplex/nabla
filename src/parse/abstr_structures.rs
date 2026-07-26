use std::fmt;
use std::sync::*;

// top level structures:
// data
// function signature
// function definition

use super::loc::*;

use super::concrete_token::*;

use super::printer::*;

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
        doc = cat_space(mk_lit("("), doc);
        for i in self.type_parameters.iter() {
            doc = cat_space(doc, i.to_doc());
        }
        doc = cat_space(doc, mk_lit(")"));
        doc
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
            return cat_space(
                cat_space(
                    mk_lit("("),
                    cat_space(cat_space(doc_head, mk_lit("->")), doc_tail),
                ),
                mk_lit(")"),
            );
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
        cat_space(
            self.identifier.to_doc(),
            cat_space(mk_lit("::"), self.ty.to_doc()),
        )
    }
}

// might be useful when adding support for imperative constructs / do notation
#[derive(Clone, Debug)]
pub(crate) struct BlockExpr(pub Vec<AExprAnnot>);

impl DocPrinter for BlockExpr {
    fn to_doc(&self) -> Box<Doc> {
        let mut doc = mk_nil();
        let mut is_first = true;
        for i in self.0.iter() {
            if !is_first {
                doc = mk_cat(doc, mk_line());
            }
            is_first = false;
            doc = mk_cat(doc, i.to_doc());
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
        let mut doc = cat_space(mk_lit("data"), self.identifier.to_doc());
        for i in self.params.iter() {
            doc = cat_space(doc, i.to_doc());
        }
        doc = cat_space(doc, mk_lit("{"));

        let mut doc_fields = mk_nil();
        for (field_name, type_expr) in self.components.iter() {
            doc_fields = mk_cat(doc_fields, mk_line_force());
            doc_fields = mk_cat(doc_fields, field_name.to_doc());
            doc_fields = cat_space(doc_fields, mk_lit("::"));
            doc_fields = cat_space(doc_fields, type_expr.to_doc());
            doc_fields = mk_cat(doc_fields, mk_lit(", "));
        }
        doc = mk_cat(doc, mk_nest(4, doc_fields));
        doc = mk_cat(doc, mk_cat(mk_line_force(), mk_lit("}")));
        doc
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
        let mut doc = mk_lit("data");
        doc = cat_space(doc, self.identifier.to_doc());
        for i in self.params.iter() {
            doc = cat_space(doc, i.to_doc());
        }

        let mut doc_variants = mk_nil();
        for (idx, (constructor_name, type_exprs)) in self.variants.iter().enumerate() {
            let mut doc_variant = if idx == 0 { mk_lit("=") } else { mk_lit("|") };
            doc_variant = cat_space(doc_variant, constructor_name.to_doc());
            for i in type_exprs.iter() {
                doc_variant = cat_space(doc_variant, i.to_doc());
            }
            doc_variant = mk_cat(mk_line_force(), doc_variant);
            doc_variants = mk_cat(doc_variants, doc_variant);
        }
        mk_cat(doc, mk_nest(4, doc_variants))
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
        mk_lit(&format!("\"{}\"", self.literal.token))
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
        doc = mk_cat(doc, mk_nest(4, mk_cat(mk_line_force(), self.expr.to_doc())));
        mk_cat(mk_line_force(), doc)
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
    pub name: Option<ConcreteTokenAndLoc>,   // named or annonymous
    pub pattern: Vec<ConcreteTokenAndLoc>,   // parameter tokens (verbatim); TODO: retire this?
    pub param_patterns: Vec<PatternExpr>,    // original parameter patterns
    pub expr: Box<AExprAnnot>,               // body of lambda
    pub type_expr: Option<ATypeExprComplex>, // optional type annotation
}

impl DocPrinter for AbstractionExpr {
    fn to_doc(&self) -> Box<Doc> {
        let mut doc_abstr = match &self.name {
            Some(x) => {
                let mut doc_name = x.to_doc();
                if let Some(type_expr) = &self.type_expr {
                    doc_name = mk_cat(doc_name, mk_cat(mk_lit(" :: "), type_expr.to_doc()));
                }
                cat_space(doc_name, mk_lit("="))
            }
            _ => mk_nil(),
        };
        doc_abstr = cat_space(doc_abstr, mk_lit("\\"));
        for (idx, pat_expr) in self.param_patterns.iter().enumerate() {
            if idx != 0 {
                doc_abstr = cat_space(doc_abstr, pat_expr.to_doc());
            } else {
                doc_abstr = mk_cat(doc_abstr, pat_expr.to_doc());
            }
        }
        doc_abstr = cat_space(doc_abstr, mk_lit("->"));
        let doc_body = self.expr.to_doc();
        cat_space(doc_abstr, mk_nest(4, doc_body))
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
            Wild => mk_lit("_"),
            Variable(x) => x.to_doc(),
            Literal(x) => x.to_doc(), // 42, "hello" (literal value)
            Range { start, end } => {
                cat_space(cat_space(start.to_doc(), mk_lit("..")), end.to_doc())
            }
            Constructor {
                qualified,
                constructor,
                args,
            } => {
                let mut doc = mk_nil();
                if let Some(x) = &qualified {
                    doc = mk_cat(doc, x.to_doc());
                }
                doc = mk_cat(doc, mk_lit("."));
                doc = cat_space(doc, constructor.to_doc());
                doc = mk_cat(doc, args.to_doc());
                doc
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
            Inclusive(x) => x.to_doc(),
            Exclusive(x) => x.to_doc(),
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
                    mk_nil()
                } else {
                    let mut doc = mk_nil();
                    for i in x.iter() {
                        doc = cat_space(doc, i.to_doc());
                    }
                    doc
                }
            }
            Record { fields, rest } => {
                let mut doc = mk_lit("{");
                doc = mk_cat(doc, mk_line());
                for (field, pat) in fields.iter() {
                    let mut entry = cat_space(field.to_doc(), mk_lit(":"));
                    entry = cat_space(entry, pat.to_doc());
                    entry = mk_cat(entry, mk_lit(","));
                    doc = mk_cat(mk_cat(doc, mk_line()), entry);
                }
                doc = mk_cat(doc, mk_line());
                doc = mk_cat(doc, mk_lit("}"));
                doc
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
            doc_pat_and_guard = cat_space(doc_pat_and_guard, mk_lit("|"));
            doc_pat_and_guard = cat_space(doc_pat_and_guard, doc_guard);
        }
        let doc_clause_lhs = cat_space(doc_pat_and_guard, mk_lit("->"));
        cat_space(doc_clause_lhs, mk_nest(4, self.body.to_doc()))
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
        use Doc::*;
        let mut header = cat_space(mk_lit("case"), self.argument.expr.to_doc());
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
        let mut doc = mk_nil();
        if let Some(x) = &self.qualified {
            doc = mk_cat(doc, x.to_doc());
        }
        doc = mk_cat(doc, mk_lit("."));
        doc = mk_cat(doc, self.constructor.to_doc());

        if let Some(x) = &self.record_fields {
            doc = cat_space(doc, mk_lit("{"));
            for (field, field_expr) in x.iter() {
                doc = mk_cat(doc, mk_line());
                doc = cat_space(field.to_doc(), field_expr.to_doc());
                doc = cat_space(doc, mk_lit(","));
            }
            doc = mk_cat(doc, mk_line());
            doc = mk_cat(doc, mk_lit("}"));
            doc
        } else {
            if self.args.is_empty() {
                return doc;
            }
            for i in self.args.iter() {
                doc = cat_space(doc, i.to_doc());
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
        let mut doc_app = self.fun.to_doc();
        let mut doc_args = mk_nil();
        for (idx, arg) in self.arguments.iter().enumerate() {
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

// expressions end ---

pub(crate) struct TopLevelItems(pub Vec<TopLevelItem>);

impl DocPrinter for TopLevelItems {
    fn to_doc(&self) -> Box<Doc> {
        let mut doc = mk_nil();
        for i in self.0.iter() {
            doc = mk_cat(doc, mk_cat(mk_line_force(), i.to_doc()));
        }
        doc
    }
}

#[derive(Clone, Debug)]
pub enum TopLevelItem {
    DataRecord(DataRecord),
    DataSum(DataSum),
    FunctionSignature(FnSig),
    FunctionDefinition(AbstractionExpr),
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
            return mk_cat(
                mk_cat(
                    mk_lit("("),
                    cat_space(cat_space(doc_expr, mk_lit("::")), x.to_doc()),
                ),
                mk_lit(")"),
            );
        }
        doc_expr
    }
}

#[derive(Clone, Debug)]
pub(crate) enum AExpr {
    StringExpr(LiteralStringExpr),
    NumericExpr(LiteralNumericExpr),
    UnitExpr,
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
            UnitExpr => mk_lit("()"),
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

pub(crate) struct DisplayItemIndent<T> {
    pub current: usize,
    pub indent: usize,
    pub item: T,
}

impl<T> DisplayItemIndent<T> {
    fn new(current: usize, indent: usize, item: T) -> Self {
        Self {
            current,
            indent,
            item,
        }
    }
    fn get_relative_indent(&self) -> usize {
        self.current.max(self.indent) - self.current
    }
    fn indent(&self) -> String {
        format!(
            "{empty:>width$}",
            empty = "",
            width = self.get_relative_indent()
        )
    }
    fn indent_abs(&self) -> String {
        format!(
            "{empty:>width$}",
            empty = "",
            width = self.current.max(self.indent)
        )
    }
    fn indent_abs_offset(&self, additional_offset: usize) -> String {
        format!(
            "{empty:>width$}",
            empty = "",
            width = self.current.max(self.indent) + additional_offset
        )
    }
}

impl fmt::Display for TopLevelItem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TopLevelItem::DataRecord(x) => {
                write!(f, "{}", DisplayItemIndent::new(0, 0, x));
            }
            TopLevelItem::DataSum(x) => {
                write!(f, "{}", DisplayItemIndent::new(0, 0, x));
            }
            TopLevelItem::FunctionSignature(x) => {
                write!(f, "{}", DisplayItemIndent::new(0, 0, x));
            }
            TopLevelItem::FunctionDefinition(x) => {
                write!(f, "{}", DisplayItemIndent::new(0, 0, x));
            }
        }
        Ok(())
    }
}

impl fmt::Display for DisplayItemIndent<&DataRecord> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}data {} < ", self.indent(), self.item.identifier.token);

        let _l = self.item.params.len();

        for (_idx, i) in self.item.params.iter().enumerate() {
            write!(f, "{} ", i);
        }
        writeln!(f, "> {{");

        for (component_name, type_expr) in self.item.components.iter() {
            writeln!(
                f,
                "{}  {} :: {}",
                self.indent(),
                component_name.token,
                type_expr
            );
        }

        writeln!(f, "}}");

        Ok(())
    }
}

impl fmt::Display for DisplayItemIndent<&DataSum> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}data {} < ", self.indent(), self.item.identifier.token).ok();

        let _l = self.item.params.len();

        for (_idx, i) in self.item.params.iter().enumerate() {
            write!(f, "{} ", i).ok();
        }
        writeln!(f, "> =").ok();

        for (idx, (constructor_identifier, parameter_types)) in
            self.item.variants.iter().enumerate()
        {
            write!(
                f,
                "{}  {}{} ",
                self.indent(),
                if idx > 0 { "| " } else { "" },
                constructor_identifier.token
            )
            .ok();
            for i in parameter_types.iter() {
                write!(f, "{} ", i).ok();
            }
            writeln!(f).ok();
        }

        Ok(())
    }
}

impl fmt::Display for DisplayItemIndent<&FnSig> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}[FnSig] {} :: {}",
            self.indent(),
            self.item.identifier.token,
            self.item.ty
        );
        Ok(())
    }
}

impl fmt::Display for ATypeExprComplex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ATypeExprComplex::Iden(x) => {
                write!(f, "{}", x);
            }
            ATypeExprComplex::Fun(x) => {
                write!(f, "{}", x);
            }
        }
        Ok(())
    }
}

impl fmt::Display for ATypeExprIden {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.type_parameters.is_empty() {
            write!(f, "{}", self.identifier.token);
        } else {
            write!(f, "{}< ", self.identifier.token);
            for i in self.type_parameters.iter() {
                write!(f, "{} ", i);
            }
            write!(f, ">");
        }
        Ok(())
    }
}

impl fmt::Display for ATypeExprFun {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        {
            let guard = self.head.lock().unwrap();
            let content_head = &*guard;
            write!(
                f,
                "{}{}",
                if self.tail.is_some() { "(" } else { "" },
                content_head
            );
        }

        if let Some(x) = self.tail.as_deref() {
            let guard = x.lock().unwrap();
            let content_tail = &*guard;
            write!(f, " -> {})", content_tail);
        }
        Ok(())
    }
}

impl fmt::Display for DisplayItemIndent<&AbstractionExpr> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut s = String::new();
        // TODO: add wrapping parenthesis () around lambda

        match &self.item.name {
            Some(x) => {
                s += &format!("{}", x.token);
            }
            _ => {
                s += &format!("\\");
            }
        }

        if let Some(ref x) = self.item.type_expr {
            s += &format!(" :: {}", x);
        }

        if !self.item.pattern.is_empty() {
            s += " ";
        }

        for (idx, p) in self.item.pattern.iter().enumerate() {
            s += &format!(
                "{}{}",
                p.token,
                if idx + 1 < self.item.pattern.len() {
                    " "
                } else {
                    ""
                }
            );
        }

        s += " -> ";

        let _lhs_length = s.len();

        writeln!(f, "{}{}", self.indent(), s);

        write!(
            f,
            "{}",
            DisplayItemIndent::new(
                // self.indent_abs().len() + lhs_length,
                0,
                self.indent + 2,
                &*self.item.expr
            )
        );

        Ok(())
    }
}

impl fmt::Display for DisplayItemIndent<&LetExpr> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "{}let", self.indent());
        for (pattern, def_expr) in self.item.defs.iter() {
            let mut s = format_pattern(pattern);
            s += " = ";
            writeln!(f, "{}{}", self.indent_abs_offset(2), s);
            writeln!(
                f,
                "{}",
                DisplayItemIndent::new(0, self.indent + 4, def_expr)
            );
        }

        writeln!(f, "{}in", self.indent_abs());
        writeln!(
            f,
            "{}",
            DisplayItemIndent::new(0, self.indent + 2, &*self.item.expr)
        );
        Ok(())
    }
}

impl fmt::Display for DisplayItemIndent<&CaseExpr> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut s = "case ".to_string();
        s += &format!(
            "{} of",
            DisplayItemIndent::new(
                self.indent_abs().len() + s.len(),
                self.indent + 2,
                &*self.item.argument
            )
        );
        writeln!(f, "{}{}", self.indent(), s)?;

        for (idx, clause) in self.item.clauses.iter().enumerate() {
            let mut ss = String::new();
            let display_pattern =
                DisplayItemIndent::new(self.indent_abs().len(), self.indent, &clause.pattern);
            ss += &format!("{}", &display_pattern);
            if let Some(guard) = &clause.guard {
                ss += " | ";
                ss += &format!(
                    "{}",
                    DisplayItemIndent::new(
                        self.indent_abs().len() + ss.len(),
                        self.indent + 2,
                        guard
                    )
                );
            }
            ss += " -> {";
            writeln!(f, "{}  {}", self.indent_abs(), ss)?;
            writeln!(
                f,
                "{}  {}",
                self.indent_abs(),
                DisplayItemIndent::new(self.indent_abs().len() + 2, self.indent + 2, &*clause.body)
            )?;
            write!(f, "{}  }}", self.indent_abs())?;
            if idx + 1 < self.item.clauses.len() {
                writeln!(f)?;
            }
        }
        Ok(())
    }
}

impl fmt::Display for DisplayItemIndent<&LiteralStringExpr> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}", self.indent(), self.item.literal.token);
        Ok(())
    }
}

impl fmt::Display for DisplayItemIndent<&LiteralNumericExpr> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}", self.indent(), self.item.literal.token);
        Ok(())
    }
}
impl fmt::Display for DisplayItemIndent<&IdenExpr> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}", self.indent(), self.item.iden.token);
        Ok(())
    }
}

impl fmt::Display for DisplayItemIndent<&ConstructorExpr> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f);
        write!(f, "{}", self.indent_abs())?;

        // print qualified name if present
        if let Some(qualified) = &self.item.qualified {
            write!(f, "{}.{}", qualified.token, self.item.constructor.token)?;
        } else {
            write!(f, "{}", self.item.constructor.token)?;
        }

        // print arguments
        for arg in &self.item.args {
            writeln!(f);
            write!(f, "{}", self.indent_abs())?;
            write!(
                f,
                "  {}",
                DisplayItemIndent::new(self.current + self.indent, self.indent, &arg.expr)
            )?;
        }
        Ok(())
    }
}

impl fmt::Display for DisplayItemIndent<&PatternExpr> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.item {
            PatternExpr::Wild => write!(f, "_"),
            PatternExpr::Variable(var) => write!(f, "{}", var.token),
            PatternExpr::Literal(lit) => write!(
                f,
                "{}",
                DisplayItemIndent::new(self.current, self.indent, &lit.expr)
            ),
            PatternExpr::Range { start, end } => {
                let (start_bound_type, start_lit) = match start {
                    PatternRangeBound::Inclusive(lit) => ("[", lit),
                    PatternRangeBound::Exclusive(lit) => ("(", lit),
                };
                let (end_bound_type, end_lit) = match end {
                    PatternRangeBound::Inclusive(lit) => ("]", lit),
                    PatternRangeBound::Exclusive(lit) => (")", lit),
                };
                write!(
                    f,
                    "{}{}..{}{}",
                    start_bound_type,
                    DisplayItemIndent::new(self.current, self.indent, &start_lit.expr),
                    DisplayItemIndent::new(self.current, self.indent, &end_lit.expr),
                    end_bound_type,
                )
            }
            PatternExpr::Constructor {
                qualified,
                constructor,
                args,
            } => {
                if let Some(qual) = qualified {
                    write!(f, "{}.{}", qual.token, constructor.token)?;
                } else {
                    write!(f, "{}", constructor.token)?;
                }
                match args {
                    PatternConstructorArgs::Positional(args) => {
                        for arg in args {
                            write!(
                                f,
                                " {}",
                                DisplayItemIndent::new(self.current, self.indent, arg)
                            )?;
                        }
                    }
                    PatternConstructorArgs::Record { fields, rest } => {
                        write!(f, " {{ ")?;
                        let mut first = true;
                        for (field_name, field_pattern) in fields {
                            if !first {
                                write!(f, ", ")?;
                            }
                            first = false;
                            write!(f, "{}", field_name.token)?;
                            // check if it's field punning by checking if the pattern is a variable with same name
                            match field_pattern {
                                PatternExpr::Variable(var) if var.token == field_name.token => {
                                    // field punning - just show the field name
                                }
                                _ => {
                                    // explicit binding
                                    write!(
                                        f,
                                        " = {}",
                                        DisplayItemIndent::new(
                                            self.current,
                                            self.indent,
                                            field_pattern
                                        )
                                    )?;
                                }
                            }
                        }
                        if *rest {
                            if !first {
                                write!(f, ", ")?;
                            }
                            write!(f, "..")?;
                        }
                        write!(f, " }}")?;
                    }
                }
                Ok(())
            }
        }
    }
}

impl fmt::Display for DisplayItemIndent<&AppExpr> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = "(".to_string();
        writeln!(
            f,
            "{}{}{}",
            self.indent(),
            s,
            DisplayItemIndent::new(
                self.indent_abs().len() + s.len() + 1,
                self.indent + 1,
                &*self.item.fun
            )
        );
        for i in self.item.arguments.iter() {
            writeln!(
                f,
                "{} {}",
                self.indent_abs(),
                DisplayItemIndent::new(self.indent_abs().len() + 1, self.indent + 1, i)
            );
        }
        write!(f, "{})", self.indent_abs());
        Ok(())
    }
}
impl fmt::Display for DisplayItemIndent<&BlockExpr> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let l = self.item.0.len();
        for (idx, i) in self.item.0.iter().enumerate() {
            let s = (if idx == 0 {
                self.indent()
            } else {
                self.indent_abs()
            })
            .to_string();

            write!(
                f,
                "{}  {}",
                s,
                DisplayItemIndent::new(self.indent_abs().len() + 2, self.indent + 2, i)
            );
            if idx + 1 < l {
                writeln!(f);
            }
        }
        Ok(())
    }
}

impl fmt::Display for DisplayItemIndent<&AExprAnnot> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}{}",
            DisplayItemIndent::new(self.current, self.indent, &self.item.expr),
            DisplayItemIndent::new(self.current, self.indent, &self.item.type_expr)
        ); //TODO: write annotation
        Ok(())
    }
}

fn format_pattern(pattern: &PatternExpr) -> String {
    use PatternExpr::*;
    match pattern {
        Wild => "_".into(),
        Variable(var) => format!("{}", var.token),
        Literal(expr) => match &expr.expr {
            AExpr::NumericExpr(lit) => format!("{}", lit.literal.token),
            AExpr::StringExpr(lit) => format!("{}", lit.literal.token),
            other => format!("{:?}", other),
        },
        Range { start, end } => {
            let (start_bound_type, start_expr) = match start {
                PatternRangeBound::Inclusive(expr) => ("[", expr),
                PatternRangeBound::Exclusive(expr) => ("(", expr),
            };
            let (end_bound_type, end_expr) = match end {
                PatternRangeBound::Inclusive(expr) => ("]", expr),
                PatternRangeBound::Exclusive(expr) => (")", expr),
            };
            format!(
                "{}{}..{}{}",
                start_bound_type,
                format_pattern(&PatternExpr::Literal(start_expr.clone())),
                format_pattern(&PatternExpr::Literal(end_expr.clone())),
                end_bound_type,
            )
        }
        Constructor {
            qualified,
            constructor,
            args,
        } => {
            let mut s = String::new();
            if let Some(q) = qualified {
                s.push_str(&format!("{}.", q.token));
            }
            s.push_str(&format!("{}", constructor.token));
            match args {
                PatternConstructorArgs::Positional(args) => {
                    if !args.is_empty() {
                        let rendered: Vec<String> = args.iter().map(format_pattern).collect();
                        s.push(' ');
                        s.push_str(&rendered.join(" "));
                    }
                }
                PatternConstructorArgs::Record { fields, rest } => {
                    s.push_str(" {");
                    let mut parts: Vec<String> = fields
                        .iter()
                        .map(|(name, pat)| format!("{} = {}", name.token, format_pattern(pat)))
                        .collect();
                    if *rest {
                        parts.push("..".into());
                    }
                    s.push_str(&parts.join(", "));
                    s.push('}');
                }
            }
            s
        }
    }
}

impl fmt::Display for DisplayItemIndent<&Option<ATypeExprComplex>> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.item {
            Some(x) => {
                write!(f, " :: {}", x);
            }
            _ => {}
        }
        Ok(())
    }
}

impl fmt::Display for DisplayItemIndent<&AExpr> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.item {
            AExpr::UnitExpr => {
                write!(f, "{}()", self.indent_abs())?;
            }
            AExpr::LetExpression(x) => {
                write!(
                    f,
                    "{}",
                    DisplayItemIndent::new(self.current, self.indent, x)
                );
            }
            AExpr::AbstractionExpression(x) => {
                write!(
                    f,
                    "{}",
                    DisplayItemIndent::new(self.current, self.indent, x)
                );
            }
            AExpr::CaseExpression(x) => {
                write!(
                    f,
                    "{}",
                    DisplayItemIndent::new(self.current, self.indent, x)
                );
            }
            AExpr::StringExpr(x) => {
                write!(
                    f,
                    "{}",
                    DisplayItemIndent::new(self.current, self.indent, x)
                );
            }
            AExpr::NumericExpr(x) => {
                write!(
                    f,
                    "{}",
                    DisplayItemIndent::new(self.current, self.indent, x)
                );
            }
            AExpr::IdentifierExpression(x) => {
                write!(
                    f,
                    "{}",
                    DisplayItemIndent::new(self.current, self.indent, x)
                );
            }
            AExpr::ApplyExpression(x) => {
                write!(
                    f,
                    "{}",
                    DisplayItemIndent::new(self.current, self.indent, x)
                );
            }
            AExpr::BlockExpression(x) => {
                write!(
                    f,
                    "{}",
                    DisplayItemIndent::new(self.current, self.indent, x)
                );
            }
            AExpr::ConstructorExpression(x) => {
                write!(
                    f,
                    "{}",
                    DisplayItemIndent::new(self.current, self.indent, x)
                );
            }
        }
        Ok(())
    }
}
