# elkjs

The [Eclipse Layout Kernel](https://eclipse.dev/elk/) compiled to JavaScript, as published on npm. bbgen's
circuit view lays out its symbols and routes its wires with ELK's layered algorithm, run in QuickJS
([src/elk.rs](../../src/elk.rs)); [build.rs](../../build.rs) compiles this file to QuickJS bytecode once, at build
time.

| | |
|---|---|
| package | [elkjs 0.12.0](https://www.npmjs.com/package/elkjs/v/0.12.0) (17 July 2026), `lib/elk.bundled.js`, unmodified |
| source | https://github.com/kieler/elkjs, https://github.com/eclipse/elk |
| tarball | https://registry.npmjs.org/elkjs/-/elkjs-0.12.0.tgz |
| sha256 of `elk.bundled.js` | `1222e44f953ce7746af23801e723708f8e6f436b8b377a6a5fc7552f34a307b3` |
| licence | EPL-2.0 OR GPL-3.0-or-later; bbgen uses it under the Eclipse Public License 2.0 ([LICENSE.md](LICENSE.md)) |

`tests/elk.rs` checks the checksum, and that layouts come out as elkjs gives them in Node.

To update: fetch the new release (`npm pack elkjs@VERSION`), replace `elk.bundled.js` and `LICENSE.md`, update this
table, record Node's layouts of `tests/elk/*.in.json` again (`tests/elk/README.md`), and look at what moved in the
circuit views.
