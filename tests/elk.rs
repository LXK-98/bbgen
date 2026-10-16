//! ELK in QuickJS (src/elk.rs) lays graphs out exactly as elkjs does in Node: tests/elk/*.out.json are Node's
//! layouts of tests/elk/*.in.json, made with the vendored file (tests/elk/record.mjs).

mod common;

use serde_json::Value;
use sha2::{Digest, Sha256};

fn json(path: &std::path::Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn graphs() -> Vec<(String, Value, Value)> {
    let dir = common::root().join("tests/elk");
    let mut names: Vec<String> = std::fs::read_dir(&dir).unwrap().filter_map(|e| e.ok())
        .filter_map(|e| e.file_name().to_string_lossy().strip_suffix(".in.json").map(String::from)).collect();
    names.sort();
    names.into_iter().map(|n| {
        let (input, output) = (json(&dir.join(format!("{}.in.json", n))), json(&dir.join(format!("{}.out.json", n))));
        (n, input, output)
    }).collect()
}

#[test]
fn the_vendored_elkjs_is_the_one_its_readme_names() {
    let file = std::fs::read(common::root().join("vendor/elkjs/elk.bundled.js")).unwrap();
    let digest: String = Sha256::digest(&file).iter().map(|b| format!("{:02x}", b)).collect();
    let readme = std::fs::read_to_string(common::root().join("vendor/elkjs/README.md")).unwrap();
    assert!(readme.contains(&digest), "vendor/elkjs/elk.bundled.js is {}, which vendor/elkjs/README.md doesn't name", digest);
}

#[test]
fn layouts_are_as_in_node() {
    let all = graphs();
    assert!(all.len() >= 3);
    for (name, input, expected) in all {
        let got = bbgen::elk::layout(&input).unwrap_or_else(|e| panic!("{}: {}", name, e));
        assert!(got == expected, "{}: ELK in QuickJS differs from elkjs in Node", name);
    }
}

#[test]
fn the_same_graph_gets_the_same_layout_every_time_on_every_thread() {
    let (_, input, expected) = graphs().into_iter().find(|(n, _, _)| n == "ports").unwrap();
    for _ in 0..3 {
        assert!(bbgen::elk::layout(&input).unwrap() == expected);
    }
    let runs: Vec<usize> = (0..6).collect();
    let results = common::parallel(&runs, |_| bbgen::elk::layout(&input).unwrap());
    assert!(results.iter().all(|r| *r == expected));
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
    // and the engine still works afterwards
    let (_, input, expected) = graphs().into_iter().find(|(n, _, _)| n == "ports").unwrap();
    assert!(bbgen::elk::layout(&input).unwrap() == expected);
}
