# Benchmark results

Compared with baseline.json where it differs: ✓ better, ✗ worse.

| circuit | err | warn | slant | over | wire | lead | cols | s | notes |
|---|---|---|---|---|---|---|---|---|---|
| 01-led | 0 | 1 | 1 | 0 | 0 | 15.7 | 1 | 0.04 | DIAGONAL 1 |
| 02-pico-button | 0 | 0 | 0 | 0 | 23 | 0 | 24 | 2.57 |  |
| 03-pico-pot | 0 | 0 | 0 | 0 | 29 | 0 | 25 | 1.64 |  |
| 04-555-blinker | 0 | 0 | 0 | 0 | 38 | 15.7 | 9 | 4.46 |  |
| 05-555-4017-chaser | 0 | 0 | 1 | 0 | 82.1 | 15.7 | 16 | 7.69 |  |
| 06-lm386-amp | 0 | 0 | 0 | 0 | 25 | 90.4 | 10 | 6.69 |  |
| 07-lm358-amp | 0 | 0 | 0 | 0 | 41 | 83.5 | 8 | 5.28 |  |
| 08-relay-driver | 0 | 1 | 0 | 0 | 20 (-5 ✓) | 20 (-25.7 ✓) | 36 | 2.37 (-5.35 ✓) | DANGLING 1 |
| 09-pico-595-leds | 0 | 2 | 2 | 0 | 115.2 | 0 | 45 | 3.34 | OVER_PART 2 |
| 10-atmega328-breadboard | 0 | 0 | 0 | 0 | 23 | 68.9 | 17 | 1.37 |  |
| 11-l293d-motors | 0 | 5 | 2 | 0 | 97.1 | 105.4 | 32 | 3.19 | DIAGONAL 1, OVER_PART 4 |
| 12-pico-i2c | 0 | 1 | 0 | 1 | 112 (-1 ✓) | 0 | 50 (-1 ✓) | 9.65 | OVER_PART 1 |
| 13-ben-eater-register | 0 | 23 | 3 | 0 | 478.5 | 286.6 | 32 | 8.52 | DIAGONAL 2, OVER_PART 21 |
| 14-7seg-multiplexed | 0 (-11 ✓) | 21 (+3 ✗) | 12 (-2 ✓) | 12 (-2 ✓) | 695.1 (+303.9 ✗) | 0 | 62 (+1 ✗) | 12.97 | DIAGONAL 7, OVER_PART 14 |
| 15-ipod-m1 | 0 | 0 | 0 | 0 | 100 | 148.4 (+1.5 ✗) | 51 (+1 ✗) | 6.84 |  |
| 16-ipod-m2 | - | - | - | - | - | - | - | - | CircuitError: parts.U1: bbgen only places 0.3 in DIPs; give it at: |
| 17-esp32-devkit | - | - | - | - | - | - | - | - | CircuitError: parts.ESP32: bbgen only places 0.3 in DIPs; give it at: |

15/17 built; 0 errors, 54 warnings, 21 slanted, 76.6 s in all
