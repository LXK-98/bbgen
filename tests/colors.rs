//! A colour in a circuit file is a name or `#rrggbb`; anything else is the file's error, never markup in the
//! drawing. bbgen studio puts the drawing in its page as it is, so a colour that closed its attribute could run script
//! there. Names and text are escaped in the drawing whatever they hold.

use bbgen::circuit::Circuit;
use bbgen::val::Val;

const MARKUP: &str = r##""#000\"/><img src=x onerror=\"alert(1)\">""##;

/// A circuit whose parts are already placed (no search), and one bbgen has to place.
fn placed(extra_wire: &str, net_color: &str, part_color: &str) -> String {
    format!("parts:\n  Pico: {{type: pico, at: c3}}\n  R1: {{type: resistor, value: 330R, pins: [Pico.GP15, a26]{part_color}}}\n  \
             D1: {{type: led, pins: [R1.2, bot-]}}\nwires:\n  - [Pico.3, bot-{extra_wire}]\n\
             nets:\n  GND: {{pins: [Pico.GND, D1.K]{net_color}}}\n  LED_DRIVE: [Pico.GP15, R1.1]\n  LED_A: [R1.2, D1.A]\n")
}

fn unplaced(extra_wire: &str, net_color: &str, part_color: &str) -> String {
    format!("parts:\n  R1: {{type: resistor, value: 330R{part_color}}}\n  D1: {{type: led}}\nwires:\n  - [j1, j5{extra_wire}]\n\
             nets:\n  A: {{pins: [R1.2, D1.A]{net_color}}}\n  B: [R1.1, D1.K]\n")
}

fn build(yaml: &str) -> Result<Circuit, String> {
    Circuit::new(&Val::load(yaml).unwrap(), "colors.yaml")
}

#[test]
fn a_colour_that_isnt_one_is_an_error() {
    for circuit in [placed, unplaced] {
        let wire = circuit(&format!(", {MARKUP}"), "", "");
        let net = circuit("", &format!(", color: {MARKUP}"), "");
        let part = circuit("", "", &format!(", color: {MARKUP}"));
        let short = circuit(", '#abc'", "", "");
        for (what, yaml) in [("wires", wire), ("nets", net), ("parts", part), ("wires", short)] {
            let err = build(&yaml).err().unwrap_or_else(|| panic!("built with a bad colour:\n{yaml}"));
            assert!(err.starts_with(what) && err.contains("unknown color"), "{err}");
        }
    }
}

#[test]
fn names_and_hex_colours_still_draw() {
    for circuit in [placed, unplaced] {
        let yaml = circuit(", Purple", ", color: '#1A2b3C'", ", color: grey");
        let c = build(&yaml).unwrap();
        let (svg, _) = bbgen::render::draw(&c, &bbgen::check::check(&c), None);
        assert!(svg.contains("#1A2b3C"));
    }
}

#[test]
fn text_is_escaped_in_the_drawing() {
    let yaml = "title: '<img src=x onerror=alert(1)>'\nparts:\n  R1: {type: resistor, value: 330R, title: '\"/><b>'}\n  \
                D1: {type: led}\nnets:\n  '\"><img src=x>': [R1.2, D1.A]\n  B: [R1.1, D1.K]\n";
    let c = build(yaml).unwrap();
    let r = bbgen::check::check(&c);
    for svg in [bbgen::render::draw(&c, &r, None).0, bbgen::render::draw_annotated(&c, &r).0] {
        assert!(!svg.contains("<img") && !svg.contains("<b>"), "{svg}");
    }
}
