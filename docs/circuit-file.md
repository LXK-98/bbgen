# The circuit file

A circuit file is YAML: the boards, the parts, the wires, and the nets, which say what should be
connected. Only the parts and the nets are needed. Leave out where a part goes and bbgen places it
([Automatic placement](#automatic-placement)); leave out the wires and bbgen lays them
([Automatic wiring](#automatic-wiring)). Anything you do give is kept, so you can pin down what matters to
you and let bbgen fit the rest around it. Whatever the file says, the check holds the board to the nets.

`bbgen docs` prints where this page is, for an agent that has bbgen but not its source.


## A whole file

This is [examples/eeprom.yaml](../examples/eeprom.yaml), with every part placed and every wire given by hand:

```yaml
title: Pico + I2C EEPROM
notes: ["The LED's long leg is the anode."]   # copied into the checklist

boards:                       # optional; default is one full-size board
  - type: full                # full (63 columns) or half (30)
    rails: {top: "+-", bottom: "+-"}   # rail order top to bottom, as printed on yours
    split_rails: false        # true if your rails break in the middle
                              # below: 1 snaps this board on under board 1

parts:
  Pico: {type: pico, at: h3}  # pin 1 in h3, USB to the left
  U1: {type: dip, pins: 8, width: 3, at: f30, title: 24LC256,
       labels: [A0, A1, A2, GND, SDA, SCL, WP, VCC]}
  R1: {type: resistor, value: 330R, pins: [Pico.GP15, j27]}
  D1: {type: led, pins: [R1.2, bot-]}
  C1: {type: capacitor, value: 100nF, pins: [top+33, top-33]}
  J1: {type: offboard, title: Logic analyzer, cols: 2, labels: {1: SDA, 2: SCL, 3: GND, 4: "5V!"}}

wires:                        # a list, or groups of lists (each group is a checklist section)
  power:
    - [Pico.3V3, top+]
    - [Pico.38, top-]
    - [Pico.3, bot-]
    - [U1.VCC, top+]
    - [U1.GND, bot-, black]   # optional colour; otherwise the net's colour
  i2c:
    - [Pico.GP16, U1.SDA]
    - [Pico.GP17, U1.SCL]
    - [U1.SDA, J1.SDA]
    - [U1.SCL, J1.SCL]
    - [bot-, J1.GND]

nets:                         # what should be connected; the check holds the board to this
  3V3: {pins: [Pico.3V3, U1.VCC, C1.1], color: red}
  GND: [Pico.GND, U1.GND, C1.2, D1.K, J1.GND]
  SDA: [Pico.GP16, U1.SDA, J1.SDA]
  SCL: [Pico.GP17, U1.SCL, J1.SCL]
  LED_DRIVE: [Pico.GP15, R1.1]
  LED: [R1.2, D1.A]
```

## Holes

- Terminal holes are `<row><column>`: rows `a`-`e` and `f`-`j`, columns from 1. Each column's
  a-e is one strip and its f-j another, with the trench between e and f (0.3 in apart).
- Rails are `top+`, `top-`, `bot+`, `bot-`, with a column: `top+12`. Rail holes come in groups
  of five, so some columns have none:

  | board | columns with rail holes | columns without |
  |---|---|---|
  | `full` (63 columns) | 3-7, 9-13, 15-19, 21-25, 27-31, 33-37, 39-43, 45-49, 51-55, 57-61 | 1, 2, 8, 14, 20, 26, 32, 38, 44, 50, 56, 62, 63 |
  | `half` (30 columns) | 2-6, 8-12, 14-18, 20-24, 26-30 | 1, 7, 13, 19, 25 |

  A pin that goes to a rail wants a column with rail holes, or its wire can't run straight.
- With several boards (placed left to right), prefix the board id: `2:a12`. Board ids default to
  1, 2, ...
- A board with `below: 1` is snapped on under board 1 instead, long side to long side, on the same
  0.1 in grid: its row `a` is 1.0 in under board 1's row `i`, so a 1.0 in wide module (an ESP32
  DevKit) sits across the two with a row of pins in each. A wire from one board to another is
  always a flexible jumper.

## Wire ends and legs

A wire end or a two-lead part's leg is one of:

- a hole: `a12`, `top+30`
- a bare rail: `top+`, which takes the free rail hole nearest the other end
- a part pin: `U1.3`, `U1.VCC`, `Pico.GP15`. This takes a free hole in that pin's strip. A name
  shared by several pins is ambiguous unless they're joined on the part (`Pico.GND` is fine), or
- an off-board pin (`J1.SDA`). Wires only; legs go in holes.

A wire can bend on its way: `{from: U1.OUT, to: U1.IN-, via: [d43, g43]}` runs through those
holes in order. Bends are drawn and listed in the checklist; they don't take up the holes.

A flexible jumper (`{from: U2.EN, to: U1.EN, flex: true}`) arches over the board instead of lying
on it, like a Dupont jumper wire: it may slant and cross parts and wires, since it's above them,
and the holes it passes over stay free. It still needs a hole at each end. It's drawn as an arch
with square plugs, and the checklist lists it last, with a stock length (10, 20 or 30 cm), since it
goes in after everything it arches over.

Pins that share a name without being joined on the part (five header pins all labelled `GND`) are
named by number instead: `J1.3`, `J1.6`.

bbgen picks the holes for both ends together. It prefers a pair that makes the wire (or the part)
run straight along a row or a column without lying along another wire or part, then the one over
the fewest part bodies, then the holes nearest the pins, then the shortest. Give rail columns
yourself when both ends are rails (`[top+28, top-28]` for a decoupling cap), since there's no other
end to steer by.

## Automatic wiring

With `autowire: true` at the top of the file, bbgen adds the wires itself: place the parts, list
the nets, and leave `wires` out (or write only the ones you want a say in; they're kept). Each net
is joined with the fewest, shortest wires it can find, preferring straight runs and rail drops
from the row next to the rail, and each off-board pin gets its own flying lead. The checklist lists
them by net, rails' nets first.

A wire bbgen lays on the board is always straight, crosses no part, runs along no other wire, and
lies over no other wire's end or a pin's last free hole. Where no such wire will do, it goes the
way you would by hand: along to a strip nothing uses yet and on from another of its holes (or
across the trench in its column), two or three straight wires in all, like a feedback wire taken
round the end of a chip. Only when that's no good either does it use a flexible jumper: a crowded
board gets a few jumpers arching over it rather than bent or slanted wires, and a jumper over a
part (worse, over a tall one) costs more than one over bare board. The shortest nets are wired
first, since they have the fewest ways to go; a long one can still go round them. `flex: false` at the top of the file turns flexible jumpers off, for an all-rigid
build; then bbgen bends wires around parts, and warns about what it couldn't lay straight.

A net that's a supply says `rail: +`, a ground `rail: -`; bbgen picks which rail. Every supply
gets a `+` rail of its own and every ground a `-` one (a full board has two of each, four with
`split_rails: true`, and each further board adds more); with more than that, bbgen stops with an
error rather than putting two voltages on one rail. `rail: top+` (or a list, `rail: [top-, bot-]`)
puts a net on those rails instead, and `rail: none` keeps a net off the rails. A net that says
nothing is taken as a ground or supply by its name (`GND`, `VSS`; `VSYS`, `VBUS`, `VCC`, `5V`,
`3V3`, ...), and the checklist says it assumed so.

Which rail a net runs on comes from, in order:

- the rails its parts already plug into (a decoupling cap with a leg in `top+28`)
- the rails its `rail:` names
- a lone ground takes the `-` rails no other net uses; each supply a free `+` rail of its own (with
  two, placement tries both ways round)
- the lone ground (or lone supply) also takes the free rail on the other side of the trench when it
  has pins there that nothing joins to a rail, and dropping them to it saves more than the wire
  linking the two rails: a pin under a Pico then drops to the bottom rail instead of a jumper
  arching over the Pico

A rail nothing on the board uses isn't wired. Two rails of one net are joined at their ends (the
outermost column where a straight link fits), out of the circuit's way: that's setting up the
board, not the circuit. A flying lead from a supply or ground pin always lands on its net's rail,
never in a strip in the middle of the board, and as far out along the rail as it can: at the end
nearest the connector. Those rail ends are kept free of parts, and wires avoid them, so the leads
come in from the board's edge; a flying lead avoids passing over parts and wires (never over a
tall one: a pot, a can, an LED, a transistor).

Anything that ends up slanted or crossing still gets its warning: then the fix is to move a part.
Many flexible jumpers say the same: parts that talk to each other are far apart or crowded.

## Automatic placement

Leave out a part's place and bbgen places it: `at` (and `rot`) on a Pico, DIP or SIP, `pins` on a
resistor, capacitor, diode or LED. A file can then be just parts and nets; bbgen places, wires
(`autowire` comes on by itself) and checks it in a few seconds. Anything you do give is kept, so
pin a part with `at:` where you want it and let bbgen fit the rest around it. A strip you put a leg or a
wire end in is that net's: bbgen puts no other net's pin there, and no pin or body on a hole you've taken.
If that leaves a part nowhere to go, the build stops and names what's in the way. A chip goes near what
it's wired to, your hand-placed parts included: place the LEDs, and the chip driving them through resistors
lands beside them.

```yaml
parts:
  Pico: {type: pico}
  U1: {type: dip, pins: 8, title: 24LC256, labels: [A0, A1, A2, GND, SDA, SCL, WP, VCC]}
  C1: {type: capacitor, value: 100nF, near: U1}   # near: keep it by that part
  R1: {type: resistor, value: 330R}
  D1: {type: led}
nets: ...
```

- Every leg of a part bbgen places must be in a net: that's how it knows where the part goes.
- A DIP is placed straddling the trench. A 0.3 in one (`width: 3`) sits with a row each side of it;
  a 0.6 in one (an SMD adapter, `width: 6`) can sit four ways (rows `b` and `f` up to `e` and `i`),
  and bbgen picks the one that leaves room where its wires go. A 1.0 in part (an ESP32 DevKit)
  would cover one side's strips whole, so give it `at:` (or use two boards, as builders do).
- A decoupling cap (both nets on rails) goes next to a pin of its supply that has none yet;
  `near:` overrides that.
- With several boards (`boards: [full, full]`), bbgen places parts on any of them, each part on
  one board; a net's rails run on every board, linked across at their ends.
- The same file always gives the same layout. The checklist says where everything went.
- It builds the layouts it tries several at a time, one per CPU core; `BBGEN_JOBS=1` (or any other
  number) in the environment changes that. The layout it ends with is the same either way.

A part wider than the trench (a Pico) may sit a row off centre, when its busy side needs the
room. It places by trying layouts and building the best ones for real, then repairing the best build a
move at a time (turning chips, moving them all a row, re-placing the two-lead parts): it prefers straight wires and parts, short wires, parts with room around them,
and no warnings. It's a good start, not an oracle: if something reads badly, give that part a
place and build again.

## Straight, not slanted

Wires and parts that slant are hard to trace and get in the way. bbgen warns (`DIAGONAL`) only
when one could have been straight, and says how:

- a hole written out where another hole in the same strip would line up (`bot+48 instead of bot+47`)
- a strip in a column the rails have no hole in, so it can't reach the rail straight (move the part)
- every straight route blocked by other wires or parts (it names them)

A wire crossing the trench between two different columns can't be straight, and flying leads to
off-board pins and flexible jumpers don't count. When a slant is the right call, say why, and the warning goes:
`slant: <reason>` on the part, or `{from: ..., to: ..., slant: <reason>}` for a wire. A wire with
`via` is routed by you and isn't checked.

## Off the parts

A wire or two-lead part lying over a part's body hides its pins and has to come out before the
part can. bbgen warns (`OVER_PART`), and for a wire suggests bends that take it around:
`via: [d43, g43]`. The checklist lists the bends with the wire. It also warns about a wire that
ends in a hole under a two-lead part's body (a resistor standing up to a rail covers the holes
between its legs), and one that runs along another wire or part, hiding it. Wires may cross. An LED
or a can counts as a body about two holes across, and a wire bbgen lays keeps a quarter of a hole
from a tall part (a pot, a transistor, a regulator); a DIP's or a module's body hugs its pins.

Some parts need room beyond their body: a Pico needs space for its USB cable's plug past the USB
end (rows `e`-`f`, the 8 columns from 2 past pin 1; the "USB Cable" zone of KiCad's Pico
footprint). The holes in it are taken, so nothing can go there (`HOLE_TAKEN`), a wire or part
across it gets `OVER_PART`, the drawing hatches it, and the checklist says which holes to keep
clear. Put the USB end at the board's edge and it costs no holes at all.

A module that stands on its headers (an ESP32 DevKit, a Pico) can say `raised: true`: then the
holes under it aren't taken, and flat wires may end there or run beneath it, out past its ends
(never across a row of its pins: the header's plastic sits on the board). They go in before it,
and the checklist lists the module after them and marks each one that runs under it; the drawing
shows it see-through over them. Nothing else fits under it: a two-lead part's leg, or a flexible
jumper's or a flying lead's plug, there is an error (`HOLE_TAKEN`). That's how a 1.0 in wide
module fits on one board, with its pins in rows `b` and `j`.

## Where parts land

The rows are spaced like the holes, with the trench counting as two more: `a`-`e` are 1 apart,
and `e` to `f` is 3. So:

- **A DIP** (`width: 3`) straddles the trench only with pin 1 in row `f` (`rot: 0`) or row `e`
  (`rot: 180`). Anywhere else its two rows land in the same half, shorting each pair of
  opposite pins, or in the trench (`OFF_HOLES`). Unrotated, pins 1 to n/2 run right from pin 1
  and the rest come back along the row above, so the last pin sits above pin 1. `rot: 180`
  turns that around: pin 1 top-right, pins running left, notch to the right. `rot: 90` and
  `270` stand a part across the columns, which puts each row of pins in one strip.
- **A Pico** with pin 1 in `h<col>` has pins 1-20 in row `h`, running right from `<col>`, and
  pins 21-40 in row `c`, running back left, so pin 40 is above pin 1. Its body blocks rows `d`
  to `g`, which leaves each of its strips two free holes: `a` and `b` on top, `i` and `j` below.
  Only two wires can run along the rows from one side of the Pico over the same stretch, so
  spread its connections over both sides and both ends.

## Parts from KiCad's library

Name a KiCad symbol and bbgen takes the part from KiCad's library: its pin names from the symbol,
and from its footprint where the pins sit, the body's outline and any keep-out zones (like the
Pico's USB cable).

```yaml
parts:
  U1: {symbol: Timer:NE555P}                                    # pins named from the symbol
  Pico: {symbol: MCU_Module:RaspberryPi_Pico}                   # a module: GND pins joined, USB keep-out
  U2: {symbol: Memory_EEPROM:24LC256, footprint: Package_DIP:DIP-8_W7.62mm}  # the symbol names none
  RV1: {footprint: Potentiometer_THT:Potentiometer_Bourns_3296W_Vertical}    # no symbol: pins by number
  U3: {symbol: Regulator_Linear:AP2112K-1.8, adapter: {pins: 6, map: {5: 6}}} # SMD, on an adapter
```

- `bbgen find ap2112` searches the library and shows each symbol's name, footprint and
  description.
- Pins go by the symbol's names, without KiCad's markup (`~{RST}` is `/RST`, `V_{IN}` is
  `VIN`), or by number. Pins sharing a power name (several `GND`) are joined inside the part.
- A footprint's pads must sit on the 0.1 in grid: through-hole parts mostly do. A surface-mount
  part needs `adapter:`, the SMD-to-DIP board it sits on: its pin count, and `map` for pads that
  don't land on the adapter pin with their own number. Check that against your adapter; the
  checklist says what was assumed.
- `rot: 0` lays a part out like bbgen's own DIP: pin 1 bottom-left, its row running right. Some
  footprints come out as a column (a trimmer's three legs) and need `rot: 90` to lie along a
  row; leave `at` out and bbgen chooses.
- bbgen finds the library where KiCad installs it (system packages, Flatpak, macOS, Windows, or
  `KICAD*_SYMBOL_DIR`). Point it elsewhere with `kicad: /path` in the file or `$BBGEN_KICAD`.

## Part types

| type | placed by | notes |
|---|---|---|
| `pico` | `at` (pin 1), `rot`, or bbgen | Raspberry Pi Pico, pins named GP0..GP28, GND, VSYS, 3V3, ... Unrotated: USB left, pins 1-20 along the bottom. Its GND pins are joined on the board; AGND isn't. Its body blocks rows d-g beneath it (unless `raised: true`: on its headers, with flat wires under it), and its USB cable rows e-f past the USB end. |
| `dip` | `at` (pin 1), `rot`, or bbgen | `pins`, `width` (row spacing in 0.1 in, default 3), `labels`. Unrotated: pin 1 bottom-left, notch left. Use it for SMD-to-DIP adapters (`width: 6`) and modules like an ESP32 DevKit (`width: 10`) too; `raised: true` for one standing on its headers, with flat wires under it. |
| `sip` | `at` (pin 1), `rot`, or bbgen | a single row: headers, trimmers |
| `resistor`, `capacitor`, `electrolytic`, `diode`, `led` | `pins: [leg1, leg2]`, or bbgen | `value`. Polarised legs are `+`/`-` and `A`/`K`, in that order. An LED is drawn from above, the flat on its rim at the cathode; its `color` is `red` (the default), `green`, `yellow`, `blue`, `white`, `orange` or `#rrggbb`. |
| any, from KiCad | `at` (pin 1), `rot`, or bbgen | `symbol: Library:Name`, `footprint: Library:Name`, `adapter` (see above) |
| `offboard` | drawn beside the board | `labels`, `cols`, `numbering: row` (1, 2 across the first row) or `column`, `place: right` or `below`. A label ending in `!` is drawn red. |

`rot` is 0, 90, 180 or 270, clockwise. `labels` is a list (one per pin) or a `{pin: name}` map.
Pins without a label are named by number. An off-board part has `pins` pins, or without that,
one per label (a list) or up to the highest pin in the map. Every part takes `title`, `value`
and `color`; a
resistor's `value` (`10k`, `4k7`, `330R`) is drawn as its colour bands. Two-lead parts also take
`slant` (see above).

## Colours

Wires, nets and parts take `red`, `black`, `blue`, `green`, `yellow`, `orange`, `white`, `purple`,
`gray`, `brown`, `pink`, `cyan`, or `#rrggbb`; any other colour is an error. A wire is its net's
colour unless it says otherwise. A net without a colour gets black if it's a ground, red if it's a supply (orange, pink
and brown for a second, third and fourth supply), and otherwise a colour no other net has, until
they run out.

## What the check reports

| code | meaning |
|---|---|
| `OPEN` | a net is in more than one unconnected piece |
| `SHORT` | two nets are connected together |
| `STRAY` | a pin that's in no net is connected to something |
| `SHORTED_PART` | both legs of a two-lead part are in the same connection |
| `HOLE_TAKEN` | two things in one hole, a hole under a part's body, or a leg or a plug (a flexible jumper's, a flying lead's) under a raised module, where only flat wires fit |
| `OFF_HOLES` | a pin lands in the trench or off the board |
| `NOT_PLACED`, `TWO_NETS`, `BAD_*` | a pin in a net isn't on the board; a pin in two nets; a reference that doesn't resolve |
| `BAD_RAIL` | a net's `rail:` isn't a rail, names one another net already uses, puts a supply on a `-` rail (or a ground on a `+` one), or no rail is left for it |
| `END_COVERED` | a wire ends in a hole that a wire put in before it lies over (wires go in by number, flexible jumpers last): it can't be plugged in |
| `DANGLING` (warning) | a wire ends in a strip with nothing else in it |
| `NO_EFFECT` (warning) | both ends of a wire are in the same strip |
| `DIAGONAL` (warning) | a wire or two-lead part slants where it could run straight; the message says how |
| `OVER_PART` (warning) | a wire or two-lead part lies over a part's body (for a wire, the message gives bends around it), a wire ends under a two-lead part or runs along a part, a two-lead part lies across a raised module, or a flexible jumper arches over a keep-out |
| `NESTED` (warning) | a wire lies over one put in before it, end to end: buildable, but it hides the inner one |
