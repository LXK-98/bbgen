//! Every example's and benchmark's schematic, in both styles: drawn without a problem when read back (each wired net
//! in one piece, nothing of one net on another), and as recorded in benchmarks/golden/schematics/ (the wires style
//! for the examples only). A change meant to move them
//! fails here by design: look at the new drawings, then record them with `BBGEN_RECORD=1 cargo test --release
//! --test schematic`.

mod common;

use std::path::PathBuf;

/// (name, file): the examples, and the benchmarks as placement left them (the schematic depends only on the parts
/// and nets, and these load without placing).
fn circuits() -> Vec<(String, PathBuf)> {
    let mut out = Vec::new();
    for dir in ["examples", "benchmarks/golden/placed"] {
        let mut files: Vec<PathBuf> = std::fs::read_dir(common::root().join(dir)).unwrap().filter_map(|e| e.ok()).map(|e| e.path())
            .filter(|p| p.extension().map(|x| x == "yaml").unwrap_or(false)).collect();
        files.sort();
        out.extend(files.into_iter().map(|p| (p.file_stem().unwrap().to_string_lossy().into_owned(), p)));
    }
    out
}

#[test]
fn schematics_read_back_right_and_are_as_recorded() {
    let record = std::env::var("BBGEN_RECORD").is_ok();
    let dir = common::root().join("benchmarks/golden/schematics");
    if record {
        std::fs::create_dir_all(&dir).unwrap();
    }
    let all = circuits();
    let results = common::parallel(&all, |(name, path)| {
        use bbgen::schematic::Style;
        let c = bbgen::circuit::Circuit::load(path.to_str().unwrap()).unwrap();
        let mut problems: Vec<String> = Vec::new();
        // both styles drawn right; the wires style recorded for the examples (it's meant for small circuits)
        let example = path.starts_with(common::root().join("examples"));
        for (style, suffix) in [(Style::Symbols, ""), (Style::Wires, ".wires")] {
            let s = bbgen::schematic::draw_with(&c, style).unwrap_or_else(|e| panic!("{}{}: {}", name, suffix, e));
            problems.extend(s.problems.iter().map(|p| format!("{}{}: {}", name, suffix, p)));
            for part in &c.parts {
                if !s.svg.contains(&format!("data-part=\"{}\"", part.r#ref)) {
                    problems.push(format!("{}{}: {} isn't drawn", name, suffix, part.r#ref));
                }
            }
            if style == Style::Wires && !example {
                continue;
            }
            let page = bbgen::schematic::page(&s);
            let file = dir.join(format!("{}{}.svg", name, suffix));
            if record {
                std::fs::write(&file, &page).unwrap();
            } else if std::fs::read_to_string(&file).ok().as_deref() != Some(page.as_str()) {
                problems.push(format!("{}{}: the schematic isn't as recorded in {}", name, suffix, file.display()));
            }
        }
        problems
    });
    let problems: Vec<String> = results.into_iter().flatten().collect();
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// What `bbgen build --schematic` draws, from the file's parts and nets without placing it (while placement runs),
/// is the schematic of the placed circuit.
#[test]
fn drawn_from_the_netlist_as_from_the_placed_circuit() {
    let mut files: Vec<PathBuf> = common::benchmarks();
    files.extend(circuits().into_iter().filter(|(_, p)| p.starts_with(common::root().join("examples"))).map(|(_, p)| p));
    let dir = common::root().join("benchmarks/golden/schematics");
    let problems: Vec<String> = common::parallel(&files, |path| {
        let name = path.file_stem().unwrap().to_string_lossy().into_owned();
        let data = bbgen::val::Val::load(&std::fs::read_to_string(path).unwrap()).unwrap();
        let c = bbgen::circuit::Circuit::netlist(&data, path.to_str().unwrap()).unwrap();
        let page = bbgen::schematic::page(&bbgen::schematic::draw(&c).unwrap());
        (std::fs::read_to_string(dir.join(format!("{}.svg", name))).ok().as_deref() != Some(page.as_str()))
            .then(|| format!("{}: drawn from its netlist, the schematic isn't as recorded", name))
    }).into_iter().flatten().collect();
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}
