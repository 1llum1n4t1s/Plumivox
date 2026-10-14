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
  minifyWhitespace: true,
  banner: { js: "import { createRequire as __plumivoxCreateRequire } from 'node:module';\nimport { fileURLToPath as __plumivoxFileURLToPath } from 'node:url';\nimport { dirname as __plumivoxDirname } from 'node:path';\nconst require = __plumivoxCreateRequire(import.meta.url);\nconst __filename = __plumivoxFileURLToPath(import.meta.url);\nconst __dirname = __plumivoxDirname(__filename);" },
  // ssh2の任意native高速化を使わず、配布物にJavaScript実装だけを同梱する。
  plugins: [{
    name: 'ssh-javascript-only',
    setup(builder) {
      builder.onResolve({ filter: /^cpu-features$|sshcrypto\.node$/ }, args => ({ path: args.path, namespace: 'ssh-javascript-only' }));
      builder.onLoad({ filter: /.*/, namespace: 'ssh-javascript-only' }, () => ({
        contents: "throw new Error('Optional native acceleration is disabled.');", loader: 'js',
      }));
    },
  }],
  legalComments: 'eof',
  logLevel: 'info',
});
