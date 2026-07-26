use super::abstr_structures::*;

use std::collections::*;

fn to_doc(expr: &CaseExpr) -> Box<Doc> {
    use Doc::*;
    let header = cat_space(mk_lit("case"), to_doc(expr.argument.expr));
    header = cat_space(header, mk_lit("of"));
    header = mk_cat(header, mk_line());

    let mut body = mk_nil();
    for clause in expr.clauses.iter() {
        let doc_clause = to_doc(clause);
        body = mk_cat(body, mk_line());
        body = mk_cat(body, doc_clause);
    }
    body = mk_nest(4, body);
    mk_cat(header, body)
}
