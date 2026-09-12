use crate::typecheck::subst::subst_ty;
use crate::typecheck::ty_expr::*;
use crate::typecheck::ty_inference::*;
use crate::typecheck::ty_var_name::*;
use crate::util::printer::*;

/// type scheme, a template used for instantiating a type:
///
/// 'TyScheme' [TyVarName] TyExpr
///
/// within a type scheme's type expression, `ty_expr`, referenced type variables
/// can be either:
///   - schematic (referenced in `ty_vars_schematic`), or
///   - non-schematic
/// where:
///   - schematic => a fresh new type variable is generated at use site
///   - non-schematic => it is copied at use site
///
/// schematic type variables, `ty_vars_schematic`, for a type scheme are
/// analogous to formal parameters in a value-level lambda abstraction
///
/// using the method of looking to variables, during the type checking phase, as
/// more type information is discovered, substitutions are made to non-schematic
/// type variables
///
/// implementation typically saves and manipulates
/// `value_level_variable -> type_scheme` in an environment map for use during
/// the type checking phase
#[derive(Clone, Debug)]
pub(crate) struct TyScheme {
    // when type scheme is instantiated, schematic type variables are
    // instantiated afresh (unconstrained and able to be adjusted to fit with
    // the surrounding context), and non-schematic type variables are copied
    // (constrained)
    pub ty_vars_schematic: Vec<TyVarName>,

    // type expression can use a combination of:
    //   - schematic type variables in `ty_vars_schematic`
    //   - unbound/free type variables (non-schematic type variables)
    pub ty_expr: Box<TyExpr>,
}

impl DocPrinter for TyScheme {
    fn to_doc(&self) -> Box<Doc> {
        let mut doc = Doc::lit("Scheme{[");
        for i in self.ty_vars_schematic.iter() {
            doc = doc.cat(i.to_doc()).cat_lit(",");
        }
        doc.cat_lit("]")
            .cat_space(self.ty_expr.to_doc())
            .cat_lit("}")
    }
}

/// apply substitution for a schematic type variable and remove that type
/// variable from the schematic type variables
///
/// internally errors out if provided type variable is not a schematic type variable
pub(crate) fn apply_ty_scheme(tvn: &TyVarName, ty_expr: &TyExpr, ty_scheme: &TyScheme) -> TyScheme {
    // sanity check
    // [todo]: fix complexity
    if !ty_scheme.ty_vars_schematic.contains(tvn) {
        panic!("schematic type variable does not contain {:?}", tvn);
    }

    // remove from schematic type variables
    let mut ty_vars_schematic_new = ty_scheme.ty_vars_schematic.clone();
    ty_vars_schematic_new.retain(|x| x != tvn);

    let subst = subst_delta(tvn, ty_expr);

    TyScheme {
        ty_vars_schematic: ty_vars_schematic_new,
        // apply substitution
        ty_expr: Box::new(subst_ty(&subst, &ty_scheme.ty_expr)),
    }
}
