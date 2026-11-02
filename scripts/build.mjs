import { copyFile, mkdir, chmod } from 'node:fs/promises';
import path from 'node:path';
import { root, cargo, run } from './native.mjs';

const universal = process.argv.includes('--universal');
if (universal && process.platform !== 'darwin') throw new Error('Universal builds require macOS');
if (!['win32', 'darwin'].includes(process.platform)) throw new Error('配布ビルドの対象はWindowsとmacOSです');
const target = path.resolve(process.env.CARGO_TARGET_DIR || path.join(root, 'target'));
await mkdir(path.join(root, 'dist'), { recursive: true });
const filename = process.platform === 'win32' ? 'plumivox.exe' : 'plumivox';
if (universal) {
  for (const arch of ['aarch64-apple-darwin', 'x86_64-apple-darwin']) {
    run(cargo, ['build', '--locked', '--release', '--bin', 'plumivox', '--target', arch]);
  }
  run('/usr/bin/lipo', ['-create', ...['aarch64-apple-darwin', 'x86_64-apple-darwin'].map(arch => path.join(target, arch, 'release', 'plumivox')), '-output', path.join(root, 'dist', filename)]);
} else {
  run(cargo, ['build', '--locked', '--release', '--bin', 'plumivox']);
  await copyFile(path.join(target, 'release', filename), path.join(root, 'dist', filename));
}
if (process.platform === 'darwin') {
  await chmod(path.join(root, 'dist', filename), 0o755);
  // lipo後の両スライスへローカル署名を付け、arm64でも実行可能な配布物にする。
  run('/usr/bin/codesign', ['--force', '--sign', '-', path.join(root, 'dist', filename)]);
}
// 旧登録向けランチャーも同じ配布実行ファイルだけを使う。
await copyFile(path.join(root, 'scripts', 'compat-launcher.mjs'), path.join(root, 'dist', 'plumivox.mjs'));
run(process.execPath, ['scripts/licenses.mjs']);
console.log(`Native distribution: dist/${filename}${universal ? ' (universal)' : ''}`);
