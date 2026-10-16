/// <reference types="node" />
// The studio's engine for `pnpm dev` in a browser: the same things the Tauri app does (build a circuit, list the
// repo's examples, say when a file changes, open it in an editor), as HTTP endpoints on the dev server. Only the dev server
// has them; the built app talks to Tauri instead (src-tauri/src/lib.rs).

import { execFile, spawn, type ChildProcess } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, watch, writeFileSync } from 'node:fs';
import { homedir, tmpdir } from 'node:os';
import { basename, delimiter, dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import type { Plugin } from 'vite';

const repo = resolve(dirname(fileURLToPath(import.meta.url)), '../..');

/** the bbgen picked in the studio, if one was */
let chosen: string | null = null;

/** bbgen, and how it was found: the one picked in the studio, else $BBGEN, else the repo's launcher (it builds bbgen
 * first when the sources changed). */
function findBbgen(pick: string | null = chosen): { path: string; how: string } {
	if (pick) return { path: pick, how: 'picked in the studio' };
	if (process.env.BBGEN) return { path: process.env.BBGEN, how: '$BBGEN' };
	return { path: join(repo, 'bin/bbgen'), how: "this checkout's bin/bbgen" };
}

const bbgen = () => findBbgen().path;

/** run `pick` as bbgen from now on (null: find it as at the start), if it is one: which, how found, its version */
function useBbgen(pick: string | null): Promise<{ path: string; how: string; version: string | null; error: string | null }> {
	const found = findBbgen(pick);
	return new Promise((done, fail) =>
		execFile(found.path, ['--version'], (e, out) => {
			const text = String(out ?? '').trim();
			const error = !e && text.startsWith('bbgen ') ? null : e ? `can't run ${found.path}: ${e.message}` : `${found.path} doesn't answer --version as bbgen does`;
			if (pick && error) return fail(new Error(error));
			chosen = pick;
			done({ ...found, version: error ? null : text, error });
		})
	);
}

function isCircuit(path: string): boolean {
	return /\.ya?ml$/i.test(path) && existsSync(path);
}

/** the build running now: a newer one stops it, as does /api/stop */
let running: ChildProcess | null = null;

/** `bbgen build`, as src-tauri/src/lib.rs runs it; with `onStep`, `--events`, and each step's line to it as it's printed */
function run(path: string, schematic: boolean, wires: boolean, onStep?: (line: string) => void): Promise<object> {
	const started = Date.now();
	const args = ['build', path, onStep ? '--events' : '--json', ...(schematic ? [wires ? '--wires' : '--schematic'] : [])];
	return new Promise((done) => {
		const child = spawn(bbgen(), args);
		running?.kill();
		running = child;
		let out = '';
		let err = '';
		let partial = '';
		child.stdout.on('data', (d) => {
			// a step's line starts with its "event"; the scene (or the error) is the rest
			const lines = (partial + d).split('\n');
			partial = lines.pop()!;
			for (const line of lines) {
				if (onStep && line.startsWith('{"event"')) onStep(line);
				else out += line + '\n';
			}
		});
		child.stderr.on('data', (d) => (err += d));
		child.on('error', (e) => done({ json: null, stderr: String(e), code: null, command: '', ms: 0 }));
		child.on('close', (code) => {
			if (running === child) running = null;
			out += partial;
			let json = null;
			try {
				json = out.trim() ? JSON.parse(out) : null;
			} catch {
				err += '\n(bbgen printed something that is not JSON)';
			}
			done({ json, stderr: err, code, command: `bbgen ${args.join(' ')}`, ms: Date.now() - started });
		});
	});
}

// Opening a file at a line, as src-tauri/src/lib.rs does: $VISUAL or $EDITOR (a terminal editor in a terminal
// window of its own), else the system's opener.
const IN_TERMINAL = ['vi', 'vim', 'nvim', 'nano', 'micro', 'hx', 'helix', 'kak', 'ne', 'joe', 'mg'];
const TERMINALS: [string, string[]][] = [
	['ptyxis', ['--']], ['gnome-terminal', ['--']], ['konsole', ['-e']], ['kitty', []], ['alacritty', ['-e']],
	['foot', []], ['wezterm', ['start', '--']], ['xterm', ['-e']]
];

const onPath = (name: string) => (process.env.PATH ?? '').split(delimiter).some((d) => d && existsSync(join(d, name)));

function atLine(editor: string, path: string, line: number | null): string[] {
	if (!line) return [path];
	if (['code', 'code-insiders', 'codium', 'cursor', 'windsurf'].includes(editor)) return ['--goto', `${path}:${line}`];
	if (['zed', 'zeditor', 'subl', 'hx', 'helix', 'micro'].includes(editor)) return [`${path}:${line}`];
	if (editor === 'kate') return ['--line', String(line), path];
	return [`+${line}`, path];
}

function editor(path: string, line: number | null): string[] {
	const opener = [process.platform === 'darwin' ? 'open' : process.platform === 'win32' ? 'explorer' : 'xdg-open', path];
	const set = [process.env.VISUAL, process.env.EDITOR].find((v) => v?.trim());
	if (!set) return opener;
	const words = set.trim().split(/\s+/);
	const name = basename(words[0]).replace(/\.exe$/i, '');
	words.push(...atLine(name, path, line));
	if (!IN_TERMINAL.includes(name)) return words;
	const terminal: [string, string[]] | undefined = process.env.TERMINAL ? [process.env.TERMINAL, ['-e']] : TERMINALS.find(([t]) => onPath(t));
	if (!terminal || process.platform === 'darwin' || process.platform === 'win32') return opener;
	return [terminal[0], ...terminal[1], ...words];
}

/** bbgen's files for the export menu, as src-tauri's `export` makes them: built into a folder of its own, then read */
function exportFiles(path: string, what: string, wires: boolean): Promise<{ name: string; text: string }[]> {
	const stem = basename(path).replace(/\.ya?ml$/i, '');
	const exts: Record<string, string[]> = { board: ['svg'], bare: ['svg'], checklist: ['md'], schematic: ['schematic.svg'], all: ['svg', 'md', 'schematic.svg'] };
	if (!exts[what]) return Promise.reject(new Error(`nothing to export called ${what}`));
	const tmp = mkdtempSync(join(tmpdir(), 'bbgen-studio-'));
	const args = ['build', path, '-o', tmp, '--no-png'];
	if (what === 'bare') args.push('--bare');
	if (what === 'schematic' || what === 'all') args.push(wires ? '--wires' : '--schematic');
	return new Promise<{ name: string; text: string }[]>((done, fail) => {
		const child = spawn(bbgen(), args);
		let err = '';
		child.stderr.on('data', (d) => (err += d));
		child.on('error', fail);
		child.on('close', (code) => {
			// 1 is a board the check found errors on: still a board to export
			if (code !== 0 && code !== 1) return fail(new Error(err.trim().replace(/^bbgen: /, '')));
			try {
				done(exts[what].map((e) => ({ name: `${stem}.${e}`, text: readFileSync(join(tmp, `${stem}.${e}`), 'utf8') })));
			} catch (e) {
				fail(e);
			}
		});
	}).finally(() => rmSync(tmp, { recursive: true, force: true }));
}

export function examples(): string[] {
	const out: string[] = [];
	for (const dir of ['examples', 'benchmarks']) {
		const d = join(repo, dir);
		if (!existsSync(d)) continue;
		for (const f of readdirSync(d).sort()) if (/\.ya?ml$/.test(f)) out.push(join(d, f));
	}
	return out;
}

/** where the build guide keeps its progress: the file the desktop app uses (Tauri's app data folder for
 * com.bbgen.studio), so the browser and the app share it */
function progressFile(): string {
	const id = 'com.bbgen.studio';
	const base =
		process.platform === 'darwin'
			? join(homedir(), 'Library/Application Support')
			: process.platform === 'win32'
				? (process.env.APPDATA ?? join(homedir(), 'AppData/Roaming'))
				: (process.env.XDG_DATA_HOME ?? join(homedir(), '.local/share'));
	return join(base, id, 'guide-progress.json');
}

function readProgress(): Record<string, { done: string[] }> {
	try {
		return JSON.parse(readFileSync(progressFile(), 'utf8'));
	} catch {
		return {};
	}
}

function saveProgress(path: string, done: string[]) {
	const all = readProgress();
	if (done.length) all[path] = { done };
	else delete all[path];
	mkdirSync(dirname(progressFile()), { recursive: true });
	writeFileSync(progressFile(), JSON.stringify(all, null, 2));
}

/** a request's body, as JSON */
function body(req: import('node:http').IncomingMessage): Promise<unknown> {
	return new Promise((done, fail) => {
		let text = '';
		req.on('data', (d) => (text += d));
		req.on('end', () => {
			try {
				done(JSON.parse(text));
			} catch (e) {
				fail(e);
			}
		});
	});
}

export function bbgenDev(): Plugin {
	return {
		name: 'bbgen-dev',
		apply: 'serve',
		configureServer(server) {
			server.middlewares.use(async (req, res, next) => {
				const url = new URL(req.url ?? '/', 'http://localhost');
				const path = url.searchParams.get('path') ?? '';
				const json = (status: number, body: unknown) => {
					res.statusCode = status;
					res.setHeader('content-type', 'application/json');
					res.end(JSON.stringify(body));
				};
				if (url.pathname === '/api/examples') return json(200, examples());
				if (url.pathname === '/api/build') {
					if (!isCircuit(path)) return json(400, { error: `not a circuit file: ${path}` });
					const [schematic, wires] = [url.searchParams.get('schematic') === '1', url.searchParams.get('wires') === '1'];
					if (url.searchParams.get('events') !== '1') return json(200, await run(path, schematic, wires));
					// --events: newline-delimited JSON, each step's line as bbgen prints it, then {"done": what json() sends}
					res.setHeader('content-type', 'application/x-ndjson');
					res.setHeader('cache-control', 'no-cache');
					const out = await run(path, schematic, wires, (line) => res.write(line + '\n'));
					res.end(JSON.stringify({ done: out }) + '\n');
					return;
				}
				if (url.pathname === '/api/progress') {
					if (req.method !== 'POST') return json(200, readProgress()[path]?.done ?? []);
					return body(req).then(
						(b) => {
							saveProgress(path, (b as { done: string[] }).done);
							json(200, {});
						},
						(e) => json(400, { error: String(e) })
					);
				}
				if (url.pathname === '/api/stop') {
					running?.kill();
					return json(200, {});
				}
				if (url.pathname === '/api/bbgen') {
					return useBbgen(url.searchParams.get('path')?.trim() || null).then(
						(info) => json(200, info),
						(e) => json(400, { error: e.message })
					);
				}
				if (url.pathname === '/api/export') {
					if (!isCircuit(path)) return json(400, { error: `not a circuit file: ${path}` });
					return exportFiles(path, url.searchParams.get('what') ?? '', url.searchParams.get('wires') === '1').then(
						(files) => json(200, { files }),
						(e) => json(500, { error: String(e.message ?? e) })
					);
				}
				if (url.pathname === '/api/edit') {
					if (!isCircuit(path)) return json(400, { error: `not a circuit file: ${path}` });
					const [cmd, ...args] = editor(path, Number(url.searchParams.get('line')) || null);
					const child = spawn(cmd, args, { detached: true, stdio: 'ignore' });
					child.on('error', (e) => json(500, { error: `can't run ${cmd}: ${e.message}` }));
					child.on('spawn', () => {
						child.unref();
						json(200, {});
					});
					return;
				}
				if (url.pathname === '/api/watch') {
					if (!isCircuit(path)) return json(400, { error: `not a circuit file: ${path}` });
					res.setHeader('content-type', 'text/event-stream');
					res.setHeader('cache-control', 'no-cache');
					res.write(': watching\n\n');
					// watch the folder: editors save by writing a new file and renaming it over the old one
					let timer: ReturnType<typeof setTimeout> | undefined;
					const name = path.split('/').pop();
					const watcher = watch(dirname(path), (_, file) => {
						if (file !== name) return;
						clearTimeout(timer);
						timer = setTimeout(() => res.write('data: changed\n\n'), 150);
					});
					req.on('close', () => {
						clearTimeout(timer);
						watcher.close();
					});
					return;
				}
				next();
			});
		}
	};
}
