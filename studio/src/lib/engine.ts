// The only way the studio reaches bbgen. In the desktop app that's Tauri commands (src-tauri); under `pnpm dev`
// in a browser it's the dev server's endpoints (vite/bbgen-dev.ts). Either way bbgen runs as its own process.

import type { BuildOutput } from './scene';

/** files dragged over the window: their paths, or null where the engine can't know them (a browser) */
export type Drag = { type: 'over'; paths: string[] | null } | { type: 'drop'; paths: string[] | null } | { type: 'leave' };

export interface Engine {
	kind: 'tauri' | 'web';
	/** run `bbgen build <path> --json`, with `--schematic` when asked (`--wires`: every net a wire, no labels) */
	build(path: string, schematic: boolean, wires: boolean): Promise<BuildOutput>;
	/** circuit files that come with bbgen (its examples and benchmarks), when it can find them */
	examples(): Promise<string[]>;
	/** call onChange when the file is saved; returns a function that stops watching */
	watch(path: string, onChange: () => void): () => void;
	/** a native open-file dialog, where there is one */
	pick: (() => Promise<string | null>) | null;
	/** call onDrag as files are dragged over the window and dropped on it */
	onDrag(onDrag: (e: Drag) => void): void;
}

function inTauri(): boolean {
	return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
}

async function tauriEngine(): Promise<Engine> {
	const { invoke } = await import('@tauri-apps/api/core');
	const { listen } = await import('@tauri-apps/api/event');
	const { open } = await import('@tauri-apps/plugin-dialog');
	const { getCurrentWebview } = await import('@tauri-apps/api/webview');
	return {
		kind: 'tauri',
		build: (path, schematic, wires) => invoke<BuildOutput>('build', { path, schematic, wires }),
		examples: () => invoke<string[]>('examples'),
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
		build: (path, schematic, wires) =>
			get<BuildOutput>(`/api/build?path=${encodeURIComponent(path)}&schematic=${schematic ? 1 : 0}&wires=${wires ? 1 : 0}`),
		examples: () => get<string[]>('/api/examples').catch(() => []),
		watch(path, onChange) {
			const source = new EventSource(`/api/watch?path=${encodeURIComponent(path)}`);
			source.onmessage = () => onChange();
			return () => source.close();
		},
		pick: null,
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
