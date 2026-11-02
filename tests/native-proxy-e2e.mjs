import assert from 'node:assert/strict';
import net from 'node:net';
import http from 'node:http';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { mkdir, writeFile, lstat, realpath, readdir } from 'node:fs/promises';
import { generateKeyPairSync, createHash, randomUUID } from 'node:crypto';
import ssh2 from 'ssh2';
import { Client } from '@modelcontextprotocol/sdk/client/index.js';
import { StdioClientTransport } from '@modelcontextprotocol/sdk/client/stdio.js';
import { isInside } from './path-containment.mjs';

// Real Rust MCP + detached native daemon + authenticated SSH, entirely on loopback.
// DNS and TCP production implementations are deliberately not replaced.
const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const executable = path.resolve(process.env.PLUMIVOX_NATIVE_FIXTURE || path.join(repo, 'target', 'debug', `plumivox-fixture${process.platform === 'win32' ? '.exe' : ''}`));
assert.ok((await lstat(executable)).isFile(), 'Build the native test fixture first');
const output = path.resolve(process.env.PLUMIVOX_PROXY_NATIVE_E2E_OUTPUT_DIR || path.join(await realpath(os.tmpdir()), 'Codex', 'Plumivox', randomUUID()));
let ancestor = output;
while (true) { try { await lstat(ancestor); break; } catch (e) { if (e.code !== 'ENOENT') throw e; ancestor = path.dirname(ancestor); } }
for (let p = ancestor; ; p = path.dirname(p)) { assert.ok(!(await lstat(p)).isSymbolicLink(), 'Evidence path crosses a link'); if (path.dirname(p) === p) break; }
const resolved = path.join(await realpath(ancestor), path.relative(ancestor, output));
for (const root of [repo, path.join(os.homedir(), '.codex'), ...(process.env.CODEX_HOME ? [path.resolve(process.env.CODEX_HOME)] : [])]) assert.ok(!isInside(output, root) && !isInside(resolved, root));
await mkdir(output, { recursive: true });
const privateKey = generateKeyPairSync('rsa', { modulusLength: 2048, privateKeyEncoding: { type: 'pkcs1', format: 'pem' }, publicKeyEncoding: { type: 'spki', format: 'pem' } }).privateKey;
const parsedKey = ssh2.utils.parseKey(privateKey);
const pin = `SHA256:${createHash('sha256').update(parsedKey.getPublicSSH()).digest('base64').replace(/=+$/, '')}`;
const keyFile = path.join(output, 'synthetic-key.pem');
await writeFile(keyFile, privateKey, { mode: 0o600, flag: 'wx' });
const fakeApiKey = 'synthetic-native-proxy-api-key';
const fakeCookie = 'synthetic-native-proxy-cookie';
const fakePassword = 'synthetic-native-proxy-password';
const secrets = [privateKey, fakeApiKey, fakeCookie, fakePassword];
const safe = text => secrets.reduce((v, secret) => v.replaceAll(secret, '[redacted]'), String(text));
const results = [], trace = [], diagnostics = [];
let leaked = false;
const delay = ms => new Promise(resolve => setTimeout(resolve, ms));
const listen = server => new Promise((resolve, reject) => { server.once('error', reject); server.listen(0, '127.0.0.1', () => { server.removeListener('error', reject); resolve(server.address().port); }); });
const close = server => new Promise(resolve => { if (server.address()) server.close(resolve); else resolve(); });
const freePort = async () => { const s = net.createServer(); const p = await listen(s); await close(s); return p; };
const until = async (predicate, message, milliseconds = 5000) => { const deadline = Date.now() + milliseconds; while (Date.now() < deadline) { if (await predicate()) return; await delay(25); } assert.fail(message); };
let mode = 'accept', attempts = 0, unforwarded = 0;
const delayed = [], connections = new Set(), listeners = new Set(), sockets = new Set();
const ssh = new ssh2.Server({ hostKeys: [privateKey] }, connection => {
  attempts++; connections.add(connection); trace.push({ type: 'ssh-connection', attempts });
  connection.on('error', () => {});
  connection.on('authentication', context => {
    if (mode === 'stall') { delayed.push(context); return; }
    authenticate(context);
  });
  const owned = new Set();
  connection.on('request', (accept, reject, request, info) => {
    trace.push({ type: 'ssh-request', request });
    if (request === 'cancel-tcpip-forward') {
      if(mode==='unforward-stall')return;
      unforwarded++; for (const server of owned) { server.close(); listeners.delete(server); } owned.clear(); accept?.(); return;
    }
    if (request !== 'tcpip-forward' || info.bindAddr !== '127.0.0.1' || mode === 'bind-reject') { reject?.(); return; }
    const server = net.createServer(socket => {
      sockets.add(socket); socket.on('error', () => {}); socket.on('close', () => sockets.delete(socket));
      connection.forwardOut(info.bindAddr, info.bindPort, socket.remoteAddress, socket.remotePort, (error, channel) => {
        if (error) { socket.destroy(); return; }
        channel.on('error', () => socket.destroy()); channel.on('close', () => socket.destroy()); socket.on('close', () => channel.destroy()); socket.pipe(channel).pipe(socket);
      });
    });
    owned.add(server); listeners.add(server);
    server.once('error', () => { owned.delete(server); listeners.delete(server); reject?.(); });
    server.listen(info.bindPort, info.bindAddr, () => { if (!connections.has(connection)) { server.close(); return; } accept?.(); });
  });
  connection.on('close', () => { connections.delete(connection); for (const server of owned) { server.close(); listeners.delete(server); } owned.clear(); });
});
function authenticate(context) {
  if (mode === 'auth-reject' || context.method !== 'publickey' || context.username !== 'fixture' || !context.key.data.equals(parsedKey.getPublicSSH())) return context.reject();
  if (context.signature && parsedKey.verify(context.blob, context.signature, context.hashAlgo) !== true) return context.reject();
  context.accept();
}
const sshPort = await listen(ssh);
let apiRequests=0;
const api = http.createServer((req, res) => { apiRequests++;res.writeHead(200, { 'content-type': 'application/json' }); res.end(JSON.stringify({ status: 'success', tweet_id: '901' })); });
const apiPort = await listen(api);
const clients = new Set();
let nextClient = 0;
async function settings(name, overrides = {}) {
  const local = await freePort(), remote = await freePort();
  const config = { twitterapi_io: {
    managed: { user_name: 'Fixture', api_key: fakeApiKey, proxy: `http://fixture:${fakePassword}@127.0.0.1:${remote}`, proxy_mode: 'managed', login_cookies: fakeCookie },
    external: { user_name: 'External', api_key: fakeApiKey, proxy: 'http://127.0.0.1:8888', proxy_mode: 'external', login_cookies: fakeCookie },
  }, plumivox: { managed_proxy: { ssh_host: '127.0.0.1', ssh_port: sshPort, ssh_user: 'fixture', private_key_file: keyFile, host_key_sha256: pin, local_port: local, remote_port: remote, ...overrides } } };
  const file = path.join(output, `${name}.json`); await writeFile(file, JSON.stringify(config), { flag: 'wx' });
  return { file, local, remote, config };
}
async function clientFor(config, extraEnv={}) {
  const transport = new StdioClientTransport({ command: executable, args: [`http://127.0.0.1:${apiPort}`, config.file, path.join(output, `state-${++nextClient}`), '15000'], cwd: repo, stderr: 'pipe', env: { ...process.env, PLUMIVOX_SECRETS_FILE: config.file, PLUMIVOX_PROXY_TEST_IDLE_MS: '2000',PLUMIVOX_PROXY_TEST_STARTUP_MS:'5000',...extraEnv } });
  transport.stderr?.on('data', bytes => { leaked ||= secrets.some(s => String(bytes).includes(s)); diagnostics.push(safe(bytes)); });
  const client = new Client({ name: 'native-proxy-e2e', version: '1.0.0' });
  client.onerror = e => diagnostics.push(safe(e.message));
  await client.connect(transport); clients.add(client); return client;
}
async function closeClient(client) { clients.delete(client); await client.close(); }
async function call(client, name, args = {}, options = {}) {
  const result = await client.callTool({ name, arguments: args }, undefined, { timeout: 20000, ...options });
  assert.ok(secrets.every(s => !JSON.stringify(result).includes(s)), 'MCP leaked a secret');
  const data = result.structuredContent || JSON.parse(result.content.filter(c => c.type === 'text').map(c => c.text).join(''));
  return { result, data };
}
async function success(client, name, args) { const { result, data } = await call(client, name, args); assert.notEqual(result.isError, true, JSON.stringify(data)); return data; }
const state = data => data.state ?? data.proxy?.state ?? data.status?.state;
async function expectError(client, code) { const { result, data } = await call(client, 'start_proxy', { account: 'managed' }); assert.equal(result.isError, true); assert.equal(data.error?.code ?? data.code, code); }
async function rejectedTunnel(port) {
  const socket = net.connect({ host: '127.0.0.1', port }); sockets.add(socket); socket.on('error', () => {}); socket.on('close', () => sockets.delete(socket));
  const response = new Promise((resolve, reject) => { let data = ''; socket.on('data', part => { data += part.toString(); if (data.includes('\r\n\r\n')) resolve(data); }); socket.on('error', reject); socket.on('end', () => resolve(data)); });
  socket.write('CONNECT localhost:443 HTTP/1.1\r\n\r\n');
  try { assert.match(await Promise.race([response, delay(3000).then(() => { throw new Error('Reverse CONNECT did not answer'); })]), /403 Forbidden/); } finally { socket.destroy(); }
}
async function test(name, run) { try { await run(); results.push({ name, ok: true }); } catch (e) { results.push({ name, ok: false, error: safe(e.stack) }); } }

const primary = await settings('primary');
let first, second, isolated;
try {
  await test('two MCPs share one native SSH; detach and external leave it alive', async () => {
    first = await clientFor(primary); second = await clientFor(primary); const before = attempts;
    const ready = await Promise.all([success(first, 'start_proxy', { account: 'managed' }), success(second, 'start_proxy', { account: 'managed' })]);
    assert.ok(ready.every(v => state(v) === 'ready')); assert.equal(attempts - before, 1); assert.equal(connections.size, 1);
    const equivalent=structuredClone(primary.config);equivalent.plumivox.managed_proxy.private_key_file=`${path.dirname(keyFile)}${path.sep}unused${path.sep}..${path.sep}${path.basename(keyFile)}`;
    await writeFile(primary.file,JSON.stringify(equivalent));await success(second,'start_proxy',{account:'managed'});assert.equal(attempts-before,1);await writeFile(primary.file,JSON.stringify(primary.config));
    await rejectedTunnel(primary.remote); await closeClient(first); first = null;
    assert.equal(state(await success(second, 'get_proxy_status')), 'ready'); assert.equal(connections.size, 1);
    const requestsBefore=apiRequests;await success(second,'post_tweet',{account:'external',text:'External route stays independent'});assert.equal(apiRequests,requestsBefore+1);
    assert.equal(state(await success(second,'get_proxy_status')),'ready');assert.equal(connections.size,1);
    const external = await call(second, 'start_proxy', { account: 'external' });
    assert.equal(external.result.isError, true); assert.equal(external.data.error.code, 'PROXY_EXTERNAL');
    assert.equal(state(await success(second, 'get_proxy_status')), 'ready'); assert.equal(connections.size, 1);
  });
  await test('SSH disconnect reconnects transport without another MCP start', async () => {
    assert.ok(second); const before = attempts;
    for (const connection of connections) connection.end();
    await until(() => attempts > before && connections.size === 1, 'Managed transport did not reconnect');
    await until(async () => state(await success(second, 'get_proxy_status')) === 'ready', 'Reconnected transport did not become ready');
    assert.equal(attempts, before + 1); await rejectedTunnel(primary.remote);
  });
  await test('different configuration has independent daemon; stop unbinds only its own ports', async () => {
    assert.ok(second); const other = await settings('isolated'); isolated = await clientFor(other);
    assert.equal(state(await success(isolated, 'start_proxy', { account: 'managed' })), 'ready'); assert.equal(connections.size, 2);
    const before = unforwarded; assert.equal(state(await success(second, 'stop_proxy')), 'stopped');
    await until(() => connections.size === 1, 'Stopping primary did not disconnect only primary'); assert.equal(unforwarded, before + 1);
    assert.equal(state(await success(isolated, 'get_proxy_status')), 'ready'); await rejectedTunnel(other.remote);
    const probe = net.createServer(); await new Promise((resolve, reject) => { probe.once('error', reject); probe.listen(primary.local, '127.0.0.1', resolve); }); await close(probe);
    await success(isolated, 'stop_proxy'); await until(() => connections.size === 0, 'Isolated SSH did not stop');
    await closeClient(second); second = null; await closeClient(isolated); isolated = null;
  });
  await test('cancelled readiness waiter does not cancel another MCP or shared SSH', async () => {
    mode = 'stall'; const a = await clientFor(primary), b = await clientFor(primary); const abort = new AbortController();
    const cancelled = call(a, 'start_proxy', { account: 'managed' }, { signal: abort.signal }).then(value => ({ value }), error => ({ error }));
    const surviving = success(b, 'start_proxy', { account: 'managed' });
    await until(() => delayed.length > 0, 'Authentication was not held'); abort.abort(); await cancelled;
    mode = 'accept'; for (const context of delayed.splice(0)) authenticate(context);
    assert.equal(state(await surviving), 'ready'); assert.equal(connections.size, 1);
    await closeClient(a); assert.equal(state(await success(b, 'get_proxy_status')), 'ready'); await rejectedTunnel(primary.remote);
    await success(b, 'stop_proxy'); await until(() => connections.size === 0, 'Cancelled waiter test did not stop SSH'); await closeClient(b);
  });
  await test('same MCP stop cancels pending posting readiness before any API request', async()=>{
    mode='stall';const client=await clientFor(primary);const before=apiRequests;
    const pending=call(client,'post_tweet',{account:'managed',text:'Do not transmit after stop'});
    await until(()=>delayed.length>0,'Posting did not wait for SSH authentication');
    const stopping=success(client,'stop_proxy');
    const cancelled=await pending;assert.equal(cancelled.result.isError,true);assert.equal(cancelled.data.error.code,'PROXY_STOPPED');
    assert.equal(state(await stopping),'stopped');assert.equal(apiRequests,before);
    mode='accept';for(const context of delayed.splice(0)){try{context.reject();}catch{}}
    await until(()=>connections.size===0,'Stop retained pending SSH');await closeClient(client);
  });
  await test('simultaneous gated daemon launches both attach to the winning owner',async()=>{
    const config=await settings('bind-race');const gate=path.join(output,`bind-race-${randomUUID()}`);const env={PLUMIVOX_PROXY_TEST_BIND_GATE:gate};
    const a=await clientFor(config,env),b=await clientFor(config,env);const before=attempts;
    const pending=[a,b].map(client=>success(client,'start_proxy',{account:'managed'}).then(value=>({value}),error=>({error})));
    await until(async()=>(await readdir(output)).filter(name=>name.startsWith(path.basename(gate)+'.')&&name.endsWith('.waiting')).length===2,'Both daemon launches did not reach bind gate');
    await writeFile(`${gate}.release`,'release',{flag:'wx'});const ready=await Promise.all(pending);for(const result of ready){assert.equal(result.error,undefined,String(result.error));assert.equal(state(result.value),'ready');}
    assert.equal(attempts-before,1);await success(a,'stop_proxy');await until(()=>connections.size===0,'Bind-race owner did not stop');assert.equal(state(await success(b,'get_proxy_status')),'stopped');assert.equal(state(await success(b,'stop_proxy')),'stopped');await closeClient(a);await closeClient(b);
  });
  await test('stop before daemon bind waits for pending launch and prevents late resurrection',async()=>{
    const config=await settings('bind-gated');const gate=path.join(output,`bind-gate-${randomUUID()}`);
    const a=await clientFor(config,{PLUMIVOX_PROXY_TEST_BIND_GATE:gate}),b=await clientFor(config,{PLUMIVOX_PROXY_TEST_BIND_GATE:gate});
    const pendingA=call(a,'start_proxy',{account:'managed'}).then(value=>({value}),error=>({error}));
    await until(async()=>(await readdir(output)).some(name=>name.startsWith(path.basename(gate)+'.')&&name.endsWith('.waiting')),'Daemon did not reach pre-bind gate');
    const cancelB=new AbortController();const pendingB=call(b,'start_proxy',{account:'managed'},{signal:cancelB.signal}).then(value=>({value}),error=>({error}));
    await until(async()=>(await readdir(output)).filter(name=>name.startsWith(path.basename(gate)+'.')&&name.endsWith('.waiting')).length===2,'Second startup did not reach bind gate');
    const stoppedPromise=success(a,'stop_proxy');let stopped=false;stoppedPromise.then(()=>{stopped=true;});
    await delay(150);assert.equal(stopped,false,'Stop returned before pending daemon could bind');
    await writeFile(`${gate}.release`,'release',{flag:'wx'});
    const cancelled=await pendingA;assert.equal(cancelled.value?.data.error.code,'PROXY_STOPPED');
    assert.equal(state(await stoppedPromise),'stopped');await pendingB;await delay(1200);
    assert.equal(connections.size,0,'Another startup waiter resurrected explicitly stopped daemon');
    await closeClient(a);await closeClient(b);
  });
  await test('same MCP retains earlier configured daemon beyond idle while using another setting',async()=>{
    const config=await settings('retained');const original=JSON.stringify(config.config);const client=await clientFor(config);
    assert.equal(state(await success(client,'start_proxy',{account:'managed'})),'ready');
    const alternate=structuredClone(config.config);alternate.plumivox.managed_proxy.local_port=await freePort();alternate.plumivox.managed_proxy.remote_port=await freePort();
    await writeFile(config.file,JSON.stringify(alternate));assert.equal(state(await success(client,'start_proxy',{account:'managed'})),'ready');
    await delay(2300);assert.equal(connections.size,2,'Changing configuration detached the in-flight prior proxy');
    await rejectedTunnel(config.remote);await success(client,'stop_proxy');await until(()=>connections.size===1,'Stopping latest configuration stopped earlier proxy too');
    await writeFile(config.file,original);assert.equal(state(await success(client,'start_proxy',{account:'managed'})),'ready');assert.equal(connections.size,1);
    await success(client,'stop_proxy');await closeClient(client);await until(()=>connections.size===0,'Retained proxy did not stop');
  });
  await test('unanswered unforward cannot hang stop and active partial CONNECT is closed',async()=>{
    const config=await settings('forced-stop');const client=await clientFor(config);await success(client,'start_proxy',{account:'managed'});
    const socket=net.connect({host:'127.0.0.1',port:config.remote});sockets.add(socket);socket.on('error',()=>{});socket.on('close',()=>sockets.delete(socket));
    const closed=new Promise(resolve=>socket.once('close',resolve));socket.write('CONNECT x.com:443 HTTP/1.1\r\n');
    await until(async()=>{const status=await success(client,'get_proxy_status');return status.activeConnections===1;},'Partial CONNECT was not active');
    mode='unforward-stall';await Promise.race([success(client,'stop_proxy'),delay(5000).then(()=>{throw new Error('Unanswered unforward hung stop');})]);
    await closed;await until(()=>connections.size===0,'Forced stop retained SSH');mode='accept';await closeClient(client);
  });
  for(const [label,content,code] of [['empty','', 'PROXY_KEY_UNAVAILABLE'],['oversized',Buffer.alloc(1024*1024+1),'PROXY_KEY_TOO_LARGE'],['directory',null,'PROXY_KEY_UNAVAILABLE'],['missing',undefined,'PROXY_KEY_UNAVAILABLE']])await test(`${label} key rejected before SSH and posting API`,async()=>{
    const key=path.join(output,`${label}-key`);if(content===null)await mkdir(key);else if(content!==undefined)await writeFile(key,content,{flag:'wx'});
    const config=await settings(`key-${label}`,{private_key_file:key});const client=await clientFor(config);const before=attempts,requests=apiRequests;
    await expectError(client,code);const post=await call(client,'post_tweet',{account:'managed',text:'Invalid key must never transmit'});assert.equal(post.data.error.code,code);assert.equal(attempts,before);assert.equal(apiRequests,requests);
    await closeClient(client);
  });
  for(const side of ['local','remote'])await test(`${side} port owner is preserved on bind conflict`,async()=>{
    const owner=net.createServer();const port=await listen(owner);const config=await settings(`port-${side}`,{[`${side}_port`]:port});const client=await clientFor(config);
    try{await expectError(client,side==='local'?'PROXY_PORT_BUSY':'PROXY_REMOTE_BIND_FAILED');assert.equal(owner.listening,true);await success(client,'stop_proxy');}finally{await closeClient(client);await close(owner);}
  });
  for (const [name, selectedMode, overrides, code] of [
    ['host pin mismatch', 'accept', { host_key_sha256: `SHA256:${Buffer.alloc(32).toString('base64').replace(/=+$/, '')}` }, 'PROXY_HOST_KEY_MISMATCH'],
    ['public key authentication rejected', 'auth-reject', {}, 'PROXY_AUTH_FAILED'],
    ['remote bind rejected', 'bind-reject', {}, 'PROXY_REMOTE_BIND_FAILED'],
  ]) await test(name, async () => {
    mode = selectedMode; const config = await settings(`failure-${results.length}`, overrides); const client = await clientFor(config);
    await expectError(client, code); const status = await success(client, 'get_proxy_status'); assert.equal(state(status), 'failed'); assert.equal(status.lastErrorCode, code);
    await success(client, 'stop_proxy'); await closeClient(client); await until(() => connections.size === 0, 'Failed SSH leaked a connection'); mode = 'accept';
  });
  await test('final MCP departure stops native daemon after idle period', async () => {
    const config = await settings('idle'); const client = await clientFor(config);
    assert.equal(state(await success(client, 'start_proxy', { account: 'managed' })), 'ready');
    await closeClient(client); await until(() => connections.size === 0, 'Last client departure retained SSH beyond fixture idle period');
    const probe = net.createServer(); await new Promise((resolve, reject) => { probe.once('error', reject); probe.listen(config.local, '127.0.0.1', resolve); }); await close(probe);
  });
  await test('diagnostics and protocol never expose credentials', async () => { assert.equal(leaked, false); });
} finally {
  mode = 'accept'; for (const context of delayed.splice(0)) { try { context.reject(); } catch {} }
  for (const client of clients) { try { await call(client, 'stop_proxy'); } catch {} try { await client.close(); } catch {} }
  for (const socket of sockets) socket.destroy(); for (const connection of connections) connection.end();
  for (const server of listeners) server.close(); await close(ssh); await close(api);
  const summary = { total: results.length, passed: results.filter(r => r.ok).length, failed: results.filter(r => !r.ok).length, results, output };
  await writeFile(path.join(output, 'summary.json'), JSON.stringify(summary, null, 2));
  await writeFile(path.join(output, 'ssh-trace.json'), JSON.stringify(trace, null, 2));
  await writeFile(path.join(output, 'diagnostics.txt'), diagnostics.join('\n'));
  console.log(JSON.stringify(summary)); if (summary.failed) process.exitCode = 1;
}
