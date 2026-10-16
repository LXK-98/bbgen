/// <reference types="node" />
// The studio's engine for `pnpm dev` in a browser: the same three things the Tauri app does (build a circuit,
// list the repo's examples, say when a file changes), as HTTP endpoints on the dev server. Only the dev server
// has them; the built app talks to Tauri instead (src-tauri/src/main.rs).

import { spawn } from 'node:child_process';
import { existsSync, readdirSync, watch } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import type { Plugin } from 'vite';

const repo = resolve(dirname(fileURLToPath(import.meta.url)), '../..');

/** bbgen: $BBGEN, else the repo's launcher (it builds bbgen first when the sources changed). */
function bbgen(): string {
	return process.env.BBGEN || join(repo, 'bin/bbgen');
}

function isCircuit(path: string): boolean {
	return /\.ya?ml$/i.test(path) && existsSync(path);
}

function run(path: string, schematic: boolean, rails: boolean): Promise<object> {
	const started = Date.now();
	const args = ['build', path, '--json', ...(schematic ? [rails ? '--rails' : '--schematic'] : [])];
	return new Promise((done) => {
		const child = spawn(bbgen(), args);
		let out = '';
		let err = '';
		child.stdout.on('data', (d) => (out += d));
		child.stderr.on('data', (d) => (err += d));
		child.on('error', (e) => done({ json: null, stderr: String(e), code: null, command: '', ms: 0 }));
		child.on('close', (code) => {
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

export function examples(): string[] {
	const out: string[] = [];
	for (const dir of ['examples', 'benchmarks']) {
		const d = join(repo, dir);
		if (!existsSync(d)) continue;
		for (const f of readdirSync(d).sort()) if (/\.ya?ml$/.test(f)) out.push(join(d, f));
	}
	return out;
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
					return json(200, await run(path, url.searchParams.get('schematic') === '1', url.searchParams.get('rails') === '1'));
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
