// The only way the studio reaches bbgen. In the desktop app that's Tauri commands (src-tauri); under `pnpm dev`
// in a browser it's the dev server's endpoints (vite/bbgen-dev.ts). Either way bbgen runs as its own process.

import type { BuildOutput } from './scene';

export interface Engine {
	kind: 'tauri' | 'web';
	/** run `bbgen build <path> --json`, with `--schematic` when asked (`--rails`: its supplies as wires) */
	build(path: string, schematic: boolean, rails: boolean): Promise<BuildOutput>;
	/** circuit files that come with bbgen (its examples and benchmarks), when it can find them */
	examples(): Promise<string[]>;
	/** call onChange when the file is saved; returns a function that stops watching */
	watch(path: string, onChange: () => void): () => void;
	/** a native open-file dialog, where there is one */
	pick: (() => Promise<string | null>) | null;
}

function inTauri(): boolean {
	return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
}

async function tauriEngine(): Promise<Engine> {
	const { invoke } = await import('@tauri-apps/api/core');
	const { listen } = await import('@tauri-apps/api/event');
	const { open } = await import('@tauri-apps/plugin-dialog');
	return {
		kind: 'tauri',
		build: (path, schematic, rails) => invoke<BuildOutput>('build', { path, schematic, rails }),
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
		build: (path, schematic, rails) =>
			get<BuildOutput>(`/api/build?path=${encodeURIComponent(path)}&schematic=${schematic ? 1 : 0}&rails=${rails ? 1 : 0}`),
		examples: () => get<string[]>('/api/examples').catch(() => []),
		watch(path, onChange) {
			const source = new EventSource(`/api/watch?path=${encodeURIComponent(path)}`);
			source.onmessage = () => onChange();
			return () => source.close();
		},
		pick: null
	};
}

let engine: Promise<Engine> | null = null;

export function getEngine(): Promise<Engine> {
	engine ??= inTauri() ? tauriEngine() : Promise.resolve(webEngine());
	return engine;
}
