use crate::core::core_ir::*;
use crate::parse::concrete_token::*;
use crate::typecheck::adt::*;
use crate::typecheck::ty_var_name::*;

/// this provides a blueprint for instantiations of ADT
/// and when that happens, type level application happens
/// with type parameters if present
///
/// [todo]: reuse code in typecheck/adt.rs ?
#[derive(Clone, Debug)]
pub(crate) struct CoreADTDef {
    // type constructor name
    pub name: String,

    // schematic type variables / placeholder types that can be present in
    // a constructor field type
    //
    // when ADT is instantiated with constructor function, perform type level
    // application to provide actual types for these place holder types
    pub ty_params: Vec<TyVarName>,

    // constructors:
    //   1 variant : product type
    //   >=2 variants : sum type
    pub constructors: Vec<CoreConDef>,
}

impl CoreADTDef {
    /// return the type of the ADT that may contain generic/placeholder
    /// types in its type expression
    ///
    /// we use type application if there are any type parameters
    ///
    /// this is the final type being returned by each construction function
    /// of the ADT
    ///
    /// note: constructor function is responsible for introducing explicit type
    /// parameters with `ForAll` for each of the generic/placeholder types
    ///
    /// eg: Maybe would be `(TyApp Maybe a)` where `a` is generic
    pub(crate) fn ty(&self) -> CoreTy {
        let core_ty_constructor = CoreTy::TyConstructor(CoreTyCon::User(CoreTyConUser {
            name: self.name.clone(),
        }));
        let mut core_ty = core_ty_constructor;
        for ty_param in self.ty_params.iter() {
            core_ty = CoreTy::App(CoreTyApp {
                ty_fun: Box::new(core_ty),
                ty_arg: Box::new(CoreTy::Var(ty_param.clone())),
            });
        }
        core_ty
    }
}

/// this represents a constructor for a ADT
///
/// note: we remove fields (as present in a record) and encode
/// using position as in a product type
///
/// [todo]: reuse code in typecheck/adt.rs ?
#[derive(Clone, Debug)]
pub(crate) struct CoreConDef {
    // constructor name (may match the ADT name for product types)
    pub name: String,

    // positional field types
    //
    // may reference generic types in `ty_params` in CoreADTDef
    pub field_types: Vec<CoreTy>,
}

/// returns core IR type for a constructor function by possibly introducing
/// `ForAll` for each type parameter
///
/// eg: `Just` constructor function has the type `ForAll a. a -> Maybe a`
///     `None` constructor function has the type `ForAll a. Maybe a`
pub(crate) fn get_ty_for_adt_constructor_fn(
    adt_def: &CoreADTDef,
    constructor_def: &CoreConDef,
) -> CoreTy {
    // final return type is the ADT type
    let mut core_ty = adt_def.ty();

    // make the arrow type for the constructor function based on the fields
    // note: process in reverse order
    for field_ty in constructor_def.field_types.iter().rev() {
        let app = CoreTy::App(CoreTyApp {
            ty_fun: Box::new(CoreTy::TyConstructor(CoreTyCon::Builtin(
                CoreTyConBuiltin::Arrow,
            ))),
            ty_arg: Box::new(field_ty.clone()),
        });

        core_ty = CoreTy::App(CoreTyApp {
            ty_fun: Box::new(app),
            ty_arg: Box::new(core_ty),
        });
    }

    // introduce `ForAll`s for each generic/placeholder type of the ADT
    //
    // note: left most `ty_param` correspond to outermost `ForAll`
    for ty_param in adt_def.ty_params.iter().rev() {
        core_ty = CoreTy::ForAll(CoreTyForAll {
            ty_var: ty_param.clone(),
            ty_expr: Box::new(core_ty),
        });
    }

    core_ty
}

pub(crate) fn get_name_for_adt_constructor_fn(
    adt_def: &CoreADTDef,
    constructor_def: &CoreConDef,
) -> String {
    format!("{}.{}", adt_def.name, constructor_def.name)
}

pub(crate) fn get_vvar_for_adt_constructor_fn(
    adt_def: &CoreADTDef,
    constructor_def: &CoreConDef,
) -> CoreExpr {
    let constructor_name = get_name_for_adt_constructor_fn(adt_def, constructor_def);
    let constructor_ty = get_ty_for_adt_constructor_fn(adt_def, constructor_def);

    use crate::typecheck::v_expr::VVar;
    use crate::typecheck::v_var_name::VVarName;

    // [todo]: get proper span from source instead of constructing this thing
    let vvar = VVar::Named(VVarName {
        token: ConcreteToken::Iden(constructor_name),
        loc: None,
        builtin: None,
    });

    CoreExpr::Variable(CoreVar::ValueVariable(CoreVVar {
        vvar,
        ty: constructor_ty,
    }))
}

// helper conversion functions --->>
impl CoreConDef {
    pub(crate) fn from_constructor_def(
        core_ty_con_env: &crate::core::core_ty_con_env::CoreTyConEnv,
        constructor_def: &ConstructorDef,
    ) -> Self {
        let ConstructorDef {
            name, field_types, ..
        } = constructor_def;
        CoreConDef {
            name: name.clone(),
            field_types: field_types
                .iter()
                .map(|x| core_ty_from_ty_expr(core_ty_con_env, x))
                .collect(),
        }
    }
}

// <<--- helper conversion functions
