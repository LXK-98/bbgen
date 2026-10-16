// Lays out each tests/elk/*.in.json with the vendored elkjs in Node, into *.out.json: what bbgen's ELK (in QuickJS)
// must give. Run from the repo: node tests/elk/record.mjs
import fs from 'node:fs';
import path from 'node:path';
import { createRequire } from 'node:module';

const here = path.dirname(new URL(import.meta.url).pathname);
const ELK = createRequire(import.meta.url)('../../vendor/elkjs/elk.bundled.js');
for (const name of fs.readdirSync(here).filter((f) => f.endsWith('.in.json')).sort()) {
	const graph = JSON.parse(fs.readFileSync(path.join(here, name), 'utf8'));
	const out = await new ELK().layout(graph);
	// without $H, the object ids ELK's compiled Java adds (bbgen leaves them out too)
	const json = JSON.stringify(out, (k, v) => (k === '$H' ? undefined : v));
	fs.writeFileSync(path.join(here, name.replace('.in.json', '.out.json')), json + '\n');
	console.log(name);
}
