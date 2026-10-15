//! The record of a build, event by event, in the format bbgen/dump.py writes: for checking this port against
//! the Python with `benchmarks/golden.py --diff`.

use serde_json::{Map, Value};
use std::cell::RefCell;

thread_local! {
    static EVENTS: RefCell<Option<Vec<Value>>> = const { RefCell::new(None) };
    static PLACING: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

pub fn active() -> bool {
    EVENTS.with(|e| e.borrow().is_some())
}

/// A build's own events are recorded only outside placement's trial builds (dump.py's build=True).
pub fn building() -> bool {
    active() && PLACING.with(|p| p.get()) == 0
}

/// Run f as placement: the trial builds it makes record no build events.
pub fn placing<T>(f: impl FnOnce() -> T) -> T {
    PLACING.with(|p| p.set(p.get() + 1));
    let out = f();
    PLACING.with(|p| p.set(p.get() - 1));
    out
}

pub fn event(kind: &str, fields: Vec<(&str, Value)>) {
    EVENTS.with(|e| {
        if let Some(events) = e.borrow_mut().as_mut() {
            let mut m = Map::new();
            m.insert("kind".into(), Value::String(kind.into()));
            for (k, v) in fields {
                m.insert(k.into(), v);
            }
            events.push(Value::Object(m));
        }
    });
}

pub fn start() {
    EVENTS.with(|e| *e.borrow_mut() = Some(Vec::new()));
}

pub fn finish() -> Vec<Value> {
    EVENTS.with(|e| e.borrow_mut().take().unwrap_or_default())
}

/// A float as dump.py writes it: as is, or "inf" / "-inf" / "nan".
pub fn num(x: f64) -> Value {
    if x.is_finite() {
        serde_json::Number::from_f64(x).map(Value::Number).unwrap()
    } else if x.is_nan() {
        Value::String("nan".into())
    } else if x > 0.0 {
        Value::String("inf".into())
    } else {
        Value::String("-inf".into())
    }
}

pub fn xy(p: (f64, f64)) -> Value {
    Value::Array(vec![num(p.0), num(p.1)])
}
