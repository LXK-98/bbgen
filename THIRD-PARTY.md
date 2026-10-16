# Third-party material

bbgen is MIT-licensed ([LICENSE](LICENSE)). This page lists what in it comes from elsewhere: code in this
repository taken from other projects, what the release binaries are built with, and what bbgen uses on your
system without shipping it.

## In this repository

| what | where | from | licence |
|---|---|---|---|
| Python's random numbers: seeding from an int, `random()`, `_randbelow`, `shuffle`, `choice` | [src/pyrand.rs](src/pyrand.rs) | [CPython](https://github.com/python/cpython) (`Modules/_randommodule.c`, `Lib/random.py`), whose generator is the Mersenne Twister reference code `mt19937ar.c` by Makoto Matsumoto and Takuji Nishimura | PSF-2.0; BSD-3-Clause |
| Python's `math.hypot` for two coordinates | [src/pyf.rs](src/pyf.rs) | CPython (`vector_norm` in `Modules/mathmodule.c`) | PSF-2.0 |
| The studio's UI components | [studio/src/lib/components/ui/](studio/src/lib/components/ui) | [shadcn-svelte](https://github.com/huntabyte/shadcn-svelte), added with its CLI | MIT |

bbgen began in Python, and it keeps the layouts it made then: the same file gets the same layout. So it
draws the same random numbers and does the same floating-point arithmetic as CPython. The changes to those
parts of Python, as the PSF License asks: translated to Rust, and cut down to what bbgen uses (the lines
above), with nothing else changed.

## In the release binaries

`bbgen` and `bbgen-studio` are built with Rust crates and, for the studio, JavaScript packages and a font.
Every release package has the full list, each with its version, licence, source and licence text, in
`share/bbgen/THIRD-PARTY-NOTICES.md`; [packaging/notices.py](packaging/notices.py) writes it from what the
build actually used. The ones to know about:

| component | used for | licence |
|---|---|---|
| [ELK](https://eclipse.dev/elk/), the Eclipse Layout Kernel, as ported to Rust by [elk-rs](https://github.com/openedges/elk-rs) | laying out the schematic | EPL-2.0 |
| [Tauri](https://tauri.app) and its crates | the studio's window | MIT or Apache-2.0 |
| a few crates under the studio's Tauri (`cssparser`, `selectors`, `option-ext`, ...) | | MPL-2.0 |
| [Svelte](https://svelte.dev), [SvelteKit](https://svelte.dev/docs/kit), [bits-ui](https://bits-ui.com), [Lucide](https://lucide.dev) icons, [Tailwind CSS](https://tailwindcss.com) | the studio's interface | MIT, ISC |
| [IBM Plex Sans](https://github.com/IBM/plex) (from [Fontsource](https://fontsource.org)) | the studio's font | OFL-1.1 |
| [unsafe-libyaml](https://github.com/dtolnay/unsafe-libyaml), libyaml translated to Rust | reading circuit files | MIT |

The EPL-2.0 and MPL-2.0 code is used unmodified; its source is where the notices file says (for ELK, the
elk-rs repository at the tag in [Cargo.toml](Cargo.toml)).

## Used, not included

- **KiCad's symbol and footprint libraries**, read from your KiCad install when a circuit names a KiCad part.
  They're [CC-BY-SA-4.0 with an exception](https://www.kicad.org/libraries/license/) for the designs and
  files made with them, so bbgen's drawings, checklists and schematics aren't bound by it.
- **WebKitGTK and GTK** (LGPL-2.1 or later), the system libraries the studio runs on.
- **ImageMagick, rsvg-convert and ffmpeg**, programs bbgen runs if they're installed, for PNGs and videos.

## Licence texts

### PSF License (CPython)

```
PYTHON SOFTWARE FOUNDATION LICENSE VERSION 2
--------------------------------------------

1. This LICENSE AGREEMENT is between the Python Software Foundation
("PSF"), and the Individual or Organization ("Licensee") accessing and
otherwise using this software ("Python") in source or binary form and
its associated documentation.

2. Subject to the terms and conditions of this License Agreement, PSF hereby
grants Licensee a nonexclusive, royalty-free, world-wide license to reproduce,
analyze, test, perform and/or display publicly, prepare derivative works,
distribute, and otherwise use Python alone or in any derivative version,
provided, however, that PSF's License Agreement and PSF's notice of copyright,
i.e., "Copyright (c) 2001 Python Software Foundation; All Rights Reserved"
are retained in Python alone or in any derivative version prepared by Licensee.

3. In the event Licensee prepares a derivative work that is based on
or incorporates Python or any part thereof, and wants to make
the derivative work available to others as provided herein, then
Licensee hereby agrees to include in any such work a brief summary of
the changes made to Python.

4. PSF is making Python available to Licensee on an "AS IS"
basis.  PSF MAKES NO REPRESENTATIONS OR WARRANTIES, EXPRESS OR
IMPLIED.  BY WAY OF EXAMPLE, BUT NOT LIMITATION, PSF MAKES NO AND
DISCLAIMS ANY REPRESENTATION OR WARRANTY OF MERCHANTABILITY OR FITNESS
FOR ANY PARTICULAR PURPOSE OR THAT THE USE OF PYTHON WILL NOT
INFRINGE ANY THIRD PARTY RIGHTS.

5. PSF SHALL NOT BE LIABLE TO LICENSEE OR ANY OTHER USERS OF PYTHON
FOR ANY INCIDENTAL, SPECIAL, OR CONSEQUENTIAL DAMAGES OR LOSS AS
A RESULT OF MODIFYING, DISTRIBUTING, OR OTHERWISE USING PYTHON,
OR ANY DERIVATIVE THEREOF, EVEN IF ADVISED OF THE POSSIBILITY THEREOF.

6. This License Agreement will automatically terminate upon a material
breach of its terms and conditions.

7. Nothing in this License Agreement shall be deemed to create any
relationship of agency, partnership, or joint venture between PSF and
Licensee.  This License Agreement does not grant permission to use PSF
trademarks or trade name in a trademark sense to endorse or promote
products or services of Licensee, or any third party.

8. By copying, installing or otherwise using Python, Licensee
agrees to be bound by the terms and conditions of this License
Agreement.
```

### Mersenne Twister (mt19937ar.c)

```
Copyright (C) 1997 - 2002, Makoto Matsumoto and Takuji Nishimura,
All rights reserved.

Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions
are met:

1. Redistributions of source code must retain the above copyright
notice, this list of conditions and the following disclaimer.

2. Redistributions in binary form must reproduce the above copyright
notice, this list of conditions and the following disclaimer in the
documentation and/or other materials provided with the distribution.

3. The names of its contributors may not be used to endorse or promote
products derived from this software without specific prior written
permission.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS
"AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT
LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR
A PARTICULAR PURPOSE ARE DISCLAIMED.  IN NO EVENT SHALL THE COPYRIGHT OWNER
OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL,
EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO,
PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR
PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF
LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING
NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS
SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
```

### shadcn-svelte

```
MIT License

Copyright (c) 2023 Hunter Johnston <https://github.com/huntabyte>
Copyright (c) 2023 CokaKoala <https://github.com/adriangonz97>
Copyright (c) 2023 shadcn

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```
