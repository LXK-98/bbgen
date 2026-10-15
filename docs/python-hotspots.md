# Where the time went (the Python bbgen)

The profile of the Python bbgen that decided the Rust rewrite (docs/rust-port-plan.md, Phase 0). The
script that wrote it, `benchmarks/hotspots.py`, went with the Python.

Written by `benchmarks/hotspots.py`. Serial: one process (`BBGEN_JOBS=1`). Shares are of the serial build under cProfile, in percent; the stages nest (autowire is part of the trial builds, `_relay` part of autowire), so they don't add up to 100. *Overhead* is rounding positions into keys and building and hashing `Hole`s. *Outputs*: check, drawing and checklist of the finished circuit. *Wasted*: trial builds a worker started that the search didn't use.

| circuit | serial s | builds | ms/build | outputs ms | autowire | _relay | _reach | _rails | place_legs | anneal | check | overhead | parallel s | speed-up | wasted | hash |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| 01-led | 0.2 | 15 | 12 | 5 | 38 | 0 | 0 | 7 | 47 | 0 | 0 | 12 | 0.2 | 1.3× | 2 | `ad0bd17eb3` |
| 02-pico-button | 3.3 | 99 | 33 | 6 | 20 | 8 | 6 | 3 | 32 | 42 | 0 | 8 | 2.0 | 1.6× | 21 | `c3ada2d524` |
| 03-pico-pot | 4.8 | 45 | 105 | 9 | 7 | 3 | 2 | 1 | 11 | 78 | 0 | 3 | 2.5 | 1.9× | 0 | `562e1ee2d9` |
| 04-555-blinker | 11.6 | 125 | 92 | 9 | 40 | 22 | 16 | 7 | 45 | 13 | 0 | 14 | 3.6 | 3.2× | 33 | `f19f7bd532` |
| 05-555-4017-chaser | 19.1 | 132 | 144 | 49 | 56 | 39 | 29 | 5 | 35 | 8 | 0 | 15 | 10.9 | 1.8× | 99 | `b016f14062` |
| 06-lm386-amp | 13.6 | 183 | 74 | 15 | 37 | 6 | 4 | 13 | 28 | 33 | 0 | 8 | 5.7 | 2.4× | 19 | `5f9d6b82a9` |
| 07-lm358-amp | 10.5 | 157 | 67 | 19 | 44 | 23 | 16 | 6 | 42 | 11 | 0 | 14 | 4.6 | 2.3× | 56 | `8be43d8845` |
| 08-relay-driver | 9.2 | 150 | 61 | 8 | 20 | 8 | 6 | 1 | 44 | 30 | 0 | 9 | 4.7 | 2.0× | 93 | `32d7701fc7` |
| 09-pico-595-leds | 19.8 | 143 | 138 | 27 | 43 | 23 | 17 | 3 | 39 | 16 | 0 | 13 | 5.4 | 3.7× | 46 | `c0c7e9b716` |
| 10-atmega328-breadboard | 4.9 | 153 | 32 | 10 | 44 | 6 | 4 | 11 | 25 | 25 | 1 | 9 | 2.5 | 2.0× | 21 | `4ee55676c7` |
| 11-l293d-motors | 7.8 | 123 | 63 | 33 | 43 | 15 | 8 | 4 | 29 | 22 | 0 | 11 | 3.2 | 2.5× | 13 | `5ccedc0fd3` |
| 12-pico-i2c | 22.2 | 156 | 141 | 33 | 39 | 10 | 7 | 8 | 46 | 12 | 0 | 11 | 6.4 | 3.4× | 19 | `85fb99b1e6` |
| 13-ben-eater-register | 36.4 | 159 | 227 | 218 | 70 | 36 | 26 | 13 | 26 | 4 | 0 | 14 | 11.3 | 3.2× | 54 | `e4060bb6ac` |
| 14-7seg-multiplexed | 33.1 | 228 | 144 | 117 | 65 | 30 | 22 | 14 | 17 | 15 | 0 | 11 | 9.5 | 3.5× | 65 | `4ea45b6447` |
| 15-ipod-m1 | 37.0 | 158 | 233 | 68 | 32 | 13 | 9 | 3 | 49 | 17 | 0 | 11 | 11.3 | 3.3× | 24 | `1bd5a04b85` |
| 16-ipod-m2 | 182.1 | 193 | 939 | 1103 | 56 | 22 | 15 | 0 | 36 | 8 | 0 | 13 | 75.2 | 2.4× | 55 | `4fa311a975` |
| 17-esp32-devkit | 91.0 | 186 | 487 | 21 | 73 | 66 | 51 | 1 | 18 | 10 | 0 | 18 | 30.7 | 3.0× | 37 | `400881a370` |
| 18-traffic-light | 9.3 | 121 | 76 | 12 | 27 | 13 | 9 | 4 | 46 | 22 | 0 | 11 | 2.9 | 3.2× | 33 | `a789d8040f` |
| 19-two-supplies | 13.5 | 122 | 109 | 10 | 17 | 12 | 10 | 1 | 24 | 58 | 0 | 7 | 7.3 | 1.8× | 109 | `3500d23c32` |
| 20-esp32-offboard | 32.5 | 207 | 156 | 25 | 24 | 1 | 1 | 2 | 61 | 14 | 0 | 11 | 14.8 | 2.2× | 172 | `82c92d5160` |
| 21-esp32-under | 27.4 | 130 | 209 | 23 | 45 | 31 | 25 | 3 | 34 | 20 | 0 | 13 | 6.6 | 4.2× | 23 | `23f6488a6f` |

In all: 589 s serial, 221 s parallel, 2985 trial builds needed, 3979 started by workers.

Own time by module, all circuits (weighted by serial time):

| module | % |
|---|---|
| Python built-ins | 30.3 |
| autoplace.py | 27.7 |
| autowire.py | 20.8 |
| circuit.py | 12.2 |
| board.py | 7.1 |
| measure.py | 1.4 |
| check.py | 0.3 |
| parts.py | 0.2 |
| kicad.py | 0.0 |
| trace.py | 0.0 |
| costs.py | 0.0 |
