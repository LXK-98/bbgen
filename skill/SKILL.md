---
name: breadboard
description: Lay out a circuit on a breadboard with bbgen. Write the circuit YAML, then render the breadboard drawing (SVG/PNG) and build checklist, and check the wiring against the intended nets. Use when the user wants a breadboard layout, a wiring diagram to build from, or build instructions for a circuit.
---

# Breadboard layouts with bbgen

`bbgen` (on PATH, source in `~/Documents/Code/bbgen`) turns a circuit YAML into a drawing, a
build checklist and a wiring check. The full file format is in
`~/Documents/Code/bbgen/README.md`. Read it before writing a circuit file, especially "Where
parts land" and the rail columns under "Holes": placing parts needs both.

## The loop

1. **Get the nets right first.** Take them from the project's own design (schematic, netlist,
   design doc), not from memory. Every pin that should connect to something goes in `nets`.
2. **Write the YAML** next to the project's other hardware files (e.g. `hardware/breadboard/`):
   the parts and the nets. Take parts from KiCad's library wherever it has them
   (`symbol: Timer:NE555P`; find names with `bbgen find <words>`): their pin names then come
   from KiCad, not from you. Two-lead parts and connectors stay bbgen's own types.
   Leave out `at`, two-lead parts' `pins` and `wires`: bbgen places and wires everything itself
   (README, "Automatic placement"). Say what each supply and ground is: `rail: +` or `rail: -`
   (bbgen picks the physical rail; each voltage gets its own), and `rail: none` for a net that only
   looks like one. Give a decoupling cap `near:` its chip.
   Only place a part yourself (`at`, `pins`) where you have a reason to, such as the user's
   preference or a physical constraint bbgen can't know.
   If the parts won't fit one board (bbgen says so, or the build is a mass of jumpers), add one:
   `boards: [full, full]` puts them side by side, `{type: full, below: 1}` snaps the second under
   the first. bbgen places parts on any of them; a wire between two boards is always a flexible
   jumper. See "Wide modules" below for a module too wide for one board.
3. **Check the nets against the circuit you meant.** Run `bbgen build <file> --schematic` and look at
   `<file>.schematic.png` with the Read tool: it's the circuit drawn from your nets. Compare it with the
   design (or the datasheet's application circuit): every pin where it should be, supplies and grounds right,
   nothing missing. The wiring check can't catch a wrong net; this is where you do.
4. **Run `bbgen build <file>`.** Fix *every* error. The check is the point: never hand over a
   layout with errors, and never silence one by dropping a pin from `nets`.
5. **Work through the warnings.** Each says how to fix it:
   - `DIAGONAL`: take the hole it suggests, or move the part it names. "Every straight route is
     blocked" usually means parts are crowded on one side: move them to make room before you
     settle for the slant. If a slant really is the best option, record why with
     `slant: <reason>` instead of leaving the warning.
   - `OVER_PART`: add the `via` bends it suggests, or move the part.
   - On a wire or part bbgen placed, the fix is placement: give the part it names an `at` (or
     `pins`) of your own and build again; bbgen fits the rest around it.
   - Flexible jumpers (`flex`, drawn as arches with square plugs) aren't warnings: bbgen uses
     them where no wire could lie straight on the board (first trying two or three straight wires
     through an empty strip), and always between two boards. A few are normal on a busy board;
     many between the same parts mean those parts should sit closer, so place them yourself.

   Rebuild after each round.
6. **Look at the PNG** with the Read tool, for what the check can't see: parts crowding each
   other, labels you can't read. Trust the check over your eye for which hole a wire is in.
7. Hand over the `.png`/`.svg`, the schematic and the `.md` checklist. Say what you assumed, and list any
   `slant` reasons you gave.

## Rules

- The generated `.svg`, `.png` and `.md` are outputs: edit the YAML and rebuild.
- Physical facts you can't know (adapter row spacing, how an adapter maps chip pins to its
  header pins, which way the user's rails are printed) are assumptions. Put them in `notes` and
  tell the user to check them. An SMD part on an adapter says its mapping in `adapter: {map:}`;
  the checklist prints it as an assumption.
- Use part pins (`U1.VCC`) for wire ends and let bbgen pick the holes. Give explicit holes only
  where you need a specific position, such as junction strips or both ends on rails.
- Mark dangerous off-board pins with a trailing `!` in their label (drawn in red).
- `raised: true` assumes the module stands high enough on its headers for flat jumper wires to
  pass under it (true of an ESP32 DevKit or a Pico on standard headers). Say so in `notes`.

## Wide modules

A module's rows of pins straddle the trench. A 0.3 in DIP or a 0.6 in SMD adapter (`width: 6`)
fits one board, and bbgen places it. A 1.0 in module, such as a 38-pin ESP32 DevKitC (`type: dip`,
`width: 10`), covers every free hole beside one of its rows on one board. There are three ways to
build it; ask the user which they want, since it's their hardware:

- **On one board, wired under it:** `raised: true` on the module. Flat wires run under it and
  out past its ends; they go in first and the checklist puts the module in after them. This is
  the tidiest build if the user is happy to lay wires before seating the module.
- **Across two boards:** `boards: [full, {type: full, below: 1}]`, the boards snapped together
  long side to long side; the module sits with a row of pins on each. Wires between the boards
  are flexible jumpers.
- **Off the board:** `type: offboard` with its `labels`, `cols: 2` and `numbering: column` (pins 1 to
  19 down one column, as a DevKit counts them), reached by Dupont leads.

`benchmarks/17-esp32-devkit.yaml`, `20-esp32-offboard.yaml` and `21-esp32-under.yaml` are the
same circuit built each way: start from the one that matches.

## Layout rules

These are what bbgen's own placement aims for; follow them when you place a part yourself.
Wires and parts run straight along a row or a column, never slanted when it can be avoided.

- **Rails skip every sixth column** (8, 14, 20, 26, ... on a full board). Place each part so
  every pin that goes to a rail, directly or through a two-lead part, sits in a column that has
  rail holes. Check this before the first build.
- **Leave room for parallel runs.** Each wire running along a row needs a row of its own, and a
  strip only has the holes its pins don't use. A Pico's strips have two, so no more than two
  wires can leave one side of it over the same stretch: put parts on both sides of the Pico
  (place it mid-board) and at both ends, rather than all to one side.
- **Two-lead parts** lie along a row, or along a column to a rail. When you write a hole for one
  leg, put it in the row or column of the other leg.
- **A wire that must change both row and column** (across the trench between two columns) is
  fine as is; bend it with `via` only if it would cross a part.
- **Supply and ground leads** from an off-board connector land on their net's rail, at the end
  nearest the connector; keep parts out of those last rail holes.
