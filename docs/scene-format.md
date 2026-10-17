# The scene format

`bbgen build circuit.yaml --json` (or `check`) prints what the build produced as one JSON document on stdout,
and nothing else. It writes no files unless `-o DIR` is given. The exit status is the same as without
`--json`: 0 when the check passes, 1 when it finds errors, 2 when the file can't be read. bbgen studio reads
this; so can scripts. `tests/scene.rs` holds every example and benchmark to what's described here.

`schema` goes up whenever a change would break a reader. Fields may be added without it going up.

## A circuit that can't be read

```json
{"schema": 1, "bbgen": "0.1.0", "error": {"message": "did not find expected ',' or '}' (line 3, column 1)", "line": 3, "column": 1}}
```

`line` is where the YAML reader stopped, or, when the message starts with what it's about (`parts.U1: ...`,
`wires.power[2]: ...`, `nets.GND: ...`), that entry's line in the file. `column` is the YAML reader's only. Both
are null when the message doesn't say where (a part that names a pin that doesn't exist).

## A circuit that was built

| field | what it is |
|---|---|
| `schema`, `bbgen` | the format's version, bbgen's version |
| `source`, `title`, `notes`, `groups` | the file, its title and notes, its wire groups in order |
| `width`, `height`, `svg` | the drawing as `bbgen build --bare` writes it (the boards and what's on them: no title, legend or background), with groups added (below) |
| `boards` | `id`, `kind` (`full`, `half`), `columns`, `split_rails`, `rect`, `rails` (each rail's nets), `holes` |
| `parts` | see below |
| `wires` | see below |
| `nets` | `name`, `color` (hex), `pins`, `rails` (as the file asks), `polarity` (`+`, `-` or null), `guessed`, `line` |
| `issues` | `level`, `code`, `message`, and what it's about: `parts`, `pins`, `holes`, and `line` when the message starts with an entry of the file (`nets.GND: ...`) |
| `checklist` | the build's steps in building order: `section` (`under`: flat wires a part goes over; `chips`: chips and modules; `parts`: small parts; `wires`; `flex`: flexible jumpers; `leads`: each off-board part and its flying leads), `item` (`part:U1`, `wire:W6`), `text`. The order within each section is in `checklist::steps` |
| `sections` | the checklist's sections in building order: `key` (a step's `section`), `title` (its heading in the checklist), `intro` (what the checklist says under it: the order within it) |
| `schematic` | with `--schematic` or `--wires` only: `svg` (the circuit's schematic, on nothing, in `currentColor`), `width`, `height`, `style` (`symbols`, or `wires`: every net a wire, no labels), `labels` (the nets drawn as labels), `problems` (what reading the drawing back found wrong: nothing, unless bbgen has a bug), `ms`. `{"error": ...}` when it couldn't be drawn |
| `stats` | `errors`, `warnings`, `parts`, `wires`, `nets`, `ms` (load, place, wire and check) |

Positions (`rect`, `holes`, `xy`, `points`, `body`) are in the drawing's pixels: `[x, y]`, or
`[x0, y0, x1, y1]` for a rectangle. `holes` maps every hole's name (`f30`, `top+33`, `2:a12` on another
board) to its centre.

`line` (on parts, wires, nets and issues) is the line of the file the thing is written on, counting from 1: a
part's or net's key, a wire's list item. It's null for what bbgen added (the wires it lays) and for a net the
file doesn't name.

Pins and holes are named as the checklist names them: a pin is `U1.5 SDA` (number, then name when it has
one), a hole `f30`.

### Parts

`ref`, `kind`, `title`, `value`, `color`, `source` (the KiCad symbol and footprint, or empty), `adapter`,
`at` (pin 1's hole), `rot`, `raised`, `slant`, `keepouts` (their names), `body` (the body's rectangle, for
chips, modules and off-board parts), `text` (its checklist line, after the ref), and:

- `placed_by`: `you` when the file gives its place (`at:` for a chip or module, `pins:` for a two-lead part),
  `bbgen` when bbgen placed it.
- `legs`: a two-lead part's legs as the file writes them (`Pico.GP15`, `j27`); the holes they ended up in
  are its pins'.
- `pins`: `num`, `name`, `hole`, `xy`, `net` (null when the pin is in no net).

### Wires

`id` (`W6`), `num`, `group`, `nets` (what its ends are connected to), `color` (hex), `a` and `b` (each `hole`,
`pin` for an off-board end, and `spec`, the end as written), `spec`, `via`, `points` (the wire's path, bends
included; empty when it couldn't be placed), `flex` (a flexible jumper), `flying` (to an off-board pin),
`laid_by` (`you` or `bbgen`), `slant`, `under` (the raised modules and two-lead parts it runs under), `text` (its checklist line,
after the number).

## Steps as they're taken: `--events`

`--events` is `--json` with a line before the scene for each step of placing and wiring, printed as it's
taken, so a program can show the run live (bbgen studio's Generation mode). Each starts `{"event":`; the
scene, or the error, is still the last line.

```jsonc
{"event": "step", "n": 21, "phase": "repair", "caption": "R6 into its 4th best legs", "kept": true,
 "focus": ["R6"], "score": 250.2, "from": 256.0, "svg": "<svg ...>", "width": 1422, "height": 428}
```

| field | what it is |
|---|---|
| `n` | the step's number, from 1 |
| `phase` | `start` (a starting layout), `turn` (footprint parts turned round), `repair` (a part moved), `wiring` (a wire laid or dropped, once the parts are placed) |
| `caption` | what the step tried |
| `kept` | whether bbgen kept it; a wiring step always is |
| `focus` | the parts and wires it changed (ringed in the drawing) |
| `score`, `from` | the layout's score (lower is better) and the score of the best before it; `from` is null for the first layout, and both are null for wiring |
| `svg`, `width`, `height` | the board as it stands, drawn as the scene's `svg` is: with kept steps only |

A drawing costs a check and a drawing of the whole circuit, so tried steps come without one: with them all
drawn, benchmark 09 takes twice as long and prints 24 MB. As it is, `--events` adds about a tenth to its
build (0.27 s to 0.30 s), a third to benchmark 13's. `tests/scene.rs` checks that the scene after the events
is the one `--json` prints.

## The drawing's groups

Every piece of the drawing that belongs to something is inside a group saying what:

| group | around |
|---|---|
| `<g data-part="U1">` | a part's body, pins, legs and label. A part has several: its label is drawn last so it sits over wires, and a raised module is drawn over the wires under it. `data-keepout=""` marks its keep-out zones. |
| `<g data-wire="W6" data-net="SDA">` | a wire. `data-net` lists its nets, space-separated. Its number tag is a second group with `data-label=""`. |
| `<g data-issue="3">` | the rings for `issues[3]` |

In `schematic.svg`: `<g data-part="U1">` around a symbol (with `data-unit="A"` for one unit of several),
`<g data-net="SDA">` around a net's wires, branch dots, labels and power symbols, and `<g data-pin="U1.5">` around
a pin, named by the symbol's pin number.

Without `--json` there are no groups. Without `--json` or `--bare`, the drawing is byte for byte what it was:
title, status line, legend and a white background.
