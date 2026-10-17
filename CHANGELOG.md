# Changelog

Until 1.0, a new minor version (0.2.0) may change the circuit file or what bbgen writes; a patch (0.1.1) only
fixes things. Each release's section here is its notes on GitHub.

## Unreleased

- **The board is drawn the way breadboards are made.** With column 1 at the left, row `j` is now at the top and
  row `a` at the bottom, and each pair of rails has its `-` rail above its `+` rail, as on BusBoard's BB830, the
  MB-102 boards and Adafruit's. The drawing used to be the mirror image of a real board: its hole names didn't
  match the ones printed on yours, and a chip placed by its pin 1 (`at:`) could only go in upside down. A hole's
  name now means the hole with that name on your board. A file that places parts by hand keeps its drawing if
  its rows are swapped `a`↔`j`, `b`↔`i`, `c`↔`h`, `d`↔`g`, `e`↔`f`; the rails now default to `-+` (top to
  bottom) on both sides.
- Placement keeps a part's body off every hole of its own pins' strips, leaving each pin a hole for its wire: a
  module whose body hangs over its pins' strips (an OLED) could be placed with no way to wire it.
- Placement no longer puts a chip's pin in a strip where the file has put a leg or a wire end of another net, or
  a pin or the chip's body on a hole the file takes: it used to short into LEDs placed by hand. When there's no
  other place for a chip, bbgen says so and names what's in the way, and it never returns a layout with a short
  of its own making.
- Placement draws chips toward what the file places: a leg placed by hand counts as a fixed pin of its net, and
  a chain of resistors or diodes bbgen places pulls the chip at its far end toward it. Place the LEDs, and the
  chip driving them through resistors lands beside them.
- A flat wire no longer crosses another flat wire: it moves to other holes in the same strips, or becomes a
  flexible jumper where none clear it. A signal wired along the board's outer row used to block the supply wires
  laid after it.
- A wire running under a resistor or other two-lead part says so in the checklist, naming the part to put in
  after it, as one under a raised module does.
- A closed pipe (`bbgen check FILE --json | head`) ends bbgen quietly instead of panicking.
- Parts that repeat around parts the file places are placed alike: each LED's resistor the same way round its
  LED, where the board leaves room. Chips keep off the strip across the trench from a hand-placed leg, where
  those parts go.

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
