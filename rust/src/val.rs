//! A circuit file's values as Python sees them after `yaml.safe_load`: what bbgen's code tests for truth,
//! turns into strings with `str()`, and looks up by key.

use yaml_rust2::{Yaml, YamlLoader};

#[derive(Clone, Debug, PartialEq)]
pub enum Val {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    List(Vec<Val>),
    Map(Vec<(Val, Val)>),
}

impl Val {
    pub fn from_yaml(y: &Yaml) -> Val {
        match y {
            Yaml::Real(s) => Val::Float(parse_real(s)),
            Yaml::Integer(i) => Val::Int(*i),
            Yaml::String(s) => Val::Str(s.clone()),
            Yaml::Boolean(b) => Val::Bool(*b),
            Yaml::Array(a) => Val::List(a.iter().map(Val::from_yaml).collect()),
            Yaml::Hash(h) => Val::Map(h.iter().map(|(k, v)| (Val::from_yaml(k), Val::from_yaml(v))).collect()),
            Yaml::Null | Yaml::BadValue => Val::Null,
            Yaml::Alias(_) => Val::Null,
        }
    }

    pub fn load(text: &str) -> Result<Val, String> {
        let docs = YamlLoader::load_from_str(text).map_err(|e| e.to_string())?;
        Ok(docs.first().map(Val::from_yaml).unwrap_or(Val::Null))
    }

    /// `spec.get(key)` on a mapping (None when not a mapping or not there).
    pub fn get(&self, key: &str) -> Option<&Val> {
        match self {
            Val::Map(items) => items.iter().find(|(k, _)| matches!(k, Val::Str(s) if s == key)).map(|(_, v)| v),
            _ => None,
        }
    }

    /// `spec.get(key)`, a missing key and None both giving Null.
    pub fn at(&self, key: &str) -> &Val {
        static NULL: Val = Val::Null;
        self.get(key).unwrap_or(&NULL)
    }

    pub fn truthy(&self) -> bool {
        match self {
            Val::Null => false,
            Val::Bool(b) => *b,
            Val::Int(i) => *i != 0,
            Val::Float(f) => *f != 0.0,
            Val::Str(s) => !s.is_empty(),
            Val::List(l) => !l.is_empty(),
            Val::Map(m) => !m.is_empty(),
        }
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Val::Null)
    }

    /// Python's `str()`.
    pub fn py_str(&self) -> String {
        match self {
            Val::Null => "None".into(),
            Val::Bool(b) => if *b { "True".into() } else { "False".into() },
            Val::Int(i) => i.to_string(),
            Val::Float(f) => crate::pyf::repr(*f),
            Val::Str(s) => s.clone(),
            Val::List(l) => format!("[{}]", l.iter().map(|v| v.py_repr()).collect::<Vec<_>>().join(", ")),
            Val::Map(m) => format!("{{{}}}", m.iter().map(|(k, v)| format!("{}: {}", k.py_repr(), v.py_repr()))
                .collect::<Vec<_>>().join(", ")),
        }
    }

    pub fn py_repr(&self) -> String {
        match self {
            Val::Str(s) => repr_str(s),
            _ => self.py_str(),
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Val::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_list(&self) -> Option<&Vec<Val>> {
        match self {
            Val::List(l) => Some(l),
            _ => None,
        }
    }

    pub fn as_map(&self) -> Option<&Vec<(Val, Val)>> {
        match self {
            Val::Map(m) => Some(m),
            _ => None,
        }
    }

    pub fn as_int(&self) -> Option<i64> {
        match self {
            Val::Int(i) => Some(*i),
            Val::Bool(b) => Some(*b as i64),
            _ => None,
        }
    }

    /// Python's `int(x)` for the values a circuit file holds there (a number, or a string of digits).
    pub fn py_int(&self) -> Option<i64> {
        match self {
            Val::Int(i) => Some(*i),
            Val::Bool(b) => Some(*b as i64),
            Val::Float(f) => Some(f.trunc() as i64),
            Val::Str(s) => s.trim().parse().ok(),
            _ => None,
        }
    }

    /// To JSON, as `json.dumps` would write the value (for the dump of the placed file).
    pub fn to_json(&self) -> serde_json::Value {
        use serde_json::Value as J;
        match self {
            Val::Null => J::Null,
            Val::Bool(b) => J::Bool(*b),
            Val::Int(i) => J::from(*i),
            Val::Float(f) => serde_json::Number::from_f64(*f).map(J::Number).unwrap_or(J::Null),
            Val::Str(s) => J::String(s.clone()),
            Val::List(l) => J::Array(l.iter().map(Val::to_json).collect()),
            Val::Map(m) => J::Object(m.iter().map(|(k, v)| (k.py_str(), v.to_json())).collect()),
        }
    }
}

fn parse_real(s: &str) -> f64 {
    let t = s.trim().to_ascii_lowercase().replace('_', "");
    match t.as_str() {
        ".inf" | "+.inf" => f64::INFINITY,
        "-.inf" => f64::NEG_INFINITY,
        ".nan" => f64::NAN,
        _ => t.parse().unwrap_or(f64::NAN),
    }
}

/// Python's `repr()` of a string.
pub fn repr_str(s: &str) -> String {
    let quote = if s.contains('\'') && !s.contains('"') { '"' } else { '\'' };
    let mut out = String::with_capacity(s.len() + 2);
    out.push(quote);
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c == quote => {
                out.push('\\');
                out.push(c);
            }
            c if (c as u32) < 0x20 || c as u32 == 0x7f => out.push_str(&format!("\\x{:02x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push(quote);
    out
}
