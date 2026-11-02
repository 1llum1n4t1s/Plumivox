import { spawnSync } from 'node:child_process';
import { readdir, readFile, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { cargo, root } from './native.mjs';

// 配布バイナリへ静的に取り込む依存のライセンスを同梱する。
const result = spawnSync(cargo, ['metadata', '--locked', '--format-version', '1'], { cwd: root, encoding: 'utf8', maxBuffer: 32 * 1024 * 1024, windowsHide: true });
if (result.error || result.status !== 0) throw new Error('Cargo metadata unavailable');
const { packages } = JSON.parse(result.stdout);
const sections = ['Plumivox native dependency licenses\nGenerated from Cargo.lock.'];
for (const pkg of packages.filter(pkg => pkg.name !== 'plumivox').sort((a, b) => a.name.localeCompare(b.name))) {
  const directory = path.dirname(pkg.manifest_path);
  const names = (await readdir(directory)).filter(name => /^(LICENSE|LICENCE|COPYING|NOTICE)([._-]|$)/i.test(name));
  if (pkg.license_file && !names.includes(pkg.license_file)) names.push(pkg.license_file);
  const texts = [];
  for (const name of names) {
    try { texts.push(`${name}\n${await readFile(path.join(directory, name), 'utf8')}`); }
    catch (error) { if (error.code !== 'EISDIR') throw error; }
  }
  sections.push(`${pkg.name} ${pkg.version} (${pkg.license || 'see license file'})\n${texts.join('\n\n')}`);
}
// Unicode 17の絵文字判定データは既存emoji-regexパッケージから移植した。
sections.push(`emoji-regex 11.0.0 (MIT)\n${await readFile(path.join(root, 'node_modules', 'emoji-regex', 'LICENSE-MIT.txt'), 'utf8')}`);
await writeFile(path.join(root, 'dist', 'THIRD-PARTY-LICENSES.txt'), sections.join('\n\n============================================================\n\n').replace(/[ \t]+(?=\r?$)/gm, ''));
