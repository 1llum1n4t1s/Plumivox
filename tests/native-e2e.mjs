import assert from 'node:assert/strict';
import http from 'node:http';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { mkdir, writeFile, readFile, realpath, lstat } from 'node:fs/promises';
import { randomUUID } from 'node:crypto';
import { Client } from '@modelcontextprotocol/sdk/client/index.js';
import { StdioClientTransport } from '@modelcontextprotocol/sdk/client/stdio.js';
import { isInside } from './path-containment.mjs';

// Rustの製品ハンドラーを別プロセスのstdio経由で検証する。接続先変更はtest-fixtureビルドだけが受け付ける。
const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const executable = path.resolve(process.env.PLUMIVOX_NATIVE_FIXTURE || path.join(repo, 'target', 'debug', `plumivox-fixture${process.platform === 'win32' ? '.exe' : ''}`));
assert.ok((await lstat(executable)).isFile(), 'Build the Rust test-fixture executable first');
const output = path.resolve(process.env.PLUMIVOX_NATIVE_E2E_OUTPUT_DIR || path.join(await realpath(os.tmpdir()), 'Codex', 'Plumivox', randomUUID()));
const forbidden = [repo, path.join(os.homedir(), '.codex'), ...(process.env.CODEX_HOME ? [path.resolve(process.env.CODEX_HOME)] : [])];
let ancestor = output;
while (true) {
  try { await lstat(ancestor); break; }
  catch (error) { if (error.code !== 'ENOENT') throw error; ancestor = path.dirname(ancestor); }
}
for (let current = ancestor; ; current = path.dirname(current)) {
  assert.ok(!(await lstat(current)).isSymbolicLink(), 'Evidence path must not cross a symbolic link or junction');
  if (path.dirname(current) === current) break;
}
const resolvedOutput = path.join(await realpath(ancestor), path.relative(ancestor, output));
assert.ok(!forbidden.some((root) => isInside(output, root) || isInside(resolvedOutput, root)), 'Evidence must be outside repository and Codex user directory');
assert.ok(!output.split(path.sep).some((part) => part.toLowerCase() === 'artifacts'));
await mkdir(output, { recursive: true });
const secretsPath = path.join(output, 'synthetic-secrets.json');
const fakeKey = 'synthetic-native-api-key-never-display';
const fakeCookie = 'synthetic-native-cookie-never-display';
const config = { twitterapi_io: {
  posting: { user_name: 'Poster', api_key: fakeKey, proxy: 'http://127.0.0.1:8888', login_cookies: fakeCookie },
  reading: { user_name: 'Reader', api_key: fakeKey },
} };
const initialSecrets = JSON.stringify(config);
await writeFile(secretsPath, initialSecrets, { flag: 'wx' });
const pngPath = path.join(output, 'one-pixel.png');
await writeFile(pngPath, Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+/l9sAAAAASUVORK5CYII=', 'base64'), { flag: 'wx' });
const trace = [];
const results = [];
const diagnostics = [];
let scenario = 'normal';
let nextPost = 900;
let accepted;
let released;
let cancelledRequest;
let uploads = 0;
const tweet = (id) => ({ id, text: '日本語 🫱🏻‍🫲🏽', createdAt: '2026-10-12T12:00:00Z', author: { userName: 'PublicUser', name: 'Public User' }, url: `https://x.com/PublicUser/status/${id}` });
const mock = http.createServer(async (req, res) => {
  const url = new URL(req.url, 'http://127.0.0.1');
  const chunks = [];
  for await (const chunk of req) chunks.push(chunk);
  let body;
  try { body = JSON.parse(Buffer.concat(chunks).toString()); } catch { /* GETまたはmultipart */ }
  trace.push({ method: req.method, path: url.pathname, query: Object.fromEntries(url.searchParams), authenticated: req.headers['x-api-key'] === fakeKey, cookieHeaderPresent: Boolean(req.headers.cookie), ...(body ? { postingPayloadValid: body.login_cookies === fakeCookie && body.proxy === config.twitterapi_io.posting.proxy && typeof body.tweet_text === 'string' } : {}) });
  const send = (value, status = 200) => { if (!res.destroyed) { res.writeHead(status, { 'content-type': 'application/json' }); res.end(JSON.stringify(value)); } };
  if (scenario === 'cancel') {
    released = () => send({ tweets: [tweet('502')], has_next_page: false, next_cursor: '' });
    cancelledRequest = new Promise((resolve) => res.once('close', () => resolve(!res.writableEnded)));
    accepted?.();
    return;
  }
  if (scenario === 'timeout') { const timer = setTimeout(() => send({ tweets: [tweet('502')] }), 1000); timer.unref(); return; }
  if (/^http\d+$/.test(scenario)) return send({ message: fakeCookie, api_key: fakeKey }, Number(scenario.slice(4)));
  if (scenario === 'html') { res.writeHead(200); res.end(`<html>${fakeCookie}</html>`); return; }
  if (scenario === 'jsonerror') return send({ status: 'error', message: fakeCookie });
  if (url.pathname.endsWith('/upload_media_v2')) {
    uploads++;
    if (scenario === 'upload-fail' || (scenario === 'upload-recover' && uploads === 1)) return send({ message: fakeCookie }, 502);
    return send({ status: 'success', media_id: '700', media_id_string: '700' });
  }
  if (url.pathname.endsWith('/create_tweet_v2')) return send(scenario === 'missing-id' ? { status: 'success' } : { status: 'success', tweet_id: String(nextPost++) });
  if (url.pathname.endsWith('/user/info')) return send({ data: { id: '101', userName: 'Reader' } });
  if (url.pathname.endsWith('/tweets')) return send({ tweets: [tweet('502')] });
  if (url.pathname.endsWith('/tweet/advanced_search')) return send({ tweets: [tweet('603')], has_next_page: !url.searchParams.get('cursor'), next_cursor: url.searchParams.get('cursor') ? '' : 'native-search-next' });
  if (url.pathname.endsWith('/user/last_tweets') || url.pathname.endsWith('/user/tweet_timeline')) return send({ tweets: [tweet('501')], has_next_page: false, next_cursor: '' });
  if (url.pathname.endsWith('/user/followings')) return send({ followings: [], has_next_page: false, next_cursor: '' });
  send({ message: 'Unhandled local fixture endpoint' }, 404);
});
await new Promise((resolve) => mock.listen(0, '127.0.0.1', resolve));
const env = Object.fromEntries(Object.entries(process.env).filter(([key, value]) => value !== undefined && !['PLUMIVOX_SECRETS_FILE', 'PLUMIVOX_DEFAULT_ACCOUNT', 'PLUMIVOX_STATE_DIR'].includes(key)));
const transport = new StdioClientTransport({ command: executable, args: [`http://127.0.0.1:${mock.address().port}`, secretsPath, path.join(output, 'auth-state'), '250', '--slow-image-upload'], cwd: repo, env, stderr: 'pipe' });
transport.stderr?.on('data', (chunk) => diagnostics.push(String(chunk)));
const client = new Client({ name: 'plumivox-native-e2e', version: '1.0.0' });
const safe = (value) => String(value).replaceAll(fakeKey, '[redacted]').replaceAll(fakeCookie, '[redacted]');
async function run(name, action) {
  scenario = 'normal';
  try { await action(); results.push({ name, passed: true }); }
  catch (error) { results.push({ name, passed: false, error: safe(error.stack || error) }); }
}
async function call(name, args, failure = false, noNetwork = false) {
  const before = trace.length;
  let result;
  try { result = await client.callTool({ name, arguments: args }); }
  catch (error) {
    if (!failure || error.code !== -32602) throw error;
    if (noNetwork) assert.equal(trace.length, before, 'Rejected input caused HTTP traffic');
    return { error: { code: 'MCP_INVALID_PARAMS' } };
  }
  assert.ok(!JSON.stringify(result).includes(fakeKey) && !JSON.stringify(result).includes(fakeCookie), 'MCP response exposed synthetic secret');
  const text = result.content?.filter((item) => item.type === 'text').map((item) => item.text).join('\n');
  let data = result.structuredContent;
  if (!data) {
    try { data = JSON.parse(text); }
    catch { assert.equal(result.isError, true); data = { error: { code: 'MCP_INVALID_PARAMS' } }; }
  }
  assert.equal(Boolean(result.isError), failure, safe(JSON.stringify(data)));
  if (failure) assert.ok(data.error?.code);
  if (noNetwork) assert.equal(trace.length, before, 'Rejected input caused HTTP traffic');
  return data;
}
try {
  await client.connect(transport);
  await run('MCP discovery', async () => assert.deepEqual((await client.listTools()).tools.map((tool) => tool.name).sort(), ['get_following_feed', 'get_my_posts', 'get_proxy_status', 'get_tweets', 'list_accounts', 'login_account', 'post_tweet', 'refresh_session', 'search_tweets', 'start_proxy', 'stop_proxy']));
  await run('List accounts without provider access', () => call('list_accounts', {}, false, true));
  await run('Ambiguous account rejected', async () => assert.equal((await call('search_tweets', { keyword: 'needle' }, true, true)).error.code, 'ACCOUNT_REQUIRED'));
  await run('Unknown account rejected', async () => assert.equal((await call('get_tweets', { account: 'missing', tweetIds: ['502'] }, true, true)).error.code, 'ACCOUNT_UNKNOWN'));
  await run('Read-only account cannot post', async () => assert.equal((await call('post_tweet', { account: 'reading', text: 'test' }, true, true)).error.code, 'POSTING_UNAVAILABLE'));
  await run('Tweet lookup preserves UTF-8', async () => assert.equal((await call('get_tweets', { account: 'reading', tweetIds: ['502'] })).tweets[0].text, tweet('502').text));
  await run('Own timeline', async () => assert.equal((await call('get_my_posts', { account: 'reading' })).tweets[0].id, '501'));
  await run('Empty following feed', async () => assert.deepEqual((await call('get_following_feed', { account: 'reading' })).tweets, []));
  for (const tweetIds of [[], ['bad'], ['502', 'bad']]) await run(`Invalid lookup IDs ${JSON.stringify(tweetIds)}`, () => call('get_tweets', { account: 'reading', tweetIds }, true, true));
  for (const text of ['', ' ', 'あ'.repeat(141)]) await run(`Invalid post text length ${text.length}`, () => call('post_tweet', { account: 'posting', text }, true, true));
  for (const [name, text, length] of [['Japanese', 'あ'.repeat(140), 280], ['Family emoji', '👨‍👩‍👧‍👦', 2], ['URL', `https://example.com/${'a'.repeat(300)}`, 23]]) await run(`${name} weighted text`, async () => { const data = await call('post_tweet', { account: 'posting', text }); assert.equal(data.weightedLength, length); assert.match(data.tweetId, /^\d+$/); });
  await run('Explicit repeated posts each sent once', async () => { const before = trace.length; const one = await call('post_tweet', { account: 'posting', text: 'same' }); const two = await call('post_tweet', { account: 'posting', text: 'same' }); assert.notEqual(one.tweetId, two.tweetId); assert.equal(trace.length - before, 2); });
  await run('Image upload precedes one post', async () => { const before = trace.length; await call('post_tweet', { account: 'posting', text: 'image', images: [pngPath] }); assert.deepEqual(trace.slice(before).map((request) => request.path.split('/').at(-1)), ['upload_media_v2', 'create_tweet_v2']); });
  await run('Recoverable upload retries without resending post', async () => { scenario = 'upload-recover'; uploads = 0; const before = trace.length; await call('post_tweet', { account: 'posting', text: 'image', images: [pngPath] }); assert.deepEqual(trace.slice(before).map((request) => request.path.split('/').at(-1)), ['upload_media_v2', 'upload_media_v2', 'create_tweet_v2']); });
  await run('Failed upload never creates a post', async () => { scenario = 'upload-fail'; const before = trace.length; await call('post_tweet', { account: 'posting', text: 'image', images: [pngPath] }, true); assert.ok(trace.length > before); assert.ok(trace.slice(before).every((request) => request.path.endsWith('/upload_media_v2'))); });
  await run('Search cursor continuation', async () => { const args = { account: 'reading', keyword: 'needle' }; const first = await call('search_tweets', args); assert.ok(first.nextCursor); await call('search_tweets', { ...args, cursor: first.nextCursor }); assert.equal(trace.at(-1).query.cursor, 'native-search-next'); await call('search_tweets', { ...args, keyword: 'changed', cursor: first.nextCursor }, true, true); });
  await run('Configured cookie login does not perform HTTP', async () => assert.equal((await call('login_account', { account: 'posting' }, false, true)).reused, true));
  await run('Default account reread and explicit selection retained', async () => {
    try { await writeFile(secretsPath, JSON.stringify({ twitterapi_io: { ...config.twitterapi_io, default_account: 'posting' } })); assert.equal((await call('search_tweets', { keyword: 'needle' })).account, 'posting'); await call('post_tweet', { account: 'reading', text: 'test' }, true, true); }
    finally { await writeFile(secretsPath, initialSecrets); }
  });
  await run('Invalid default does not silently select another account', async () => {
    try { await writeFile(secretsPath, JSON.stringify({ twitterapi_io: { ...config.twitterapi_io, default_account: 'missing' } })); assert.equal((await call('search_tweets', { keyword: 'needle' }, true, true)).error.code, 'ACCOUNT_DEFAULT_INVALID'); }
    finally { await writeFile(secretsPath, initialSecrets); }
  });
  await run('Unreadable selected configuration does not fall back', async () => {
    try { await writeFile(secretsPath, '{invalid synthetic JSON'); await call('get_tweets', { account: 'reading', tweetIds: ['502'] }, true, true); }
    finally { await writeFile(secretsPath, initialSecrets); }
  });
  for (const mode of ['http401', 'http429', 'http500', 'http502', 'http503', 'http504', 'html', 'jsonerror', 'timeout']) await run(`Safe provider failure without read retry: ${mode}`, async () => { scenario = mode; const before = trace.length; await call('get_tweets', { account: 'reading', tweetIds: ['502'] }, true); assert.equal(trace.length - before, 1); });
  for (const mode of ['http502', 'missing-id', 'timeout']) await run(`Uncertain post is not resent: ${mode}`, async () => { scenario = mode; const before = trace.length; await call('post_tweet', { account: 'posting', text: 'test' }, true); assert.equal(trace.length - before, 1); });
  await run('Cancellation reaches actual request and does not resend', async () => {
    scenario = 'cancel';
    const before = trace.length;
    const ready = new Promise((resolve) => { accepted = resolve; });
    const controller = new AbortController();
    const request = client.callTool({ name: 'search_tweets', arguments: { account: 'reading', keyword: 'needle' } }, undefined, { signal: controller.signal, timeout: 5000 });
    const rejected = assert.rejects(request);
    let timer;
    try {
      await Promise.race([ready, new Promise((_, reject) => { timer = setTimeout(() => reject(new Error('Request did not reach local API')), 3000); })]);
      clearTimeout(timer);
      controller.abort();
      await rejected;
      assert.equal(await Promise.race([cancelledRequest, new Promise((_, reject) => { timer = setTimeout(() => reject(new Error('Cancellation did not close provider request')), 2000); })]), true);
      scenario = 'normal';
      await call('get_tweets', { account: 'reading', tweetIds: ['502'] });
      assert.equal(trace.length - before, 2);
    }
    finally { clearTimeout(timer); accepted = undefined; controller.abort(); released?.(); await rejected; }
  });
  await run('Wire authentication and sanitized diagnostics', async () => { assert.ok(trace.length > 0); assert.ok(trace.every((request) => request.authenticated && !request.cookieHeaderPresent)); assert.ok(trace.filter((request) => request.path.endsWith('/create_tweet_v2')).every((request) => request.method === 'POST' && request.postingPayloadValid)); assert.ok(!diagnostics.join('').includes(fakeKey) && !diagnostics.join('').includes(fakeCookie)); assert.equal(await readFile(secretsPath, 'utf8'), initialSecrets); });
} catch (error) { results.push({ name: 'Native stdio connection', passed: false, error: safe(error.stack || error) }); }
finally {
  await client.close();
  mock.closeAllConnections();
  await new Promise((resolve) => mock.close(resolve));
  await writeFile(path.join(output, 'http-trace.json'), JSON.stringify(trace, null, 2));
  const summary = { total: results.length, passed: results.filter((result) => result.passed).length, failed: results.filter((result) => !result.passed).length, results, retainedFiles: [secretsPath, pngPath, path.join(output, 'auth-state')], retention: 'Synthetic configuration, image and any encrypted sessions are retained for the caller to clean through the approved platform workflow after inspecting the summary and HTTP trace.' };
  await writeFile(path.join(output, 'summary.json'), JSON.stringify(summary, null, 2));
  console.log(JSON.stringify({ output, total: summary.total, passed: summary.passed, failed: summary.failed }));
  if (summary.failed) process.exitCode = 1;
}
