use crate::typecheck::ty_expr::*;
use crate::typecheck::ty_scheme::*;
use crate::typecheck::ty_var_name::*;

/// capability to query and mutate substitution map
/// for ty_var_name -> ty_expr
pub(crate) trait Subst {
    fn default() -> Self;
    fn new(key: TyVarName, val: TyExpr) -> Self;
    fn new_with(f: impl Fn() -> Self) -> Self
    where
        Self: Sized;
    fn get(&self, key: &TyVarName) -> Option<TyExpr>;
    fn insert(&self, key: TyVarName, val: TyExpr) -> Self;

    /// equivalent to:
    ///   subst2 . subst1
    ///     = \x -> subst_ty(subst2, subst1(x)), if x is in subst1,
    ///     = \x -> subst2(x), if x is not in subst1
    fn compose(subst2: &Self, subst1: &Self) -> Self;
}

pub(crate) fn subst_ty(subst: &impl Subst, ty_expr: &TyExpr) -> TyExpr {
    match ty_expr {
        TyExpr::TyVar(ty_var_name) => subst.get(ty_var_name).unwrap(),
        TyExpr::TyApp(TyApplication { ty_func, ty_arg }) => TyExpr::TyApp(TyApplication {
            ty_func: Box::new(subst_ty(subst, ty_func)),
            ty_arg: Box::new(subst_ty(subst, ty_arg)),
        }),
    }
}

pub(crate) fn subst_ty_scheme(subst: &(impl Subst + Clone), ty_scheme: &TyScheme) -> TyScheme {
    // apply filter on substitution in order to not alter any schematic type variables,
    // before applying the substitution on the type scheme's type expression
    let mut subst_exclude_schematic_ty_vars = subst.clone();
    for i in ty_scheme.ty_vars_schematic.iter() {
        subst_exclude_schematic_ty_vars =
            subst_exclude_schematic_ty_vars.insert(i.clone(), TyExpr::TyVar(i.clone()));
    }
    TyScheme {
        ty_vars_schematic: ty_scheme.ty_vars_schematic.clone(),
        // apply substitution
        ty_expr: Box::new(subst_ty(
            &subst_exclude_schematic_ty_vars,
            &ty_scheme.ty_expr,
        )),
    }
}
