import assert from 'node:assert/strict';
import net from 'node:net';
import http from 'node:http';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { mkdir, readFile, writeFile, realpath, stat } from 'node:fs/promises';
import { generateKeyPairSync, createHash, createHmac, randomBytes, randomUUID } from 'node:crypto';
import ssh2 from 'ssh2';
const { Server: SshServer, utils: sshUtils } = ssh2;
import { Client } from '@modelcontextprotocol/sdk/client/index.js';
import { StdioClientTransport } from '@modelcontextprotocol/sdk/client/stdio.js';

const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const output = path.resolve(process.env.PLUMIVOX_PROXY_SHARED_E2E_OUTPUT_DIR || path.join(os.tmpdir(), 'Codex', 'Plumivox', randomUUID()));
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
const daemonTrace = path.join(output, 'daemon-trace.jsonl');
await writeFile(daemonTrace, '');
const ipcPathFile = path.join(output, 'ipc-path.json');
const runtimeDirectory = path.join(output, 'runtime');
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
const delayedAuthentication = [];
const sshConnections = new Set(), forwarding = new Set(), relaySockets = new Set();
let sshAttempts = 0;
const ssh = new SshServer({ hostKeys: [privateKey] }, connection => {
  sshAttempts++; sshConnections.add(connection);
  connection.on('error', () => {});
  connection.on('authentication', context => {
    if (sshMode === 'stall') { delayedAuthentication.push(context); return; }
    if (sshMode === 'reject' || context.method !== 'publickey' || context.username !== 'proxy-test' || !context.key.data.equals(parsedKey.getPublicSSH())) return context.reject();
    if (context.signature && parsedKey.verify(context.blob, context.signature, context.hashAlgo) !== true) return context.reject();
    context.accept();
  });
  const owned = new Set();
  connection.on('request', (accept, reject, name, info) => {
    if (name === 'cancel-tcpip-forward') {
      for (const server of owned) { server.close(); forwarding.delete(server); }
      owned.clear(); accept?.(); return;
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
  connection.on('close', () => { sshConnections.delete(connection); for (const server of owned) { server.close(); forwarding.delete(server); } });
});
const sshPort = await listen(ssh);
const localPort = await unusedPort(), remotePort = await unusedPort();
const publicProxy = 'http://proxy-test:' + fakePassword + '@127.0.0.1:' + remotePort;
const config = { twitterapi_io: { managed: { user_name: 'ProxyTest', api_key: fakeApiKey, proxy: publicProxy, proxy_mode: 'managed', login_cookies: fakeCookie, email: 'proxy@example.invalid', password: fakePassword } }, plumivox: { managed_proxy: { ssh_host: '127.0.0.1', ssh_port: sshPort, ssh_user: 'proxy-test', private_key_file: keyPath, host_key_sha256: pin, local_port: localPort, remote_port: remotePort } } };
config.twitterapi_io.external = { ...config.twitterapi_io.managed, proxy_mode: 'external' };
await writeFile(secretsPath, JSON.stringify(config));
await writeFile(settingsPath, JSON.stringify({ secretsPath, egressTrace, echoPort, apiBaseUrl: 'http://127.0.0.1:' + apiPort, stateDirectory: path.join(output, 'session-state'), daemonTrace, ipcPathFile, runtimeDirectory }));
const clients = new Set();
const startClient = async (fixtureSettingsPath = settingsPath) => {
  const transport = new StdioClientTransport({ command: process.execPath, args: [path.join(repo, 'tests/proxy-shared-mcp-fixture.mjs'), fixtureSettingsPath], cwd: repo, stderr: 'pipe', env: { ...process.env, PLUMIVOX_SECRETS_FILE: secretsPath } });
  transport.stderr?.on('data', data => { diagnosticsLeaked ||= secretValues.some(secret => String(data).includes(secret)); diagnostics.push(safe(data)); });
  const client = new Client({ name: 'plumivox-shared-proxy-e2e', version: '1.0.0' });
  client.onerror = error => diagnostics.push(safe(error.message));
  await client.connect(transport); clients.add(client); return client;
};
const closeClient = async client => { clients.delete(client); await client.close(); };
const call = async (client, name, args = {}, options = {}) => {
  const result = await client.callTool({ name, arguments: args }, undefined, { timeout: 7000, ...options });
  assert.ok(secretValues.every(secret => !JSON.stringify(result).includes(secret)), 'Secret leaked in MCP result');
  const data = result.structuredContent || JSON.parse(result.content.filter(item => item.type === 'text').map(item => item.text).join(''));
  return { result, data };
};
const succeeds = async (client, name, args) => { const { result, data } = await call(client, name, args); assert.notEqual(result.isError, true, JSON.stringify(data)); return data; };
const status = async client => succeeds(client, 'get_proxy_status');
const daemonEvents = async () => (await readFile(daemonTrace, 'utf8')).trim().split('\n').filter(Boolean).map(line => JSON.parse(line));
const openTunnel = async (head = '') => {
  const socket = net.connect({ host: '127.0.0.1', port: remotePort }); socket.on('error', () => {});
  let data = ''; socket.on('data', chunk => { data += chunk.toString(); });
  await new Promise((resolve, reject) => { socket.once('connect', resolve); socket.once('error', reject); });
  socket.write('CONNECT x.com:443 HTTP/1.1\r\nHost: x.com:443\r\n\r\n' + head);
  return { socket, data: () => data };
};
const echoCheck = async value => { const tunnel = await openTunnel(value); try { await until(() => tunnel.data().includes(value), 'Echo did not traverse shared SSH forwarding'); } finally { tunnel.socket.destroy(); } };
const normalizedConfig = { sshHost: '127.0.0.1', sshPort, sshUser: 'proxy-test', privateKeyFile: path.resolve(keyPath), hostKeySha256: pin, localPort, remotePort };
const contextFor = metadata => createHash('sha256').update('Plumivox shared proxy metadata v1\0').update(JSON.stringify(metadata)).digest('hex');
const ipcContext = contextFor(normalizedConfig);
const ipcSecret = createHash('sha256').update('Plumivox shared proxy IPC v1\0').update(privateKey).digest();
secretValues.push(ipcSecret.toString('hex'), ipcSecret.toString('base64'));
const nonce = () => randomBytes(32).toString('hex');
const proof = (kind, body) => createHmac('sha256', ipcSecret).update(JSON.stringify(['Plumivox shared proxy IPC v2', kind, body])).digest('hex');
const framesFor = socket => {
  let pending = '', ended = false; const frames = [], waiters = [];
  socket.on('data', chunk => { pending += chunk.toString(); let newline;
    while ((newline = pending.indexOf('\n')) !== -1) {
      let frame; try { frame = JSON.parse(pending.slice(0, newline)); } catch { socket.destroy(); return; }
      pending = pending.slice(newline + 1); const waiter = waiters.shift(); if (waiter) waiter.resolve(frame); else frames.push(frame);
    }
  });
  socket.on('close', () => { ended = true; for (const waiter of waiters.splice(0)) waiter.reject(new Error('IPC socket closed')); });
  return { next: () => { if (frames.length) return Promise.resolve(frames.shift()); if (ended) return Promise.reject(new Error('IPC socket closed'));
    return new Promise((resolve, reject) => { const waiter = { resolve: value => { clearTimeout(timer); resolve(value); }, reject: error => { clearTimeout(timer); reject(error); } };
      const timer = setTimeout(() => { const index = waiters.indexOf(waiter); if (index !== -1) waiters.splice(index, 1); socket.destroy(); reject(new Error('IPC frame missing')); }, 1500); waiters.push(waiter); }); } };
};
const ipcRejected = async frame => {
  const { socketPath } = JSON.parse(await readFile(ipcPathFile, 'utf8'));
  const socket = net.connect(socketPath); socket.on('error', () => {}); const frames = framesFor(socket);
  await new Promise((resolve, reject) => { socket.once('connect', resolve); socket.once('error', reject); });
  socket.write(JSON.stringify({ type: 'hello', version: 2, context: ipcContext, clientNonce: nonce() }) + '\n');
  const challenge = await frames.next(); assert.equal(challenge.type, 'challenge'); assert.equal(challenge.version, 2);
  const closed = new Promise(resolve => socket.once('close', resolve));
  if (frame.length > 8192) socket.write(frame);
  else socket.write(JSON.stringify({ type: 'request', version: 2, context: ipcContext, clientNonce: challenge.clientNonce,
    channelNonce: challenge.channelNonce, challenge: challenge.challenge, requestId: 1, operation: 'stop', hmac: '0'.repeat(64) }) + '\n');
  try { await Promise.race([closed, delay(1500).then(() => { throw new Error('Invalid IPC connection not closed'); })]); } finally { socket.destroy(); }
};
let capturedChallenge, capturedResult;
const fakePeerCheck = async mode => {
  const attackPipe = '\\\\.\\pipe\\plumivox-e2e-' + randomUUID();
  const sockets = new Set();
  const fake = net.createServer(socket => {
    sockets.add(socket); socket.on('error', () => {}); socket.on('close', () => sockets.delete(socket)); const frames = framesFor(socket);
    void (async () => {
      const hello = await frames.next();
      const body = { type: 'challenge', version: 2, context: mode === 'scope' ? contextFor({ ...normalizedConfig, sshUser: 'other-scope' }) : hello.context,
        clientNonce: hello.clientNonce, channelNonce: nonce(), challenge: nonce() };
      const challenge = { ...body, proof: proof('server', body) };
      if (mode === 'challenge-replay') { socket.write(JSON.stringify(capturedChallenge) + '\n'); return; }
      socket.write(JSON.stringify(challenge) + '\n');
      if (mode === 'scope') return;
      if (mode === 'normal') capturedChallenge = structuredClone(challenge);
      while (!socket.destroyed) {
        const request = await frames.next();
        const response = { type: 'result', version: 2, context: hello.context, clientNonce: hello.clientNonce, channelNonce: body.channelNonce,
          requestChallenge: request.challenge, requestId: request.requestId, operation: request.operation, challenge: nonce(), ok: true,
          status: { state: 'ready', managed: true, activeConnections: 0, lastErrorCode: null } };
        const result = { ...response, proof: proof('response', response) };
        if (mode === 'normal') capturedResult = structuredClone(result);
        if (mode === 'result-replay') socket.write(JSON.stringify(capturedResult) + '\n');
        else { if (mode === 'status-tamper') result.status.activeConnections = 1; socket.write(JSON.stringify(result) + '\n'); }
      }
    })().catch(() => socket.destroy());
  });
  await new Promise((resolve, reject) => { fake.once('error', reject); fake.listen(attackPipe, resolve); });
  const attackSettingsPath = path.join(output, 'attack-' + mode + '-settings.json');
  await writeFile(attackSettingsPath, JSON.stringify({ ...JSON.parse(await readFile(settingsPath, 'utf8')), attackPipe }));
  let client;
  try {
    client = await startClient(attackSettingsPath);
    const before = trace.length, attempts = sshAttempts;
    const response = await call(client, 'start_proxy', { account: 'managed' });
    if (mode === 'normal') { assert.notEqual(response.result.isError, true); assert.equal(response.data.state, 'ready'); assert.equal((await status(client)).state, 'ready'); }
    else { assert.equal(response.result.isError, true); assert.equal(response.data.error.code, 'PROXY_IPC_AUTH_FAILED'); }
    assert.equal(trace.length, before); assert.equal(sshAttempts, attempts);
  } finally { if (client) await closeClient(client); for (const socket of sockets) socket.destroy(); await close(fake); }
};
const run = async (name, action) => { try { await action(); results.push({ name, status: 'pass' }); } catch (error) { results.push({ name, status: 'fail', message: safe(error.message) }); } };
let first, second;
try {
  [first, second] = await Promise.all([startClient(), startClient()]);
  await run('Two independent MCPs discover shared proxy operations', async () => {
    assert.equal((await first.listTools()).tools.length, 11); assert.equal((await second.listTools()).tools.length, 11);
    assert.equal((await status(first)).state, 'stopped'); assert.equal((await status(second)).state, 'stopped');
  });
  for (const [name, filename, content, code] of [
    ['Empty', 'empty-key.pem', '', 'PROXY_KEY_UNAVAILABLE'],
    ['Oversized', 'oversized-key.pem', Buffer.alloc(1024 * 1024 + 1), 'PROXY_KEY_TOO_LARGE'],
    ['Non-file', 'key-directory', null, 'PROXY_KEY_UNAVAILABLE'],
    ['Missing', 'missing-key.pem', undefined, 'PROXY_KEY_UNAVAILABLE'],
  ]) await run(name + ' shared private key rejected before SSH or API connection', async () => {
    const fixture = path.join(output, filename), attempts = sshAttempts, requests = trace.length;
    if (content === null) await mkdir(fixture);
    else if (content !== undefined) await writeFile(fixture, content);
    config.plumivox.managed_proxy.private_key_file = fixture;
    await writeFile(secretsPath, JSON.stringify(config));
    try {
      for (const [name, args] of [['start_proxy', { account: 'managed' }], ['post_tweet', { account: 'managed', text: 'invalid-key' }]]) {
        const { result, data } = await call(first, name, args);
        assert.equal(result.isError, true); assert.equal(data.error.code, code);
      }
      assert.equal(sshAttempts, attempts); assert.equal(trace.length, requests);
    } finally {
      config.plumivox.managed_proxy.private_key_file = keyPath;
      await writeFile(secretsPath, JSON.stringify(config));
    }
  });
  await run('Concurrent MCP start creates one SSH and forwarding owner', async () => {
    await Promise.all([succeeds(first, 'start_proxy', { account: 'managed' }), succeeds(second, 'start_proxy', { account: 'managed' })]);
    assert.equal((await status(first)).state, 'ready'); assert.equal((await status(second)).state, 'ready');
    assert.equal(sshAttempts, 1); assert.equal(sshConnections.size, 1); assert.equal(forwarding.size, 1);
    assert.equal((await daemonEvents()).filter(event => event.event === 'start').length, 1);
    await echoCheck('shared-concurrent-start');
  });
  await run('Equivalent key path retains shared IPC identity', async () => {
    const attempts = sshAttempts, starts = (await daemonEvents()).filter(event => event.event === 'start').length;
    config.plumivox.managed_proxy.private_key_file = path.dirname(keyPath) + path.sep + '.' + path.sep + path.basename(keyPath);
    await writeFile(secretsPath, JSON.stringify(config));
    try {
      await succeeds(second, 'start_proxy', { account: 'managed' });
      assert.equal(sshAttempts, attempts);
      assert.equal((await daemonEvents()).filter(event => event.event === 'start').length, starts);
      await echoCheck('same-shared-ipc-identity');
    } finally {
      config.plumivox.managed_proxy.private_key_file = keyPath;
      await writeFile(secretsPath, JSON.stringify(config));
    }
  });
  await run('Attached idle MCPs keep daemon alive beyond idle timeout', async () => {
    const attempts = sshAttempts; await delay(1000); assert.equal((await status(second)).state, 'ready');
    assert.equal(sshAttempts, attempts); assert.equal(sshConnections.size, 1); await echoCheck('shared-not-idle');
  });
  await run('Closing one MCP leaves other MCP and active CONNECT intact', async () => {
    const tunnel = await openTunnel('before-close'); await until(() => tunnel.data().includes('before-close'), 'Tunnel not ready');
    try { await closeClient(first); assert.equal((await status(second)).state, 'ready');
      tunnel.socket.write('after-close'); await until(() => tunnel.data().includes('after-close'), 'Closing one MCP destroyed shared tunnel');
      assert.equal(sshConnections.size, 1);
    } finally { tunnel.socket.destroy(); }
    first = await startClient(); await succeeds(first, 'start_proxy', { account: 'managed' });
  });
  await run('External posting in one MCP leaves shared managed forwarding intact', async () => {
    const before = trace.length; await succeeds(first, 'post_tweet', { account: 'external', text: 'shared-external' });
    assert.equal(trace.length, before + 1); assert.equal((await status(second)).state, 'ready'); await echoCheck('external-shared-still-ready');
  });
  await run('Unauthenticated IPC operation rejected without changing shared state', async () => {
    const attempts = sshAttempts; await ipcRejected(JSON.stringify({ type: 'request', operation: 'stop', hmac: '0'.repeat(64) }) + '\n');
    assert.equal((await status(second)).state, 'ready'); assert.equal(sshAttempts, attempts); await echoCheck('unauthenticated-ipc-safe');
  });
  await run('Oversized IPC frame rejected without changing shared state', async () => {
    await ipcRejected('x'.repeat(8193)); assert.equal((await status(second)).state, 'ready'); await echoCheck('oversized-ipc-safe');
  });
  await run('Fragmented IPC frames preserve authentication and successive challenge order', async () => {
    const { socketPath } = JSON.parse(await readFile(ipcPathFile, 'utf8'));
    const socket = net.connect(socketPath); socket.on('error', () => {}); const frames = framesFor(socket);
    const fragmented = async body => {
      const bytes = JSON.stringify(body) + '\n';
      socket.write(bytes.slice(0, 11)); await delay(30); socket.write(bytes.slice(11));
    };
    try {
      await new Promise((resolve, reject) => { socket.once('connect', resolve); socket.once('error', reject); });
      await fragmented({ type: 'hello', version: 2, context: ipcContext, clientNonce: nonce() });
      let challenge = await frames.next(); assert.equal(challenge.type, 'challenge');
      for (let requestId = 1; requestId <= 3; requestId++) {
        const body = { type: 'request', version: 2, context: ipcContext, clientNonce: challenge.clientNonce,
          channelNonce: challenge.channelNonce, challenge: challenge.challenge, requestId, operation: 'status' };
        if (requestId === 2) socket.write(JSON.stringify({ ...body, hmac: proof('request', body) }) + '\n');
        else await fragmented({ ...body, hmac: proof('request', body) });
        const response = await frames.next();
        assert.equal(response.requestId, requestId); assert.equal(response.requestChallenge, body.challenge);
        assert.equal(response.ok, true); assert.equal(response.status.state, 'ready');
        const { proof: signature, ...responseBody } = response; assert.equal(signature, proof('response', responseBody));
        challenge = response;
      }
    } finally { socket.destroy(); }
  });
  await run('Global stop closes every user tunnel without automatic restart', async () => {
    const tunnel = await openTunnel('global-stop'); await until(() => tunnel.data().includes('global-stop'), 'Tunnel not ready');
    await succeeds(first, 'stop_proxy'); await until(() => tunnel.socket.destroyed, 'Shared tunnel survived global stop');
    await until(() => sshConnections.size === 0 && forwarding.size === 0, 'SSH owner survived global stop');
    const attempts = sshAttempts; await delay(650); assert.equal((await status(second)).state, 'stopped'); assert.equal(sshAttempts, attempts);
    const events = await daemonEvents(); assert.equal(events.filter(event => event.event === 'start').length, events.filter(event => event.event === 'exit').length);
  });
  await run('Cancelling one MCP startup leaves second caller able to finish', async () => {
    sshMode = 'stall'; const before = sshAttempts, abort = new AbortController();
    const cancelled = call(first, 'start_proxy', { account: 'managed' }, { signal: abort.signal }).catch(() => null);
    const other = succeeds(second, 'start_proxy', { account: 'managed' }).then(value => ({ value }), error => ({ error }));
    await until(() => sshAttempts > before && delayedAuthentication.length > 0, 'Shared SSH startup did not begin');
    abort.abort(); await cancelled; await delay(50); sshMode = 'accept';
    for (const context of delayedAuthentication.splice(0)) {
      if (context.method !== 'publickey' || context.username !== 'proxy-test' || !context.key.data.equals(parsedKey.getPublicSSH())
        || (context.signature && parsedKey.verify(context.blob, context.signature, context.hashAlgo) !== true)) context.reject();
      else context.accept();
    }
    const completed = await other; if (completed.error) throw completed.error; assert.equal((await status(second)).state, 'ready'); assert.equal(sshAttempts, before + 1);
    await echoCheck('one-cancelled-other-ready');
  });
  await run('Final MCP departure shuts daemon and SSH down after idle timeout', async () => {
    await closeClient(first); await closeClient(second);
    await until(() => sshConnections.size === 0 && forwarding.size === 0, 'Idle daemon retained SSH or forwarding');
    await until(async () => { const events = await daemonEvents(); return events.filter(event => event.event === 'start').length === events.filter(event => event.event === 'exit').length; }, 'Idle daemon retained IPC listener');
  });
  for (const [name, mode] of [
    ['Fresh IPC v2 signed challenge and response accepted', 'normal'],
    ['Past server challenge rejected by fresh client nonce', 'challenge-replay'],
    ['Past signed result rejected by channel and request binding', 'result-replay'],
    ['Signed response status modification rejected', 'status-tamper'],
    ['Correct MAC for a different configuration scope rejected', 'scope'],
  ]) await run(name, () => fakePeerCheck(mode));
  await run('Shared MCP diagnostics contain no credentials or protocol errors', async () => { assert.equal(diagnosticsLeaked, false); assert.deepEqual(diagnostics, []); });
} finally {
  for (const client of clients) { try { await closeClient(client); } catch {} }
  await delay(600);
  for (const socket of relaySockets) socket.destroy();
  for (const connection of sshConnections) connection.end();
  for (const server of forwarding) server.close();
  for (const socket of echoSockets) socket.destroy();
  await Promise.all([close(ssh), close(api), close(echo)]);
  const summary = { passed: results.filter(result => result.status === 'pass').length, failed: results.filter(result => result.status === 'fail').length, results, diagnostics: diagnostics.map(safe), fixtureCleanupRequired: ['synthetic-ssh-key.pem', 'synthetic-proxy-secrets.json', 'fixture-settings.json', 'ipc-path.json', 'session-state', 'runtime', ...['normal', 'challenge-replay', 'result-replay', 'status-tamper', 'scope'].map(mode => 'attack-' + mode + '-settings.json')] };
  await writeFile(path.join(output, 'summary.json'), JSON.stringify(summary, null, 2));
  await writeFile(path.join(output, 'http-trace.json'), JSON.stringify(trace, null, 2));
  console.log(JSON.stringify({ output, passed: summary.passed, failed: summary.failed }));
  if (summary.failed) process.exitCode = 1;
}
