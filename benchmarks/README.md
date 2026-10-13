# Benchmarks

Seventeen circuits someone might really build on a breadboard, from an LED and a resistor to the
iPod LCD driver's 18-bit bus. Each file holds only parts and nets: no places, no legs, no wires.
bbgen does all of it, so the run measures bbgen alone.

```bash
python3 benchmarks/run.py              # build them all, compare with baseline.json
python3 benchmarks/run.py 05 m2        # just these
python3 benchmarks/run.py --save       # make this run the baseline
python3 benchmarks/run.py --no-png     # faster
```

Drawings and checklists land in `benchmarks/out/` (not committed); the table in `results.md`.
Columns: errors, warnings, slanted wires and parts, wire runs over a part's body, on-board wire
length and flying-lead length (in holes), columns the layout spans, and seconds to place, wire
and check. A change shows as `(+3 ✗)` or `(-12 ✓)` against the baseline.

| tier | circuits |
|---|---|
| 1, very simple | 01 LED, 02 Pico button + LED, 03 pot into a Pico ADC |
| 2, one chip | 04 555 blinker, 05 555 + 4017 chaser, 06 LM386 amp, 07 LM358 amp, 08 transistor relay driver |
| 3, MCU + chips | 09 Pico + 74HC595 + LEDs, 10 ATmega328P on a breadboard, 11 L293D motors, 12 Pico I2C bus |
| 4, complex | 13 Ben Eater register, 14 multiplexed 7-segment, 15 iPod M1, 16 iPod M2, 17 ESP32 DevKit |

Known gaps the circuits work around (noted in each file): a 6 mm tactile switch and an HC-49
crystal are off KiCad's 0.1 in grid, so they're a 4-pin DIP and a two-lead part; TO-92 parts use
`TO-92_Inline_Wide`. 16 and 17 need 0.6 and 1.0 in wide parts, which bbgen doesn't place yet.
