import assert from 'node:assert/strict';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawn, spawnSync } from 'node:child_process';
import { mkdir, writeFile, readFile, realpath, lstat, chmod, symlink } from 'node:fs/promises';
import { randomUUID } from 'node:crypto';
import { isInside } from './path-containment.mjs';

// 実際のmacOSセットアップ入口を使う。資格情報・SSH鍵は全て専用の合成データ。
assert.equal(process.platform, 'darwin', 'Native setup E2E requires macOS');
const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const binary = path.resolve(process.env.PLUMIVOX_NATIVE_BINARY || path.join(repo, 'dist', 'plumivox'));
assert.ok((await lstat(binary)).isFile(), 'Build the native distribution first');
const output = path.resolve(process.env.PLUMIVOX_NATIVE_SETUP_E2E_OUTPUT_DIR
  || path.join(await realpath(os.tmpdir()), 'Codex', 'Plumivox', randomUUID()));
let ancestor = output;
while (true) {
  try { await lstat(ancestor); break; }
  catch (error) { if (error.code !== 'ENOENT') throw error; ancestor = path.dirname(ancestor); }
}
for (let current = ancestor; ; current = path.dirname(current)) {
  assert.ok(!(await lstat(current)).isSymbolicLink(), 'Evidence path crosses a symbolic link');
  if (path.dirname(current) === current) break;
}
const resolved = path.join(await realpath(ancestor), path.relative(ancestor, output));
for (const blocked of [repo, path.join(os.homedir(), '.codex'), ...(process.env.CODEX_HOME ? [process.env.CODEX_HOME] : [])]) {
  assert.ok(!isInside(output, blocked) && !isInside(resolved, blocked), 'Evidence must remain outside repository and Codex user directory');
}
await mkdir(output, { recursive: true, mode: 0o700 });
assert.equal((await lstat(output)).mode & 0o077, 0, 'Evidence directory must be private');
const env = Object.fromEntries(Object.entries(process.env).filter(([key, value]) => value !== undefined
  && !['PLUMIVOX_SECRETS_FILE', 'PLUMIVOX_DEFAULT_ACCOUNT', 'PLUMIVOX_STATE_DIR', 'PLUMIVOX_TEST_HOOKS'].includes(key)));
const key = path.join(output, 'keys', 'plumivox_ed25519');
const settings = path.join(output, 'settings.json');
const fakeSecrets = ['setup-synthetic-api-never-display', 'setup-synthetic-email-never-display',
  'setup-synthetic-password-never-display', 'setup-synthetic-proxy-never-display'];
const fingerprint = `SHA256:${'A'.repeat(43)}`;
const redact = (value) => fakeSecrets.reduce((text, secret) => text.replaceAll(secret, '[redacted]'), String(value));
const results = [];
async function test(name, action) {
  try { await action(); results.push({ name, ok: true }); }
  catch (error) { results.push({ name, ok: false, error: redact(error.message) }); }
}
function noLeak(text) {
  for (const secret of fakeSecrets) assert.ok(!text.includes(secret), 'Setup output exposed a synthetic credential');
  assert.ok(!text.includes('PRIVATE KEY'), 'Setup output exposed private key data');
}
function cli(args, expected = 0) {
  const result = spawnSync(binary, ['--setup', ...args], { env, encoding: 'utf8', timeout: 30000 });
  const text = `${result.stdout || ''}${result.stderr || ''}`;
  noLeak(text);
  assert.equal(result.error, undefined, result.error?.message);
  assert.equal(result.status, expected, redact(text));
  return text;
}
const profile = (user) => ({ user_name: user, api_key: fakeSecrets[0], email: fakeSecrets[1], password: fakeSecrets[2],
  proxy_mode: 'managed', proxy: `http://plumivox:${fakeSecrets[3]}@example.invalid:3128` });
const initial = { unrelated: { preserve: ['keep', 7] }, twitterapi_io: { primary: profile('Primary'), secondary: profile('Secondary'), default_account: 'primary' },
  plumivox: { managed_proxy: { ssh_host: 'example.invalid', ssh_user: 'plumivox', ssh_port: 22, local_port: 43128,
    remote_port: 43129, private_key_file: key, host_key_sha256: fingerprint } } };

await test('native-prepare-key-and-refuse-replacement', async () => {
  const first = cli(['--prepare-key', '--private-key-path', key]);
  assert.match(first, /ssh-ed25519 /);
  for (const target of [path.dirname(key), key, `${key}.pub`]) {
    assert.equal((await lstat(target)).mode & 0o077, 0);
  }
  const privateBefore = await readFile(key);
  const publicBefore = await readFile(`${key}.pub`);
  assert.match(cli(['--prepare-key', '--private-key-path', key], 1), /KEY_EXISTS/);
  assert.deepEqual(await readFile(key), privateBefore);
  assert.deepEqual(await readFile(`${key}.pub`), publicBefore);
});

await writeFile(settings, JSON.stringify(initial), { flag: 'wx', mode: 0o600 });
await test('native-check-valid-settings-is-readonly', async () => {
  const before = await readFile(settings);
  assert.match(cli(['--check', '--settings-path', settings, '--account', 'primary']), /ネットワーク通信は実行していません/);
  assert.deepEqual(await readFile(settings), before);
});
await test('native-default-only-preserves-accounts-relay-and-unrelated-data', async () => {
  cli(['--settings-path', settings, '--set-default-account', 'secondary']);
  const expected = structuredClone(initial);
  expected.twitterapi_io.default_account = 'secondary';
  assert.deepEqual(JSON.parse(await readFile(settings, 'utf8')), expected);
  assert.equal((await lstat(settings)).mode & 0o077, 0);
  const before = await readFile(settings);
  assert.match(cli(['--settings-path', settings, '--set-default-account', 'missing'], 1), /DEFAULT_ACCOUNT_INVALID/);
  assert.deepEqual(await readFile(settings), before);
});
await test('native-wizard-refuses-same-account-and-shared-relay', async () => {
  const before = await readFile(settings);
  for (const account of ['primary', 'new_account']) {
    assert.match(cli(['--settings-path', settings, '--private-key-path', key, '--account', account], 1), /SETTINGS_EXISTS/);
    assert.deepEqual(await readFile(settings), before);
  }
});
await test('native-check-rejects-links-and-public-private-key-permissions', async () => {
  const linked = path.join(output, 'linked-settings.json');
  await symlink(settings, linked);
  assert.match(cli(['--check', '--settings-path', linked, '--account', 'primary'], 1), /PATH_INVALID/);
  await chmod(key, 0o644);
  try { assert.match(cli(['--check', '--settings-path', settings, '--account', 'primary'], 1), /ACL_UNSAFE/); }
  finally { await chmod(key, 0o600); }
});

await test('native-wizard-registers-with-hidden-terminal-secrets-and-first-default', async () => {
  const target = path.join(output, 'wizard-settings.json');
  const answers = [
    ['Xユーザー名 (@なし): ', 'Wizard'], ['VPSホスト名/IP: ', 'example.invalid'], ['SSHユーザー [plumivox]: ', ''],
    ['SSHポート [22]: ', ''], ['PC内部ポート [43128]: ', ''], ['VPS内部ポート [43129]: ', ''],
    ['コンソールで確認済みのSHA256指紋: ', fingerprint], ['公開プロキシポート [3128]: ', ''],
    ['プロキシユーザー [plumivox]: ', ''], ['twitterapi.io APIキー: ', fakeSecrets[0]], ['Xログインメール: ', fakeSecrets[1]],
    ['Xパスワード: ', fakeSecrets[2]], ['Base32の2FA秘密鍵 (未使用なら空欄): ', ''], ['プロキシパスワード: ', fakeSecrets[3]],
  ];
  const result = await new Promise((resolve, reject) => {
    // macOSのscriptはNodeのsocket stdinを拒否するため、標準ptyで制御端末を作る。
    // 回答はプロセスの標準入力だけに渡し、引数や平文の回答ファイルへ保存しない。
    const driver = `import errno,json,os,pty,select,signal,sys,termios,time
answers=json.loads(sys.stdin.readline())
pid,master=pty.fork()
if pid==0:
    os.execv(sys.argv[1],sys.argv[1:])
output=b''
pending=b''
index=0
deadline=time.monotonic()+25
timed_out=False
while True:
    remaining=deadline-time.monotonic()
    if remaining<=0:
        timed_out=True
        os.kill(pid,signal.SIGTERM)
        break
    if not select.select([master],[],[],remaining)[0]:
        continue
    try:
        data=os.read(master,65536)
    except OSError as error:
        if error.errno==errno.EIO:
            break
        raise
    if not data:
        break
    output+=data
    pending+=data
    while index<len(answers) and answers[index][0].encode() in pending:
        prompt,answer=answers[index]
        pending=pending.split(prompt.encode(),1)[1]
        if index>=9:
            # rpasswordはプロンプト表示後に端末ECHOを無効にする。切替完了を待つ。
            while termios.tcgetattr(master)[3] & termios.ECHO:
                if time.monotonic()>=deadline:
                    raise RuntimeError('Password prompt did not disable terminal echo')
                time.sleep(0.001)
        os.write(master,(answer+'\\n').encode())
        index+=1
os.close(master)
_,status=os.waitpid(pid,0)
print(json.dumps({'status':os.waitstatus_to_exitcode(status),'text':output.decode(errors='replace'),'answered':index,'timedOut':timed_out}))
`;
    const child = spawn('/usr/bin/python3', ['-c', driver, binary, '--setup', '--settings-path', target,
      '--private-key-path', key, '--account', 'wizard'], { env, stdio: ['pipe', 'pipe', 'pipe'] });
    let stdout = '';
    let stderr = '';
    const timer = setTimeout(() => { child.kill('SIGTERM'); reject(new Error('Terminal setup driver timed out')); }, 30000);
    child.stdout.setEncoding('utf8');
    child.stderr.setEncoding('utf8');
    child.stdout.on('data', (chunk) => { stdout += chunk; });
    child.stderr.on('data', (chunk) => { stderr += chunk; });
    child.on('error', (error) => { clearTimeout(timer); reject(error); });
    child.on('close', (status) => {
      clearTimeout(timer);
      if (status !== 0) { reject(new Error(`Terminal setup driver failed: ${redact(stderr)}`)); return; }
      try { resolve(JSON.parse(stdout)); } catch { reject(new Error('Terminal setup driver returned invalid result')); }
    });
    child.stdin.end(`${JSON.stringify(answers)}\n`);
  });
  noLeak(result.text);
  assert.equal(result.answered, answers.length, `Wizard stopped at prompt ${result.answered}: ${redact(result.text)}`);
  assert.equal(result.status, 0, redact(result.text));
  const doc = JSON.parse(await readFile(target, 'utf8'));
  assert.equal(doc.twitterapi_io.default_account, 'wizard');
  assert.equal(doc.twitterapi_io.wizard.api_key, fakeSecrets[0]);
  assert.equal(doc.twitterapi_io.wizard.password, fakeSecrets[2]);
  assert.equal(new URL(doc.twitterapi_io.wizard.proxy).password, fakeSecrets[3]);
  assert.equal((await lstat(target)).mode & 0o077, 0);
  cli(['--check', '--settings-path', target, '--account', 'wizard']);
});

const summary = { failed: results.filter((result) => !result.ok).length, results };
await writeFile(path.join(output, 'summary.json'), JSON.stringify(summary, null, 2), { flag: 'wx', mode: 0o600 });
console.log(JSON.stringify({ output, ...summary }, null, 2));
process.exitCode = summary.failed ? 1 : 0;
