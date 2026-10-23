use crate::parse::concrete_token;
use crate::typecheck::env_v_var_to_ty_scheme::*;
use crate::typecheck::ty_expr::*;
use crate::typecheck::ty_scheme::*;
use crate::typecheck::ty_var_name_supply::*;
use crate::typecheck::v_expr::*;
use crate::typecheck::v_expr_typed::*;
use crate::typecheck::v_var_name::*;

// initialize all builtin values (operators, functions) in the given environment
pub fn init_builtin_values(env: &mut EnvVVarToTyScheme, ns: &mut TyVarNameSupply) {
    init_v_var_ty_scheme_builtin_unary_plus(env, ns);
    init_v_var_ty_scheme_builtin_binary_plus(env, ns);
    init_v_var_ty_scheme_builtin_unary_minus(env, ns);
    init_v_var_ty_scheme_builtin_unary_not(env, ns);
    init_v_var_ty_scheme_builtin_binary_minus(env, ns);
    init_v_var_ty_scheme_builtin_binary_mul(env, ns);
    init_v_var_ty_scheme_builtin_binary_div(env, ns);
    init_v_var_ty_scheme_builtin_binary_lt(env, ns);
    init_v_var_ty_scheme_builtin_binary_le(env, ns);
    init_v_var_ty_scheme_builtin_binary_gt(env, ns);
    init_v_var_ty_scheme_builtin_binary_ge(env, ns);
    init_v_var_ty_scheme_builtin_binary_eq(env, ns);
    init_v_var_ty_scheme_builtin_logical_and(env, ns);
    init_v_var_ty_scheme_builtin_logical_or(env, ns);
    init_v_var_ty_scheme_builtin_match_fail(env, ns);
    init_unboxed_primitive_ops(env);
    init_v_var_ty_scheme_builtin_int64_from_float64(env, ns);
    init_v_var_ty_scheme_builtin_float64_from_int64(env, ns);
}

/// resolve builtin function name to FnBuiltin
pub(crate) fn resolve_builtin_fn(name: &str) -> Option<FnBuiltin> {
    match name {
        "prim_int64_from_float64" => Some(FnBuiltin::PrimConvertInt64FromFloat64),
        "prim_float64_from_int64" => Some(FnBuiltin::PrimConvertFloat64FromInt64),
        "prim_add_int64" => Some(FnBuiltin::PrimAddInt64),
        "prim_sub_int64" => Some(FnBuiltin::PrimSubInt64),
        "prim_mul_int64" => Some(FnBuiltin::PrimMulInt64),
        "prim_div_int64" => Some(FnBuiltin::PrimDivInt64),
        "prim_neg_int64" => Some(FnBuiltin::PrimNegInt64),
        "prim_eq_int64" => Some(FnBuiltin::PrimEqInt64),
        "prim_lt_int64" => Some(FnBuiltin::PrimLtInt64),
        "prim_lte_int64" => Some(FnBuiltin::PrimLteInt64),
        "prim_gt_int64" => Some(FnBuiltin::PrimGtInt64),
        "prim_gte_int64" => Some(FnBuiltin::PrimGteInt64),
        "prim_add_float64" => Some(FnBuiltin::PrimAddFloat64),
        "prim_sub_float64" => Some(FnBuiltin::PrimSubFloat64),
        "prim_mul_float64" => Some(FnBuiltin::PrimMulFloat64),
        "prim_div_float64" => Some(FnBuiltin::PrimDivFloat64),
        "prim_neg_float64" => Some(FnBuiltin::PrimNegFloat64),
        "prim_eq_float64" => Some(FnBuiltin::PrimEqFloat64),
        "prim_lt_float64" => Some(FnBuiltin::PrimLtFloat64),
        "prim_lte_float64" => Some(FnBuiltin::PrimLteFloat64),
        "prim_gt_float64" => Some(FnBuiltin::PrimGtFloat64),
        "prim_gte_float64" => Some(FnBuiltin::PrimGteFloat64),
        "int64_from_float64" => Some(FnBuiltin::ConvertInt64FromFloat64),
        "float64_from_int64" => Some(FnBuiltin::ConvertFloat64FromInt64),
        "match_fail" => Some(FnBuiltin::MatchFail),
        _ => None,
    }
}

// maps builtin value-level variable unary plus to type scheme ([tv_numeric] (tv_numeric -> tv_numeric))
pub fn init_v_var_ty_scheme_builtin_unary_plus(
    env_v_var_to_ty_scheme: &mut EnvVVarToTyScheme,
    ns: &mut TyVarNameSupply,
) {
    let ty_var_numeric = ns.generate();
    env_v_var_to_ty_scheme.insert(
        VVar::Named(VVarName {
            token: concrete_token::ConcreteToken::UnaryPlus,
            loc: None,
            builtin: Some(FnBuiltin::UnaryPlus),
        }),
        TyScheme {
            ty_vars_schematic: vec![ty_var_numeric.clone()],
            // maps to a function type expression: ty_var_numeric -> ty_var_numeric
            ty_expr: Box::new(mk_ty_arrow(
                TyExpr::TyVar(ty_var_numeric.clone()),
                TyExpr::TyVar(ty_var_numeric),
            )),
        },
    );
}

// maps builtin value-level variable binary add to type scheme ([tv_numeric] (tv_numeric -> tv_numeric -> tv_numeric))
pub fn init_v_var_ty_scheme_builtin_binary_plus(
    env_v_var_to_ty_scheme: &mut EnvVVarToTyScheme,
    ns: &mut TyVarNameSupply,
) {
    let ty_var_numeric = ns.generate();
    env_v_var_to_ty_scheme.insert(
        VVar::Named(VVarName {
            token: concrete_token::ConcreteToken::BinaryPlus,
            loc: None,
            builtin: Some(FnBuiltin::BinaryAdd),
        }),
        TyScheme {
            ty_vars_schematic: vec![ty_var_numeric.clone()],
            // maps to a function type expression: ty_var_numeric -> ty_var_numeric -> ty_var_numeric
            ty_expr: Box::new(mk_ty_arrow_multi(
                vec![
                    TyExpr::TyVar(ty_var_numeric.clone()),
                    TyExpr::TyVar(ty_var_numeric.clone()),
                ],
                TyExpr::TyVar(ty_var_numeric),
            )),
        },
    );
}

// maps builtin unary minus to type scheme ([tv_numeric] (tv_numeric -> tv_numeric))
pub fn init_v_var_ty_scheme_builtin_unary_minus(
    env_v_var_to_ty_scheme: &mut EnvVVarToTyScheme,
    ns: &mut TyVarNameSupply,
) {
    let ty_var_numeric = ns.generate();
    env_v_var_to_ty_scheme.insert(
        VVar::Named(VVarName {
            token: concrete_token::ConcreteToken::UnaryMinus,
            loc: None,
            builtin: Some(FnBuiltin::UnaryNegate),
        }),
        TyScheme {
            ty_vars_schematic: vec![ty_var_numeric.clone()],
            ty_expr: Box::new(mk_ty_arrow(
                TyExpr::TyVar(ty_var_numeric.clone()),
                TyExpr::TyVar(ty_var_numeric),
            )),
        },
    );
}

// maps builtin unary logical not to type scheme (Bool -> Bool)
// note: mk_ty_bool resolves to the concrete Bool ADT
pub fn init_v_var_ty_scheme_builtin_unary_not(
    env_v_var_to_ty_scheme: &mut EnvVVarToTyScheme,
    ns: &mut TyVarNameSupply,
) {
    env_v_var_to_ty_scheme.insert(
        VVar::Named(VVarName {
            token: concrete_token::ConcreteToken::UnaryNot,
            loc: None,
            builtin: Some(FnBuiltin::UnaryLogicalNot),
        }),
        TyScheme {
            ty_vars_schematic: Vec::new(),
            ty_expr: Box::new(mk_ty_arrow(mk_ty_bool(), mk_ty_bool())),
        },
    );
}

// maps builtin binary minus to type scheme ([tv_numeric] (tv_numeric -> tv_numeric -> tv_numeric))
pub fn init_v_var_ty_scheme_builtin_binary_minus(
    env_v_var_to_ty_scheme: &mut EnvVVarToTyScheme,
    ns: &mut TyVarNameSupply,
) {
    let ty_var_numeric = ns.generate();
    env_v_var_to_ty_scheme.insert(
        VVar::Named(VVarName {
            token: concrete_token::ConcreteToken::BinaryMinus,
            loc: None,
            builtin: Some(FnBuiltin::BinarySub),
        }),
        TyScheme {
            ty_vars_schematic: vec![ty_var_numeric.clone()],
            ty_expr: Box::new(mk_ty_arrow_multi(
                vec![
                    TyExpr::TyVar(ty_var_numeric.clone()),
                    TyExpr::TyVar(ty_var_numeric.clone()),
                ],
                TyExpr::TyVar(ty_var_numeric),
            )),
        },
    );
}

// maps builtin binary multiply to type scheme ([tv_numeric] (tv_numeric -> tv_numeric -> tv_numeric))
pub fn init_v_var_ty_scheme_builtin_binary_mul(
    env_v_var_to_ty_scheme: &mut EnvVVarToTyScheme,
    ns: &mut TyVarNameSupply,
) {
    let ty_var_numeric = ns.generate();
    env_v_var_to_ty_scheme.insert(
        VVar::Named(VVarName {
            token: concrete_token::ConcreteToken::BinaryMul,
            loc: None,
            builtin: Some(FnBuiltin::BinaryMul),
        }),
        TyScheme {
            ty_vars_schematic: vec![ty_var_numeric.clone()],
            ty_expr: Box::new(mk_ty_arrow_multi(
                vec![
                    TyExpr::TyVar(ty_var_numeric.clone()),
                    TyExpr::TyVar(ty_var_numeric.clone()),
                ],
                TyExpr::TyVar(ty_var_numeric),
            )),
        },
    );
}

// maps builtin binary divide to type scheme ([tv_numeric] (tv_numeric -> tv_numeric -> tv_numeric))
pub fn init_v_var_ty_scheme_builtin_binary_div(
    env_v_var_to_ty_scheme: &mut EnvVVarToTyScheme,
    ns: &mut TyVarNameSupply,
) {
    let ty_var_numeric = ns.generate();
    env_v_var_to_ty_scheme.insert(
        VVar::Named(VVarName {
            token: concrete_token::ConcreteToken::BinaryDiv,
            loc: None,
            builtin: Some(FnBuiltin::BinaryDiv),
        }),
        TyScheme {
            ty_vars_schematic: vec![ty_var_numeric.clone()],
            ty_expr: Box::new(mk_ty_arrow_multi(
                vec![
                    TyExpr::TyVar(ty_var_numeric.clone()),
                    TyExpr::TyVar(ty_var_numeric.clone()),
                ],
                TyExpr::TyVar(ty_var_numeric),
            )),
        },
    );
}

// maps builtin binary less-than to type scheme ([tv] (tv -> tv -> Bool))
pub fn init_v_var_ty_scheme_builtin_binary_lt(
    env_v_var_to_ty_scheme: &mut EnvVVarToTyScheme,
    ns: &mut TyVarNameSupply,
) {
    let ty_var_any = ns.generate();
    env_v_var_to_ty_scheme.insert(
        VVar::Named(VVarName {
            token: concrete_token::ConcreteToken::AngleL,
            loc: None,
            builtin: Some(FnBuiltin::BinaryLess),
        }),
        TyScheme {
            ty_vars_schematic: vec![ty_var_any.clone()],
            ty_expr: Box::new(mk_ty_arrow_multi(
                vec![TyExpr::TyVar(ty_var_any.clone()), TyExpr::TyVar(ty_var_any)],
                mk_ty_bool(),
            )),
        },
    );
}

// maps builtin binary less-than-or-equal to type scheme ([tv] (tv -> tv -> Bool))
pub fn init_v_var_ty_scheme_builtin_binary_le(
    env_v_var_to_ty_scheme: &mut EnvVVarToTyScheme,
    ns: &mut TyVarNameSupply,
) {
    let ty_var_any = ns.generate();
    env_v_var_to_ty_scheme.insert(
        VVar::Named(VVarName {
            token: concrete_token::ConcreteToken::LessEqual,
            loc: None,
            builtin: Some(FnBuiltin::BinaryLessEqual),
        }),
        TyScheme {
            ty_vars_schematic: vec![ty_var_any.clone()],
            ty_expr: Box::new(mk_ty_arrow_multi(
                vec![TyExpr::TyVar(ty_var_any.clone()), TyExpr::TyVar(ty_var_any)],
                mk_ty_bool(),
            )),
        },
    );
}

// maps builtin binary greater-than to type scheme ([tv] (tv -> tv -> Bool))
pub fn init_v_var_ty_scheme_builtin_binary_gt(
    env_v_var_to_ty_scheme: &mut EnvVVarToTyScheme,
    ns: &mut TyVarNameSupply,
) {
    let ty_var_any = ns.generate();
    env_v_var_to_ty_scheme.insert(
        VVar::Named(VVarName {
            token: concrete_token::ConcreteToken::AngleR,
            loc: None,
            builtin: Some(FnBuiltin::BinaryGreater),
        }),
        TyScheme {
            ty_vars_schematic: vec![ty_var_any.clone()],
            ty_expr: Box::new(mk_ty_arrow_multi(
                vec![TyExpr::TyVar(ty_var_any.clone()), TyExpr::TyVar(ty_var_any)],
                mk_ty_bool(),
            )),
        },
    );
}

// maps builtin binary greater-than-or-equal to type scheme ([tv] (tv -> tv -> Bool))
pub fn init_v_var_ty_scheme_builtin_binary_ge(
    env_v_var_to_ty_scheme: &mut EnvVVarToTyScheme,
    ns: &mut TyVarNameSupply,
) {
    let ty_var_any = ns.generate();
    env_v_var_to_ty_scheme.insert(
        VVar::Named(VVarName {
            token: concrete_token::ConcreteToken::GreaterEqual,
            loc: None,
            builtin: Some(FnBuiltin::BinaryGreaterEqual),
        }),
        TyScheme {
            ty_vars_schematic: vec![ty_var_any.clone()],
            ty_expr: Box::new(mk_ty_arrow_multi(
                vec![TyExpr::TyVar(ty_var_any.clone()), TyExpr::TyVar(ty_var_any)],
                mk_ty_bool(),
            )),
        },
    );
}

// maps builtin binary equality to type scheme ([tv] (tv -> tv -> Bool))
pub fn init_v_var_ty_scheme_builtin_binary_eq(
    env_v_var_to_ty_scheme: &mut EnvVVarToTyScheme,
    ns: &mut TyVarNameSupply,
) {
    let ty_var_any = ns.generate();
    env_v_var_to_ty_scheme.insert(
        VVar::Named(VVarName {
            token: concrete_token::ConcreteToken::EqualEqual,
            loc: None,
            builtin: Some(FnBuiltin::BinaryEqual),
        }),
        TyScheme {
            ty_vars_schematic: vec![ty_var_any.clone()],
            ty_expr: Box::new(mk_ty_arrow_multi(
                vec![TyExpr::TyVar(ty_var_any.clone()), TyExpr::TyVar(ty_var_any)],
                mk_ty_bool(),
            )),
        },
    );
}

// maps builtin logical and to type scheme (Bool -> Bool -> Bool)
// note: mk_ty_bool resolves to the concrete Bool ADT
pub fn init_v_var_ty_scheme_builtin_logical_and(
    env_v_var_to_ty_scheme: &mut EnvVVarToTyScheme,
    ns: &mut TyVarNameSupply,
) {
    env_v_var_to_ty_scheme.insert(
        VVar::Named(VVarName {
            token: concrete_token::ConcreteToken::BinaryAnd,
            loc: None,
            builtin: Some(FnBuiltin::LogicalAnd),
        }),
        TyScheme {
            ty_vars_schematic: Vec::new(),
            ty_expr: Box::new(mk_ty_arrow_multi(
                vec![mk_ty_bool(), mk_ty_bool()],
                mk_ty_bool(),
            )),
        },
    );
}

// maps builtin logical or to type scheme (Bool -> Bool -> Bool)
// note: mk_ty_bool resolves to the concrete Bool ADT
pub fn init_v_var_ty_scheme_builtin_logical_or(
    env_v_var_to_ty_scheme: &mut EnvVVarToTyScheme,
    ns: &mut TyVarNameSupply,
) {
    env_v_var_to_ty_scheme.insert(
        VVar::Named(VVarName {
            token: concrete_token::ConcreteToken::BinaryOr,
            loc: None,
            builtin: Some(FnBuiltin::LogicalOr),
        }),
        TyScheme {
            ty_vars_schematic: Vec::new(),
            ty_expr: Box::new(mk_ty_arrow_multi(
                vec![mk_ty_bool(), mk_ty_bool()],
                mk_ty_bool(),
            )),
        },
    );
}

// maps builtin match-fail to type scheme ([tv] tv)
pub fn init_v_var_ty_scheme_builtin_match_fail(
    env_v_var_to_ty_scheme: &mut EnvVVarToTyScheme,
    ns: &mut TyVarNameSupply,
) {
    let ty_var_any = ns.generate();
    env_v_var_to_ty_scheme.insert(
        VVar::Named(VVarName {
            token: concrete_token::ConcreteToken::Iden("match_fail".to_string()),
            loc: None,
            builtin: Some(FnBuiltin::MatchFail),
        }),
        TyScheme {
            ty_vars_schematic: vec![ty_var_any.clone()],
            ty_expr: Box::new(TyExpr::TyVar(ty_var_any)),
        },
    );
}

fn register_unboxed_binop(
    env: &mut EnvVVarToTyScheme,
    name: &str,
    builtin: FnBuiltin,
    arg1_ty: TyExpr,
    arg2_ty: TyExpr,
    ret_ty: TyExpr,
) {
    env.insert(
        VVar::Named(VVarName {
            token: concrete_token::ConcreteToken::Iden(name.to_string()),
            loc: None,
            builtin: Some(builtin),
        }),
        TyScheme {
            ty_vars_schematic: Vec::new(),
            ty_expr: Box::new(mk_ty_arrow_multi(vec![arg1_ty, arg2_ty], ret_ty)),
        },
    );
}

fn register_unboxed_unop(
    env: &mut EnvVVarToTyScheme,
    name: &str,
    builtin: FnBuiltin,
    arg_ty: TyExpr,
    ret_ty: TyExpr,
) {
    env.insert(
        VVar::Named(VVarName {
            token: concrete_token::ConcreteToken::Iden(name.to_string()),
            loc: None,
            builtin: Some(builtin),
        }),
        TyScheme {
            ty_vars_schematic: Vec::new(),
            ty_expr: Box::new(mk_ty_arrow(arg_ty, ret_ty)),
        },
    );
}

pub fn init_unboxed_primitive_ops(env: &mut EnvVVarToTyScheme) {
    let i64_ops: [(&str, FnBuiltin, fn() -> TyExpr); 9] = [
        (
            "prim_add_int64",
            FnBuiltin::PrimAddInt64,
            mk_ty_unboxed_int64,
        ),
        (
            "prim_sub_int64",
            FnBuiltin::PrimSubInt64,
            mk_ty_unboxed_int64,
        ),
        (
            "prim_mul_int64",
            FnBuiltin::PrimMulInt64,
            mk_ty_unboxed_int64,
        ),
        (
            "prim_div_int64",
            FnBuiltin::PrimDivInt64,
            mk_ty_unboxed_int64,
        ),
        ("prim_eq_int64", FnBuiltin::PrimEqInt64, mk_ty_bool),
        ("prim_lt_int64", FnBuiltin::PrimLtInt64, mk_ty_bool),
        ("prim_lte_int64", FnBuiltin::PrimLteInt64, mk_ty_bool),
        ("prim_gt_int64", FnBuiltin::PrimGtInt64, mk_ty_bool),
        ("prim_gte_int64", FnBuiltin::PrimGteInt64, mk_ty_bool),
    ];
    for (name, builtin, ret_fn) in i64_ops {
        register_unboxed_binop(
            env,
            name,
            builtin,
            mk_ty_unboxed_int64(),
            mk_ty_unboxed_int64(),
            ret_fn(),
        );
    }
    register_unboxed_unop(
        env,
        "prim_neg_int64",
        FnBuiltin::PrimNegInt64,
        mk_ty_unboxed_int64(),
        mk_ty_unboxed_int64(),
    );

    let f64_ops: [(&str, FnBuiltin, fn() -> TyExpr); 9] = [
        (
            "prim_add_float64",
            FnBuiltin::PrimAddFloat64,
            mk_ty_unboxed_float64,
        ),
        (
            "prim_sub_float64",
            FnBuiltin::PrimSubFloat64,
            mk_ty_unboxed_float64,
        ),
        (
            "prim_mul_float64",
            FnBuiltin::PrimMulFloat64,
            mk_ty_unboxed_float64,
        ),
        (
            "prim_div_float64",
            FnBuiltin::PrimDivFloat64,
            mk_ty_unboxed_float64,
        ),
        ("prim_eq_float64", FnBuiltin::PrimEqFloat64, mk_ty_bool),
        ("prim_lt_float64", FnBuiltin::PrimLtFloat64, mk_ty_bool),
        ("prim_lte_float64", FnBuiltin::PrimLteFloat64, mk_ty_bool),
        ("prim_gt_float64", FnBuiltin::PrimGtFloat64, mk_ty_bool),
        ("prim_gte_float64", FnBuiltin::PrimGteFloat64, mk_ty_bool),
    ];
    for (name, builtin, ret_fn) in f64_ops {
        register_unboxed_binop(
            env,
            name,
            builtin,
            mk_ty_unboxed_float64(),
            mk_ty_unboxed_float64(),
            ret_fn(),
        );
    }
    register_unboxed_unop(
        env,
        "prim_neg_float64",
        FnBuiltin::PrimNegFloat64,
        mk_ty_unboxed_float64(),
        mk_ty_unboxed_float64(),
    );

    register_unboxed_unop(
        env,
        "prim_int64_from_float64",
        FnBuiltin::PrimConvertInt64FromFloat64,
        mk_ty_unboxed_float64(),
        mk_ty_unboxed_int64(),
    );
    register_unboxed_unop(
        env,
        "prim_float64_from_int64",
        FnBuiltin::PrimConvertFloat64FromInt64,
        mk_ty_unboxed_int64(),
        mk_ty_unboxed_float64(),
    );
}

// maps `int64_from_float64` to a builtin function (Float64 -> Int64)
pub fn init_v_var_ty_scheme_builtin_int64_from_float64(
    env_v_var_to_ty_scheme: &mut EnvVVarToTyScheme,
    _ns: &mut TyVarNameSupply,
) {
    env_v_var_to_ty_scheme.insert(
        VVar::Named(VVarName {
            token: concrete_token::ConcreteToken::Iden("int64_from_float64".to_string()),
            loc: None,
            builtin: Some(FnBuiltin::ConvertInt64FromFloat64),
        }),
        TyScheme {
            ty_vars_schematic: Vec::new(),
            // maps to a function type expression: Float64 -> Int64
            ty_expr: Box::new(mk_ty_arrow(mk_ty_float64(), mk_ty_int64())),
        },
    );
}

// maps `float64_from_int64` to a builtin function (Int64 -> Float64)
pub fn init_v_var_ty_scheme_builtin_float64_from_int64(
    env_v_var_to_ty_scheme: &mut EnvVVarToTyScheme,
    _ns: &mut TyVarNameSupply,
) {
    env_v_var_to_ty_scheme.insert(
        VVar::Named(VVarName {
            token: concrete_token::ConcreteToken::Iden("float64_from_int64".to_string()),
            loc: None,
            builtin: Some(FnBuiltin::ConvertFloat64FromInt64),
        }),
        TyScheme {
            ty_vars_schematic: Vec::new(),
            // maps to a function type expression: Int64 -> Float64
            ty_expr: Box::new(mk_ty_arrow(mk_ty_int64(), mk_ty_float64())),
        },
    );
}

pub(crate) fn mk_builtin_var_gt() -> VVar {
    VVar::Named(VVarName {
        token: concrete_token::ConcreteToken::AngleR,
        loc: None,
        builtin: Some(FnBuiltin::BinaryGreater),
    })
}

pub(crate) fn mk_builtin_var_ge() -> VVar {
    VVar::Named(VVarName {
        token: concrete_token::ConcreteToken::GreaterEqual,
        loc: None,
        builtin: Some(FnBuiltin::BinaryGreaterEqual),
    })
}

pub(crate) fn mk_builtin_var_lt() -> VVar {
    VVar::Named(VVarName {
        token: concrete_token::ConcreteToken::AngleL,
        loc: None,
        builtin: Some(FnBuiltin::BinaryLess),
    })
}

pub(crate) fn mk_builtin_var_le() -> VVar {
    VVar::Named(VVarName {
        token: concrete_token::ConcreteToken::LessEqual,
        loc: None,
        builtin: Some(FnBuiltin::BinaryLessEqual),
    })
}

pub(crate) fn mk_builtin_var_eq() -> VVar {
    VVar::Named(VVarName {
        token: concrete_token::ConcreteToken::EqualEqual,
        loc: None,
        builtin: Some(FnBuiltin::BinaryEqual),
    })
}

pub(crate) fn mk_builtin_var_and() -> VVar {
    VVar::Named(VVarName {
        token: concrete_token::ConcreteToken::And,
        loc: None,
        builtin: Some(FnBuiltin::LogicalAnd),
    })
}

pub(crate) fn mk_builtin_typed_vexpr_gt(
    env_v_var_to_ty_scheme: &mut EnvVVarToTyScheme,
    expr1: &TypedVExpr,
    expr2: &TypedVExpr,
) -> TypedVExpr {
    let vvar = mk_builtin_var_gt();
    let callable =
        mk_builtin_typed_vexpr_callable(env_v_var_to_ty_scheme, &vvar, &[expr1.ty().clone()]);
    TypedVExpr::Application(TypedVAppExpr {
        callable: Box::new(callable),
        args: vec![expr1.clone(), expr2.clone()],
        ty: mk_ty_bool(),
    })
}

pub(crate) fn mk_builtin_typed_vexpr_ge(
    env_v_var_to_ty_scheme: &mut EnvVVarToTyScheme,
    expr1: &TypedVExpr,
    expr2: &TypedVExpr,
) -> TypedVExpr {
    let vvar = mk_builtin_var_ge();
    let callable =
        mk_builtin_typed_vexpr_callable(env_v_var_to_ty_scheme, &vvar, &[expr1.ty().clone()]);
    TypedVExpr::Application(TypedVAppExpr {
        callable: Box::new(callable),
        args: vec![expr1.clone(), expr2.clone()],
        ty: mk_ty_bool(),
    })
}

pub(crate) fn mk_builtin_typed_vexpr_lt(
    env_v_var_to_ty_scheme: &mut EnvVVarToTyScheme,
    expr1: &TypedVExpr,
    expr2: &TypedVExpr,
) -> TypedVExpr {
    let vvar = mk_builtin_var_lt();
    let callable =
        mk_builtin_typed_vexpr_callable(env_v_var_to_ty_scheme, &vvar, &[expr1.ty().clone()]);
    TypedVExpr::Application(TypedVAppExpr {
        callable: Box::new(callable),
        args: vec![expr1.clone(), expr2.clone()],
        ty: mk_ty_bool(),
    })
}

pub(crate) fn mk_builtin_typed_vexpr_le(
    env_v_var_to_ty_scheme: &mut EnvVVarToTyScheme,
    expr1: &TypedVExpr,
    expr2: &TypedVExpr,
) -> TypedVExpr {
    let vvar = mk_builtin_var_le();
    let callable =
        mk_builtin_typed_vexpr_callable(env_v_var_to_ty_scheme, &vvar, &[expr1.ty().clone()]);
    TypedVExpr::Application(TypedVAppExpr {
        callable: Box::new(callable),
        args: vec![expr1.clone(), expr2.clone()],
        ty: mk_ty_bool(),
    })
}

pub(crate) fn mk_builtin_typed_vexpr_eq(
    env_v_var_to_ty_scheme: &mut EnvVVarToTyScheme,
    expr1: &TypedVExpr,
    expr2: &TypedVExpr,
) -> TypedVExpr {
    let vvar = mk_builtin_var_eq();
    let callable =
        mk_builtin_typed_vexpr_callable(env_v_var_to_ty_scheme, &vvar, &[expr1.ty().clone()]);
    TypedVExpr::Application(TypedVAppExpr {
        callable: Box::new(callable),
        args: vec![expr1.clone(), expr2.clone()],
        ty: mk_ty_bool(),
    })
}

pub(crate) fn mk_builtin_typed_vexpr_callable(
    env_v_var_to_ty_scheme: &mut EnvVVarToTyScheme,
    vvar: &VVar,
    ty_args: &[TyExpr],
) -> TypedVExpr {
    let ty_scheme = env_v_var_to_ty_scheme.get(vvar).unwrap();
    let ty_callable = ty_scheme.ty_expr.clone();
    let callable = TypedVExpr::Variable(TypedVVariable {
        var: vvar.clone(),
        ty: *ty_callable,
        ty_args: ty_args.iter().cloned().collect(),
        ty_schematic: ty_scheme.clone(),
    });
    callable
}

pub(crate) fn mk_builtin_typed_vexpr_logical_and(
    env_v_var_to_ty_scheme: &mut EnvVVarToTyScheme,
    expr1: &TypedVExpr,
    expr2: &TypedVExpr,
) -> TypedVExpr {
    let var_logical_and = mk_builtin_var_and();

    let ty_scheme = env_v_var_to_ty_scheme.get(&var_logical_and).unwrap();

    let ty_logical_and = ty_scheme.ty_expr.clone();

    let callable = TypedVExpr::Variable(TypedVVariable {
        var: var_logical_and,
        ty: *ty_logical_and,
        ty_args: Vec::new(),
        ty_schematic: ty_scheme.clone(),
    });

    TypedVExpr::Application(TypedVAppExpr {
        callable: Box::new(callable),
        args: vec![expr1.clone(), expr2.clone()],
        ty: mk_ty_bool(),
    })
}
