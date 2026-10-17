//! Every action the workflows use is pinned to a commit. Whoever controls an action can move its tag (`@v4`) or
//! branch (`@stable`) to other code, and the release workflow builds the binaries everyone installs.

mod common;

#[test]
fn actions_are_pinned_to_commits() {
    let mut seen = 0;
    let mut loose = Vec::new();
    for entry in std::fs::read_dir(common::root().join(".github/workflows")).unwrap() {
        let path = entry.unwrap().path();
        let text = std::fs::read_to_string(&path).unwrap();
        for (i, line) in text.lines().enumerate() {
            let Some(at) = line.find("uses:").filter(|_| !line.trim_start().starts_with('#')) else { continue };
            let action = line[at + 5..].split(" #").next().unwrap().trim().trim_matches(['"', '\'']);
            seen += 1;
            // a local action is this repo's own code
            if action.starts_with("./") {
                continue;
            }
            let sha = action.rsplit_once('@').map(|(_, r)| r).unwrap_or_default();
            if sha.len() != 40 || !sha.chars().all(|c| c.is_ascii_hexdigit()) {
                loose.push(format!("{}:{}: {}", path.file_name().unwrap().to_string_lossy(), i + 1, action));
            }
        }
    }
    assert!(seen > 0, "no actions found in .github/workflows");
    assert!(loose.is_empty(), "pin these to a commit (`uses: owner/action@<sha> # v1.2.3`):\n{}", loose.join("\n"));
}
