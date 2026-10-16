// What `bbgen build circuit.yaml --json` prints (docs/scene-format.md in the bbgen repo).

export const SCHEMA = 1;

export type Xy = [number, number];
export type Rect = [number, number, number, number];

export interface Pin {
	num: number;
	name: string;
	hole: string | null;
	xy: Xy | null;
	net: string | null;
}

export interface Part {
	ref: string;
	kind: string;
	title: string;
	value: string;
	color: string | null;
	source: string;
	adapter: string;
	at: string | null;
	rot: number;
	raised: boolean;
	placed_by: 'you' | 'bbgen' | null;
	legs: string[];
	slant: string | null;
	keepouts: string[];
	body: Rect | null;
	pins: Pin[];
	text: string;
}

export interface End {
	hole: string | null;
	pin: string | null;
	spec: string;
}

export interface Wire {
	id: string;
	num: number;
	group: string;
	nets: string[];
	color: string;
	a: End | null;
	b: End | null;
	spec: [string, string];
	via: string[];
	points: Xy[];
	flex: boolean;
	flying: boolean;
	laid_by: 'you' | 'bbgen';
	slant: string | null;
	under: string[];
	text: string;
}

export interface Net {
	name: string;
	color: string;
	pins: string[];
	rails: string[];
	polarity: '+' | '-' | null;
	guessed: boolean;
}

export interface Issue {
	level: 'error' | 'warning';
	code: string;
	message: string;
	parts: string[];
	pins: string[];
	holes: string[];
}

export interface Board {
	id: string;
	kind: string;
	columns: number;
	split_rails: boolean;
	rect: Rect;
	rails: Record<string, string[]>;
	holes: Record<string, Xy>;
}

export interface Step {
	section: string;
	item: string;
	text: string;
}

export interface Scene {
	schema: number;
	bbgen: string;
	source: string;
	title: string;
	notes: string[];
	groups: string[];
	boards: Board[];
	parts: Part[];
	wires: Wire[];
	nets: Net[];
	issues: Issue[];
	checklist: Step[];
	stats: { errors: number; warnings: number; parts: number; wires: number; nets: number; ms: number };
	width: number;
	height: number;
	svg: string;
}

export interface SceneError {
	schema: number;
	bbgen: string;
	error: { message: string; line: number | null; column: number | null };
}

/** One run of bbgen, as the engine reports it. */
export interface BuildOutput {
	/** what bbgen printed on stdout: a scene or an error */
	json: Scene | SceneError | null;
	stderr: string;
	code: number | null;
	command: string;
	ms: number;
}

export function isError(x: Scene | SceneError | null): x is SceneError {
	return !!x && 'error' in x;
}

/** What hovering or selecting can point at. */
export type Key = `part:${string}` | `wire:${string}` | `net:${string}` | `issue:${number}` | `hole:${string}`;

export function keyKind(key: Key | null): 'part' | 'wire' | 'net' | 'issue' | 'hole' | null {
	return key ? (key.slice(0, key.indexOf(':')) as 'part' | 'wire' | 'net' | 'issue' | 'hole') : null;
}

export function keyName(key: Key): string {
	return key.slice(key.indexOf(':') + 1);
}

/** `U1.5 SDA` → `U1` */
export function pinPart(pin: string): string {
	return pin.split('.')[0];
}

/** `checklist` text has `holes` in backticks; split it for display. */
export function codeSpans(text: string): { code: boolean; text: string }[] {
	return text.split('`').map((t, i) => ({ code: i % 2 === 1, text: t }));
}
