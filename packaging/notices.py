#!/usr/bin/env python3
"""Write THIRD-PARTY-NOTICES.md for a release package: everything bbgen's binaries are built from that isn't
bbgen, each with its version, licence, where its source is, and its licence text.

    packaging/notices.py OUT.md

The Rust crates come from `cargo metadata`: what each binary links (normal dependencies, not build scripts or
macros). The studio's JavaScript, CSS and fonts come from a build of its interface with source maps, which
name every module that went in. THIRD-PARTY.md, the code in bbgen's own source from elsewhere, goes first.
"""
import glob
import json
import os
import re
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
STUDIO = os.path.join(ROOT, "studio")
LICENCE_FILE = re.compile(r"^(licen[cs]e|copying|notice|copyright|unlicense)([-._].*)?$", re.I)


def licence_files(folder, walk_up=0):
    """The licence and notice files in a package's folder; for a crate in a git checkout (elk-rs's), those
    at the checkout's root when the crate's folder has none."""
    for _ in range(walk_up + 1):
        found = sorted(f for f in os.listdir(folder) if LICENCE_FILE.match(f) and os.path.isfile(os.path.join(folder, f)))
        if found:
            return [os.path.join(folder, f) for f in found]
        folder = os.path.dirname(folder)
    return []


def crates(manifest, host):
    """The crates a binary links: (name, version, licence, source, [licence files])."""
    meta = json.loads(subprocess.run(
        ["cargo", "metadata", "--format-version", "1", "--locked", "--filter-platform", host, "--manifest-path", manifest],
        check=True, capture_output=True, text=True).stdout)
    packages = {p["id"]: p for p in meta["packages"]}
    nodes = {n["id"]: n for n in meta["resolve"]["nodes"]}
    root = meta["resolve"]["root"]
    seen, todo = set(), [root]
    while todo:
        pid = todo.pop()
        if pid in seen:
            continue
        seen.add(pid)
        for dep in nodes[pid]["deps"]:
            if any(k["kind"] is None for k in dep["dep_kinds"]):
                todo.append(dep["pkg"])
    out = []
    for pid in seen - {root}:
        p = packages[pid]
        if all("proc-macro" in t["kind"] for t in p["targets"]):
            continue  # runs while compiling; nothing of it is in the binary
        folder = os.path.dirname(p["manifest_path"])
        files = [os.path.join(folder, p["license_file"])] if p.get("license_file") else []
        files = files or licence_files(folder, walk_up=3 if "/git/checkouts/" in folder else 0)
        source = p.get("repository") or (p["source"] or "").removeprefix("git+").split("#")[0] or ""
        out.append((p["name"], p["version"], p.get("license") or "see its licence file", source, files))
    return out


def npm_packages():
    """The studio's interface as built: the packages its JavaScript came from (by its source maps), its fonts and
    Tailwind's CSS."""
    subprocess.run(["pnpm", "exec", "vite", "build", "--sourcemap", "--logLevel", "error"], cwd=STUDIO, check=True,
                   stdout=subprocess.DEVNULL)
    build = os.path.join(STUDIO, "build")
    folders = set()
    for m in glob.glob(os.path.join(build, "**", "*.map"), recursive=True):
        data = json.load(open(m))
        for s in data.get("sources", []):
            path = os.path.normpath(os.path.join(os.path.dirname(m), data.get("sourceRoot") or "", s))
            hit = list(re.finditer(r"node_modules/((?:@[^/]+/)?[^/]+)/", path))
            if hit:
                folders.add(path[:hit[-1].end() - 1])
    own = json.load(open(os.path.join(STUDIO, "package.json")))
    names = [n for n in {**own.get("dependencies", {}), **own.get("devDependencies", {})} if n.startswith("@fontsource")]
    css = "".join(open(f, errors="replace").read() for f in glob.glob(os.path.join(build, "**", "*.css"), recursive=True))
    if "tailwindcss v" in css:
        names.append("tailwindcss")
    folders |= {os.path.realpath(os.path.join(STUDIO, "node_modules", n)) for n in names}
    out = []
    for folder in folders:
        meta = json.load(open(os.path.join(folder, "package.json")))
        repo = meta.get("repository") or meta.get("homepage") or ""
        repo = repo.get("url", "") if isinstance(repo, dict) else repo
        repo = re.sub(r"^git\+|\.git$", "", repo)
        if repo and not re.match(r"^(https?|git|ssh)://", repo):
            repo = "https://github.com/" + repo.removeprefix("github:")
        licence = meta.get("license") or meta.get("licenses") or "see its licence file"
        out.append((meta["name"], meta["version"], licence if isinstance(licence, str) else json.dumps(licence), repo,
                    licence_files(folder)))
    return out


def main(out_path):
    host = re.search(r"^host: (.+)$", subprocess.run(["rustc", "-vV"], capture_output=True, text=True).stdout, re.M)[1]
    cli = crates(os.path.join(ROOT, "Cargo.toml"), host)
    studio = crates(os.path.join(STUDIO, "src-tauri", "Cargo.toml"), host)
    where = {}
    for name, binary in [(c[:2], "bbgen") for c in cli] + [(c[:2], "bbgen-studio") for c in studio]:
        where.setdefault(name, []).append(binary)
    rust = sorted({c[:2]: c for c in cli + studio}.values())
    js = sorted(npm_packages())

    texts = {}  # licence text -> the packages that carry it
    def note(name, version, files):
        for f in files:
            text = open(f, errors="replace").read().strip()
            texts.setdefault(text, []).append(f"{name} {version}")

    lines = ["# Third-party notices", "",
             "bbgen is MIT-licensed (LICENSE, beside this file). Below: the code in bbgen's own source that comes from",
             "elsewhere, then everything the `bbgen` and `bbgen-studio` binaries in this package are built with, each",
             "with its licence and where its source is, and last every licence text they carry.", ""]
    # THIRD-PARTY.md, its title dropped and its headings a level down
    third = open(os.path.join(ROOT, "THIRD-PARTY.md")).read().split("\n", 1)[1]
    lines += ["## From the repository's THIRD-PARTY.md", re.sub(r"^(#+) ", r"#\1 ", third, flags=re.M), ""]
    lines += ["## Rust crates", "", "| crate | version | licence | in | source |", "|---|---|---|---|---|"]
    for name, version, licence, source, files in rust:
        lines.append(f"| {name} | {version} | {licence} | {', '.join(where[(name, version)])} | {source} |")
        note(name, version, files)
    lines += ["", "## JavaScript, CSS and fonts in bbgen-studio", "", "| package | version | licence | source |", "|---|---|---|---|"]
    for name, version, licence, source, files in js:
        lines.append(f"| {name} | {version} | {licence} | {source} |")
        note(name, version, files)
    # a package without a licence file of its own: its licence's text as another package with the same licence has it
    by_licence = {}
    for name, version, licence, _, files in rust + js:
        if files:
            by_licence.setdefault(licence, f"{name} {version}")
    missing = []
    for name, version, licence, _, files in rust + js:
        if not files:
            missing.append(f"{name} {version} ({licence}" + (f": the text is with {by_licence[licence]}'s)" if licence in by_licence else ")"))
    if missing:
        lines += ["", "These have no licence file of their own: " + "; ".join(missing) + "."]
    lines += ["", "## Licence texts", ""]
    for text, users in sorted(texts.items(), key=lambda t: t[1][0]):
        lines += [f"### {', '.join(users)}", "", "```", text.replace("```", "'''"), "```", ""]
    with open(out_path, "w") as f:
        f.write("\n".join(lines))
    print(f"{out_path}: {len(rust)} crates, {len(js)} JavaScript packages, {len(texts)} licence texts", file=sys.stderr)


if __name__ == "__main__":
    if len(sys.argv) != 2:
        sys.exit(__doc__.strip().split("\n\n")[1])
    main(sys.argv[1])
