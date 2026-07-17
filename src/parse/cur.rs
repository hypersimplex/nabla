use std::io::Cursor;

use std::io::Seek;

#[derive(Clone, Debug)]
pub(crate) struct Cur<'a> {
    cursor: Cursor<&'a [u8]>,
    len_bytes: usize,
    pos_bytes: usize,
    // index wrt. utf8 char:
    pos_linear: usize,
    pos_row: usize,
    pos_column: usize,
}

impl<'a> Cur<'a> {
    pub fn new(content: &'a [u8]) -> Self {
        let s = Self {
            cursor: Cursor::new(content),
            len_bytes: content.len(),
            pos_bytes: 0,
            pos_linear: 0,
            pos_row: 0,
            pos_column: 0,
        };
        //checks for valid utf8
        std::str::from_utf8(s.cursor.get_ref()).ok();
        s
    }
    pub fn forward(&mut self) -> Option<char> {
        let c = self.as_str()[self.pos_bytes..].chars().next();
        match c {
            Some(x) => {
                let n_bytes = x.len_utf8();
                assert!(self.pos_bytes + n_bytes <= self.len_bytes);
                self.pos_bytes += n_bytes;
                self.cursor
                    .seek(std::io::SeekFrom::Current(n_bytes as i64))
                    .expect("out of range seek");
                if x == '\n' {
                    self.update_pos_next_line();
                } else {
                    self.update_pos_same_line_next_char();
                }
                Some(x)
            }
            _ => None,
        }
    }
    pub fn backward(&mut self) -> Option<char> {
        let c = self.as_str()[..self.pos_bytes].chars().next_back();
        match c {
            Some(x) => {
                let n_bytes = x.len_utf8();
                assert!(n_bytes <= self.pos_bytes);
                self.pos_bytes -= n_bytes;
                self.cursor
                    .seek(std::io::SeekFrom::Current(-(n_bytes as i64)))
                    .expect("out of range seek");
                if x == '\n' {
                    self.update_pos_prev_line();
                } else {
                    self.update_pos_same_line_prev_char();
                }
                Some(x)
            }
            _ => None,
        }
    }
    ///go to beginning
    pub fn reset(&mut self) {
        self.cursor.seek(std::io::SeekFrom::Start(0)).ok();
        self.pos_bytes = 0;
        self.pos_linear = 0;
        self.pos_row = 0;
        self.pos_column = 0;
    }
    ///offset from current position
    pub fn step(&mut self, offset: i64) -> i64 {
        let mut ret = 0;
        if offset >= 0 {
            for _ in 0..offset {
                if self.forward().is_some() {
                    ret += 1;
                } else {
                    break;
                }
            }
            ret
        } else {
            for _ in offset..0 {
                if self.backward().is_some() {
                    ret -= 1;
                } else {
                    break;
                }
            }
            ret
        }
    }
    pub fn peek_n(&self, count: i64) -> Vec<char> {
        let mut s = self.clone();
        (0..count.abs())
            .map_while(|_| {
                if count >= 0 {
                    s.forward()
                } else {
                    s.backward()
                }
            })
            .collect()
    }
    pub fn peek_nth(&self, count: i64) -> Option<char> {
        let mut ret = None;
        let mut s = self.clone();
        for _ in 0..count.abs() {
            if count >= 0 {
                match s.forward() {
                    Some(x) => ret = Some(x),
                    _ => return None,
                }
            } else {
                match s.backward() {
                    Some(x) => ret = Some(x),
                    _ => return None,
                }
            }
        }
        ret
    }
    pub fn peek_forward(&self) -> Option<char> {
        self.peek_n(1).into_iter().next()
    }
    pub fn peek_backward(&self) -> Option<char> {
        self.peek_n(-1).into_iter().next()
    }
    pub fn row(&self) -> usize {
        self.pos_row
    }
    pub fn col(&self) -> usize {
        self.pos_column
    }
    pub fn pos_linear(&self) -> usize {
        self.pos_linear
    }
    fn as_str(&self) -> &'a str {
        unsafe { std::str::from_utf8_unchecked(self.cursor.get_ref()) }
    }
    fn update_pos_next_line(&mut self) {
        self.pos_linear += 1;
        self.pos_row += 1;
        self.pos_column = 0;
    }
    fn update_pos_prev_line(&mut self) {
        self.pos_linear -= 1;
        self.pos_row -= 1;
        //count unmbers of chars from prev line
        let mut count = 0;
        let it = self.as_str()[..self.pos_bytes].chars().rev();
        for x in it {
            count += 1;
            if x == '\n' {
                count -= 1;
                break;
            }
        }
        self.pos_column = count;
    }
    fn update_pos_same_line_next_char(&mut self) {
        self.pos_linear += 1;
        self.pos_column += 1;
    }
    fn update_pos_same_line_prev_char(&mut self) {
        self.pos_linear -= 1;
        self.pos_column -= 1;
    }
}

#[test]
fn test_cur() {
    let s = "test string here!";
    let mut cur = Cur::new(s.as_bytes());
    let _s_check = cur.as_str();
    for i in s.chars() {
        assert_eq!(cur.forward(), Some(i));
    }
    assert_eq!(cur.forward(), None);
    for i in s.chars().rev() {
        assert_eq!(cur.backward(), Some(i));
    }
    assert_eq!(vec!['t', 'e', 's'], cur.peek_n(3));
    assert_eq!(4, cur.step(4));
    assert_eq!(vec![' '], cur.peek_n(1));
    assert_eq!(Some(' '), cur.peek_forward());
    assert_eq!(Some('t'), cur.peek_backward());
    assert_eq!(vec!['t', 's', 'e', 't'], cur.peek_n(-4));
    assert_eq!(-4, cur.step(-4));
}

static TEST_CONTENT: &str = r###"
data T B {
  v :: u32,
  x :: B,
}

// Sum constructor
data T2 A = T20 T A
              | T21 u32 
              | T22 u32 i32
              | T23 A A
              | T23 T3
"###;

#[test]
fn test_position_updates() {
    let mut cur = Cur::new(TEST_CONTENT.as_bytes());
    assert_eq!(0, cur.row());
    assert_eq!(0, cur.col());
    assert_eq!(Some('\n'), cur.forward());
    assert_eq!(1, cur.row());
    assert_eq!(0, cur.col());
    for _i in 0..10 {
        assert!(cur.forward().is_some());
    }
    assert_eq!(1, cur.row());
    assert_eq!(10, cur.col());
    assert_eq!(Some('\n'), cur.forward());
    assert_eq!(2, cur.row());
    assert_eq!(0, cur.col());
    assert_eq!(Some('\n'), cur.backward());
    assert_eq!(1, cur.row());
    assert_eq!(10, cur.col());
    cur.reset();
    assert_eq!(0, cur.row());
    assert_eq!(0, cur.col());
    assert_eq!(Some('S'), cur.peek_nth(41));
    assert_eq!(41, cur.step(41));
    assert_eq!(6, cur.row());
    assert_eq!(4, cur.col());
    while cur.forward().is_some() {}
    assert_eq!(12, cur.row());
    assert_eq!(0, cur.col());
    assert_eq!(cur.pos_linear(), TEST_CONTENT.len());
}
