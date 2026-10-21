import net from 'node:net';
import { timingSafeEqual } from 'node:crypto';
import { Client } from 'ssh2';
import { createEgressServer } from './proxy-egress.mjs';
import { normalizeProxyConfig, withProxyPrivateKey } from './proxy-config.mjs';

const MAX_CONNECTIONS = 16;
const MAX_RECONNECTS = 6;
const SSH_SHUTDOWN_TIMEOUT_MS = 1_000;
const MESSAGES = {
  PROXY_CONFIG: '管理プロキシの設定が不正です。',
  PROXY_KEY_UNAVAILABLE: '管理プロキシの秘密鍵を読み取れません。',
  PROXY_KEY_TOO_LARGE: '管理プロキシの秘密鍵がサイズ上限を超えています。',
  PROXY_HOST_KEY_MISMATCH: '管理プロキシの接続先ホスト鍵が一致しません。',
  PROXY_AUTH_FAILED: '管理プロキシのSSH認証に失敗しました。',
  PROXY_PORT_BUSY: '管理プロキシのポートは使用中です。',
  PROXY_REMOTE_BIND_FAILED: '管理プロキシの転送ポートを確保できません。',
  PROXY_CONNECT_FAILED: '管理プロキシに接続できません。',
  PROXY_TIMEOUT: '管理プロキシの準備が時間内に完了しませんでした。',
  PROXY_CANCELLED: '管理プロキシの準備待機を取り消しました。',
  PROXY_STOPPED: '管理プロキシを停止しました。',
  PROXY_CLOSED: '管理プロキシの管理を終了しました。',
};
const PERMANENT = new Set([
  'PROXY_CONFIG', 'PROXY_KEY_UNAVAILABLE', 'PROXY_KEY_TOO_LARGE',
  'PROXY_HOST_KEY_MISMATCH', 'PROXY_AUTH_FAILED', 'PROXY_PORT_BUSY',
  'PROXY_REMOTE_BIND_FAILED',
]);

export class ManagedProxyError extends Error {
  constructor(code) {
    const safeCode = Object.hasOwn(MESSAGES, code) ? code : 'PROXY_CONNECT_FAILED';
    super(MESSAGES[safeCode]);
    this.name = 'ManagedProxyError';
    this.code = safeCode;
  }
}

function normalizedConfig(config) {
  if (config == null) return null;
  const snapshot = normalizeProxyConfig(config);
  if (!snapshot) throw new ManagedProxyError('PROXY_CONFIG');
  return snapshot;
}

async function readPrivateKey(path) {
  return withProxyPrivateKey(path, ManagedProxyError, bytes => Buffer.from(bytes));
}

/** SSH接続だけを再接続する。利用者の投稿・ログインは再送しない。 */
export function createManagedProxyManager({
  createClient = () => new Client(), egressFactory = createEgressServer,
  readyTimeoutMs = 20_000, reconnectDelayMs = 1_000,
} = {}) {
  if (typeof createClient !== 'function' || typeof egressFactory !== 'function' ||
    !Number.isInteger(readyTimeoutMs) || readyTimeoutMs < 1 ||
    !Number.isInteger(reconnectDelayMs) || reconnectDelayMs < 1) throw new ManagedProxyError('PROXY_CONFIG');
  let generation = 0;
  let desired = null;
  let context = null;
  let state = 'stopped';
  let lastErrorCode = null;
  let closed = false;
  let reconnectTimer = null;
  let retries = 0;
  let transition = Promise.resolve();
  const waiters = new Set();

  const current = ctx => !closed && ctx === context && !ctx.disposed && ctx.generation === generation && desired !== null;
  const getStatus = () => ({
    state, activeConnections: context?.connections.size ?? 0,
    lastErrorCode, managed: desired !== null,
  });
  function settleWaiters(code = null) {
    for (const waiter of waiters) waiter.finish(code);
  }
  function clearReconnect() {
    clearTimeout(reconnectTimer);
    reconnectTimer = null;
  }
  function closeSsh(ctx) {
    const client = ctx.client;
    if (!client || ctx.sshClosed) return Promise.resolve();
    return new Promise(resolve => {
      let timer;
      let finished = false;
      let forcing = false;
      const finish = () => {
        if (finished) return;
        finished = true;
        clearTimeout(timer);
        client.removeListener('close', finish);
        resolve();
      };
      const force = () => {
        if (finished || forcing) return;
        forcing = true;
        clearTimeout(timer);
        timer = setTimeout(finish, SSH_SHUTDOWN_TIMEOUT_MS);
        try { client.destroy(); } catch { finish(); }
      };
      client.once('close', finish);
      if (ctx.sshReady && ctx.forwardRequested && !ctx.sshEnded) {
        // 生きている自己接続は、remote listenerの解除応答を受けてから切断する。
        // forwardInの応答が未着でも、同じSSH接続の要求順で解除を予約する。
        timer = setTimeout(force, SSH_SHUTDOWN_TIMEOUT_MS);
        try {
          client.unforwardIn('127.0.0.1', ctx.config.remotePort, () => {
            if (finished || forcing) return;
            try { client.end(); } catch { force(); }
          });
        } catch { force(); }
      } else {
        // 未認証・切断済みでは解除応答を待たず、自分の接続だけを破棄する。
        force();
      }
    });
  }
  function cleanup(ctx) {
    if (!ctx) return Promise.resolve();
    if (ctx.cleanup) return ctx.cleanup;
    ctx.disposed = true;
    clearTimeout(ctx.readyTimer);
    for (const connection of [...ctx.connections]) connection.stop();
    ctx.key?.fill(0);
    ctx.key = null;
    ctx.cleanup = Promise.all([
      closeSsh(ctx), Promise.resolve().then(() => ctx.egress?.close()).catch(() => {}),
    ]).then(() => {});
    return ctx.cleanup;
  }
  function failureCode(error, ctx) {
    if (ctx.hostRejected) return 'PROXY_HOST_KEY_MISMATCH';
    if (error instanceof ManagedProxyError) return error.code;
    if (error?.code === 'PROXY_PORT_BUSY' || error?.code === 'EADDRINUSE') return 'PROXY_PORT_BUSY';
    if (error?.level === 'client-authentication') return 'PROXY_AUTH_FAILED';
    return 'PROXY_CONNECT_FAILED';
  }
  function failed(ctx, code) {
    if (!current(ctx)) return;
    lastErrorCode = code;
    const permanent = PERMANENT.has(code) || retries >= MAX_RECONNECTS;
    state = permanent ? 'failed' : 'reconnecting';
    const stopped = cleanup(ctx);
    if (permanent) {
      settleWaiters(code);
      return;
    }
    const delay = Math.min(reconnectDelayMs * 2 ** retries, 30_000);
    retries += 1;
    const expectedGeneration = generation;
    reconnectTimer = setTimeout(() => {
      reconnectTimer = null;
      transition = stopped.then(() => {
        if (closed || generation !== expectedGeneration || !desired) return;
        launch(expectedGeneration);
      });
    }, delay);
    reconnectTimer.unref();
  }
  function incoming(ctx, details, accept, reject) {
    const loopback = address => typeof address === 'string' && net.isIP(address) === 4 && address.startsWith('127.');
    if (!current(ctx) || state !== 'ready' || details?.destIP !== '127.0.0.1' ||
      details?.destPort !== ctx.config.remotePort || !loopback(details?.srcIP) || ctx.connections.size >= MAX_CONNECTIONS) {
      try { reject(); } catch { /* 遅延したSSH要求は破棄する。 */ }
      return;
    }
    let channel;
    let socket;
    let lifetime;
    let handshake;
    let done = false;
    const connection = { stop: () => {
      if (done) return;
      done = true;
      clearTimeout(lifetime);
      clearTimeout(handshake);
      ctx.connections.delete(connection);
      socket?.destroy();
      channel?.destroy();
    } };
    ctx.connections.add(connection);
    try {
      channel = accept();
      channel.on('error', connection.stop);
      channel.on('close', connection.stop);
      channel.on('end', connection.stop);
      // acceptが同期的に停止を誘発しても、新たなローカルsocketを作らない。
      if (done || !current(ctx)) { channel.destroy(); connection.stop(); return; }
      socket = net.connect({ host: '127.0.0.1', port: ctx.config.localPort, family: 4 });
      socket.on('error', connection.stop);
      socket.on('close', connection.stop);
      socket.on('end', connection.stop);
      socket.setTimeout(60_000, connection.stop);
      handshake = setTimeout(connection.stop, 15_000);
      lifetime = setTimeout(connection.stop, 10 * 60_000);
      handshake.unref();
      lifetime.unref();
      socket.once('connect', () => {
        if (done || !current(ctx)) { connection.stop(); return; }
        clearTimeout(handshake);
        socket.pipe(channel);
        channel.pipe(socket);
      });
    } catch {
      connection.stop();
    }
  }
  function launch(expectedGeneration) {
    const ctx = {
      generation: expectedGeneration, config: desired, client: null, egress: null,
      key: null, disposed: false, cleanup: null, hostRejected: false,
      sshReady: false, sshEnded: false, sshClosed: false, forwardRequested: false,
      connections: new Set(), readyTimer: null, pending: null,
    };
    context = ctx;
    ctx.readyTimer = setTimeout(() => failed(ctx, 'PROXY_TIMEOUT'), readyTimeoutMs);
    ctx.readyTimer.unref();
    ctx.pending = (async () => {
      try {
        ctx.key = await readPrivateKey(ctx.config.privateKeyFile);
        if (!current(ctx)) { ctx.key.fill(0); ctx.key = null; return; }
        ctx.egress = egressFactory({ port: ctx.config.localPort });
        await ctx.egress.start();
        if (!current(ctx)) { await ctx.egress.close(); return; }
        ctx.client = createClient();
        ctx.client.on('error', error => failed(ctx, failureCode(error, ctx)));
        ctx.client.on('end', () => { ctx.sshEnded = true; failed(ctx, 'PROXY_CONNECT_FAILED'); });
        ctx.client.on('close', () => { ctx.sshClosed = true; failed(ctx, 'PROXY_CONNECT_FAILED'); });
        ctx.client.on('tcp connection', (details, accept, reject) => incoming(ctx, details, accept, reject));
        ctx.client.once('ready', () => {
          if (!current(ctx)) return;
          ctx.sshReady = true;
          try {
            ctx.forwardRequested = true;
            ctx.client.forwardIn('127.0.0.1', ctx.config.remotePort, (error, boundPort) => {
              if (!current(ctx)) return;
              if (error || (boundPort !== undefined && boundPort !== ctx.config.remotePort)) {
                failed(ctx, 'PROXY_REMOTE_BIND_FAILED');
                return;
              }
              clearTimeout(ctx.readyTimer);
              state = 'ready';
              lastErrorCode = null;
              retries = 0;
              settleWaiters();
            });
          } catch {
            failed(ctx, 'PROXY_REMOTE_BIND_FAILED');
          }
        });
        const pinned = Buffer.from(ctx.config.hostKeySha256.slice(7), 'base64');
        const connectionConfig = {
          host: ctx.config.sshHost, port: ctx.config.sshPort, username: ctx.config.sshUser,
          privateKey: ctx.key, hostHash: 'sha256',
          hostVerifier: hash => {
            if (!current(ctx)) return false;
            const valid = typeof hash === 'string' && /^[a-f0-9]{64}$/i.test(hash) &&
              timingSafeEqual(Buffer.from(hash, 'hex'), pinned);
            if (!valid) {
              ctx.hostRejected = true;
              // verifier内でSSHをdestroyせず、当該ハンドシェイクの終了後に清掃する。
              queueMicrotask(() => failed(ctx, 'PROXY_HOST_KEY_MISMATCH'));
            }
            return valid;
          },
          authHandler: ['publickey'], tryKeyboard: false,
          readyTimeout: readyTimeoutMs, keepaliveInterval: 15_000, keepaliveCountMax: 3,
        };
        try {
          ctx.client.connect(connectionConfig);
        } catch {
          // 有効なポート等を確認済み。同期例外は鍵解析・認証設定失敗として再試行しない。
          failed(ctx, 'PROXY_AUTH_FAILED');
        }
      } catch (error) {
        failed(ctx, failureCode(error, ctx));
      }
    })();
  }
  function waitForReady(signal) {
    if (signal?.aborted) return Promise.reject(new ManagedProxyError('PROXY_CANCELLED'));
    if (state === 'ready') return Promise.resolve(getStatus());
    if (state === 'failed') return Promise.reject(new ManagedProxyError(lastErrorCode));
    return new Promise((resolve, reject) => {
      let timer;
      const cancelled = () => waiter.finish('PROXY_CANCELLED');
      const waiter = { finish: code => {
        if (!waiters.delete(waiter)) return;
        clearTimeout(timer);
        signal?.removeEventListener('abort', cancelled);
        if (code) reject(new ManagedProxyError(code));
        else resolve(getStatus());
      } };
      waiters.add(waiter);
      timer = setTimeout(() => waiter.finish('PROXY_TIMEOUT'), readyTimeoutMs);
      signal?.addEventListener('abort', cancelled, { once: true });
      if (signal?.aborted) cancelled();
    });
  }
  async function stop() {
    generation += 1;
    clearReconnect();
    desired = null;
    state = 'stopped';
    lastErrorCode = null;
    settleWaiters(closed ? 'PROXY_CLOSED' : 'PROXY_STOPPED');
    const previous = context;
    context = null;
    const previousTransition = transition;
    transition = Promise.all([cleanup(previous), previous?.pending, previousTransition]).then(() => {});
    await transition;
    return getStatus();
  }
  async function ensureReady(config, { signal } = {}) {
    // 外部プロキシの呼出しは、別アカウントが共有中継を使用中でも管理状態に触れない。
    if (config == null) return { managed: false, state: 'external' };
    if (closed) throw new ManagedProxyError('PROXY_CLOSED');
    if (signal?.aborted) throw new ManagedProxyError('PROXY_CANCELLED');
    const next = normalizedConfig(config);
    if (JSON.stringify(next) !== JSON.stringify(desired)) {
      generation += 1;
      const expectedGeneration = generation;
      clearReconnect();
      settleWaiters('PROXY_STOPPED');
      const previous = context;
      context = null;
      desired = next;
      state = 'starting';
      lastErrorCode = null;
      retries = 0;
      transition = Promise.all([transition, cleanup(previous), previous?.pending]).then(() => {
        if (!closed && generation === expectedGeneration && desired !== null) launch(expectedGeneration);
      });
    }
    return waitForReady(signal);
  }
  async function close() {
    closed = true;
    return stop();
  }
  return { ensureReady, start: ensureReady, stop, close, getStatus };
}
