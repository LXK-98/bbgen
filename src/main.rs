//! bbgen: breadboard layouts from YAML: a drawing, a build checklist and a wiring check.
//!
//!     bbgen build circuit.yaml [-o DIR] [--no-png] [--steps] [--bare] [--video FILE] [--video-all]
//!     bbgen check circuit.yaml                                  the check only
//!     bbgen build|check circuit.yaml --json                     what was built, as JSON on stdout (docs/scene-format.md)
//!     bbgen find WORDS... [--kicad DIR]                         search KiCad's library for a symbol
//!
//! For bbgen's own tests and benchmarks (benchmarks/README.md):
//!
//!     bbgen dev build FILE [--dump OUT.jsonl] [--repeat N]     build, record its events, time it
//!     bbgen dev measure FILE OUTDIR [--no-png]                 what benchmarks/run.py tabulates
//!     bbgen dev frames FILE OUTDIR [--all]                     a video's frames as SVG files
//!     bbgen dev symbols FILE OUT.svg [--rot N] [--mirror]      every part's schematic symbol, in a grid
//!     bbgen dev schematic FILE OUT.svg                         the circuit's schematic, and the graph ELK laid out
//!     bbgen dev elk GRAPH.json [OUT.json] [--repeat N]         a graph laid out by ELK, and how long it took
//!     bbgen dev kicad-roots | score-lines | yaml-json FILE | pyf-check

use bbgen::check::{check, Result};
use bbgen::circuit::Circuit;
use bbgen::val::Val;
use std::io::BufRead;
use std::path::{Component, Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

const USAGE: &str = "usage: bbgen {build,check,find} ...\n\n\
  build circuit.yaml [-o DIR] [--no-png] [--steps] [--bare] [--video FILE] [--video-all]\n\
                                                    write <name>.svg/.png/.md next to the YAML and check the wiring\n\
                                                    (--bare: the drawing without title, legend or background)\n\
  check circuit.yaml                                check the wiring only\n\
  build|check circuit.yaml --json                   print what was built as JSON (no files unless -o)\n\
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
    let (mut video, mut video_all): (Option<PathBuf>, bool) = (None, false);
    let mut json = false;
    let mut bare = false;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "-o" | "--out" if cmd == "build" && i + 1 < args.len() => {
                out_dir = Some(PathBuf::from(&args[i + 1]));
                i += 1;
            }
            "--no-png" if cmd == "build" => no_png = true,
            "--steps" if cmd == "build" => steps = true,
            "--video" if cmd == "build" && i + 1 < args.len() => {
                video = Some(PathBuf::from(&args[i + 1]));
                i += 1;
            }
            "--video-all" if cmd == "build" => video_all = true,
            "--json" => json = true,
            "--bare" if cmd == "build" => bare = true,
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
    if json {
        return run_json(cmd, &circuit, out_dir.is_some().then(|| (out_dir.unwrap(), no_png)));
    }
    if video.is_some() {
        bbgen::trace::start(video_all);
    }
    let c = match Circuit::load(&circuit.to_string_lossy()) {
        Ok(c) => c,
        Err(e) => {
            bbgen::trace::finish();
            eprintln!("bbgen: {}", e);
            return 2;
        }
    };
    let result = check(&c);
    if video.is_some() {
        let status = format!("{} errors, {} warnings", result.errors(), result.warnings());
        bbgen::trace::step(&c, &format!("done: {}", status), true, &[], Some(bbgen::trace::LAST_SECONDS), false);
    }
    let frames = bbgen::trace::finish();
    report(&result);
    if let Some(v) = &video {
        match bbgen::trace::write_video(&frames, v) {
            Ok(()) => println!("wrote {} ({} frames)", v.display(), frames.len()),
            Err(e) => eprintln!("bbgen: no video written: {}", e),
        }
    }
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
            let (text, _) = if bare {
                bbgen::render::draw_bare(&c, &result, highlight.as_deref())
            } else {
                bbgen::render::draw(&c, &result, highlight.as_deref())
            };
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

/// build|check --json: the scene on stdout, nothing else. With -o, the drawing and checklist are written there too.
fn run_json(cmd: &str, circuit: &Path, files: Option<(PathBuf, bool)>) -> i32 {
    let path = circuit.to_string_lossy().into_owned();
    let start = Instant::now();
    let c = match Circuit::load(&path) {
        Ok(c) => c,
        Err(e) => {
            println!("{}", bbgen::scene::error(&e));
            return 2;
        }
    };
    let result = check(&c);
    let ms = start.elapsed().as_secs_f64() * 1000.0;
    let data = std::fs::read_to_string(&path).ok().and_then(|t| Val::load(&t).ok());
    println!("{}", bbgen::scene::scene(&c, &result, data.as_ref(), ms));
    if let (Some((out, no_png)), "build") = (files, cmd) {
        let out = clean(&out);
        if !out.as_os_str().is_empty() {
            std::fs::create_dir_all(&out).ok();
        }
        let stem = circuit.file_stem().unwrap().to_string_lossy().into_owned();
        let svg = out.join(format!("{}.svg", stem));
        let written = std::fs::write(&svg, bbgen::render::draw(&c, &result, None).0)
            .and_then(|_| std::fs::write(out.join(format!("{}.md", stem)), bbgen::checklist::checklist(&c, &result)));
        if let Err(e) = written {
            eprintln!("bbgen: {}: {}", out.display(), e);
            return 2;
        }
        if !no_png {
            png(&svg);
        }
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

// ---- for bbgen's own tests and benchmarks ----

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
        Some("measure") => dev_measure(&args[1..]),
        Some("symbols") if args.len() >= 3 => dev_symbols(&args[1..]),
        Some("elk") if args.len() >= 2 => dev_elk(&args[1..]),
        Some("schematic") if args.len() >= 3 => dev_schematic(&args[1..]),
        Some("kicad-roots") => {
            // the KiCad library folders parts are read from, one a line
            for r in bbgen::kicad::library_roots(None) {
                println!("{}", r.display());
            }
            0
        }
        Some("frames") => {
            // dev frames FILE OUTDIR [--all]: a video's frames as SVG files (00000.svg ...), for comparing
            bbgen::trace::start(args.iter().any(|a| a == "--all"));
            let c = match Circuit::load(&args[1]) {
                Ok(c) => c,
                Err(e) => { eprintln!("{}", e); return 2; }
            };
            let r = check(&c);
            let status = format!("{} errors, {} warnings", r.errors(), r.warnings());
            bbgen::trace::step(&c, &format!("done: {}", status), true, &[], Some(bbgen::trace::LAST_SECONDS), false);
            let frames = bbgen::trace::finish();
            std::fs::create_dir_all(&args[2]).unwrap();
            for (k, (svg, secs)) in frames.iter().enumerate() {
                std::fs::write(Path::new(&args[2]).join(format!("{:05}.svg", k)), svg).unwrap();
                println!("{:05} {}", k, bbgen::pyf::repr(*secs));
            }
            0
        }
        _ => {
            eprintln!("usage: bbgen dev {{build FILE [--dump OUT] [--repeat N] | measure FILE OUTDIR [--no-png] | frames FILE OUTDIR [--all] | symbols FILE OUT.svg [--rot N] [--mirror] | elk GRAPH.json [OUT.json] [--repeat N] | schematic FILE OUT.svg | kicad-roots | score-lines | yaml-json FILE | pyf-check}}");
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

/// dev measure FILE OUTDIR [--no-png]: build a benchmark, write its drawing and checklist (and PNG) to OUTDIR,
/// and print what benchmarks/run.py tabulates, as JSON, with the file's own `expect:`.
fn dev_measure(args: &[String]) -> i32 {
    use bbgen::measure as m;
    use serde_json::json;
    let (path, out) = (&args[0], Path::new(&args[1]));
    let want_png = !args.iter().any(|a| a == "--no-png");
    let expect = std::fs::read_to_string(path).ok().and_then(|t| Val::load(&t).ok())
        .map(|d| d.at("expect").to_json()).unwrap_or(serde_json::Value::Null);
    let start = Instant::now();
    let c = match Circuit::load(path) {
        Ok(c) => c,
        Err(e) => {
            let secs = bbgen::pyf::round(start.elapsed().as_secs_f64(), 2);
            println!("{}", json!({"status": format!("CircuitError: {}", e), "seconds": secs, "expect": expect}));
            return 0;
        }
    };
    let r = check(&c);
    let seconds = start.elapsed().as_secs_f64();
    let mut board = 0.0f64;
    for w in &c.wires {
        if w.on_board() && !w.flex {
            let pts = c.wire_points(w);
            board += bbgen::pyf::fsum(pts.windows(2).map(|s| bbgen::pyf::dist(s[0], s[1])));
        }
    }
    let (svg, canvas) = bbgen::render::draw(&c, &r, None);
    let keep = |who: &str| who.ends_with("cable") || who.ends_with("Cable") || who.ends_with("Keep Out");
    let cols: Vec<i32> = c.occupied.iter().filter(|(h, who)| !h.is_rail() && !keep(who)).map(|(h, _)| h.col).collect();
    let mut codes: Vec<(&str, i64)> = Vec::new();
    for i in &r.issues {
        match codes.iter_mut().find(|(k, _)| *k == i.code) {
            Some(slot) => slot.1 += 1,
            None => codes.push((i.code, 1)),
        }
    }
    std::fs::create_dir_all(out).ok();
    let stem = Path::new(path).file_stem().unwrap().to_string_lossy().into_owned();
    let svg_path = out.join(format!("{}.svg", stem));
    std::fs::write(&svg_path, svg).unwrap();
    std::fs::write(out.join(format!("{}.md", stem)), bbgen::checklist::checklist(&c, &r)).unwrap();
    if want_png {
        png(&svg_path);
    }
    let codes: serde_json::Map<String, serde_json::Value> = codes.into_iter().map(|(k, v)| (k.to_string(), json!(v))).collect();
    let num = |x: f64| bbgen::dump::num(x);
    println!("{}", json!({
        "status": "ok", "seconds": num(bbgen::pyf::round(seconds, 2)), "parts": c.parts.len(), "wires": c.wires.len(),
        "errors": r.errors(), "warnings": r.warnings(), "codes": codes,
        "slanted": m::slanted(&c), "bent": m::bent(&c), "crossings": m::crossings(&c), "lead_crossings": m::lead_crossings(&c),
        "flex": c.wires.iter().filter(|w| w.flex).count(), "rail_links_inside": m::rail_links_inside(&c),
        "label_clashes": m::label_clashes(&c, &canvas), "board_wire": num(bbgen::pyf::round(board, 1)),
        "flying": num(bbgen::pyf::round(m::flying_length(&c), 1)),
        "columns": if cols.is_empty() { 0 } else { cols.iter().max().unwrap() - cols.iter().min().unwrap() + 1 },
        "keepout_holes": c.occupied.values().filter(|w| keep(w)).count(),
        "score": num(bbgen::pyf::round(bbgen::autoplace::score(&c, &r), 1)),
        "expect": expect,
    }));
    0
}

/// dev symbols FILE OUT.svg [--rot N] [--mirror]: every part's symbol (each unit of one with several) in a grid,
/// turned by N degrees (and mirrored), with the PNG beside it; and which of a part's pins its symbol doesn't carry.
fn dev_symbols(args: &[String]) -> i32 {
    use bbgen::symbol::{self, Orient, PX};
    let (path, out) = (&args[0], Path::new(&args[1]));
    let rot = args.iter().position(|a| a == "--rot").and_then(|i| args.get(i + 1)).and_then(|r| r.parse().ok()).unwrap_or(0);
    let mirror = args.iter().any(|a| a == "--mirror");
    let data = match std::fs::read_to_string(path).map_err(|e| e.to_string()).and_then(|t| Val::load(&t)) {
        Ok(d) => d,
        Err(e) => { eprintln!("{}: {}", path, e); return 2; }
    };
    let library = bbgen::circuit::library_for(&data);
    let mut cells: Vec<(String, Vec<symbol::Prim>, (f64, f64, f64, f64))> = Vec::new();
    for (r, spec) in data.at("parts").as_map().map(|m| m.as_slice()).unwrap_or(&[]) {
        let r = r.py_str();
        let part = match bbgen::parts::make_part(&r, spec, library.as_deref()) {
            Ok(p) => p,
            Err(e) => { eprintln!("{}", e); return 2; }
        };
        let sym = symbol::of_part(&part, library.as_deref());
        if let Some(note) = &sym.note {
            println!("{}: {}", r, note);
        }
        if !sym.unmapped.is_empty() {
            let pins: Vec<String> = sym.unmapped.iter().map(|&i| part.pin_str(i)).collect();
            println!("{}: no pin on its symbol for {}", r, pins.join(", "));
        }
        for u in 0..sym.drawing.units.len() {
            let prims = symbol::prims(&sym, u, Orient { rot, mirror }, (0.0, 0.0));
            let b = symbol::bounds(&prims);
            cells.push((format!("{} · {}", sym.unit_ref(u), sym.source), prims, b));
        }
    }
    // rows of cells, each its symbol's size plus room for its caption
    let (pad, caption, row_width) = (4.0, 4.0, 250.0);
    let (mut x, mut y, mut row_h, mut width) = (pad, pad, 0.0f64, 0.0f64);
    let mut body = String::new();
    for (name, prims, b) in &cells {
        let (w, h) = (b.2 - b.0, b.3 - b.1);
        let cell_w = w.max(name.chars().count() as f64 * 1.0 * 0.55) + 2.0 * pad;
        if x > pad && x + cell_w > row_width {
            x = pad;
            y += row_h;
            row_h = 0.0;
        }
        let (ox, oy) = (x + pad - b.0, y + caption + pad - b.1);
        body.push_str(&format!("<text x=\"{:.1}\" y=\"{:.1}\" font-size=\"8\" fill=\"{}\">{}</text>",
                               (x + pad) * PX, (y + caption) * PX, bbgen::render::MUTED, symbol::escape(name)));
        body.push_str(&format!("<g transform=\"translate({:.2} {:.2})\">{}</g>", ox * PX, oy * PX, symbol::svg(prims)));
        x += cell_w;
        width = width.max(x);
        row_h = row_h.max(h + caption + 2.0 * pad);
    }
    let height = y + row_h + pad;
    let svg = format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w:.0}\" height=\"{h:.0}\" viewBox=\"0 0 {w:.0} {h:.0}\" \
                       font-family=\"{}\" color=\"{}\"><rect width=\"100%\" height=\"100%\" fill=\"white\"/>{}</svg>\n",
                      bbgen::render::FONT, bbgen::render::INK, body, w = (width + pad) * PX, h = height * PX);
    if let Some(dir) = out.parent() {
        std::fs::create_dir_all(dir).ok();
    }
    std::fs::write(out, svg).unwrap();
    png(out);
    0
}

/// dev elk GRAPH.json [OUT.json] [--repeat N]: a graph in ELK's JSON laid out by ELK (to OUT.json, or stdout), and how
/// long loading ELK and the layout took (and N more layouts of it, on average).
fn dev_elk(args: &[String]) -> i32 {
    let repeat: usize = args.iter().position(|a| a == "--repeat").and_then(|i| args.get(i + 1)).and_then(|n| n.parse().ok()).unwrap_or(0);
    let out_path = args.get(1).filter(|a| !a.starts_with("--"));
    let graph: serde_json::Value = match std::fs::read_to_string(&args[0]).map_err(|e| e.to_string())
        .and_then(|t| serde_json::from_str(&t).map_err(|e| e.to_string())) {
        Ok(g) => g,
        Err(e) => { eprintln!("{}: {}", args[0], e); return 2; }
    };
    let start = Instant::now();
    if let Err(e) = bbgen::elk::ready() {
        eprintln!("{}", e);
        return 2;
    }
    let loaded = start.elapsed();
    let start = Instant::now();
    let out = match bbgen::elk::layout(&graph) {
        Ok(g) => g,
        Err(e) => { eprintln!("{}", e); return 1; }
    };
    eprintln!("ELK loaded in {:.1} ms, laid out in {:.1} ms", loaded.as_secs_f64() * 1000.0, start.elapsed().as_secs_f64() * 1000.0);
    if repeat > 0 {
        let start = Instant::now();
        for _ in 0..repeat {
            std::hint::black_box(bbgen::elk::layout(&graph).unwrap());
        }
        eprintln!("then {:.1} ms a layout ({} more)", start.elapsed().as_secs_f64() * 1000.0 / repeat as f64, repeat);
    }
    let text = serde_json::to_string_pretty(&out).unwrap() + "\n";
    match out_path {
        Some(path) => std::fs::write(path, text).unwrap(),
        None => print!("{}", text),
    }
    0
}

/// dev schematic FILE OUT.svg: the circuit's schematic (and its PNG), with the graph given to ELK and what came back
/// beside it (OUT.elk.json, OUT.laid.json), for ELK's online viewer.
fn dev_schematic(args: &[String]) -> i32 {
    let out = Path::new(&args[1]);
    let c = match Circuit::load(&args[0]) {
        Ok(c) => c,
        Err(e) => { eprintln!("{}: {}", args[0], e); return 2; }
    };
    let start = Instant::now();
    let s = match bbgen::schematic::draw(&c) {
        Ok(s) => s,
        Err(e) => { eprintln!("{}", e); return 1; }
    };
    eprintln!("schematic in {:.0} ms; labels: {}", start.elapsed().as_secs_f64() * 1000.0,
              if s.labelled.is_empty() { "none".into() } else { s.labelled.join(" ") });
    for p in &s.problems {
        eprintln!("problem: {}", p);
    }
    if let Some(dir) = out.parent() {
        std::fs::create_dir_all(dir).ok();
    }
    std::fs::write(out, bbgen::schematic::page(&s)).unwrap();
    std::fs::write(out.with_extension("elk.json"), serde_json::to_string_pretty(&s.graph).unwrap()).unwrap();
    std::fs::write(out.with_extension("laid.json"), serde_json::to_string_pretty(&s.laid_out).unwrap()).unwrap();
    png(out);
    0
}
