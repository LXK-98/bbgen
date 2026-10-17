# Open points

What's left to improve in bbgen's placement and wiring, as of the `routing-improvements` branch
(October 2026). Each point says what the problem is, how it shows in the benchmarks, what was
tried and what came of it, and where to start. Numbers are from `benchmarks/results.md` unless
a point says otherwise.

bbgen has since been rewritten in Rust with exactly the same results ([rust-port-plan.md](rust-port-plan.md)):
every layout and number below still holds, only the times are about 25× shorter. How placement and wiring
work is described at the top of [src/autowire.rs](../../src/autowire.rs), [src/search.rs](../../src/search.rs)
and [src/legs.rs](../../src/legs.rs); every weight they trade off is in [src/costs.rs](../../src/costs.rs).
Names below like `_relay` or `_Model.cost` are the Python's; in Rust they're `relay` and `Model::cost`.

## Measuring a change

- `python3 benchmarks/run.py` builds all 21 circuits and compares them with `benchmarks/baseline.json`;
  `--check` exits 1 on any regression; `--no-png` is faster; `run.py 13 14` builds only those. A full
  run takes about 8 s on 8 cores (16 alone about 3 s).
- Check one circuit's numbers with `bbgen dev measure FILE OUTDIR --no-png` (the same JSON run.py
  tabulates) rather than adding up the table by hand.
- To prove a change leaves layouts alone (a refactor, a speed-up), compare the layout hash
  `bbgen dev build FILE` prints (every part's holes and every wire), before and after. `cargo test`
  does this and more for all 21 against `benchmarks/golden/`. `BBGEN_JOBS=1` and the default must
  print the same: the parallel search is meant to be exact.
- A change meant to move wires fails `cargo test` by design: judge it with run.py, then record it with
  `benchmarks/record.py` (it needs PyYAML) and commit the records with it (benchmarks/README.md). It
  leaves a record whose events are the same untouched, so the diff shows only what moved.
- Placement scores every trial with the full wiring, so a wiring change also changes which layout
  wins. To judge a wiring change on its own, wire the placed layouts in `benchmarks/golden/placed/`
  (the benchmarks as placement left them) with the old and the new code.

## Where things stand

Against `main` before this work, over the 17 circuits that built there: jumpers 60 → 46, crossings
80 → 55, warnings 5 → 2, slants 4 → 1, label clashes 3 → 0. 16 (two boards side by side), 17 (two
boards stacked), 20 and 21 (new) build too. The points below are what's left.

## 1. A bus between two chips runs out of rows (13)

**Issue.** 13 (Ben Eater's register) has 16 jumpers, one more than on `main`. Most are straight runs
of the 8-bit bus between the 74LS173s and the 74LS245: each bus line needs a row of its own across
the gap between the chips, and a chip's side has four free rows (a–d or g–j), so lines five to eight
of a bus become jumpers.

**Tried.**
- Routing through empty strips (`_relay` in autowire): it finds routes across the trench in free
  columns, but there are few free columns between packed chips. It cut jumpers elsewhere, not here.
- Wiring the shortest nets first: the bus lines were losing their rows to long control nets
  (LOAD, CLK, CLR) routed before them; shortest-first took 13 from 33 crossings to 12 and its
  warnings from 3 to 1. Kept.
- Rip-up and reroute (taking out a jumper's net and the nets in its way, wiring it first): it traded
  one jumper for another every time, since the rows are simply full. Left out; the reasoning is in
  the notes at the top of src/autowire.rs.
- Placement counting each column's free rows (`taken` in `_Model.cost`) instead of assuming two:
  helped 11, 12, 14 and 15; did nothing for 13.

**Starting points.**
- A placement term for buses: nets that run in parallel between the same two chips (the same pin
  order on both) need one row each across the gap. Placement could leave a gap of several free
  columns between such chips (where wires can cross the trench) or stagger them so half the bus
  goes on each side of the trench.
- Turning one chip so the bus pins face the other chip's on the other half of the board, then
  crossing the trench in free columns at the gap.
- Accept it: a real builder runs a bus like this with jumpers too. Ben Eater's own boards do.

## 2. A wire end under a resistor (13)

**Issue.** 13's one warning: `W13 ends in j33, under R8` (`OVER_PART`). A two-lead part lying along a row
covers the holes between its legs; a wire end in one can't be plugged in once the part is in.

**Tried.** Nothing yet. Autowire already charges `COVERED_COST` (20) for such an end, so every other way
to join that net must have cost more; that's the first thing to confirm.

**Starting points.** Find out what the alternatives cost (print `_cost` for that net's candidate
pairs, as in point 3's diagnosis). Either make `covered` holes unusable when the strip has another
free hole, or have leg placement (`_Legs.costly`) charge a resistor for covering the free holes a
strip of its own net still needs.

## 3. Circuit 14: a resistor over the 74HC595, and a crowded left half

**Issue.** 14 (multiplexed 7-segment) keeps one warning: `R6 lies over U1`, a resistor lying across the
74HC595. It also has 18 jumpers and 25 crossings, mostly jumpers crossing jumpers.

**Diagnosis.** The display's pins at the top sit in strips its body covers (no free hole), and the left
part of the board holds the 595, four transistors and twelve resistors: columns 1–29 had a single
wholly free strip. R6's candidate legs were all between its SF strip (below the trench) and an empty
strip above it, across the chip. (Found by unpinning R6 in the placed layout and printing
`_Legs.choose`'s candidates.)

**Tried.**
- `LEG_OVER_COST` (30) in the layout score for a two-lead part over a body: no change on its own;
  the search had no layout without it to choose.
- `ROOMLESS_COST` in the estimate (each two-lead part between two signal nets wants a free strip
  within 9 columns of its nets): 2 warnings → 1, but 17 → 18 jumpers and 19 → 25 crossings. 10 did
  nothing, 30 lost 05's gain. Kept at 20.
- A wall term (`WALL_COST`) for parts no wire can lie across, like the display: crossings 32 → 21.
- A bigger repair budget (`REPAIR_BUILDS` 240): no change.

**Starting points.** The board is too full for this circuit. Try it on two boards
(`boards: [full, full]`) and see whether the jumpers go; if so, that's the honest fix, and the
estimate could suggest it (a warning when the parts' estimated room is short). Otherwise, let leg
placement put a resistor's leg in an empty strip further away (beyond `ROOM_REACH`) when every
nearer pair lies across a body.

## 4. Flying leads crossing each other (11, 15, 19, 20)

**Issue.** Crossings above `main`'s: 11 has 4 (3 on main), 15 has 5 (4), 19 has 2 (0); 20 (new) has 6 among
its nine Dupont leads. All are flying leads from off-board connectors crossing each other.

**Diagnosis.**
- 11: the motor leads are fixed by the L293D's output pins and the two motor connectors' order.
- 15: the 3V0 lead comes from the connector's bottom pin but 3V0 has the top `+` rail.
- 19: 5V comes from the upper connector pin but has the bottom `+` rail; GND the opposite.
- 20: the ESP32's header pins are in DevKit order; its signal pins land wherever their parts are.

**Tried.**
- `_settle_leads`: once every lead is in, each crossed lead is laid again knowing the rest (15: 8 → 4,
  10: 2 → 0). Kept.
- Crossing leads of the same net swap holes (06: 3 → 2). Kept.
- A lone supply or ground also takes the rail across the trench when its leads are much shorter to it
  (20's 3V3 lead). Kept.
- Keeping chip pins out of the last rail columns kept for leads (in the placement estimate): crossings
  went up (06: 3 → 7, 05: 2 → 5). Reverted.

**Starting points.**
- Choose the rail sides of supplies with the connector in view: a supply whose connector pin is above
  another's should get the upper rail. Today `rail_plan` (autoplace) and `rails` (autowire) fill rails
  top first, and placement tries both orders only for two free supplies (`_rail_order`, `orders` in
  `search::autoplace`).
- 19 is such a case, and the rail order isn't what goes wrong. With 5V first and the LD1117 at `e61`
  turned 270, 19 scores 36.0 (1 crossing, 4 wires, 16.7 holes of lead, 8 columns) against today's 46.6
  (2, 6, 25.3, 11). The search builds that start, but it scores 79 there: the small parts' first legs
  bunch up into a warning that repair would remove. Only the best start (64.3, 3V3 first) is turned and
  repaired, and repair moves parts, never the rail order, so the better layout is never reached.
  - Repairing the best start of each rail order: no change (5V first's best start is the LD1117 at
    `f61`, below the trench, and repair can't take it across); 16 a second slower. Reverted.
  - Repairing every start (8 to 12 of them): 19 gets its 36.0 layout; 14 loses its warning and its
    slant; 02, 09, 18 and 20 get shorter wires or leads. But 13 goes 12 → 36 crossings (16 → 12
    jumpers), 14 25 → 35, 07 1 → 4, and the run takes 53 s instead of 8. Reverted.
  - So two things: the start scores rank on problems repair fixes easily (a start's score is a poor
    guide to where it ends), and once the search finds more, the score lets crossings go cheaply
    (`CROSS_WEIGHT` 2 against `FLEX_WEIGHT` 12 per jumper: 13 traded 4 jumpers for 24 crossings).
    A better search needs the score to hold crossings dearer first, or it makes 13 and 14 worse.
    Cheaper than repairing every start: polish each start a little (a few builds of leg moves) before
    choosing, so the starts are compared as they'd be once their small parts are tidied.
- Where the file doesn't fix the connector's pin order (off-board parts whose labels are just names),
  bbgen could order the pins to match where their nets land; for a real header (the ESP32) it can't.

**Research** (October 2026), on the two halves of 19's finding above.

*Which starts get the repair.* Ranking starts by their raw score is the "greedy trap" the pilot method was
made for: judge each choice by the result of finishing it with a cheap heuristic, not by how it looks now
([Duin & Voß 1999; Voß, Fink & Duin 2005](https://ideas.repec.org/a/spr/annopr/v136y2005i1p285-30210.1007-s10479-005-2060-2.html)).
Repairing every start is that method at full price. Sharing a budget between starts is a bandit problem:
[Györgyi & Kocsis](https://arxiv.org/abs/1401.3894) give each local-search instance resources by how far it
could still get, and need only logarithmically more evaluations than a search started in the right place.
The simple form is successive halving: give every start a small budget, keep the better half, double the
budget, repeat ([overview](https://www.emergentmind.com/topics/successive-halving-sh)). Its known risk is
pruning a slow starter too early, which is 19's `e61` start exactly, so the first round has to be long
enough to tidy the small parts. For bbgen: every start gets a few repair builds, the better half twice as
many, and so on, until one start has the full `REPAIR_BUILDS`. That costs about twice today's one repair,
not 8–12 times.

The closest working tool, [Stripboard Editor](https://stripboard-editor.com/how-auto-layout-works) (simulated
annealing on stripboard), does three things worth copying. It scores during search with a fast approximate
wiring and runs the slow, thorough finish only on new best boards. It makes several runs from different
random starts and keeps the best by the full-price score. And it raises the price of slanted or crossing
wires during the run (25 → 400) while the temperature falls, so rough arrangements can form early and get
tidied late. That is a penalty schedule: a start's fixable faults count for little at first.

*How much a crossing is worth.* A weighted sum works only when the weights say what the user prefers;
otherwise the objectives are put in order instead (lexicographic optimization).
[Paquete & Stützle](https://eden.dei.uc.pt/%7Epaquete/papers/patat2002.pdf) compare two ways to search like
that: comparing level by level (Lex-tie), or meeting each level in turn (Lex-seq). Level by level falls
behind when the top level is hard to meet. [Timefold](https://docs.timefold.ai/timefold-solver/latest/constraints-and-score/overview)
has hard, medium and soft score levels, and warns against a big weight where a level is meant. The
[stripboard ASP paper](https://arxiv.org/html/2512.04910v1) ranks its objectives by priority too (strip
distance, then area, then width). In diagrams, crossings are by far the aesthetic that most affects how
easily people read them ([Purchase 1997](https://eprints.gla.ac.uk/35804)), more than bends. A builder
follows a checklist wire by wire, so that applies less, but it argues for counting crossings dearer than 2.

What to try, in order:
1. Settle what a jumper is worth against crossings (a preference, not a weight to tune): today 1 jumper
   = 6 crossings. Count a flying lead over a board wire apart from two wires on the board, since the lead
   runs above it. Then check that today's layouts keep their numbers.
2. Successive halving over the starts instead of repairing only the best.
3. If the score still trades badly once the search finds more: levels (errors, then warnings, then the
   rest) in place of the 1000 and 15 weights, or Stripboard Editor's rising price for crossings.

**Tried: the new prices alone** (`CROSS_WEIGHT` 6 for two wires flat on the board or two in the air,
`CROSS_OVER_WEIGHT` 1 for a lead or jumper passing over a flat wire). Crossings fall in 13 (12 → 10),
14 (25 → 17), 15 (5 → 4), 16 (204 → 193) and 19 (2 → 1; 19 also gets 8 columns and 16.7 holes of lead,
most of its better layout). They rise in 10, 17 and 18, all of it leads passing over board wires, as
meant. But 13 gains a warning and 16 two (`OVER_PART`). In 16 that's a trade the score now allows (2
warnings at 15 against 11 crossings). In 13 it isn't: the old warning-free layout scores 879.8 under the
new prices, the one found 904.3. The search took another path from its one repaired start and missed
it. Raising warnings to 60 kept both warnings and cost 09 a slant and 4 jumpers, so the weights can't be
judged apart from the search: a change of price changes which start wins and where repair goes. Not
kept yet; worth judging again with step 2 in place, and with each benchmark's old layout scored under
the new prices as a check (`bbgen dev build golden/placed/<name>.yaml` prints its score).

**Tried next, judged by each benchmark's old layout scored under the new prices** (a search change can
then be told from a price change: if the search ends above the old layout, it missed it).
- Repairing every start, with the new prices: 14 of 21 better than their old layouts, 6 the same, 07 still
  missed (total score 7378 → 7150). 19 gets its 30.0 layout, 10 and 18 lose their crossings. But 13 and
  16 take warnings (2 and 4): at 15 a warning is cheaper than a jumper.
- Warnings at 60 in the score itself: 13 drops to 0 warnings, but 19 loses its good layout and 04 and 16
  miss theirs. Repair is a local search, and a dear warning walls off the paths that pass through one.
- Repairing at 15, choosing among the repaired starts at 60 (`PICK_WARNING_WEIGHT`): 13 keeps one warning
  and 13 crossings, 19 keeps 30.0. This is what Stripboard Editor does (cheap penalties while searching,
  the full price to choose).
- Successive halving, simulated on every start's recorded repair (score after each kept move): it gets a
  third of the gain (7580 → 7514 against 7398 for repairing all) for 1.2× the builds; a longer first round
  gets no more. A start's score after a few builds says little about where it ends (13, 14, 20 lose their
  winner early). Not built.
- **Verdict: worse, reverted.** Looked at as drawings, the layouts below read messier than before in most
  cases, although they score lower: parts crammed into a cluster with labels over each other (18), legs
  stretched out to save a wire (02), LEDs scattered over both halves (09). The score rewards short and few
  wires and hardly sees crowding, labels over parts, stretched legs or parts out of line; a stronger search
  finds the layouts that exploit that. The old, weaker search mostly kept the estimate's tidy placements (it
  likes room around parts). So the score has to see tidiness before the search is made stronger, and every
  change has to be judged on the drawings, not only on the numbers.
- What was tried (all reverted): every start turned and repaired, side by side on threads,
  each building its trials one at a time; the winner chosen at the full warning price; the new crossing
  prices. Same layouts at `BBGEN_JOBS=1`. Against the old layouts: 14 better, 5 the same, 07 and 16
  missed (16 takes 2 warnings; at the full price its old layout is better and no start reaches it).
  Crossings over all 21: 265 → 245, jumpers 91 → 91, warnings 2 → 4 (16). The cost is time: 8 s → 33 s
  for all 21, 16 alone 3 s → 11–15 s, since 8 repairs are 8 times the work (16: 54 s on one core).

**Checked again (October 2026, with the supply-lead rules in).** Repairing each start in turn (a temporary
`BBGEN_START` override, not kept) over all 21 benchmarks and the 4 examples: in 12 of 22 circuits with more than
one start, a start other than the one picked ends with a lower score (19: 45.1 → 29.3, 18: 66.0 → 51.9, 13:
783.1 → 682.4, 02: 57.0 → 50.0, 09: 229.4 → 212.3, 17: 174.6 → 162.9). Drawn, most of those lower scores are the
same exploits as above: 18's LEDs leave their row for three places and a label clashes, 02 stretches legs to save
two wires, 09 spreads its LEDs over both halves, 19 packs everything into one corner with labels over the
regulator. The 555s lose nothing (04's chosen start and its first two end within a point). So the verdict stands:
the score, not the choice of start, is what to change first. Picked up as point 12.

**Research (October 2026): measuring tidiness.**
- Learning a layout score from examples is established for graphic design: an energy of design-principle
  terms (alignment, white space, symmetry), weights fitted to a small number of example layouts by nonlinear
  inverse optimization ([O'Donovan, Agarwala & Hertzmann 2014](https://www.dgp.toronto.edu/~donovan/layout/)).
  With pairwise judgments ("this one is tidier"), each pair becomes a margin constraint, score(tidier) <
  score(messier), as in a ranking SVM ([Form + Function](https://arxiv.org/pdf/1912.05047)); with many pairs,
  a Bradley–Terry fit gives a latent score to regress on features ([3D shape study](https://arxiv.org/pdf/2505.12373)).
  bbgen has too few judgments to fit weights, but the pairs work as a test the hand-set weights must pass.
- Breadboard and stripboard tools keep tidiness out of the score by rule: Stripboard Editor enforces
  clearance as a hard constraint and only produces overlap-free packings, scoring area, wires and "wire mess"
  ([how auto-layout works](https://stripboard-editor.com/how-auto-layout-works)); a generative breadboard tool
  leaves an empty row between components so they're told apart ([Trigger-action-circuits](https://image-ppubs.uspto.gov/dirsearch-public/print/downloadPdf/10558776)).
  tscircuit lays like parts out on a grid or along a line (LED grids, keyboards)
  ([automatic PCB layout](https://docs.tscircuit.com/guides/tscircuit-essentials/automatic-pcb-layout)).
- What bbgen's final score lacks, by the drawings above: room around parts (the placement estimate has it,
  `CROWD_COST`, `ROOM_COST`, `SPACE_COST`; the built score doesn't), labels over parts (the score counts only
  two-lead parts' labels near each other), like parts in line (an LED bank in one row), and a leg's stretch
  priced against a wire it saves (3 a hole beyond `span_max`, while a wire costs 2 plus its length).

What to try, in order: (1) a tidiness test: pairs of placed layouts of one circuit, one judged tidier,
checked by `cargo test` (score(tidier) < score(messier)); the pairs above are the first. (2) Score terms for what
the pairs show, kept only when every pair ranks right, run.py's numbers on today's layouts hold, and all 21
drawings read no worse. (3) Then the search: repair the best two or three starts and pick by the full score.

**Kept (October 2026): rules for the supply leads, and crossings on the board only.**
- `crossings` (the `cross` column, and the score) counts only wires on the board: where an off-board part
  sits is the drawing's choice, and a lead runs above the board. `lead×` counts crossings with a lead in
  them, for information.
- Every flying lead of a rail's net gets its hole before anything else is placed (`autoplace::lead_plan`):
  on a rail of its net, every rail lead of one connector on the same side of the board, in the outermost
  free holes, shared out by fewest crossings; a ground may take a free `-` rail; a lead leaves a part's
  straight drop to the rail its hole. Wiring lays them first and `settle_leads` leaves them be. 19's leads
  no longer cross, 13 loses its warning; 16 is worse (37 → 75 board crossings), a search miss: its old
  layout scores better under the new rules.
- Tried and left out: a fence keeping the circuit out of the leads' columns, in placement's model or as a
  move of the finished layout toward it. Every variant reshuffled the search (05 and 13 messier, 13 up to 4
  warnings), as any change to placement's limits or prices does while it repairs one start only.

## 5. Circuit 16 is big: 41 jumpers, 204 crossings, 75 s

**Issue.** The iPod M2 driver (three 24-pin level shifters on 0.6 in adapters, a Pico, an 18-bit bus) on two
boards side by side. It builds without errors or warnings, but it's a tangle, and it takes a third of
the benchmark run.

**Starting points.** Most of its jumpers are the bus (point 1) and wires between the two boards, which are
always jumpers (point 9). Try it with the second board stacked below (`below: 1`) instead of beside: the
boards' columns line up and the leads to the connector get shorter. It's the stress test for points 1, 6
and 7.

## 6. Speed: placement rebuilds the whole circuit for every move

**Status: mostly done by the Rust rewrite.** A full run went from 221 s to 8 s on 8 cores. Kept circuits
come back from the threads that built them, and trials are built in order on threads, which makes the
results exactly the same. What follows is the Python's story; incremental repair is still open and would
help 16 most (about 3 s, mostly wiring).

**Issue.** Every trial layout is a fresh `Circuit` built from the file: parse, place, wire, check, score.
The repair search builds up to 120 of them, plus starts, turns and polish. A full benchmark run is
about 3.5 minutes; 16 alone 75 s.

**Tried and kept.**
- Trial layouts built side by side in worker processes (`_scored` in autoplace): exact, same layouts,
  2–3× faster (464 s → 160 s at the time).
- Caching: a board's strips' holes, the empty strips (until a wire goes in or out, `c._wired`), a body's
  place along a scan (`_step_blocked`), the annealer's positions (`x_of`), each leg's share of a pair's
  cost (`_Legs.leg`).

**Why it's not more than 3×.** The search often finds an improvement early in a batch of eight, a kept
layout is built again in the main process to get its circuit back, and annealing runs one seed at a
time.

**Starting points.**
- Incremental repair: a move that changes one part only needs that part's nets re-wired. Keep the
  wired circuit, take out the moved part's nets' wires (`_unwire`), move the part, and wire those nets
  again; legs placed afresh make it harder (they move strips other nets use). Check it with the
  fingerprint script against a full rebuild.
- Send the winning circuit back from the worker (pickled) instead of building it again.
- Anneal the four seeds in parallel; the layouts reference `Pin` objects, so return choices by index.
- Profile first (`perf record -g target/release/bbgen dev build FILE`; a build with frame pointers,
  `RUSTFLAGS="-C force-frame-pointers=yes"`, gives clean call graphs): in Rust the wiring (`join`,
  `relay`, `settle_leads`) and leg placement (`Legs::choose`) were the hot spots.

## 7. An exact solver for small circuits or stuck regions

**Issue.** Everything is greedy or local search. Small circuits and small regions (one chip's
neighbourhood) could be solved exactly.

**Starting points.** An answer-set programming approach has laid out stripboards end to end
(arXiv 2512.04910); an ILP or SAT model over strips (each wire a choice of two holes in a row or a
column, no two ends in one hole, no wire over another's end) would fit breadboards. Use it on a region
that still has jumpers after the normal pass, with a time limit, and keep the result only if it's
better.

## 8. Rip-up and reroute

Tried and left out (see point 1, and the notes at the top of src/autowire.rs): on these layouts the jumpers are there
for want of room, not wiring order. Worth trying again if a layout turns up where a jumper's net, wired
first, would lie straight without pushing another net off the board.

## 9. Physical assumptions to check against real hardware

- **Stacked boards** (`below:`): the lower board is assumed to sit on the same 0.1 in grid, its row a
  1.0 in (`STACK` = 20 holes) under the upper board's row i, so a 1.0 in module sits in both. Measure two
  of the boards you build on, snapped together; if the spacing differs, `STACK` in src/board.rs is the place.
- **Raised modules** (`raised: true`): assumed high enough on their headers for flat jumper wire to pass
  under (true of standard 2.54 mm headers with their plastic spacer). Header plastic is modelled as an
  obstacle along each row of pins.
- **Wires between boards are always flexible jumpers** (the user's rule): a rigid wire across the join
  of two boards is never laid.

## 10. Smaller things

- **A label can sit on a flying lead.** In 20, R2's label lies across the ONEWIRE lead W11. The label
  check counts a label over a wire end or another label, not one a wire passes through, so the `labels`
  column says 0. Count labels over any wire, then move the label like the others.
- **A raised Pico** builds (`type: pico, raised: true`), but only a DIP-style module is covered by tests
  (the raised cases in `benchmarks/golden/cases/`). Add one, including that a wire under it can't cross
  its pin rows.
- **Large KiCad modules hang off the board** in the drawing (the OLED in 17 and 21): their footprint
  body is the whole module. Harmless to the check; a module could be drawn as its header only, with the
  rest outlined.
- **Labels over wires and pin names** (second Knight Rider run, point 17): in
  [repro/knight-rider-run2-placed.yaml](repro/knight-rider-run2-placed.yaml), "U3 CD4070" lies over W9, "C1 1uF"
  over U1's pin names, "R2 47k" over W20. The label check counts labels over wire ends and other labels only
  (see "A label can sit on a flying lead" above).
- **A resistor in adjacent holes stands up, and the checklist doesn't say so.** In the same run R2 47k sits in
  `c10`/`c11` (0.1 in apart). Say "stand it up" in its checklist line, or keep two-lead parts from being placed
  that close unless they're meant to stand.
- **Long slants across the trench aren't warned about.** With `flex: false`, rigid wires slant 8-12 columns
  across the trench with 0 warnings: a wire crossing the trench between different columns is exempt from
  DIAGONAL, however far it slants. Warn past a few columns, or route it straight with a bend.
- **A small edit reshuffles the board.** Moving one resistor (R14) a row moved three unrelated chips (f4/f12/e29
  to f9/f16/f25). That's the search (point 12), but it makes tuning by hand slow; a way to freeze bbgen's
  choices into the file (write `at:` back) would help.
- **Two-board benchmarks are slow to place** (17: 31 s, against 7 s for 21, the same circuit on one
  board). Not looked into; likely suspects are the bigger search space (the model gives each board its
  own run of columns) and the jumpers between boards, each of which runs the relay search first.

## 11. Left over from the Rust rewrite

Each of these is known and not yet done. The ones that move wires need `benchmarks/record.py` afterwards,
and `run.py` to judge them.

**Behaviour that depends on the machine:**
- **The golden records hold for this platform's maths library.** Annealing's `exp`, the drawing's
  `atan2`, `cos`, `sin`, `log10` and `powf` come from the C library (glibc here), as they did in the
  Python. Another libm may differ in the last bit and pick another layout or move a label. If
  `cargo test` fails on macOS or Windows, look there first.
- **KiCad's Windows install isn't searched.** src/kicad.rs, `library_roots`, lost the Python's
  `C:/Program Files/KiCad/*/share/kicad` (the glob only handles paths from `/`). On Windows, set
  `BBGEN_KICAD` until it's added.
- **The KiCad library's version is part of the records** (`golden/index.json`). After updating KiCad,
  record again and look at what changed.

**Speed** (beyond point 6). All of these leave results the same, which `cargo test` checks:
- Owner names (`"W12"`, `"the body of U1"`) are strings that get compared and formatted in hot paths.
- `unwire` recomputes every wire's covered holes, and `empty_strips` rescans the board after each wire.
- Many small `Vec`s are allocated per call (`between`, `strip_holes`, `bodies_crossed`, `options`). Profiles
  put allocation at 10–15 %.
- The parallel search builds a batch of trials (one per core) and throws away what comes after the one
  it keeps. On 8 cores it gets only 2–2.5× over one.

**What could go once records are re-made anyway.** src/pyf.rs, src/pyrand.rs and src/yaml.rs copy
Python's rounding, `math.dist`, `sum()`, random numbers, set order and PyYAML exactly. They exist only
so that the same file still gets the same layout as before. Plain Rust arithmetic and a seeded PRNG
would be simpler and a little faster, at the cost of one big re-record. YAML 1.1 reading (`yes`/`on`,
octal) is a user-facing behaviour, though: keep it unless the file format changes on purpose.

**Smaller things:**
- **PNGs need ImageMagick or `rsvg-convert`.** The `resvg` crate could draw them in-process.
- **Error texts for broken YAML are libyaml's, not PyYAML's.** It's the same problem in different words.
- **The unit tests are end to end.** They are the golden records and the expected outputs, plus a few
  unit tests for the float, random and combination helpers. A change that's meant to move wires gives a
  test nothing smaller to lean on than the whole benchmark; unit tests per module (board, parts, check)
  would help.
- **`flying` is written as `0.0` where the Python wrote `0`** in `benchmarks/baseline.json`. They compare
  equal.

## 12. The score doesn't see tidiness, so a stronger search makes messier boards

**Issue.** bbgen repairs only the starting layout that scores best, and a start's score is a poor guide to
where repair ends: in 12 of 22 circuits another start would end lower (point 4, "Checked again"). But the
search can't simply be made stronger. Twice, repairing every start was tried and reverted (point 4): the
lower scores came from layouts that read messier, since the score rewards short and few wires and hardly
sees crowding, labels over parts, stretched legs or parts out of line. Today's weak choice of start happens
to keep the placement estimate's tidy layouts (the estimate likes room around parts).

**What the drawings show the score lacks** (the October 2026 check, each a start bbgen didn't pick):
- 18: the three LEDs leave their row for three places, R3 stands on end, a label clashes (66.0 → 51.9).
- 02: legs stretched to save two wires (57.0 → 50.0).
- 09: the LEDs spread over both halves of the board (229.4 → 212.3).
- 19: everything packed into one corner, labels over the regulator (45.1 → 29.3). Compact, but crowded.
- 07, 08, 13, 17 also end lower from another start; not yet judged.

So the missing terms are room around parts (the estimate has `CROWD_COST`, `ROOM_COST` and `SPACE_COST`, the
built score in `autoplace::score` doesn't), labels over parts (it counts only two-lead parts' labels near each
other), like parts in line (an LED bank in one row), and a leg's stretch priced against the wire it saves (3 a
hole beyond `span_max`, while a wire costs 2 plus its length).

**Research.** Point 4, "Research (October 2026): measuring tidiness": layout scores fitted to judged examples
(O'Donovan et al. 2014), judgments as margin constraints (ranking SVM) or a Bradley–Terry fit when there are
many, and breadboard and stripboard tools that keep tidiness out of the score by rule (clearance as a hard
constraint, an empty row between parts, like parts on a grid).

**Plan, in order.**
1. **A tidiness test.** Pairs of placed layouts of one circuit, one judged tidier by a person, kept in
   `benchmarks/` and checked by `cargo test`: score(tidier) < score(messier). The eight pairs above are the first
   batch; they need judging (18, 02 and 09 are judged above, and the score ranks all three the wrong way
   round). This keeps the judgment in the repo instead of re-learning it after each revert.
2. **Score terms for what the pairs show**, one at a time. Keep a term only when every pair ranks right,
   `run.py`'s numbers on today's layouts hold, and none of the 21 drawings reads worse.
3. **Then the search:** repair the best two or three starts and pick by the full score, at about two to three
   times today's placement time (repairing all of them cost four to six times: 8 s → 33–53 s for all 21).
   Successive halving was simulated in point 4 and got a third of the gain; leave it out.

**To make the pairs again.** In `search::autoplace`, where the starting layouts are scanned, temporarily
replace `let kept = best.as_ref().map(|x| b.score < x.0).unwrap_or(true);` with a choice of `i == k` for
`k` from a `BBGEN_START` variable, then build each benchmark with `BBGEN_START=0`, `1`, ... and `-o DIR`. The
pairs above are (picked start, other start, both counted from 1): 18 (8, 5), 02 (1, 3), 09 (1, 4), 19 (4, 1),
13 (2, 8), 17 (1, 7), 07 (1, 3), 08 (4, 3). Keep them as placed files, so the test doesn't depend on the search:
`bbgen dev build FILE --dump OUT.jsonl` writes the layout as its `placed` event, which `benchmarks/record.py`
turns into `golden/placed/<name>.yaml` the same way.

## 13. Chips are drawn as blue boards, not as ICs

**Issue.** A chip in the breadboard drawing doesn't look like a chip. `footprint()` in
[src/render.rs](../../src/render.rs) draws a DIP (`type: dip`) or a small KiCad part as one flat blue rectangle
(`#24467a`) over its whole footprint, with gold pads and the pin names inside, so a 74HC595 looks like a small
module on a blue board, the way the Pico is drawn (green, `#2f6e3d`). Someone building from the drawing should
see the black chip they hold in their hand.

**What a chip should look like.** A black body, narrower than its footprint, standing over the trench between
its pin rows; a notch at pin 1's end (a dot by pin 1 as well); short silver legs from the body's sides into
the holes; and its name (`74HC595`) printed along the body, with the ref (`U1`) as the label. Pin names stay
outside the body by their holes, or move to the label side, as they read best. Modules (the Pico, the big
KiCad modules) stay boards.

**Where to start.** Only the drawing changes: `body_rect` in [src/parts.rs](../../src/parts.rs) also says which
holes a part covers and where labels can't go (`src/circuit.rs`) and what the studio lights up for a part
(`src/scene.rs`), so leave it as it is and draw the chip inside it. Which parts are chips: `Kind::Dip`, and
`Kind::Kicad` parts with a DIP footprint (two rows, not `big`); SMD parts on a DIP adapter are an adapter
board with the chip on it. Look at the drawings before and after (the examples, and 12, 13, 14 among the
benchmarks). The drawings in `benchmarks/golden/expected/` will change on purpose; record them again with
`benchmarks/record.py` ([benchmarks/README.md](../../benchmarks/README.md)).

## 14. The README says too little about the studio

**Issue.** The [README](../../README.md) gives bbgen studio a line in the list of outputs, one in Install and a
short paragraph under Use, and shows no picture of it: someone reading it can't tell what the studio looks like
or what it's for before installing it. The README as a whole could say more too: what a circuit file looks like
before the Use section, and what the build checklist and the schematic look like.

**What it should show.** Screenshots of the studio: a board with a part or net lit up on hover, the build view
part way through a build, and placement and wiring as bbgen works. A sentence or two with each on what it shows
and when it helps, and a link to [studio/README.md](../../studio/README.md) for the rest.

**Where to start.** Take the screenshots from a built example (`examples/eeprom.yaml` is the one the README
already shows), keep them small enough for the repository (PNG, a few hundred KB each), and put them next to
`examples/eeprom.png` or in a `docs/images/` folder. Check the README on GitHub afterwards: it renders HTML
in Markdown its own way.

**The studio is one-way for an agent** (from the Knight Rider feedback, point 15). An agent can open the studio
and the user sees every rebuild. But the agent can't see what the user sees, whether they changed or moved
anything there, or take a screenshot. A file the studio writes, or `bbgen studio --status`, would let
"interactive" go both ways: the user moves a part and the agent fixes things around it. A bigger job than
the README; settle what the studio is for first.

## 15. Placement shorts a chip into a hand-placed part's strip

**Done (October 2026).** `Model::new` reads what the file places at holes itself (`hand_placed`): each leg of a
two-lead part given `pins:`, and each wire end, whose strip carries the net at the wire's other end (a pin, a rail
only one net has, or a strip known already, followed along chains of wires; a strip whose net can't be told, or
holds two, takes no chip pin at all). A chip's place is dropped from its choices (`clashes`) when a pin lands in
such a strip of another net (or of none), or a pin or its body on a taken hole. Dropping places up front does what
an infinite cost would, more simply, since hand placements never move: annealing, turning and repair all choose
from the same lists. When a chip has no place left, the build fails naming what's in the way. After repair,
`own_error` fails the build too if a SHORT, STRAY or HOLE_TAKEN still names a part bbgen placed and one the file
places; that should now be unreachable, and the user chose a failure over a drawing with the error. No benchmark
places legs or wires by hand around chips bbgen places, so every record holds. Tests: `tests/hand_placed.rs`, with
the reproduction (now 0 errors, 11 flexible jumpers against 10 with the chips placed by hand) and an earlier
variant of it, and small circuits for the error and for wire chains.

Prior art: chip placers treat parts placed beforehand as fixed obstacles and take the sites they block out of the
set a cell can go to; a region that's too full is reported, not overlapped
([US 7036103](https://image-ppubs.uspto.gov/dirsearch-public/print/downloadPdf/7036103),
[US 7934188](https://patents.google.com/patent/US7934188B2)). A short is a hard constraint in the stripboard ASP
work too ([arXiv 2512.04910](https://arxiv.org/html/2512.04910v1)).

Left open then: hand-placed legs didn't pull their nets' pins in the estimate; point 17 does that now. A
hand-placed part lying across a chip bbgen places stays a warning (`OVER_PART`), priced by the score.

The original report follows.

**Issue.** From the first project built with the breadboard skill (a Knight Rider LED runner: an NE555, two
CD4017s, 8 LEDs placed by hand, 42 parts, 39 nets; the feedback is in
`~/Projects/knight-rider/bbgen-feedback/`). With the chips left to bbgen, placement put U3's pin 10 (Q4) in
`e47`, the strip where L1's anode was placed by hand (`a47`). The result was `SHORT nets B_Q4, LED1`, at
`BBGEN_JOBS=1` and the default alike. The reproduction is `repro-placer-short.yaml` in that folder; it still
shorts on `main` (October 2026). With the three chips placed by hand, the same file builds without errors.
"Place the LEDs, let bbgen do the rest" turned into "do the rest except the chips".

**Diagnosis.** Placement's estimate (`Model::cost` in [src/search.rs](../../src/search.rs)) knows only the
footprint parts given with `at:` (`fixed`, built in `Model::new`). Hand-placed legs of two-lead parts are read
only for the rail plan (`legs`), so the estimate doesn't know that a strip already belongs to a net. The built
score (`autoplace::score`) charges 1000 per error but doesn't forbid one. When every start shorts and repair
can't get out, bbgen returns a layout that fails its own check. A small case (one 4017, four LEDs placed by
hand) placed cleanly: on a roomy board the cheapest places just happen to miss the LEDs.

**Starting points.**
- The strip of every leg placed by hand, and of every wire end given by hand, belongs to that leg's net. In
  `Model::cost`, a pin of another net in such a strip makes the layout impossible (infinite cost), as an
  overlap of bodies does today.
- If the only layouts placement finds have errors it made itself (none with every part as given), fail
  with the part's name, "couldn't place U3 without shorting into L1's strip", rather than return one.
- Add the reproduction as a test: no SHORT, and the LEDs where the file put them.

## 16. More from the Knight Rider feedback

The other points from that feedback that are worth doing (point 15 has the source). Left out: a
`bbgen explore` mode for trying placements (a script does that once `--json` has a score) and repeating
patterns of parts (a CAD tool's job).

- **A layout score in `--json`.** `stats` has errors, warnings, parts, wires, nets and time. To compare
  variants, the agent parsed the Markdown checklist for `Flexible jumpers (N)`; the count is in
  `wires[].flex` but easy to miss. Add the flexible jumpers, jumpers over chips, total wire length and
  `autoplace::score` to `stats`, and the jumper count to the `check:` line.
- **Pin names in `bbgen find`** (or `bbgen pins Timer:NE555P`). Today the agent has to grep the
  `.kicad_sym` files, which is awkward when a symbol extends another (`BC547` extends `Q_NPN_CBE`).
  src/kicad.rs already resolves `extends`.
- **`find` says when a footprint won't do.** `4xxx:4017` has no footprint and `BC547`'s TO-92_Inline is
  off the grid; the build error names the fix (src/parts.rs), but only after a build. `find` could say
  "no footprint: try Package_DIP:DIP-16_W7.62mm" or "off the grid: try TO-92_Inline_Wide". Keep the
  footprint the file's choice rather than pick one silently.
- **Document `near:` for every two-lead part.** circuit-file.md shows it only on a capacitor; it works on
  resistors and diodes too.
- **`near:` for one end:** `near: {pin: 2, to: L2}`. `near: L2` on a diode moved the whole diode to the
  LEDs and stretched its other end across the board. Try this before groups.
- **Groups of parts** (`groups: {LED2: [D1, D2, R2, L2]}` or `cluster: L2`): keep a repeated channel (an
  LED with its resistor and diodes, a button input, an ADC channel) together around a part placed by hand.
  The best of about 30 variants still had 10 flexible jumpers, with the diodes around counter A, far from
  their LEDs. **Done (October 2026), as repeated groups found in the netlist and placed alike, with no new
  syntax:** see "Repeated groups" below.
- **Labels over each other:** "L8" and "L7" printed as "L8L7" with the LEDs two columns apart.
  `tests/hand-placed/knight-rider.yaml` shows it (L7 at `a59`, L8 at `a61`); the label check
  (`measure::label_clashes`) should have counted it.
- **An LED across a rail** (row `j` to `bot-`, across `bot+`, with the rails `+-` on both edges): find out
  whether bbgen allows this, warns, or says nothing, and say so in circuit-file.md.
- **Say why a jumper is flexible**, as DIAGONAL warnings already do: "W44 is flexible because every
  straight route from g34 to d26 is blocked by U3 and W39". That shows which part to move. Later.

**Repeated groups (done, October 2026; kept by the user's choice, with the decoder circuit's extra wires).**
Instead of `groups:` in the file, bbgen finds the groups and places the ones that repeat the same way
(src/repeats.rs). This also covers "`near:` for one end" above: a diode's end sits by its LED as part of its group.
- *A group* is the two-lead parts bbgen places that hang together, through nets no chip, rail or lead reaches, on
  one two-lead part the file places (an LED with its resistor and diodes). Groups are copies when their parts are of
  the same kinds and joined the same way, with each net leaving the group going to the same kind of thing (the same
  rail, or a chip), and their hand-placed parts the same shape. Knight Rider's first circuit has 6 copies of
  LED ← resistor ← two diodes (L6's cathode goes to a chip, so its LED lies differently and it's alone) and 2 of
  LED ← resistor; run 2's has 8 of LED ← resistor.
- *One shape for the copies:* the legs of the first n parts out from the hand-placed part, as holes relative to it,
  each part lying straight at its natural span. A search (a beam of leg pairs per part, branch and bound) finds
  the shape that fits the most copies, then covers the most parts, then costs least (the leg costs of legs.rs),
  trying each group as the pattern. A copy it doesn't fit, and the parts beyond n, get the usual leg placement.
  Copies stay together through repair: they're not re-ranked one at a time (`Circuit::repeated`, carried through
  the trials as `_repeated`).
- *Room:* the estimate charges `FRONT_COST` for each strip a chip takes across the trench from a hand-placed leg
  that bbgen hangs parts on, since that's where an LED's resistor stands.
- *Found on the way:* with the LEDs two columns apart, every copy shares its rows, so no part longer than two holes
  can lie along a row without running into the next copy: the only straight places are down the LED's own column,
  across the trench. An LED with a resistor and two diodes can't repeat without slanted diodes, so only the
  resistor repeats there.
- *Results* (flexible jumpers / wires; main → repeats only → room only → both): `knight-rider.yaml` 16/35 → 13/36
  → 16/36 → 13/36; `knight-rider-left.yaml` 16/35 → 15/36 → 13/37 → 12/41; `decoder-leds.yaml` 6/41 → 7/50 →
  6/41 → 7/50. No benchmark places parts by hand, so every record holds. On the drawings: the resistors stand in a
  row under the LEDs, but run 2's decoder circuit now needs a wire from each resistor's lower strip to the decoder
  (on main the resistors lay from the decoder's strips to the LEDs', doing the wires' job); the two-diode circuits
  keep their diodes scattered in the lower half with long jumpers.
- *Next:* the diodes. They can't repeat beside LEDs two columns apart; a second pattern placed by the chip pins
  that drive them (each diode from its counter's strip, the wire to the LED's strip) might, which means a shape
  anchored on a chip rather than on a hand-placed part. Groups with no hand-placed part (a button input, an ADC
  channel) need the same: bbgen would choose where each copy goes, probably in a row in front of the pins that
  drive them, and that would move the benchmarks. And judge the decoder circuit's extra wires again once point 12
  gives the score a sense of tidiness.

**Prior art.** Datapath placers find repeated subcircuits (templates, bit slices) before placing, since a general
placer scatters them, and lay them out as identical slices ([Hassoun & McCreary, ICCAD 1999](https://www.cs.tufts.edu/~soha/paperArchive/1999%20Regularity%20extraction%20via%20clan-based%20structural%20circuit%20decomposition.pdf);
structural versus functional extraction, [UT Austin](https://www.cerc.utexas.edu/utda/publications/C135.pdf); a
survey in [US 8589855](https://patents.justia.com/patent/8589855)). PCB tools copy one channel's layout to the
others by hand: Altium's Copy Room Formats with channel classes ([Altium](https://resources.altium.com/p/how-to-simplify-your-circuit-replication-with-multi-channel-design)),
KiCad 9's Repeat Multi-Channel Layout ([forum](https://forum.kicad.info/t/v9-0-0-multi-channel-layout-how/59749)),
which refuses copies whose outside connections differ ("topology mismatch",
[forum](https://forum.kicad.info/t/multi-channel-im-getting-a-topology-mismatch-status-when-attempting-to-repeat-layout/58795)),
as bbgen leaves L6 alone. bbgen's groups are small, so a signature and a backtracking match do instead of the
general subgraph search those papers need.

## 17. Chips aren't drawn toward what the file places

**Done (October 2026), with a regression kept on purpose.** The estimate now knows the legs the file places: each is a
fixed pin of its net in `where_` (`Model::anchors`), so nets and the links through a resistor reach it. And a net
with no pin of its own placed, on a chain of two-lead parts bbgen places that reaches such a leg (`Model::chained`),
sits at the median of the nets it's linked to, as a placer puts a movable cell between its neighbours; the links
through it pull too. No benchmark places legs by hand around chips bbgen places, so every record holds.
- Run-2's circuit with only the LEDs placed (`tests/hand-placed/decoder-leds.yaml`): flexible jumpers at its seven
  LED positions 10, 12, 8, 11, 11, 7, 10 → 6, 6, 8, 7, 7, 6, 7, and drawn as tidy as the run's hand-tuned final.
- **The regression:** the first circuit, where each LED is two parts deep (counter → diode → resistor → LED, 12
  diodes and 8 resistors), reads worse. `tests/hand-placed/knight-rider.yaml` goes 11 → 15 flexible jumpers: the
  counters move toward the LEDs, the 20 small parts have no room between them, and jumpers cross both counters.
  `knight-rider-left.yaml` goes 14 → 13, messy either way. Following the chains didn't fix it, nor did a weaker
  pull along them (½, ¼) or the legs in the links only: 13-15 every time. The estimate pulls chips toward the
  LEDs but keeps no room for the parts that go between. The user chose to keep the change, since a chip driving
  each LED through one resistor is the common case. The way out is likely point 16's groups (each LED with its
  resistor and diodes kept together, so they claim their room), or room for a chain's parts in the estimate
  (`ROOMLESS_COST` looks for one free strip per link, not for a whole chain). Judge any fix on both Knight Rider
  files' drawings.

The analysis that led there follows.

**Issue.** From a second run of the Knight Rider task (October 2026, a subagent with the fix from point 15; its
report and every variant are in `~/Projects/knight-rider/bbgen-feedback/run-2/`). It designed a 555 → CD4040 →
CD4070 → CD4028 runner, with each decoder output driving an LED through a 1k resistor, and placed only the 8 LEDs.
Every build was clean, but the decoder often landed at the far end of the board from the LEDs (LEDs at columns
3-17 put U4 at column 58). Over 7 LED positions, flexible jumpers ranged from 7 to 12 with no pattern. Placing U4
by hand next to the LEDs got 6, with the rest of the chips in signal order. The input file is
[repro/knight-rider-run2.yaml](repro/knight-rider-run2.yaml), with U4 and R14 placed by hand; take their `at:` and
`pins:` out to see it.

**Diagnosis.** The estimate (`Model::cost`) only knows where footprint pins are: `where_` is built from the
layouts, so a two-lead part's legs placed by hand count for nothing. A net like `L1: [R11.2, D1.A]`, with D1's anode
placed by hand, has no known point. And `LEG_LINK_COST`, which pulls the two nets of a resistor together (here `O0`
from the decoder and `L1` to the LED), needs a known pin on both sides, so it never pulls the decoder toward the
LEDs.

**Starting points.**
- Put the holes of legs placed by hand into `where_` as fixed points of their nets (the hand-placed strips from
  point 15, `hand_strips`, already have them), so nets and resistor links reach them.
- A chain through a two-lead part (chip → resistor → hand-placed LED) is the usual case; check `LEG_LINK_COST`'s
  weight once the far side is known.
- No benchmark places legs by hand around chips bbgen places, so the records hold either way; judge it on
  run-2's circuit and the two in `tests/hand-placed/` by their drawings.

**Research (October 2026).** In analytical placement, pre-placed I/O pads and terminals are vertices of the net
model with fixed coordinates: each connection to one adds its position to the system, and that is what pulls the
movable cells toward them ([Placement (EDA)](https://en.wikipedia.org/wiki/Placement_(electronic_design_automation));
FastPlace, [Viswanathan & Chu](https://home.engineering.iastate.edu/cnchu/pubs/c20.pdf)). A part with no fixed
place between two others (a resistor between a chip and an LED) is a movable cell whose best place is between its
neighbours, so a chain of them still pulls the chip toward the pad. PCB placers cluster by connectivity with fixed
parts as locked nodes ([MIT thesis 2025](https://dspace.mit.edu/handle/1721.1/164641); Altium's
[cluster placer](https://www.altium.com/documentation/altium-designer/autoplacer-cmd-runautoplacerrunautoplacer-ad)).
bbgen's estimate leaving hand-placed legs out is a placer forgetting its pads.

**Tried (October 2026, reverted): legs placed by hand as fixed pins in `where_`.** Flexible jumpers, before → after:
run-2's circuit with only the LEDs placed, at its seven LED positions
([repro/knight-rider-run2-leds-only.yaml](repro/knight-rider-run2-leds-only.yaml) is the first): 10 → 6, 12 → 6,
8 → 8, 11 → 7, 11 → 7, 7 → 6, 10 → 7. Drawn, the first is as tidy as the run's hand-tuned final: the chips in signal
order, the decoder by the LEDs, no jumper across the board. But the first circuit (`tests/hand-placed/knight-rider.yaml`)
went 11 → 14 and read messier: its LEDs are two parts deep (counter → diode → resistor → LED), so only the nets one
part from an LED pull (A_Q0, B_Q2, and A_CARRY straight to L6). Both counters moved under the LEDs and long jumpers ran
back to the diodes left in the middle. A partial pull distorts: the estimate needs the chains too.

## 18. A rigid wire crosses other rigid wires instead of going round or becoming a jumper

**Done (October 2026), with a search miss in 13.** The cause was wiring order: nets without a rail are wired first,
so W23 (CLK) took row a before the supply drops W6 and W16, which then had to cross it on their way up to the rail.
With flexible jumpers on, wiring's `cost` doesn't charge for crossing wires at all (`CROSS_WIRE_COST` counts only
with `flex: false`), so nothing steered it to a row further in. Of the benchmarks' flat crossings (not counting
jumpers), all 8 were this pattern: a rail's wire crossing a signal's wire laid before it.

`settle_crossings` in src/autowire.rs runs once the board's wires are in. Each flat wire crossing another is laid
again: the cheapest pair of holes in the same two strips, each flat wire crossed priced at `CROSS_WIRE_COST`, kept
if it costs less and crosses fewer (W23 goes to `d13` → `d22`). Where no holes clear it, it becomes a flexible
jumper between the same holes; a wire to a rail crossing a signal's wire leaves the jumper to the signal, so a drop
to the rail stays short and flat. On the benchmarks' placed layouts, wiring alone leaves no flat crossing.

With placement: 05, 07, 09 and 16 read better (16: 75 → 44 crossings, 384 → 240 with a lead; 07 loses its jumper
over the op-amp), 11 and 15 about the same, flexible jumpers 93 → 92 over all 21. **13 is a search miss**: it takes
2 OVER_PART warnings (wire ends under R2 and R8), but its old layout, wired the new way, still has none and scores
795.1 against 837.2 for the one found. As in point 4, a change to what trials score reshuffles which start wins
and where repair goes (point 12). Kept by the user's choice; the baseline holds 13's warnings until the search
finds the better layout again.

The report follows.

**Issue.** In the second Knight Rider run's final layout,
[repro/knight-rider-run2-placed.yaml](repro/knight-rider-run2-placed.yaml) (layout hash `950fcc074e` from
`bbgen dev build`), W23 (CLK, orange) runs flat along row a from `a13` to `a22`. On the way it crosses W6 (VCC,
`top+16` → `b16`) and W16 (GND, `b21` → `top-21`), lying over both, since wires go in by number. Two rows down
(`c13` → `c22`) it would cross nothing. `bbgen dev measure` counts 4 crossings in this layout; the check says
nothing.

**What it should do** (the user's rule): a wire lying flat on the board doesn't cross another flat wire. If no
straight route avoids them, it becomes a flexible jumper, which arches over them.

**Diagnosis so far.** Wiring charges `CROSS_WIRE_COST` (1) per wire crossed, against `FLEX_COST` (10) for a
jumper, so crossing two wires is the cheap choice by design. circuit-file.md says a wire "runs along no other
wire" and nothing about crossing. Why `c13` → `c22` lost to `a13` → `a22` isn't known yet: print the candidates'
costs for W23 (as in point 3's diagnosis).

**To reproduce.** `bbgen build docs/design/repro/knight-rider-run2-placed.yaml -o OUT` and look at W23; the placed
file fixes the parts, so it shows the wiring whatever placement does later. `bbgen check … --json` gives each
wire's `points`.

**Starting points.** Make a flat wire crossing another flat wire an obstacle, like a part's body: a straight route
across one is out, and a bent route or a jumper takes its place. It moves wires in most benchmarks (104 crossings
over all 21 in `baseline.json`), so judge it on run.py and the drawings, and check that jumpers don't multiply.
Update circuit-file.md's promise either way.

## 19. A wire under a two-lead part: the docs and the checklist line

**Done (October 2026).** `Circuit::under_of` counts the two-lead parts lying across a flat wire as well as raised
modules, so the checklist line names them (`runs under R3: lay it flat, before it goes in`), as do `under` in
`--json` and the studio's "Runs under"; the checklist's sorting into "Wires under parts" uses the same list.
circuit-file.md now says a wire crosses no chip or module and may run under a two-lead part. Five benchmark
checklists gained the note (07, 13, 14, 16, 17); nothing moved.

The report follows.

**Issue.** circuit-file.md says a wire bbgen lays on the board "crosses no part". But a flat wire may run under a
two-lead part lying across it, on purpose: the checklist lists it under "Wires under parts", to lay first. The
second Knight Rider run reported this as a bug (variant `v8-u4-e42.yaml` in run-2: W4 `bot+42` → `i42` under R14
at `j41`/`j44`, W17 `bot-58` → `top-58` under R18 at `e56`/`e59`, with no warning).

**What's missing.** The docs should say a flat wire may run under a resistor or diode lying across it. And the
wire's checklist line should name the part, as it does for a raised module (`runs under U1: lay it flat, before
it goes in`): `checklist::line` uses `Circuit::under_of`, which knows only raised modules and holes under bodies,
while the section's sorting (`below` in checklist.rs) also counts two-lead parts crossed (`measure::cross`).

