// The only way the studio reaches bbgen. In the desktop app that's Tauri commands (src-tauri); under `pnpm dev`
// in a browser it's the dev server's endpoints (vite/bbgen-dev.ts). Either way bbgen runs as its own process.

import type { BuildOutput, StepEvent } from './scene';

/** files dragged over the window: their paths, or null where the engine can't know them (a browser) */
export type Drag = { type: 'over'; paths: string[] | null } | { type: 'drop'; paths: string[] | null } | { type: 'leave' };

/** what the export menu writes: one of bbgen's files, or all of them (the board, its checklist, the schematic) */
export type Export = 'board' | 'bare' | 'checklist' | 'schematic' | 'all';

const EXPORTS: Record<Exclude<Export, 'all'>, { ext: string; kind: string }> = {
	board: { ext: 'svg', kind: 'SVG drawing' },
	bare: { ext: 'bare.svg', kind: 'SVG drawing' },
	checklist: { ext: 'md', kind: 'Markdown' },
	schematic: { ext: 'schematic.svg', kind: 'SVG drawing' }
};

const folderOf = (path: string) => path.replace(/[\\/][^\\/]*$/, '');
const stemOf = (path: string) => path.split(/[\\/]/).pop()!.replace(/\.ya?ml$/i, '');

/** the name an export is saved under unless you pick another: `eeprom.svg`, `eeprom.bare.svg`, ... */
export function exportName(path: string, what: Exclude<Export, 'all'>): string {
	return `${stemOf(path)}.${EXPORTS[what].ext}`;
}

/** the bbgen the engine runs: where, how it was found, its version (`bbgen 0.1.0`), or why it can't be run */
export interface BbgenInfo {
	path: string;
	how: string;
	version: string | null;
	error: string | null;
}

export interface Engine {
	kind: 'tauri' | 'web';
	/** run `bbgen build <path> --json`, with `--schematic` when asked (`--wires`: every net a wire, no labels); with
	 * onStep, `--events`, and each step to onStep as bbgen takes it. A newer build stops the one running. */
	build(path: string, schematic: boolean, wires: boolean, onStep?: (e: StepEvent) => void): Promise<BuildOutput>;
	/** stop the build that's running: it ends with no exit status */
	stop(): Promise<void>;
	/** circuit files that come with bbgen (its examples and benchmarks), when it can find them */
	examples(): Promise<string[]>;
	/** the circuit the app was started with (`bbgen studio FILE`), the first time it's asked; else null */
	opened(): Promise<string | null>;
	/** call onOpen with each circuit sent while the app is open (`bbgen studio FILE` again) */
	onOpen(onOpen: (path: string) => void): void;
	/** call onChange when the file is saved; returns a function that stops watching */
	watch(path: string, onChange: () => void): () => void;
	/** a native open-file dialog, where there is one */
	pick: (() => Promise<string | null>) | null;
	/** a native dialog to pick a bbgen binary, where there is one */
	pickBbgen: (() => Promise<string | null>) | null;
	/** open the circuit file in an editor, at a line if given */
	edit(path: string, line: number | null): Promise<void>;
	/** write one of bbgen's files where you choose (all of them into a folder); the files written, or null if
	 * you cancelled. The schematic is drawn as the circuit view shows it (`wires`: no labels). */
	export(path: string, what: Export, wires: boolean): Promise<string[] | null>;
	/** run `path` as bbgen from now on (null: find it as usual), after checking it is one; which bbgen runs now */
	useBbgen(path: string | null): Promise<BbgenInfo>;
	/** call onDrag as files are dragged over the window and dropped on it */
	onDrag(onDrag: (e: Drag) => void): void;
	/** the build guide's steps done for a circuit (each its checklist item and text), kept in the app's data folder */
	progress(path: string): Promise<string[]>;
	saveProgress(path: string, done: string[]): Promise<void>;
	/** fill the screen with the window, or stop */
	fullscreen(on: boolean): Promise<void>;
}

function inTauri(): boolean {
	return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
}

async function tauriEngine(): Promise<Engine> {
	const { invoke, Channel } = await import('@tauri-apps/api/core');
	const { listen } = await import('@tauri-apps/api/event');
	const { open, save } = await import('@tauri-apps/plugin-dialog');
	const { getCurrentWebview } = await import('@tauri-apps/api/webview');
	return {
		kind: 'tauri',
		build(path, schematic, wires, onStep) {
			const steps = new Channel<StepEvent>();
			if (onStep) steps.onmessage = onStep;
			return invoke<BuildOutput>('build', { path, schematic, wires, events: !!onStep, onStep: steps });
		},
		stop: () => invoke('stop'),
		examples: () => invoke<string[]>('examples'),
		opened: () => invoke<string | null>('opened'),
		onOpen(onOpen) {
			listen<string>('open-file', (e) => onOpen(e.payload));
		},
		edit: (path, line) => invoke('edit', { path, line }),
		useBbgen: (path) => invoke<BbgenInfo>('use_bbgen', { path }),
		progress: (path) => invoke<string[]>('progress', { path }),
		saveProgress: (path, done) => invoke('save_progress', { path, done }),
		async fullscreen(on) {
			const { getCurrentWindow } = await import('@tauri-apps/api/window');
			await getCurrentWindow().setFullscreen(on);
		},
		async export(path, what, wires) {
			let to: string | null;
			if (what === 'all') {
				const dir = await open({ directory: true, defaultPath: folderOf(path), title: 'Export into a folder' });
				to = typeof dir === 'string' ? dir : null;
			} else {
				const { ext, kind } = EXPORTS[what];
				to = await save({
					defaultPath: `${folderOf(path)}/${exportName(path, what)}`,
					filters: [{ name: kind, extensions: [ext.split('.').pop()!] }]
				});
			}
			return to ? invoke<string[]>('export', { path, what, wires, to }) : null;
		},
		watch(path, onChange) {
			let stop: (() => void) | null = null;
			let stopped = false;
			listen<string>('file-changed', (e) => {
				if (e.payload === path) onChange();
			}).then((un) => (stopped ? un() : (stop = un)));
			invoke('watch', { path });
			return () => {
				stopped = true;
				stop?.();
				invoke('unwatch', { path });
			};
		},
		async pick() {
			const file = await open({ multiple: false, filters: [{ name: 'Circuit', extensions: ['yaml', 'yml'] }] });
			return typeof file === 'string' ? file : null;
		},
		async pickBbgen() {
			const file = await open({ multiple: false, title: 'Pick a bbgen to run' });
			return typeof file === 'string' ? file : null;
		},
		onDrag(onDrag) {
			// 'over' doesn't repeat the paths 'enter' gave
			let paths: string[] = [];
			getCurrentWebview().onDragDropEvent(({ payload: e }) => {
				if (e.type === 'enter') paths = e.paths;
				if (e.type === 'enter' || e.type === 'over') onDrag({ type: 'over', paths });
				else if (e.type === 'drop') onDrag({ type: 'drop', paths: e.paths });
				else onDrag({ type: 'leave' });
			});
		}
	};
}

function webEngine(): Engine {
	const get = async <T>(url: string): Promise<T> => {
		const r = await fetch(url);
		const body = await r.json();
		if (!r.ok) throw new Error(body.error ?? r.statusText);
		return body as T;
	};
	return {
		kind: 'web',
		async build(path, schematic, wires, onStep) {
			const q = `path=${encodeURIComponent(path)}&schematic=${schematic ? 1 : 0}&wires=${wires ? 1 : 0}`;
			if (!onStep) return get<BuildOutput>(`/api/build?${q}`);
			// newline-delimited JSON: the steps as bbgen takes them, then {"done": the build}
			const r = await fetch(`/api/build?${q}&events=1`);
			if (!r.ok || !r.body) throw new Error((await r.json().catch(() => null))?.error ?? r.statusText);
			const reader = r.body.pipeThrough(new TextDecoderStream()).getReader();
			let rest = '';
			for (;;) {
				const { value, done } = await reader.read();
				const lines = (rest + (value ?? '')).split('\n');
				rest = done ? '' : lines.pop()!;
				for (const line of lines.filter(Boolean)) {
					const x = JSON.parse(line);
					if (x.done) return x.done as BuildOutput;
					onStep(x as StepEvent);
				}
				if (done) throw new Error('the dev server stopped before the build ended');
			}
		},
		async stop() {
			await get('/api/stop');
		},
		examples: () => get<string[]>('/api/examples').catch(() => []),
		// a browser isn't started with a circuit, nor sent one
		opened: async () => null,
		onOpen() {},
		// a browser can't write where you choose: the files are downloads
		async export(path, what, wires) {
			const q = `path=${encodeURIComponent(path)}&what=${what}&wires=${wires ? 1 : 0}`;
			const { files } = await get<{ files: { name: string; text: string }[] }>(`/api/export?${q}`);
			const names: string[] = [];
			for (const f of files) {
				const name = what === 'all' ? f.name : exportName(path, what);
				const a = document.createElement('a');
				a.href = URL.createObjectURL(new Blob([f.text], { type: name.endsWith('.md') ? 'text/markdown' : 'image/svg+xml' }));
				a.download = name;
				a.click();
				setTimeout(() => URL.revokeObjectURL(a.href), 1000);
				names.push(name);
			}
			return names;
		},
		useBbgen: (path) => get<BbgenInfo>(`/api/bbgen${path ? `?path=${encodeURIComponent(path)}` : ''}`),
		progress: (path) => get<string[]>(`/api/progress?path=${encodeURIComponent(path)}`),
		async saveProgress(path, done) {
			const r = await fetch(`/api/progress?path=${encodeURIComponent(path)}`, { method: 'POST', body: JSON.stringify({ done }) });
			if (!r.ok) throw new Error((await r.json().catch(() => null))?.error ?? r.statusText);
		},
		async fullscreen(on) {
			if (on && !document.fullscreenElement) await document.documentElement.requestFullscreen?.();
			else if (!on && document.fullscreenElement) await document.exitFullscreen();
		},
		edit: (path, line) => get(`/api/edit?path=${encodeURIComponent(path)}${line ? `&line=${line}` : ''}`),
		watch(path, onChange) {
			const source = new EventSource(`/api/watch?path=${encodeURIComponent(path)}`);
			source.onmessage = () => onChange();
			return () => source.close();
		},
		pick: null,
		pickBbgen: null,
		onDrag(onDrag) {
			// a browser gives a dropped file's name and contents but not its path, so a drop can't open it; it's
			// caught all the same, or the browser would leave the studio to show the file
			const files = (e: DragEvent) => !!e.dataTransfer?.types.includes('Files');
			let depth = 0; // dragenter and dragleave fire for each element crossed
			window.addEventListener('dragenter', (e) => {
				if (files(e) && depth++ === 0) onDrag({ type: 'over', paths: null });
			});
			window.addEventListener('dragleave', (e) => {
				if (files(e) && --depth === 0) onDrag({ type: 'leave' });
			});
			window.addEventListener('dragover', (e) => {
				if (files(e)) e.preventDefault();
			});
			window.addEventListener('drop', (e) => {
				if (!files(e)) return;
				e.preventDefault();
				depth = 0;
				onDrag({ type: 'drop', paths: null });
			});
		}
	};
}

let engine: Promise<Engine> | null = null;

export function getEngine(): Promise<Engine> {
	engine ??= inTauri() ? tauriEngine() : Promise.resolve(webEngine());
	return engine;
}
