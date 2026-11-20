# Third-party scripts and fonts

Files here are copied unchanged from their published packages and served
from `/static/vendor/`. The i18n scan skips this folder.

| File | Package | Licence |
| --- | --- | --- |
| `abcjs-basic-min.js` | abcjs 6.7.1 (`dist/abcjs-basic-min.js` on npm), draws ABC music notation as SVG | MIT, `abcjs-LICENSE.md` |
| `jsxgraphcore.js` | JSXGraph 1.14.2 (`distrib/jsxgraphcore.js` on npm), interactive geometry for the maths question types (`static/measure.js`) | MIT or LGPL 3, `jsxgraph-LICENSE.MIT` |
| `atkinson-hyperlegible-{400,700}-{latin,latin-ext}.woff2` | Atkinson Hyperlegible (Braille Institute), regular and bold, as Google Fonts serves it (v12), split into Latin and Latin Extended; the Letters reading setting (`static/reading.css`, #233) | SIL Open Font Licence 1.1, `atkinson-hyperlegible-OFL.txt` |

To update abcjs: download the package tarball from npm
(`https://registry.npmjs.org/abcjs/-/abcjs-<version>.tgz`), copy
`dist/abcjs-basic-min.js` and `LICENSE.md` here, and change the version above.

To update JSXGraph: download `https://registry.npmjs.org/jsxgraph/-/jsxgraph-<version>.tgz`,
copy `distrib/jsxgraphcore.js` and `LICENSE.MIT` (as `jsxgraph-LICENSE.MIT`) here,
and change the version above. Pages load it only when they draw something with it.

To update Atkinson Hyperlegible: ask `https://fonts.googleapis.com/css2?family=Atkinson+Hyperlegible:wght@400;700`
with a current browser's User-Agent, download the four `woff2` files it names
under the names above, copy `ofl/atkinsonhyperlegible/OFL.txt` from
github.com/google/fonts as `atkinson-hyperlegible-OFL.txt`, and keep the
`unicode-range`s in `static/reading.css` in step with that CSS.
