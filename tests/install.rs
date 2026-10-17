//! packaging/install.sh run on its own, as `curl ... | sh` runs it: it installs a release only once the download
//! matches SHA256SUMS, and won't install one it can't check. The release is a stand-in whose install.sh only says
//! what it was given, so nothing gets installed.
#![cfg(all(target_os = "linux", target_arch = "x86_64"))]

mod common;

use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::process::Command;

const PACKAGE: &str = "bbgen-x86_64-linux.tar.gz";

/// Where `tool` is on this machine's PATH.
fn find(tool: &str) -> Option<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?).map(|d| d.join(tool)).find(|p| p.is_file())
}

/// A folder for one case, empty: the release, the tools install.sh may use, the prefix it's given.
fn scratch(case: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("bbgen-install-{}-{}", std::process::id(), case));
    std::fs::remove_dir_all(&dir).ok();
    std::fs::create_dir_all(dir.join("release/bbgen-0.0.0-x86_64-linux")).unwrap();
    std::fs::create_dir_all(dir.join("tools")).unwrap();
    dir
}

/// The stand-in release's download folder, `dir/release`: the package and its SHA256SUMS.
fn release(dir: &Path) -> PathBuf {
    let rel = dir.join("release");
    std::fs::write(rel.join("bbgen-0.0.0-x86_64-linux/install.sh"), "echo \"the package's install.sh, given: $*\"\n").unwrap();
    let tar = Command::new("tar").args(["-czf", PACKAGE, "bbgen-0.0.0-x86_64-linux"]).current_dir(&rel).status().unwrap();
    assert!(tar.success());
    let sum = Sha256::digest(std::fs::read(rel.join(PACKAGE)).unwrap()).iter().map(|b| format!("{:02x}", b)).collect::<String>();
    std::fs::write(rel.join("SHA256SUMS"), format!("{}  {}\n", sum, PACKAGE)).unwrap();
    rel
}

/// install.sh with only `checkers` of the checksum tools to hand (and what else it needs): what it printed, and
/// whether it succeeded.
fn install(dir: &Path, checkers: &[&str]) -> (String, bool) {
    let tools = dir.join("tools");
    for tool in ["sh", "curl", "mktemp", "uname", "grep", "tar", "gzip", "rm", "mkdir", "dirname"].iter().chain(checkers) {
        let at = find(tool).unwrap_or_else(|| panic!("the installer test needs {tool}"));
        std::os::unix::fs::symlink(at, tools.join(tool)).unwrap();
    }
    let out = Command::new(tools.join("sh")).arg(common::root().join("packaging/install.sh"))
        .args(["--prefix".as_ref(), dir.join("prefix").as_os_str(), "--no-skill".as_ref()])
        .env("PATH", &tools).env("BBGEN_RELEASE_URL", format!("file://{}", dir.join("release").display()))
        .output().unwrap();
    let printed = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    std::fs::remove_dir_all(dir).ok();
    (printed, out.status.success())
}

#[test]
fn a_download_that_matches_is_installed() {
    let dir = scratch("matches");
    release(&dir);
    let (printed, ok) = install(&dir, &["sha256sum"]);
    assert!(ok && printed.contains("the package's install.sh, given: --prefix"), "{printed}");
}

#[test]
fn a_download_that_doesnt_match_isnt() {
    let dir = scratch("tampered");
    let rel = release(&dir);
    let mut package = std::fs::read(rel.join(PACKAGE)).unwrap();
    package.push(0);
    std::fs::write(rel.join(PACKAGE), package).unwrap();
    let (printed, ok) = install(&dir, &["sha256sum"]);
    assert!(!ok && printed.contains("doesn't match its checksum") && !printed.contains("the package's install.sh"), "{printed}");
}

#[test]
fn a_download_that_cant_be_checked_isnt() {
    let dir = scratch("unchecked");
    release(&dir);
    let (printed, ok) = install(&dir, &[]);
    assert!(!ok && printed.contains("needs sha256sum or shasum") && !printed.contains("the package's install.sh"), "{printed}");
}

#[test]
fn shasum_checks_it_where_sha256sum_is_missing() {
    if find("shasum").is_none() {
        return;
    }
    let dir = scratch("shasum");
    release(&dir);
    let (printed, ok) = install(&dir, &["shasum"]);
    assert!(ok && printed.contains("the package's install.sh, given: --prefix"), "{printed}");
}
