use crate::typecheck::ty_expr::*;
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
        let mut doc = mk_nil();
        doc = mk_cat(doc, mk_lit("Scheme{["));
        for i in self.ty_vars_schematic.iter() {
            doc = mk_cat(doc, i.to_doc());
            doc = mk_cat(doc, mk_lit(","));
        }
        doc = mk_cat(doc, mk_lit("]"));
        doc = cat_space(doc, self.ty_expr.to_doc());
        doc = mk_cat(doc, mk_lit("}"));
        doc
    }
}
