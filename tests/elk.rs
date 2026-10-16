//! ELK (elk-rs, src/elk.rs) lays graphs out as recorded: tests/elk/*.out.json are its layouts of tests/elk/*.in.json.
//! They were checked against elkjs in Node when bbgen moved from elkjs to elk-rs (the same to 1e-14 px). Upgrading
//! elk-rs may move them: look at the schematics (tests/schematic.rs), then record these again with
//! `BBGEN_RECORD=1 cargo test --release --test elk`.

mod common;

use serde_json::Value;

fn json(path: &std::path::Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn graphs() -> Vec<(String, Value, std::path::PathBuf)> {
    let dir = common::root().join("tests/elk");
    let mut names: Vec<String> = std::fs::read_dir(&dir).unwrap().filter_map(|e| e.ok())
        .filter_map(|e| e.file_name().to_string_lossy().strip_suffix(".in.json").map(String::from)).collect();
    names.sort();
    names.into_iter().map(|n| {
        let input = json(&dir.join(format!("{}.in.json", n)));
        (n.clone(), input, dir.join(format!("{}.out.json", n)))
    }).collect()
}

#[test]
fn layouts_are_as_recorded() {
    let all = graphs();
    assert!(all.len() >= 3);
    for (name, input, out) in all {
        let got = bbgen::elk::layout(&input).unwrap_or_else(|e| panic!("{}: {}", name, e));
        if std::env::var("BBGEN_RECORD").is_ok() {
            std::fs::write(&out, serde_json::to_string(&got).unwrap() + "\n").unwrap();
        } else {
            assert!(got == json(&out), "{}: ELK's layout differs from the record", name);
        }
    }
}

#[test]
fn the_same_graph_gets_the_same_layout_every_time_on_every_thread() {
    let (_, input, _) = graphs().into_iter().find(|(n, _, _)| n == "ports").unwrap();
    let first = bbgen::elk::layout(&input).unwrap();
    for _ in 0..3 {
        assert!(bbgen::elk::layout(&input).unwrap() == first);
    }
    let runs: Vec<usize> = (0..6).collect();
    let results = common::parallel(&runs, |_| bbgen::elk::layout(&input).unwrap());
    assert!(results.iter().all(|r| *r == first));
}

#[test]
fn a_broken_graph_says_what_is_wrong() {
    let graph = serde_json::json!({
        "id": "root",
        "children": [{"id": "a", "width": 10, "height": 10}],
        "edges": [{"id": "e", "sources": ["a"], "targets": ["nowhere"]}],
    });
    let e = bbgen::elk::layout(&graph).unwrap_err();
    assert!(e.starts_with("ELK: ") && e.contains("nowhere"), "{}", e);
    // and ELK still works afterwards
    let (_, input, _) = graphs().into_iter().find(|(n, _, _)| n == "ports").unwrap();
    assert!(bbgen::elk::layout(&input).is_ok());
}
