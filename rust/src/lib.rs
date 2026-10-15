//! bbgen in Rust: breadboard layouts from YAML. A port of the Python package, kept to the same results.
pub mod autoplace;
pub mod autowire;
pub mod board;
pub mod check;
pub mod checklist;
pub mod circuit;
pub mod costs;
pub mod dump;
pub mod kicad;
pub mod legs;
pub mod measure;
pub mod parts;
pub mod pyf;
pub mod pyrand;
pub mod render;
pub mod search;
pub mod trace;
pub mod val;
pub mod yaml;

use circuit::Circuit;

/// Maps and sets hashed with foldhash, seeded the same every run (positions as keys hash badly with FxHash:
/// their low bits are mostly zero).
pub type Hasher = foldhash::fast::FixedState;
pub type HashMap<K, V> = std::collections::HashMap<K, V, Hasher>;
pub type HashSet<K> = std::collections::HashSet<K, Hasher>;
/// An insertion-ordered map (Python's dict).
pub type IndexMap<K, V> = indexmap::IndexMap<K, V, Hasher>;

/// benchmarks/hotspots.py's layout_hash: every part's holes and every wire, as Python's repr, hashed.
pub fn layout_hash(c: &Circuit) -> String {
    use md5::{Digest, Md5};
    use val::repr_str;
    let tuple = |items: Vec<String>| if items.len() == 1 { format!("({},)", items[0]) } else { format!("({})", items.join(", ")) };
    let mut parts: Vec<(String, Vec<String>)> = c.parts.iter().map(|p| {
        (p.r#ref.clone(), p.pins.iter().map(|x| x.hole.map(|h| c.hole_name(h)).unwrap_or("-".into())).collect())
    }).collect();
    parts.sort();
    let end = |e: &circuit::End| match e.hole {
        Some(h) => c.hole_name(h),
        None => c.pin_str(e.pin.unwrap()),
    };
    let mut wires: Vec<(String, String, String, bool)> = c.wires.iter()
        .map(|w| (w.group.clone(), end(w.a.as_ref().unwrap()), end(w.b.as_ref().unwrap()), w.flex)).collect();
    wires.sort();
    let p = parts.iter().map(|(r, holes)| tuple(vec![repr_str(r), tuple(holes.iter().map(|h| repr_str(h)).collect())])).collect::<Vec<_>>();
    let w = wires.iter().map(|(g, a, b, f)| tuple(vec![repr_str(g), repr_str(a), repr_str(b), if *f { "True".into() } else { "False".into() }]))
        .collect::<Vec<_>>();
    let text = format!("([{}], [{}])", p.join(", "), w.join(", "));
    let digest = Md5::digest(text.as_bytes());
    digest.iter().map(|b| format!("{:02x}", b)).collect::<String>()[..10].to_string()
}
