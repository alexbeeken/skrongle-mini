# Third-party scripts

Files here are copied unchanged from their published packages and served
from `/static/vendor/`. The i18n scan skips this folder.

| File | Package | Licence |
| --- | --- | --- |
| `abcjs-basic-min.js` | abcjs 6.7.1 (`dist/abcjs-basic-min.js` on npm), draws ABC music notation as SVG | MIT, `abcjs-LICENSE.md` |
| `jsxgraphcore.js` | JSXGraph 1.14.2 (`distrib/jsxgraphcore.js` on npm), interactive geometry for the maths question types (`static/measure.js`) | MIT or LGPL 3, `jsxgraph-LICENSE.MIT` |

To update abcjs: download the package tarball from npm
(`https://registry.npmjs.org/abcjs/-/abcjs-<version>.tgz`), copy
`dist/abcjs-basic-min.js` and `LICENSE.md` here, and change the version above.

To update JSXGraph: download `https://registry.npmjs.org/jsxgraph/-/jsxgraph-<version>.tgz`,
copy `distrib/jsxgraphcore.js` and `LICENSE.MIT` (as `jsxgraph-LICENSE.MIT`) here,
and change the version above. Pages load it only when they draw something with it.
