//! [WIP] internal constructs of STG
//!
//! the runtime portion to support this is at `src/runtime/` directory

/// top-level bindings
#[derive(Clone, Debug)]
pub(crate) struct StgTopLevel(pub Vec<StgBind>);

#[derive(Clone, Debug)]
pub(crate) enum StgExpr {
    // function application or entering the closure
    AppFn(StgAppFn),

    // apply constructor
    AppCon(StgAppCon),

    // primitive function/op application
    AppPrim(StgAppPrim),

    // evaluation
    Case(StgCase),

    // corresponds to heap allocation of the RHS closure
    Let(StgLet),

    Lit(StgLit),
}

/// function application
///
/// at STG level, application can be dynamic / higher-order, then
/// it can be one of the cases:
///   - exact saturation
///   - under-saturation (needs to create another Partial Application closure)
///   - over-saturation (leftover arguments passed to the returned closure)
///   - 0 arguments (enter the closure); cases to consider:
///     - a thunk / updatable closure that is unevaluated (not in WHNF)
///     - constructor/primitive (already in WHNF)
///     - non-updatable closure (already in WHNF)
///     - indirection (already in WHNF)
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct StgAppFn {
    // stores name, id, and other meta info that conceptually acts like a simple
    // variable
    pub fun: StgVar,

    // flattened arguments
    pub args: Vec<StgArg>,
}

/// constructor application
///
/// at STG level, application is fully saturating
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct StgAppCon {
    pub constructor: StgCon,

    // flattened arguments
    pub args: Vec<StgArg>,
}

/// argument is expected to be in ANF which means these cannot be an arbitrarily
/// complex expression
///
/// this is expected to be only used for applications
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum StgArg {
    Var(StgVar),
    Lit(StgLit),
}

/// this is like a variable, but contains more information that are helpful
/// to STG-level machinery
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct StgVar {
    pub name: String,       // for debuggability
    pub id: u64,            // used by runtime
    pub repr_rt: StgReprRt, // used by runtime for memory layout
}

/// representation seen by the runtime
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum StgReprRt {
    Ptr, // pointer to heap allocated data
    PrimInt64,
    PrimFloat64,
    PrimBytes, // unboxed byte buffer that have static lifetime
}

/// primitive unboxed literal
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum StgLit {
    Int(i64),
    Float(f64),
    Bytes(String),
}

/// at STG level, application is fully saturating
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct StgAppPrim {
    pub prim_fn: StgPrimFn,
    pub args: Vec<StgArg>,
}

/// primitive machine operations / functions (unboxed arithmetic, comparisons, conversions)
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum StgPrimFn {
    // Int64# arithmetic
    AddInt,
    SubInt,
    MulInt,
    DivInt,
    NegInt,

    // Int64# comparisons
    EqInt,
    LtInt,
    LeInt,
    GtInt,
    GeInt,

    // Float64# arithmetic
    AddFloat,
    SubFloat,
    MulFloat,
    DivFloat,
    NegFloat,

    // Float64# comparisons
    EqFloat,
    LtFloat,
    LeFloat,
    GtFloat,
    GeFloat,

    // Conversions
    IntFromFloat,
    FloatFromInt,
}

/// let expression; simplified to support mutually recursive group which
/// subsumes non-rec `let`
#[derive(Clone, Debug)]
pub(crate) struct StgLet {
    pub binds: Vec<StgBind>,
    pub body: Box<StgExpr>,
}

/// binding pair
#[derive(Clone, Debug)]
pub(crate) struct StgBind {
    pub lhs: StgVar,
    pub rhs: StgClosure,
}

/// note: runtime needs to reserve at least 2 words of payload for any closure
/// in order to support indirection closure, even if `free_vars` is empty
#[derive(Clone, Debug)]
pub(crate) struct StgClosure {
    pub free_vars: Vec<StgVar>,
    pub updatable: bool,
    pub params: Vec<StgVar>,
    pub body: StgExpr,
}

#[derive(Clone, Debug)]
pub(crate) struct StgCase {
    pub scrutinee: Box<StgExpr>,

    pub binder_to_eval_result: StgVar,

    // note: alternatives need to be in discriminant order for offset jump
    pub alternatives: Vec<StgAlt>,
}

/// pattern selector in a case alternative
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum StgAltCon {
    /// default / wildcard fallback branch
    Default,

    /// matches ADT constructor
    Data(StgCon),

    /// matches unboxed primitive literal
    Lit(StgLit),
}

/// an alternative branch in a case expression
#[derive(Clone, Debug)]
pub(crate) struct StgAlt {
    pub con: StgAltCon,

    /// binders for constructor fields (guaranteed empty for Lit and Default)
    pub params: Vec<StgVar>,

    pub body: StgExpr,
}

/// data constructor
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct StgCon {
    pub ty_name: String,          // ADT name
    pub constructor_name: String, // constructor variant
    pub discriminant: u64,        // for runtime jump offset
    pub arity: u64,
    pub fields: Vec<StgReprRt>,
}
