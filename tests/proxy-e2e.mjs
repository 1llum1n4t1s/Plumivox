import assert from 'node:assert/strict';
import net from 'node:net';
import http from 'node:http';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { mkdir, readFile, writeFile, realpath, stat } from 'node:fs/promises';
import { generateKeyPairSync, createHash, randomUUID } from 'node:crypto';
import ssh2 from 'ssh2';
const { Server: SshServer, utils: sshUtils } = ssh2;
import { Client } from '@modelcontextprotocol/sdk/client/index.js';
import { StdioClientTransport } from '@modelcontextprotocol/sdk/client/stdio.js';

const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const output = path.resolve(process.env.PLUMIVOX_PROXY_E2E_OUTPUT_DIR || path.join(os.tmpdir(), 'Codex', 'Plumivox', randomUUID()));
const inside = (child, parent) => { const relative = path.relative(parent, child); return relative === '' || (!relative.startsWith('..') && !path.isAbsolute(relative)); };
let ancestor = output;
while (true) { try { await stat(ancestor); break; } catch (error) { if (error.code !== 'ENOENT') throw error; ancestor = path.dirname(ancestor); } }
const resolved = path.join(await realpath(ancestor), path.relative(ancestor, output));
for (const forbidden of [repo, path.join(os.homedir(), '.codex'), ...(process.env.CODEX_HOME ? [path.resolve(process.env.CODEX_HOME)] : [])]) {
  assert.ok(!inside(output, forbidden) && !inside(resolved, forbidden), 'Evidence must be outside repository and Codex user directory');
}
await mkdir(output, { recursive: true });
const results = [], trace = [], diagnostics = [];
let diagnosticsLeaked = false;
const privateKey = generateKeyPairSync('rsa', { modulusLength: 2048, privateKeyEncoding: { type: 'pkcs1', format: 'pem' }, publicKeyEncoding: { type: 'spki', format: 'pem' } }).privateKey;
const parsedKey = sshUtils.parseKey(privateKey);
const pin = 'SHA256:' + createHash('sha256').update(parsedKey.getPublicSSH()).digest('base64').replace(/=+$/, '');
const keyPath = path.join(output, 'synthetic-ssh-key.pem');
const secretsPath = path.join(output, 'synthetic-proxy-secrets.json');
const settingsPath = path.join(output, 'fixture-settings.json');
const egressTrace = path.join(output, 'egress-trace.jsonl');
await writeFile(keyPath, privateKey, { mode: 0o600 });
await writeFile(egressTrace, '');
const fakeApiKey = 'synthetic-proxy-api-key';
const fakeCookie = 'synthetic-proxy-cookie';
const fakePassword = 'synthetic-proxy-password';
const secretValues = [fakeApiKey, fakeCookie, fakePassword, privateKey];
const safe = value => secretValues.reduce((text, secret) => text.replaceAll(secret, '[redacted]'), String(value));
const delay = ms => new Promise(resolve => setTimeout(resolve, ms));
const listen = server => new Promise((resolve, reject) => { server.once('error', reject); server.listen(0, '127.0.0.1', () => { server.removeListener('error', reject); resolve(server.address().port); }); });
const close = server => new Promise(resolve => { if (server.address()) server.close(resolve); else resolve(); });
const unusedPort = async () => { const server = net.createServer(); const port = await listen(server); await close(server); return port; };
const until = async (predicate, message, ms = 4500) => { const end = Date.now() + ms; while (Date.now() < end) { if (await predicate()) return; await delay(30); } assert.fail(message); };
const echoSockets = new Set();
const echo = net.createServer(socket => { echoSockets.add(socket); socket.on('error', () => {}); socket.on('close', () => echoSockets.delete(socket)); socket.pipe(socket); });
const echoPort = await listen(echo);
let apiScenario = 'success';
const api = http.createServer(async (req, res) => {
  const chunks = []; for await (const chunk of req) chunks.push(chunk);
  const payload = JSON.parse(Buffer.concat(chunks).toString() || '{}');
  trace.push({ type: 'api', path: req.url, method: req.method, postingPayloadValid: payload.login_cookies === fakeCookie && payload.proxy === publicProxy });
  res.writeHead(200, { 'content-type': 'application/json' });
  res.end(JSON.stringify(apiScenario === 'missing-id' ? { status: 'success' } : { status: 'success', tweet_id: '9001' }));
});
const apiPort = await listen(api);
let sshMode = 'accept';
const sshConnections = new Set(), forwarding = new Set(), relaySockets = new Set();
let sshAttempts = 0, forwardingCancellations = 0, forwardingReleaseDelayMs = 0;
const ssh = new SshServer({ hostKeys: [privateKey] }, connection => {
  sshAttempts++; sshConnections.add(connection);
  connection.on('error', () => {});
  connection.on('authentication', context => {
    if (sshMode === 'stall') return;
    if (sshMode === 'reject' || context.method !== 'publickey' || context.username !== 'proxy-test' || !context.key.data.equals(parsedKey.getPublicSSH())) return context.reject();
    if (context.signature && parsedKey.verify(context.blob, context.signature, context.hashAlgo) !== true) return context.reject();
    context.accept();
  });
  const owned = new Set();
  const release = async () => {
    await delay(forwardingReleaseDelayMs);
    for (const server of owned) { await close(server); forwarding.delete(server); }
    owned.clear();
  };
  connection.on('request', (accept, reject, name, info) => {
    if (name === 'cancel-tcpip-forward') {
      forwardingCancellations++;
      if (sshMode === 'ignore-cancel') return;
      void release().then(() => accept?.()); return;
    }
    if (name !== 'tcpip-forward' || info.bindAddr !== '127.0.0.1') { reject?.(); return; }
    const server = net.createServer(socket => {
      relaySockets.add(socket); socket.on('error', () => {}); socket.on('close', () => relaySockets.delete(socket));
      connection.forwardOut(info.bindAddr, info.bindPort, socket.remoteAddress, socket.remotePort, (error, channel) => {
        if (error) { socket.destroy(); return; }
        channel.on('error', () => socket.destroy()); socket.on('close', () => channel.destroy()); channel.on('close', () => socket.destroy());
        socket.pipe(channel).pipe(socket);
      });
    });
    server.once('error', () => reject?.());
    server.listen(info.bindPort, info.bindAddr, () => { owned.add(server); forwarding.add(server); accept?.(info.bindPort); });
  });
  connection.on('close', () => { sshConnections.delete(connection); void release(); });
});
const sshPort = await listen(ssh);
const localPort = await unusedPort(), remotePort = await unusedPort();
const publicProxy = 'http://proxy-test:' + fakePassword + '@127.0.0.1:' + remotePort;
const config = { twitterapi_io: { managed: { user_name: 'ProxyTest', api_key: fakeApiKey, proxy: publicProxy, proxy_mode: 'managed', login_cookies: fakeCookie, email: 'proxy@example.invalid', password: fakePassword } }, plumivox: { managed_proxy: { ssh_host: '127.0.0.1', ssh_port: sshPort, ssh_user: 'proxy-test', private_key_file: keyPath, host_key_sha256: pin, local_port: localPort, remote_port: remotePort } } };
config.twitterapi_io.external = { ...config.twitterapi_io.managed, proxy_mode: 'external' };
await writeFile(secretsPath, JSON.stringify(config));
await writeFile(settingsPath, JSON.stringify({ secretsPath, egressTrace, echoPort, apiBaseUrl: 'http://127.0.0.1:' + apiPort, stateDirectory: path.join(output, 'session-state') }));
let client;
const startClient = async () => {
  const transport = new StdioClientTransport({ command: process.execPath, args: [path.join(repo, 'tests/proxy-mcp-fixture.mjs'), settingsPath], cwd: repo, stderr: 'pipe', env: { ...process.env, PLUMIVOX_SECRETS_FILE: secretsPath } });
  transport.stderr?.on('data', data => { diagnosticsLeaked ||= secretValues.some(secret => String(data).includes(secret)); diagnostics.push(safe(data)); });
  client = new Client({ name: 'plumivox-proxy-e2e', version: '1.0.0' });
  client.onerror = error => diagnostics.push(safe(error.message));
  await client.connect(transport);
};
const call = async (name, args = {}, options = {}) => {
  const result = await client.callTool({ name, arguments: args }, undefined, { timeout: 7000, ...options });
  assert.ok(secretValues.every(secret => !JSON.stringify(result).includes(secret)), 'Secret leaked in MCP result');
  const data = result.structuredContent || JSON.parse(result.content.filter(item => item.type === 'text').map(item => item.text).join(''));
  return { result, data };
};
const succeeds = async (name, args) => { const { result, data } = await call(name, args); assert.notEqual(result.isError, true, JSON.stringify(data)); return data; };
const fails = async (name, args) => { const { result, data } = await call(name, args); assert.equal(result.isError, true); assert.ok(data.error?.code); return data; };
const status = async () => { const data = await succeeds('get_proxy_status'); return data.proxy || data.status || data; };
const events = async () => (await readFile(egressTrace, 'utf8')).trim().split('\n').filter(Boolean).map(line => JSON.parse(line));
const connects = async () => (await events()).filter(event => event.type === 'connect').length;
const openTunnel = async (authority = 'x.com:443', head = '') => {
  const socket = net.connect({ host: '127.0.0.1', port: remotePort }); socket.on('error', () => {});
  let data = ''; socket.on('data', chunk => { data += chunk.toString(); });
  await new Promise((resolve, reject) => { socket.once('connect', resolve); socket.once('error', reject); });
  socket.write('CONNECT ' + authority + ' HTTP/1.1\r\nHost: ' + authority + '\r\n\r\n' + head);
  return { socket, data: () => data };
};
const run = async (name, action) => { try { await action(); results.push({ name, status: 'pass' }); } catch (error) { results.push({ name, status: 'fail', message: safe(error.message) }); } };
try {
  await startClient();
  await run('Proxy MCP discovery and stopped metadata', async () => {
    assert.equal((await client.listTools()).tools.length, 11);
    assert.equal((await status()).state, 'stopped');
  });
  for (const [name, filename, content, code] of [
    ['Empty', 'empty-key.pem', '', 'PROXY_KEY_UNAVAILABLE'],
    ['Oversized', 'oversized-key.pem', Buffer.alloc(1024 * 1024 + 1), 'PROXY_KEY_TOO_LARGE'],
    ['Non-file', 'key-directory', null, 'PROXY_KEY_UNAVAILABLE'],
    ['Missing', 'missing-key.pem', undefined, 'PROXY_KEY_UNAVAILABLE'],
  ]) await run(name + ' private key rejected before SSH or API connection', async () => {
    const fixture = path.join(output, filename), attempts = sshAttempts, requests = trace.length;
    if (content === null) await mkdir(fixture);
    else if (content !== undefined) await writeFile(fixture, content);
    config.plumivox.managed_proxy.private_key_file = fixture;
    await writeFile(secretsPath, JSON.stringify(config));
    try {
      assert.equal((await fails('start_proxy', { account: 'managed' })).error.code, code);
      assert.equal((await fails('post_tweet', { account: 'managed', text: 'invalid-key' })).error.code, code);
      assert.equal(sshAttempts, attempts); assert.equal(trace.length, requests);
    } finally {
      await succeeds('stop_proxy');
      config.plumivox.managed_proxy.private_key_file = keyPath;
      await writeFile(secretsPath, JSON.stringify(config));
    }
  });
  await run('Reverse SSH forwarding carries CONNECT and early bytes exactly once', async () => {
    await succeeds('start_proxy', { account: 'managed' });
    assert.equal((await status()).state, 'ready');
    const tunnel = await openTunnel('x.com:443', 'early-bytes');
    try { await until(() => tunnel.data().includes('early-bytes'), 'Echo did not traverse SSH reverse forwarding');
      assert.ok(tunnel.data().startsWith('HTTP/1.1 200 ')); assert.equal(tunnel.data().split('early-bytes').length, 2);
      const event = (await events()).find(event => event.type === 'connect');
      assert.deepEqual(event, { type: 'connect', host: '93.184.216.34', family: 4, port: 443 });
      const lookup = (await events()).find(event => event.type === 'lookup'); assert.equal(lookup.all, true); assert.equal(lookup.verbatim, true);
    } finally { tunnel.socket.destroy(); }
  });
  await run('External account posting leaves shared managed forwarding ready', async () => {
    const before = trace.length;
    await succeeds('post_tweet', { account: 'external', text: 'external-proxy-e2e' });
    assert.equal(trace.length, before + 1);
    assert.equal((await status()).state, 'ready');
    const tunnel = await openTunnel('x.com:443', 'external-kept-alive');
    try { await until(() => tunnel.data().includes('external-kept-alive'), 'External posting stopped managed forwarding'); } finally { tunnel.socket.destroy(); }
  });
  await run('Fragmented CONNECT preserves early payload exactly once', async () => {
    const socket = net.connect({ host: '127.0.0.1', port: remotePort }); socket.on('error', () => {});
    let data = ''; socket.on('data', chunk => { data += chunk.toString(); });
    try {
      await new Promise((resolve, reject) => { socket.once('connect', resolve); socket.once('error', reject); });
      socket.write('CONNE'); await delay(30);
      socket.write('CT x.com:443 HTTP/1.1\r\nHost: x.com:443\r\n'); await delay(30);
      socket.write('\r\nfragmented-early-bytes');
      await until(() => data.includes('fragmented-early-bytes'), 'Fragmented CONNECT payload missing');
      assert.ok(data.startsWith('HTTP/1.1 200 ')); assert.equal(data.split('fragmented-early-bytes').length, 2);
    } finally { socket.destroy(); }
  });
  for (const [name, authority] of [['Forbidden domain', 'x.com.evil.invalid:443'], ['Private DNS', 'private.x.com:443'], ['Mixed public and private DNS', 'mixed.x.com:443'], ['Literal private IP', '127.0.0.1:443'], ['Non-HTTPS destination port', 'x.com:80']]) {
    await run(name + ' rejected before upstream connection', async () => {
      const before = await connects(); const tunnel = await openTunnel(authority);
      try { await until(() => new RegExp('HTTP/1[.]1 [45]').test(tunnel.data()), 'CONNECT rejection not received'); assert.equal(await connects(), before); }
      finally { tunnel.socket.destroy(); }
    });
  }
  await run('Client disconnect during DNS never starts upstream connection', async () => {
    const before = await connects(), lookups = (await events()).length;
    const tunnel = await openTunnel('slow.x.com:443');
    await until(async () => (await events()).length > lookups, 'DNS did not start');
    tunnel.socket.destroy(); await delay(450); assert.equal(await connects(), before);
  });
  await run('Explicit same-text calls each post once and missing ID never retries', async () => {
    const before = trace.length;
    await succeeds('post_tweet', { account: 'managed', text: 'proxy-e2e' });
    await succeeds('post_tweet', { account: 'managed', text: 'proxy-e2e' });
    apiScenario = 'missing-id'; await fails('post_tweet', { account: 'managed', text: 'proxy-e2e' }); apiScenario = 'success';
    assert.equal(trace.length - before, 3); assert.ok(trace.slice(before).every(event => event.postingPayloadValid));
  });
  await run('SSH disconnect reconnects without replaying API requests', async () => {
    const before = sshAttempts, requests = trace.length;
    for (const connection of sshConnections) connection.end();
    await until(async () => sshAttempts > before && (await status()).state === 'ready', 'SSH connection did not recover');
    const tunnel = await openTunnel(); try { await until(() => tunnel.data().includes('200 Connection Established'), 'Recovered forwarding unusable'); } finally { tunnel.socket.destroy(); }
    assert.equal(trace.length, requests);
  });
  await run('Stop waits for delayed remote listener release before immediate restart', async () => {
    forwardingReleaseDelayMs = 350;
    const before = forwardingCancellations, requests = trace.length;
    try {
      await succeeds('stop_proxy');
      assert.equal(forwarding.size, 0, 'Stop returned before owned remote listener was released');
      await succeeds('start_proxy', { account: 'managed' });
      assert.ok(forwardingCancellations > before, 'Owned forwarding was not explicitly cancelled');
      const tunnel = await openTunnel('x.com:443', 'immediate-restart');
      try { await until(() => tunnel.data().includes('immediate-restart'), 'Immediate restart forwarding unusable'); }
      finally { tunnel.socket.destroy(); }
      assert.equal(trace.length, requests);
    } finally { forwardingReleaseDelayMs = 0; await succeeds('stop_proxy'); await until(() => forwarding.size === 0, 'Delayed listener survived cleanup'); }
  });
  await run('Remote port owned by another listener fails permanently without stealing or API replay', async () => {
    const blocker = net.createServer(socket => socket.end('other-owner'));
    await new Promise(resolve => blocker.listen(remotePort, '127.0.0.1', resolve));
    try {
      const before = sshAttempts, requests = trace.length;
      const result = await fails('start_proxy', { account: 'managed' });
      assert.equal(result.error.code, 'PROXY_REMOTE_BIND_FAILED');
      await delay(350);
      assert.equal(sshAttempts, before + 1);
      assert.equal((await status()).state, 'failed');
      assert.equal(blocker.listening, true);
      const socket = net.connect({ host: '127.0.0.1', port: remotePort });
      let data = ''; socket.on('data', chunk => { data += chunk; }); socket.on('error', () => {});
      try { await until(() => data.includes('other-owner'), 'Other owner listener was disturbed'); }
      finally { socket.destroy(); }
      assert.equal(trace.length, requests);
    } finally { await succeeds('stop_proxy'); await close(blocker); }
  });
  await run('Unanswered forwarding cancellation cannot hang explicit stop', async () => {
    await succeeds('start_proxy', { account: 'managed' });
    sshMode = 'ignore-cancel';
    const before = forwardingCancellations, started = Date.now();
    try {
      await succeeds('stop_proxy');
      assert.ok(forwardingCancellations > before, 'Stop did not request forwarding cancellation');
      assert.ok(Date.now() - started < 4000, 'SSH shutdown exceeded its finite timeout');
      await until(() => forwarding.size === 0, 'Forced shutdown left owned listener behind');
      assert.equal((await status()).state, 'stopped');
    } finally { sshMode = 'accept'; }
  });
  await run('Stop closes active tunnels and prevents automatic resurrection', async () => {
    await succeeds('start_proxy', { account: 'managed' });
    const tunnel = await openTunnel(); await until(() => tunnel.data().includes('200 Connection Established'), 'Tunnel not established');
    await succeeds('stop_proxy'); await until(() => tunnel.socket.destroyed, 'Active tunnel survived stop');
    const after = sshAttempts; await delay(300); const data = await status();
    assert.equal(data.state, 'stopped'); assert.equal(data.activeConnections, 0); assert.equal(sshAttempts, after);
  });
  await run('Local port conflict fails without stopping existing listener', async () => {
    const blocker = net.createServer(socket => socket.destroy()); await new Promise(resolve => blocker.listen(localPort, '127.0.0.1', resolve));
    try { const data = await fails('start_proxy', { account: 'managed' }); assert.equal(data.error.code, 'PROXY_PORT_BUSY'); assert.equal(blocker.listening, true); }
    finally { await succeeds('stop_proxy'); await close(blocker); }
  });
  await run('Host pin mismatch blocks SSH and posting API', async () => {
    config.plumivox.managed_proxy.host_key_sha256 = 'SHA256:' + 'A'.repeat(43); await writeFile(secretsPath, JSON.stringify(config));
    const before = trace.length; await fails('post_tweet', { account: 'managed', text: 'blocked' }); assert.equal(trace.length, before);
    await succeeds('stop_proxy'); config.plumivox.managed_proxy.host_key_sha256 = pin; await writeFile(secretsPath, JSON.stringify(config));
  });
  await run('SSH authentication failure blocks login and posting API', async () => {
    sshMode = 'reject'; const before = trace.length;
    const failures = await Promise.all(Array.from({ length: 3 }, () => fails('start_proxy', { account: 'managed' })));
    assert.ok(failures.every(value => value.error.code === failures[0].error.code));
    await fails('post_tweet', { account: 'managed', text: 'blocked' });
    await fails('refresh_session', { account: 'managed' }); assert.equal(trace.length, before);
    await succeeds('stop_proxy'); sshMode = 'accept';
  });
  await run('MCP cancellation during startup prevents delayed readiness', async () => {
    sshMode = 'stall'; const before = sshAttempts, abort = new AbortController();
    const pending = call('start_proxy', { account: 'managed' }, { signal: abort.signal }).catch(() => null);
    await until(() => sshAttempts > before, 'SSH startup did not begin'); abort.abort(); await pending;
    await succeeds('stop_proxy'); sshMode = 'accept'; const attempts = sshAttempts;
    await delay(350); assert.equal((await status()).state, 'stopped'); assert.equal(sshAttempts, attempts);
  });
  await run('Concurrent startup waiters settle on readiness and stop after individual cancellation', async () => {
    await Promise.all(Array.from({ length: 3 }, () => succeeds('start_proxy', { account: 'managed' })));
    assert.equal((await status()).state, 'ready'); await succeeds('stop_proxy');
    sshMode = 'stall'; const before = sshAttempts, abort = new AbortController();
    const cancelled = call('start_proxy', { account: 'managed' }, { signal: abort.signal }).catch(() => null);
    const stopped = Array.from({ length: 2 }, () => fails('start_proxy', { account: 'managed' }));
    try {
      await until(() => sshAttempts > before, 'Concurrent SSH startup did not begin');
      abort.abort(); await cancelled; await succeeds('stop_proxy');
      for (const value of await Promise.all(stopped)) assert.equal(value.error.code, 'PROXY_STOPPED');
      assert.equal((await status()).state, 'stopped');
    } finally { sshMode = 'accept'; await succeeds('stop_proxy'); }
  });
  await run('Stop during unresolved DNS prevents later upstream connection', async () => {
    await succeeds('start_proxy', { account: 'managed' }); const before = await connects(), lookups = (await events()).length;
    const tunnel = await openTunnel('slow.x.com:443'); await until(async () => (await events()).length > lookups, 'DNS did not begin');
    await succeeds('stop_proxy'); await delay(450); assert.equal(await connects(), before); assert.equal((await status()).activeConnections, 0); tunnel.socket.destroy();
  });
  await run('MCP diagnostics contain no credentials or protocol errors', async () => { assert.equal(diagnosticsLeaked, false); assert.deepEqual(diagnostics, []); });
} finally {
  await client?.close().catch(() => {});
  for (const socket of relaySockets) socket.destroy();
  for (const connection of sshConnections) connection.end();
  for (const server of forwarding) server.close();
  for (const socket of echoSockets) socket.destroy();
  await Promise.all([close(ssh), close(api), close(echo)]);
  const summary = { passed: results.filter(result => result.status === 'pass').length, failed: results.filter(result => result.status === 'fail').length, results, diagnostics: diagnostics.map(safe), fixtureCleanupRequired: ['synthetic-ssh-key.pem', 'synthetic-proxy-secrets.json', 'fixture-settings.json', 'session-state'] };
  await writeFile(path.join(output, 'summary.json'), JSON.stringify(summary, null, 2));
  await writeFile(path.join(output, 'http-trace.json'), JSON.stringify(trace, null, 2));
  console.log(JSON.stringify({ output, passed: summary.passed, failed: summary.failed }));
  if (summary.failed) process.exitCode = 1;
}
