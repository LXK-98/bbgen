//! The scene (`--json`) for every example and benchmark: its drawing is the drawing `bbgen build --bare` writes, with
//! groups saying what each piece is, every part and wire has its group, and what it names exists.

use bbgen::check::check;
use bbgen::circuit::Circuit;
use bbgen::val::Val;
use serde_json::Value;
use std::collections::HashSet;

/// The drawing's tags, in order, without group closes and the groups `--json` adds (the counts are checked apart).
fn elements(svg: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = svg;
    while let Some(at) = rest.find('<') {
        rest = &rest[at..];
        let end = rest.find('>').map(|e| e + 1).unwrap_or(rest.len());
        let tag = &rest[..end];
        if !tag.starts_with("<g data-") && tag != "</g>" {
            out.push(tag);
        }
        rest = &rest[end..];
    }
    out
}

#[test]
fn scenes() {
    let mut files: Vec<std::path::PathBuf> = Vec::new();
    for dir in ["examples", "benchmarks"] {
        for e in std::fs::read_dir(dir).unwrap() {
            let p = e.unwrap().path();
            if p.extension().map(|x| x == "yaml").unwrap_or(false) {
                files.push(p);
            }
        }
    }
    files.sort();
    assert!(files.len() > 20);
    for path in files {
        let name = path.to_string_lossy().into_owned();
        let c = Circuit::load(&name).unwrap_or_else(|e| panic!("{}: {}", name, e));
        let r = check(&c);
        let data = Val::load(&std::fs::read_to_string(&path).unwrap()).ok();
        let s: Value = bbgen::scene::scene(&c, &r, data.as_ref(), 0.0);
        let svg = s["svg"].as_str().unwrap();

        // the bare drawing, element for element
        let plain = bbgen::render::draw_bare(&c, &r, None).0;
        let opened = svg.matches("<g data-").count();
        assert_eq!(svg.matches("<g ").count() - opened, plain.matches("<g ").count(), "{}", name);
        assert_eq!(svg.matches("</g>").count() - opened, plain.matches("</g>").count(), "{}: groups unbalanced", name);
        assert!(elements(svg) == elements(&plain), "{}: drawing differs", name);

        // every part and placed wire is in the drawing under its name; every name the scene uses exists
        let refs: HashSet<&str> = s["parts"].as_array().unwrap().iter().map(|p| p["ref"].as_str().unwrap()).collect();
        for p in s["parts"].as_array().unwrap() {
            let r = p["ref"].as_str().unwrap();
            if p["pins"][0]["xy"].is_array() {
                assert!(svg.contains(&format!("data-part=\"{}\"", r)), "{}: no group for {}", name, r);
            }
            assert!(p["placed_by"].is_string(), "{}: {} placed by whom?", name, r);
        }
        let nets: HashSet<&str> = s["nets"].as_array().unwrap().iter().map(|n| n["name"].as_str().unwrap()).collect();
        for w in s["wires"].as_array().unwrap() {
            let id = w["id"].as_str().unwrap();
            if w["points"].as_array().unwrap().len() >= 2 {
                assert!(svg.contains(&format!("data-wire=\"{}\"", id)), "{}: no group for {}", name, id);
            }
            for n in w["nets"].as_array().unwrap() {
                assert!(nets.contains(n.as_str().unwrap()), "{}: {} on unknown net {}", name, id, n);
            }
        }
        for step in s["checklist"].as_array().unwrap() {
            let item = step["item"].as_str().unwrap();
            let ok = match item.split_once(':') {
                Some(("part", r)) => refs.contains(r),
                Some(("wire", w)) => s["wires"].as_array().unwrap().iter().any(|x| x["id"] == w),
                _ => false,
            };
            assert!(ok, "{}: checklist names {}", name, item);
        }
        assert_eq!(s["checklist"].as_array().unwrap().len(),
                   s["parts"].as_array().unwrap().len() + s["wires"].as_array().unwrap().len(), "{}", name);
        for b in s["boards"].as_array().unwrap() {
            assert!(!b["holes"].as_object().unwrap().is_empty());
        }
    }
}
