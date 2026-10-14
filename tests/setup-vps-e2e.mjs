// 実インストーラーを別プロセスで起動するCLI E2E。root権限を偽装しない。
import { spawnSync } from 'node:child_process';
import { mkdir, writeFile, readFile, realpath, access } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { randomUUID } from 'node:crypto';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const output = path.resolve(process.env.PLUMIVOX_SETUP_VPS_E2E_OUTPUT_DIR ?? path.join(os.tmpdir(), 'Codex', 'Plumivox', randomUUID()));
const within = (target, directory) => {
  const relative = path.relative(directory, target);
  return relative === '' || (!relative.startsWith(`..${path.sep}`) && relative !== '..' && !path.isAbsolute(relative));
};
// 既存祖先の実体を確認し、リンク経由の禁止先も拒否する。
let ancestor = output;
const suffix = [];
while (true) {
  try { ancestor = await realpath(ancestor); break; }
  catch (error) {
    if (error.code !== 'ENOENT') throw error;
    suffix.unshift(path.basename(ancestor));
    const parent = path.dirname(ancestor);
    if (parent === ancestor) throw error;
    ancestor = parent;
  }
}
const resolvedOutput = path.join(ancestor, ...suffix);
for (const forbidden of [await realpath(root), path.join(os.homedir(), '.codex'), process.env.CODEX_HOME].filter(Boolean)) {
  if (within(resolvedOutput, path.resolve(forbidden))) throw new Error('E2E出力先はリポジトリ/Codexユーザーディレクトリの外にしてください');
}
await mkdir(resolvedOutput, { recursive: true });
const bash = process.env.PLUMIVOX_E2E_BASH ?? (process.platform === 'win32' ? 'C:/Program Files/Git/bin/bash.exe' : 'bash');
const installer = path.join(root, 'setup', 'vps', 'install.sh');
const posixPath = value => process.platform === 'win32' ? value.replaceAll('\\', '/').replace(/^([a-z]):/i, (_, drive) => `/${drive.toLowerCase()}`) : value;
const traceFile = path.join(resolvedOutput, 'mutation-trace.txt');
const fixtureKey = path.join(resolvedOutput, 'public-key.pub');
await writeFile(fixtureKey, 'ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA\n');
const originalKey = await readFile(fixtureKey, 'utf8');
const fakeBin = path.join(resolvedOutput, 'fake-bin');
await mkdir(fakeBin);
// rootなしで停止するはずの経路から変更コマンドへ到達したら必ず記録・拒否する。
for (const command of ['apt-get', 'systemctl', 'ufw', 'install', 'useradd', 'chmod', 'rm', 'htpasswd']) {
  await writeFile(path.join(fakeBin, command), `#!/usr/bin/env bash\nprintf '%s\\n' '${command}' >> '${posixPath(traceFile)}'\nexit 91\n`, { mode: 0o755 });
}
const results = [];
const execute = (name, args, expectedStatus, pattern) => {
  const result = spawnSync(bash, args, {
    encoding: 'utf8', timeout: 15_000,
    env: { ...process.env, PATH: `${fakeBin}${path.delimiter}${process.env.PATH ?? ''}` },
  });
  const outputText = `${result.stdout ?? ''}${result.stderr ?? ''}`;
  const passed = !result.error && result.status === expectedStatus && (!pattern || pattern.test(outputText));
  results.push({ name, passed, exitCode: result.status, error: result.error ? 'Bashを起動できませんでした' : undefined });
};
execute('Bash構文', ['-n', posixPath(installer)], 0);
execute('日本語ヘルプ', [posixPath(installer), '--help'], 0, /--check[\s\S]*パスワード/);
execute('パスワード引数を拒否', [posixPath(installer), '--password', 'synthetic-secret'], 1, /未対応の引数/);
execute('値欠落を拒否', [posixPath(installer), '--public-port'], 1, /値がありません/);
execute('不正ポートを拒否', [posixPath(installer), '--public-port', '65536'], 1, /1024～65535/);
execute('同一ポートを拒否', [posixPath(installer), '--public-port', '43129'], 1, /ポートを分けて/);
execute('ユーザー名の注入を拒否', [posixPath(installer), '--ssh-user', 'a;id'], 1, /ユーザー名/);
execute('公開鍵未指定を拒否', [posixPath(installer), '--check'], 1, /公開鍵/);
// rootのLinuxホストでも実導入を実行せず、安全な拒否経路だけに限定する。
const uid = spawnSync(bash, ['-c', 'printf "%s" "$EUID"'], { encoding: 'utf8' });
if (uid.stdout !== '0') {
  execute('非rootのcheckを拒否', [posixPath(installer), '--check', '--public-key-file', posixPath(fixtureKey)], 1, /sudoで実行/);
  execute('非rootの導入を拒否', [posixPath(installer), '--public-key-file', posixPath(fixtureKey)], 1, /sudoで実行/);
}
let mutations = [];
try { await access(traceFile); mutations = (await readFile(traceFile, 'utf8')).trim().split('\n').filter(Boolean); } catch (error) { if (error.code !== 'ENOENT') throw error; }
results.push({ name: '変更コマンド未呼出し', passed: mutations.length === 0 });
results.push({ name: '公開鍵入力を保持', passed: originalKey === await readFile(fixtureKey, 'utf8') });
await writeFile(path.join(resolvedOutput, 'mutation-trace.json'), JSON.stringify({ commands: mutations }, null, 2));
const failed = results.filter(result => !result.passed).length;
await writeFile(path.join(resolvedOutput, 'summary.json'), JSON.stringify({
  results, failed, limitation: 'Linux root環境でのパッケージ導入・Squid起動・SSH実接続は未検証',
}, null, 2));
console.log(`VPSセットアップCLI E2E: ${results.length - failed}/${results.length} 合格 (${resolvedOutput})`);
if (failed) process.exitCode = 1;
