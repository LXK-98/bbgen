<p align="center"><img src="studio/src-tauri/icons/icon.svg" width="112" alt=""></p>

# bbgen

**Breadboard layouts from a circuit file.** Say which parts you have and what connects to what: bbgen places
the parts on a breadboard, lays the wires, checks the wiring and draws the board to build from. Write the file
yourself, or have an AI agent write it: bbgen is made for both.

![A Raspberry Pi Pico and an I2C EEPROM on a breadboard, as bbgen draws it](examples/eeprom.png)

One circuit file gives you:

- **a drawing** of the board (SVG and PNG): every part with labelled pins, numbered wires coloured by net, and
  a ring around anything that's wrong
- **a build checklist** in building order, each step with the holes it goes in and what it connects
- **a wiring check**: what the strips and wires actually connect, held to the nets you declared. Opens, shorts,
  stray connections, pins in the trench or under another part, two things in one hole: all errors
- **a schematic**, drawn from the same file, to check the nets against the circuit you meant
- **bbgen studio**, a desktop app to inspect the board, watch bbgen place and wire it, and build it step by step

bbgen is young (0.x): the circuit file may still change between minor versions.

## Install

On Linux (x86_64):

```bash
curl -fsSL https://github.com/LXK-98/bbgen/releases/latest/download/install.sh | sh
```

This puts `bbgen` and `bbgen-studio` in `~/.local/bin`, bbgen studio in your desktop's menu, and the
[skill](#with-an-ai-agent) in Claude Code. No root needed; run it again to update. Options go after
`sh -s --`: `--no-skill` leaves the skill out, `--prefix DIR` installs somewhere else, and `--uninstall` takes it
all out again. Or download the package from the [releases](https://github.com/LXK-98/bbgen/releases), unpack it
and run its `install.sh`.

bbgen itself needs nothing else. bbgen studio needs WebKitGTK 4.1, which most Linux desktops have (the installer
says if yours doesn't). For parts from KiCad's library, bbgen reads KiCad's own install; for PNGs it needs
ImageMagick or `rsvg-convert`, for videos ImageMagick and ffmpeg.

Packages for other systems may follow. Until then, bbgen builds from source anywhere Rust does:
see [docs/development.md](docs/development.md).

## Use

A circuit file can be just the parts and the nets:

```yaml
title: Pico + I2C EEPROM
parts:
  Pico: {type: pico}
  U1: {type: dip, pins: 8, title: 24LC256, labels: [A0, A1, A2, GND, SDA, SCL, WP, VCC]}
  C1: {type: capacitor, value: 100nF, near: U1}
  R1: {type: resistor, value: 330R}
  D1: {type: led}
nets:
  3V3: {pins: [Pico.3V3, U1.VCC, C1.1], rail: +}
  GND: {pins: [Pico.GND, U1.GND, U1.A0, U1.A1, U1.A2, U1.WP, C1.2, D1.K], rail: -}
  SDA: [Pico.GP16, U1.SDA]
  SCL: [Pico.GP17, U1.SCL]
  LED_DRIVE: [Pico.GP15, R1.1]
  LED: [R1.2, D1.A]
```

```bash
bbgen build circuit.yaml
```

places and wires it, checks it, and writes `circuit.svg`, `circuit.png` and the checklist `circuit.md` next to
it. Give a part a place (`at: f30`), or a wire yourself, and bbgen keeps it and fits the rest around it.

```bash
bbgen build circuit.yaml --schematic   # also the circuit's schematic
bbgen build circuit.yaml --video x.mp4 # also a video of bbgen placing and wiring it
bbgen check circuit.yaml               # the check only
bbgen find ne555                       # look a part up in KiCad's library
bbgen studio circuit.yaml              # open it in bbgen studio
```

The exit status is 0 when the check passes, 1 when it finds errors, and 2 when the file can't be read.

**bbgen studio** opens a circuit, builds it and lets you inspect the board: hover a part, wire or net to
light it up, click it for what matters about it. It rebuilds whenever the file is saved, shows bbgen placing and
wiring step by step, and walks you through building the board. Open it from your menu, or with
`bbgen studio circuit.yaml`.

## With an AI agent

bbgen is made to be driven by an agent: everything it does is a command, and the check tells the agent when
it's done.

- **Claude Code**: the installer adds a skill, [`breadboard`](skill/SKILL.md). Ask Claude Code for a breadboard
  layout of your circuit and it writes the file from your design, checks the schematic against it, builds,
  fixes every error, looks at the drawing, and hands you the drawing and the checklist.
- **Other agents**: give them [skill/SKILL.md](skill/SKILL.md) as instructions (installed at
  `~/.local/share/bbgen/skill/SKILL.md`). `bbgen docs` prints where the circuit file's reference is, and
  `--json` prints what was built for a program to read ([docs/scene-format.md](docs/scene-format.md)).
- **Watch it work**: `bbgen studio circuit.yaml` opens the circuit in bbgen studio and returns at once; the
  studio rebuilds every time the agent saves the file. A studio that's open already switches to the circuit.

## Documentation

| | |
|---|---|
| [docs/circuit-file.md](docs/circuit-file.md) | the circuit file: holes, wires, automatic placement and wiring, part types, KiCad parts, colours, what the check reports |
| [docs/outputs.md](docs/outputs.md) | what bbgen writes: the drawing, the checklist, steps, the video, the schematic |
| [docs/scene-format.md](docs/scene-format.md) | the JSON that `--json` and `--events` print |
| [studio/README.md](studio/README.md) | bbgen studio |
| [docs/development.md](docs/development.md) | building from source, the project's layout, tests, benchmarks, releases |
| [docs/design/](docs/design) | design notes: how bbgen got here, and what's still open |
| [CHANGELOG.md](CHANGELOG.md) | what changed in each release |

## Licence

bbgen is MIT-licensed: see [LICENSE](LICENSE). It builds on others' work:

- the schematic is laid out by [ELK](https://eclipse.dev/elk/), the Eclipse Layout Kernel, ported to Rust by
  [elk-rs](https://github.com/openedges/elk-rs) (EPL-2.0)
- bbgen studio runs on [Tauri](https://tauri.app), [Svelte](https://svelte.dev), [bits-ui](https://bits-ui.com)
  and [shadcn-svelte](https://shadcn-svelte.com), with [Lucide](https://lucide.dev) icons and the
  [IBM Plex Sans](https://github.com/IBM/plex) font (OFL-1.1)
- bbgen keeps the layouts it made when it was written in Python, so part of
  [CPython](https://github.com/python/cpython) is ported in: its random numbers, with the Mersenne Twister
  (PSF-2.0 and BSD-3-Clause)
- parts come from [KiCad's libraries](https://www.kicad.org/libraries/), which bbgen reads from your KiCad
  install and doesn't include (CC-BY-SA-4.0, with an exception for designs made with them)

[THIRD-PARTY.md](THIRD-PARTY.md) has the details. Every release package lists all it's built from, with each
licence's text, in `share/bbgen/THIRD-PARTY-NOTICES.md`.
