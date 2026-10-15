# Benchmarks

Twenty-one circuits someone might really build on a breadboard, from an LED and a resistor to the
iPod LCD driver's 18-bit bus. Each file holds only parts and nets: no places, no legs, no wires.
bbgen does all of it, so the run measures bbgen alone.

```bash
python3 benchmarks/run.py              # build them all, compare with baseline.json
python3 benchmarks/run.py 05 m2        # just these
python3 benchmarks/run.py --save       # make this run the baseline
python3 benchmarks/run.py --no-png     # faster
python3 benchmarks/run.py --check      # exit 1 on a missed expectation or a regression
```

Drawings and checklists land in `benchmarks/out/` (not committed); the table in `results.md`.
Columns: errors, warnings, slanted wires and parts (warned about or not), bent wires, pairs of
wires crossing, flexible jumpers, rail links away from the rails' ends, labels hiding a wire end
or another label, on-board wire length and flying-lead length (in holes), columns the layout
spans, and seconds to place, wire and check. A change shows as `(+3 ✗)` or `(-12 ✓)` against the
baseline.

Every circuit is held to no errors, no bent or slanted wires and rail links only at the rails'
ends; a file's `expect:` adds or loosens limits (`expect: {flex: 1, crossings: 0}`), with a comment
saying why. `--check` fails on a missed expectation, or on errors, warnings, slants, bends,
crossings, jumpers, misplaced rail links or label clashes going up against the baseline. The
quick circuits (01, 02, 18, 19) are also checked by the unit tests. After `--save` (which keeps the
drawings as `out/baseline/`), each circuit whose measures change gets its drawing before (top)
and after in `out/diff/`.

| tier | circuits |
|---|---|
| 1, very simple | 01 LED, 02 Pico button + LED, 03 pot into a Pico ADC |
| 2, one chip | 04 555 blinker, 05 555 + 4017 chaser, 06 LM386 amp, 07 LM358 amp, 08 transistor relay driver, 18 Pico traffic light (coloured LEDs), 19 5 V and 3.3 V side by side (two supplies) |
| 3, MCU + chips | 09 Pico + 74HC595 + LEDs, 10 ATmega328P on a breadboard, 11 L293D motors, 12 Pico I2C bus |
| 4, complex | 13 Ben Eater register, 14 multiplexed 7-segment, 15 iPod M1, 16 iPod M2, 17 ESP32 DevKit across two stacked boards, 20 the same ESP32 off the board on leads, 21 the same on one board, wired under it |

Known gaps the circuits work around (noted in each file): a 6 mm tactile switch and an HC-49
crystal are off KiCad's 0.1 in grid, so they're a 4-pin DIP and a two-lead part; TO-92 parts use
`TO-92_Inline_Wide`. 16 takes two boards side by side. The ESP32 DevKitC's rows are 1.0 in apart,
which on one board covers one side's strips whole: 17 puts it across two boards snapped together,
20 off the board on Dupont leads, and 21 on one board with flat wires run under it (`raised: true`).

What the benchmarks still show (13's bus, 14's crowding, leads crossing, 16's size), what was tried
for each and where to start: [docs/open-points.md](../docs/open-points.md).
## For the Rust port

Two more scripts, for [docs/rust-port-plan.md](../docs/rust-port-plan.md):

```bash
python3 benchmarks/hotspots.py         # where the time goes: builds, ms per build, each stage's share
python3 benchmarks/golden.py           # record the golden corpus into golden/
python3 benchmarks/golden.py --diff golden/13-ben-eater-register.jsonl.gz other.jsonl
python3 benchmarks/rs_check.py         # the Rust port (rust/) against the golden records, and its speed
python3 benchmarks/rs_check.py --full  # ... placing from scratch: the whole record, on one core and on all
python3 benchmarks/rs_trials.py        # every trial placement built, rebuilt in Rust: the same score?
python3 benchmarks/rs_outputs.py       # bbgen and bbgen-rs on the same files: the same SVG, checklist, report
```

`hotspots.py` writes `hotspots.md`: each circuit built serially (plain, then under cProfile) and with
the parallel search, the trial builds it needed and the ones workers started for nothing, and the
layout's hash. `golden.py` records each circuit's build event by event ([bbgen/dump.py](../bbgen/dump.py)):
the choices placement weighs, every trial and its score, the wires as they go in and the cost each was
chosen at, the check. It also writes the file as placed (`golden/placed/`), which wires the same layout
without placing anything. A port writes the same events; `--diff` names the first one where the two
part ways (from a placed file's build on, when one side built only that). `rs_check.py` builds `rust/`,
runs it on every placed file, diffs its events with the golden record and times it (`--python` times
the Python too).
