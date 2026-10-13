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
   the order the user should build them (power, then signals, ...).
3. **Run `bbgen build <file>`.** Fix *every* error and think about every warning. The check is
   the point: never hand over a layout with errors, and never silence one by dropping a pin
   from `nets`.
4. **Look at the PNG** with the Read tool. Check that parts don't crowd each other, wires
   aren't needlessly long, and labels are readable. Move things and rebuild until it reads well.
5. Hand over the `.png`/`.svg` and the `.md` checklist. Say what you assumed.

## Rules

- The generated `.svg`, `.png` and `.md` are outputs: edit the YAML and rebuild.
- Physical facts you can't know (adapter row spacing, how an adapter maps chip pins to its
  header pins, which way the user's rails are printed) are assumptions. Put them in `notes` and
  tell the user to check them.
- Use part pins (`U1.VCC`) for wire ends and let bbgen pick the holes. Give explicit holes only
  where you need a specific position, such as junction strips or both ends on rails.
- Mark dangerous off-board pins with a trailing `!` in their label (drawn in red).
