use super::abstr_structures::*;
use crate::util::printer::Doc;

use std::collections::*;

fn to_doc(expr: &CaseExpr) -> Box<Doc> {
    let header = Doc::lit("case")
        .cat_space(to_doc(expr.argument.expr))
        .cat_space_lit("of")
        .cat_line();

    let mut body = Doc::nil();
    for clause in expr.clauses.iter() {
        let doc_clause = to_doc(clause);
        body = body.cat_line().cat(doc_clause);
    }
    header.cat(body.nest(4))
}
