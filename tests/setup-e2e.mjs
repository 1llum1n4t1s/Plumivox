import assert from 'node:assert/strict';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';
import { mkdir, writeFile, readFile, realpath, stat } from 'node:fs/promises';
import { randomUUID } from 'node:crypto';

// 失敗条件: 上書き・不正指紋/ポート/2FA・鍵欠落・保存先リンク/ACL・秘密値漏洩・同時変更。
const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const output = path.resolve(process.env.PLUMIVOX_SETUP_E2E_OUTPUT_DIR || path.join(os.tmpdir(), 'Codex', 'Plumivox', randomUUID()));
const inside = (child, parent) => { const value = path.relative(parent, child); return value === '' || (!value.startsWith('..') && !path.isAbsolute(value)); };
let ancestor = output;
while (true) { try { await stat(ancestor); break; } catch (error) { if (error.code !== 'ENOENT') throw error; ancestor = path.dirname(ancestor); } }
const resolved = path.join(await realpath(ancestor), path.relative(ancestor, output));
for (const blocked of [repo, path.join(os.homedir(), '.codex'), ...(process.env.CODEX_HOME ? [process.env.CODEX_HOME] : [])]) {
  assert.ok(!inside(output, blocked) && !inside(resolved, blocked));
}
await mkdir(output, { recursive: true });
const results = [];
async function test(name, fn) { try { await fn(); results.push({ name, ok: true }); } catch (error) { results.push({ name, ok: false, error: error.message }); } }
const psQuote = (value) => `'${value.replaceAll("'", "''")}'`;
const script = path.join(repo, 'setup', 'Setup-Plumivox.ps1');
const local = path.join(output, 'local');
const key = path.join(local, 'Kagayoi', 'Plumivox', 'keys', 'plumivox_ed25519');
const settings = path.join(output, 'settings.json');
const secrets = ['dummy-api-secret', 'dummy-email-secret', 'dummy-x-password', 'dummy-proxy-password'];
const valid = ['example_user', 'example.invalid', '', '', '', '', `SHA256:${'A'.repeat(43)}`, '', '', ...secrets.slice(0, 3), '', secrets[3]];
let sequence = 0;
async function runPs(body, values = [], env = {}, promptHook = '') {
  const wrapper = path.join(output, `invoke-${++sequence}.ps1`);
  const fixture = path.join(output, `input-${sequence}.json`);
  await writeFile(fixture, JSON.stringify(values));
  await writeFile(wrapper, `$ErrorActionPreference='Stop'\n. ${psQuote(script)}\n$script:answers=[Collections.Generic.Queue[string]]::new()\n(Get-Content -LiteralPath ${psQuote(fixture)} -Raw | ConvertFrom-Json) | ForEach-Object { $script:answers.Enqueue($_) }\nfunction Read-Host { param([string]$Prompt,[switch]$AsSecureString)\n${promptHook}\n$v=$script:answers.Dequeue()\nif($AsSecureString){ if($v -eq '') { return [Security.SecureString]::new() }; return ConvertTo-SecureString $v -AsPlainText -Force }; return $v\n}\ntry { ${body}\n} catch { Write-Host 'SETUP_FAILED'; exit 1 }\n`);
  return spawnSync('pwsh', ['-NoProfile', '-File', wrapper], { encoding: 'utf8', env: { ...process.env, LOCALAPPDATA: local, PLUMIVOX_SECRETS_FILE: '', ...env } });
}
const invoke = `Invoke-PlumivoxSetup -SettingsPath ${psQuote(settings)} -PrivateKeyPath ${psQuote(key)}`;
const checkNoLeak = (result) => { for (const secret of secrets) assert.ok(!`${result.stdout}${result.stderr}`.includes(secret)); };

await test('portable-source-priority-and-frozen-selection', async () => {
  const home = path.join(output, 'home');
  await mkdir(path.join(home, 'dev', 'Secret'), { recursive: true });
  const legacy = path.join(home, 'dev', 'Secret', 'secrets.json');
  const portable = path.join(local, 'Kagayoi', 'Plumivox', 'settings.json');
  await mkdir(path.dirname(portable), { recursive: true });
  await writeFile(portable, JSON.stringify({ twitterapi_io: { portable: {} } }));
  const module = new URL('../src/config.mjs', import.meta.url).href;
  const run = (code, override = {}) => spawnSync(process.execPath, ['--input-type=module', '-e', `import {resolveSecretsPath,createAccountStore} from ${JSON.stringify(module)}; ${code}`], { encoding: 'utf8', env: { ...process.env, USERPROFILE: home, HOME: home, LOCALAPPDATA: local, PLUMIVOX_SECRETS_FILE: '', ...override } });
  assert.equal(run('console.log(resolveSecretsPath())').stdout.trim(), portable);
  await writeFile(legacy, 'invalid JSON');
  assert.equal(run('console.log(resolveSecretsPath())').stdout.trim(), legacy);
  assert.equal(run(`console.log(resolveSecretsPath(${JSON.stringify(settings)}))`).stdout.trim(), settings);
  assert.equal(run(`console.log(resolveSecretsPath(${JSON.stringify(settings)}))`, { PLUMIVOX_SECRETS_FILE: portable }).stdout.trim(), portable);
  const invalid = run(`const store=createAccountStore(); process.env.PLUMIVOX_SECRETS_FILE=${JSON.stringify(portable)}; try { await store.listAccounts(); } catch(e) { console.log(e.code); }`);
  // 設定ストアの実際の読込みAPIで不正な正本から別ファイルへ逃げない。
  assert.equal(invalid.status, 0);
  assert.match(invalid.stdout, /CONFIG_UNAVAILABLE/);
});

if (process.platform === 'win32') {
  await test('prepare-dedicated-key-and-refuse-replacement', async () => {
    const first = await runPs("$PSNativeCommandArgumentPassing='Legacy'; Invoke-PlumivoxSetup -PrepareKey");
    assert.equal(first.status, 0, first.stdout + first.stderr);
    assert.match(first.stdout, /ssh-ed25519 /);
    assert.ok(!first.stdout.includes('PRIVATE KEY'));
    assert.equal((await runPs('Invoke-PlumivoxSetup -PrepareKey')).status, 1);
    const repeated = spawnSync('pwsh', ['-NoProfile', '-File', script, '-PrepareKey'], { encoding: 'utf8', env: { ...process.env, LOCALAPPDATA: local, PLUMIVOX_SECRETS_FILE: '' } });
    assert.equal(repeated.status, 1); assert.match(repeated.stdout, /既存鍵をそのまま再利用/); checkNoLeak(repeated);
  });
  await test('wizard-preserves-unrelated-config-and-check-is-readonly', async () => {
    await writeFile(settings, JSON.stringify({ unrelated: { value: ['keep', 7] }, twitterapi_io: { other: { user_name: 'other' } } }));
    const configured = await runPs(invoke, valid);
    assert.equal(configured.status, 0, configured.stdout + configured.stderr); checkNoLeak(configured);
    const raw = await readFile(settings, 'utf8');
    const doc = JSON.parse(raw);
    assert.deepEqual(doc.unrelated, { value: ['keep', 7] });
    assert.deepEqual(doc.twitterapi_io.other, { user_name: 'other' });
    assert.equal(doc.twitterapi_io.main.proxy_mode, 'managed');
    const checked = await runPs(`$PSNativeCommandArgumentPassing='Legacy'; ${invoke} -Check`);
    assert.equal(checked.status, 0, checked.stdout + checked.stderr); checkNoLeak(checked);
    assert.equal(await readFile(settings, 'utf8'), raw);
    const refused = await runPs(invoke, valid); assert.equal(refused.status, 1); checkNoLeak(refused);
    assert.equal(await readFile(settings, 'utf8'), raw);
  });
  await test('invalid-fingerprint-port-totp-and-missing-key-refused', async () => {
    for (const [index, value] of [[6, 'SHA256:not-verified'], [3, '0'], [12, '123456']]) {
      const target = path.join(output, `invalid-${index}.json`);
      const answers = [...valid]; answers[index] = value;
      const result = await runPs(`Invoke-PlumivoxSetup -SettingsPath ${psQuote(target)} -PrivateKeyPath ${psQuote(key)}`, answers);
      assert.equal(result.status, 1); checkNoLeak(result);
      await assert.rejects(readFile(target), { code: 'ENOENT' });
    }
    const result = await runPs(`Invoke-PlumivoxSetup -SettingsPath ${psQuote(path.join(output, 'missing.json'))} -PrivateKeyPath ${psQuote(path.join(output, 'absent-key'))}`, valid);
    assert.equal(result.status, 1); checkNoLeak(result);
  });
  await test('production-cli-errors-do-not-echo-secret-input', async () => {
    const result = spawnSync('pwsh', ['-NoProfile', '-File', script, '-SettingsPath', settings], { encoding: 'utf8', env: { ...process.env, PLUMIVOX_SECRETS_FILE: '' } });
    assert.equal(result.status, 1); checkNoLeak(result);
  });
  await test('observed-concurrent-settings-change-is-preserved', async () => {
    const target = path.join(output, 'race.json');
    await writeFile(target, '{"unrelated":"original"}');
    const changed = '{"unrelated":"concurrent-change"}';
    const result = await runPs(`Invoke-PlumivoxSetup -SettingsPath ${psQuote(target)} -PrivateKeyPath ${psQuote(key)}`, valid, {},
      `if ($Prompt -eq 'プロキシパスワード') { [IO.File]::WriteAllText(${psQuote(target)}, ${psQuote(changed)}) }`);
    assert.equal(result.status, 1); checkNoLeak(result);
    assert.equal(await readFile(target, 'utf8'), changed);
  });
  await test('check-refuses-invalid-proxy-and-broad-private-key-acl', async () => {
    const doc = JSON.parse(await readFile(settings, 'utf8'));
    const target = path.join(output, 'bad-proxy.json');
    doc.twitterapi_io.main.proxy = 'http://example.invalid:3128';
    await writeFile(target, JSON.stringify(doc));
    assert.equal((await runPs(`Set-PlumivoxPrivateAcl ${psQuote(target)}; Invoke-PlumivoxSetup -Check -SettingsPath ${psQuote(target)}`)).status, 1);
    const checked = spawnSync('pwsh', ['-NoProfile', '-File', script, '-Check', '-SettingsPath', target], { encoding: 'utf8', env: { ...process.env, LOCALAPPDATA: local, PLUMIVOX_SECRETS_FILE: '' } });
    assert.equal(checked.status, 1); assert.match(checked.stdout, /認証付きプロキシURL/); checkNoLeak(checked);
    const broad = `\n$acl=Get-Acl -LiteralPath ${psQuote(key)}\n$acl.AddAccessRule([Security.AccessControl.FileSystemAccessRule]::new([Security.Principal.SecurityIdentifier]::new('S-1-1-0'), 'Read', 'Allow'))\n[IO.FileSystemAclExtensions]::SetAccessControl([IO.FileInfo]::new(${psQuote(key)}), $acl)\ntry { ${invoke} -Check } finally { Set-PlumivoxPrivateAcl ${psQuote(key)} }`;
    const result = await runPs(broad);
    assert.equal(result.status, 1); checkNoLeak(result);
  });
}
const summary = { failed: results.filter((result) => !result.ok).length, results };
await writeFile(path.join(output, 'summary.json'), JSON.stringify(summary, null, 2));
console.log(JSON.stringify({ output, ...summary }, null, 2));
process.exitCode = summary.failed ? 1 : 0;
