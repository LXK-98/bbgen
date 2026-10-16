# bbgen studio

A desktop app for bbgen: open a circuit, build it, and inspect the breadboard and the circuit's schematic. Hovering a part, wire, net,
issue or hole lights it up on the board and in every list; clicking it shows what matters about it in the
inspector. The circuit file is watched, so saving it (from an editor, or Claude Code) rebuilds the board.

The studio never links bbgen's library: every build runs the `bbgen` binary as `bbgen build FILE --json` and
reads the scene it prints ([docs/scene-format.md](../docs/scene-format.md)). The plan, and what comes after
inspecting (generation progress, the build guide, editing, an assistant), is in
[docs/studio-plan.md](../docs/studio-plan.md).

SvelteKit (a single page, `adapter-static`) with shadcn-svelte, in a Tauri 2 window.

## Run it

Needs Node with pnpm, a Rust toolchain, and Tauri's system libraries (on Fedora: `webkit2gtk4.1-devel`,
`libsoup3-devel`, `javascriptcoregtk4.1-devel`).

```bash
pnpm install
```

```bash
pnpm tauri dev
```

or, without the desktop window, in a browser at http://localhost:5173:

```bash
pnpm dev
```

In the browser, the dev server stands in for Tauri ([vite/bbgen-dev.ts](vite/bbgen-dev.ts)): it runs bbgen,
lists the repo's examples and benchmarks, and watches the open file. There's no native file dialog there, so
open a file by typing its full path in the Open dialog. In the desktop window, dropping a `.yaml` file on it
opens it too.

"Show in YAML" in the inspector and on error banners opens the file at that line in `$VISUAL` or `$EDITOR`
(a terminal editor such as nano in a terminal window), or with the system's opener when neither is set.

Which bbgen runs: the one picked in the "bbgen output" tab (Change…), else `$BBGEN` if set, else a `bbgen`
beside the app, else this checkout's `bin/bbgen` (which builds bbgen first when its sources changed), else
`bbgen` on `PATH`.

## Where things are

| | |
|---|---|
| [src/lib/engine.ts](src/lib/engine.ts) | the only way the UI reaches bbgen: Tauri commands, or the dev server's endpoints |
| [src/lib/scene.ts](src/lib/scene.ts) | the scene's types |
| [src/lib/studio.svelte.ts](src/lib/studio.svelte.ts) | the state: the open file, its last good build, hover and selection |
| [src/lib/lookup.ts](src/lib/lookup.ts) | strips, a part's wires, what a hover lights up, the hole under the pointer |
| [src/lib/guide.ts](src/lib/guide.ts) | the build guide: the holes a step goes in, tagged on the board, and where to zoom to |
| [src/lib/components/studio/](src/lib/components/studio) | the board, the circuit, the lists, the inspector, the checklist and issues drawer, the bars |
| [Drawing.svelte](src/lib/components/studio/Drawing.svelte) | one of bbgen's drawings with pan, zoom, hover and selection: the board and the circuit are both one |
| [src-tauri/src/lib.rs](src-tauri/src/lib.rs) | the native side: runs bbgen, lists examples, watches files |

The top bar switches the centre between the **Board**, the **Circuit** (the schematic `bbgen build --schematic`
draws; the studio asks for it only while it's shown) and **Both**, side by side. Hover and selection are one for
both: a net or part lights in each, a wire on the board lights its net in the circuit, and a pin picked in the
circuit (`pin:U1.5`) lights its hole and strip on the board.

**Export** (top bar) writes bbgen's files for the open circuit: the board as `bbgen build` draws it, the bare
board, the checklist, the schematic, or all three into a folder. bbgen builds them afresh, so they're what
`bbgen build` would write. In the browser they're downloads.

**Generation** (top bar) watches bbgen build the board: it rebuilds with `bbgen build --events`, which prints
each step of placing and wiring as it takes it. The centre shows the board as it stood at the latest kept step,
the parts and wires that step changed ringed, with the phase (starting layouts, turning, moving parts, wiring)
and what the step did. Under it, the score of every layout tried (a dot each, the kept ones larger, the best so
far as a line, on a log scale: an error costs 1000) and every step in a log. The scrubber, ← and →, a
click on the chart or on a kept step in the log show the board as it was then. Right-click a kept step in the
log and **Select for compare**, then right-click another and **Compare with selected**: the centre shows both
boards, the earlier left of a divider and the later right of it; drag the divider to see more of either, Esc
to stop. Saves rebuild step by step while
Generation is on; Inspect's rebuilds don't ask for the steps, since on a placed file the drawings they carry
make the build ten times longer. While a build runs, **Rebuild** is **Stop**, which kills bbgen and leaves the
last board up.

**Build guide** (top bar) walks you through building the board, step by step, in the checklist's building order
(wires that parts go over, chips and modules, small parts, wires, flexible jumpers, flying leads). What's not in
yet is drawn faint and dashed, the step in hand glows with its holes tagged in large type, and the board glides to
it (**Follow**). **Done, next** (→, Space or Enter) ticks it off and moves on, **Back** (←) goes back; the drawer
lists every step with a box to tick. What's done is kept per circuit in the app's data folder, so the guide picks
up where you left off. **Bench view** (B, Esc to leave) fills the screen with the board and the step, in larger
type.

Zoomed in, a minimap in the bottom right shows where you are; drag on it to move the view.

The board is bbgen's own SVG, put in the page once per build. Hover and selection add classes to its
`data-part`, `data-wire`, `data-net` and `data-issue` groups; nothing is redrawn as the pointer moves.

## Checks

```bash
pnpm check
```

```bash
pnpm test
```

The tests build every example and benchmark (as placed) with `../target/release/bbgen` (or `$BBGEN`), and
are skipped when it isn't built. `src/lib/done.spec.ts` is Phase 1's done check: everything in a scene lights
something on the board and in the schematic, and a broken file says on which line.
