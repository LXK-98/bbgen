//! What the tests share: the golden records' location, reading them, and comparing JSON as Python would.
#![allow(dead_code)]

use std::io::Read;
use std::path::PathBuf;

pub fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

pub fn benchmarks() -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(root().join("benchmarks")).unwrap().filter_map(|e| e.ok()).map(|e| e.path())
        .filter(|p| p.extension().map(|x| x == "yaml").unwrap_or(false)).collect();
    v.sort();
    v
}

/// A golden record (benchmarks/golden/<name>.jsonl.gz): one event a line.
pub fn record(name: &str) -> Vec<serde_json::Value> {
    let f = std::fs::File::open(root().join("benchmarks/golden").join(format!("{}.jsonl.gz", name))).unwrap();
    let mut text = String::new();
    flate2::read::GzDecoder::new(f).read_to_string(&mut text).unwrap();
    text.lines().filter(|l| !l.trim().is_empty()).map(|l| serde_json::from_str(l).unwrap()).collect()
}

/// JSON values equal as Python compares them: 13 == 13.0.
pub fn same(a: &serde_json::Value, b: &serde_json::Value) -> bool {
    use serde_json::Value as J;
    match (a, b) {
        (J::Number(x), J::Number(y)) => x.as_f64() == y.as_f64(),
        (J::Array(x), J::Array(y)) => x.len() == y.len() && x.iter().zip(y).all(|(p, q)| same(p, q)),
        (J::Object(x), J::Object(y)) => x.len() == y.len() && x.iter().all(|(k, v)| y.get(k).map(|w| same(v, w)).unwrap_or(false)),
        _ => a == b,
    }
}

/// Run f over items on a few threads, in any order.
pub fn parallel<T: Sync, R: Send>(items: &[T], f: impl Fn(&T) -> R + Sync) -> Vec<R> {
    let n = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
    let next = std::sync::atomic::AtomicUsize::new(0);
    let out: std::sync::Mutex<Vec<(usize, R)>> = std::sync::Mutex::new(Vec::new());
    std::thread::scope(|s| {
        for _ in 0..n {
            s.spawn(|| loop {
                let i = next.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                if i >= items.len() {
                    break;
                }
                let r = f(&items[i]);
                out.lock().unwrap().push((i, r));
            });
        }
    });
    let mut v = out.into_inner().unwrap();
    v.sort_by_key(|(i, _)| *i);
    v.into_iter().map(|(_, r)| r).collect()
}
