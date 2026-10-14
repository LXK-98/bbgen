//! bbgen-rs: build a placed circuit file, wire it and check it, as the Python does.
//!
//!     bbgen-rs build placed.yaml [--dump out.jsonl] [--repeat N]
//!
//! --dump writes the build's events (bbgen/dump.py's format) for `benchmarks/golden.py --diff`; --repeat builds
//! it N times and prints the time per build.

use bbgen::circuit::Circuit;
use bbgen::val::Val;
use std::io::BufRead;
use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("build") => std::process::exit(build(&args[2..])),
        Some("pyf-check") => pyf_check(),
        _ => {
            eprintln!("usage: bbgen-rs build placed.yaml [--dump out.jsonl] [--repeat N]");
            std::process::exit(2);
        }
    }
}

fn build(args: &[String]) -> i32 {
    let path = &args[0];
    let mut dump_to: Option<String> = None;
    let mut repeat = 0usize;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--dump" => { dump_to = Some(args[i + 1].clone()); i += 2; }
            "--repeat" => { repeat = args[i + 1].parse().unwrap(); i += 2; }
            other => { eprintln!("unknown option {}", other); return 2; }
        }
    }
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) => { eprintln!("{}: {}", path, e); return 2; }
    };
    let data = match Val::load(&text) {
        Ok(d) => d,
        Err(e) => { eprintln!("{}: {}", path, e); return 2; }
    };
    if dump_to.is_some() {
        bbgen::dump::start();
    }
    let start = Instant::now();
    let c = match Circuit::new(&data, path) {
        Ok(c) => c,
        Err(e) => { eprintln!("{}: {}", path, e); return 2; }
    };
    let r = bbgen::check::check(&c);
    let first = start.elapsed();
    let score = bbgen::autoplace::score(&c, &r);
    let hash = bbgen::layout_hash(&c);
    if let Some(out) = dump_to {
        let issues: Vec<serde_json::Value> = r.issues.iter().map(|i| serde_json::json!([i.level, i.code, i.message])).collect();
        bbgen::dump::event("check", vec![("issues", serde_json::Value::Array(issues)), ("score", bbgen::dump::num(score)),
                                         ("hash", serde_json::json!(hash))]);
        let events = bbgen::dump::finish();
        let lines: Vec<String> = events.iter().map(|e| serde_json::to_string(e).unwrap()).collect();
        std::fs::write(&out, lines.join("\n") + "\n").unwrap();
    }
    println!("{} wires, {} errors, {} warnings, score {}, hash {}, first build {:.1} ms", c.wires.len(), r.errors(), r.warnings(),
             bbgen::pyf::repr(score), hash, first.as_secs_f64() * 1000.0);
    if repeat > 0 {
        let start = Instant::now();
        for _ in 0..repeat {
            let c = Circuit::new(&data, path).unwrap();
            let r = bbgen::check::check(&c);
            std::hint::black_box(bbgen::autoplace::score(&c, &r));
        }
        println!("{:.3} ms per build ({} builds)", start.elapsed().as_secs_f64() * 1000.0 / repeat as f64, repeat);
    }
    if r.errors() > 0 { 1 } else { 0 }
}

fn pyf_check() {
    // lines of "a b c d": dist, hypot(a-c, b-d), round(a,3), round(a,6), sum, a*b, for checking against Python
    use bbgen::pyf;
    let stdin = std::io::stdin();
    for line in stdin.lock().lines() {
        let v: Vec<f64> = line.unwrap().split_whitespace().map(|s| s.parse().unwrap()).collect();
        println!("{} {} {} {} {} {}", pyf::repr(pyf::dist((v[0], v[1]), (v[2], v[3]))), pyf::repr(pyf::hypot(v[0] - v[2], v[1] - v[3])),
                 pyf::repr(pyf::round(v[0], 3)), pyf::repr(pyf::round(v[0], 6)), pyf::repr(pyf::fsum(v.iter().copied())), pyf::repr(v[0] * v[1]));
    }
}
