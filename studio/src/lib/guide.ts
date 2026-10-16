// The build guide's view of a step: the holes it goes in, tagged on the board, and where on the board it is.

import type { SceneIndex } from './lookup';
import type { Rect, Scene, Step, Xy } from './scene';

/** a hole (or an off-board pin) a step goes in: where, its name, what it is (`pin 1`, `A`), and which side of it
 * the tag sits, away from the rest of the step (a unit step along one axis) */
export interface Tag {
	xy: Xy;
	label: string;
	sub: string | null;
	dir: Xy;
}

/** how far a tag reaches from its hole, in the drawing's pixels, near enough at the zoom a step is shown at */
const TAG_ROOM = 70;

const LEADS = new Set(['resistor', 'capacitor', 'electrolytic', 'diode', 'led']);

/** a pin's name when it says more than its number: `A`, `GND` */
const named = (name: string, num: number) => (name && name !== String(num) ? name : null);

/** The holes to tag for a step, and the rectangle the step covers. A two-lead part, or one with up to four pins,
 * has every leg tagged (with its name: `A`, `E`); a chip or module only pin 1; a wire both ends; an off-board part
 * none (it isn't on the board). */
export function stepTags(step: Step, scene: Scene, ix: SceneIndex): { tags: Tag[]; box: Rect | null } {
	const holeXy = new Map(ix.holes.map((h) => [h.name, h.xy]));
	const [kind, name] = [step.item.slice(0, step.item.indexOf(':')), step.item.slice(step.item.indexOf(':') + 1)];
	const at: { xy: Xy; label: string; sub: string | null }[] = [];
	const extent: Xy[] = [];
	let centre: Xy | null = null;

	if (kind === 'part') {
		const part = ix.part.get(name);
		if (!part) return { tags: [], box: null };
		const placed = part.pins.filter((p) => p.hole && p.xy);
		const all = LEADS.has(part.kind) || (part.kind !== 'offboard' && part.pins.length <= 4);
		const tagged = part.kind === 'offboard' ? [] : all ? placed : placed.filter((p) => p.num === 1);
		for (const p of tagged) at.push({ xy: p.xy!, label: p.hole!, sub: all ? named(p.name, p.num) : 'pin 1' });
		for (const p of part.pins) if (p.xy) extent.push(p.xy);
		if (part.body) {
			extent.push([part.body[0], part.body[1]], [part.body[2], part.body[3]]);
			centre = [(part.body[0] + part.body[2]) / 2, (part.body[1] + part.body[3]) / 2];
		}
	} else if (kind === 'wire') {
		const wire = ix.wire.get(name);
		if (!wire) return { tags: [], box: null };
		for (const end of [wire.a, wire.b]) {
			if (!end) continue;
			if (end.hole && holeXy.has(end.hole)) at.push({ xy: holeXy.get(end.hole)!, label: end.hole, sub: null });
			else if (end.pin) {
				const [ref, num] = end.pin.split(' ')[0].split('.');
				const pin = ix.part.get(ref)?.pins.find((p) => String(p.num) === num);
				if (pin?.xy) at.push({ xy: pin.xy, label: end.pin.split(' ')[0], sub: named(pin.name, pin.num) });
			}
		}
		extent.push(...wire.points);
	}
	extent.push(...at.map((t) => t.xy));
	if (!extent.length) return { tags: [], box: null };
	const xs = extent.map((p) => p[0]);
	const ys = extent.map((p) => p[1]);
	centre ??= [xs.reduce((a, b) => a + b, 0) / xs.length, ys.reduce((a, b) => a + b, 0) / ys.length];

	const tags = at.map((t) => {
		const [dx, dy] = [t.xy[0] - centre[0], t.xy[1] - centre[1]];
		// away from the step's middle, along the axis it's furthest along; straight up when it's the middle
		const dir: Xy = Math.abs(dx) < 1 && Math.abs(dy) < 1 ? [0, -1] : Math.abs(dx) > Math.abs(dy) ? [Math.sign(dx), 0] : [0, Math.sign(dy)];
		return { ...t, dir };
	});
	// room for the tags, which stand off their holes
	for (const t of tags) {
		xs.push(t.xy[0] + t.dir[0] * TAG_ROOM);
		ys.push(t.xy[1] + t.dir[1] * TAG_ROOM);
	}
	return { tags, box: [Math.min(...xs), Math.min(...ys), Math.max(...xs), Math.max(...ys)] };
}
