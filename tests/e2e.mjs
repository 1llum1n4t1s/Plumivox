import assert from 'node:assert/strict';
import http from 'node:http';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { mkdir, writeFile, realpath, stat, readFile } from 'node:fs/promises';
import { randomUUID, createHash } from 'node:crypto';
import { Client } from '@modelcontextprotocol/sdk/client/index.js';
import { StdioClientTransport } from '@modelcontextprotocol/sdk/client/stdio.js';

const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const output = path.resolve(process.env.PLUMIVOX_E2E_OUTPUT_DIR || path.join(os.tmpdir(), 'Codex', 'Plumivox', randomUUID()));
const isInside = (child, parent) => { const relative = path.relative(parent, child); return relative === '' || (!relative.startsWith('..') && !path.isAbsolute(relative)); };
const forbidden = [repo, path.join(os.homedir(), '.codex'), ...(process.env.CODEX_HOME ? [path.resolve(process.env.CODEX_HOME)] : [])];
let ancestor = output;
while (true) { try { await stat(ancestor); break; } catch (error) { if (error.code !== 'ENOENT') throw error; const next = path.dirname(ancestor); if (next === ancestor) throw error; ancestor = next; } }
const resolvedOutput = path.join(await realpath(ancestor), path.relative(ancestor, output));
assert.ok(!forbidden.some((root) => isInside(output, root) || isInside(resolvedOutput, root)), 'Evidence must be outside repository and Codex user directory');
assert.ok(!output.split(path.sep).some((part) => part.toLowerCase() === 'artifacts'), 'Do not create an artifacts directory');
await mkdir(output, { recursive: true });
const secretsPath = path.join(output, 'synthetic-secrets.json');
const pngPath = path.join(output, 'one-pixel.png');
const invalidPath = path.join(output, 'invalid.png');
const oversizedPath = path.join(output, 'oversized.png');
const fakeKey = 'synthetic-api-secret-never-display';
const fakeCookie = 'synthetic-cookie-secret-never-display';
const fakePassword = 'synthetic-password-never-display';
const fakeSeed = 'JBSWY3DPEHPK3PXP';
const stateDirectory = path.join(output, 'auth-state');
const syntheticConfig = { twitterapi_io: {
  posting: { user_name: 'Poster', api_key: fakeKey, proxy: 'http://127.0.0.1:8888', login_cookies: fakeCookie },
  reading: { user_name: 'Reader', api_key: fakeKey },
  broken: { user_name: 'Broken', api_key: '', email: 'auth@example.invalid', password: fakePassword, proxy: 'http://127.0.0.1:8888' },
  auth: { user_name: 'AuthUser', api_key: fakeKey, proxy: 'http://127.0.0.1:8888', email: 'auth@example.invalid', password: fakePassword, totp_secret: fakeSeed },
  noCredentials: { user_name: 'NoCredentials', api_key: fakeKey, proxy: 'http://127.0.0.1:8888' },
  badSeed: { user_name: 'BadSeed', api_key: fakeKey, proxy: 'http://127.0.0.1:8888', email: 'auth@example.invalid', password: fakePassword, totp_secret: '123456' },
} };
const initialSecrets = JSON.stringify(syntheticConfig);
await writeFile(secretsPath, initialSecrets);
const mismatchedSecretsPath = path.join(output, 'synthetic-mismatched-secrets.json');
const mismatchedConfig = structuredClone(syntheticConfig);
mismatchedConfig.twitterapi_io.auth.api_key = 'synthetic-mismatched-api-key';
await writeFile(mismatchedSecretsPath, JSON.stringify(mismatchedConfig));
const singleAccountSecretsPath = path.join(output, 'synthetic-single-account-secrets.json');
await writeFile(singleAccountSecretsPath, JSON.stringify({ twitterapi_io: { auth: syntheticConfig.twitterapi_io.auth } }));
const ownSessionPath = path.join(stateDirectory, createHash('sha256').update('auth\nauthuser').digest('hex'), 'session.dpapi');
const corruptedStateDirectory = path.join(output, 'corrupted-auth-state');
const corruptedPostingDirectory = path.join(corruptedStateDirectory, createHash('sha256').update('posting\nposter').digest('hex'));
await mkdir(corruptedPostingDirectory, { recursive: true });
await writeFile(path.join(corruptedPostingDirectory, 'session.dpapi'), 'deliberately invalid synthetic encrypted session');
const png = Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+/l9sAAAAASUVORK5CYII=', 'base64');
await writeFile(pngPath, png);
await writeFile(invalidPath, 'not an image');
await writeFile(oversizedPath, Buffer.concat([png, Buffer.alloc(5 * 1024 * 1024)]));

const trace = [];
const results = [];
let scenario = 'normal';
let nextPost = 900;
let uploadAccepted;
let loginAccepted;
let releaseHeldLogin;
let activeLogins = 0;
let peakLogins = 0;
const tweet = (id, createdAt = '2026-10-12T12:00:00Z') => ({ id, text: `Tweet ${id}`, createdAt, author: { userName: 'PublicUser', name: 'Public User' }, url: `https://x.com/PublicUser/status/${id}` });
const mock = http.createServer(async (req, res) => {
  const url = new URL(req.url, 'http://127.0.0.1');
  const chunks = [];
  for await (const chunk of req) chunks.push(chunk);
  const body = Buffer.concat(chunks);
  let payload;
  if (req.headers['content-type']?.startsWith('application/json')) { try { payload = JSON.parse(body); } catch {} }
  trace.push({ method: req.method, path: url.pathname, query: Object.fromEntries(url.searchParams), authenticated: req.headers['x-api-key'] === fakeKey, cookieHeaderPresent: Boolean(req.headers.cookie), contentType: req.headers['content-type'] || null, bodyBytes: body.length,
    ...(url.pathname.endsWith('/create_tweet_v2') ? { postingPayloadValid: payload?.login_cookies === fakeCookie && payload?.proxy === 'http://127.0.0.1:8888' && typeof payload?.tweet_text === 'string', mediaIds: payload?.media_ids ?? [] } : {}),
    ...(url.pathname.endsWith('/upload_media_v2') ? { multipartPayloadValid: body.includes(Buffer.from(fakeCookie)) && body.includes(Buffer.from('name="proxy"')) && body.includes(Buffer.from('name="file"')) && body.includes(png) } : {}),
    ...(url.pathname.endsWith('/user_login_v2') ? { loginPayloadValid: payload?.user_name === 'AuthUser' && payload?.email === 'auth@example.invalid' && payload?.password === fakePassword && payload?.proxy === 'http://127.0.0.1:8888', totpSeedExact: payload?.totp_secret === fakeSeed, loginPayloadKeys: Object.keys(payload || {}).sort() } : {}),
  });
  const send = (data, code = 200) => { res.writeHead(code, { 'content-type': 'application/json' }); res.end(JSON.stringify(data)); };
  if (/^http(401|429|500)$/.test(scenario)) return send({ message: fakeKey }, Number(scenario.slice(4)));
  if (scenario === 'html') { res.writeHead(200, { 'content-type': 'text/html' }); return res.end(`<html>${fakeCookie}</html>`); }
  if (scenario === 'jsonerror') return send({ status: 'error', msg: fakeCookie });
  if (scenario === 'timeout') { setTimeout(() => { if (!res.destroyed) send({ tweet_id: '999' }); }, 700).unref(); return; }
  if (url.pathname.endsWith('/user_login_v2')) {
    if (scenario === 'interprocess-login') {
      activeLogins++;
      peakLogins = Math.max(peakLogins, activeLogins);
      res.once('close', () => { activeLogins--; });
      if (!releaseHeldLogin) {
        releaseHeldLogin = () => { if (!res.destroyed && !res.writableEnded) send({ status: 'success', login_cookie: fakeCookie }); };
        loginAccepted?.();
        return;
      }
      return send({ status: 'success', login_cookie: fakeCookie });
    }
    if (scenario === 'login-delay') {
      loginAccepted?.();
      setTimeout(() => { if (!res.destroyed) send({ status: 'success', login_cookie: fakeCookie }); }, 150).unref();
      return;
    }
    return send(scenario === 'login-missing-cookie' ? { status: 'success' } : scenario === 'login-plural-cookie' ? { status: 'success', login_cookies: fakeCookie } : { status: 'success', login_cookie: fakeCookie });
  }
  if (url.pathname.endsWith('/user/info')) return send({ data: { id: '101', userName: 'Reader' } });
  if (url.pathname.endsWith('/user/last_tweets') || url.pathname.endsWith('/user/tweet_timeline')) {
    if (scenario === 'timeline-nested') {
      const continuation = url.searchParams.get('cursor') === 'nested-timeline-next';
      return send({ status: 'success', code: 0, msg: 'success', data: { tweets: [tweet(continuation ? '505' : '504')] }, has_next_page: !continuation, next_cursor: continuation ? '' : 'nested-timeline-next' });
    }
    if (scenario === 'timeline-nested-invalid') return send({ status: 'success', data: { tweets: {} }, has_next_page: false, next_cursor: '' });
    if (scenario === 'timeline-nested-missing') return send({ status: 'success', data: {}, has_next_page: false, next_cursor: '' });
    if (scenario === 'timeline-nested-array') return send({ status: 'success', data: [tweet('504')], has_next_page: false, next_cursor: '' });
    if (scenario === 'timeline-invalid-top') return send({ status: 'success', tweets: {}, data: { tweets: [tweet('504')] }, has_next_page: false, next_cursor: '' });
    return send({ tweets: [tweet('501')], has_next_page: true, next_cursor: 'timeline-next' });
  }
  if (url.pathname.endsWith('/tweets')) return send({ tweets: [tweet('502')] });
  if (url.pathname.endsWith('/user/followings')) {
    if (url.searchParams.get('cursor') === 'following-next') return send({ followings: [{ userName: 'LaterPublic', isProtected: false }], has_next_page: false, next_cursor: '' });
    return send({ followings: scenario === 'empty' ? [] : [...Array.from({ length: 25 }, (_, index) => ({ userName: `Public${index}`, isProtected: false })), { userName: 'Private', isProtected: true }], has_next_page: scenario === 'following-page', next_cursor: scenario === 'following-page' ? 'following-next' : '' });
  }
  if (url.pathname.endsWith('/tweet/advanced_search')) {
    const query = url.searchParams.get('query') || '';
    if (query.toLowerCase().includes('from:laterpublic')) return send({ tweets: [tweet('604')], has_next_page: false, next_cursor: '' });
    const second = query.toLowerCase().includes('from:public20');
    return send({ tweets: second ? [tweet('601', '2026-10-12T13:00:00Z'), tweet('602', '2026-10-12T14:00:00Z')] : [tweet('603', '2026-10-12T11:00:00Z'), tweet('601', '2026-10-12T13:00:00Z')], has_next_page: !url.searchParams.get('cursor'), next_cursor: !url.searchParams.get('cursor') ? 'search-next' : '' });
  }
  if (url.pathname.endsWith('/upload_media_v2')) {
    if (scenario === 'cancel-upload') {
      uploadAccepted?.();
      setTimeout(() => { if (!res.destroyed) send({ status: 'success', media_id: '700', media_id_string: '700' }); }, 150).unref();
      return;
    }
    return send({ status: 'success', media_id: '700', media_id_string: '700' });
  }
  if (url.pathname.endsWith('/create_tweet_v2')) return send(scenario === 'missing-id' ? { status: 'success' } : { status: 'success', tweet_id: String(nextPost++) });
  send({ status: 'error', message: 'Unhandled local mock endpoint' }, 404);
});
await new Promise((resolve) => mock.listen(0, '127.0.0.1', resolve));
const apiBaseUrl = `http://127.0.0.1:${mock.address().port}`;
const makeTransport = (configPath = secretsPath, sessionRoot = stateDirectory, timeoutMs = 250) => new StdioClientTransport({ command: process.execPath, args: [path.join(repo, 'tests/fixture-server.mjs'), apiBaseUrl, configPath, sessionRoot, String(timeoutMs)], cwd: repo, env: Object.fromEntries(Object.entries(process.env).filter(([key, value]) => value !== undefined && !['PLUMIVOX_SECRETS_FILE', 'PLUMIVOX_DEFAULT_ACCOUNT', 'PLUMIVOX_STATE_DIR'].includes(key))), stderr: 'pipe' });
let transport = makeTransport();
let client = new Client({ name: 'plumivox-e2e', version: '1.0.0' });
const protocolErrors = [];
const diagnostics = [];
client.onerror = (error) => protocolErrors.push(safe(error.message));
transport.stderr?.on('data', (data) => diagnostics.push(String(data)));
const sensitiveValues = [fakeKey, fakeCookie, fakePassword, fakeSeed, 'http://127.0.0.1:8888'];
const safe = (value) => sensitiveValues.reduce((text, secret) => text.replaceAll(secret, '[REDACTED]'), String(value)).replaceAll(secretsPath, '[SYNTHETIC_CONFIG]');
const attachDiagnostics = () => { client.onerror = (error) => protocolErrors.push(safe(error.message)); transport.stderr?.on('data', (data) => diagnostics.push(String(data))); };
const restart = async (configPath = secretsPath, sessionRoot = stateDirectory) => { await client.close(); transport = makeTransport(configPath, sessionRoot); client = new Client({ name: 'plumivox-e2e', version: '1.0.0' }); attachDiagnostics(); await client.connect(transport); };
const run = async (name, check) => { if (process.env.PLUMIVOX_E2E_CASE && name !== process.env.PLUMIVOX_E2E_CASE) return; try { await check(); results.push({ name, status: 'pass' }); } catch (error) { results.push({ name, status: 'fail', message: safe(error.message) }); } };
const call = async (name, args = {}, connection = client) => {
  const result = await connection.callTool({ name, arguments: args }, undefined, { timeout: 30000 });
  const text = result.content?.filter((item) => item.type === 'text').map((item) => item.text).join('\n');
  assert.ok(sensitiveValues.every((secret) => !JSON.stringify(result).includes(secret)), 'Secret leaked in MCP result');
  let data = result.structuredContent;
  if (!data) {
    try { data = JSON.parse(text); }
    catch {
      // 入力schemaの拒否はSDK標準のMCPエラー本文。アプリ処理エラーとは別に確認する。
      assert.equal(result.isError, true);
      assert.match(text, /^MCP error -32602: Input validation error:/);
      data = { error: { code: 'MCP_INVALID_PARAMS', message: 'SDK input validation error' } };
    }
  }
  return { result, data };
};
const succeeds = async (name, args) => { const { result, data } = await call(name, args); assert.notEqual(result.isError, true, JSON.stringify(data)); return data; };
const fails = async (name, args, noNetwork = false) => { const before = trace.length; const { result, data } = await call(name, args); assert.equal(result.isError, true); assert.ok(data.error?.code && data.error?.message); if (noNetwork) assert.equal(trace.length, before, 'Invalid input caused HTTP traffic'); return data; };
const assertLoginMetadata = (data, reused) => {
  assert.deepEqual(Object.keys(data).sort(), ['account', 'acquiredAt', 'postConfigured', 'reused', 'sessionSource', 'userName']);
  assert.equal(data.postConfigured, true);
  assert.equal(data.reused, reused);
  assert.ok(typeof data.sessionSource === 'string');
  if (!reused) assert.ok(Number.isFinite(Date.parse(data.acquiredAt)));
};
try {
  await client.connect(transport);
  // connect後もSDKが標準出力の非JSON行を検出する。
  const existingOnError = transport.onerror;
  transport.onerror = (error) => { protocolErrors.push(safe(error.message)); existingOnError?.(error); };
  await run('MCP discovery', async () => { const tools = await client.listTools(); assert.deepEqual(tools.tools.map((tool) => tool.name).sort(), ['get_following_feed', 'get_my_posts', 'get_tweets', 'list_accounts', 'login_account', 'post_tweet', 'refresh_session']); });
  await run('Account metadata without secrets', async () => { const data = await succeeds('list_accounts'); assert.equal(data.accounts.length, Object.keys(syntheticConfig.twitterapi_io).length); assert.equal(data.accounts.find((item) => item.account === 'reading').readConfigured, true); assert.equal(data.accounts.find((item) => item.account === 'reading').postConfigured, false); });
  await run('Ambiguous account rejected before HTTP', () => fails('post_tweet', { text: 'hello' }, true));
  await run('Missing API key rejected before HTTP', () => fails('get_my_posts', { account: 'broken' }, true));
  await run('Cookie-free read and timeline pagination', async () => { const data = await succeeds('get_my_posts', { account: 'reading', includeReplies: false }); assert.equal(data.account, 'reading'); assert.equal(data.tweets[0].id, '501'); assert.equal(data.nextCursor, 'timeline-next'); assert.equal(data.hasNextPage, true); await succeeds('get_my_posts', { account: 'reading', cursor: data.nextCursor }); assert.equal(trace.at(-1).query.cursor, 'timeline-next'); });
  await run('Nested real timeline response preserves top-level pagination', async () => {
    scenario = 'timeline-nested';
    try {
      const data = await succeeds('get_my_posts', { account: 'reading' });
      assert.equal(data.account, 'reading');
      assert.deepEqual(data.tweets.map((item) => item.id), ['504']);
      assert.equal(data.hasNextPage, true);
      assert.equal(data.nextCursor, 'nested-timeline-next');
      const continued = await succeeds('get_my_posts', { account: 'reading', cursor: data.nextCursor });
      assert.equal(trace.at(-1).query.cursor, 'nested-timeline-next');
      assert.deepEqual(continued.tweets.map((item) => item.id), ['505']);
      assert.equal(continued.hasNextPage, false);
      assert.equal(continued.nextCursor, null);
    } finally { scenario = 'normal'; }
  });
  for (const problem of ['timeline-nested-invalid', 'timeline-nested-missing', 'timeline-nested-array', 'timeline-invalid-top']) await run(`${problem} response rejected`, async () => {
    scenario = problem;
    try { const data = await fails('get_my_posts', { account: 'reading' }); assert.equal(data.error.code, 'INVALID_RESPONSE'); }
    finally { scenario = 'normal'; }
  });
  await run('Tweet lookup', async () => { const data = await succeeds('get_tweets', { account: 'reading', tweetIds: ['502'] }); assert.equal(data.tweets[0].id, '502'); });
  await run('Non-numeric tweet ID rejected', () => fails('get_tweets', { account: 'reading', tweetIds: ['bad'] }, true));
  await run('Missing Cookie rejected before HTTP', () => fails('post_tweet', { account: 'reading', text: 'hello' }, true));
  for (const [name, args] of [ ['Empty text', { text: '' }], ['Japanese 141 characters', { text: 'あ'.repeat(141) }], ['Relative image', { text: 'image', images: ['relative.png'] }], ['Invalid image format', { text: 'image', images: [invalidPath] }], ['Invalid second image', { text: 'image', images: [pngPath, invalidPath] }], ['Oversized image', { text: 'image', images: [oversizedPath] }], ['More than four images', { text: 'image', images: Array(5).fill(pngPath) }] ]) await run(`${name} rejected before HTTP`, () => fails('post_tweet', { account: 'posting', ...args }, true));
  for (const [name, text, length] of [['Japanese 140 characters', 'あ'.repeat(140), 280], ['Compound family emoji', '👨‍👩‍👧‍👦', 2], ['Long URL', `https://example.com/${'a'.repeat(300)}`, 23]]) await run(name, async () => { const data = await succeeds('post_tweet', { account: 'posting', text }); assert.equal(data.weightedLength, length); assert.ok(/^\d+$/.test(data.tweetId)); assert.ok(data.url.includes(data.tweetId)); });
  await run('Explicit duplicate posts send twice', async () => { const before = trace.filter((item) => item.path.endsWith('/create_tweet_v2')).length; await succeeds('post_tweet', { account: 'posting', text: 'explicit duplicate' }); await succeeds('post_tweet', { account: 'posting', text: 'explicit duplicate' }); assert.equal(trace.filter((item) => item.path.endsWith('/create_tweet_v2')).length - before, 2); });
  await run('Image uses multipart upload then one post', async () => { const before = trace.length; await succeeds('post_tweet', { account: 'posting', text: 'image', images: [pngPath] }); const requests = trace.slice(before); assert.equal(requests.filter((item) => item.path.endsWith('/upload_media_v2')).length, 1); const upload = requests.find((item) => item.path.endsWith('/upload_media_v2')); assert.match(upload.contentType, /^multipart\/form-data; boundary=/); assert.equal(upload.multipartPayloadValid, true); assert.equal(requests.filter((item) => item.path.endsWith('/create_tweet_v2')).length, 1); assert.deepEqual(requests.find((item) => item.path.endsWith('/create_tweet_v2')).mediaIds, ['700']); });
  await run('MCP cancellation during upload prevents tweet creation', async () => {
    scenario = 'cancel-upload';
    const before = trace.length;
    const controller = new AbortController();
    let acceptanceTimer;
    const accepted = new Promise((resolve, reject) => {
      uploadAccepted = resolve;
      acceptanceTimer = setTimeout(() => reject(new Error('Mock did not receive image upload')), 1000);
    });
    const request = client.callTool({ name: 'post_tweet', arguments: { account: 'posting', text: 'cancel during upload', images: [pngPath] } }, undefined, { signal: controller.signal, timeout: 5000 });
    const cancelled = assert.rejects(request);
    try {
      await accepted;
      controller.abort();
      await cancelled;
      // アップロード応答予定(150ms)を過ぎても投稿要求が始まらないことを確認する。
      await new Promise((resolve) => setTimeout(resolve, 220));
      const requests = trace.slice(before);
      assert.equal(requests.filter((item) => item.path.endsWith('/upload_media_v2')).length, 1);
      assert.equal(requests.filter((item) => item.path.endsWith('/create_tweet_v2')).length, 0);
    } finally {
      clearTimeout(acceptanceTimer);
      uploadAccepted = undefined;
      controller.abort();
      await cancelled;
      scenario = 'normal';
    }
  });
  for (const problem of ['http401', 'http429', 'http500', 'html', 'jsonerror', 'missing-id', 'timeout']) await run(`${problem} is safe failure with no retry`, async () => {
    scenario = problem;
    const before = trace.length;
    try {
      const data = await fails('post_tweet', { account: 'posting', text: 'failure probe' });
      assert.equal(trace.length - before, 1);
      if (['http500', 'html', 'timeout'].includes(problem)) {
        assert.equal(data.error.code, 'POST_UNCERTAIN');
        assert.match(data.error.message, /成否は未確定/);
        assert.match(data.error.message, /投稿済みの可能性/);
        assert.match(data.error.message, /自動再送はしていません/);
      } else if (['http401', 'http429'].includes(problem)) {
        assert.equal(data.error.code, 'API_HTTP_ERROR');
      } else if (problem === 'jsonerror') {
        assert.equal(data.error.code, 'API_REJECTED');
      }
    } finally { scenario = 'normal'; }
  });
  let cursor;
  await run('Feed batches deduplicate and sort with coverage', async () => { const before = trace.length; const data = await succeeds('get_following_feed', { account: 'reading' }); assert.equal(data.mode, 'reconstructed_following_feed'); assert.equal(data.officialHomeEquivalent, false); assert.deepEqual(data.tweets.map((item) => item.id), ['602', '601', '603']); assert.equal(data.coverage.protectedAccountsExcluded, 1); assert.equal(data.hasNextPage, true); assert.ok(data.nextCursor); cursor = data.nextCursor; const search = trace.slice(before).filter((item) => item.path.endsWith('/advanced_search')); assert.equal(search.length, 2); for (const request of search) { assert.ok((request.query.query.match(/from:/g) || []).length <= 20); assert.ok(request.query.query.includes('include:nativeretweets')); assert.equal(request.query.queryType, 'Latest'); assert.ok(!request.query.query.includes('from:Private')); } });
  await run('Feed cursor account binding rejects before HTTP', async () => { assert.ok(cursor, 'Feed did not provide cursor'); await fails('get_following_feed', { account: 'posting', cursor }, true); });
  await run('Malformed cursor rejects before HTTP', () => fails('get_following_feed', { account: 'reading', cursor: 'bad-cursor!' }, true));
  await run('Feed continuation', async () => { assert.ok(cursor); const data = await succeeds('get_following_feed', { account: 'reading', cursor }); assert.equal(data.hasNextPage, false); });
  await run('Empty following coverage', async () => { scenario = 'empty'; try { const data = await succeeds('get_following_feed', { account: 'reading' }); assert.deepEqual(data.tweets, []); assert.equal(data.hasNextPage, false); } finally { scenario = 'normal'; } });
  await run('Incomplete following list reported', async () => { scenario = 'following-page'; try { const data = await succeeds('get_following_feed', { account: 'reading' }); assert.equal(data.coverage.followingHasNextPage, true); } finally { scenario = 'normal'; } });
  await run('Following list continuation changes searched users and preserves partial coverage', async () => {
    const before = trace.length;
    const data = await succeeds('get_following_feed', { account: 'reading', followingCursor: 'following-next' });
    assert.deepEqual(data.tweets.map((item) => item.id), ['604']);
    assert.equal(data.coverage.accounts, 1);
    assert.equal(data.coverage.followingPage, 'continuation');
    assert.equal(data.coverage.followingHasNextPage, false);
    assert.equal(data.coverage.completeFollowingList, false);
    const requests = trace.slice(before);
    assert.equal(requests.length, 2);
    assert.equal(requests[0].path, '/twitter/user/followings');
    assert.equal(requests[0].query.cursor, 'following-next');
    assert.match(requests[1].query.query, /from:laterpublic/i);
    assert.ok(!/from:public\d/i.test(requests[1].query.query));
  });
  await run('Following and search cursors together reject before HTTP', async () => {
    assert.ok(cursor);
    const data = await fails('get_following_feed', { account: 'reading', followingCursor: 'following-next', cursor }, true);
    assert.equal(data.error.code, 'INVALID_CURSOR');
  });
  await run('Login ambiguous account rejected before HTTP', () => fails('login_account', {}, true));
  await run('Refresh ambiguous account rejected before HTTP', () => fails('refresh_session', {}, true));
  await run('Login unknown account rejected before HTTP', () => fails('login_account', { account: 'unknown' }, true));
  await run('Login missing API key rejected before HTTP', () => fails('login_account', { account: 'broken' }, true));
  await run('Login missing credentials rejected before HTTP', () => fails('login_account', { account: 'noCredentials' }, true));
  await run('Invalid TOTP seed rejected before HTTP', () => fails('login_account', { account: 'badSeed' }, true));
  await run('Configured cookie login reuses without HTTP', async () => {
    const before = trace.length;
    const data = await succeeds('login_account', { account: 'posting' });
    assertLoginMetadata(data, true);
    assert.equal(data.account, 'posting');
    assert.equal(trace.length, before);
  });
  await run('Login saves DPAPI and passes exact Base32 seed once', async () => {
    const before = trace.length;
    const data = await succeeds('login_account', { account: 'auth' });
    assertLoginMetadata(data, false);
    assert.equal(data.account, 'auth');
    assert.equal(data.userName, 'AuthUser');
    assert.equal(data.sessionSource, 'plumivox_dpapi');
    assert.equal(trace.length - before, 1);
    assert.equal(trace.at(-1).path, '/twitter/user_login_v2');
    assert.equal(trace.at(-1).loginPayloadValid, true);
    assert.equal(trace.at(-1).totpSeedExact, true);
    assert.deepEqual(trace.at(-1).loginPayloadKeys, ['email', 'password', 'proxy', 'totp_secret', 'user_name']);
    const encrypted = await readFile(ownSessionPath, 'utf8');
    assert.ok(encrypted.length > 100 && !encrypted.trimStart().startsWith('{'));
    assert.ok(sensitiveValues.every((secret) => !encrypted.includes(secret)), 'DPAPI file exposes plaintext secret');
    assert.equal(await readFile(secretsPath, 'utf8'), initialSecrets, 'Login rewrote API key configuration');
  });
  await run('Restart reloads encrypted session and reuses without HTTP', async () => {
    await restart();
    const before = trace.length;
    const data = await succeeds('login_account', { account: 'auth' });
    assertLoginMetadata(data, true);
    assert.equal(data.sessionSource, 'plumivox_dpapi');
    assert.ok(Number.isFinite(Date.parse(data.acquiredAt)));
    assert.equal(trace.length, before);
    const accounts = await succeeds('list_accounts');
    assert.equal(accounts.accounts.find((item) => item.account === 'auth').postConfigured, true);
  });
  await run('Single configured account is selected when login account is omitted', async () => {
    try {
      await restart(singleAccountSecretsPath);
      const before = trace.length;
      const data = await succeeds('login_account');
      assertLoginMetadata(data, true);
      assert.equal(data.account, 'auth');
      assert.equal(trace.length, before);
    } finally { await restart(); }
  });
  await run('Explicit refresh logs in once and accepts plural cookie', async () => {
    scenario = 'login-plural-cookie';
    const before = trace.length;
    try {
      const data = await succeeds('refresh_session', { account: 'auth' });
      assertLoginMetadata(data, false);
      assert.equal(trace.length - before, 1);
      assert.equal(trace.at(-1).path, '/twitter/user_login_v2');
      assert.equal(trace.at(-1).totpSeedExact, true);
      assert.equal(await readFile(secretsPath, 'utf8'), initialSecrets);
    } finally { scenario = 'normal'; }
  });
  for (const problem of ['http401', 'html', 'jsonerror', 'login-missing-cookie', 'timeout']) await run(`${problem} login failure retains encrypted session and does not retry`, async () => {
    const previous = await readFile(ownSessionPath);
    const before = trace.length;
    scenario = problem;
    try {
      await fails('refresh_session', { account: 'auth' });
      assert.equal(trace.length - before, 1);
      assert.deepEqual(await readFile(ownSessionPath), previous);
    } finally { scenario = 'normal'; }
    const reusedBefore = trace.length;
    const data = await succeeds('login_account', { account: 'auth' });
    assertLoginMetadata(data, true);
    assert.equal(trace.length, reusedBefore);
  });
  await run('Posting authentication failure retains saved cookie and does not retry', async () => {
    const previous = await readFile(ownSessionPath);
    scenario = 'http401';
    const before = trace.length;
    try { await fails('post_tweet', { account: 'auth', text: 'authentication failure probe' }); assert.equal(trace.length - before, 1); }
    finally { scenario = 'normal'; }
    assert.deepEqual(await readFile(ownSessionPath), previous);
    const reusedBefore = trace.length;
    assertLoginMetadata(await succeeds('login_account', { account: 'auth' }), true);
    assert.equal(trace.length, reusedBefore);
  });
  await run('Session refresh after rejected post never resends the post', async () => {
    const before = trace.length;
    assertLoginMetadata(await succeeds('refresh_session', { account: 'auth' }), false);
    const requests = trace.slice(before);
    assert.equal(requests.length, 1);
    assert.equal(requests[0].path, '/twitter/user_login_v2');
    assert.equal(requests.filter((item) => item.path.endsWith('/create_tweet_v2')).length, 0);
  });
  await run('Concurrent same-account refresh refuses second login before HTTP', async () => {
    scenario = 'login-delay';
    let acceptanceTimer;
    const accepted = new Promise((resolve, reject) => { loginAccepted = resolve; acceptanceTimer = setTimeout(() => reject(new Error('Mock did not receive login')), 8000); });
    const first = succeeds('refresh_session', { account: 'auth' });
    first.catch(() => {});
    try {
      await accepted;
      const data = await fails('refresh_session', { account: 'auth' }, true);
      assert.equal(data.error.code, 'AUTH_BUSY');
      assertLoginMetadata(await first, false);
    } finally { clearTimeout(acceptanceTimer); loginAccepted = undefined; await first.catch(() => {}); scenario = 'normal'; }
  });
  await run('Independent MCP processes serialize same-account refresh before login API', async () => {
    const ownerTransport = makeTransport(secretsPath, stateDirectory, 15000);
    const contenderTransport = makeTransport(secretsPath, stateDirectory, 15000);
    const owner = new Client({ name: 'plumivox-lock-owner', version: '1.0.0' });
    const contender = new Client({ name: 'plumivox-lock-contender', version: '1.0.0' });
    for (const stream of [ownerTransport.stderr, contenderTransport.stderr]) stream?.on('data', (data) => diagnostics.push(String(data)));
    let acceptanceTimer;
    let first;
    try {
      await Promise.all([owner.connect(ownerTransport), contender.connect(contenderTransport)]);
      scenario = 'interprocess-login';
      peakLogins = 0;
      releaseHeldLogin = undefined;
      const before = trace.length;
      const accepted = new Promise((resolve, reject) => { loginAccepted = resolve; acceptanceTimer = setTimeout(() => reject(new Error('Mock did not receive owner login')), 10000); });
      first = call('refresh_session', { account: 'auth' }, owner);
      first.catch(() => {});
      await accepted;
      const second = await call('refresh_session', { account: 'auth' }, contender);
      assert.equal(second.result.isError, true, 'Competing process refreshed instead of refusing the update');
      assert.equal(second.data.error.code, 'AUTH_BUSY');
      assert.equal(trace.slice(before).filter((item) => item.path.endsWith('/user_login_v2')).length, 1);
      assert.equal(peakLogins, 1);
      releaseHeldLogin();
      const completed = await first;
      assert.notEqual(completed.result.isError, true, JSON.stringify(completed.data));
      assertLoginMetadata(completed.data, false);
    } finally {
      clearTimeout(acceptanceTimer);
      loginAccepted = undefined;
      releaseHeldLogin?.();
      await first?.catch(() => {});
      await Promise.all([owner.close().catch(() => {}), contender.close().catch(() => {})]);
      releaseHeldLogin = undefined;
      scenario = 'normal';
    }
  });
  await run('Stopping mutex owner process permits another MCP process to refresh', async () => {
    const ownerTransport = makeTransport(secretsPath, stateDirectory, 15000);
    const contenderTransport = makeTransport(secretsPath, stateDirectory, 15000);
    const owner = new Client({ name: 'plumivox-stopped-lock-owner', version: '1.0.0' });
    const contender = new Client({ name: 'plumivox-recovered-lock-contender', version: '1.0.0' });
    for (const stream of [ownerTransport.stderr, contenderTransport.stderr]) stream?.on('data', (data) => diagnostics.push(String(data)));
    let acceptanceTimer;
    let pending;
    try {
      await Promise.all([owner.connect(ownerTransport), contender.connect(contenderTransport)]);
      scenario = 'interprocess-login';
      releaseHeldLogin = undefined;
      const before = trace.length;
      const accepted = new Promise((resolve, reject) => { loginAccepted = resolve; acceptanceTimer = setTimeout(() => reject(new Error('Mock did not receive owner login')), 10000); });
      pending = call('refresh_session', { account: 'auth' }, owner).then(() => 'completed', () => 'closed');
      await accepted;
      await owner.close();
      assert.equal(await pending, 'closed');
      assert.equal(ownerTransport.pid, null);
      scenario = 'normal';
      const recovered = await call('refresh_session', { account: 'auth' }, contender);
      assert.notEqual(recovered.result.isError, true, JSON.stringify(recovered.data));
      assertLoginMetadata(recovered.data, false);
      assert.equal(trace.slice(before).filter((item) => item.path.endsWith('/user_login_v2')).length, 2);
      assert.equal(trace.slice(before).filter((item) => item.path.endsWith('/create_tweet_v2')).length, 0);
    } finally {
      clearTimeout(acceptanceTimer);
      loginAccepted = undefined;
      releaseHeldLogin?.();
      await Promise.all([owner.close().catch(() => {}), contender.close().catch(() => {})]);
      await pending;
      releaseHeldLogin = undefined;
      scenario = 'normal';
    }
  });
  await run('Changed API binding rejects saved cookie before posting without destroying original', async () => {
    const previous = await readFile(ownSessionPath);
    try {
      await restart(mismatchedSecretsPath);
      const data = await succeeds('list_accounts');
      assert.equal(data.accounts.find((item) => item.account === 'auth').postConfigured, false);
      await fails('post_tweet', { account: 'auth', text: 'must not send' }, true);
      assert.deepEqual(await readFile(ownSessionPath), previous);
    } finally { await restart(); }
    const before = trace.length;
    assertLoginMetadata(await succeeds('login_account', { account: 'auth' }), true);
    assert.equal(trace.length, before);
  });
  await run('Invalid own session refuses legacy fallback while reading remains available', async () => {
    try {
      await restart(secretsPath, corruptedStateDirectory);
      const data = await succeeds('list_accounts');
      assert.equal(data.accounts.find((item) => item.account === 'posting').postConfigured, false);
      await fails('post_tweet', { account: 'posting', text: 'must not fallback to configured cookie' }, true);
      const posts = await succeeds('get_my_posts', { account: 'posting' });
      assert.equal(posts.tweets[0].id, '501');
    } finally { await restart(); }
  });
  await run('Authentication, HTTP payloads and clean MCP diagnostics', async () => { assert.ok(trace.length > 0); assert.ok(trace.every((item) => item.authenticated && !item.cookieHeaderPresent)); for (const request of trace) { assert.equal(request.method, /\/(create_tweet|upload_media|user_login)_v2$/.test(request.path) ? 'POST' : 'GET'); if (request.path.endsWith('/create_tweet_v2')) assert.equal(request.postingPayloadValid, true); if (request.path.endsWith('/user_login_v2')) { assert.equal(request.loginPayloadValid, true); assert.equal(request.totpSeedExact, true); } } assert.deepEqual(protocolErrors, []); assert.ok(sensitiveValues.every((secret) => !diagnostics.join('').includes(secret))); assert.equal(await readFile(secretsPath, 'utf8'), initialSecrets); });
} catch (error) { results.push({ name: 'MCP process startup', status: 'fail', message: safe(error.message) }); }
finally { await client.close().catch(() => {}); mock.closeAllConnections(); await new Promise((resolve) => mock.close(resolve)); }
if (!results.length) results.push({ name: 'Selected E2E case exists', status: 'fail', message: 'No case matched PLUMIVOX_E2E_CASE' });
const summary = { status: results.every((item) => item.status === 'pass') ? 'pass' : 'fail', passed: results.filter((item) => item.status === 'pass').length, failed: results.filter((item) => item.status === 'fail').length, results, retainedFiles: [secretsPath, mismatchedSecretsPath, singleAccountSecretsPath, pngPath, invalidPath, oversizedPath, stateDirectory, corruptedStateDirectory], retention: 'Synthetic input fixtures and encrypted sessions are retained for the caller to remove through the approved Windows recycle-bin workflow; summary and HTTP trace are reproducible evidence.' };
await writeFile(path.join(output, 'summary.json'), JSON.stringify(summary, null, 2));
await writeFile(path.join(output, 'http-trace.json'), JSON.stringify(trace, null, 2));
console.log(JSON.stringify({ status: summary.status, passed: summary.passed, failed: summary.failed, output }));
for (const failure of results.filter((item) => item.status === 'fail')) console.error(`${failure.name}: ${failure.message}`);
process.exitCode = summary.failed ? 1 : 0;
