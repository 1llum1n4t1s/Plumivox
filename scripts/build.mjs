import { build } from 'esbuild';
import { fileURLToPath } from 'node:url';
import path from 'node:path';

const root = fileURLToPath(new URL('../', import.meta.url));

// 配布先では node_modules を参照せず、Node の組み込みモジュールだけを利用する。
await build({
  absWorkingDir: root,
  entryPoints: ['src/cli.mjs'],
  outfile: path.join(root, 'dist/plumivox.mjs'),
  bundle: true,
  platform: 'node',
  target: 'node22',
  format: 'esm',
  banner: { js: "import { createRequire as __plumivoxCreateRequire } from 'node:module';\nconst require = __plumivoxCreateRequire(import.meta.url);" },
  legalComments: 'eof',
  logLevel: 'info',
});
