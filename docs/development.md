# Developing bbgen

## Building from source

bbgen is written in Rust; bbgen studio is a SvelteKit interface in a [Tauri](https://tauri.app) window.

```bash
git clone https://github.com/LXK-98/bbgen
```

### bbgen

Needs a Rust toolchain (`cargo`, from [rustup](https://rustup.rs)). Link the launcher onto your PATH, which
builds bbgen the first time and again whenever the sources change:

```bash
ln -s "$PWD/bbgen/bin/bbgen" ~/.local/bin/bbgen
```

or install the binary: `cargo install --path bbgen`. bbgen builds wherever Rust does; only the release
packages are Linux-only so far.

### bbgen studio

Needs Node 22 or later and pnpm 10 (with Node 24, `corepack enable` gives you the version `studio/package.json`
names), the Rust toolchain, and Tauri's system libraries:

- Fedora: `sudo dnf install webkit2gtk4.1-devel libsoup3-devel javascriptcoregtk4.1-devel`
- Debian, Ubuntu: `sudo apt install build-essential pkg-config libwebkit2gtk-4.1-dev libsoup-3.0-dev libjavascriptcoregtk-4.1-dev libdbus-1-dev`
- anything else: [Tauri's prerequisites](https://tauri.app/start/prerequisites/)

Then, in `studio/`, `pnpm install` and `pnpm tauri dev`. [studio/README.md](../studio/README.md) has the rest,
including running it in a browser.

### The skill

From a checkout, link the skill in rather than copying it, so it follows the checkout (`install.sh` leaves a
link like this alone):

```bash
ln -s "$PWD/bbgen/skill" ~/.claude/skills/breadboard
```

## Where things are

| | |
|---|---|
| [src/](../src) | bbgen. [main.rs](../src/main.rs) is the command line; the rest is the library it calls |
| [src/circuit.rs](../src/circuit.rs), [yaml.rs](../src/yaml.rs), [val.rs](../src/val.rs) | reading a circuit file, with every pin placed |
| [src/board.rs](../src/board.rs), [parts.rs](../src/parts.rs), [kicad.rs](../src/kicad.rs) | the breadboard's geometry, bbgen's own parts, and parts from KiCad's library |
| [src/search.rs](../src/search.rs), [autoplace.rs](../src/autoplace.rs), [legs.rs](../src/legs.rs), [repeats.rs](../src/repeats.rs) | placing what the file leaves unplaced (groups that repeat, alike), and which nets run on which rails |
| [src/autowire.rs](../src/autowire.rs) | the wires for every net the file leaves unconnected |
| [src/costs.rs](../src/costs.rs) | every weight placement and wiring trade off, in one place |
| [src/check.rs](../src/check.rs), [measure.rs](../src/measure.rs) | the wiring check, and the measures of how a layout reads |
| [src/render.rs](../src/render.rs), [checklist.rs](../src/checklist.rs), [trace.rs](../src/trace.rs) | the drawing, the checklist, and the video |
| [src/schematic.rs](../src/schematic.rs), [symbol.rs](../src/symbol.rs), [elk.rs](../src/elk.rs) | the schematic: symbols, and their layout by ELK |
| [src/scene.rs](../src/scene.rs), [dump.rs](../src/dump.rs) | the JSON for programs (`--json`, `--events`), and the records the tests hold bbgen to |
| [src/pyf.rs](../src/pyf.rs), [pyrand.rs](../src/pyrand.rs) | Python's floating-point arithmetic and random numbers, bit for bit (bbgen began in Python) |
| [studio/](../studio) | bbgen studio ([studio/README.md](../studio/README.md)) |
| [tests/](../tests) | the tests (below) |
| [benchmarks/](../benchmarks) | 21 circuits bbgen places and wires from scratch, and the golden records the tests use |
| [examples/](../examples) | example circuits and what bbgen writes for them |
| [skill/](../skill) | the Claude Code skill |
| [bin/bbgen](../bin/bbgen) | the launcher that runs bbgen from a checkout, building it first when the sources changed |
| [packaging/](../packaging) | the Linux package: its build, `install.sh`, the menu entry, the third-party notices |
| [docs/](.) | the reference, and [docs/design/](design) for the design notes: how bbgen got here and what's still open |

## Tests

```bash
cargo test --release
```

The tests hold bbgen to its records in `benchmarks/golden/`: each benchmark placed and wired event
by event, every trial layout placement builds and its score, and what `bbgen build` prints and
writes for the examples and 113 circuits built to break it. A change that moves a wire fails them
by design; once the benchmarks say the change is better, record it with `benchmarks/record.py`.

A few tests check what placement must do rather than what it did: a chip bbgen places keeps out of strips the file
gives another net, says what's in the way when nothing else fits, and parts repeated around hand-placed LEDs are
placed alike ([tests/hand_placed.rs](../tests/hand_placed.rs),
its circuits in `tests/hand-placed/`).

Some tests guard against attacks rather than bugs. A circuit file can come from anyone, or from an agent
someone misled, so a colour that isn't one is an error and names are escaped in the drawing
([tests/colors.rs](../tests/colors.rs)). A file can't use YAML aliases to fill memory. Every workflow action
is pinned to a commit ([tests/workflows.rs](../tests/workflows.rs)), and `install.sh` installs only a download
it has checked ([tests/install.rs](../tests/install.rs)).

Most of the circuits take parts from KiCad's library, so the tests need it installed: the records were made with
KiCad 10.0's. CI installs it from the [KiCad PPA](https://launchpad.net/~kicad) (`kicad-symbols`,
`kicad-footprints`); a KiCad 10 install of any kind does. bbgen studio has its own checks and tests:
`pnpm check` and `pnpm test` in `studio/`, and after `pnpm build`, `cargo test --manifest-path src-tauri/Cargo.toml`.
These hold the studio to its Content Security Policy, check the paths its commands take, and check that the dev
server answers only the studio's own page.

## Benchmarks

Changing how bbgen places or wires? Run the benchmarks before and after: 21 real circuits that
bbgen places and wires from scratch, measured for errors, warnings, slants, crossings, jumpers and
time, with `--check` failing on any regression ([benchmarks/README.md](../benchmarks/README.md)). The
costs placement and wiring trade off are all in [src/costs.rs](../src/costs.rs), in holes of wire
length so they compare directly. What's still open, what was tried and where to start is in
[docs/design/open-points.md](design/open-points.md).

## Releases

Versions follow [Semantic Versioning](https://semver.org) as 0.x versions do: until 1.0, a new minor version
(0.2.0) may change the circuit file or what bbgen writes, and a patch (0.1.1) only fixes things. The version is
in four places, kept the same: [Cargo.toml](../Cargo.toml), [studio/package.json](../studio/package.json),
[studio/src-tauri/Cargo.toml](../studio/src-tauri/Cargo.toml) and
[studio/src-tauri/tauri.conf.json](../studio/src-tauri/tauri.conf.json).

To release: set the version, add its section to [CHANGELOG.md](../CHANGELOG.md), commit, and push a tag for it
(`v0.2.0`). [The release workflow](../.github/workflows/release.yml) checks the tag against Cargo.toml, builds
the package on Ubuntu 22.04 (so it runs on glibc 2.35 and newer), and publishes the release with the
changelog's section as its notes.

The package is made by [packaging/build.sh](../packaging/build.sh), which works locally too: it writes
`target/dist/bbgen-x86_64-linux.tar.gz`, `install.sh` and `SHA256SUMS`. Built locally, the binaries need the
glibc of the machine that built them. The package holds:

- `bin/bbgen` and `bin/bbgen-studio`
- `share/applications/bbgen-studio.desktop` and the icon, in `share/icons/hicolor/`
- `share/bbgen/`: the README, the licence, the docs, the examples, the skill, and `THIRD-PARTY-NOTICES.md`,
  which [packaging/notices.py](../packaging/notices.py) writes from what the build used (every crate and
  JavaScript package, with its licence text)
- `install.sh`, which installs all that into `~/.local` (or `--prefix`), remembering what it put where for
  `--uninstall`. Run on its own, it downloads the latest release's package and checks it against
  `SHA256SUMS` first; `BBGEN_RELEASE_URL` points it at another release's download folder, such as
  `https://github.com/LXK-98/bbgen/releases/download/v0.1.0`.
