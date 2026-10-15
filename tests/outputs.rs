//! `bbgen build` on the placed benchmarks, the examples and the circuits the old unit tests built (many of them
//! broken on purpose): the same report, exit status, checklist and drawing as the Python bbgen gave
//! (benchmarks/golden/expected/: its checklists and reports in full, its drawings' SHA-256).

mod common;

use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

fn files(group: &str) -> Vec<(String, PathBuf)> {
    let dir = match group {
        "examples" => common::root().join("examples"),
        g => common::root().join("benchmarks/golden").join(g),
    };
    let mut v: Vec<(String, PathBuf)> = std::fs::read_dir(dir).unwrap().filter_map(|e| e.ok()).map(|e| e.path())
        .filter(|p| p.extension().map(|x| x == "yaml").unwrap_or(false)).map(|p| (group.to_string(), p)).collect();
    v.sort();
    v
}

fn check(group: &str, path: &Path) -> Vec<String> {
    let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
    let exp = common::root().join("benchmarks/golden/expected").join(group);
    let tmp = std::env::temp_dir().join(format!("bbgen-test-{}-{}-{}", std::process::id(), group, stem));
    let done = std::process::Command::new(env!("CARGO_BIN_EXE_bbgen")).arg("build").arg(path).arg("-o").arg(&tmp).arg("--no-png")
        .output().unwrap();
    let mut problems = Vec::new();
    let printed = format!("{}{}", String::from_utf8_lossy(&done.stdout), String::from_utf8_lossy(&done.stderr))
        .replace(&tmp.to_string_lossy().to_string(), "OUT") + &format!("exit {}\n", done.status.code().unwrap());
    let want = std::fs::read_to_string(exp.join(format!("{}.out", stem))).unwrap();
    if printed != want {
        problems.push(format!("{}/{}: printed\n{}\ninstead of\n{}", group, stem, printed, want));
    }
    let md = exp.join(format!("{}.md", stem));
    if md.exists() && std::fs::read_to_string(&md).unwrap() != std::fs::read_to_string(tmp.join(format!("{}.md", stem))).unwrap_or_default() {
        problems.push(format!("{}/{}: the checklist differs", group, stem));
    }
    let sha = exp.join(format!("{}.svg.sha256", stem));
    if sha.exists() {
        let svg = std::fs::read(tmp.join(format!("{}.svg", stem))).unwrap_or_default();
        let got = Sha256::digest(&svg).iter().map(|b| format!("{:02x}", b)).collect::<String>();
        if got != std::fs::read_to_string(&sha).unwrap().trim() {
            problems.push(format!("{}/{}: the drawing differs", group, stem));
        }
    }
    std::fs::remove_dir_all(&tmp).ok();
    problems
}

#[test]
fn builds_print_and_write_what_they_did() {
    let all: Vec<(String, PathBuf)> = ["placed", "examples", "cases"].iter().flat_map(|g| files(g)).collect();
    assert_eq!(all.len(), 138);
    let problems: Vec<String> = common::parallel(&all, |(g, p)| check(g, p)).into_iter().flatten().collect();
    assert!(problems.is_empty(), "\n{}", problems.join("\n"));
}
