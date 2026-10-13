# Benchmark results

Compared with baseline.json where it differs: ✓ better, ✗ worse.

| circuit | err | warn | slant | over | wire | flex | lead | cols | s | notes |
|---|---|---|---|---|---|---|---|---|---|---|
| 01-led | 0 | 1 | 1 | 0 | 0 | 0 | 15.7 | 1 | 0.04 | DIAGONAL 1 |
| 02-pico-button | 0 | 0 | 0 | 0 | 18 (-5 ✓) | 0 | 0 | 25 (+1 ✗) | 2.04 |  |
| 03-pico-pot | 0 | 0 | 0 | 0 | 29 | 0 | 0 | 25 | 1.81 |  |
| 04-555-blinker | 0 | 0 | 0 | 0 | 31 (-7 ✓) | 1 | 15.7 | 8 (-1 ✓) | 3.91 |  |
| 05-555-4017-chaser | 0 | 0 | 1 | 0 | 73.1 (-9 ✓) | 0 | 15.7 | 16 | 10.09 (+2.4 ✗) |  |
| 06-lm386-amp | 0 | 0 | 0 | 0 | 25 | 0 | 90.4 | 10 | 7.87 |  |
| 07-lm358-amp | 0 | 0 | 0 | 0 | 25 (-16 ✓) | 1 | 84.5 (+1 ✗) | 8 | 5.24 |  |
| 08-relay-driver | 0 | 1 | 0 | 0 | 20 | 0 | 20 | 36 | 3.18 (+0.81 ✗) | DANGLING 1 |
| 09-pico-595-leds | 0 | 0 (-2 ✓) | 2 | 0 | 32.2 (-83 ✓) | 3 | 0 | 45 | 5.63 (+2.29 ✗) |  |
| 10-atmega328-breadboard | 0 | 0 | 0 | 0 | 6 (-17 ✓) | 1 | 68.9 | 17 | 1.82 |  |
| 11-l293d-motors | 0 | 0 (-5 ✓) | 0 (-2 ✓) | 0 | 26 (-71.1 ✓) | 4 | 105.4 | 30 (-2 ✓) | 3.5 |  |
| 12-pico-i2c | 0 | 0 (-1 ✓) | 1 (+1 ✗) | 0 (-1 ✓) | 97.6 (-14.4 ✓) | 1 | 0 | 50 | 5.95 (-3.7 ✓) |  |
| 13-ben-eater-register | 0 | 0 (-23 ✓) | 0 (-3 ✓) | 0 | 158 (-320.5 ✓) | 13 | 268.8 (-17.8 ✓) | 34 (+2 ✗) | 17.81 (+9.29 ✗) |  |
| 14-7seg-multiplexed | 0 | 2 (-19 ✓) | 2 (-10 ✓) | 0 (-12 ✓) | 168 (-527.1 ✓) | 15 | 0 | 62 | 14.2 | OVER_PART 2 |
| 15-ipod-m1 | 0 | 0 | 0 | 0 | 94 (-6 ✓) | 0 | 143 (-5.4 ✓) | 51 | 11.65 (+4.81 ✗) |  |
| 16-ipod-m2 | - | - | - | - | - | - | - | - | - | CircuitError: parts.U1: bbgen only places 0.3 in DIPs; give it at: |
| 17-esp32-devkit | - | - | - | - | - | - | - | - | - | CircuitError: parts.ESP32: bbgen only places 0.3 in DIPs; give it at: |

15/17 built; 0 errors, 4 warnings, 7 slanted, 39 flexible jumpers, 94.7 s in all
