# bbgen Studio: a plan

A desktop app for bbgen: open a circuit, build it, and look at the board. Hovering over a part, wire or
net lights it up, clicking it selects it, and an inspector says what matters about it. Later the same
app shows bbgen placing and wiring as it goes, walks you through building the board step by step, edits
boards, and lets you work on them with Claude.

The studio is a separate program. It never links bbgen's library: every build, check and part search
runs the `bbgen` binary and reads what it prints. bbgen stays the one place that knows how to place,
wire, check and draw.

Mockup (three screens: Inspect, Build guide, Generation; the Inspect one works, hover and click):
https://claude.ai/artifact/JJiYFhnAVriMLC3m3ueWtc. It uses bbgen's real drawings of
`examples/eeprom.yaml` and of benchmark 09 part-way through placement, and the real captions and scores
from `bbgen dev frames --all`.

**Where it stands (October 2026):** Phase 0 is done: `bbgen build --json` prints the scene
([scene-format.md](scene-format.md)). Phase 1 mostly is: [studio/](../studio) is the Inspect mode, in
SvelteKit with shadcn-svelte in a Tauri window ([studio/README.md](../studio/README.md)), with light, dark
and system themes, the bare board (`bbgen build --bare`: no title, legend or background), a legend of its
own over the board's top right, and a toast after each rebuild. What's left, in order, is under
[Next steps](#next-steps). The sections after it are the plan as written, with what changed on the way
noted where it did.

## Next steps

### Finish Phase 1

1. **Run the desktop window.** `pnpm tauri dev` compiles, but the window has never been opened: every
   check so far was in a browser against the Vite dev server. Check in the Tauri window that the native
   Open dialog, recent files, watching (the `notify` watcher, not the dev server's), killing a running build
   when a new one starts, and the theme following the OS (WebKitGTK and `prefers-color-scheme`) all work.
   Fonts: compare the board's labels with the PNG of the same build; ship DejaVu Sans if they differ.
2. **Open by dropping a file.** Done (October 2026). The engine's `onDrag` passes on Tauri's
   `onDragDropEvent` (the dev server: the window's HTML drag events); a drop opens the first `.yaml`/`.yml`
   and ignores the rest, and while files are dragged the centre says what a drop would open, or that it's not
   a circuit. In the browser a drop has no path, so it says so and brings up the Open dialog. Checked in a
   browser, the Tauri path by calling `studio.drag` with paths; still to see in the desktop window.
3. **Show in YAML.** Done (October 2026). Parts, wires, nets and issues in the scene have a `line`
   (yaml.rs keeps each key's and list item's line, three levels deep), and an error's `line` comes from its
   path too (`parts.U1: ...`), not only from the YAML reader; `tests/scene.rs` holds every part, wire you laid
   and net of every example and benchmark to the line that names it. The inspector has "Show in YAML · line N"
   for each, and so do the error banners, in the circuit view as well as the board. It runs `$VISUAL` or
   `$EDITOR` at the line as that editor takes it (`+N file`, VS Code's `--goto file:N`, Zed's `file:N`, ...);
   a terminal editor (nano, vim, helix, ...) gets a terminal window of its own (`$TERMINAL`, else the first of
   ptyxis, gnome-terminal, konsole, kitty, alacritty, foot, wezterm, xterm); with neither set, the system's
   opener, at the top of the file. Checked in a browser, with the edit request caught; still to see in the
   desktop window.
4. **Export.** A menu in the top bar: SVG (as `bbgen build` writes it, decorations included), the bare
   SVG, the checklist (.md), and all of them, as `bbgen build FILE -o DIR` (with `--bare` for the bare
   SVG) into a folder picked in a save dialog. A new engine call, `export(path, dir, bare)`, in both
   `src-tauri/src/lib.rs` and `vite/bbgen-dev.ts`. Needs `dialog:allow-save` in the capabilities. A
   PNG would need rsvg-convert or ImageMagick; leave it out unless asked.
5. **Minimap.** A small copy of the board in a corner (the bottom right is free; the legend is top
   right, zoom top left) with the visible area as a rectangle you can drag. It's the same SVG at a fixed
   scale, so render it once per build (`<img>` of the svg as a data URL is cheaper than a second DOM
   copy). Hidden when the whole board is in view.
6. **A setting for which bbgen runs**, shown in the output tab with its version, for when `$BBGEN` and
   the lookup aren't enough.
7. **Phase 1's done check, written down as a test.** For each example and benchmark: the scene's svg
   matches `bbgen build --bare` (already in `tests/scene.rs`), every part, wire and net key lights at least
   one group (extend `lookup.spec.ts` from eeprom to all of them, as the Rust test does), a broken file
   gives an error with a line. Then by hand: a save in an editor rebuilds in under a second on benchmark 17.
8. **Small things seen along the way.** The examples' committed SVGs are stale (eeprom's is 1508 px
   wide, a build now gives 1610): regenerate them with `bbgen build`, look at them, and commit them on
   their own. A selected wire may be a different wire after a rebuild (numbers shift); leave it until
   editing, as planned.

### Then Phase 2 (generation), from bbgen's side first

1. `--events` in bbgen: JSON lines from `trace::step`, a drawing on kept steps only, as in Phase 0 §3.
   Measure the slow-down on benchmark 09 with and without drawings before deciding on the rule.
2. The engine reads stdout as a stream (Tauri: a channel from the command; dev server: SSE) instead of
   waiting for the end, and Stop kills the process.
3. A Generation mode in the top bar: the board as it is at the latest kept step, the moved part ringed
   (`focus`), the phase, the score chart, the event log; afterwards a scrubber over the kept steps.

Phases 3 to 5 stay as written below.

## What each side owns

| bbgen (the binary) | the studio |
|---|---|
| reading the YAML, placing, wiring, checking | windows, files, recent files, watching for saves |
| the drawing (SVG) and the checklist's sentences | hover, selection, zoom, which mode you're in |
| what an error or warning means | showing it next to what it's about |
| the scene format: what a build produced, as JSON | build-guide progress, later edit history and the chat |

Why run the binary rather than link the library, besides keeping the two apart:

- A build that panics or runs long can't take the window down, and Stop is killing a process.
- bbgen can change underneath; the studio only depends on the scene format, which is versioned.
- The interface the studio uses is the one scripts, the Claude Code skill and (later) the assistant use.
- It costs a process start and about 150 KB of JSON per build. Builds take under 10 ms (`eeprom`) to
  about 1 s (benchmark 17), so that's lost in the noise.

The studio shows bbgen's own drawing, not one of its own. What you inspect is exactly what
`bbgen build` writes, and a layout is judged by that drawing, so there must be only one.

## Phase 0: what bbgen needs to grow

Today bbgen writes files and prints a text report. The drawing is flat: about 1,000 rects, circles and
paths with no ids, so nothing in it says which part or wire an element belongs to. Four additions, each
useful on its own:

### 1. `bbgen build FILE --json`: the scene

One JSON document on stdout. Writes no files unless `-o` is given. Exit status as now (0 passed,
1 errors, 2 the file can't be read).

```jsonc
{
  "schema": 1, "bbgen": "0.1.0", "source": "examples/eeprom.yaml",
  "title": "Pico + I2C EEPROM", "notes": ["The LED's long leg is the anode."],
  "boards": [{"id": "1", "kind": "full", "columns": 63, "split_rails": false,
              "rails": {"top+": "3V3", "top-": "GND", "bot+": null, "bot-": "GND"},
              "holes": {"a1": [60, 152], "top+3": [100, 92], ...}}],     // centres, in SVG pixels
  "parts": [{"ref": "U1", "kind": "dip", "title": "24LC256", "value": "", "source": "dip",
             "at": "f30", "rot": 0, "placed_by": "you", "raised": false,
             "body": [630, 222, 710, 302], "keepouts": [],
             "pins": [{"num": 5, "name": "SDA", "hole": "e33", "xy": [700, 232], "net": "SDA"}, ...]}],
  "wires": [{"num": 6, "group": "i2c", "net": "SDA", "color": "#1f5fbf",
             "a": {"spec": "Pico.GP16", "hole": "b22", "pin": "Pico.21"},
             "b": {"spec": "U1.SDA", "hole": "b33", "pin": "U1.5"},
             "via": [], "points": [[480, 172], [700, 172]], "flex": false, "laid_by": "you"}],
  "nets": [{"name": "SDA", "color": "#1f5fbf", "pins": ["Pico.21", "U1.5", "J1.1"],
            "rails": [], "polarity": null, "guessed": false}],
  "issues": [{"level": "warning", "code": "DIAGONAL", "message": "...", "pins": ["R2.1"], "holes": ["j40"]}],
  "checklist": [{"section": "parts", "item": "part:U1", "text": "24LC256: pin 1 in `f30`, notch ..."},
                {"section": "i2c", "item": "wire:W6", "text": "blue: `b22` → `b33` · SDA (...)"}],
  "svg": "<svg ...>",
  "stats": {"errors": 0, "warnings": 0, "parts": 6, "wires": 10, "ms": 4}
}
```

- Everything is in the drawing's pixel space (`Canvas::x`, `Canvas::y`), so overlays line up with the
  SVG without the studio knowing about grid units, trenches or boards snapped on below.
- `holes` lets the studio name the hole under the pointer (the nearest centre within half a pitch).
  830 on a full board: small, and exact for rail gaps, the trench and several boards.
- `checklist` is the same sentences `checklist.rs` writes, with what each line is about. The inspector
  shows a part's own line, and the build guide is made from them.
- When the file can't be read: `{"schema": 1, "error": {"message": ..., "line": 3, "column": 1}}`.
  `yaml.rs` already knows the line and column; circuit errors (a bad reference) should say which
  part or net they're in.

### 2. The drawing, annotated

With `--json`, each thing in the SVG is wrapped in a group that says what it is:
`<g data-part="U1">`, `<g data-wire="W6" data-net="SDA">`, `<g data-issue="3">`. A part is drawn in
several passes (keep-outs, body, its label late so it sits over wires, a raised module over the wires
under it), so one part gets several groups with the same attribute; the studio selects all of them.
This is what makes hover and dimming cheap: a CSS class on the matching groups, no redraw.

Only `--json` annotates, so the files `bbgen build` writes stay byte for byte what `tests/outputs.rs`
holds them to.

### 3. `--events`: progress as it happens (for Phase 2)

JSON lines on stdout before the scene, one per step placement and wiring already report through
`trace::step` (starting layouts, turns, repairs, each wire):

```jsonc
{"event": "step", "phase": "repair", "n": 21, "caption": "R6 into its 4th best legs",
 "score": 250, "from": 256, "kept": true, "focus": ["R6"], "svg": "<svg ...>"}
```

The drawing comes with kept steps only; tried ones (`--video-all`'s extra frames) come without it, or
the run slows down: `trace::step` checks and draws the whole circuit for every frame, and benchmark 09
makes 150 frames with every try.

### 4. Building what isn't saved (for Phase 4)

`bbgen build - --source examples/eeprom.yaml --json` reads the YAML from stdin, with `--source` for
the title and relative paths. And `bbgen find --json` for a part picker.

Tests: the scene JSON of each example becomes a record in `tests/outputs.rs`, and the format is
written up in `docs/scene-format.md`. `schema` goes up on any change that breaks a reader; the studio
refuses a bbgen whose schema it doesn't know, and says which version it needs.

## The studio's stack

**Tauri 2, with a SvelteKit + shadcn-svelte UI** (first planned as React; Svelte was chosen when
building it). Tauri is a Rust shell around the system's web view
(WebKitGTK on Fedora). The board is an SVG, and a browser is the best thing there is at showing an SVG
and telling you which element the pointer is over; dimming everything but one net is a CSS rule.
The rest (lists, tables, the checklist, later a YAML editor and a chat) is ordinary web UI. The Rust
side is where running bbgen, watching files and, later, keeping an API key belong, and it's the
toolchain the repo already has.

Considered:

- egui or iced (native Rust): the board would have to be redrawn by the studio or rasterised from
  bbgen's SVG with hit-testing done by hand; text, tables, an editor and a chat are much more work.
- Electron: the same web UI with ten times the size, and a Node backend in a Rust project.
- A local web server (axum) and the browser: simplest to start, no native file dialogs or windows.
  The UI would be the same code, so this stays open.

SvelteKit runs as a single page (`adapter-static`, no server rendering), which is what Tauri serves.
One rule: the framework doesn't own the board. The SVG goes into the page once per build, and hover and
selection toggle classes on its groups directly; re-rendering 1,000 elements on every mouse move is what
would make it feel slow.

## The app

As in the mockup:

- **Top bar**: the file, the mode (Inspect, Build guide, Generation, Edit), the check's result, Rebuild,
  and whether it's watching the file.
- **Left**: parts, nets and wires, with a filter, and the check summary.
- **Centre**: the board, with zoom, fit, a minimap and layer toggles; a drawer under it with the
  checklist, the issues and bbgen's output.
- **Right**: the inspector; later an Assistant tab beside it.
- **Status bar**: what's under the pointer, down to the hole and its strip.

One hover and one selection, shared by every view: a row hovered in a list lights up on the board,
a checklist line selects its part, a net in a pin table selects the net.

What the inspector shows:

- **Part**: ref, title, kind and value; where it sits (pin 1, which way round, which board); whether
  you placed it or bbgen did (a leg bbgen picked from `Pico.GP15` says so); its checklist line; a pin
  table (number, name, hole, net, wires); its issues; where it came from (built-in, KiCad symbol and
  footprint, adapter); keep-outs. Actions: show in the YAML, copy its holes.
- **Wire**: number, net, group; both ends as hole and pin; length; rigid, flexible jumper or flying
  lead; laid by you or by bbgen; bends; where it comes in the build; its checklist line and issues.
- **Net**: colour, supply, ground or signal (and whether bbgen guessed that from the name), its rails,
  pins and wires, and how the check sees it: one piece, open, or shorted to what.
- **Hole** (hover, in the status bar): its name, its strip, what's in it, its net.
- **Issue**: code and message, its rings on the board; clicking it zooms there.

## Inside the studio

As built (see [studio/README.md](../studio/README.md)):

```
studio/                       its own Cargo project and package.json, beside bbgen's
  src-tauri/src/lib.rs        runs bbgen, lists the examples, watches files: the commands the UI calls
  vite/bbgen-dev.ts           the same three things for `pnpm dev` in a browser
  src/lib/
    engine.ts                 the only way the UI reaches bbgen (Tauri, or the dev server)
    scene.ts                  the scene's types
    studio.svelte.ts          the last good scene, the newest error, hover, selection
    lookup.ts                 strips, a part's wires, what a key lights up, the hole under the pointer
    components/studio/        the board, lists, inspector, drawer, bars, open dialog
```

- **A separate Cargo project, not a workspace member**, so `cargo test` and `bin/bbgen` stay as quick
  as now and Tauri's few hundred dependencies stay out of bbgen's `Cargo.lock`.
- **Finding bbgen**: `$BBGEN`, then a binary shipped next to the app (a Tauri sidecar for releases),
  then the checkout's `bin/bbgen` launcher, then `PATH`. Each scene says its schema, and the studio
  refuses one it doesn't read. (A setting in the app is still to come.)
- **Builds**: one at a time; a new build or Stop kills the running one. A build that fails keeps the
  last good board on screen with the error over it, so a typo mid-edit doesn't blank the window.
- **Watching**: rebuild about 150 ms after a save. This fits how bbgen is used today: Claude Code or an
  editor writes the YAML, the studio follows along.
- **Selection keys** are the circuit's own names: `part:U1`, `wire:W6`, `net:SDA`, `hole:1:f30`. After
  a rebuild the selection stays if its key still exists. Wire numbers can shift when wiring changes,
  so a selected wire may then be a different one; good enough to start, revisit with editing.
- **Modes share the scene and the board**: each mode is a view over the same data, not a new screen
  with its own copy, so the Assistant panel can sit beside any of them.

## Phases

**0. bbgen's machine interface.** `--json`, the annotated drawing, errors with positions, records in
the tests, `docs/scene-format.md`. Useful without the studio: scripts and the skill can read a build.

**1. Inspect.** The shell; open, recent files, drag and drop; build, watch and rebuild; the board with
pan, zoom and minimap; hover and selection everywhere; the inspector; the checklist, issues and output
drawer; the status bar; export (runs `bbgen build -o`).
Done when every example and benchmark shows the same drawing as bbgen's PNG, every part, wire and net
in them highlights and inspects, a save in an editor rebuilds in under a second, and a broken file
shows its error and line while the last board stays up.

**2. Generation.** `--events`; the live board with the moved part ringed, phases, a score chart (each
layout tried as a dot, the best so far as a line), an event log, Stop. The run's events are kept, so
afterwards you can scrub back through it: what `--video` shows, without ImageMagick or ffmpeg.

**3. Build guide.** Steps from the scene's checklist. What's not in yet is drawn dashed, the step in
hand ringed, its holes tagged in large type; Next and Back from the keyboard; a full-screen bench view.
Progress saved per circuit, in the app's data folder (a file beside the YAML would end up in git).
With the annotated drawing this is all done in the page, without asking bbgen again.

**4. Edit.** A YAML pane (CodeMirror) that edits through a parser that keeps comments (the `yaml`
package's document model), completes part types, pin names and holes, and shows bbgen's errors on
their lines; unsaved text is built through stdin. Then editing on the board: drag a part to a hole
(writes `at:`), "keep it here" on something bbgen placed (writes its `at:` or `pins:`), undo through
the YAML's history. The YAML stays the only source of truth: the studio never keeps a second format.

**5. Assistant.** A chat beside the board, aware of the selection ("why is W8 a flying lead?",
"move C1 next to U1's VCC"). Claude with tools that are the engine's commands: read the scene, build,
check, find a part, propose a YAML edit. `skill/SKILL.md` is most of its instructions already. Every
change it makes is a YAML edit shown as a diff to accept, followed by a rebuild, so it can't hand over
a board bbgen hasn't checked. The API key lives in the OS keyring, on the Rust side. Running Claude
Code headless as a subprocess, the way the studio runs bbgen, is the other way to do it; decide when
Phase 4 is done.

## Risks and open questions

- **The drawing's structure.** `render.rs` interleaves passes (late labels, raised modules drawn after
  wires). Annotating means threading "whose is this" through `Canvas::add`. Check it on all 21
  benchmarks: every element in some group, none in two.
- **Size.** About 1,000 elements per board, so three boards are about 3,000: fine for a browser if
  hover changes classes and nothing re-renders.
- **Fonts.** The SVG asks for DejaVu Sans; the web view may draw it slightly differently from the PNG
  `rsvg-convert` or ImageMagick makes. Ship DejaVu Sans with the studio if label placement looks off.
- **Wire numbers aren't stable across rebuilds** (see selection keys). Matters for editing and for an
  assistant referring to "W8" after a change.
- **Events slow builds** if every step is drawn; draw kept steps only (see Phase 0, 3).
- **Golden records.** `--json` has its own records; the CLI's files mustn't change because of it.
