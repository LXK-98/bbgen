// The dev server's endpoints answer the studio's own page only. Any page open in the browser can send requests to
// localhost: it can't read the answers, but the requests would do their work (run a program, open an editor).

import type { IncomingMessage, ServerResponse } from 'node:http';
import { describe, expect, it } from 'vitest';
import { bbgenDev } from './bbgen-dev';

type Middleware = (req: IncomingMessage, res: ServerResponse, next: () => void) => unknown;

/** the plugin's middleware, as Vite installs it */
function middleware(): Middleware {
	let installed: Middleware | null = null;
	const server = { middlewares: { use: (m: Middleware) => (installed = m) } };
	(bbgenDev().configureServer as (server: unknown) => void)(server);
	return installed!;
}

/** a request to the dev server on localhost:5173: the status it answers, or 'vite' when it leaves it to Vite */
function ask(url: string, headers: Record<string, string> = {}, method = 'GET'): Promise<number | 'vite'> {
	const req = { url, method, headers: { host: 'localhost:5173', ...headers } } as unknown as IncomingMessage;
	return new Promise((done) => {
		const res = { statusCode: 200, setHeader() {}, end: () => done(res.statusCode) };
		middleware()(req, res as unknown as ServerResponse, () => done('vite'));
	});
}

describe('the dev server', () => {
	it("answers the studio's own page", async () => {
		expect(await ask('/api/examples', { 'sec-fetch-site': 'same-origin' })).toBe(200);
		// typed in the address bar
		expect(await ask('/api/examples', { 'sec-fetch-site': 'none' })).toBe(200);
		// a browser that doesn't send Sec-Fetch-Site
		expect(await ask('/api/examples', { origin: 'http://localhost:5173' })).toBe(200);
		// not a browser: curl
		expect(await ask('/api/examples')).toBe(200);
	});

	it('refuses pages on other sites', async () => {
		for (const site of ['cross-site', 'same-site']) {
			expect(await ask('/api/bbgen?path=/usr/bin/true', { 'sec-fetch-site': site })).toBe(403);
			expect(await ask('/api/edit?path=/tmp/x.yaml', { 'sec-fetch-site': site })).toBe(403);
			expect(await ask('/api/progress?path=x', { 'sec-fetch-site': site, origin: 'https://evil.example' }, 'POST')).toBe(403);
		}
		// a browser that doesn't send Sec-Fetch-Site still sends the Origin of a POST
		expect(await ask('/api/progress?path=x', { origin: 'https://evil.example' }, 'POST')).toBe(403);
	});

	it('leaves the page itself to Vite', async () => {
		expect(await ask('/', { 'sec-fetch-site': 'cross-site' })).toBe('vite');
	});
});
