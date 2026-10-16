// Lookups over a scene: what's in each strip, which wires touch a part, what a key lights up on the board.

import { pinPart, type Key, type Net, type Part, type Scene, type Wire, type Xy } from './scene';

export interface StripInfo {
	id: string;
	/** a readable name: `f–j 30`, `top+ rail` */
	label: string;
	holes: string[];
	pins: { part: string; pin: string }[];
	wires: string[];
	nets: string[];
}

export interface SceneIndex {
	part: Map<string, Part>;
	wire: Map<string, Wire>;
	net: Map<string, Net>;
	/** hole name → strip id */
	stripOf: Map<string, string>;
	strip: Map<string, StripInfo>;
	/** every hole with its position, for finding the one under the pointer */
	holes: { name: string; xy: Xy }[];
	wiresOfPart: Map<string, string[]>;
	partsOfNet: Map<string, string[]>;
	wiresOfNet: Map<string, string[]>;
	/** hole name → what sits in it (`U1.5 SDA`, `W6`) */
	inHole: Map<string, string[]>;
}

const TERMINAL = /^(?:(.+):)?([a-j])(\d+)$/;
const RAIL = /^(?:(.+):)?((?:top|bot)[+-])(\d+)$/;

/** The strip a hole belongs to: its column's half of the terminal area, or its rail (each half when split). */
export function stripId(hole: string, scene: Scene): { id: string; label: string } | null {
	const t = TERMINAL.exec(hole);
	if (t) {
		const board = t[1] ?? scene.boards[0]?.id ?? '1';
		const top = t[2] <= 'e';
		return { id: `${board}:${top ? 'top' : 'bot'}:${t[3]}`, label: `${top ? 'a–e' : 'f–j'} ${t[3]}` };
	}
	const r = RAIL.exec(hole);
	if (r) {
		const board = r[1] ?? scene.boards[0]?.id ?? '1';
		const b = scene.boards.find((x) => x.id === board);
		const half = b?.split_rails ? (Number(r[3]) <= b.columns / 2 ? ' (left)' : ' (right)') : '';
		return { id: `${board}:${r[2]}${half}`, label: `${r[2]} rail${half}` };
	}
	return null;
}

function push<K, V>(m: Map<K, V[]>, k: K, v: V) {
	const list = m.get(k);
	if (!list) m.set(k, [v]);
	else if (!list.includes(v)) list.push(v);
}

export function indexScene(scene: Scene): SceneIndex {
	const ix: SceneIndex = {
		part: new Map(scene.parts.map((p) => [p.ref, p])),
		wire: new Map(scene.wires.map((w) => [w.id, w])),
		net: new Map(scene.nets.map((n) => [n.name, n])),
		stripOf: new Map(),
		strip: new Map(),
		holes: [],
		wiresOfPart: new Map(),
		partsOfNet: new Map(),
		wiresOfNet: new Map(),
		inHole: new Map()
	};
	for (const b of scene.boards) {
		for (const [name, xy] of Object.entries(b.holes)) {
			ix.holes.push({ name, xy });
			const s = stripId(name, scene);
			if (!s) continue;
			ix.stripOf.set(name, s.id);
			if (!ix.strip.has(s.id)) ix.strip.set(s.id, { id: s.id, label: s.label, holes: [], pins: [], wires: [], nets: [] });
			ix.strip.get(s.id)!.holes.push(name);
		}
	}
	const stripAt = (hole: string | null) => (hole ? ix.strip.get(ix.stripOf.get(hole) ?? '') : undefined);
	for (const p of scene.parts) {
		for (const pin of p.pins) {
			const label = `${p.ref}.${pin.num}${pin.name && pin.name !== String(pin.num) ? ' ' + pin.name : ''}`;
			if (pin.hole) push(ix.inHole, pin.hole, label);
			const s = stripAt(pin.hole);
			if (s) {
				s.pins.push({ part: p.ref, pin: label });
				if (pin.net && !s.nets.includes(pin.net)) s.nets.push(pin.net);
			}
			if (pin.net) push(ix.partsOfNet, pin.net, p.ref);
		}
	}
	for (const w of scene.wires) {
		for (const n of w.nets) push(ix.wiresOfNet, n, w.id);
		for (const end of [w.a, w.b]) {
			if (!end) continue;
			if (end.pin) push(ix.wiresOfPart, pinPart(end.pin), w.id);
			if (end.hole) push(ix.inHole, end.hole, w.id);
			const s = stripAt(end.hole);
			if (s) {
				if (!s.wires.includes(w.id)) s.wires.push(w.id);
				for (const n of w.nets) if (!s.nets.includes(n)) s.nets.push(n);
			}
		}
	}
	// a part's wires: those ending in one of its pins' strips
	for (const p of scene.parts) {
		for (const pin of p.pins) {
			for (const w of stripAt(pin.hole)?.wires ?? []) push(ix.wiresOfPart, p.ref, w);
		}
	}
	for (const list of ix.wiresOfPart.values()) list.sort((a, b) => ix.wire.get(a)!.num - ix.wire.get(b)!.num);
	return ix;
}

/** A value for a quoted CSS attribute selector (CSS.escape where there is one, as in a browser). */
const escape = (v: string) => (typeof CSS !== 'undefined' ? CSS.escape(v) : v.replace(/["\\]/g, '\\$&'));

/** What the drawing should light up for a key, as CSS selectors over its `data-*` groups. */
export function selectorsFor(key: Key, scene: Scene, ix: SceneIndex): string[] {
	const q = (attr: string, v: string) => `[data-${attr}="${escape(v)}"]`;
	const name = key.slice(key.indexOf(':') + 1);
	switch (key.slice(0, key.indexOf(':'))) {
		case 'part':
			return [q('part', name)];
		case 'wire':
			return [q('wire', name)];
		case 'net':
			return [`[data-net~="${escape(name)}"]`, ...(ix.partsOfNet.get(name) ?? []).map((r) => q('part', r))];
		case 'issue': {
			const issue = scene.issues[Number(name)];
			return [q('issue', name), ...(issue?.parts ?? []).map((r) => q('part', r))];
		}
		case 'hole':
			return stripSelectors(name, ix);
		case 'pin': {
			// the pin's part, and what's in its strip
			const hole = pinHole(name, ix);
			return [q('part', name.split('.')[0]), ...(hole ? stripSelectors(hole, ix) : [])];
		}
	}
	return [];
}

function stripSelectors(hole: string, ix: SceneIndex): string[] {
	const q = (attr: string, v: string) => `[data-${attr}="${escape(v)}"]`;
	const s = ix.strip.get(ix.stripOf.get(hole) ?? '');
	if (!s) return [];
	return [...new Set(s.pins.map((p) => p.part))].map((r) => q('part', r)).concat(s.wires.map((w) => q('wire', w)));
}

/** A pin's hole on the board: `U1.5` (its part and number) → `e33`. */
export function pinHole(pin: string, ix: SceneIndex): string | null {
	const [ref, num] = pin.split('.');
	return ix.part.get(ref)?.pins.find((p) => String(p.num) === num)?.hole ?? null;
}

/** The net a pin is on: `U1.5` → `SDA`. */
export function pinNet(pin: string, ix: SceneIndex): string | null {
	const [ref, num] = pin.split('.');
	return ix.part.get(ref)?.pins.find((p) => String(p.num) === num)?.net ?? null;
}

/**
 * What the schematic should light up for a key. Its groups are `data-part` (a symbol), `data-net` (a net's wires,
 * labels and power symbols) and `data-pin`. A wire has no place in a schematic, so it lights its nets; a hole
 * lights the pins in its strip.
 */
export function circuitSelectors(key: Key, scene: Scene, ix: SceneIndex): string[] {
	const q = (attr: string, v: string) => `[data-${attr}="${escape(v)}"]`;
	const name = key.slice(key.indexOf(':') + 1);
	const pinsOf = (pins: string[]) => pins.map((p) => q('pin', p.split(' ')[0]));
	switch (key.slice(0, key.indexOf(':'))) {
		case 'part':
			return [q('part', name)];
		case 'pin': {
			const net = pinNet(name, ix);
			return [q('pin', name), ...(net ? [q('net', net)] : [])];
		}
		case 'wire':
			return (ix.wire.get(name)?.nets ?? []).flatMap((n) => [q('net', n), ...pinsOf(ix.net.get(n)?.pins ?? [])]);
		case 'net':
			return [q('net', name), ...pinsOf(ix.net.get(name)?.pins ?? [])];
		case 'issue':
			return (scene.issues[Number(name)]?.parts ?? []).map((r) => q('part', r));
		case 'hole': {
			const s = ix.strip.get(ix.stripOf.get(name) ?? '');
			return s ? s.pins.map((p) => q('pin', p.pin.split(' ')[0])) : [];
		}
	}
	return [];
}

/** The hole nearest a point, if it's within half a hole's pitch. */
export function holeAt(ix: SceneIndex, x: number, y: number): string | null {
	let best: string | null = null;
	let bd = 10 * 10;
	for (const h of ix.holes) {
		const d = (h.xy[0] - x) ** 2 + (h.xy[1] - y) ** 2;
		if (d < bd) {
			bd = d;
			best = h.name;
		}
	}
	return best;
}
