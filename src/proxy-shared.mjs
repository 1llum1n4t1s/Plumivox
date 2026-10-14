import net from 'node:net';
import { spawn } from 'node:child_process';
import { open, mkdir } from 'node:fs/promises';
import { isAbsolute, join, resolve } from 'node:path';
import { tmpdir } from 'node:os';
import { createHash, createHmac, randomBytes, timingSafeEqual } from 'node:crypto';
import { createManagedProxyManager } from './proxy-manager.mjs';

const MAX_FRAME_BYTES = 8 * 1024;
const MAX_KEY_BYTES = 1024 * 1024;
const STATES = new Set(['stopped', 'starting', 'ready', 'reconnecting', 'failed', 'external']);
const MESSAGES = {
  PROXY_CONFIG: '共有プロキシの設定が不正です。',
  PROXY_KEY_UNAVAILABLE: '共有プロキシの秘密鍵を読み取れません。',
  PROXY_KEY_TOO_LARGE: '共有プロキシの秘密鍵がサイズ上限を超えています。',
  PROXY_HOST_KEY_MISMATCH: '管理プロキシの接続先ホスト鍵が一致しません。',
  PROXY_AUTH_FAILED: '管理プロキシのSSH認証に失敗しました。',
  PROXY_PORT_BUSY: '管理プロキシのポートは使用中です。',
  PROXY_REMOTE_BIND_FAILED: '管理プロキシの転送ポートを確保できません。',
  PROXY_CONNECT_FAILED: '管理プロキシに接続できません。',
  PROXY_TIMEOUT: '共有プロキシの準備が時間内に完了しませんでした。',
  PROXY_CANCELLED: '共有プロキシの準備待機を取り消しました。',
  PROXY_STOPPED: '共有プロキシを停止しました。',
  PROXY_CLOSED: '共有プロキシの利用を終了しました。',
  PROXY_IPC_FAILED: '共有プロキシとの通信に失敗しました。',
  PROXY_IPC_AUTH_FAILED: '共有プロキシとの通信を認証できません。',
  PROXY_DAEMON_UNAVAILABLE: '共有プロキシの管理プロセスを利用できません。',
  PROXY_START_FAILED: '共有プロキシの管理プロセスを起動できません。',
  PROXY_UNSUPPORTED: '共有プロキシはWindowsでのみ利用できます。',
};

export class SharedProxyError extends Error {
  constructor(code) {
    const safeCode = typeof code === 'string' && Object.hasOwn(MESSAGES, code) ? code : 'PROXY_IPC_FAILED';
    super(MESSAGES[safeCode]);
    this.name = 'SharedProxyError';
    this.code = safeCode;
  }
}

function safeCode(error) {
  return typeof error?.code === 'string' && Object.hasOwn(MESSAGES, error.code) ? error.code : 'PROXY_IPC_FAILED';
}

function safeStatus(status) {
  if (!status || !STATES.has(status.state)) throw new SharedProxyError('PROXY_IPC_FAILED');
  return {
    state: status.state, managed: status.managed === true,
    activeConnections: Number.isInteger(status.activeConnections) && status.activeConnections >= 0 && status.activeConnections <= 16
      ? status.activeConnections : 0,
    lastErrorCode: typeof status.lastErrorCode === 'string' && Object.hasOwn(MESSAGES, status.lastErrorCode) ? status.lastErrorCode : null,
  };
}

const stoppedStatus = () => ({ state: 'stopped', managed: false, activeConnections: 0, lastErrorCode: null });

function snapshotConfig(config) {
  const port = value => Number.isInteger(value) && value >= 1024 && value <= 65535;
  if (!config || typeof config !== 'object' ||
    typeof config.sshHost !== 'string' || !config.sshHost || config.sshHost.length > 253 || /[\s\x00-\x1f]/.test(config.sshHost) ||
    typeof config.sshUser !== 'string' || !/^[A-Za-z0-9._-]{1,64}$/.test(config.sshUser) ||
    !Number.isInteger(config.sshPort) || config.sshPort < 1 || config.sshPort > 65535 ||
    !port(config.localPort) || !port(config.remotePort) ||
    typeof config.privateKeyFile !== 'string' || !isAbsolute(config.privateKeyFile) ||
    typeof config.hostKeySha256 !== 'string' || !/^SHA256:[A-Za-z0-9+/]{43}=?$/.test(config.hostKeySha256)) {
    throw new SharedProxyError('PROXY_CONFIG');
  }
  const fingerprint = config.hostKeySha256.slice(7).replace(/=$/, '');
  if (Buffer.from(fingerprint, 'base64').toString('base64').replace(/=$/, '') !== fingerprint) throw new SharedProxyError('PROXY_CONFIG');
  return {
    sshHost: config.sshHost.toLowerCase(), sshPort: config.sshPort, sshUser: config.sshUser,
    privateKeyFile: resolve(config.privateKeyFile), hostKeySha256: `SHA256:${fingerprint}`,
    localPort: config.localPort, remotePort: config.remotePort,
  };
}

function configId(config) {
  return createHash('sha256').update('Plumivox shared proxy metadata v1\0').update(JSON.stringify(config)).digest('hex');
}

function defaultSocketPath(config) {
  if (process.platform !== 'win32') throw new SharedProxyError('PROXY_UNSUPPORTED');
  return `\\\\.\\pipe\\plumivox-${configId(config)}`;
}

async function defaultDeriveSecret(config) {
  let file;
  let bytes;
  try {
    file = await open(config.privateKeyFile, 'r');
    const stat = await file.stat();
    if (!stat.isFile() || stat.size < 1) throw new SharedProxyError('PROXY_KEY_UNAVAILABLE');
    if (stat.size > MAX_KEY_BYTES) throw new SharedProxyError('PROXY_KEY_TOO_LARGE');
    bytes = Buffer.alloc(MAX_KEY_BYTES + 1);
    let length = 0;
    while (length < bytes.length) {
      const result = await file.read(bytes, length, bytes.length - length, length);
      if (!result.bytesRead) break;
      length += result.bytesRead;
    }
    if (length > MAX_KEY_BYTES) throw new SharedProxyError('PROXY_KEY_TOO_LARGE');
    if (!length) throw new SharedProxyError('PROXY_KEY_UNAVAILABLE');
    return createHash('sha256').update('Plumivox shared proxy IPC v1\0').update(bytes.subarray(0, length)).digest();
  } catch (error) {
    throw error instanceof SharedProxyError ? error : new SharedProxyError('PROXY_KEY_UNAVAILABLE');
  } finally {
    bytes?.fill(0);
    await file?.close().catch(() => {});
  }
}

function secretBytes(secret) {
  if (!Buffer.isBuffer(secret) || secret.length !== 32) throw new SharedProxyError('PROXY_CONFIG');
  return Buffer.from(secret);
}

function mac(secret, kind, body) {
  return createHmac('sha256', secret).update(JSON.stringify(['Plumivox shared proxy IPC v2', kind, body])).digest('hex');
}

function validMac(actual, expected) {
  return typeof actual === 'string' && /^[a-f0-9]{64}$/.test(actual) &&
    timingSafeEqual(Buffer.from(actual, 'hex'), Buffer.from(expected, 'hex'));
}

const nonce = () => randomBytes(32).toString('hex');
const validNonce = value => typeof value === 'string' && /^[a-f0-9]{64}$/.test(value);
function exactKeys(value, keys) {
  return value && typeof value === 'object' && !Array.isArray(value) &&
    Object.keys(value).length === keys.length && keys.every(key => Object.hasOwn(value, key));
}

function challengeBody(session, challenge) {
  return { type: 'challenge', version: 2, context: session.context, clientNonce: session.clientNonce, channelNonce: session.channelNonce, challenge };
}

function requestBody(session, requestId, operation) {
  return {
    type: 'request', version: 2, context: session.context, clientNonce: session.clientNonce,
    channelNonce: session.channelNonce, challenge: session.challenge, requestId, operation,
  };
}

function responseBody(session, request, challenge, result) {
  const body = {
    type: 'result', version: 2, context: session.context, clientNonce: session.clientNonce,
    channelNonce: session.channelNonce, requestChallenge: request.challenge, requestId: request.requestId,
    operation: request.operation, challenge, ok: result.ok,
  };
  if (result.ok) body.status = result.status;
  else body.code = result.code;
  return body;
}

function validStatus(status) {
  return exactKeys(status, ['state', 'managed', 'activeConnections', 'lastErrorCode']) && STATES.has(status.state) &&
    typeof status.managed === 'boolean' && Number.isInteger(status.activeConnections) && status.activeConnections >= 0 && status.activeConnections <= 16 &&
    (status.lastErrorCode === null || (typeof status.lastErrorCode === 'string' && Object.hasOwn(MESSAGES, status.lastErrorCode)));
}

function sameChannel(frame, session) {
  return frame.version === 2 && frame.context === session.context &&
    frame.clientNonce === session.clientNonce && frame.channelNonce === session.channelNonce;
}

function validChallenge(frame, session, secret) {
  return exactKeys(frame, ['type', 'version', 'context', 'clientNonce', 'channelNonce', 'challenge', 'proof']) &&
    frame.type === 'challenge' && frame.version === 2 && frame.context === session.context && frame.clientNonce === session.clientNonce &&
    validNonce(frame.channelNonce) && validNonce(frame.challenge) &&
    validMac(frame.proof, mac(secret, 'server', challengeBody({ ...session, channelNonce: frame.channelNonce }, frame.challenge)));
}

function validResponse(frame, session, secret) {
  const request = session.pending?.request;
  if (!request || !exactKeys(frame, ['type', 'version', 'context', 'clientNonce', 'channelNonce', 'requestChallenge', 'requestId', 'operation', 'challenge', 'ok', frame.ok ? 'status' : 'code', 'proof']) ||
    frame.type !== 'result' || !sameChannel(frame, session) || frame.requestChallenge !== request.challenge ||
    frame.requestId !== request.requestId || frame.operation !== request.operation || !validNonce(frame.challenge) ||
    typeof frame.ok !== 'boolean' || (frame.ok ? !validStatus(frame.status) : typeof frame.code !== 'string' || !Object.hasOwn(MESSAGES, frame.code))) return false;
  const result = frame.ok ? { ok: true, status: safeStatus(frame.status) } : { ok: false, code: frame.code };
  return validMac(frame.proof, mac(secret, 'response', responseBody(session, request, frame.challenge, result)));
}

function writeFrame(socket, frame, callback) {
  const bytes = Buffer.from(`${JSON.stringify(frame)}\n`);
  if (bytes.length > MAX_FRAME_BYTES) throw new SharedProxyError('PROXY_IPC_FAILED');
  socket.write(bytes, callback);
}

function readFrames(socket, onFrame, onInvalid) {
  let pending = Buffer.alloc(0);
  socket.on('data', chunk => {
    if (pending.length + chunk.length > MAX_FRAME_BYTES) { onInvalid(); return; }
    pending = Buffer.concat([pending, chunk]);
    while (pending.length) {
      const boundary = pending.indexOf(10);
      if (boundary < 0) break;
      const bytes = pending.subarray(0, boundary);
      pending = pending.subarray(boundary + 1);
      let frame;
      try {
        frame = JSON.parse(bytes.toString('utf8'));
        if (!frame || typeof frame !== 'object' || Array.isArray(frame)) throw new Error();
      } catch { onInvalid(); return; }
      onFrame(frame);
      if (socket.destroyed) return;
    }
  });
}

function bounded(promise, milliseconds, signal, cancellationCode = 'PROXY_CANCELLED') {
  if (signal?.aborted) return Promise.reject(new SharedProxyError(cancellationCode));
  return new Promise((resolvePromise, rejectPromise) => {
    let finished = false;
    const finish = (error, value) => {
      if (finished) return;
      finished = true;
      clearTimeout(timer);
      signal?.removeEventListener('abort', cancelled);
      if (error) rejectPromise(error); else resolvePromise(value);
    };
    const cancelled = () => finish(new SharedProxyError(cancellationCode));
    const timer = setTimeout(() => finish(new SharedProxyError('PROXY_TIMEOUT')), milliseconds);
    signal?.addEventListener('abort', cancelled, { once: true });
    promise.then(value => finish(null, value), error => finish(error));
    if (signal?.aborted) cancelled();
  });
}

function delay(milliseconds, signal) {
  return new Promise((resolvePromise, rejectPromise) => {
    const cancelled = () => { clearTimeout(timer); signal.removeEventListener('abort', cancelled); rejectPromise(new SharedProxyError('PROXY_CLOSED')); };
    const timer = setTimeout(() => { signal?.removeEventListener('abort', cancelled); resolvePromise(); }, milliseconds);
    signal?.addEventListener('abort', cancelled, { once: true });
    if (signal?.aborted) cancelled();
  });
}

async function defaultSpawnDaemon({ daemonEntry, config, socketPath, idleMs, runtimeDirectory, signal }) {
  if (typeof daemonEntry !== 'string' || !isAbsolute(daemonEntry)) throw new SharedProxyError('PROXY_CONFIG');
  if (signal?.aborted) throw new SharedProxyError('PROXY_CLOSED');
  await mkdir(runtimeDirectory, { recursive: true, mode: 0o700 });
  if (signal?.aborted) throw new SharedProxyError('PROXY_CLOSED');
  const bootstrap = Buffer.from(`${JSON.stringify({ config, socketPath, idleMs })}\n`);
  if (bootstrap.length > MAX_FRAME_BYTES) throw new SharedProxyError('PROXY_CONFIG');
  const child = spawn(process.execPath, [daemonEntry, '--proxy-daemon'], {
    cwd: runtimeDirectory, detached: true, windowsHide: true, stdio: ['pipe', 'ignore', 'ignore'],
  });
  await new Promise((resolvePromise, rejectPromise) => {
    const failed = () => rejectPromise(new SharedProxyError('PROXY_START_FAILED'));
    child.once('error', failed);
    child.stdin.on('error', () => {});
    child.once('spawn', () => {
      child.stdin.end(bootstrap, () => { child.unref(); resolvePromise(); });
    });
  });
  return child;
}

/** 各MCPは認証済みpipeだけを所有し、共有中継の寿命はdaemonが管理する。 */
export function createSharedProxyManager({
  daemonEntry, spawnDaemon = defaultSpawnDaemon, daemonIdleMs = 300_000,
  ipcTimeoutMs = 10_000, startupTimeoutMs = 20_000, pollDelayMs = 250, maxPollAttempts = 30,
  socketPathFactory = defaultSocketPath, deriveSecret = defaultDeriveSecret, connect = net.connect,
  runtimeDirectory = join(process.env.LOCALAPPDATA || tmpdir(), 'Plumivox', 'runtime'),
} = {}) {
  if (![daemonIdleMs, ipcTimeoutMs, startupTimeoutMs, pollDelayMs, maxPollAttempts].every(value => Number.isInteger(value) && value > 0) ||
    maxPollAttempts > 30 || ![spawnDaemon, socketPathFactory, deriveSecret, connect].every(value => typeof value === 'function') ||
    !isAbsolute(runtimeDirectory)) throw new SharedProxyError('PROXY_CONFIG');
  let closed = false;
  let lastRecord = null;
  const records = new Map();
  const shutdown = new AbortController();
  const check = (record, epoch = record.epoch) => {
    if (closed) throw new SharedProxyError('PROXY_CLOSED');
    if (record.epoch !== epoch) throw new SharedProxyError('PROXY_STOPPED');
  };
  function recordFor(config) {
    const snapshot = snapshotConfig(config);
    const id = configId(snapshot);
    if (!records.has(id)) records.set(id, { config: snapshot, path: socketPathFactory(snapshot), key: null, keyPromise: null, session: null, connecting: null, starting: null, stopping: null, epoch: 0, spawned: false });
    const record = records.get(id);
    if (typeof record.path !== 'string' || !record.path) throw new SharedProxyError('PROXY_CONFIG');
    lastRecord = record;
    return record;
  }
  async function keyFor(record) {
    if (record.key) return record.key;
    if (!record.keyPromise) {
      record.keyPromise = Promise.resolve().then(() => deriveSecret(record.config)).then(secret => {
        const key = secretBytes(secret);
        if (closed) { key.fill(0); throw new SharedProxyError('PROXY_CLOSED'); }
        record.key = key;
        return key;
      }).finally(() => { record.keyPromise = null; });
    }
    return record.keyPromise;
  }
  function closeSession(record, session, code = 'PROXY_IPC_FAILED') {
    if (!session || session.closed) return;
    session.closed = true;
    clearTimeout(session.handshakeTimer);
    clearTimeout(session.requestTimer);
    session.handshakeReject?.(new SharedProxyError(code));
    session.pending?.reject(new SharedProxyError(code));
    session.pending = null;
    session.socket.destroy();
    if (record.session === session) record.session = null;
  }
  async function connectSession(record, milliseconds = ipcTimeoutMs) {
    check(record);
    if (record.connecting) return record.connecting;
    if (record.session && !record.session.closed) return record.session;
    const epoch = record.epoch;
    record.connecting = (async () => {
      const key = await bounded(keyFor(record), milliseconds, shutdown.signal, 'PROXY_CLOSED');
      check(record, epoch);
      let socket;
      try { socket = connect({ path: record.path }); } catch { throw new SharedProxyError('PROXY_DAEMON_UNAVAILABLE'); }
      const session = {
        socket, context: configId(record.config), clientNonce: nonce(), channelNonce: null, requestId: 0,
        challenge: null, pending: null, closed: false, queue: Promise.resolve(),
        handshakeTimer: null, requestTimer: null, handshakeReject: null,
      };
      record.session = session;
      await new Promise((resolvePromise, rejectPromise) => {
        session.handshakeReject = rejectPromise;
        session.handshakeTimer = setTimeout(() => closeSession(record, session, 'PROXY_TIMEOUT'), milliseconds);
        socket.on('error', error => closeSession(record, session,
          error?.code === 'ENOENT' || error?.code === 'ECONNREFUSED' ? 'PROXY_DAEMON_UNAVAILABLE' : 'PROXY_IPC_FAILED'));
        socket.on('close', () => closeSession(record, session));
        socket.once('connect', () => {
          if (session.closed) return;
          try { writeFrame(socket, { type: 'hello', version: 2, context: session.context, clientNonce: session.clientNonce }); }
          catch { closeSession(record, session); }
        });
        readFrames(socket, frame => {
          if (session.closed) return;
          if (session.channelNonce === null) {
            if (!validChallenge(frame, session, key)) { closeSession(record, session, 'PROXY_IPC_AUTH_FAILED'); return; }
            clearTimeout(session.handshakeTimer);
            session.handshakeReject = null;
            session.channelNonce = frame.channelNonce;
            session.challenge = frame.challenge;
            resolvePromise();
          } else {
            if (!validResponse(frame, session, key)) { closeSession(record, session, 'PROXY_IPC_AUTH_FAILED'); return; }
            const pending = session.pending;
            session.pending = null;
            clearTimeout(session.requestTimer);
            session.challenge = frame.challenge;
            if (!frame.ok) pending.reject(new SharedProxyError(frame.code));
            else {
              try { pending.resolve(safeStatus(frame.status)); } catch (error) { pending.reject(error); closeSession(record, session); }
            }
          }
        }, () => closeSession(record, session));
      });
      check(record, epoch);
      return session;
    })().finally(() => { record.connecting = null; });
    return record.connecting;
  }
  function rpc(record, session, operation, milliseconds = ipcTimeoutMs) {
    const epoch = record.epoch;
    const operationPromise = session.queue.then(() => {
      check(record, epoch);
      if (session.closed || !session.challenge) throw new SharedProxyError('PROXY_IPC_FAILED');
      return new Promise((resolvePromise, rejectPromise) => {
        session.requestId += 1;
        const request = requestBody(session, session.requestId, operation);
        session.pending = { resolve: resolvePromise, reject: rejectPromise, request };
        session.requestTimer = setTimeout(() => closeSession(record, session, 'PROXY_TIMEOUT'), milliseconds);
        const frame = { ...request, hmac: mac(record.key, 'request', request) };
        session.challenge = null;
        try { writeFrame(session.socket, frame); } catch { closeSession(record, session); }
      });
    });
    session.queue = operationPromise.catch(() => {});
    return operationPromise;
  }
  async function startRecord(record) {
    if (record.stopping) await record.stopping;
    check(record);
    if (record.starting) return record.starting;
    const epoch = record.epoch;
    const deadline = Date.now() + startupTimeoutMs;
    let spawned = false;
    record.starting = (async () => {
      for (let attempt = 0; attempt < maxPollAttempts; attempt += 1) {
        check(record, epoch);
        const remaining = deadline - Date.now();
        if (remaining <= 0) throw new SharedProxyError('PROXY_TIMEOUT');
        try {
          const session = await connectSession(record, Math.min(ipcTimeoutMs, remaining));
          check(record, epoch);
          const status = await rpc(record, session, 'ready', Math.max(1, Math.min(ipcTimeoutMs, deadline - Date.now())));
          if (status.state !== 'ready' || !status.managed) throw new SharedProxyError('PROXY_CONNECT_FAILED');
          return status;
        } catch (error) {
          check(record, epoch);
          if (!['PROXY_DAEMON_UNAVAILABLE', 'PROXY_IPC_FAILED'].includes(error?.code)) throw error;
          if (!spawned) {
            spawned = true;
            record.spawned = true;
            await bounded(Promise.resolve().then(() => spawnDaemon({
              daemonEntry, config: record.config, socketPath: record.path, idleMs: daemonIdleMs, runtimeDirectory, signal: shutdown.signal,
            })), Math.max(1, deadline - Date.now()), shutdown.signal, 'PROXY_CLOSED');
            check(record, epoch);
          }
          await delay(Math.min(pollDelayMs, Math.max(1, deadline - Date.now())), shutdown.signal);
        }
      }
      throw new SharedProxyError('PROXY_DAEMON_UNAVAILABLE');
    })().finally(() => { record.starting = null; });
    return record.starting;
  }
  async function ensureReady(config, { signal } = {}) {
    if (config == null) return { managed: false, state: 'external' };
    if (closed) throw new SharedProxyError('PROXY_CLOSED');
    if (signal?.aborted) throw new SharedProxyError('PROXY_CANCELLED');
    const record = recordFor(config);
    return bounded(startRecord(record), startupTimeoutMs, signal);
  }
  async function getStatus() {
    if (!lastRecord || closed) return stoppedStatus();
    try { return await rpc(lastRecord, await connectSession(lastRecord), 'status'); }
    catch (error) {
      if (error?.code === 'PROXY_DAEMON_UNAVAILABLE') return stoppedStatus();
      throw error instanceof SharedProxyError ? error : new SharedProxyError(safeCode(error));
    }
  }
  async function stop() {
    if (!lastRecord || closed) return stoppedStatus();
    const record = lastRecord;
    if (record.stopping) return record.stopping;
    record.epoch += 1;
    closeSession(record, record.session, 'PROXY_STOPPED');
    record.stopping = (async () => {
      // 既に進行中のspawnを待ち、新規daemonは起動しない。
      await record.starting?.catch(() => {});
      const deadline = Date.now() + startupTimeoutMs;
      try {
        for (let attempt = 0; attempt < maxPollAttempts; attempt += 1) {
          try {
            const session = await connectSession(record, Math.max(1, Math.min(ipcTimeoutMs, deadline - Date.now())));
            return await rpc(record, session, 'stop');
          } catch (error) {
            if (error?.code !== 'PROXY_DAEMON_UNAVAILABLE') throw error;
            if (!record.spawned) return stoppedStatus();
            if (Date.now() >= deadline) throw new SharedProxyError('PROXY_TIMEOUT');
            await delay(pollDelayMs, shutdown.signal);
          }
        }
        throw new SharedProxyError('PROXY_DAEMON_UNAVAILABLE');
      } catch (error) {
        throw error instanceof SharedProxyError ? error : new SharedProxyError(safeCode(error));
      } finally {
        closeSession(record, record.session, 'PROXY_STOPPED');
        record.spawned = false;
      }
    })().finally(() => { record.stopping = null; });
    return record.stopping;
  }
  async function close() {
    if (closed) return stoppedStatus();
    closed = true;
    shutdown.abort();
    for (const record of records.values()) {
      record.epoch += 1;
      closeSession(record, record.session, 'PROXY_CLOSED');
      record.key?.fill(0);
      record.key = null;
    }
    await Promise.allSettled([...records.values()].flatMap(record => [record.starting, record.connecting, record.stopping].filter(Boolean)));
    records.clear();
    lastRecord = null;
    return stoppedStatus();
  }
  return { ensureReady, start: ensureReady, stop, close, getStatus };
}

/** stdinのbootstrapをCLIが検証してから呼ぶ。設定・鍵・生エラーをIPCへ返さない。 */
export async function runProxyDaemon({ config, socketPath, secret, managerFactory = createManagedProxyManager, idleMs = 300_000 } = {}) {
  const snapshot = snapshotConfig(config);
  if (process.platform !== 'win32') throw new SharedProxyError('PROXY_UNSUPPORTED');
  if (typeof socketPath !== 'string' || socketPath !== defaultSocketPath(snapshot) ||
    typeof managerFactory !== 'function' || !Number.isInteger(idleMs) || idleMs < 1) throw new SharedProxyError('PROXY_CONFIG');
  const key = secretBytes(secret ?? await defaultDeriveSecret(snapshot));
  const manager = managerFactory();
  const connections = new Set();
  let closing = null;
  let stopping = false;
  let idleTimer = null;
  let stopTimer = null;
  const server = net.createServer(socket => {
    if (stopping || connections.size >= 64) { socket.on('error', () => {}); socket.destroy(); return; }
    let challenge = null;
    const peer = {
      socket, context: configId(snapshot), clientNonce: null, channelNonce: null,
      challenge: null, requestId: 0, authenticated: false, busy: false, timer: null, controller: null,
    };
    connections.add(peer);
    peer.timer = setTimeout(() => socket.destroy(), 3_000);
    peer.timer.unref();
    socket.on('error', () => {});
    socket.on('close', () => {
      clearTimeout(peer.timer);
      peer.controller?.abort();
      connections.delete(peer);
      scheduleIdle(peer.authenticated);
    });
    readFrames(socket, frame => {
      if (stopping || peer.busy) { socket.destroy(); return; }
      if (peer.channelNonce === null) {
        if (!exactKeys(frame, ['type', 'version', 'context', 'clientNonce']) || frame.type !== 'hello' || frame.version !== 2 ||
          frame.context !== peer.context || !validNonce(frame.clientNonce)) { socket.destroy(); return; }
        peer.clientNonce = frame.clientNonce;
        peer.channelNonce = nonce();
        challenge = nonce();
        const body = challengeBody(peer, challenge);
        writeFrame(socket, { ...body, proof: mac(key, 'server', body) });
        return;
      }
      const request = { ...requestBody({ ...peer, challenge }, frame.requestId, frame.operation) };
      if (!exactKeys(frame, ['type', 'version', 'context', 'clientNonce', 'channelNonce', 'challenge', 'requestId', 'operation', 'hmac']) ||
        frame.type !== 'request' || !sameChannel(frame, peer) || !['ready', 'status', 'stop'].includes(frame.operation) ||
        !challenge || frame.challenge !== challenge || !Number.isSafeInteger(frame.requestId) || frame.requestId !== peer.requestId + 1 ||
        !validMac(frame.hmac, mac(key, 'request', request))) {
        socket.destroy();
        return;
      }
      challenge = null;
      peer.requestId = frame.requestId;
      peer.busy = true;
      peer.authenticated = true;
      clearTimeout(peer.timer);
      clearTimeout(idleTimer);
      idleTimer = null;
      void handle(request);
    }, () => socket.destroy());

    async function handle(request) {
      const operation = request.operation;
      const controller = new AbortController();
      peer.controller = controller;
      let timedOut = false;
      peer.timer = setTimeout(() => { timedOut = true; controller.abort(); }, 10_000);
      try {
        let status;
        if (operation === 'ready') status = await bounded(Promise.resolve(manager.ensureReady(snapshot, { signal: controller.signal })), 10_000, controller.signal);
        else if (operation === 'status') status = await bounded(Promise.resolve(manager.getStatus()), 10_000, controller.signal);
        else {
          stopping = true;
          status = await bounded(Promise.resolve(manager.stop()), 10_000, controller.signal);
        }
        if (socket.destroyed) return;
        challenge = nonce();
        const body = responseBody(peer, request, challenge, { ok: true, status: safeStatus(status) });
        writeFrame(socket, { ...body, proof: mac(key, 'response', body) }, operation === 'stop' ? () => { void close(); } : undefined);
      } catch (error) {
        if (!socket.destroyed) {
          challenge = nonce();
          const body = responseBody(peer, request, challenge, { ok: false, code: timedOut ? 'PROXY_TIMEOUT' : safeCode(error) });
          writeFrame(socket, { ...body, proof: mac(key, 'response', body) }, operation === 'stop' ? () => { void close(); } : undefined);
        }
      } finally {
        clearTimeout(peer.timer);
        peer.controller = null;
        peer.busy = false;
        if (operation === 'stop' && !closing) {
          // 停止要求元がreply前に離脱してもdaemonを終了する。
          stopTimer = setTimeout(() => { void close(); }, 100);
          stopTimer.unref();
        }
      }
    }
  });
  function scheduleIdle(authenticatedDeparture = false) {
    if (stopping || closing || [...connections].some(peer => peer.authenticated)) return;
    // 未認証の接続・切断では「無利用」の起点を延長しない。
    if (idleTimer && !authenticatedDeparture) return;
    clearTimeout(idleTimer);
    idleTimer = setTimeout(() => { void close(); }, idleMs);
    idleTimer.unref();
  }
  async function close() {
    if (closing) return closing;
    stopping = true;
    clearTimeout(idleTimer);
    clearTimeout(stopTimer);
    closing = (async () => {
      for (const peer of connections) {
        clearTimeout(peer.timer);
        peer.controller?.abort();
        peer.socket.destroy();
      }
      try { await manager.close(); }
      finally {
        if (server.listening) await new Promise(resolvePromise => server.close(resolvePromise));
        key.fill(0);
      }
    })();
    return closing;
  }
  server.on('error', () => { if (server.listening) void close(); });
  try {
    await new Promise((resolvePromise, rejectPromise) => {
      const failed = () => rejectPromise(new SharedProxyError('PROXY_PORT_BUSY'));
      server.once('error', failed);
      server.listen({ path: socketPath, readableAll: false, writableAll: false }, () => {
        server.removeListener('error', failed);
        resolvePromise();
      });
    });
  } catch (error) {
    await close();
    throw error instanceof SharedProxyError ? error : new SharedProxyError('PROXY_START_FAILED');
  }
  scheduleIdle();
  return { close, getStatus: async () => safeStatus(await manager.getStatus()), socketPath };
}
