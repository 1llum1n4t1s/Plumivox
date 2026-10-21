import net from 'node:net';
import { lookup as defaultLookup } from 'node:dns/promises';

const MAX_CONNECTIONS = 16;
const MAX_HEADER = 8 * 1024;
const MAX_HEAD = 64 * 1024;
const HANDSHAKE_MS = 15_000;
const IDLE_MS = 60_000;
const LIFETIME_MS = 10 * 60_000;
const DOMAINS = ['x.com', 'twitter.com', 'twimg.com'];
const MESSAGES = {
  PROXY_CONFIG: 'プロキシ出口の設定が不正です。',
  PROXY_PORT_BUSY: 'プロキシ出口のポートは使用中です。',
  PROXY_START_FAILED: 'プロキシ出口を起動できませんでした。',
};

export class ProxyEgressError extends Error {
  constructor(code) {
    const safeCode = Object.hasOwn(MESSAGES, code) ? code : 'PROXY_START_FAILED';
    super(MESSAGES[safeCode]);
    this.name = 'ProxyEgressError';
    this.code = safeCode;
  }
}

function publicAddress(address, family) {
  if (typeof address !== 'string' || address.includes('%') || net.isIP(address) !== family) return false;
  if (family === 4) {
    const [a, b, c] = address.split('.').map(Number);
    return a > 0 && a < 224 && a !== 10 && a !== 127 &&
      !(a === 100 && b >= 64 && b <= 127) &&
      !(a === 169 && b === 254) && !(a === 172 && b >= 16 && b <= 31) &&
      !(a === 192 && ((b === 0 && (c === 0 || c === 2)) ||
        (b === 88 && c === 99) || b === 168)) &&
      !(a === 198 && (b === 18 || b === 19 || (b === 51 && c === 100))) &&
      !(a === 203 && b === 0 && c === 113);
  }
  if (family !== 6) return false;
  // 公開global-unicastだけを許可し、トンネル・文書用等の特殊範囲も除外する。
  const halves = address.toLowerCase().split('::');
  const left = halves[0] ? halves[0].split(':') : [];
  const right = halves.length === 2 && halves[1] ? halves[1].split(':') : [];
  const words = halves.length === 2
    ? [...left, ...Array(8 - left.length - right.length).fill('0'), ...right]
    : left;
  const [a, b] = words.map(word => Number.parseInt(word, 16));
  return a >= 0x2000 && a <= 0x3fff && a !== 0x2002 && a !== 0x3fff &&
    !(a === 0x2001 && (b <= 0x01ff || b === 0x0db8));
}

function targetFromHeader(bytes) {
  if (bytes.some(byte => byte > 126 || (byte < 32 && byte !== 13 && byte !== 10 && byte !== 9))) return null;
  const lines = bytes.toString('ascii').split('\r\n');
  const first = /^CONNECT ([A-Za-z0-9.-]+):443 HTTP\/1\.[01]$/.exec(lines.shift());
  if (!first) return null;
  const host = first[1].toLowerCase();
  if (host.length > 253 || net.isIP(host) || !host.split('.').every(label =>
    label.length <= 63 && /^[a-z0-9](?:[a-z0-9-]*[a-z0-9])?$/.test(label)) ||
    !DOMAINS.some(domain => host === domain || host.endsWith(`.${domain}`))) return null;
  const seen = new Set();
  for (const line of lines) {
    if (line === '') continue;
    const match = /^([!#$%&'*+.^_`|~0-9A-Za-z-]+):[ \t]*([^\r\n]*)$/.exec(line);
    if (!match) return null;
    const name = match[1].toLowerCase();
    if (seen.has(name)) return null;
    seen.add(name);
    // CONNECTのbodyや二重の宛先解釈を認めない。
    if (name === 'transfer-encoding' || name === 'content-length' ||
      (name === 'host' && match[2].trim().toLowerCase() !== `${host}:443`)) return null;
  }
  return host;
}

/** lookup/connectの注入はローカルE2E専用。本番では標準のDNSとTCPを使う。 */
export function createEgressServer({ port = 43128, lookup = defaultLookup, connect = net.connect } = {}) {
  if (!Number.isInteger(port) || port < 1024 || port > 65535 ||
    typeof lookup !== 'function' || typeof connect !== 'function') throw new ProxyEgressError('PROXY_CONFIG');
  const connections = new Set();
  let running = false;
  let starting = null;
  let closing = null;
  let stopRequested = false;

  const server = net.createServer(client => {
    if (!running || connections.size >= MAX_CONNECTIONS) {
      client.on('error', () => {});
      client.destroy();
      return;
    }
    const controller = new AbortController();
    const state = { upstream: null, stopped: false, rejected: false, established: false, timers: new Set(), stop: null };
    connections.add(state);
    const timer = (callback, milliseconds) => {
      const handle = setTimeout(callback, milliseconds);
      handle.unref();
      state.timers.add(handle);
      return handle;
    };
    const clearTimer = handle => {
      clearTimeout(handle);
      state.timers.delete(handle);
    };
    const stop = () => {
      if (state.stopped) return;
      state.stopped = true;
      controller.abort();
      for (const handle of state.timers) clearTimeout(handle);
      state.timers.clear();
      client.destroy();
      state.upstream?.destroy();
      connections.delete(state);
    };
    state.stop = stop;
    const reject = status => {
      if (state.stopped || state.rejected) return;
      if (state.established) { stop(); return; }
      state.rejected = true;
      controller.abort();
      state.upstream?.destroy();
      // 応答は固定値のみ。書込み待ちも短時間で必ず終了する。
      client.end(`HTTP/1.1 ${status}\r\nConnection: close\r\nContent-Length: 0\r\n\r\n`, stop);
      timer(stop, 1000);
    };
    client.on('error', stop);
    client.on('close', stop);
    client.on('end', stop);
    client.setTimeout(IDLE_MS, stop);
    const handshakeTimer = timer(() => reject('504 Gateway Timeout'), HANDSHAKE_MS);
    timer(stop, LIFETIME_MS);
    let header = Buffer.alloc(0);
    const onData = chunk => {
      if (state.stopped) return;
      // 受信chunk自身が巨大でも、コピーや保持をせず上限で拒否する。
      if (header.length + chunk.length > MAX_HEADER + MAX_HEAD) {
        client.removeListener('data', onData);
        client.pause();
        reject('431 Request Header Fields Too Large');
        return;
      }
      header = header.length ? Buffer.concat([header, chunk]) : chunk;
      const boundary = header.indexOf('\r\n\r\n');
      if (boundary < 0) {
        if (header.length >= MAX_HEADER) {
          client.removeListener('data', onData);
          client.pause();
          reject('431 Request Header Fields Too Large');
        }
        return;
      }
      client.removeListener('data', onData);
      client.pause();
      const head = header.subarray(boundary + 4);
      const host = boundary + 4 <= MAX_HEADER ? targetFromHeader(header.subarray(0, boundary)) : null;
      if (!host || head.length > MAX_HEAD) {
        reject('403 Forbidden');
        return;
      }
      void openTunnel(host, head);
      header = null;
    };
    client.on('data', onData);

    async function openTunnel(host, head) {
      try {
        // lookup自体が取消非対応でも、待機を解除して遅延結果の接続を防ぐ。
        const answers = await new Promise((resolve, rejectLookup) => {
          const cancelled = () => rejectLookup(new Error('cancelled'));
          controller.signal.addEventListener('abort', cancelled, { once: true });
          Promise.resolve().then(() => {
            if (state.stopped || state.rejected) throw new Error('cancelled');
            return lookup(host, { all: true, verbatim: true, signal: controller.signal });
          }).then(resolve, rejectLookup).finally(() => {
            controller.signal.removeEventListener('abort', cancelled);
          });
        });
        if (state.stopped || state.rejected) return;
        if (!Array.isArray(answers) || answers.length === 0 || answers.length > 64 ||
          !answers.every(answer => answer && publicAddress(answer.address, answer.family))) {
          reject('403 Forbidden');
          return;
        }
        const answer = answers[0];
        const upstream = connect({ host: answer.address, port: 443, family: answer.family });
        state.upstream = upstream;
        upstream.on('error', () => reject('502 Bad Gateway'));
        upstream.on('close', stop);
        upstream.on('end', stop);
        upstream.setTimeout(IDLE_MS, stop);
        upstream.once('connect', () => {
          if (state.stopped || state.rejected) return;
          state.established = true;
          clearTimer(handshakeTimer);
          client.write('HTTP/1.1 200 Connection Established\r\n\r\n');
          if (head.length) upstream.write(head);
          upstream.pipe(client);
          client.pipe(upstream);
          client.resume();
        });
      } catch {
        if (!state.stopped) reject('502 Bad Gateway');
      }
    }
  });
  // 起動後のエラーにも生エラーを公開しない。
  server.on('error', () => {
    if (running) {
      running = false;
      for (const state of connections) state.stop();
      if (server.listening) server.close();
    }
  });

  async function start() {
    if (closing) await closing;
    if (running) return;
    if (starting) return starting;
    stopRequested = false;
    starting = new Promise((resolve, rejectStart) => {
      const failed = error => {
        server.removeListener('listening', listening);
        rejectStart(new ProxyEgressError(error?.code === 'EADDRINUSE' ? 'PROXY_PORT_BUSY' : 'PROXY_START_FAILED'));
      };
      const listening = () => {
        server.removeListener('error', failed);
        running = !stopRequested;
        resolve();
      };
      server.once('error', failed);
      server.once('listening', listening);
      try {
        server.listen({ host: '127.0.0.1', port, backlog: MAX_CONNECTIONS });
      } catch (error) {
        server.removeListener('error', failed);
        failed(error);
      }
    });
    try { await starting; } finally { starting = null; }
  }

  async function close() {
    if (closing) return closing;
    stopRequested = true;
    running = false;
    closing = (async () => {
      if (starting) await starting.catch(() => {});
      for (const state of connections) state.stop();
      if (server.listening) await new Promise(resolve => server.close(resolve));
    })();
    try { await closing; } finally { closing = null; }
  }

  return { start, close, getStatus: () => ({ running, activeConnections: connections.size }) };
}
