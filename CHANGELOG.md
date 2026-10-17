# Changelog

Until 1.0, a new minor version (0.2.0) may change the circuit file or what bbgen writes; a patch (0.1.1) only
fixes things. Each release's section here is its notes on GitHub.

## Unreleased

- Placement no longer puts a chip's pin in a strip where the file has put a leg or a wire end of another net, or
  a pin or the chip's body on a hole the file takes: it used to short into LEDs placed by hand. When there's no
  other place for a chip, bbgen says so and names what's in the way, and it never returns a layout with a short
  of its own making.
- Placement draws chips toward what the file places: a leg placed by hand counts as a fixed pin of its net, and
  a chain of resistors or diodes bbgen places pulls the chip at its far end toward it. Place the LEDs, and the
  chip driving them through resistors lands beside them.
- A closed pipe (`bbgen check FILE --json | head`) ends bbgen quietly instead of panicking.

## 0.1.0 (2026-10-17)

The first public release.

- **bbgen** places a circuit's parts on one or more breadboards, lays the wires, checks the wiring against the
  nets, and writes a drawing (SVG and PNG) and a build checklist. Give as much or as little of the layout as you
  like: a file can be just parts and nets.
- Parts from KiCad's symbol and footprint libraries (`symbol:`, `footprint:`), found with `bbgen find`; SMD
  parts on DIP adapters.
- `--schematic` draws the circuit's schematic from the same file, laid out by ELK; `--steps` and `--video` show
  the build step by step; `--json` and `--events` report it for programs.
- **bbgen studio**, a desktop app: inspect a build, watch bbgen place and wire it, compare its steps, export its
  files, and follow a build guide at the bench. It rebuilds whenever the circuit file is saved.
- `bbgen studio FILE` opens a circuit in bbgen studio from a terminal or an agent, and `bbgen docs` says where
  the circuit file's reference is.
- A Claude Code skill for laying out circuits with an agent.
- A Linux package (x86_64) with a one-line installer.
