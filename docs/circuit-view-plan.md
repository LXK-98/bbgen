# The circuit view: a plan

A schematic of the circuit, drawn from the same YAML as the board: each part as its schematic symbol, signal
nets as wires, supplies and grounds as power symbols. bbgen draws it, `bbgen build` writes it beside the board
(`circuit.schematic.svg`), and bbgen studio shows it beside the board with hover and selection shared between
the two: hover a net in the circuit and its wires light up on the board, hover a pin on the board and it lights
up on its symbol.

Why it's worth having: the check holds the board to the nets, but nothing holds the nets to the circuit you
meant. When Claude writes the nets from a datasheet, or you do from memory, a pin in the wrong net builds a
board that passes its check and doesn't work. A schematic is how a person sees that at a glance. The board
drawing can't show it, because a circuit's connections don't look like anything there.

## Decisions

### 1. bbgen draws it, not the studio

For the same reasons as the board ([studio-plan.md](studio-plan.md)): one drawing, which the studio shows
as it is; `bbgen build` and the Claude Code skill get it too; the tests hold it to records. The studio reuses
what it already does for the board: bbgen's SVG put in the page once, with `data-part`, `data-net` and (new)
`data-pin` groups that hover and selection toggle classes on.

### 2. ELK's layered layout places the symbols and routes the wires; we don't write our own

Laying out a schematic means placing boxes whose pins sit at fixed points on their sides, then routing
wires between those pins, horizontally and vertically, without crossing a box. That's the problem the Eclipse
Layout Kernel's layered algorithm was built for: dataflow diagrams with ports ("Drawing layered graphs with port
constraints", Schulze, Spönemann and von Hanxleden, JVLC 2014). It's what people who draw schematics from
netlists use today:

- [Weave](https://arxiv.org/abs/2607.03835) (July 2026) turns SPICE netlists into LTspice schematics with
  elkjs, plus a few placement rules of its own (below), and checks every drawing against the netlist.
- [netlistsvg](https://github.com/nturley/netlistsvg) draws Yosys netlists, analog and digital, with elkjs.
- D2 lays out diagrams with ELK, and has since ported it to Go to drop the JavaScript
  ([elk-go](https://pkg.go.dev/github.com/d2lang/elk-go), checked against elkjs 0.12.0 output for output).

No Rust crate does ports and orthogonal routing: [bra0-layout](https://docs.rs/bra0-layout),
[rust-sugiyama](https://lib.rs/crates/rust-sugiyama) and gen-sugiyama do the layering only. Writing the rest
ourselves (layering, crossing reduction with ports, node placement, orthogonal routing of nets with many pins)
would be a project the size of placement, and would trail ELK for a long time.

Considered and not taken:

| | why not |
|---|---|
| elkjs in the studio | fast (10–50 ms in the web view), but then the studio draws a picture bbgen doesn't, and the command line and the skill get no schematic |
| netlistsvg as is | Yosys JSON in, symbols from its own skin files, no KiCad symbols, nothing for hover; its layout is elkjs anyway, so we'd use a small part of it |
| tscircuit's solvers ([schematic-trace-solver](https://github.com/tscircuit/schematic-trace-solver), [autolayout](https://github.com/tscircuit/autolayout)) | TypeScript, built around tscircuit's own circuit model, young; they draw net labels for most connections, and users there find that cluttered |
| a KiCad schematic (`.kicad_sch`) drawn by `kicad-cli` | KiCad doesn't lay out schematics, so placing and routing would still be ours, and it would need KiCad installed. Worth having later as an export (step 8). |
| Graphviz | its orthogonal routing ignores ports |

### 3. elkjs runs inside bbgen, in QuickJS

Spiked (October 2026): elkjs 0.12.0's `elk.bundled.js` runs unchanged in QuickJS through the
[rquickjs](https://crates.io/crates/rquickjs) crate (0.14). It needs four small shims (a `module`/`exports`
object, `global`, a silent `console`, and a `setTimeout` that queues callbacks, which elkjs's in-process worker
uses), and it lays out the same graph to the same size as Node does. The graphs were each benchmark's parts as
boxes and its signal nets as edges:

| | nodes, edges | Node (after warm-up) | QuickJS |
|---|---|---|---|
| `examples/eeprom.yaml` | 6, 6 | 12 ms | 75 ms |
| 13 (Ben Eater's register) | 25, 49 | 40 ms | 470 ms |
| 16 (iPod M2) | 32, 74 | 51 ms | 700 ms |

Loading elkjs into QuickJS takes another 140 ms per process. QuickJS can load precompiled bytecode instead,
which should cut most of that. So a schematic costs 0.2 s on a small circuit and under a second on the
biggest, which is why it's a flag at first (see Output).

What it costs:

- 1.5 MB of JavaScript in the binary, and QuickJS's C sources, which `rquickjs-sys` compiles with the C
  compiler Rust already needs for linking (Linux, macOS, and MSVC's build tools on Windows).
- elkjs is EPL-2.0 (or GPL-3.0). Shipping it unmodified, with its licence beside it (`vendor/elkjs/`), is
  fine in an MIT project. The README says so.
- The maths comes from the C library, as with annealing's `exp` today (open point 11): another libm may
  move a wire by a pixel.

If speed ever matters, the layout sits behind one function (`elk::layout(graph) -> graph`) that a native
port could replace, as D2's did.

### 4. What makes it read like a schematic is ours

The spike drew the benchmarks with ELK alone: each chip a box with its pins in DIP order down its sides, each
net an edge from its first pin. 13 came out as a tangle of 49 wires around three boxes, and in the 555
blinker every wire leaves the chip from the same side. ELK places and routes well, but what it's given decides
how the drawing reads. Weave and netlistsvg found the same thing: they add rules for power, for parts that
hang off a net, and for symbols. The rules below are those, with what bbgen knows that they don't (which pins
are supplies, which part decouples which chip, KiCad's symbols).

### 5. Black on white, like a schematic

Wires and symbols in one colour (`currentColor`, so the studio's dark theme works; black in the file bbgen
writes). Net colours would mean little with 58 nets (16) and would fight the symbols. The studio lights a
net in its colour on hover and selection, so it's easy to match with the board.

## What the drawing shows

### Symbols

- **KiCad parts** (`symbol:`, 48 of them in the benchmarks) are drawn with their own symbol, as KiCad draws
  it. KiCad's symbols already put the pins where a schematic wants them: the 74LS173's inputs on the left,
  its outputs on the right, VCC at the top and GND at the bottom, and each pin says what it is
  (`input`, `output`, `tri_state`, `power_in`, ...). [src/kicad.rs](../src/kicad.rs) reads only pin numbers,
  names and types today. It needs to read the graphics too (`rectangle`, `polyline`, `circle`, `arc`, `text`),
  each pin's position, direction, length and shape (inverted, clock), and `pin_names`/`pin_numbers` hiding.
  It should take De Morgan style 1, and read every unit: a symbol with several units (an LM358's two
  amplifiers and its power unit) is drawn as one symbol per unit, as in KiCad.
- **Two-lead parts** are drawn by bbgen in KiCad's style (`Device:R`, `C`, `C_Polarized`, `D`, `LED`), on
  the same 0.1 in grid, with their ref and value. They aren't copied from KiCad's library: built-in parts
  must draw the same without KiCad installed.
- **The Pico** (`type: pico`) gets a built-in box laid out like KiCad's `MCU_Module:RaspberryPi_Pico`:
  GPIOs down both sides in number order, supplies at the top, ground at the bottom.
- **Pins joined on the part** (the Pico's seven GNDs, a KiCad symbol's repeated power pins) are one pin.
- **DIPs and SIPs given only labels** (`type: dip`) get a plain box, Weave's fallback for parts it has no
  symbol for. Supply-named pins go at the top and ground-named ones at the bottom. The rest stay on the side
  the pinout puts them (1 to n/2 on the left), but in whatever order ELK finds fewest crossings with
  (`FIXED_SIDE`). The pin numbers are drawn, so it can still be checked against a datasheet.
- **Off-board connectors** look like KiCad's `Conn_01xNN` (`Conn_02xNN` with `cols: 2`): pins in order down
  one side, facing the circuit.
- **Parts with only a footprint** (the trimmer in 16) get a plain box with pin numbers.
- **SMD parts on an adapter**: the circuit's pins are the adapter's, so pins are mapped back to the symbol's
  numbers through `map`.

### Supplies and grounds are symbols, not wires

Every pin on a net with a polarity (`rail: +` or `-`, or guessed from its name) ends in a power symbol: a bar
with the net's name for a supply, the ground symbol for a ground. Weave and netlistsvg do the same, and the
reason is in the numbers: in 16, GND has 48 pins and VDDIO 17; as wires, they'd be most of the drawing. The
symbol sits at the end of the pin, pointing the way the pin does. Each node is made big enough for its
symbols (as Weave sizes its nodes), so ELK keeps everything else clear of them.

### Two-lead parts sit in line or hang

- **Between two signal nets**: in line, horizontally, in the flow (R1 between the Pico's GP15 and the LED).
- **One leg on a supply or ground**: standing up, hanging from its signal, with the power symbol at its far
  end. A pull-up hangs up to its supply, and an LED or a pull-down hangs down to ground (Weave's "shunts" and
  "divider legs"). So GP15 → R1 → LED → GND reads as it does in a textbook. These first go into ELK as
  nodes with their signal pin on the top or bottom side. If ELK routes those with loops, they are placed after
  layout instead, under or over the pin they hang from, in room the node reserved for them (as Weave does).
- **Both legs on supplies** (decoupling caps): out of the flow, in a row with their power symbols, beside the
  chip they decouple: its `near:`, else the chip with a pin on that supply nearest in the file (the rule
  placement already uses). Weave calls these "supply corners".

### Signals flow left to right

ELK's layers need every edge to have a direction, and a net has none. Turning the edges of an undirected
graph into directed ones for a layered drawing is studied by [Zink et al.](https://arxiv.org/abs/2008.10583)
(cable plans with port constraints, 2022). They compare several ways and report 10–30 % fewer crossings
than KIELER (ELK's predecessor). What bbgen has to go on, in order:

1. the pins' types, from KiCad: an `output`, `tri_state` or `power_out` pin drives its net, an `input` pin
   is driven;
2. which side of its symbol a pin is on: a pin on the right drives, one on the left is driven (KiCad's
   convention, and the generic box's);
3. the Pico drives (it's the controller) and an off-board connector comes in from the left, unless the pins
   above say otherwise.

Each net becomes edges from its driving pin to each of its other pins, merged into one wire with dots where it
branches (ELK's `mergeEdges` and the junction points it returns). This is where the drawing will need the
most tuning, judged on the drawings.

### Long nets become labels

A net that would run across the whole drawing (a clock to three chips, a lead to a connector on the far side)
is drawn as a short stub with its name at each pin instead, as KiCad users do. Start with every signal net as
wires. Then, a net at a time, label the one whose wires are longest and cross most, and lay out again while
that makes the drawing better. The numbers pick the candidates; the drawings decide what the rule ends up
being (see [Judging it](#judging-it)). A bus (13's eight lines between two chips, in order on both) stays
wires: ELK keeps parallel edges between ordered ports parallel.

### Text

Each symbol's ref and value (or title), placed where KiCad's symbol puts its `Reference` and `Value` (beside
the symbol for built-ins). Pin names and numbers as the symbol shows them. Net names on labels and power
symbols only.

## The drawing checks itself

As Weave does, bbgen reads back what the drawing connects and compares it with the nets, the way the check
compares the board. Wire ends and junctions that touch, pins at wire ends, and power symbols and labels by
name make up the drawing's nets. Any difference is a bbgen bug: the tests fail on it, and a build that hits
one labels that net instead (always right) and prints a note. The same pass checks for a wire through a
symbol, a wire end on another net's pin, and labels drawn over each other. The tests hold every benchmark to
none of these.

## Output

- `bbgen build circuit.yaml --schematic` writes `circuit.schematic.svg` (and `.png`, as for the board) next
  to the rest. It starts as a flag so the benchmark times and the golden records don't move until the
  drawings are good. Then it becomes the default, with `--no-schematic`.
- With `--json`, the scene gets a `schematic` field: `svg`, `width`, `height`, and each symbol's `ref`,
  `unit` and `rect`. `schema` stays 1, since it's a field added. Groups in its SVG: `data-part="U1"` (with
  `data-unit` for multi-unit symbols), `data-net="SDA"` around wires, junctions, labels and power symbols,
  and `data-pin="U1.5"`.
- `bbgen dev schematic FILE` prints the graph given to ELK and what ELK returned, to look at in
  [ELK's online viewer](https://rtsys.informatik.uni-kiel.de/elklive/json.html) when a layout reads badly.
- The checklist doesn't change.

## In the studio

- The centre pane gets a switch: **Board**, **Circuit**, or **Both** (side by side, resizable). The studio
  asks for `--schematic` once the Circuit or Both view has been opened.
- Board.svelte's pan, zoom and hover become a shared drawing component. The board adds holes and strips on
  top; the circuit adds pins.
- One hover and one selection, as now. A net or part lights in both. A wire hovered on the board lights its
  net in the circuit, since a wire has no place in a schematic. A new key, `pin:U1.5`, lights the pin on its
  symbol and its hole and strip on the board; the inspector shows the pin's part, net and hole.
- The status bar names the pin or net under the pointer in the circuit.
- The export menu (studio plan, step 4) gets the schematic.

## Steps

Each step ends with drawings to look at: the examples and all 21 benchmarks, before and after.

0. **Spike.** Done: elkjs in QuickJS from Rust, same output as Node, timings above.
1. **Symbols.** Done (October 2026). [src/symbol.rs](../src/symbol.rs) has the symbols and draws them, turned and
   mirrored; `kicad::drawing` reads KiCad's (cached apart from `kicad::symbol`, so placement is no slower).
   `bbgen dev symbols FILE OUT.svg [--rot N] [--mirror]` draws every part's symbol in a grid. Checked by eye on all
   21 benchmarks, and against `kicad-cli sym export svg` for the 74HC595, NE555P, LM358, DS18B20 and AP2112K:
   same pins, fields and units. `tests/symbols.rs` holds every pin in a net to a pin you can see on its symbol.
   Found on the way:
   - Every symbol the benchmarks use is drawn with rectangles, polylines, circles, arcs, text and pins (line,
     inverted, clock); KiCad's extra GND pins are hidden and stacked on the visible one.
   - A part with only a potentiometer's footprint (the four trimmers) gets a potentiometer symbol.
   - Buttons are written as 4-pin DIPs with a title, so they stay boxes: which pins the switch joins isn't
     known. The skill could give them `symbol: Switch:SW_Push` instead.
   - A symbol turned to lie flat keeps its fields where the library put them, as vertical text. KiCad moves
     them beside the part when it's placed; that belongs to step 3.
   - The pins a symbol has no pin for are real gaps: the empty pin of a 5-pin chip on a 6-pin adapter, and two
     pins of the relay's footprint. None is in a net.
2. **The engine.** Done (October 2026). [vendor/elkjs/](../vendor/elkjs) holds elkjs 0.12.0 unmodified, with its
   licence, version and checksum. [build.rs](../build.rs) compiles it to QuickJS bytecode, and
   [src/elk.rs](../src/elk.rs) has `layout(&Value) -> Result<Value, String>`. A thread loads ELK the first time
   it lays something out. `bbgen dev elk GRAPH.json [OUT.json] [--repeat N]` lays out a graph and times it.
   [tests/elk.rs](../tests/elk.rs) holds three graphs to Node's layouts of them, number for number, and checks
   that layouts are the same every time and on every thread, and that a broken graph says what's wrong.
   - Bytecode: parsing the 1.5 MB of JavaScript was 122 ms of the 128 ms load. Stripped bytecode (2 MB) loads in
     about 8 ms and lays out the same. Bytecode with its debug info would be 10 MB.
   - Layouts take 60–110 ms for small graphs (eeprom, 6 nodes) and 450–700 ms for big ones (13, 16), the same
     on a second layout in one process. A profile puts 63 % in QuickJS's interpreter loop and most of the rest
     in property lookups. Registering only the layered algorithm and raising the GC threshold made no
     difference, so the time is QuickJS interpreting ELK. For step 6: the schematic depends only on the parts
     and nets, so a build can lay it out on a thread of its own while placement runs.
   - The only difference from Node was `$H`, an object id ELK's compiled Java adds to what it touches,
     numbered differently in each engine. It's left out of the output on both sides.
   - Costs: the stripped binary went from 3.1 MB to 6.2 MB, and a clean build from 14 s to 38 s (QuickJS is
     compiled for the build script too). Incremental builds are as before.
3. **A first schematic.** Done (October 2026). [src/schematic.rs](../src/schematic.rs): each unit a node, its
   signal pins ports fixed on the node's edge (the pin is drawn on to the edge, so a node's box can take in its
   text and power symbols); supplies and grounds as power symbols; two-lead parts in line between signals or
   hanging from one; each net drawn from its driving pin, merged, with junction dots. `bbgen dev schematic FILE
   OUT.svg` draws one (with the ELK graph in and out beside it). 65–820 ms each over the 21 benchmarks.
   `tests/schematic.rs` draws the examples. What the drawings show:
   - Reads well already: 04 (the 555's timing network and output, as in a textbook), 07 and 09.
   - **Symbols facing the wrong way.** A part driving from its left side (the Pico in 09 and 16) lands right of
     or below what it drives, and its wires go round it; one fed on its right (07's Output connector, the
     24LC256 in the eeprom example) lands on the left. Mirror such a symbol, as KiCad users do.
   - **Fields of in-line parts** stay vertical where the library put them (R3 in 04). Place them above,
     horizontally.
   - **Power symbols on side pins** point sideways (connectors' GND, the 555's RST, the 595's OE). Supplies
     should point up and grounds down, after a short wire.
   - **Decoupling caps and power units** sit in a row at the top left, far from their chips, leaving much of the
     page empty (16: nine caps).
   - **Long nets.** 16's VDDIO (17 pins, no rail) is a wire tree over the whole page, and the nets to its 34-pin
     connector and 13's bus cross everything: these want labels (step 5).
   - **Connectors in the middle**: 13's Bus connector sits between the chips; connectors belong at the edges.
4. **The rules.** First pass done (October 2026), each judged on the drawings of all 21 benchmarks:
   - **Mirroring**: a symbol is mirrored when its driving pins point left or its driven ones right, more than
     the other way (the Pico in 09, 18; the connectors in 07; the 24LC256). 09 now reads Pico → 595 → LEDs.
   - **Connectors at the edges**: an off-board part goes in ELK's first layer when it mostly drives, its last
     when it's mostly driven (`layerConstraint`).
   - **In-line parts** have their ref and value above them, across.
   - **Power symbols**: supplies point up and grounds down, after a short wire from a side pin. Neighbouring
     pins on one supply, on one side, share a symbol, their wires joined (the 74LS173's OE1/OE2, the
     ATmega's VCC/AVCC). A side pin's symbol points straight out when another pin on that side is in the way
     (the FTDI header's GND and 5V); pointing sideways it's narrower, so neighbours don't touch.
   - **Decoupling caps** (both legs on supplies) stand above the chip they're `near`, else the chip with a pin
     on their supply that has fewest so far, chips before modules. An op-amp's power unit stands there too.
   - **A potentiometer** between a supply and a ground has the supply on top.
   - Still to do: 13 and 16 need labels for their buses and long nets (step 5); 16's caps are spread over the
     page beside their chips, but the page is large; 08's relay wiring takes detours.
5. **Labels and the self-check.** Long nets as labels; the read-back check; records of each benchmark's
   and example's schematic in `benchmarks/golden/` and `tests/`, held like the board's.
6. **The command line and the scene.** `--schematic`, the `schematic` field, `bbgen dev schematic`; README,
   [scene-format.md](scene-format.md), and the skill (look at the schematic to check the nets against the
   circuit you meant, before building).
7. **The studio.** Circuit and Both views, the shared drawing component, `pin:` keys, linked hover; lookup
   tests over the schematic's groups.
8. **Later.** Write a `.kicad_sch`: the positions and KiCad symbols are already there, so a circuit can be
   opened in KiCad and carried on with. Hints in the YAML if a layout needs a nudge (which side a connector
   goes, a symbol flipped), the way `at:` pins a part on the board; then dragging a symbol in the studio
   writes them (Phase 4 of the studio plan).

## Judging it

As with layouts, how it looks decides, not a number. A schematic reads well when signals run left to right,
each chip sits with what it talks to, parts that hang off a net hang, nothing is crowded, and few wires cross
or bend. Wires crossed and bent, total length and labels count are worth printing per benchmark to see what a
change did. But a change is kept only when the drawings look better.

## Risks and open questions

- **Big circuits.** 16 (32 parts, 58 nets, three 24-pin chips) may need many labels to be readable at
  all. That's normal for a schematic of that size, but it's the stress test.
- **Pins on the top and bottom.** ELK's layered layout routes top and bottom ports with extra bends; hanging
  parts may have to be placed after layout (step 4).
- **Generic boxes read worse than KiCad symbols.** A `type: dip` with labels has no pin types, so signal
  direction is guessed from its sides. The skill can prefer `symbol:` where KiCad has the part.
- **Speed.** 0.2–0.85 s per schematic in QuickJS. A flag until bytecode loading and the drawings are
  settled; a native port if it's ever the slow part.
- **ELK's version is part of the records**, like KiCad's library: upgrading elkjs means recording again and
  looking at what moved.
