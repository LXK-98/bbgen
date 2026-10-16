// One-line descriptions of what a key points at, for tooltips and the status bar.

import type { SceneIndex } from './lookup';
import { keyKind, keyName, type Key, type Part, type Scene, type Wire } from './scene';

export interface Description {
	title: string;
	lines: string[];
	color: string | null;
}

export function partName(p: Part): string {
	return [p.title, p.value].filter(Boolean).join(' ') || p.kind;
}

export function kindName(p: Part): string {
	const names: Record<string, string> = {
		pico: 'Raspberry Pi Pico',
		dip: `DIP-${p.pins.length}`,
		sip: `${p.pins.length}-pin header`,
		kicad: p.source.split(' ')[0] || 'KiCad part',
		resistor: 'resistor',
		capacitor: 'capacitor',
		electrolytic: 'electrolytic capacitor',
		diode: 'diode',
		led: 'LED',
		offboard: 'off the board'
	};
	return names[p.kind] ?? p.kind;
}

export function endName(e: Wire['a']): string {
	if (!e) return '?';
	return e.hole ?? e.pin ?? e.spec;
}

export function wireKind(w: Wire): string {
	if (w.flying) return 'flying lead to an off-board pin';
	if (w.flex) return 'flexible jumper, arching over the board';
	return 'rigid wire, flat on the board';
}

export function describe(key: Key, scene: Scene, ix: SceneIndex): Description | null {
	const name = keyName(key);
	switch (keyKind(key)) {
		case 'part': {
			const p = ix.part.get(name);
			if (!p) return null;
			const holes = p.pins.map((x) => x.hole).filter(Boolean);
			const where = p.kind === 'offboard' ? 'beside the board' : p.at ? `pin 1 in ${p.at}` : 'not placed';
			return {
				title: `${p.ref} · ${partName(p)}`,
				lines: [`${kindName(p)}, ${where}`, `${holes.length} of ${p.pins.length} pins on the board`],
				color: p.color
			};
		}
		case 'wire': {
			const w = ix.wire.get(name);
			if (!w) return null;
			return {
				title: `${w.id} · ${w.nets.join(' / ') || 'no net'}`,
				lines: [`${endName(w.a)} → ${endName(w.b)}`, `${wireKind(w)}, ${w.group}`],
				color: w.color
			};
		}
		case 'net': {
			const n = ix.net.get(name);
			if (!n) return null;
			const wires = ix.wiresOfNet.get(name) ?? [];
			return {
				title: `net ${n.name}`,
				lines: [`${n.pins.length} pins · ${wires.length ? wires.join(', ') : 'no wires'}`],
				color: n.color
			};
		}
		case 'issue': {
			const i = scene.issues[Number(name)];
			if (!i) return null;
			return { title: `${i.level}: ${i.code}`, lines: [i.message], color: null };
		}
		case 'hole': {
			const s = ix.strip.get(ix.stripOf.get(name) ?? '');
			const here = ix.inHole.get(name) ?? [];
			return {
				title: `hole ${name}`,
				lines: [
					`strip ${s?.label ?? '?'}${s?.nets.length ? ' · net ' + s.nets.join(' / ') : ''}`,
					here.length ? `in it: ${here.join(', ')}` : 'empty'
				],
				color: null
			};
		}
	}
	return null;
}
