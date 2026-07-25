use std::cmp::Ordering;
use std::fmt;
use std::path;
use std::sync::Arc;
use std::sync::Mutex;

use super::concrete_token::*;

#[derive(Clone, Copy, PartialOrd, Ord, PartialEq, Eq)]
pub(crate) struct Span {
    //wrt. utf8 char indices
    pub linear: usize,
    pub line: usize,
    pub col: usize,
}

impl Span {
    pub(crate) fn new(linear: usize, line: usize, col: usize) -> Self {
        Self { linear, line, col }
    }
}

//info about location in source code
#[derive(Clone)]
pub(crate) struct Location {
    pub file: Arc<Mutex<std::path::PathBuf>>,
    pub span_start: Span,
    pub span_end: Span,
}

impl PartialEq for Location {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Ord for Location {
    fn cmp(&self, other: &Self) -> Ordering {
        // avoid deadlock by not holding multiple file locks concurrently; clone paths first
        let self_path = { self.file.lock().unwrap().clone() };
        let other_path = { other.file.lock().unwrap().clone() };
        self_path
            .as_path()
            .cmp(other_path.as_path())
            .then(self.span_start.cmp(&other.span_start))
            .then(self.span_end.cmp(&other.span_end))
    }
}

impl PartialOrd for Location {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Eq for Location {}

impl fmt::Debug for Location {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let guard = self.file.lock().unwrap();
        let p = guard.as_path();
        f.debug_struct("Location")
            .field("file", &p)
            .field("span_start", &self.span_start)
            .field("span_end", &self.span_end)
            .finish()
    }
}

impl Location {
    pub fn new(path: &std::path::Path) -> Self {
        Self {
            file: std::sync::Arc::new(std::sync::Mutex::new(path::PathBuf::from(path))),
            span_start: Span::new(0, 0, 0),
            span_end: Span::new(0, 0, 0),
        }
    }

    // For testing purposes
    pub fn dummy() -> Self {
        Self::new(std::path::Path::new("test.sx"))
    }

    /// helper to create a 0-width virtual location
    pub fn to_zero_width_start(&self) -> Self {
        let mut ret = self.clone();
        ret.span_end = ret.span_start;
        ret
    }
    /// helper to create a 0-width virtual location
    pub fn to_zero_width_end(&self) -> Self {
        let mut ret = self.clone();
        ret.span_start = ret.span_end;
        ret
    }
}

#[derive(Clone, Debug)]
pub(crate) struct ConcreteTokenAndLoc {
    pub token: ConcreteToken,
    pub loc: Location,
    pub starts_a_line: bool, // TODO: factor out this field, and rid of many use sites
}

#[derive(Clone)]
pub(crate) struct LexedTokensAndLocs(pub Vec<ConcreteTokenAndLoc>);

#[derive(Clone, Copy, Debug)]
pub(crate) enum Indent {
    PrevLvl(usize),
    CurLvl(usize),
}

pub(crate) fn update_indent(indent: Indent, current: Indent) -> Result<Indent, String> {
    match (indent, current) {
        (Indent::PrevLvl(prev), Indent::CurLvl(cur)) => {
            if cur <= prev {
                return Err(format!(
                    "current indent {} <= PrevLvl indent: {}",
                    cur, prev
                ));
            }
            Ok(Indent::CurLvl(cur))
        }
        (Indent::CurLvl(x), Indent::CurLvl(cur)) => {
            if cur < x {
                return Err(format!("current indent {} < CurLvl indent: {}", cur, x));
            }
            Ok(Indent::CurLvl(x))
        }
        (x, y) => Err(format!("unexpected indent types found: {:?}, {:?}", x, y)),
    }
}

pub(crate) fn update_indent_enclosing_delimiter(
    indent: Indent,
    current: Indent,
) -> Result<Indent, String> {
    match (indent, current) {
        (Indent::PrevLvl(prev), Indent::CurLvl(cur)) => {
            if cur < prev {
                return Err(format!(
                    "current indent {} <= PrevLvl indent: {}",
                    cur, prev
                ));
            }
            Ok(Indent::CurLvl(cur))
        }
        (Indent::CurLvl(x), Indent::CurLvl(cur)) => {
            if cur < x {
                return Err(format!("current indent {} < CurLvl indent: {}", cur, x));
            }
            Ok(Indent::CurLvl(x))
        }
        (x, y) => Err(format!("unexpected indent types found: {:?}, {:?}", x, y)),
    }
}

pub(crate) fn align_indent(indent: Indent, current: Indent) -> Result<Indent, String> {
    match (indent, current) {
        (Indent::PrevLvl(prev), Indent::CurLvl(cur)) => {
            if cur <= prev {
                return Err(format!(
                    "current indent {} <= PrevLvl indent: {}",
                    cur, prev
                ));
            }
            Ok(Indent::CurLvl(cur))
        }
        (Indent::CurLvl(x), Indent::CurLvl(cur)) => {
            if cur != x {
                return Err(format!(
                    "expect current indent {} to align with indent: {}",
                    cur, x
                ));
            }
            Ok(Indent::CurLvl(x))
        }
        (x, y) => Err(format!("unexpected indent types found: {:?}, {:?}", x, y)),
    }
}

impl fmt::Display for LexedTokensAndLocs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for i in &self.0 {
            write!(f, "{}", i.token)?
        }
        Ok(())
    }
}

impl fmt::Debug for LexedTokensAndLocs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for i in &self.0 {
            write!(
                f,
                "{:?}(span:[{:?}, {:?})) ",
                i.token, i.loc.span_start, i.loc.span_end
            )?
        }
        Ok(())
    }
}

impl fmt::Debug for Span {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.line, self.col)
    }
}
