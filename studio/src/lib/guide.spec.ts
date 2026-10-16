// The build guide on every example and benchmark (as placed): every step can be found on the board and has its
// place to zoom to, every step on the board tags at least one hole, and every part and wire is a step. It needs
// bbgen built: `cargo build --release`.

import { spawnSync } from 'node:child_process';
import { existsSync, readdirSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import { stepTags } from './guide';
import { indexScene } from './lookup';
import { isError, type Scene } from './scene';

const bbgen = process.env.BBGEN ?? '../target/release/bbgen';
const have = existsSync(bbgen);

const files = [
	...readdirSync('../examples').map((f) => `../examples/${f}`),
	...readdirSync('../benchmarks/golden/placed').map((f) => `../benchmarks/golden/placed/${f}`)
].filter((f) => f.endsWith('.yaml'));

describe.skipIf(!have)('the build guide', () => {
	it.each(files)('%s', (file) => {
		const r = spawnSync(bbgen, ['build', file, '--json'], { encoding: 'utf8', maxBuffer: 1 << 28 });
		const json = JSON.parse(r.stdout);
		expect(isError(json)).toBe(false);
		const scene = json as Scene;
		const ix = indexScene(scene);
		const keys = scene.sections.map((s) => s.key);
		expect(new Set(scene.checklist.map((s) => s.item)).size).toBe(scene.parts.length + scene.wires.length);
		for (const step of scene.checklist) {
			expect(keys).toContain(step.section);
			const { tags, box } = stepTags(step, scene, ix);
			const part = step.item.startsWith('part:') ? ix.part.get(step.item.slice(5)) : null;
			const wire = step.item.startsWith('wire:') ? ix.wire.get(step.item.slice(5)) : null;
			if (wire && wire.points.length < 2) continue; // couldn't be placed: nothing on the board
			expect(box, step.item).not.toBeNull();
			if (part?.kind === 'offboard') expect(tags, step.item).toHaveLength(0);
			else expect(tags.length, step.item).toBeGreaterThan(0);
			if (wire) expect(tags, step.item).toHaveLength(2);
			for (const t of tags) expect(Math.abs(t.dir[0]) + Math.abs(t.dir[1]), step.item).toBe(1);
		}
	});
});
