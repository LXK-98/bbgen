//! bbgen-rs: breadboard layouts from YAML, the same as `bbgen` (bbgen/cli.py).
//!
//!     bbgen-rs build circuit.yaml [-o DIR] [--no-png] [--steps]   drawing, checklist and the check
//!     bbgen-rs check circuit.yaml                                  the check only
//!     bbgen-rs find WORDS... [--kicad DIR]                         search KiCad's library for a symbol
//!
//! For checking the port against the Python:
//!
//!     bbgen-rs dev build FILE [--dump OUT.jsonl] [--repeat N]     build, record events, time it
//!     bbgen-rs dev score-lines | yaml-json FILE | pyf-check

use bbgen::check::{check, Result};
use bbgen::circuit::Circuit;
use bbgen::val::Val;
use std::io::BufRead;
use std::path::{Component, Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

const USAGE: &str = "usage: bbgen-rs {build,check,find} ...\n\n\
  build circuit.yaml [-o DIR] [--no-png] [--steps]  write <name>.svg/.png/.md next to the YAML and check the wiring\n\
  check circuit.yaml                                check the wiring only\n\
  find WORDS... [--kicad DIR]                       search KiCad's library for a part's symbol: bbgen find ne555";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = match args.first().map(String::as_str) {
        Some("build") | Some("check") => run(&args),
        Some("find") => find(&args[1..]),
        Some("dev") => dev(&args[1..]),
        Some("--version") => {
            println!("bbgen {}", env!("CARGO_PKG_VERSION"));
            0
        }
        _ => {
            eprintln!("{}", USAGE);
            2
        }
    };
    ExitCode::from(code as u8)
}

/// A path as pathlib writes it: no "." parts.
fn clean(p: &Path) -> PathBuf {
    p.components().filter(|c| !matches!(c, Component::CurDir)).collect()
}

fn report(r: &Result) {
    for i in &r.issues {
        println!("{:<8}{:<13}{}", i.level, i.code, i.message);
    }
    let (e, w) = (r.errors(), r.warnings());
    println!("check: {} error{}, {} warning{}", e, if e != 1 { "s" } else { "" }, w, if w != 1 { "s" } else { "" });
}

fn png(svg: &Path) -> Option<PathBuf> {
    let png = svg.with_extension("png");
    let have = |tool: &str| std::env::var_os("PATH").map(|p| std::env::split_paths(&p).any(|d| d.join(tool).is_file())).unwrap_or(false);
    let status = if have("magick") {
        std::process::Command::new("magick").args(["-density", "144", "-background", "white"]).arg(svg).arg(&png).status()
    } else if have("rsvg-convert") {
        std::process::Command::new("rsvg-convert").args(["--zoom", "1.5", "-b", "white", "-o"]).arg(&png).arg(svg).status()
    } else {
        eprintln!("bbgen: no PNG written (needs ImageMagick or rsvg-convert)");
        return None;
    };
    match status {
        Ok(s) if s.success() => Some(png),
        _ => {
            eprintln!("bbgen: the PNG conversion failed");
            None
        }
    }
}

/// re.sub(r"[^\w-]+", "-", group).strip("-").lower() or "group"
fn slug(group: &str) -> String {
    let mut out = String::new();
    let mut gap = false;
    for ch in group.chars() {
        if ch.is_alphanumeric() || ch == '_' || ch == '-' {
            if gap {
                out.push('-');
                gap = false;
            }
            out.push(ch);
        } else {
            gap = true;
        }
    }
    if gap {
        out.push('-');
    }
    let s = out.trim_matches('-').to_lowercase();
    if s.is_empty() { "group".into() } else { s }
}

fn run(args: &[String]) -> i32 {
    let cmd = args[0].as_str();
    let (mut circuit, mut out_dir, mut no_png, mut steps) = (None, None, false, false);
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "-o" | "--out" if cmd == "build" && i + 1 < args.len() => {
                out_dir = Some(PathBuf::from(&args[i + 1]));
                i += 1;
            }
            "--no-png" if cmd == "build" => no_png = true,
            "--steps" if cmd == "build" => steps = true,
            a if !a.starts_with('-') && circuit.is_none() => circuit = Some(PathBuf::from(a)),
            a => {
                eprintln!("bbgen: unrecognised argument {}\n{}", a, USAGE);
                return 2;
            }
        }
        i += 1;
    }
    let circuit = match circuit {
        Some(c) => c,
        None => {
            eprintln!("bbgen: {} needs a circuit file\n{}", cmd, USAGE);
            return 2;
        }
    };
    let c = match Circuit::load(&circuit.to_string_lossy()) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("bbgen: {}", e);
            return 2;
        }
    };
    let result = check(&c);
    report(&result);
    if cmd == "build" {
        let out = clean(&out_dir.unwrap_or_else(|| circuit.parent().map(Path::to_path_buf).unwrap_or_default()));
        if !out.as_os_str().is_empty() {
            std::fs::create_dir_all(&out).ok();
        }
        let stem = circuit.file_stem().unwrap().to_string_lossy().into_owned();
        let mut views: Vec<(PathBuf, Option<String>)> = vec![(out.join(format!("{}.svg", stem)), None)];
        if steps {
            for (k, group) in c.groups.iter().enumerate() {
                views.push((out.join(format!("{}.step{}-{}.svg", stem, k + 1, slug(group))), Some(group.clone())));
            }
        }
        for (svg, highlight) in views {
            let (text, _) = bbgen::render::draw(&c, &result, highlight.as_deref());
            if let Err(e) = std::fs::write(&svg, text) {
                eprintln!("bbgen: {}: {}", svg.display(), e);
                return 2;
            }
            let mut written = vec![svg.display().to_string()];
            if !no_png {
                if let Some(p) = png(&svg) {
                    written.push(p.display().to_string());
                }
            }
            println!("wrote {}", written.join(", "));
        }
        let md = out.join(format!("{}.md", stem));
        if let Err(e) = std::fs::write(&md, bbgen::checklist::checklist(&c, &result)) {
            eprintln!("bbgen: {}: {}", md.display(), e);
            return 2;
        }
        println!("wrote {}", md.display());
    }
    if result.errors() > 0 { 1 } else { 0 }
}

fn find(args: &[String]) -> i32 {
    let mut words = Vec::new();
    let mut kicad: Option<String> = None;
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--kicad" && i + 1 < args.len() {
            kicad = Some(args[i + 1].clone());
            i += 2;
            continue;
        }
        words.push(args[i].clone());
        i += 1;
    }
    if words.is_empty() {
        eprintln!("bbgen: find needs words to look for\n{}", USAGE);
        return 2;
    }
    let roots = bbgen::kicad::library_roots(kicad.as_deref());
    if roots.is_empty() {
        eprintln!("bbgen: no KiCad library found; install KiCad or point BBGEN_KICAD at it");
        return 2;
    }
    let hits = bbgen::kicad::search(&words, &roots, 25);
    for (r, fp, about) in &hits {
        let fp = if fp.is_empty() { "(no footprint: give one)" } else { fp.as_str() };
        println!("{:<44} {:<44} {}", r, fp, about.chars().take(70).collect::<String>());
    }
    if hits.is_empty() {
        println!("nothing found; try fewer or shorter words");
    }
    if hits.is_empty() { 1 } else { 0 }
}

// ---- for checking the port against the Python ----

fn dev(args: &[String]) -> i32 {
    match args.first().map(String::as_str) {
        Some("build") => dev_build(&args[1..]),
        Some("score-lines") => {
            score_lines();
            0
        }
        Some("yaml-json") => {
            let text = std::fs::read_to_string(&args[1]).unwrap();
            match Val::load(&text) {
                Ok(v) => println!("{}", serde_json::to_string(&v.to_json_typed()).unwrap()),
                Err(e) => println!("{}", serde_json::json!({"error": e})),
            }
            0
        }
        Some("pyf-check") => {
            pyf_check();
            0
        }
        _ => {
            eprintln!("usage: bbgen-rs dev {{build FILE [--dump OUT] [--repeat N] | score-lines | yaml-json FILE | pyf-check}}");
            2
        }
    }
}

fn dev_build(args: &[String]) -> i32 {
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
    let r = check(&c);
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
            let r = check(&c);
            std::hint::black_box(bbgen::autoplace::score(&c, &r));
        }
        println!("{:.3} ms per build ({} builds)", start.elapsed().as_secs_f64() * 1000.0 / repeat as f64, repeat);
    }
    if r.errors() > 0 { 1 } else { 0 }
}

/// Lines of {"data": a circuit file as JSON, "source": path}: print each one's score and hash.
fn score_lines() {
    let stdin = std::io::stdin();
    for line in stdin.lock().lines() {
        let j: serde_json::Value = serde_json::from_str(&line.unwrap()).unwrap();
        let data = Val::from_json(&j["data"]);
        match Circuit::new(&data, j["source"].as_str().unwrap_or("circuit.yaml")) {
            Ok(c) => {
                let r = check(&c);
                println!("{} {}", bbgen::pyf::repr(bbgen::autoplace::score(&c, &r)), bbgen::layout_hash(&c));
            }
            Err(e) => println!("error {}", e),
        }
    }
}

fn pyf_check() {
    use bbgen::pyf;
    let stdin = std::io::stdin();
    for line in stdin.lock().lines() {
        let v: Vec<f64> = line.unwrap().split_whitespace().map(|s| s.parse().unwrap()).collect();
        println!("{} {} {} {} {} {}", pyf::repr(pyf::dist((v[0], v[1]), (v[2], v[3]))), pyf::repr(pyf::hypot(v[0] - v[2], v[1] - v[3])),
                 pyf::repr(pyf::round(v[0], 3)), pyf::repr(pyf::round(v[0], 6)), pyf::repr(pyf::fsum(v.iter().copied())), pyf::repr(v[0] * v[1]));
    }
}
