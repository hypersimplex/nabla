use std::collections::HashMap;

use crate::builtin::types;
use crate::typecheck::adt::*;
use crate::typecheck::ty_err::*;
use crate::typecheck::ty_expr;
use crate::typecheck::ty_var_name::*;

pub struct TyConEnv {
    adts: HashMap<String, ADTDef>,
}

/// fully resolved constructor lookup result with attached ADT metadata
#[derive(Debug)]
pub(crate) struct ResolvedConstructor<'a> {
    pub ty_name: String,
    pub constructor: String,
    pub adt: &'a ADTDef,
    pub ctor: &'a ConstructorDef,
}

impl TyConEnv {
    pub fn new() -> Self {
        let mut env = TyConEnv {
            adts: HashMap::new(),
        };
        init_builtin_adts(&mut env);
        env
    }

    pub fn resolve_constructor(
        &self,
        ctor_ref: &ConstructorRef,
    ) -> Result<ResolvedConstructor<'_>, TyError> {
        match ctor_ref {
            ConstructorRef::Qualified {
                ty_name,
                constructor,
            } => {
                let adt = self
                    .adts
                    .get(ty_name)
                    .ok_or_else(|| TyError::UnknownType(ty_name.clone()))?;

                let ctor = adt
                    .constructors
                    .iter()
                    .find(|c| c.name == *constructor)
                    .ok_or_else(|| TyError::UnknownConstructor {
                        ty_name: Some(ty_name.clone()),
                        constructor: constructor.clone(),
                    })?;

                Ok(ResolvedConstructor {
                    ty_name: ty_name.clone(),
                    constructor: constructor.clone(),
                    adt,
                    ctor,
                })
            }

            ConstructorRef::Unqualified(constructor) => {
                let matches: Vec<(&str, &ADTDef, &ConstructorDef)> = self
                    .adts
                    .iter()
                    .filter_map(|(name, adt)| {
                        adt.constructors
                            .iter()
                            .find(|c| c.name == *constructor)
                            .map(|c| (name.as_str(), adt, c))
                    })
                    .collect();

                match matches.len() {
                    0 => Err(TyError::UnknownConstructor {
                        ty_name: None,
                        constructor: constructor.clone(),
                    }),
                    1 => {
                        let (ty_name, adt, ctor) = matches[0];
                        Ok(ResolvedConstructor {
                            ty_name: ty_name.to_string(),
                            constructor: constructor.clone(),
                            adt,
                            ctor,
                        })
                    }
                    _ => Err(TyError::AmbiguousConstructor {
                        constructor: constructor.clone(),
                        candidates: matches.iter().map(|(t, _, _)| t.to_string()).collect(),
                    }),
                }
            }
        }
    }

    pub fn get_adt(&self, name: &str) -> Result<&ADTDef, TyError> {
        self.adts
            .get(name)
            .ok_or_else(|| TyError::UnknownType(name.to_string()))
    }

    pub fn has_adt(&self, name: &str) -> bool {
        self.adts.contains_key(name)
    }

    pub fn add_adt(&mut self, adt: ADTDef) {
        self.adts.insert(adt.name.clone(), adt);
    }

    pub fn iter_adts(&self) -> impl Iterator<Item = &ADTDef> {
        self.adts.values()
    }

    /// check whether a type name corresponds to a registered builtin ADT
    pub fn is_builtin_adt(&self, name: &str) -> bool {
        self.adts.get(name).map_or(false, |adt| adt.is_builtin())
    }

    /// check whether a type name is reserved (primitive or builtin ADT)
    pub fn is_reserved_type_name(&self, name: &str) -> bool {
        types::resolve_builtin_ty_con(name).is_some() || self.is_builtin_adt(name)
    }

    /// validate a type constructor by name and arity, returning the resolved `TyCon`
    pub fn validate_and_resolve_ty_con(
        &self,
        name: &str,
        num_args: usize,
    ) -> Result<ty_expr::TyCon, TyError> {
        if let Some(builtin) = types::resolve_builtin_ty_con(name) {
            if num_args != builtin.arity() {
                return Err(TyError::ArityMismatch {
                    constructor: builtin.name().to_string(),
                    expected: builtin.arity(),
                    got: num_args,
                    msg: Some(format!(
                        "builtin type `{}` takes {} arguments, got {num_args}",
                        builtin.name(),
                        builtin.arity()
                    )),
                });
            }
            Ok(ty_expr::TyCon::Builtin(builtin))
        } else if let Ok(adt) = self.get_adt(name) {
            let expected = adt.ty_params.len();
            if num_args != expected {
                return Err(TyError::ArityMismatch {
                    constructor: name.to_string(),
                    expected,
                    got: num_args,
                    msg: Some(format!(
                        "type constructor `{name}` takes {expected} arguments, got {num_args}"
                    )),
                });
            }
            Ok(ty_expr::TyCon::User(name.to_string()))
        } else {
            Err(TyError::UnknownType(name.to_string()))
        }
    }

    // register an ADT skeleton (name + parameters) without constructors
    pub fn add_adt_skeleton(&mut self, name: String, ty_params: Vec<TyVarName>) {
        self.adts
            .insert(name.clone(), ADTDef::new_user(name, ty_params, Vec::new()));
    }

    // overwrite constructors for an existing ADT (used by the second pass)
    pub(crate) fn set_adt_constructors(
        &mut self,
        name: &str,
        ctors: Vec<ConstructorDef>,
    ) -> Result<(), TyError> {
        if let Some(adt) = self.adts.get_mut(name) {
            adt.constructors = ctors;
            Ok(())
        } else {
            Err(TyError::UnknownType(name.to_string()))
        }
    }
}
