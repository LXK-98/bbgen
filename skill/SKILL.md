---
name: breadboard
description: Lay out a circuit on a breadboard with bbgen. Write the circuit YAML, then render the breadboard drawing (SVG/PNG) and build checklist, and check the wiring against the intended nets. Use when the user wants a breadboard layout, a wiring diagram to build from, or build instructions for a circuit.
---

# Breadboard layouts with bbgen

`bbgen` (on PATH, source in `~/Documents/Code/bbgen`) turns a circuit YAML into a drawing, a
build checklist and a wiring check. The full file format is in
`~/Documents/Code/bbgen/README.md`. Read it before writing a circuit file.

## The loop

1. **Get the nets right first.** Take them from the project's own design (schematic, netlist,
   design doc), not from memory. Every pin that should connect to something goes in `nets`.
2. **Write the YAML** next to the project's other hardware files (e.g. `hardware/breadboard/`).
   Place the big parts first (`at`, `rot`), then the two-lead parts, then the wires, grouped in
   the order the user should build them (power, then signals, ...). Follow the layout rules
   below while placing: that's cheaper than moving parts later.
3. **Run `bbgen build <file>`.** Fix *every* error. The check is the point: never hand over a
   layout with errors, and never silence one by dropping a pin from `nets`.
4. **Work through the warnings.** Each `DIAGONAL` says how to straighten that wire or part: take
   the hole it suggests, move the part it names, or make room. If a slant really is the best
   option, record why with `slant: <reason>` instead of leaving the warning. Rebuild after each
   round.
5. **Look at the PNG** with the Read tool, for what the check can't see: parts crowding each
   other, labels you can't read. Trust the check over your eye for which hole a wire is in.
6. Hand over the `.png`/`.svg` and the `.md` checklist. Say what you assumed, and list any
   `slant` reasons you gave.

## Rules

- The generated `.svg`, `.png` and `.md` are outputs: edit the YAML and rebuild.
- Physical facts you can't know (adapter row spacing, how an adapter maps chip pins to its
  header pins, which way the user's rails are printed) are assumptions. Put them in `notes` and
  tell the user to check them.
- Use part pins (`U1.VCC`) for wire ends and let bbgen pick the holes. Give explicit holes only
  where you need a specific position, such as junction strips or both ends on rails.
- Mark dangerous off-board pins with a trailing `!` in their label (drawn in red).

## Layout rules

Wires and parts run straight along a row or a column, never slanted when it can be avoided.

- **Rails skip every sixth column** (8, 14, 20, 26, ... on a full board). Place each part so
  every pin that goes to a rail, directly or through a two-lead part, sits in a column that has
  rail holes. Check this before the first build.
- **Leave room for parallel runs.** Each wire running along a row needs a row of its own, and a
  strip only has the holes its pins don't use (a Pico's strips have two). Spread parts out, or
  run connections from both sides of the board, rather than stacking wires on one row.
- **Two-lead parts** lie along a row, or along a column to a rail. When you write a hole for one
  leg, put it in the row or column of the other leg.
- **A wire that must change both row and column** (across the trench between two columns) is
  fine as is; bend it with `via` only if it would cross a part.
