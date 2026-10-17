//! Reading a circuit file as PyYAML's `safe_load` does, which is how bbgen always read them: libyaml's
//! grammar (YAML 1.1: `rail: -}` is a plain "-") and SafeLoader's rules for what a plain scalar is (yes/no/on/off
//! are booleans, 010 is octal, 1_000 is a thousand, 1:30 is ninety, ~ and null and nothing are None). A key
//! given twice keeps its first place and its last value.

use crate::val::Val;
use std::mem::MaybeUninit;
use unsafe_libyaml::*;

pub fn load(text: &str) -> Result<Val, String> {
    load_lines(text).map(|(v, _)| v)
}

/// Where things are in a circuit file: the line (from 1) of each key and list item down to three levels,
/// by its path (`["parts", "U1"]`, `["wires", "power", "2"]`; a key as Python's `str()` writes it).
#[derive(Clone, Debug, Default)]
pub struct Lines(Vec<(Vec<String>, usize)>);

const DEPTH: usize = 3;

/// The most values a file's aliases may add, all told. Far more than any circuit needs, but each alias copies what its
/// anchor holds, so a few lines of aliases to aliases (a "billion laughs") would otherwise fill memory.
const ALIASED: usize = 100_000;

/// How many values `v` is: itself and everything in it.
fn size(v: &Val) -> usize {
    match v {
        Val::List(items) => 1 + items.iter().map(size).sum::<usize>(),
        Val::Map(items) => 1 + items.iter().map(|(k, v)| size(k) + size(v)).sum::<usize>(),
        _ => 1,
    }
}

impl Lines {
    pub fn get(&self, path: &[&str]) -> Option<usize> {
        self.0.iter().rev().find(|(p, _)| p.iter().map(String::as_str).eq(path.iter().copied())).map(|&(_, l)| l)
    }

    /// The line a message is about, when it starts with a path as bbgen writes them: `parts.U1: ...`,
    /// `nets.GND: ...`, `wires.power[2]: ...` (`wires.wires[2]` when the wires are one list).
    pub fn of_message(&self, message: &str) -> Option<usize> {
        let (head, _) = message.split_once(": ")?;
        let (section, rest) = head.split_once('.')?;
        match section {
            "parts" | "nets" => self.get(&[section, rest]),
            "wires" => {
                let (group, i) = rest.strip_suffix(']')?.rsplit_once('[')?;
                self.get(&["wires", group, i]).or_else(|| if group == "wires" { self.get(&["wires", i]) } else { None })
            }
            _ => None,
        }
    }
}

/// A circuit file's values, and where each is in it.
pub fn load_lines(text: &str) -> Result<(Val, Lines), String> {
    unsafe {
        let mut parser = MaybeUninit::<yaml_parser_t>::uninit();
        if yaml_parser_initialize(parser.as_mut_ptr()).fail {
            return Err("couldn't start the YAML parser".into());
        }
        let parser = parser.as_mut_ptr();
        yaml_parser_set_input_string(parser, text.as_ptr(), text.len() as _);
        let mut b = Builder { stack: vec![], anchors: vec![], aliased: 0, doc: None, docs: 0, lines: Lines::default() };
        let out = loop {
            let mut event = MaybeUninit::<yaml_event_t>::uninit();
            if yaml_parser_parse(parser, event.as_mut_ptr()).fail {
                let p = &*parser;
                let problem = if p.problem.is_null() { "invalid YAML".to_string() } else {
                    std::ffi::CStr::from_ptr(p.problem).to_string_lossy().into_owned()
                };
                break Err(format!("{} (line {}, column {})", problem, p.problem_mark.line + 1, p.problem_mark.column + 1));
            }
            let mut ev = event.assume_init();
            let done = ev.type_ == YAML_STREAM_END_EVENT;
            let r = b.event(&ev);
            yaml_event_delete(&mut ev);
            if let Err(e) = r {
                break Err(e);
            }
            if done {
                break Ok((b.doc.take().unwrap_or(Val::Null), std::mem::take(&mut b.lines)));
            }
        };
        yaml_parser_delete(parser);
        out
    }
}

/// A sequence or mapping being read: its items, its key waiting for a value, its anchor, and its first line.
enum Frame {
    Seq(Vec<Val>, Option<String>, usize),
    Map(Vec<(Val, Val)>, Option<Val>, Option<String>, usize),
}

struct Builder {
    stack: Vec<Frame>,
    /// each anchor's name, value and size
    anchors: Vec<(String, Val, usize)>,
    /// the values aliases have added so far
    aliased: usize,
    doc: Option<Val>,
    docs: usize,
    lines: Lines,
}

unsafe fn cstr(p: *const u8) -> Option<String> {
    if p.is_null() {
        None
    } else {
        Some(std::ffi::CStr::from_ptr(p as *const std::ffi::c_char).to_string_lossy().into_owned())
    }
}

impl Builder {
    unsafe fn event(&mut self, ev: &yaml_event_t) -> Result<(), String> {
        let line = ev.start_mark.line as usize + 1;
        match ev.type_ {
            YAML_DOCUMENT_START_EVENT => {
                self.docs += 1;
                if self.docs > 1 {
                    return Err("expected a single document in the stream".into());
                }
            }
            YAML_SCALAR_EVENT => {
                let s = ev.data.scalar;
                let bytes = std::slice::from_raw_parts(s.value, s.length as usize);
                let value = String::from_utf8_lossy(bytes).into_owned();
                let tag = cstr(s.tag);
                let v = if let Some(tag) = tag.filter(|t| t != "!") {
                    tagged(&tag, &value)?
                } else if s.style == YAML_PLAIN_SCALAR_STYLE {
                    resolve(&value)
                } else {
                    Val::Str(value)
                };
                if let Some(a) = cstr(s.anchor) {
                    self.anchors.push((a, v.clone(), 1));
                }
                self.add(v, line)?;
            }
            YAML_ALIAS_EVENT => {
                let name = cstr(ev.data.alias.anchor).unwrap_or_default();
                let (_, v, n) = self.anchors.iter().rev().find(|(a, ..)| *a == name)
                    .ok_or(format!("found undefined alias {}", name))?;
                self.aliased += n;
                if self.aliased > ALIASED {
                    return Err(format!("the aliases add more than {} values (line {}, column {})", ALIASED, line,
                                       ev.start_mark.column + 1));
                }
                let v = v.clone();
                self.add(v, line)?;
            }
            YAML_SEQUENCE_START_EVENT => self.stack.push(Frame::Seq(vec![], cstr(ev.data.sequence_start.anchor), line)),
            YAML_MAPPING_START_EVENT => self.stack.push(Frame::Map(vec![], None, cstr(ev.data.mapping_start.anchor), line)),
            YAML_SEQUENCE_END_EVENT | YAML_MAPPING_END_EVENT => {
                let (v, anchor, line) = match self.stack.pop().unwrap() {
                    Frame::Seq(items, a, l) => (Val::List(items), a, l),
                    Frame::Map(items, _, a, l) => (Val::Map(items), a, l),
                };
                if let Some(a) = anchor {
                    self.anchors.push((a, v.clone(), size(&v)));
                }
                self.add(v, line)?;
            }
            _ => {}
        }
        Ok(())
    }

    /// Where a key or list item starting on `line` is: the path to it, if it's no deeper than `DEPTH`.
    fn note(&mut self, line: usize, last: String) {
        if self.stack.len() > DEPTH {
            return;
        }
        let mut path: Vec<String> = self.stack[..self.stack.len() - 1].iter().map(|f| match f {
            Frame::Seq(items, ..) => items.len().to_string(),
            Frame::Map(_, key, ..) => key.as_ref().map(Val::py_str).unwrap_or_default(),
        }).collect();
        path.push(last);
        self.lines.0.push((path, line));
    }

    fn add(&mut self, v: Val, line: usize) -> Result<(), String> {
        match self.stack.last() {
            Some(Frame::Seq(items, ..)) => self.note(line, items.len().to_string()),
            Some(Frame::Map(_, None, ..)) => self.note(line, v.py_str()),
            _ => {}
        }
        match self.stack.last_mut() {
            None => self.doc = Some(v),
            Some(Frame::Seq(items, ..)) => items.push(v),
            Some(Frame::Map(items, key, ..)) => match key.take() {
                None => {
                    if matches!(v, Val::List(_) | Val::Map(_)) {
                        return Err("found unhashable key".into());
                    }
                    *key = Some(v);
                }
                Some(k) => match items.iter_mut().find(|(k2, _)| same_key(k2, &k)) {
                    Some(slot) => slot.1 = v,
                    None => items.push((k, v)),
                },
            },
        }
        Ok(())
    }
}

/// Dict keys the same to Python: 1, 1.0 and True are one key.
fn same_key(a: &Val, b: &Val) -> bool {
    let num = |v: &Val| match v {
        Val::Int(i) => Some(*i as f64),
        Val::Bool(b) => Some(*b as i64 as f64),
        Val::Float(f) => Some(*f),
        _ => None,
    };
    match (num(a), num(b)) {
        (Some(x), Some(y)) => x == y,
        _ => a == b,
    }
}

fn tagged(tag: &str, value: &str) -> Result<Val, String> {
    let t = tag.strip_prefix("tag:yaml.org,2002:").unwrap_or(tag.trim_start_matches('!'));
    Ok(match t {
        "str" => Val::Str(value.into()),
        "int" => int(value).ok_or(format!("invalid int {:?}", value))?,
        "float" => Val::Float(float(value).ok_or(format!("invalid float {:?}", value))?),
        "bool" => match value.to_lowercase().as_str() {
            "yes" | "true" | "on" => Val::Bool(true),
            "no" | "false" | "off" => Val::Bool(false),
            _ => return Err(format!("invalid bool {:?}", value)),
        },
        "null" => Val::Null,
        _ => Val::Str(value.into()),
    })
}

/// SafeLoader's implicit resolvers, in the order it tries them: bool, float, int, null (else a string).
pub fn resolve(v: &str) -> Val {
    if is_bool(v) {
        return Val::Bool(matches!(v, "yes" | "Yes" | "YES" | "true" | "True" | "TRUE" | "on" | "On" | "ON"));
    }
    if is_float(v) {
        if let Some(f) = float(v) {
            return Val::Float(f);
        }
    }
    if is_int(v) {
        if let Some(i) = int(v) {
            return i;
        }
    }
    if matches!(v, "~" | "null" | "Null" | "NULL" | "") {
        return Val::Null;
    }
    Val::Str(v.into())
}

fn is_bool(v: &str) -> bool {
    matches!(v, "yes" | "Yes" | "YES" | "no" | "No" | "NO" | "true" | "True" | "TRUE" | "false" | "False" | "FALSE"
        | "on" | "On" | "ON" | "off" | "Off" | "OFF")
}

fn digits(s: &str, ok: impl Fn(char) -> bool) -> usize {
    s.chars().take_while(|&c| ok(c)).count()
}

/// [-+]?(?:[0-9][0-9_]*)\.[0-9_]*(?:[eE][-+][0-9]+)? | \.[0-9][0-9_]*(?:[eE][-+][0-9]+)?
/// | [-+]?[0-9][0-9_]*(?::[0-5]?[0-9])+\.[0-9_]* | [-+]?\.(?:inf|Inf|INF) | \.(?:nan|NaN|NAN)
fn is_float(v: &str) -> bool {
    if matches!(v, ".nan" | ".NaN" | ".NAN") {
        return true;
    }
    let unsigned = v.strip_prefix(['-', '+']).unwrap_or(v);
    if matches!(unsigned, ".inf" | ".Inf" | ".INF") {
        return true;
    }
    let exp_ok = |rest: &str| -> bool {
        // (?:[eE][-+][0-9]+)?$
        if rest.is_empty() {
            return true;
        }
        let r = match rest.strip_prefix(['e', 'E']) {
            Some(r) => r,
            None => return false,
        };
        let r = match r.strip_prefix(['-', '+']) {
            Some(r) => r,
            None => return false,
        };
        !r.is_empty() && r.chars().all(|c| c.is_ascii_digit())
    };
    // \.[0-9][0-9_]*(exp)?  (no sign)
    if let Some(r) = v.strip_prefix('.') {
        if r.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false) {
            let n = digits(r, |c| c.is_ascii_digit() || c == '_');
            if exp_ok(&r[n..]) {
                return true;
            }
        }
    }
    let s = unsigned;
    if !s.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false) {
        return false;
    }
    let n = digits(s, |c| c.is_ascii_digit() || c == '_');
    let rest = &s[n..];
    // [0-9][0-9_]*\.[0-9_]*(exp)?
    if let Some(r) = rest.strip_prefix('.') {
        let m = digits(r, |c| c.is_ascii_digit() || c == '_');
        if exp_ok(&r[m..]) {
            return true;
        }
    }
    // [0-9][0-9_]*(?::[0-5]?[0-9])+\.[0-9_]*
    let mut r = rest;
    let mut groups = 0;
    while let Some(after) = r.strip_prefix(':') {
        let m = digits(after, |c| c.is_ascii_digit());
        if m == 1 || (m == 2 && after.as_bytes()[0] <= b'5') {
            r = &after[m..];
            groups += 1;
        } else {
            return false;
        }
    }
    if groups > 0 {
        if let Some(after) = r.strip_prefix('.') {
            return after.chars().all(|c| c.is_ascii_digit() || c == '_');
        }
    }
    false
}

/// [-+]?0b[0-1_]+ | [-+]?0[0-7_]+ | [-+]?(?:0|[1-9][0-9_]*) | [-+]?0x[0-9a-fA-F_]+ | [-+]?[1-9][0-9_]*(?::[0-5]?[0-9])+
fn is_int(v: &str) -> bool {
    let s = v.strip_prefix(['-', '+']).unwrap_or(v);
    if s.is_empty() {
        return false;
    }
    if let Some(r) = s.strip_prefix("0b") {
        return !r.is_empty() && r.chars().all(|c| c == '0' || c == '1' || c == '_');
    }
    if let Some(r) = s.strip_prefix("0x") {
        return !r.is_empty() && r.chars().all(|c| c.is_ascii_hexdigit() || c == '_');
    }
    if s == "0" {
        return true;
    }
    if let Some(r) = s.strip_prefix('0') {
        return !r.is_empty() && r.chars().all(|c| ('0'..='7').contains(&c) || c == '_');
    }
    if !s.chars().next().map(|c| ('1'..='9').contains(&c)).unwrap_or(false) {
        return false;
    }
    let n = digits(s, |c| c.is_ascii_digit() || c == '_');
    let mut r = &s[n..];
    if r.is_empty() {
        return true;
    }
    while let Some(after) = r.strip_prefix(':') {
        let m = digits(after, |c| c.is_ascii_digit());
        if m == 1 || (m == 2 && after.as_bytes()[0] <= b'5') {
            r = &after[m..];
        } else {
            return false;
        }
    }
    r.is_empty()
}

/// construct_yaml_int
fn int(v: &str) -> Option<Val> {
    let v = v.replace('_', "");
    let (sign, v) = match v.strip_prefix('-') {
        Some(r) => (-1i64, r.to_string()),
        None => (1, v.strip_prefix('+').unwrap_or(&v).to_string()),
    };
    let n: i64 = if v == "0" {
        0
    } else if let Some(r) = v.strip_prefix("0b") {
        i64::from_str_radix(r, 2).ok()?
    } else if let Some(r) = v.strip_prefix("0x") {
        i64::from_str_radix(r, 16).ok()?
    } else if v.starts_with('0') {
        i64::from_str_radix(&v, 8).ok()?
    } else if v.contains(':') {
        let mut base = 1i64;
        let mut total = 0i64;
        for part in v.split(':').rev() {
            total += part.parse::<i64>().ok()? * base;
            base *= 60;
        }
        total
    } else {
        v.parse().ok()?
    };
    Some(Val::Int(sign * n))
}

/// construct_yaml_float
fn float(v: &str) -> Option<f64> {
    let v = v.replace('_', "").to_lowercase();
    let (sign, v) = match v.strip_prefix('-') {
        Some(r) => (-1.0f64, r.to_string()),
        None => (1.0, v.strip_prefix('+').unwrap_or(&v).to_string()),
    };
    if v == ".inf" {
        return Some(sign * f64::INFINITY);
    }
    if v == ".nan" {
        return Some(f64::NAN);
    }
    if v.contains(':') {
        let mut base = 1.0f64;
        let mut total = 0.0f64;
        for part in v.split(':').rev() {
            total += part.parse::<f64>().ok()? * base;
            base *= 60.0;
        }
        return Some(sign * total);
    }
    // Python's float() takes "1." and ".5", which Rust's parse does too
    v.parse::<f64>().ok().map(|f| sign * f)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines() {
        let text = "title: x\nparts:\n  U1: {type: dip,\n       pins: 8}\n  R1: {type: resistor}\nwires:\n  power:\n    - [a1, a2]\n\n    - {from: a3,\n       to: a4}\n  i2c: [[b1, b2]]\nnets:\n  on: [U1.1]\n  GND: [U1.4]\n  GND: [U1.5]\n";
        let (_, l) = load_lines(text).unwrap();
        assert_eq!(l.get(&["parts", "U1"]), Some(3));
        assert_eq!(l.get(&["parts", "R1"]), Some(5));
        assert_eq!(l.get(&["wires", "power", "0"]), Some(8));
        assert_eq!(l.get(&["wires", "power", "1"]), Some(10));
        assert_eq!(l.get(&["wires", "i2c", "0"]), Some(12));
        assert_eq!(l.get(&["nets", "True"]), Some(14)); // `on` is a boolean key, as in Python
        assert_eq!(l.get(&["nets", "GND"]), Some(16)); // given twice: the value that counts is the last
        assert_eq!(l.get(&["parts", "U1", "pins", "0"]), None); // deeper than kept
        assert_eq!(l.of_message("wires.power[1]: flex is true or false"), Some(10));
        assert_eq!(l.of_message("parts.R1: needs pins"), Some(5));
        assert_eq!(l.of_message("nets.SDA: no such net"), None);
        assert_eq!(l.of_message("something else"), None);

        let (_, l) = load_lines("wires:\n  - [a1, a2]\n  - [a3, a4]\n").unwrap();
        assert_eq!(l.of_message("wires.wires[1]: unknown color 'x'"), Some(3));
    }

    #[test]
    fn aliases_copy_their_anchor_up_to_a_limit() {
        let v = load("pins: &p [U1.1, U1.2]\nnets:\n  A: *p\n  B: *p\n").unwrap();
        assert_eq!(v.at("nets").at("B"), v.at("pins"));
        // a "billion laughs": each level ten of the last, 10^9 values in a few lines
        let mut text = "a0: &a0 [x, x, x, x, x, x, x, x, x, x]\n".to_string();
        for k in 1..10 {
            let j = k - 1;
            text += &format!("a{k}: &a{k} [*a{j}, *a{j}, *a{j}, *a{j}, *a{j}, *a{j}, *a{j}, *a{j}, *a{j}, *a{j}]\n");
        }
        let err = load(&text).unwrap_err();
        assert!(err.starts_with("the aliases add more than 100000 values (line "), "{err}");
    }
}
