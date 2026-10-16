// The studio's state: the open circuit, its last good build, and what's hovered and selected. Every view reads
// and writes this one object, so hovering a row in a list lights the same thing on the board.

import { toast } from 'svelte-sonner';
import { getEngine, type BbgenInfo, type Drag, type Engine, type Export } from './engine';
import { indexScene, type SceneIndex } from './lookup';
import { isError, SCHEMA, type BuildOutput, type Key, type Scene, type SceneError, type StepEvent } from './scene';

const RECENT = 'bbgen-studio.recent';
const VIEW = 'bbgen-studio.view';
const WIRES = 'bbgen-studio.wires';
const BBGEN = 'bbgen-studio.bbgen';

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
	/** the steps of the latest build that showed them (`--events`), as bbgen took them, and the latest */
	steps = $state.raw<StepEvent[]>([]);
	step = $state.raw<StepEvent | null>(null);
	/** whether the build running shows its steps */
	stepping = $state(false);
	/** the file the steps are of */
	stepsFor = $state<string | null>(null);
	/** the kept step Generation mode shows: its number, or null for the latest */
	shown = $state<number | null>(null);
	/** this file's steps, the kept ones (those with a drawing), and the one Generation mode shows */
	ofFile = $derived(this.stepsFor === this.path ? this.steps : []);
	kept = $derived(this.ofFile.filter((e) => e.svg));
	keptAt = $derived(this.shown === null ? this.kept.length - 1 : this.kept.findIndex((e) => e.n === this.shown));
	shownStep = $derived<StepEvent | null>(this.kept[this.keptAt] ?? null);
	/** following bbgen as it goes: running, and showing the latest step */
	live = $derived(this.stepping && this.shown === null);
	/** Inspect the last build, or watch bbgen generate it (every build then shows its steps) */
	mode = $state<'inspect' | 'generation'>('inspect');
	exporting = $state(false);
	watching = $state(false);
	builtAt = $state<Date | null>(null);

	hover = $state<Key | null>(null);
	selected = $state<Key | null>(null);
	/** the hole under the pointer on the board */
	hole = $state<string | null>(null);

	/** what the centre shows: the board, the circuit's schematic, or both side by side */
	view = $state<'board' | 'circuit' | 'both'>(load(VIEW, 'board'));
	/** the circuit's supplies drawn as wires from one power symbol each, rather than a symbol at every pin */
	wires = $state<boolean>(load(WIRES, false));
	list = $state<'parts' | 'nets' | 'wires'>('parts');
	drawer = $state<'checklist' | 'issues' | 'output'>('checklist');
	filter = $state('');
	dim = $state(true);
	openDialog = $state(false);
	/** files being dragged over the window: the circuit a drop would open, if any; paths: false in a browser */
	dragging = $state.raw<{ circuit: string | null; paths: boolean } | null>(null);

	/** the bbgen that runs, and how it was found */
	bbgen = $state.raw<BbgenInfo | null>(null);
	examples = $state.raw<string[]>([]);
	recent = $state<string[]>(load(RECENT, []));

	#seq = 0;
	#stopped = 0;
	#unwatch: (() => void) | null = null;
	#debounce: ReturnType<typeof setTimeout> | undefined;

	async init() {
		this.engine = await getEngine();
		this.engine.onDrag((e) => this.drag(e));
		// the bbgen picked last time, if it still runs; else the usual one
		const picked = load<string | null>(BBGEN, null);
		this.bbgen = await this.engine.useBbgen(picked).catch(() => this.engine!.useBbgen(null)).catch(() => null);
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
			this.steps = [];
			this.step = null;
			this.stepsFor = null;
			this.#unwatch = this.engine.watch(path, () => {
				clearTimeout(this.#debounce);
				this.#debounce = setTimeout(() => this.rebuild(), 150);
			});
			this.watching = true;
		}
		await this.rebuild();
	}

	/** run `path` as bbgen from now on (null: the usual one), and rebuild with it; why not, if it isn't a bbgen */
	async useBbgen(path: string | null): Promise<string | null> {
		if (!this.engine) return null;
		try {
			this.bbgen = await this.engine.useBbgen(path);
		} catch (e) {
			return String(e).replace(/^Error: /, '');
		}
		save(BBGEN, path);
		await this.rebuild();
		return null;
	}

	/** open the circuit file in an editor, at a line if given */
	async edit(line: number | null) {
		if (!this.engine || !this.path) return;
		try {
			await this.engine.edit(this.path, line);
		} catch (e) {
			toast.error("Couldn't open an editor", { description: String(e) });
		}
	}

	/** write one of bbgen's files (or all of them) where the engine asks you to; its schematic as the view shows it */
	async export(what: Export) {
		if (!this.engine || !this.path) return;
		this.exporting = true;
		try {
			const files = await this.engine.export(this.path, what, this.wires);
			if (files?.length) {
				const name = (p: string) => p.split(/[\\/]/).pop();
				toast.success(files.length === 1 ? `Exported ${name(files[0])}` : `Exported ${files.length} files`, {
					// where it went, when the engine wrote it rather than the browser downloading it
					description: files.length > 1 ? files.map(name).join(', ') : files[0] !== name(files[0]) ? files[0] : undefined
				});
			}
		} catch (e) {
			toast.error("Couldn't export", { description: String(e).replace(/^Error: /, '') });
		} finally {
			this.exporting = false;
		}
	}

	/** files dragged over the window: a drop opens the first circuit file among them */
	drag(e: Drag) {
		if (e.type === 'leave') {
			this.dragging = null;
			return;
		}
		const circuit = e.paths?.find((p) => /\.ya?ml$/i.test(p)) ?? null;
		if (e.type === 'over') {
			this.dragging = { circuit, paths: e.paths !== null };
			return;
		}
		this.dragging = null;
		if (circuit) this.open(circuit);
		else if (e.paths === null) {
			toast.info("A browser doesn't say where a dropped file is", { description: 'Open it by its path instead.' });
			this.openDialog = true;
		} else toast.error('Not a circuit file', { description: 'Drop a .yaml or .yml file.' });
	}

	/** build the circuit again; with `steps` (in Generation mode), showing each step bbgen takes as it takes it
	 * (`--events`) */
	async rebuild(steps = this.mode === 'generation') {
		if (!this.engine || !this.path) return;
		const seq = ++this.#seq;
		this.building = true;
		this.stepping = steps;
		let onStep: ((e: StepEvent) => void) | undefined;
		if (steps) {
			const taken: StepEvent[] = [];
			this.steps = [];
			this.step = null;
			this.stepsFor = this.path;
			this.shown = null;
			onStep = (e) => {
				if (seq !== this.#seq) return;
				taken.push(e);
				this.steps = taken.slice();
				this.step = e;
			};
		}
		let out: BuildOutput;
		try {
			out = await this.engine.build(this.path, this.view !== 'board', this.wires, onStep);
		} catch (e) {
			out = { json: null, stderr: String(e), code: null, command: '', ms: 0 };
		}
		if (seq !== this.#seq) return; // a newer build started meanwhile
		this.stepping = false;
		if (this.#stopped === seq) {
			// stopped: the last board stays up, with no error
			this.building = false;
			this.output = out;
			toast.info('Stopped', { description: this.step ? `after ${this.step.n} steps` : undefined });
			return;
		}
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

	/** switch modes; Generation runs bbgen step by step, unless it has for this file already, and Inspect builds
	 * the board if there's none (a run stopped before it ended) */
	setMode(mode: 'inspect' | 'generation') {
		this.mode = mode;
		if (!this.path || this.building) return;
		if (mode === 'generation' && this.stepsFor !== this.path) this.rebuild(true);
		if (mode === 'inspect' && !this.scene && !this.error) this.rebuild(false);
	}

	/** show the kept step at index i of the kept ones; the last means the latest, following a run along */
	showKept(i: number) {
		if (!this.kept.length) return;
		const j = Math.max(0, Math.min(this.kept.length - 1, i));
		this.shown = j === this.kept.length - 1 ? null : this.kept[j].n;
	}

	/** show the board as it was at step n: the last kept step at or before it */
	showStep(n: number) {
		this.showKept(Math.max(0, this.kept.findLastIndex((e) => e.n <= n)));
	}

	/** stop the build that's running */
	async stop() {
		if (!this.engine || !this.building) return;
		this.#stopped = this.#seq;
		await this.engine.stop();
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

	/** draw every net of the circuit as a wire (no labels), or long ones as labels and supplies as power symbols */
	setWires(on: boolean) {
		this.wires = on;
		save(WIRES, on);
		if (this.view !== 'board') this.rebuild();
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
