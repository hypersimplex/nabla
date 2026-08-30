use crate::builtin::types::*;
use crate::parse::abstr_structures::*;
use crate::parse::concrete_token::*;
use crate::parse::loc::*;
use crate::typecheck::ty_env::*;
use crate::typecheck::ty_err::*;
use crate::typecheck::ty_expr::*;
use crate::typecheck::ty_var_name::*;
use crate::typecheck::ty_var_name_supply::*;
use std::collections::*;

/// algebraic data type definition that acts as schema metadata, stored in the
/// type environment and used by the type checker
///
/// this jointly represents a sum type or a record type
///
/// does not appear directly in type expressions
#[derive(Clone, Debug)]
pub(crate) struct ADTDef {
    // type constructor name
    pub name: String,

    // schematic type variables used in constructor field types
    pub ty_params: Vec<TyVarName>,

    // constructors:
    //   1 variant : product type
    //   >=2 variants : sum type
    pub constructors: Vec<ConstructorDef>,
}

impl ADTDef {
    pub fn is_product_type(&self) -> bool {
        self.constructors.len() == 1
    }

    pub fn is_sum_type(&self) -> bool {
        self.constructors.len() > 1
    }
}

/// constructor for a ADT
///
/// `field_names` is present for record type and zips with `field_types`
#[derive(Clone, Debug)]
pub(crate) struct ConstructorDef {
    // constructor name (may match the ADT name for product types)
    pub name: String,

    // positional field types; may reference the ADT's type parameters
    pub field_types: Vec<TyExpr>,

    // optional field names for record-style constructors
    pub field_names: Option<Vec<String>>,
}

#[derive(Debug)]
pub(crate) enum ConstructorRef {
    // reference to a constructor without qualifying type, e.g. `Some`
    Unqualified(String),

    // reference to a constructor qualified by its type, e.g. `Option.Some`
    Qualified {
        ty_name: String,
        constructor: String,
    },
}

#[derive(Clone, Debug)]
struct DataParams {
    name: String,
    param_vars: Vec<TyVarName>,
    param_map: HashMap<String, TyVarName>,
}

impl DataParams {
    fn from_decl(
        identifier: &ConcreteTokenAndLoc,
        params: &[ATypeExprComplex],
    ) -> Result<Self, TyError> {
        let name = get_data_type_name(identifier)?;
        let mut param_vars = Vec::new();
        let mut param_map = HashMap::new();

        for param in params {
            let pname = match param {
                ATypeExprComplex::Iden(iden) => match &iden.identifier.token {
                    ConcreteToken::Iden(s) => s.clone(),
                    other => {
                        return Err(TyError::AdtError(format!(
                            "expected identifier for type parameter, got {:?}",
                            other
                        )));
                    }
                },
                _ => {
                    return Err(TyError::AdtError(
                        "complex type parameter not supported".to_string(),
                    ));
                }
            };
            let tvn = mk_ty_var_name_userdef(&pname);
            param_map.insert(pname, tvn.clone());
            param_vars.push(tvn);
        }

        Ok(Self {
            name,
            param_vars,
            param_map,
        })
    }

    fn from_registered(adt: &ADTDef) -> Result<Self, TyError> {
        let mut param_map = HashMap::new();
        for tvn in adt.ty_params.iter() {
            match tvn {
                TyVarName::UserDefined(ud) => match &ud.token {
                    ConcreteToken::Iden(s) => {
                        param_map.insert(s.clone(), tvn.clone());
                    }
                    other => {
                        return Err(TyError::AdtError(format!(
                            "unexpected token for param {:?}",
                            other
                        )));
                    }
                },
                other => {
                    return Err(TyError::AdtError(format!(
                        "expected user defined param name for registered ADT, found {:?}",
                        other
                    )));
                }
            }
        }

        Ok(Self {
            name: adt.name.clone(),
            param_vars: adt.ty_params.clone(),
            param_map,
        })
    }
}

fn build_record_type_definition(rec: &DataRecord, params: &DataParams) -> Result<ADTDef, TyError> {
    let field_names: Vec<String> = rec
        .components
        .iter()
        .map(|(tok, _)| match &tok.token {
            ConcreteToken::Iden(s) => Ok(s.clone()),
            other => Err(TyError::AdtError(format!(
                "expected identifier for record field, got {:?}",
                other
            ))),
        })
        .collect::<Result<Vec<_>, _>>()?;

    let field_types: Vec<TyExpr> = rec
        .components
        .iter()
        .map(|(_, ty)| ty_expr_from_with_params(ty, &params.param_map))
        .collect::<Result<Vec<_>, _>>()?;

    Ok(ADTDef {
        name: params.name.clone(),
        ty_params: params.param_vars.clone(),
        constructors: vec![ConstructorDef {
            name: params.name.clone(),
            field_types,
            field_names: Some(field_names),
        }],
    })
}

fn build_sum_type_definition(sum: &DataSum, params: &DataParams) -> Result<ADTDef, TyError> {
    let ctors: Vec<ConstructorDef> = sum
        .variants
        .iter()
        .map(|(ctor_tok, args)| {
            let ctor_name = match &ctor_tok.token {
                ConcreteToken::Iden(s) => s.clone(),
                other => {
                    return Err(TyError::AdtError(format!(
                        "expected constructor identifier, got {:?}",
                        other
                    )));
                }
            };
            let field_types = args
                .iter()
                .map(|a| ty_expr_from_with_params(a, &params.param_map))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(ConstructorDef {
                name: ctor_name,
                field_types,
                field_names: None,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;

    Ok(ADTDef {
        name: params.name.clone(),
        ty_params: params.param_vars.clone(),
        constructors: ctors,
    })
}

/// add info about ADT from top level items into the type env
///
/// pass 1 registers ADT names and parameters
///
/// pass 2 fills constructors
pub(crate) fn register_adt_into_type_env(
    ty_var_ns: &mut TyVarNameSupply,
    items: &[TopLevelItem],
) -> Result<TyEnv, TyError> {
    validate_no_builtin_type_shadowing(items)?;
    let mut env = TyEnv::new();

    // pass 1: register skeletons
    for item in items.iter() {
        match item {
            TopLevelItem::DataRecord(rec) => {
                let params = DataParams::from_decl(&rec.identifier, &rec.params)?;
                env.add_adt_skeleton(params.name, params.param_vars);
            }
            TopLevelItem::DataSum(sum) => {
                let params = DataParams::from_decl(&sum.identifier, &sum.params)?;
                env.add_adt_skeleton(params.name, params.param_vars);
            }
            _ => {}
        }
    }

    // pass 2: fill constructors using the registered parameter maps
    for item in items.iter() {
        match item {
            TopLevelItem::DataRecord(rec) => {
                let ty_name = get_data_type_name(&rec.identifier)?;
                let adt = env.get_adt(&ty_name)?;
                let params = DataParams::from_registered(adt)?;
                let ctor = build_record_type_definition(rec, &params)?.constructors;
                env.set_adt_constructors(&ty_name, ctor)?;
            }
            TopLevelItem::DataSum(sum) => {
                let ty_name = get_data_type_name(&sum.identifier)?;
                let adt = env.get_adt(&ty_name)?;
                let params = DataParams::from_registered(adt)?;
                let ctors = build_sum_type_definition(sum, &params)?.constructors;
                env.set_adt_constructors(&ty_name, ctors)?;
            }
            _ => {}
        }
    }

    // TODO: determine if this is the right place to add builtin structures
    // into the env
    //
    // conflicts with user types should be enforced by
    // `validate_no_builtin_type_shadowing`

    env.add_adt(ADTDef {
        name: "Bool".to_string(),
        ty_params: vec![],
        constructors: vec![
            ConstructorDef {
                name: "True".to_string(),
                field_types: vec![],
                field_names: None,
            },
            ConstructorDef {
                name: "False".to_string(),
                field_types: vec![],
                field_names: None,
            },
        ],
    });

    let ty_var_name = ty_var_ns.generate();
    env.add_adt(ADTDef {
        name: "Maybe".to_string(),
        ty_params: vec![ty_var_name.clone()],
        constructors: vec![
            ConstructorDef {
                name: "Nothing".to_string(),
                field_types: vec![],
                field_names: None,
            },
            ConstructorDef {
                name: "Just".to_string(),
                field_types: vec![TyExpr::TyVar(ty_var_name)],
                field_names: None,
            },
        ],
    });

    env.add_adt(ADTDef {
        name: "Unit".to_string(),
        ty_params: vec![],
        constructors: vec![ConstructorDef {
            name: "Unit".to_string(),
            field_types: vec![],
            field_names: None,
        }],
    });

    Ok(env)
}

fn ty_expr_from_with_params(
    ty: &ATypeExprComplex,
    params: &HashMap<String, TyVarName>,
) -> Result<TyExpr, TyError> {
    match ty {
        ATypeExprComplex::Iden(iden) => {
            // head identifier
            let head_name = match &iden.identifier.token {
                ConcreteToken::Iden(s) => s.clone(),
                other => {
                    return Err(TyError::AdtError(format!(
                        "expected identifier in type expression, got {:?}",
                        other
                    )));
                }
            };

            // if it's a schematic type parameter, reuse the param var, or
            // map to known builtins, or
            // treat as user-defined type
            let mut head = if let Some(tvn) = params.get(&head_name) {
                TyExpr::TyVar(tvn.clone())
            } else if let Some(builtin) = resolve_builtin_type(&head_name) {
                builtin
            } else {
                TyExpr::TyVar(mk_ty_var_name_userdef(&head_name))
            };

            // apply any type parameters
            for a in iden.type_parameters.iter() {
                let arg = ty_expr_from_with_params(a, params)?;
                head = ty_app(head, arg);
            }

            Ok(head)
        }
        ATypeExprComplex::Fun(fun) => {
            // convert function types to arrow applications
            let head = {
                let g = fun.head.lock().map_err(|_| {
                    TyError::InternalError("failed to lock function type head".to_string())
                })?;
                ty_expr_from_with_params(&g, params)?
            };
            let tail = match &fun.tail {
                Some(x) => {
                    let guard = x.lock().map_err(|_| {
                        TyError::InternalError("failed to lock function type tail".to_string())
                    })?;
                    ty_expr_from_with_params(&guard, params)?
                }
                None => {
                    return Err(TyError::AdtError(
                        "expected tail in function type expression".to_string(),
                    ));
                }
            };
            Ok(mk_ty_arrow(head, tail))
        }
    }
}

fn get_data_type_name(identifier: &ConcreteTokenAndLoc) -> Result<String, TyError> {
    match &identifier.token {
        ConcreteToken::Iden(s) => Ok(s.clone()),
        other => Err(TyError::AdtError(format!(
            "expected data identifier, got {:?}",
            other
        ))),
    }
}

fn validate_no_builtin_type_shadowing(items: &[TopLevelItem]) -> Result<(), TyError> {
    for item in items.iter() {
        let identifier = match item {
            TopLevelItem::DataRecord(rec) => &rec.identifier,
            TopLevelItem::DataSum(sum) => &sum.identifier,
            _ => continue,
        };
        let name: String = get_data_type_name(identifier)?;
        if resolve_builtin_type(&name).is_some() {
            return Err(TyError::TypeConflict(format!(
                "type name `{name}` conflicts with builtin type at {:?}",
                identifier.loc
            )));
        }
    }
    Ok(())
}
