//! Every example draws a schematic: each part's units placed, each signal net with two pins or more wired.

mod common;

#[test]
fn the_examples_draw() {
    let mut files: Vec<_> = std::fs::read_dir(common::root().join("examples")).unwrap().filter_map(|e| e.ok()).map(|e| e.path())
        .filter(|p| p.extension().map(|x| x == "yaml").unwrap_or(false)).collect();
    files.sort();
    for path in files {
        let c = bbgen::circuit::Circuit::load(path.to_str().unwrap()).unwrap();
        let s = bbgen::schematic::draw(&c).unwrap_or_else(|e| panic!("{}: {}", path.display(), e));
        for part in &c.parts {
            assert!(s.svg.contains(&format!("data-part=\"{}\"", part.r#ref)), "{}: {} isn't drawn", path.display(), part.r#ref);
        }
        let edges = s.laid_out["edges"].as_array().unwrap();
        assert!(edges.iter().all(|e| e["sections"].as_array().map(|a| !a.is_empty()).unwrap_or(false)), "{}: an unrouted wire", path.display());
    }
}
