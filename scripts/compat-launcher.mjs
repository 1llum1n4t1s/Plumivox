#!/usr/bin/env node
import { spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';

// 既存の node dist/plumivox.mjs 登録用。取得・ビルド・秘密の受渡しは行わない。
if (!['win32', 'darwin'].includes(process.platform)) {
  console.error('Plumivoxの配布実行ファイルはWindows/macOS用です。');
  process.exitCode = 1;
} else {
  const binary = fileURLToPath(new URL(process.platform === 'win32' ? './plumivox.exe' : './plumivox', import.meta.url));
  const child = spawn(binary, process.argv.slice(2), { stdio: 'inherit', windowsHide: true });
  child.once('error', () => { console.error('Plumivoxの実行ファイルを起動できません。配布物を確認してください。'); process.exitCode = 1; });
  child.once('exit', (code) => { process.exitCode = code ?? 1; });
  for (const signal of ['SIGINT', 'SIGTERM']) process.once(signal, () => child.kill(signal));
}
