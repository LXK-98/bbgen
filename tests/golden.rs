//! Each benchmark placed, wired and checked from scratch: every event the same as in its golden record,
//! from the first anneal to the check (benchmarks/golden/, recorded from the Python bbgen this port
//! replaced; bbgen dev build --dump writes the same events).

mod common;

use bbgen::circuit::Circuit;

fn build(path: &std::path::Path) -> Vec<serde_json::Value> {
    bbgen::dump::start();
    let c = Circuit::load(&path.to_string_lossy()).unwrap();
    let r = bbgen::check::check(&c);
    let issues: Vec<serde_json::Value> = r.issues.iter().map(|i| serde_json::json!([i.level, i.code, i.message])).collect();
    bbgen::dump::event("check", vec![("issues", serde_json::Value::Array(issues)),
                                     ("score", bbgen::dump::num(bbgen::autoplace::score(&c, &r))),
                                     ("hash", serde_json::json!(bbgen::layout_hash(&c)))]);
    bbgen::dump::finish()
}

#[test]
fn benchmarks_match_their_golden_records() {
    let paths = common::benchmarks();
    assert_eq!(paths.len(), 21);
    let failures: Vec<String> = common::parallel(&paths, |p| {
        let name = p.file_stem().unwrap().to_string_lossy().into_owned();
        let (want, got) = (common::record(&name), build(p));
        for (i, (w, g)) in want.iter().zip(&got).enumerate() {
            if !common::same(w, g) {
                return Some(format!("{}: event {} differs\n  want {}\n  got  {}", name, i + 1,
                                    &w.to_string()[..w.to_string().len().min(400)], &g.to_string()[..g.to_string().len().min(400)]));
            }
        }
        (want.len() != got.len()).then(|| format!("{}: {} events recorded, {} now", name, want.len(), got.len()))
    }).into_iter().flatten().collect();
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}
