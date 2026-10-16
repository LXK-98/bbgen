// Phase 1's done check (docs/studio-plan.md), on every example and benchmark (as placed, which builds without
// placing): every part, wire and net lights something on the board, every part, net and pin something in the
// schematic, and a file that can't be built says on which line. It needs bbgen built: `cargo build --release`.

import { spawnSync } from 'node:child_process';
import { existsSync, mkdtempSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterAll, describe, expect, it } from 'vitest';
import { circuitSelectors, indexScene, selectorsFor } from './lookup';
import { hasSchematic, isError, type BuildOutput, type Key, type Scene } from './scene';

const bbgen = process.env.BBGEN ?? '../target/release/bbgen';
const have = existsSync(bbgen);

function build(path: string, ...flags: string[]): BuildOutput['json'] {
	const r = spawnSync(bbgen, ['build', path, '--json', ...flags], { encoding: 'utf8', maxBuffer: 1 << 28 });
	return JSON.parse(r.stdout);
}

/** Whether a selector over data attributes (`[data-x="v"]`, or `~=` for one of a list) matches anything in an SVG. */
function matcher(svg: string): (sel: string) => boolean {
	const values = new Map<string, string[]>();
	for (const m of svg.matchAll(/ (data-[\w-]+)="([^"]*)"/g)) {
		const list = values.get(m[1]) ?? [];
		list.push(m[2]);
		values.set(m[1], list);
	}
	return (sel) => {
		const m = /^\[(data-[\w-]+)(~?)="(.*)"\]$/.exec(sel);
		if (!m) throw new Error(`a selector the test can't read: ${sel}`);
		const want = m[3].replace(/\\(.)/g, '$1');
		return (values.get(m[1]) ?? []).some((v) => (m[2] ? v.split(/\s+/).includes(want) : v === want));
	};
}

const files = [
	...readdirSync('../examples').map((f) => `../examples/${f}`),
	...readdirSync('../benchmarks/golden/placed').map((f) => `../benchmarks/golden/placed/${f}`)
].filter((f) => f.endsWith('.yaml'));

describe.skipIf(!have)('every example and benchmark', () => {
	it('has them all', () => expect(files.length).toBeGreaterThan(20));

	for (const file of files) {
		it(`lights everything in ${file.split('/').pop()}`, () => {
			const out = build(file, '--schematic');
			expect(isError(out), JSON.stringify(out).slice(0, 200)).toBe(false);
			const scene = out as Scene;
			const ix = indexScene(scene);

			const board = matcher(scene.svg);
			const keys: Key[] = [
				...scene.parts.filter((p) => p.pins.some((x) => x.xy)).map((p) => `part:${p.ref}` as Key),
				...scene.wires.filter((w) => w.points.length >= 2).map((w) => `wire:${w.id}` as Key),
				...scene.nets.map((n) => `net:${n.name}` as Key)
			];
			for (const key of keys) expect(selectorsFor(key, scene, ix).some(board), `board: ${key}`).toBe(true);

			expect(hasSchematic(scene)).toBe(true);
			if (!hasSchematic(scene)) return;
			const circuit = matcher(scene.schematic.svg);
			const inCircuit: Key[] = [
				...scene.parts.map((p) => `part:${p.ref}` as Key),
				...scene.nets.map((n) => `net:${n.name}` as Key),
				...scene.parts.flatMap((p) => p.pins.filter((x) => x.net).map((x) => `pin:${p.ref}.${x.num}` as Key))
			];
			for (const key of inCircuit) expect(circuitSelectors(key, scene, ix).some(circuit), `circuit: ${key}`).toBe(true);
		});
	}
});

describe.skipIf(!have)("a file that can't be built", () => {
	const dir = mkdtempSync(join(tmpdir(), 'bbgen-studio-test-'));
	afterAll(() => rmSync(dir, { recursive: true, force: true }));

	const broken: [string, string, number][] = [
		['YAML that does not parse', 'title: x\nparts:\n  U1: {type: dip, pins: 8\nnets: {}\n', 4],
		['a part bbgen rejects', 'title: x\nparts:\n  R1: {type: resistor, value: 1k}\n  U1: {type: dip, pins: 7}\n', 4],
		['a wire bbgen rejects', 'title: x\nparts: {}\nwires:\n  power:\n    - [a1, a2]\n    - [a1, a3, mauve]\n', 6]
	];
	for (const [what, text, line] of broken) {
		it(`says on which line: ${what}`, () => {
			const file = join(dir, `${what.replace(/\W+/g, '-')}.yaml`);
			writeFileSync(file, text);
			const out = build(file);
			expect(isError(out)).toBe(true);
			if (isError(out)) expect(out.error.line, out.error.message).toBe(line);
		});
	}
});
