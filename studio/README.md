# bbgen studio

A desktop app for bbgen: open a circuit, build it, and inspect the breadboard. Hovering a part, wire, net,
issue or hole lights it up on the board and in every list; clicking it shows what matters about it in the
inspector. The circuit file is watched, so saving it (from an editor, or Claude Code) rebuilds the board.

The studio never links bbgen's library: every build runs the `bbgen` binary as `bbgen build FILE --json` and
reads the scene it prints ([docs/scene-format.md](../docs/scene-format.md)). The plan, and what comes after
inspecting (generation progress, a build guide, editing, an assistant), is in
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
open a file by typing its full path in the Open dialog.

Which bbgen runs: `$BBGEN` if set, else a `bbgen` beside the app, else this checkout's `bin/bbgen` (which
builds bbgen first when its sources changed), else `bbgen` on `PATH`.

## Where things are

| | |
|---|---|
| [src/lib/engine.ts](src/lib/engine.ts) | the only way the UI reaches bbgen: Tauri commands, or the dev server's endpoints |
| [src/lib/scene.ts](src/lib/scene.ts) | the scene's types |
| [src/lib/studio.svelte.ts](src/lib/studio.svelte.ts) | the state: the open file, its last good build, hover and selection |
| [src/lib/lookup.ts](src/lib/lookup.ts) | strips, a part's wires, what a hover lights up, the hole under the pointer |
| [src/lib/components/studio/](src/lib/components/studio) | the board, the lists, the inspector, the checklist and issues drawer, the bars |
| [src-tauri/src/lib.rs](src-tauri/src/lib.rs) | the native side: runs bbgen, lists examples, watches files |

The board is bbgen's own SVG, put in the page once per build. Hover and selection add classes to its
`data-part`, `data-wire`, `data-net` and `data-issue` groups; nothing is redrawn as the pointer moves.

## Checks

```bash
pnpm check
```

```bash
pnpm test
```

The tests build `examples/eeprom.yaml` with `../target/release/bbgen` (or `$BBGEN`), and are skipped when it
isn't built.
