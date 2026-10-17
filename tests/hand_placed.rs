//! Placement around what the file places itself (tests/hand-placed/): a chip bbgen places never gets a pin in a strip
//! the file gives another net, or on a hole the file takes, and when nothing else fits it says so by name.

mod common;

use std::process::Command;

fn run(name: &str) -> (i32, String, Option<serde_json::Value>) {
    let path = common::root().join("tests/hand-placed").join(name);
    let done = Command::new(env!("CARGO_BIN_EXE_bbgen")).arg("check").arg(&path).arg("--json").output().unwrap();
    let out = String::from_utf8_lossy(&done.stdout).into_owned();
    let err = String::from_utf8_lossy(&done.stderr).into_owned();
    (done.status.code().unwrap(), err, serde_json::from_str(&out).ok())
}

fn part<'a>(json: &'a serde_json::Value, r: &str) -> &'a serde_json::Value {
    json["parts"].as_array().unwrap().iter().find(|p| p["ref"] == r).unwrap()
}

/// Builds without errors, every LED's legs where the file puts them.
fn clean_around_leds(name: &str) {
    let (_, err, json) = run(name);
    let json = json.unwrap_or_else(|| panic!("{}: no JSON; {}", name, err));
    let errors: Vec<&serde_json::Value> = json["issues"].as_array().unwrap().iter().filter(|i| i["level"] == "error").collect();
    assert!(errors.is_empty(), "{}: {:?}", name, errors);
    let text = std::fs::read_to_string(common::root().join("tests/hand-placed").join(name)).unwrap();
    let data = bbgen::val::Val::load(&text).unwrap();
    for (r, spec) in data.at("parts").as_map().unwrap() {
        let r = r.py_str();
        if let Some(legs) = spec.get("pins").and_then(|p| p.as_list()) {
            let given: Vec<String> = legs.iter().map(|l| l.py_str()).collect();
            let got: Vec<String> = part(&json, &r)["legs"].as_array().unwrap().iter().map(|l| l.as_str().unwrap().to_string()).collect();
            assert_eq!(given, got, "{}: {} moved", name, r);
        }
    }
}

#[test]
fn chips_keep_out_of_the_strips_of_leds_placed_by_hand() {
    clean_around_leds("knight-rider.yaml");
    clean_around_leds("knight-rider-left.yaml");
}

#[test]
fn a_chip_may_share_a_strip_with_its_own_net() {
    let (code, err, json) = run("same-net-wires.yaml");
    assert_eq!(code, 0, "{}", err);
    let json = json.unwrap();
    let out = part(&json, "U1")["pins"].as_array().unwrap().iter().find(|p| p["name"] == "OUT").unwrap()["hole"].clone();
    let col: i64 = out.as_str().unwrap()[1..].parse().unwrap();
    assert_eq!(col % 3, 0, "OUT in {}: not in a strip the wires reach", out);
}

#[test]
fn no_room_is_an_error_naming_what_is_in_the_way() {
    for (name, first) in [("no-room.yaml", "(L1, L2, L3, L4, L5, L6 and 4 more)"), ("no-room-wires.yaml", "(L1, W1, W2, W3, W4, W5 and 4 more)")] {
        let (code, _, json) = run(name);
        assert_eq!(code, 2, "{}", name);
        let json = json.unwrap();
        let err = json["error"]["message"].as_str().unwrap();
        assert!(err.starts_with("parts.U1: every place for it would short a pin into a strip") && err.contains(first), "{}: {}", name, err);
    }
}
