# Benchmark results

Compared with baseline.json where it differs: ✓ better, ✗ worse.

| circuit | err | warn | slant | over | wire | lead | cols | s | notes |
|---|---|---|---|---|---|---|---|---|---|
| 01-led | 0 | 1 | 1 | 0 | 0 | 15.7 | 1 | 0.04 | DIAGONAL 1 |
| 02-pico-button | 0 | 0 | 0 | 0 | 23 | 0 | 24 | 2.89 |  |
| 03-pico-pot | 0 | 0 | 0 | 0 | 29 | 0 | 25 | 1.87 |  |
| 04-555-blinker | 0 | 0 | 0 | 0 | 38 | 15.7 | 9 | 4.2 |  |
| 05-555-4017-chaser | 0 | 0 | 1 | 0 | 82.1 | 15.7 | 16 | 7.29 |  |
| 06-lm386-amp | 0 | 0 | 0 | 0 | 25 | 90.4 | 10 | 7.36 |  |
| 07-lm358-amp | 0 | 0 | 0 | 0 | 41 | 83.5 | 8 | 5.83 |  |
| 08-relay-driver | 0 | 1 | 0 | 0 | 25 | 45.7 | 36 | 7.72 | DANGLING 1 |
| 09-pico-595-leds | 0 | 2 | 2 | 0 | 115.2 | 0 | 45 | 3.87 | OVER_PART 2 |
| 10-atmega328-breadboard | 0 | 0 | 0 | 0 | 23 | 68.9 | 17 | 1.38 |  |
| 11-l293d-motors | 0 | 5 | 2 | 0 | 97.1 | 105.4 | 32 | 3.37 | DIAGONAL 1, OVER_PART 4 |
| 12-pico-i2c | 0 | 1 | 0 | 1 | 113 | 0 | 51 | 9.53 | OVER_PART 1 |
| 13-ben-eater-register | 0 | 23 | 3 | 0 | 478.5 | 286.6 | 32 | 8.15 | DIAGONAL 2, OVER_PART 21 |
| 14-7seg-multiplexed | 11 | 18 | 14 | 14 | 391.2 | 0 | 61 | 10.92 | DIAGONAL 7, HOLE_TAKEN 11, OVER_PART 11 |
| 15-ipod-m1 | 0 | 0 | 0 | 0 | 100 | 146.9 | 50 | 6.23 |  |
| 16-ipod-m2 | - | - | - | - | - | - | - | - | CircuitError: parts.U1: bbgen only places 0.3 in DIPs; give it at: |
| 17-esp32-devkit | - | - | - | - | - | - | - | - | CircuitError: parts.ESP32: bbgen only places 0.3 in DIPs; give it at: |

15/17 built; 11 errors, 51 warnings, 23 slanted, 80.7 s in all
