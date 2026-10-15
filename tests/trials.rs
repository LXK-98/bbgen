//! Every trial layout placement built for the golden records, built again: the same score, to the last bit.
//! A trial that scores differently is a difference in building (leg placement, wiring, the check, the score),
//! not in searching.

mod common;

use bbgen::circuit::Circuit;
use bbgen::val::Val;

/// The file autoplace builds for a trial: the benchmark, autowire on, the parts it places, the layout in it.
fn trial_data(data: &Val, layout: &serde_json::Value) -> Val {
    let mut out = data.clone();
    if out.get("autowire").is_none() {
        out.map_set("autowire", Val::Bool(true));
    }
    let parts = layout["parts"].as_object().unwrap();
    out.map_set("_placed", Val::List(parts.keys().map(|k| Val::Str(k.clone())).collect()));
    if layout["rail_order"].as_array().map(|a| !a.is_empty()).unwrap_or(false) {
        out.map_set("_rail_order", Val::from_json(&layout["rail_order"]));
    }
    for (r, at) in parts {
        let spec = out.map_get_mut("parts").unwrap().map_get_mut(r).unwrap();
        match at {
            serde_json::Value::Array(a) if a.len() == 2 && a[1].is_i64() => {
                spec.map_set("at", Val::from_json(&a[0]));
                spec.map_set("rot", Val::from_json(&a[1]));
            }
            serde_json::Value::Null => spec.map_remove("pins"),
            pins => spec.map_set("pins", Val::from_json(pins)),
        }
    }
    out.map_set("_leg_rank", Val::from_json(&layout["leg_rank"]));
    out
}

#[test]
fn every_recorded_trial_scores_the_same() {
    let paths = common::benchmarks();
    let results: Vec<(usize, Vec<String>)> = common::parallel(&paths, |p| {
        let name = p.file_stem().unwrap().to_string_lossy().into_owned();
        let data = Val::load(&std::fs::read_to_string(p).unwrap()).unwrap();
        let trials: Vec<serde_json::Value> = common::record(&name).into_iter().filter(|e| e["kind"] == "trial").collect();
        let mut bad = Vec::new();
        for (i, t) in trials.iter().enumerate() {
            let c = Circuit::new(&trial_data(&data, &t["layout"]), &p.to_string_lossy()).unwrap();
            let r = bbgen::check::check(&c);
            let score = bbgen::autoplace::score(&c, &r);
            if Some(score) != t["score"].as_f64() {
                bad.push(format!("{} trial {}: recorded {}, now {}", name, i, t["score"], bbgen::pyf::repr(score)));
            }
        }
        (trials.len(), bad)
    });
    let total: usize = results.iter().map(|r| r.0).sum();
    let bad: Vec<String> = results.into_iter().flat_map(|r| r.1).collect();
    assert!(total > 2900, "only {} trials recorded", total);
    assert!(bad.is_empty(), "{} of {} trials score differently:\n{}", bad.len(), total, bad[..bad.len().min(10)].join("\n"));
}
