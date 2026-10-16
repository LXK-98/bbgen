// The studio's state: the open circuit, its last good build, and what's hovered and selected. Every view reads
// and writes this one object, so hovering a row in a list lights the same thing on the board.

import { toast } from 'svelte-sonner';
import { getEngine, type Engine } from './engine';
import { indexScene, type SceneIndex } from './lookup';
import { isError, SCHEMA, type BuildOutput, type Key, type Scene, type SceneError } from './scene';

const RECENT = 'bbgen-studio.recent';
const VIEW = 'bbgen-studio.view';

function load<T>(key: string, fallback: T): T {
	try {
		const v = localStorage.getItem(key);
		return v ? (JSON.parse(v) as T) : fallback;
	} catch {
		return fallback;
	}
}

function save(key: string, value: unknown) {
	try {
		localStorage.setItem(key, JSON.stringify(value));
	} catch {
		// private window or storage off: only the recent list is lost
	}
}

export class Studio {
	engine = $state.raw<Engine | null>(null);
	path = $state<string | null>(null);
	/** the last build that produced a board; kept while a newer one fails */
	scene = $state.raw<Scene | null>(null);
	index = $derived<SceneIndex | null>(this.scene ? indexScene(this.scene) : null);
	/** why the newest build has no board, if it hasn't */
	error = $state.raw<SceneError['error'] | null>(null);
	output = $state.raw<BuildOutput | null>(null);
	building = $state(false);
	watching = $state(false);
	builtAt = $state<Date | null>(null);

	hover = $state<Key | null>(null);
	selected = $state<Key | null>(null);
	/** the hole under the pointer on the board */
	hole = $state<string | null>(null);

	/** what the centre shows: the board, the circuit's schematic, or both side by side */
	view = $state<'board' | 'circuit' | 'both'>(load(VIEW, 'board'));
	list = $state<'parts' | 'nets' | 'wires'>('parts');
	drawer = $state<'checklist' | 'issues' | 'output'>('checklist');
	filter = $state('');
	dim = $state(true);
	openDialog = $state(false);

	examples = $state.raw<string[]>([]);
	recent = $state<string[]>(load(RECENT, []));

	#seq = 0;
	#unwatch: (() => void) | null = null;
	#debounce: ReturnType<typeof setTimeout> | undefined;

	async init() {
		this.engine = await getEngine();
		this.examples = await this.engine.examples();
		const last = this.recent[0];
		if (last) await this.open(last);
	}

	async open(path: string) {
		if (!this.engine) return;
		const same = path === this.path;
		this.path = path;
		this.recent = [path, ...this.recent.filter((p) => p !== path)].slice(0, 8);
		save(RECENT, this.recent);
		if (!same) {
			this.#unwatch?.();
			this.selected = null;
			this.hover = null;
			this.scene = null;
			this.#unwatch = this.engine.watch(path, () => {
				clearTimeout(this.#debounce);
				this.#debounce = setTimeout(() => this.rebuild(), 150);
			});
			this.watching = true;
		}
		await this.rebuild();
	}

	async rebuild() {
		if (!this.engine || !this.path) return;
		const seq = ++this.#seq;
		this.building = true;
		let out: BuildOutput;
		try {
			out = await this.engine.build(this.path, this.view !== 'board');
		} catch (e) {
			out = { json: null, stderr: String(e), code: null, command: '', ms: 0 };
		}
		if (seq !== this.#seq) return; // a newer build started meanwhile
		this.building = false;
		this.output = out;
		const took = out.ms < 1000 ? `${out.ms} ms` : `${(out.ms / 1000).toFixed(1)} s`;
		if (isError(out.json)) {
			this.error = out.json.error;
			toast.error(`Didn't build (${took})`, { description: out.json.error.message });
		} else if (out.json && out.json.schema !== SCHEMA) {
			this.error = {
				message: `This bbgen (${out.json.bbgen}) writes scene format ${out.json.schema}; the studio reads ${SCHEMA}.`,
				line: null,
				column: null
			};
		} else if (out.json) {
			this.error = null;
			this.scene = out.json;
			this.builtAt = new Date();
			const { errors, warnings } = out.json.stats;
			const check = errors ? `${errors} ${errors === 1 ? 'error' : 'errors'}` : warnings ? `${warnings} ${warnings === 1 ? 'warning' : 'warnings'}` : 'check passed';
			toast.success(`Rebuilt in ${took}`, { description: `${out.json.title}: ${check}` });
			// keep the selection if what it names is still there
			if (this.selected && !this.exists(this.selected)) this.selected = null;
		} else {
			this.error = { message: out.stderr.trim() || 'bbgen printed nothing', line: null, column: null };
			toast.error(`Didn't build (${took})`, { description: this.error.message });
		}
	}

	exists(key: Key): boolean {
		const ix = this.index;
		if (!ix || !this.scene) return false;
		const name = key.slice(key.indexOf(':') + 1);
		if (key.startsWith('part:')) return ix.part.has(name);
		if (key.startsWith('wire:')) return ix.wire.has(name);
		if (key.startsWith('net:')) return ix.net.has(name);
		if (key.startsWith('issue:')) return Number(name) < this.scene.issues.length;
		if (key.startsWith('pin:')) {
			const [ref, num] = name.split('.');
			return !!ix.part.get(ref)?.pins.some((p) => String(p.num) === num);
		}
		return ix.stripOf.has(name);
	}

	/** show the board, the circuit, or both; the circuit needs a build with its schematic */
	show(view: 'board' | 'circuit' | 'both') {
		this.view = view;
		save(VIEW, view);
		if (view !== 'board' && this.scene && !this.scene.schematic) this.rebuild();
	}

	select(key: Key | null) {
		this.selected = key;
	}

	/** handlers for anything that names a key: hover lights it, click selects it */
	pointer(key: Key) {
		return {
			onmouseenter: () => (this.hover = key),
			onmouseleave: () => {
				if (this.hover === key) this.hover = null;
			},
			onfocus: () => (this.hover = key),
			onblur: () => {
				if (this.hover === key) this.hover = null;
			},
			onclick: () => this.select(key)
		};
	}
}

export const studio = new Studio();
