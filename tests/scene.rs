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
        let text = std::fs::read_to_string(&path).unwrap();
        let s: Value = bbgen::scene::scene(&c, &r, Some(&text), 0.0);
        let svg = s["svg"].as_str().unwrap();

        // every part, wire and net the file gives has its line, and it's the line that names it
        let lines: Vec<&str> = text.lines().collect();
        let at = |v: &Value| v["line"].as_u64().map(|l| lines[l as usize - 1]);
        for p in s["parts"].as_array().unwrap() {
            let r = p["ref"].as_str().unwrap();
            assert!(at(p).is_some_and(|l| l.trim_start().starts_with(&format!("{}:", r))), "{}: {}'s line", name, r);
        }
        for w in s["wires"].as_array().unwrap() {
            let mine = w["laid_by"] == "you";
            assert_eq!(at(w).is_some(), mine, "{}: {}'s line", name, w["id"]);
            if mine {
                let a = w["spec"][0].as_str().unwrap();
                assert!(at(w).unwrap().contains(a), "{}: {} starts at {}, not on line {}", name, w["id"], a, w["line"]);
            }
        }
        let file_nets = Val::load(&text).unwrap().at("nets").as_map().map(|m| m.len()).unwrap_or(0);
        let with_line = s["nets"].as_array().unwrap().iter().filter(|n| {
            at(n).is_some_and(|l| l.trim_start().trim_matches('"').starts_with(n["name"].as_str().unwrap()))
        }).count();
        assert_eq!(with_line, file_nets, "{}: nets' lines", name);

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
        // the steps come section by section, in the sections' order
        let order: Vec<&str> = s["sections"].as_array().unwrap().iter().map(|x| x["key"].as_str().unwrap()).collect();
        let at: Vec<usize> = s["checklist"].as_array().unwrap().iter()
            .map(|x| order.iter().position(|k| *k == x["section"].as_str().unwrap()).expect("a step in no section")).collect();
        assert!(at.windows(2).all(|w| w[0] <= w[1]), "{}: steps out of their sections' order", name);
        assert_eq!(s["checklist"].as_array().unwrap().len(),
                   s["parts"].as_array().unwrap().len() + s["wires"].as_array().unwrap().len(), "{}", name);
        for b in s["boards"].as_array().unwrap() {
            assert!(!b["holes"].as_object().unwrap().is_empty());
        }
    }
}

/// `--events`: a line for each step as it's taken, then the scene `--json` prints. Placement builds its trials side
/// by side with it as without, so the board is the same.
#[test]
fn events() {
    for file in ["benchmarks/04-555-blinker.yaml", "benchmarks/09-pico-595-leds.yaml"] {
        let run = |flag: &str| {
            let out = std::process::Command::new(env!("CARGO_BIN_EXE_bbgen")).args(["build", file, flag]).output().unwrap();
            String::from_utf8(out.stdout).unwrap().lines().map(|l| serde_json::from_str::<Value>(l).unwrap()).collect::<Vec<_>>()
        };
        let (mut lines, json) = (run("--events"), run("--json"));
        let mut scene = lines.pop().unwrap();
        let mut plain = json[0].clone();
        scene["stats"]["ms"] = Value::Null;
        plain["stats"]["ms"] = Value::Null;
        assert!(scene == plain, "{}: the scene after the events isn't the one --json prints", file);
        assert!(lines.len() > 10, "{}: only {} events", file, lines.len());

        let mut best = f64::INFINITY;
        for (k, e) in lines.iter().enumerate() {
            assert_eq!(e["event"], "step");
            assert_eq!(e["n"], k as u64 + 1, "{}: the steps out of order", file);
            let kept = e["kept"].as_bool().unwrap();
            assert_eq!(e["svg"].is_string(), kept, "{}: step {}: a drawing with the kept steps, and only with them", file, k + 1);
            let phase = e["phase"].as_str().unwrap();
            assert!(["start", "turn", "repair", "wiring"].contains(&phase), "{}: phase {}", file, phase);
            if phase == "wiring" {
                assert!(kept && e["score"].is_null());
                continue;
            }
            let score = e["score"].as_f64().unwrap();
            if kept {
                assert!(score <= best, "{}: step {} kept a worse layout", file, k + 1);
                best = score;
            }
        }
        assert!(lines.iter().any(|e| e["phase"] == "wiring") && lines.iter().any(|e| e["kept"] == false));
    }
}
