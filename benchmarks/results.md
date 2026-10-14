# Benchmark results

Compared with baseline.json where it differs: ✓ better, ✗ worse.

| circuit | err | warn | slant | bent | cross | flex | links | labels | wire | lead | cols | s | notes |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| 01-led | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 15.9 | 3 | 0.11 |  |
| 02-pico-button | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 38 (+16 ✗) | 0 | 29 (+5 ✗) | 2.22 (-2.91 ✓) |  |
| 03-pico-pot | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 27 | 0 | 29 | 2 |  |
| 04-555-blinker | 0 | 0 | 0 | 0 | 0 | 0 (-1 ✓) | 0 | 0 (-1 ✓) | 47 (+8 ✗) | 16.6 (+0.7 ✗) | 8 (+1 ✗) | 6.2 |  |
| 05-555-4017-chaser | 0 | 0 | 0 | 0 | 2 | 4 (-1 ✓) | 0 | 0 (-1 ✓) | 85 (+20 ✗) | 15.7 | 19 (+1 ✗) | 11.02 (-5.48 ✓) |  |
| 06-lm386-amp | 0 | 0 | 0 | 0 | 2 | 0 | 0 | 0 | 37 | 90.3 | 12 | 7.02 |  |
| 07-lm358-amp | 0 | 0 | 0 | 0 | 1 (-1 ✓) | 1 (-1 ✓) | 0 | 0 | 48 (+21 ✗) | 85.2 (-0.8 ✓) | 7 | 7.48 |  |
| 08-relay-driver | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 20 (-12 ✓) | 63.7 (+19.9 ✗) | 38 (-10 ✓) | 7.41 |  |
| 09-pico-595-leds | 0 | 0 | 0 | 0 | 0 | 2 (-1 ✓) | 0 | 0 | 108 (+3 ✗) | 0 | 40 (+1 ✗) | 12.77 |  |
| 10-atmega328-breadboard | 0 | 0 | 0 | 0 | 0 (-2 ✓) | 0 | 0 | 0 | 26 | 54.9 (+5.5 ✗) | 17 | 4.05 |  |
| 11-l293d-motors | 0 | 0 | 0 | 0 | 4 (+1 ✗) | 2 (-2 ✓) | 0 | 0 | 72 (+19 ✗) | 214.2 (+2.2 ✗) | 31 (+2 ✗) | 6.39 |  |
| 12-pico-i2c | 0 | 0 | 0 | 0 | 2 (+2 ✗) | 1 (-2 ✓) | 0 | 0 | 101 (+29 ✗) | 0 | 43 | 19.57 (-8.24 ✓) |  |
| 13-ben-eater-register | 0 | 1 (-2 ✓) | 0 (-2 ✓) | 0 | 12 (-21 ✓) | 16 (+1 ✗) | 0 | 0 | 135 (-8 ✓) | 338.8 (+28.7 ✗) | 36 (+3 ✗) | 26.29 (+10.74 ✗) | OVER_PART 1 |
| 14-7seg-multiplexed | 0 | 2 | 2 | 0 | 19 (-13 ✓) | 17 (-4 ✓) | 0 | 0 | 76 (+28 ✗) | 0 | 63 (+1 ✗) | 23.24 (+12.05 ✗) | OVER_PART 2 |
| 15-ipod-m1 | 0 | 0 | 0 | 0 | 5 (+1 ✗) | 2 (-2 ✓) | 0 | 0 (-1 ✓) | 87 (+30 ✗) | 167.9 (+1 ✗) | 48 (+1 ✗) | 18.51 (-7.64 ✓) |  |
| 16-ipod-m2 | - | - | - | - | - | - | - | - | - | - | - | - | CircuitError: bbgen found no way to fit these parts on the board without overlapping; place the biggest yourse |
| 17-esp32-devkit | - | - | - | - | - | - | - | - | - | - | - | - | CircuitError: parts.ESP32: a 1 in DIP can't straddle the trench with a free hole by each pin; give it at: |
| 18-traffic-light | 0 | 0 | 0 | 0 | 0 | 1 (-1 ✓) | 0 | 0 | 35 (+11 ✗) | 0 | 29 | 4 (-1.57 ✓) |  |
| 19-two-supplies | 0 | 0 | 0 | 0 | 1 (+1 ✗) | 0 | 0 | 0 | 14 (+3 ✗) | 18.4 (+0.1 ✗) | 9 (-2 ✓) | 8.18 |  |

17/19 built; 0 errors, 3 warnings, 2 slanted, 46 flexible jumpers, 166.5 s in all
