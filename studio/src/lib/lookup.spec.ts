// The lookups, on the scene bbgen builds for examples/eeprom.yaml (it needs bbgen built: `cargo build --release`).

import { execFileSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import { circuitSelectors, holeAt, indexScene, pinHole, selectorsFor, stripId } from './lookup';
import { hasSchematic, type Key, type Scene } from './scene';

const bbgen = process.env.BBGEN ?? '../target/release/bbgen';
const have = existsSync(bbgen);
const scene: Scene = have ? JSON.parse(execFileSync(bbgen, ['build', '../examples/eeprom.yaml', '--json', '--schematic'], { encoding: 'utf8' })) : (null as never);

describe.skipIf(!have)('lookups on eeprom.yaml', () => {
	const ix = indexScene(scene);

	it('names strips', () => {
		expect(stripId('e33', scene)).toEqual({ id: '1:top:33', label: 'a–e 33' });
		expect(stripId('f30', scene)).toEqual({ id: '1:bot:30', label: 'f–j 30' });
		expect(stripId('top+7', scene)?.label).toBe('top+ rail');
		expect(stripId('nonsense', scene)).toBeNull();
	});

	it('knows what is in a strip', () => {
		// U1.5 SDA sits in e33; W6 (from the Pico's GP16) ends in b33, the same strip
		const s = ix.strip.get(ix.stripOf.get('e33')!)!;
		expect(s.pins.map((p) => p.pin)).toContain('U1.5 SDA');
		expect(s.wires).toContain('W6');
		expect(s.nets).toEqual(['SDA']);
		expect(s.holes).toHaveLength(5);
	});

	it("finds a part's wires through its pins' strips", () => {
		expect(ix.wiresOfPart.get('U1')).toEqual(['W4', 'W5', 'W6', 'W7', 'W8', 'W9']);
		expect(ix.wiresOfPart.get('J1')).toEqual(['W8', 'W9', 'W10']);
	});

	it('lights a net, its parts and its legend entry', () => {
		const q = selectorsFor('net:SDA', scene, ix);
		expect(q[0]).toBe('[data-net~="SDA"]');
		expect(q.slice(1).sort()).toEqual(['[data-part="J1"]', '[data-part="Pico"]', '[data-part="U1"]']);
		for (const sel of q.slice(1)) expect(scene.svg).toContain(sel.slice(1, -1));
	});

	it('finds the hole under a point, within half a pitch', () => {
		const [x, y] = scene.boards[0].holes['f30'];
		expect(holeAt(ix, x + 4, y - 4)).toBe('f30');
		expect(holeAt(ix, -100, -100)).toBeNull();
	});

	it("finds a pin's hole", () => {
		expect(pinHole('U1.5', ix)).toBe('e33');
		expect(selectorsFor('pin:U1.5', scene, ix)).toContain('[data-wire="W6"]');
	});

	it('lights every part, net and pin in the schematic', () => {
		expect(hasSchematic(scene)).toBe(true);
		if (!hasSchematic(scene)) return;
		const svg = scene.schematic.svg;
		// a selector over data attributes, matched against the SVG's text: [data-x="v"] → data-x="v"
		const found = (sel: string) => svg.includes(sel.slice(1, -1));
		const keys: Key[] = [
			...scene.parts.map((p) => `part:${p.ref}` as Key),
			...scene.nets.map((n) => `net:${n.name}` as Key),
			...scene.parts.flatMap((p) => p.pins.filter((x) => x.net).map((x) => `pin:${p.ref}.${x.num}` as Key))
		];
		for (const key of keys) {
			const q = circuitSelectors(key, scene, ix);
			expect(q.some(found), key).toBe(true);
		}
		// a wire on the board lights its net in the schematic
		expect(circuitSelectors('wire:W6', scene, ix)[0]).toBe('[data-net="SDA"]');
	});
});
