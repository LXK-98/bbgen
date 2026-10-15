//! Reading a circuit file as PyYAML's `safe_load` does, which is how bbgen always read them: libyaml's
//! grammar (YAML 1.1: `rail: -}` is a plain "-") and SafeLoader's rules for what a plain scalar is (yes/no/on/off
//! are booleans, 010 is octal, 1_000 is a thousand, 1:30 is ninety, ~ and null and nothing are None). A key
//! given twice keeps its first place and its last value.

use crate::val::Val;
use std::mem::MaybeUninit;
use unsafe_libyaml::*;

pub fn load(text: &str) -> Result<Val, String> {
    unsafe {
        let mut parser = MaybeUninit::<yaml_parser_t>::uninit();
        if yaml_parser_initialize(parser.as_mut_ptr()).fail {
            return Err("couldn't start the YAML parser".into());
        }
        let parser = parser.as_mut_ptr();
        yaml_parser_set_input_string(parser, text.as_ptr(), text.len() as _);
        let mut b = Builder { stack: vec![], anchors: vec![], doc: None, docs: 0 };
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
                break Ok(b.doc.take().unwrap_or(Val::Null));
            }
        };
        yaml_parser_delete(parser);
        out
    }
}

enum Frame {
    Seq(Vec<Val>, Option<String>),
    Map(Vec<(Val, Val)>, Option<Val>, Option<String>),
}

struct Builder {
    stack: Vec<Frame>,
    anchors: Vec<(String, Val)>,
    doc: Option<Val>,
    docs: usize,
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
                    self.anchors.push((a, v.clone()));
                }
                self.add(v)?;
            }
            YAML_ALIAS_EVENT => {
                let name = cstr(ev.data.alias.anchor).unwrap_or_default();
                let v = self.anchors.iter().rev().find(|(a, _)| *a == name).map(|(_, v)| v.clone())
                    .ok_or(format!("found undefined alias {}", name))?;
                self.add(v)?;
            }
            YAML_SEQUENCE_START_EVENT => self.stack.push(Frame::Seq(vec![], cstr(ev.data.sequence_start.anchor))),
            YAML_MAPPING_START_EVENT => self.stack.push(Frame::Map(vec![], None, cstr(ev.data.mapping_start.anchor))),
            YAML_SEQUENCE_END_EVENT | YAML_MAPPING_END_EVENT => {
                let (v, anchor) = match self.stack.pop().unwrap() {
                    Frame::Seq(items, a) => (Val::List(items), a),
                    Frame::Map(items, _, a) => (Val::Map(items), a),
                };
                if let Some(a) = anchor {
                    self.anchors.push((a, v.clone()));
                }
                self.add(v)?;
            }
            _ => {}
        }
        Ok(())
    }

    fn add(&mut self, v: Val) -> Result<(), String> {
        match self.stack.last_mut() {
            None => self.doc = Some(v),
            Some(Frame::Seq(items, _)) => items.push(v),
            Some(Frame::Map(items, key, _)) => match key.take() {
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
