//! Every part of every example and benchmark has a schematic symbol that carries each of its pins in a net, at a
//! place a wire can be drawn to.

mod common;

use bbgen::symbol;
use bbgen::val::Val;

#[test]
fn every_pin_in_a_net_has_a_place_on_its_symbol() {
    let mut files = common::benchmarks();
    let mut examples: Vec<_> = std::fs::read_dir(common::root().join("examples")).unwrap().filter_map(|e| e.ok()).map(|e| e.path())
        .filter(|p| p.extension().map(|x| x == "yaml").unwrap_or(false)).collect();
    examples.sort();
    files.extend(examples);
    let mut problems: Vec<String> = Vec::new();
    for path in &files {
        let name = path.file_stem().unwrap().to_string_lossy().into_owned();
        let data = Val::load(&std::fs::read_to_string(path).unwrap()).unwrap();
        let library = bbgen::circuit::library_for(&data);
        let mut parts = Vec::new();
        for (r, spec) in data.at("parts").as_map().unwrap() {
            let part = bbgen::parts::make_part(&r.py_str(), spec, library.as_deref()).unwrap();
            let sym = symbol::of_part(&part, library.as_deref());
            if let Some(note) = &sym.note {
                problems.push(format!("{}: {}: {}", name, part.r#ref, note));
            }
            parts.push((part, sym));
        }
        // the pins the nets name: REF.TOKEN, a token naming one pin or several joined ones
        for (net, spec) in data.at("nets").as_map().unwrap() {
            let pins = match spec {
                Val::List(l) => l.clone(),
                _ => spec.at("pins").as_list().cloned().unwrap_or_default(),
            };
            for p in pins {
                let p = p.py_str();
                let (r, token) = p.split_once('.').unwrap();
                let (part, sym) = parts.iter().find(|(part, _)| part.r#ref == r).unwrap();
                for i in part.find(token) {
                    let carriers: Vec<(usize, &symbol::SymPin)> = sym.drawing.units.iter().enumerate()
                        .flat_map(|(u, unit)| unit.pins.iter().filter(move |sp| sp.pins.contains(&i)).map(move |sp| (u, sp))).collect();
                    if carriers.is_empty() {
                        problems.push(format!("{}: {} (net {}) has no pin on {}'s symbol", name, part.pin_str(i), net.py_str(), r));
                    }
                    // a hidden pin is drawn to through the visible pin it's stacked on
                    for (u, sp) in carriers.iter().filter(|(_, sp)| sp.hidden) {
                        let visible = sym.drawing.units[*u].pins.iter().any(|q| !q.hidden && q.at == sp.at);
                        if !visible {
                            problems.push(format!("{}: {} (net {}) is a hidden pin with nothing drawn where it attaches",
                                                  name, part.pin_str(i), net.py_str()));
                        }
                    }
                }
            }
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

#[test]
fn turned_text_reads_left_to_right_or_upwards() {
    let part = bbgen::parts::make_part("R1", &Val::load("{type: resistor, value: 10k}").unwrap(), None).unwrap();
    let sym = symbol::of_part(&part, None);
    for rot in [0, 90, 180, 270] {
        for mirror in [false, true] {
            let prims = symbol::prims(&sym, 0, symbol::Orient { rot, mirror }, (0.0, 0.0));
            let texts: Vec<&symbol::Prim> = prims.iter().filter(|p| matches!(p, symbol::Prim::Text { .. })).collect();
            assert_eq!(texts.len(), 2, "R1's ref and value at rot {} mirror {}", rot, mirror);
            // turned a half turn, the text is still on the same side of the body as the symbol says (right of it)
            if rot == 180 && !mirror {
                for t in texts {
                    if let symbol::Prim::Text { at, anchor, .. } = t {
                        assert!(at.0 < 0.0 && *anchor == "end", "{:?}", t);
                    }
                }
            }
        }
    }
}
