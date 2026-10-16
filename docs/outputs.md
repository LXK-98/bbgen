# What bbgen writes

`bbgen build circuit.yaml` writes, next to the circuit file (or into `-o DIR`):

- **`circuit.svg`**, and **`circuit.png`** when ImageMagick or `rsvg-convert` is installed (`--no-png` skips
  it): the drawing. The board, every part with labelled pins, numbered wires coloured by net, a legend of the
  nets, and a ring around anything the check found wrong.
- **`circuit.md`**: the build checklist, in building order: wires that parts go over, chips and modules, small
  parts, wires, flexible jumpers, flying leads; each with the holes it goes in and what it connects, and the
  file's `notes` and what bbgen assumed at the top.

and prints what the check found ([What the check reports](circuit-file.md#what-the-check-reports)): each
problem, then how many errors and warnings there were.

```bash
bbgen build circuit.yaml           # circuit.svg/.png/.md next to the YAML, and the check
bbgen build circuit.yaml --steps   # also one image per wire group, that group highlighted
bbgen build circuit.yaml --video build.mp4   # also a video of bbgen placing and wiring it
bbgen check circuit.yaml           # the check only
bbgen build circuit.yaml --bare    # the drawing without title, legend or background
bbgen build circuit.yaml --json    # what was built, as JSON on stdout (scene-format.md)
bbgen build circuit.yaml --schematic   # also circuit.schematic.svg/.png: the circuit's schematic
bbgen build circuit.yaml --wires       # the schematic with every net a wire, no labels
```

The exit status is 0 when the check passes, 1 when it finds errors (the files are still written so you can see
them), and 2 when the file itself can't be read.

## Steps and the video

`--steps` adds an image per group of wires (`circuit.step1-power.svg`, ...), that group highlighted and the rest
dimmed, to build from one group at a time.

The video (`.mp4`, or `.gif`) has a frame per step: each starting layout that beat the one before, each chip
turned round, each move that improved the layout, then each wire as it goes in. A caption says what the step
did and how the score changed (lower is better), and what it changed is ringed in orange; while wiring, pins
not yet connected are circled in red. `--video-all` adds the moves bbgen tried and threw away, which shows where
its time goes. It needs ImageMagick and ffmpeg.

## The schematic

The check holds the board to the nets, but nothing holds the nets to the circuit you meant: a pin in the
wrong net builds a board that passes and doesn't work. `--schematic` draws the circuit from the same file,
so you can see that at a glance before building.

- Parts from KiCad's library are drawn with their own symbol (an op-amp's units apart); bbgen's own parts with
  symbols in KiCad's style: resistors, capacitors, diodes, LEDs, a pot named by its footprint, the Pico, a box
  for a DIP given only labels (supplies on top, grounds below), a connector for an off-board part.
- Supplies and grounds are power symbols, not wires, as in most schematics. A resistor or LED between two signals
  sits in line; one to a supply or ground hangs from its signal. A decoupling cap hangs from its chip's supply line
  (`near:` says which chip), and parts that only take power (a bench supply's connector, a regulator between two
  supplies) sit in a row of their own, "Power", above the rest.
- `--wires` draws every net as a wire, supplies and grounds too (each from one power symbol), with no labels at
  all, like a textbook drawing. It reads well for a circuit of a chip or two and gets busy beyond that.
- Signals run left to right, from what drives them: outputs (by KiCad's pin types), then the controller, then
  connectors. Symbols are mirrored to face their signals, and connectors go at the edges.
- Otherwise, a net that would run across the page, one with many pins, and the nets of a big connector are drawn
  as labels. A bus (`D0`...`D7`) is labelled whole. A long net stays wired if labelling it would leave a part with no
  wire at all.
- bbgen reads the drawing back and checks every wired net is in one piece and touches no other.

Placing the symbols and routing the wires is done by [ELK](https://eclipse.dev/elk/), the Eclipse Layout
Kernel, inside bbgen. The schematic needs only the parts and nets, so bbgen draws it while it places the board:
on a big circuit it costs almost nothing extra, on a small one a tenth of a second or two.

## For programs

`--json` prints what was built as one JSON document on stdout, and writes no files unless `-o` is given;
`--events` prints a JSON line for each step of placing and wiring as bbgen takes it, first. Both are written up
in [scene-format.md](scene-format.md). bbgen studio reads them, and so can scripts and agents.
