import { readdir, readFile } from 'node:fs/promises';
import { spawnSync } from 'node:child_process';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const files = [];
for (const folder of ['src', 'scripts', 'tests']) {
  for (const file of await readdir(path.join(root, folder)).catch(() => [])) {
    if (file.endsWith('.mjs')) files.push(path.join(root, folder, file));
  }
}
for (const file of files) {
  const result = spawnSync(process.execPath, ['--check', file], { stdio: 'inherit' });
  if (result.status !== 0) process.exit(1);
}
for (const file of ['package.json', 'plugin.json', 'mcp.json', '.agents/plugins/marketplace.json']) {
  JSON.parse(await readFile(path.join(root, file), 'utf8'));
}
console.log(`構文とJSON形式: ${files.length}ソース、4設定ファイル 合格`);
