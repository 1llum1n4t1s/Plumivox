import { constants } from 'node:fs';
import { copyFile, lstat, mkdir, readFile, realpath, rename, writeFile } from 'node:fs/promises';
import { createHash, randomUUID } from 'node:crypto';
import { spawn } from 'node:child_process';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const MAX_BYTES = 2 * 1024 * 1024;
const hash = (value) => createHash('sha256').update(value, 'utf8').digest('hex');
const nonempty = (value) => typeof value === 'string' && value.trim().length > 0;
const projectRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

/** 子プロセスの詳細や秘密値を含まない永続化エラー。 */
export class SessionStorageError extends Error {
  constructor(code = 'SESSION_STORAGE_UNAVAILABLE') {
    super(code === 'AUTH_BUSY' ? 'このアカウントは別のプロセスでログイン処理中です。完了後に再実行してください。'
      : 'Windowsのセッション保存先とPowerShellを確認してください。');
    this.name = 'SessionStorageError';
    this.code = code;
  }
}

// 秘密を引数・環境変数・平文ファイルへ渡さず、標準入力と内部メモリだけで扱う。
const DPAPI = `
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$VerbosePreference = 'SilentlyContinue'
$DebugPreference = 'SilentlyContinue'
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)
try {
  $request = [Console]::In.ReadToEnd() | ConvertFrom-Json
  if ($request.operation -eq 'encrypt') {
    $request.value | ConvertTo-SecureString -AsPlainText -Force | ConvertFrom-SecureString
  } elseif ($request.operation -eq 'decrypt') {
    $secure = $request.value | ConvertTo-SecureString
    [Console]::Write(([pscredential]::new('session', $secure)).GetNetworkCredential().Password)
  } elseif ($request.operation -eq 'prepare') {
    $probe = 'Plumivox DPAPI' | ConvertTo-SecureString -AsPlainText -Force | ConvertFrom-SecureString | ConvertTo-SecureString
    if (([pscredential]::new('session', $probe)).GetNetworkCredential().Password -cne 'Plumivox DPAPI') { exit 2 }
    $sid = [Security.Principal.WindowsIdentity]::GetCurrent().User
    foreach ($directory in $request.directories) {
      $info = [IO.DirectoryInfo]::new($directory)
      $acl = [IO.FileSystemAclExtensions]::GetAccessControl($info, [Security.AccessControl.AccessControlSections]::Access)
      $acl.SetAccessRuleProtection($true, $false)
      foreach ($rule in $acl.GetAccessRules($true, $true, [Security.Principal.SecurityIdentifier])) { [void]$acl.RemoveAccessRuleSpecific($rule) }
      $rule = [Security.AccessControl.FileSystemAccessRule]::new($sid, 'FullControl', 'ContainerInherit, ObjectInherit', 'None', 'Allow')
      $acl.AddAccessRule($rule)
      [IO.FileSystemAclExtensions]::SetAccessControl($info, $acl)
    }
    [Console]::Write('ok')
  } else { exit 2 }
} catch { exit 2 }
`;

// 親の標準入力がEOFになるまで同じPowerShellスレッドで所有権を保持する。
// 親プロセス終了時にもパイプが閉じるため、ロックファイルの回収は不要。
const SESSION_MUTEX = `
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$VerbosePreference = 'SilentlyContinue'
$DebugPreference = 'SilentlyContinue'
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)
$mutex = $null
$owned = $false
try {
  $request = [Console]::In.ReadLine() | ConvertFrom-Json
  if ($request.name -notmatch '^Global\\\\Kagayoi[.]Plumivox[.]Auth[.][a-f0-9]{64}$') { exit 2 }
  $mutex = [Threading.Mutex]::new($false, $request.name)
  try { $owned = $mutex.WaitOne(0) }
  catch [Threading.AbandonedMutexException] { $owned = $true }
  if (-not $owned) { [Console]::Out.WriteLine('busy'); [Console]::Out.Flush(); exit 3 }
  [Console]::Out.WriteLine('acquired')
  [Console]::Out.Flush()
  [void][Console]::In.ReadToEnd()
} catch { exit 2 }
finally {
  if ($owned) { $mutex.ReleaseMutex() }
  if ($null -ne $mutex) { $mutex.Dispose() }
}
`;

function acquireSessionMutex(name) {
  if (process.platform !== 'win32') return Promise.reject(new SessionStorageError());
  return new Promise((resolve, reject) => {
    let output = '';
    let settled = false;
    let acquired = false;
    let requestedRelease = false;
    const child = spawn('pwsh', ['-NoLogo', '-NoProfile', '-NonInteractive', '-EncodedCommand',
      Buffer.from(SESSION_MUTEX, 'utf16le').toString('base64')], { windowsHide: true, stdio: ['pipe', 'pipe', 'ignore'] });
    let finishClosed;
    const closed = new Promise((resolveClosed) => { finishClosed = resolveClosed; });
    const fail = (code) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      child.stdin.end();
      reject(new SessionStorageError(code));
    };
    const timer = setTimeout(() => { child.kill(); fail(); }, 10_000);
    child.on('error', () => { finishClosed(null); fail(); });
    child.stdin.on('error', () => fail());
    child.on('close', (code) => {
      finishClosed(code);
      if (!acquired) fail();
    });
    child.stdout.setEncoding('utf8');
    child.stdout.on('data', (chunk) => {
      output += chunk;
      if (output.length > 128) { child.kill(); fail(); return; }
      if (settled || !output.includes('\n')) return;
      const response = output.trim();
      if (response === 'busy') { fail('AUTH_BUSY'); return; }
      if (response !== 'acquired') { child.kill(); fail(); return; }
      acquired = true;
      settled = true;
      clearTimeout(timer);
      resolve(async () => {
        if (requestedRelease) return;
        requestedRelease = true;
        child.stdin.end();
        const releaseTimer = setTimeout(() => child.kill(), 5_000);
        try {
          const code = await closed;
          if (code !== 0) throw new SessionStorageError();
        } finally { clearTimeout(releaseTimer); }
      });
    });
    // ハッシュ化済みの名前だけを送り、資格情報やユーザー名は子へ渡さない。
    child.stdin.write(`${JSON.stringify({ name })}\n`);
  });
}

function protectedOperation(operation, payload) {
  if (process.platform !== 'win32') return Promise.reject(new SessionStorageError());
  return new Promise((resolve, reject) => {
    let output = '';
    let settled = false;
    const child = spawn('pwsh', ['-NoLogo', '-NoProfile', '-NonInteractive', '-EncodedCommand',
      Buffer.from(DPAPI, 'utf16le').toString('base64')], { windowsHide: true, stdio: ['pipe', 'pipe', 'ignore'] });
    const finish = (error) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      if (error) reject(new SessionStorageError());
      else resolve(output.trim());
    };
    const timer = setTimeout(() => { child.kill(); finish(true); }, 10_000);
    child.on('error', () => finish(true));
    child.stdin.on('error', () => finish(true));
    child.stdout.setEncoding('utf8');
    child.stdout.on('data', (chunk) => {
      output += chunk.toString('utf8');
      if (Buffer.byteLength(output) > MAX_BYTES) { child.kill(); finish(true); }
    });
    child.on('close', (code) => finish(code !== 0));
    child.stdin.end(JSON.stringify({ operation, ...payload }));
  });
}

function inside(candidate, root) {
  const relative = path.relative(root, candidate);
  return relative === '' || (!relative.startsWith(`..${path.sep}`) && relative !== '..' && !path.isAbsolute(relative));
}

function forbidden(candidate) {
  const codexRoot = process.env.CODEX_HOME || path.join(process.env.USERPROFILE || 'C:\\Users\\IMT', '.codex');
  return inside(candidate, projectRoot) || inside(candidate, path.resolve(codexRoot)) ||
    /(?:^|[\\/])\.codex(?:[\\/]|$)/i.test(candidate) || candidate === path.parse(candidate).root;
}

// 未作成の末尾も、既存の祖先のリンク先から解決して確認する。
async function validateDirectory(candidate, canonical = false) {
  if (!nonempty(candidate) || !path.isAbsolute(candidate)) throw new SessionStorageError();
  const absolute = path.resolve(candidate);
  if (forbidden(absolute)) throw new SessionStorageError();
  let ancestor = absolute;
  const suffix = [];
  while (true) {
    try {
      const info = await lstat(ancestor);
      if (!info.isDirectory() && !info.isSymbolicLink()) throw new SessionStorageError();
      const resolved = await realpath(ancestor);
      const target = path.join(resolved, ...suffix.reverse());
      if (forbidden(target)) throw new SessionStorageError();
      return canonical ? target : absolute;
    } catch (error) {
      if (error.code !== 'ENOENT') throw new SessionStorageError();
      suffix.push(path.basename(ancestor));
      const parent = path.dirname(ancestor);
      if (parent === ancestor) throw new SessionStorageError();
      ancestor = parent;
    }
  }
}

async function decode(file, binding, userName, own) {
  const info = await lstat(file);
  if (!info.isFile() || info.isSymbolicLink() || info.size > MAX_BYTES) throw new SessionStorageError();
  const encrypted = await readFile(file, 'utf8');
  if (Buffer.byteLength(encrypted) > MAX_BYTES) throw new SessionStorageError();
  const session = JSON.parse(await protectedOperation('decrypt', { value: encrypted }));
  if (session?.schema_version !== 2 || session.binding !== binding ||
      typeof session.user_name !== 'string' || session.user_name.toLowerCase() !== userName.toLowerCase() ||
      !nonempty(session.cookie) || (own && (session.source !== 'plumivox_api' ||
      !nonempty(session.acquired_at) || !Number.isFinite(Date.parse(session.acquired_at))))) throw new SessionStorageError();
  return { cookie: session.cookie, acquiredAt: own ? session.acquired_at : null };
}

/** 既存vava/blblセッションは読取だけを行う。 */
export async function readLegacyProtectedCookie(file, binding, userName) {
  try { return (await decode(file, binding, userName, false)).cookie; }
  catch { return null; }
}

export function createSessionStorage({ stateDirectory } = {}) {
  const root = process.env.PLUMIVOX_STATE_DIR || stateDirectory ||
    (process.env.LOCALAPPDATA ? path.join(process.env.LOCALAPPDATA, 'Kagayoi', 'Plumivox') : null);
  const directoryFor = ({ account, userName }) => root && path.join(root, hash(`${account}\n${userName.toLowerCase()}`));
  const bindingFor = ({ apiKey, proxy }) => hash(`${apiKey}\n${proxy}`);
  const saves = new Map();

  async function prepare(account) {
    try {
      if (process.platform !== 'win32') throw new SessionStorageError();
      const safeRoot = await validateDirectory(root);
      const directory = await validateDirectory(directoryFor(account));
      await mkdir(directory, { recursive: true });
      await validateDirectory(directory);
      if ((await lstat(directory)).isSymbolicLink()) throw new SessionStorageError();
      try {
        const existing = await lstat(path.join(directory, 'session.dpapi'));
        if (!existing.isFile() || existing.isSymbolicLink() || existing.size > MAX_BYTES) throw new SessionStorageError();
      } catch (error) { if (error.code !== 'ENOENT') throw error; }
      await protectedOperation('prepare', { directories: [safeRoot, directory] });
    } catch { throw new SessionStorageError(); }
  }

  return {
    prepare,
    async withLock(account, callback) {
      const directory = await validateDirectory(directoryFor(account), true);
      const name = `Global\\Kagayoi.Plumivox.Auth.${hash(directory.toLowerCase())}`;
      const release = await acquireSessionMutex(name);
      try { return await callback(account); }
      finally { await release(); }
    },
    async read(account) {
      if (!root) return { status: 'missing' };
      try {
        const directory = await validateDirectory(directoryFor(account));
        const session = await decode(path.join(directory, 'session.dpapi'), bindingFor(account), account.userName, true);
        return { status: 'valid', ...session };
      } catch (error) {
        return { status: error.code === 'ENOENT' ? 'missing' : 'invalid' };
      }
    },
    async save(account, cookie) {
      const key = directoryFor(account);
      const previous = saves.get(key) || Promise.resolve();
      const pending = previous.catch(() => {}).then(async () => {
        try {
          if (!nonempty(cookie) || Buffer.byteLength(cookie) > MAX_BYTES / 2 || /[\u0000\r\n]/u.test(cookie)) throw new SessionStorageError();
          await prepare(account);
          const acquiredAt = new Date().toISOString();
          const encrypted = await protectedOperation('encrypt', { value: JSON.stringify({ schema_version: 2,
            cookie, binding: bindingFor(account), user_name: account.userName, acquired_at: acquiredAt, source: 'plumivox_api' }) });
          const directory = await validateDirectory(key);
          const destination = path.join(directory, 'session.dpapi');
          const id = randomUUID();
          const staging = path.join(directory, `session-${id}.pending`);
          // 保存前の暗号化キャッシュを復元用に保持する。失敗しても旧本体を消さない。
          try {
            const existing = await lstat(destination);
            if (!existing.isFile() || existing.isSymbolicLink() || existing.size > MAX_BYTES) throw new SessionStorageError();
            await copyFile(destination, path.join(directory, `session-${id}.backup.dpapi`), constants.COPYFILE_EXCL);
          } catch (error) { if (error.code !== 'ENOENT') throw error; }
          await writeFile(staging, encrypted, { encoding: 'utf8', flag: 'wx', mode: 0o600 });
          await rename(staging, destination);
          return { account: account.account, userName: account.userName, sessionSource: 'plumivox_dpapi', acquiredAt };
        } catch { throw new SessionStorageError(); }
      });
      saves.set(key, pending);
      try { return await pending; }
      finally { if (saves.get(key) === pending) saves.delete(key); }
    },
  };
}
