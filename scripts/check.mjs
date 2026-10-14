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
// 同梱セットアップも配布物として構文を確認する。開発環境だけで実行する。
const setupScript = path.join(root, 'setup', 'Setup-Plumivox.ps1').replaceAll("'", "''");
const psCommand = `$tokens=$null;$errors=$null;[void][System.Management.Automation.Language.Parser]::ParseFile('${setupScript}',[ref]$tokens,[ref]$errors);if($errors.Count){$errors | ForEach-Object { [Console]::Error.WriteLine($_.Message) };exit 1}`;
const ps = spawnSync('pwsh', ['-NoProfile', '-NonInteractive', '-EncodedCommand', Buffer.from(psCommand, 'utf16le').toString('base64')], { stdio: 'inherit' });
if (ps.error || ps.status !== 0) {
  console.error('セットアップ構文の確認にはPowerShell 7 (pwsh) が必要です');
  process.exit(1);
}
const installer = path.join(root, 'setup', 'vps', 'install.sh');
const bashCandidates = process.platform === 'win32' ? ['bash', 'C:/Program Files/Git/bin/bash.exe'] : ['bash'];
let bashResult;
for (const bash of bashCandidates) {
  bashResult = spawnSync(bash, ['-n', installer.replaceAll('\\', '/')], { stdio: 'inherit' });
  if (!bashResult.error) break;
}
if (bashResult?.error || bashResult?.status !== 0) {
  console.error('VPSセットアップ構文の確認にはBash (WindowsではGit for Windows) が必要です');
  process.exit(1);
}
for (const resource of ['setup/vps/squid.conf.in', 'setup/vps/plumivox-proxy.service', 'skills/setup-plumivox/SKILL.md']) {
  await readFile(path.join(root, resource), 'utf8');
}
for (const file of await readdir(path.join(root, 'setup', 'vps'))) {
  const text = await readFile(path.join(root, 'setup', 'vps', file), 'utf8');
  if (text.includes('\r')) throw new Error(`VPS配布資材はLFで保存してください: ${file}`);
}
console.log(`構文とJSON形式: ${files.length}ソース、4設定ファイル 合格`);
console.log('同梱セットアップ: PowerShell・Bash構文と参照資材 合格');
