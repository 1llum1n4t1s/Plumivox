import assert from 'node:assert/strict';
import http from 'node:http';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { mkdir, writeFile, realpath, stat, readFile } from 'node:fs/promises';
import { randomUUID, createHash, createHmac } from 'node:crypto';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
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
const longAccountName = 'profile-' + 'p'.repeat(65);
// 署名付き検索cursorのラッパー上限を、provider cursor上限と独立して検証する。
const oversizedCursorAccountName = 'cursor-profile-' + 'p'.repeat(150000);
const expandedFeedCursorAccountName = 'feed-cursor-profile-' + 'p'.repeat(72000);
const stateDirectory = path.join(output, 'auth-state');
const syntheticConfig = { twitterapi_io: {
  posting: { user_name: 'Poster', api_key: fakeKey, proxy: 'http://127.0.0.1:8888', login_cookies: fakeCookie },
  reading: { user_name: 'Reader', api_key: fakeKey },
  broken: { user_name: 'Broken', api_key: '', email: 'auth@example.invalid', password: fakePassword, proxy: 'http://127.0.0.1:8888' },
  auth: { user_name: 'AuthUser', api_key: fakeKey, proxy: 'http://127.0.0.1:8888', email: 'auth@example.invalid', password: fakePassword, totp_secret: fakeSeed },
  noCredentials: { user_name: 'NoCredentials', api_key: fakeKey, proxy: 'http://127.0.0.1:8888' },
  badSeed: { user_name: 'BadSeed', api_key: fakeKey, proxy: 'http://127.0.0.1:8888', email: 'auth@example.invalid', password: fakePassword, totp_secret: '123456' },
  [longAccountName]: { user_name: 'Reader', api_key: fakeKey },
  [oversizedCursorAccountName]: { user_name: 'Reader', api_key: fakeKey },
  [expandedFeedCursorAccountName]: { user_name: 'Reader', api_key: fakeKey },
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
const orderedImages = Array.from({ length: 4 }, (_, index) => Buffer.concat([png, Buffer.from(`image-order-${index}`)]));
const orderedImagePaths = orderedImages.map((_, index) => path.join(output, `ordered-${index}.png`));
await Promise.all(orderedImages.map((bytes, index) => writeFile(orderedImagePaths[index], bytes)));
await writeFile(pngPath, png);
await writeFile(invalidPath, 'not an image');
await writeFile(oversizedPath, Buffer.concat([png, Buffer.alloc(5 * 1024 * 1024)]));

const trace = [];
const results = [];
const authGateFiles = [];
let scenario = 'normal';
let nextPost = 900;
let uploadAccepted;
let scenarioUploads = 0;
let searchAccepted;
let loginAccepted;
let releaseHeldLogin;
let activeLogins = 0;
let peakLogins = 0;
let oversizedResponse;
let failedUploadBody;
const tweet = (id, createdAt = '2026-10-12T12:00:00Z') => ({ id, text: `Tweet ${id}`, createdAt, author: { userName: 'PublicUser', name: 'Public User' }, url: `https://x.com/PublicUser/status/${id}` });
const providerCursorLengths = { 'cursor-provider-over': 10001, 'cursor-provider-below': 9500, 'cursor-aggregate-over': 9500, 'cursor-aggregate-below': 8000, 'cursor-wrapper-over': 9500, 'cursor-wrapper-following': 1000, 'cursor-timeline-over': 100001 };
const mock = http.createServer(async (req, res) => {
  const url = new URL(req.url, 'http://127.0.0.1');
  const chunks = [];
  for await (const chunk of req) chunks.push(chunk);
  const body = Buffer.concat(chunks);
  let payload;
  if (req.headers['content-type']?.startsWith('application/json')) { try { payload = JSON.parse(body); } catch {} }
  let uploadedImageHash;
  if (url.pathname.endsWith('/upload_media_v2') && ['image-order', 'image-later-failure', 'upload-retry-order'].includes(scenario)) {
    const form = await new Response(body, { headers: { 'content-type': req.headers['content-type'] } }).formData();
    uploadedImageHash = createHash('sha256').update(Buffer.from(await form.get('file').arrayBuffer())).digest('hex');
  }
  trace.push({ method: req.method, path: url.pathname, query: Object.fromEntries(url.searchParams), authenticated: req.headers['x-api-key'] === fakeKey, cookieHeaderPresent: Boolean(req.headers.cookie), contentType: req.headers['content-type'] || null, bodyBytes: body.length,
    ...(url.pathname.endsWith('/create_tweet_v2') ? { postingPayloadValid: payload?.login_cookies === fakeCookie && payload?.proxy === 'http://127.0.0.1:8888' && typeof payload?.tweet_text === 'string', mediaIds: payload?.media_ids ?? [] } : {}),
    ...(url.pathname.endsWith('/upload_media_v2') ? { multipartPayloadValid: body.includes(Buffer.from(fakeCookie)) && body.includes(Buffer.from('name="proxy"')) && body.includes(Buffer.from('name="file"')) && body.includes(png), ...(uploadedImageHash ? { uploadedImageHash } : {}) } : {}),
    ...(url.pathname.endsWith('/user_login_v2') ? { loginPayloadValid: payload?.user_name === 'AuthUser' && payload?.email === 'auth@example.invalid' && payload?.password === fakePassword && payload?.proxy === 'http://127.0.0.1:8888', totpSeedExact: payload?.totp_secret === fakeSeed, loginPayloadKeys: Object.keys(payload || {}).sort() } : {}),
  });
  const send = (data, code = 200) => { res.writeHead(code, { 'content-type': 'application/json' }); res.end(JSON.stringify(data)); };
  if (scenario === 'create502' && url.pathname.endsWith('/create_tweet_v2')) return send({ message: fakeCookie }, 502);
  if (scenario.startsWith('upload-') && url.pathname.endsWith('/upload_media_v2')) {
    const attempt = ++scenarioUploads;
    if (scenario === 'upload-stream502') {
      if (attempt > 1) return send({ status: 'success', media_id: '700' });
      const stats = { cancelled: false };
      failedUploadBody = stats;
      res.writeHead(502, { 'content-type': 'text/plain' });
      res.write(fakeCookie);
      const timer = setInterval(() => res.write(fakeCookie), 20);
      stats.closed = new Promise((resolve) => res.once('close', () => { clearInterval(timer); stats.cancelled = !res.writableEnded; resolve(); }));
      return;
    }
    if (scenario === 'upload-retry-time-budget') {
      setTimeout(() => { if (!res.destroyed) send(attempt === 1 ? { message: fakeCookie } : { status: 'success', media_id: '700' }, attempt === 1 ? 502 : 200); }, attempt === 1 ? 100 : 900).unref();
      return;
    }
    if (scenario === 'upload-retry-order') return send(attempt === 2 ? { message: fakeCookie } : { status: 'success', media_id: String(700 + attempt) }, attempt === 2 ? 502 : 200);
    if (scenario === 'upload-jsonerror') return send({ status: 'error', message: fakeCookie });
    if (scenario === 'upload-html') { res.writeHead(200); return res.end(fakeCookie); }
    if (scenario === 'upload-missing-id') return send({ status: 'success' });
    if (scenario === 'upload-timeout') { setTimeout(() => { if (!res.destroyed) send({ status: 'success', media_id: '700' }); }, 1800).unref(); return; }
    if (scenario === 'upload-network') return req.socket.destroy();
    if (scenario === 'upload-cancel-backoff') { res.once('finish', () => uploadAccepted?.()); return send({ message: fakeCookie }, 502); }
    if (scenario === 'upload-cancel-second') {
      if (attempt === 1) return send({ message: fakeCookie }, 502);
      uploadAccepted?.();
      setTimeout(() => { if (!res.destroyed) send({ status: 'success', media_id: '700' }); }, 150).unref();
      return;
    }
    const match = /^upload-(recover|always)-(401|429|500|502|503|504)$/.exec(scenario);
    if (match) return send(match[1] === 'recover' && attempt > 1 ? { status: 'success', media_id: '700' } : { message: fakeCookie }, match[1] === 'recover' && attempt > 1 ? 200 : Number(match[2]));
  }
  if (scenario === 'oversized-response') {
    const stats = { sentBytes: 0, totalBytes: 32 * 1024 * 1024, cancelled: false };
    oversizedResponse = stats;
    stats.closed = new Promise((resolve) => res.once('close', () => { clearInterval(timer); stats.cancelled = !res.writableEnded; resolve(); }));
    res.writeHead(200, { 'content-type': 'application/json' });
    const chunk = Buffer.alloc(1024 * 1024, 0x20);
    const timer = setInterval(() => {
      stats.sentBytes += chunk.length;
      res.write(chunk);
      if (stats.sentBytes >= stats.totalBytes) { clearInterval(timer); res.end(); }
    }, 4);
    return;
  }
  if (scenario === 'split-utf8') {
    const bytes = Buffer.from(JSON.stringify({ tweets: [{ ...tweet('506'), text: '🫱🏻‍🫲🏽 日本語' }] }));
    const split = bytes.indexOf(Buffer.from('🫱')) + 2;
    res.writeHead(200, { 'content-type': 'application/json' });
    res.write(bytes.subarray(0, split));
    setTimeout(() => { if (!res.destroyed) res.end(bytes.subarray(split)); }, 10).unref();
    return;
  }
  if (/^http(401|429|500|502|503|504)$/.test(scenario)) return send({ message: fakeKey }, Number(scenario.slice(4)));
  if (scenario === 'html') { res.writeHead(200, { 'content-type': 'text/html' }); return res.end(`<html>${fakeCookie}</html>`); }
  if (scenario === 'jsonerror') return send({ status: 'error', msg: fakeCookie });
  if (scenario === 'timeout') { setTimeout(() => { if (!res.destroyed) send({ tweet_id: '999' }); }, 700).unref(); return; }
  if (url.pathname.endsWith('/user_login_v2')) {
    if (scenario === 'login-cookie-newline') return send({ status: 'success', login_cookie: `${fakeCookie}\n` });
    if (scenario === 'login-cookie-nul') return send({ status: 'success', login_cookie: `${fakeCookie}\u0000` });
    if (scenario === 'login-cookie-utf8-over') return send({ status: 'success', login_cookie: 'あ'.repeat(400000) });
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
    if (scenario === 'cursor-timeline-over') return send({ tweets: [tweet('1200')], has_next_page: true, next_cursor: 'c'.repeat(providerCursorLengths[scenario]) });
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
    if (scenario === 'following-selection') return send({ followings: [
      { userName: 'FirstUnknown' }, { userName: 'FIRSTUNKNOWN' },
      { userName: '', isProtected: false }, { userName: 'invalid-name', protected: false },
      { userName: 'Private', isProtected: true, protected: false }, { userName: 'OtherPrivate', isProtected: false, protected: true },
      { userName: 'KnownFirst', protected: false }, { userName: 'KNOWNFIRST' },
      ...Array.from({ length: 18 }, (_, index) => ({ userName: `Chosen${index}` })),
      { userName: 'PastLimit', isProtected: false }, { userName: 'FIRSTUNKNOWN', protected: false },
      { userName: 'Chosen0', isProtected: true, protected: false }, { userName: 'Chosen1', isProtected: false },
    ], has_next_page: true, next_cursor: 'selection-next' });
    if (scenario === 'cursor-wrapper-following') return send({ followings: Array.from({ length: 20 }, (_, index) => ({ userName: `Public${index}`, isProtected: false })), has_next_page: false, next_cursor: '' });
    if (scenario === 'unknown-following') return send({ followings: [{ userName: 'KnownPublic', isProtected: false }, { userName: 'UnknownPublic' }, { userName: 'OtherUnknown' }, { userName: 'Private', isProtected: true }], has_next_page: false, next_cursor: '' });
    if (scenario.startsWith('cursor-aggregate')) return send({ followings: Array.from({ length: 160 }, (_, index) => ({ userName: `Public${index}`, isProtected: false })), has_next_page: false, next_cursor: '' });
    if (scenario === 'cursor-following-list-over') return send({ followings: [{ userName: 'Public0', isProtected: false }], has_next_page: true, next_cursor: 'f'.repeat(10001) });
    if (scenario === 'cursor-following-wrapper-over') return send({ followings: [{ userName: 'Public0', isProtected: false }], has_next_page: true, next_cursor: 'following-next' });
    if (url.searchParams.get('cursor') === 'following-next') return send({ followings: [{ userName: 'LaterPublic', isProtected: false }], has_next_page: false, next_cursor: '' });
    return send({ followings: ['empty', 'search-empty'].includes(scenario) ? [] : [...Array.from({ length: 25 }, (_, index) => ({ userName: `Public${index}`, isProtected: false })), { userName: 'Private', isProtected: true }], has_next_page: ['following-page', 'search-following-page'].includes(scenario), next_cursor: ['following-page', 'search-following-page'].includes(scenario) ? 'following-next' : '' });
  }
  if (url.pathname.endsWith('/tweet/advanced_search')) {
    const query = url.searchParams.get('query') || '';
    if (scenario === 'feed-date-order') return send({ tweets: [
      tweet('10', '1969-12-31T23:59:59Z'), tweet('20', null), tweet('30', 'invalid-date'),
      tweet('40', '1970-01-01T00:00:00Z'), tweet('50', '2026-10-12T12:00:00Z'),
      tweet('60', '2026-10-12T12:00:00Z'), tweet('070', '2026-10-12T12:00:00Z'),
      tweet('70', '2026-10-12T12:00:00Z'), tweet('80', '1969-12-31T23:59:58Z'),
    ], has_next_page: false, next_cursor: '' });
    if (providerCursorLengths[scenario]) return send({ tweets: [tweet('1200')], has_next_page: !url.searchParams.get('cursor'), next_cursor: url.searchParams.get('cursor') ? '' : 'c'.repeat(providerCursorLengths[scenario]) });
    if (scenario.startsWith('search-')) {
      if (scenario === 'search-cancel') {
        searchAccepted?.();
        setTimeout(() => { if (!res.destroyed) send({ tweets: [], has_next_page: false, next_cursor: '' }); }, 150).unref();
        return;
      }
      if (scenario === 'search-malformed') return send({ tweets: {}, has_next_page: false, next_cursor: '' });
      const phrase = query.match(/"([^"\\]*)"/)?.[1];
      const operators = query.replace(/"[^"\\]*"/g, '');
      const authors = [...operators.matchAll(/from:([A-Za-z0-9_]+)/g)].map((match) => match[1].toLowerCase());
      const since = Number(operators.match(/since_time:(\d+)/)?.[1] ?? '-Infinity');
      const until = Number(operators.match(/until_time:(\d+)/)?.[1] ?? 'Infinity');
      const searchTweet = (id, author, createdAt, text = 'needle phrase') => ({ ...tweet(id, createdAt), text, author: { userName: author } });
      const candidates = [
        searchTweet('803', 'Public0', '2026-10-12T11:00:00Z'),
        searchTweet('801', 'Public0', '2026-10-12T13:00:00Z'),
        searchTweet('802', 'Public20', '2026-10-12T14:00:00Z'),
        searchTweet('804', 'Outside', '2026-10-12T12:00:00Z'),
        searchTweet('805', 'Public0', '2026-10-12T10:59:59Z'),
        searchTweet('806', 'Public20', '2026-10-12T15:00:00Z'),
        searchTweet('807', 'Public0', '2026-10-12T12:00:00Z', 'different keyword'),
        searchTweet('808', 'LaterPublic', '2026-10-12T12:30:00Z'),
        searchTweet('809', 'Public0', '2026-10-12T12:00:00Z', 'needle OR from:Outside'),
        searchTweet('810', 'Outside', '2026-10-12T12:00:00Z', 'needle OR from:Outside'),
      ];
      // 偽APIでも語句・日時・投稿者を評価し、条件を落とした実装を成功させない。
      const selected = candidates.filter((item) => phrase && item.text.includes(phrase) && (!authors.length || authors.includes(item.author.userName.toLowerCase())) && Date.parse(item.createdAt) / 1000 >= since && Date.parse(item.createdAt) / 1000 < until);
      if (authors.includes('public20')) selected.push(...candidates.filter((item) => item.id === '801' && item.text.includes(phrase) && Date.parse(item.createdAt) / 1000 >= since && Date.parse(item.createdAt) / 1000 < until));
      return send({ tweets: selected, has_next_page: !url.searchParams.get('cursor'), next_cursor: url.searchParams.get('cursor') ? '' : 'search-filter-next' });
    }
    if (query.toLowerCase().includes('from:laterpublic')) return send({ tweets: [tweet('604')], has_next_page: false, next_cursor: '' });
    const second = query.toLowerCase().includes('from:public20');
    return send({ tweets: second ? [tweet('601', '2026-10-12T13:00:00Z'), tweet('602', '2026-10-12T14:00:00Z')] : [tweet('603', '2026-10-12T11:00:00Z'), tweet('601', '2026-10-12T13:00:00Z')], has_next_page: !url.searchParams.get('cursor'), next_cursor: !url.searchParams.get('cursor') ? 'search-next' : '' });
  }
  if (url.pathname.endsWith('/upload_media_v2')) {
    if (['image-order', 'image-later-failure'].includes(scenario)) {
      const index = scenarioUploads++;
      return send(scenario === 'image-later-failure' && index === 2 ? { status: 'success' } : { status: 'success', media_id: String(700 + index) });
    }
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
const makeTransport = (configPath = secretsPath, sessionRoot = stateDirectory, timeoutMs = 250, preloadPath, fixtureProfile) => new StdioClientTransport({ command: process.execPath, args: [...(preloadPath ? ['--import', pathToFileURL(preloadPath).href] : []), path.join(repo, 'tests/fixture-server.mjs'), apiBaseUrl, configPath, sessionRoot, String(timeoutMs), ...(fixtureProfile ? [fixtureProfile] : [])], cwd: repo, env: Object.fromEntries(Object.entries(process.env).filter(([key, value]) => value !== undefined && !['PLUMIVOX_SECRETS_FILE', 'PLUMIVOX_DEFAULT_ACCOUNT', 'PLUMIVOX_STATE_DIR'].includes(key))), stderr: 'pipe' });
// G3: 製品へtest flagを加えず、E2E子プロセスの標準入力・保存ackだけを停止する。
const authGate = async (mode) => {
  const prefix = path.join(output, `auth-gate-${randomUUID()}`);
  const preloadPath = `${prefix}.mjs`;
  const phasePath = `${prefix}-phase.json`;
  const resumePath = `${prefix}-resume`;
  const resultPath = `${prefix}-result.json`;
  authGateFiles.push(preloadPath, phasePath, resumePath, resultPath);
  const source = `import childProcess from 'node:child_process';
import { syncBuiltinESMExports } from 'node:module';
import { writeFileSync, existsSync } from 'node:fs';
import { McpServer } from ${JSON.stringify(pathToFileURL(path.join(repo, 'node_modules/@modelcontextprotocol/sdk/dist/esm/server/mcp.js')).href)};
const mode = ${JSON.stringify(mode)}, phase = ${JSON.stringify(phasePath)}, resume = ${JSON.stringify(resumePath)};
// SDKは取り消した要求の応答を抑制するため、実ハンドラが返した安全なcodeだけを観測する。
const registerTool = McpServer.prototype.registerTool;
McpServer.prototype.registerTool = function(name, config, callback) {
  return registerTool.call(this, name, config, async (...args) => {
    const result = await callback(...args);
    let code = result.structuredContent?.error?.code;
    if (!code && result.isError) { try { code = JSON.parse(result.content?.find((item) => item.type === 'text')?.text).error?.code; } catch {} }
    if (name === 'refresh_session' && typeof code === 'string') {
      writeFileSync(${JSON.stringify(resultPath)}, JSON.stringify({ code }));
    }
    return result;
  });
};
const spawn = childProcess.spawn;
childProcess.spawn = function(command, args, options) {
  const child = spawn.apply(this, arguments);
  const index = args?.indexOf('-EncodedCommand');
  if (command !== 'pwsh' || index < 0) return child;
  const script = Buffer.from(args[index + 1], 'base64').toString('utf16le');
  if (mode === 'encrypt' && script.includes("$request.operation -eq 'encrypt'")) {
    const end = child.stdin.end.bind(child.stdin);
    child.stdin.end = function(data, ...rest) {
      let request; try { request = JSON.parse(String(data)); } catch {}
      if (request?.operation !== 'encrypt') return end(data, ...rest);
      writeFileSync(phase, JSON.stringify({ phase: 'encrypt', pid: child.pid }));
      const tick = setInterval(() => { if (existsSync(resume)) { clearInterval(tick); end(data, ...rest); } }, 10);
      child.once('close', () => clearInterval(tick));
      return this;
    };
  }
  if (mode === 'saved' && script.includes('[Threading.Mutex]')) {
    const emit = child.stdout.emit;
    child.stdout.emit = function(event, data, ...rest) {
      if (event === 'data' && String(data).trim() === 'saved') {
        writeFileSync(phase, JSON.stringify({ phase: 'saved', pid: child.pid }));
        return true;
      }
      return emit.call(this, event, data, ...rest);
    };
  }
  return child;
};
syncBuiltinESMExports();
`;
  await writeFile(preloadPath, source);
  const gateTransport = makeTransport(secretsPath, stateDirectory, 15000, preloadPath);
  const gateClient = new Client({ name: 'plumivox-auth-boundary-e2e', version: '1.0.0' });
  gateClient.onerror = (error) => protocolErrors.push(safe(error.message));
  gateTransport.stderr?.on('data', (data) => diagnostics.push(String(data)));
  await gateClient.connect(gateTransport);
  return { connection: gateClient, transport: gateTransport,
    async wait() { const deadline = Date.now() + 15000; while (Date.now() < deadline) { try { return JSON.parse(await readFile(phasePath, 'utf8')); } catch (error) { if (error.code !== 'ENOENT' && !(error instanceof SyntaxError)) throw error; } await new Promise((resolve) => setTimeout(resolve, 20)); } throw new Error(`Authentication did not reach ${mode} boundary`); },
    async waitResult() { const deadline = Date.now() + 15000; while (Date.now() < deadline) { try { return JSON.parse(await readFile(resultPath, 'utf8')); } catch (error) { if (error.code !== 'ENOENT' && !(error instanceof SyntaxError)) throw error; } await new Promise((resolve) => setTimeout(resolve, 20)); } throw new Error('Authentication handler did not produce a safe error code'); },
    async resume() { await writeFile(resumePath, 'resume'); },
  };
};
const verifiedKeeper = async (ownerPid) => {
  assert.ok(Number.isInteger(ownerPid) && ownerPid > 0);
  const query = `$ids = @(Get-CimInstance Win32_Process -Filter 'ParentProcessId = ${ownerPid}' | Where-Object { $_.Name -eq 'pwsh.exe' } | ForEach-Object { if ($_.CommandLine -match '-EncodedCommand\\s+(\\S+)') { $scriptText = [Text.Encoding]::Unicode.GetString([Convert]::FromBase64String($Matches[1])); if ($scriptText.Contains('[Threading.Mutex]::new') -and $scriptText.Contains('Plumivox')) { [int]$_.ProcessId } } }); ConvertTo-Json -InputObject $ids -Compress`;
  const { stdout } = await promisify(execFile)('pwsh', ['-NoLogo', '-NoProfile', '-NonInteractive', '-Command', query], { windowsHide: true, timeout: 10000 });
  const children = JSON.parse(stdout.trim());
  assert.equal(children.length, 1, 'Exactly one verified mutex keeper must exist');
  assert.ok(children[0] !== ownerPid && children[0] !== process.pid);
  return children[0];
};
let transport = makeTransport();
let client = new Client({ name: 'plumivox-e2e', version: '1.0.0' });
const protocolErrors = [];
const diagnostics = [];
client.onerror = (error) => protocolErrors.push(safe(error.message));
transport.stderr?.on('data', (data) => diagnostics.push(String(data)));
const sensitiveValues = [fakeKey, fakeCookie, fakePassword, fakeSeed, 'http://127.0.0.1:8888'];
const safe = (value) => sensitiveValues.reduce((text, secret) => text.replaceAll(secret, '[REDACTED]'), String(value)).replaceAll(secretsPath, '[SYNTHETIC_CONFIG]');
const attachDiagnostics = () => { client.onerror = (error) => protocolErrors.push(safe(error.message)); transport.stderr?.on('data', (data) => diagnostics.push(String(data))); };
const restart = async (configPath = secretsPath, sessionRoot = stateDirectory, timeoutMs = 250) => { await client.close(); transport = makeTransport(configPath, sessionRoot, timeoutMs); client = new Client({ name: 'plumivox-e2e', version: '1.0.0' }); attachDiagnostics(); await client.connect(transport); };
const run = async (name, check) => { if (process.env.PLUMIVOX_E2E_CASE && name !== process.env.PLUMIVOX_E2E_CASE) return; try { await check(); results.push({ name, status: 'pass' }); } catch (error) { results.push({ name, status: 'fail', message: safe(error.message) }); } };
const call = async (name, args = {}, connection = client, options = {}) => {
  const result = await connection.callTool({ name, arguments: args }, undefined, { timeout: 30000, ...options });
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
  await run('MCP discovery', async () => { const tools = await client.listTools(); assert.deepEqual(tools.tools.map((tool) => tool.name).sort(), ['get_following_feed', 'get_my_posts', 'get_proxy_status', 'get_tweets', 'list_accounts', 'login_account', 'post_tweet', 'refresh_session', 'search_tweets', 'start_proxy', 'stop_proxy']); });
  await run('Account metadata without secrets', async () => { const data = await succeeds('list_accounts'); assert.equal(data.accounts.length, Object.keys(syntheticConfig.twitterapi_io).length); assert.equal(data.accounts.find((item) => item.account === 'reading').readConfigured, true); assert.equal(data.accounts.find((item) => item.account === 'reading').postConfigured, false); });
  await run('Omitted account without a configured default fails before HTTP', async () => {
    assert.equal((await fails('search_tweets', { keyword: 'default account' }, true)).error.code, 'ACCOUNT_REQUIRED');
  });
  await run('Persisted default routes omitted reads, searches, posts and reused login', async () => {
    try {
      await writeFile(secretsPath, JSON.stringify({ twitterapi_io: { ...syntheticConfig.twitterapi_io, default_account: 'posting' } }));
      await restart();
      const listed = await succeeds('list_accounts');
      assert.equal(listed.accounts.length, Object.keys(syntheticConfig.twitterapi_io).length);
      assert.deepEqual(listed.accounts.filter((item) => item.isDefault).map((item) => item.account), ['posting']);
      assert.equal((await succeeds('get_my_posts')).account, 'posting');
      assert.equal((await succeeds('search_tweets', { keyword: 'default account' })).account, 'posting');
      assert.equal((await succeeds('post_tweet', { text: '既定アカウントの検証' })).account, 'posting');
      const login = await succeeds('login_account');
      assert.equal(login.account, 'posting'); assert.equal(login.reused, true);
      assert.equal((await succeeds('get_my_posts', { account: 'reading' })).account, 'reading');
      assert.equal((await fails('post_tweet', { account: 'reading', text: '明示先を既定へ置換しない' }, true)).error.code, 'POSTING_UNAVAILABLE');
      await writeFile(secretsPath, JSON.stringify({ twitterapi_io: { ...syntheticConfig.twitterapi_io, default_account: 'reading' } }));
      assert.equal((await succeeds('search_tweets', { keyword: 'updated default' })).account, 'reading');
    } finally { await writeFile(secretsPath, initialSecrets); await restart(); }
  });
  await run('Environment, embedded profile and explicit selection preserve default precedence', async () => {
    for (const [environmentAccount, expected] of [['reading', 'reading'], [undefined, 'posting']]) {
    const selectedTransport = new StdioClientTransport({ command: process.execPath,
      args: [path.join(repo, 'tests/fixture-server.mjs'), apiBaseUrl, secretsPath, stateDirectory, '250', 'posting'], cwd: repo,
      env: { ...Object.fromEntries(Object.entries(process.env).filter(([key, value]) => value !== undefined
        && !['PLUMIVOX_SECRETS_FILE', 'PLUMIVOX_DEFAULT_ACCOUNT', 'PLUMIVOX_STATE_DIR'].includes(key))), ...(environmentAccount ? { PLUMIVOX_DEFAULT_ACCOUNT: environmentAccount } : {}) }, stderr: 'pipe' });
    const selectedClient = new Client({ name: 'default-override-e2e', version: '1.0.0' });
    try {
      await writeFile(secretsPath, JSON.stringify({ twitterapi_io: { ...syntheticConfig.twitterapi_io, default_account: 'auth' } }));
      await selectedClient.connect(selectedTransport);
      const { data } = await call('get_my_posts', {}, selectedClient); assert.equal(data.account, expected);
      const explicit = await call('get_my_posts', { account: 'auth' }, selectedClient); assert.equal(explicit.data.account, 'auth');
      const listed = await call('list_accounts', {}, selectedClient);
      assert.deepEqual(listed.data.accounts.filter((item) => item.isDefault).map((item) => item.account), [expected]);
    } finally { await selectedClient.close(); await writeFile(secretsPath, initialSecrets); }
    }
  });
  await run('Invalid configured defaults never fall back and explicit accounts still work', async () => {
    try {
      for (const value of ['', 'missing', null, 42, { user_name: 'NotAnAccount', api_key: fakeKey }]) {
        await writeFile(secretsPath, JSON.stringify({ twitterapi_io: { posting: syntheticConfig.twitterapi_io.posting, default_account: value } }));
        const listed = await succeeds('list_accounts');
        assert.equal(listed.accounts.length, 1); assert.equal(listed.accounts[0].isDefault, false);
        assert.equal((await fails('search_tweets', { keyword: 'invalid default' }, true)).error.code, 'ACCOUNT_DEFAULT_INVALID');
        assert.equal((await succeeds('get_my_posts', { account: 'posting' })).account, 'posting');
      }
      await writeFile(secretsPath, JSON.stringify({ twitterapi_io: { posting: syntheticConfig.twitterapi_io.posting } }));
      assert.equal((await succeeds('get_my_posts')).account, 'posting');
    } finally { await writeFile(secretsPath, initialSecrets); }
  });
  await run('Listed long account name can be selected through MCP', async () => {
    const accounts = await succeeds('list_accounts');
    assert.equal(accounts.accounts.find((item) => item.account === longAccountName).readConfigured, true);
    const data = await succeeds('get_tweets', { account: longAccountName, tweetIds: ['502'] });
    assert.equal(data.account, longAccountName);
    assert.equal(data.tweets[0].id, '502');
  });
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
  await run('UTF-8 response split within an emoji decodes correctly', async () => {
    scenario = 'split-utf8';
    try { const data = await succeeds('get_tweets', { account: 'reading', tweetIds: ['506'] }); assert.equal(data.tweets[0].text, '🫱🏻‍🫲🏽 日本語'); }
    finally { scenario = 'normal'; }
  });
  for (const operation of ['read', 'post']) await run(`Oversized ${operation} response stops receiving without retry`, async () => {
    const before = trace.length;
    scenario = 'oversized-response';
    try {
      const data = await fails(operation === 'read' ? 'get_tweets' : 'post_tweet', operation === 'read' ? { account: 'reading', tweetIds: ['502'] } : { account: 'posting', text: 'bounded response probe' });
      assert.equal(data.error.code, operation === 'read' ? 'INVALID_RESPONSE' : 'POST_UNCERTAIN');
      assert.equal(trace.length - before, 1);
      await oversizedResponse.closed;
      assert.equal(oversizedResponse.cancelled, true);
      assert.ok(oversizedResponse.sentBytes < oversizedResponse.totalBytes, 'Server transmitted the entire oversized response');
      results.push({ name: `Oversized ${operation} response byte evidence`, status: 'pass', sentBytes: oversizedResponse.sentBytes, totalBytes: oversizedResponse.totalBytes });
    } finally { scenario = 'normal'; }
  });
  await run('Non-numeric tweet ID rejected', () => fails('get_tweets', { account: 'reading', tweetIds: ['bad'] }, true));
  await run('Missing Cookie rejected before HTTP', async () => {
    const { error } = await fails('post_tweet', { account: 'reading', text: 'hello' }, true);
    assert.equal(error.code, 'POSTING_UNAVAILABLE');
    assert.equal(error.stage, 'pre_send');
    assert.equal(error.postAttempted, false);
    assert.equal(error.uploadAttempts, 0);
  });
  // 失敗条件を先に固定: 準備後の設定再確認失敗は既知の未送信、未知の失敗は未送信と断定しない。
  await run('Post preparation errors retain safe metadata', async () => {
    for (const mode of ['--proxy-config-change', '--unknown-proxy-error']) {
      const connection = new Client({ name: 'plumivox-preparation-e2e', version: '1.0.0' });
      const dedicatedTransport = makeTransport(secretsPath, stateDirectory, 250, undefined, mode);
      dedicatedTransport.stderr?.on('data', (data) => diagnostics.push(String(data)));
      try {
        await connection.connect(dedicatedTransport);
        const before = trace.length;
        const { result, data } = await call('post_tweet', { account: 'posting', text: 'configuration recheck' }, connection);
        assert.equal(result.isError, true);
        assert.equal(trace.length, before, 'Preparation failure caused HTTP traffic');
        if (mode === '--proxy-config-change') {
          assert.equal(data.error.code, 'AUTH_CONFIG_CHANGED');
          assert.equal(data.error.message, '中継の準備中に設定が変更されました。再実行してください。');
          assert.equal(data.error.stage, 'pre_send');
          assert.equal(data.error.postAttempted, false);
          assert.equal(data.error.uploadAttempts, 0);
        } else {
          assert.equal(data.error.code, 'INTERNAL_ERROR');
          for (const field of ['stage', 'postAttempted', 'uploadAttempts']) assert.equal(Object.hasOwn(data.error, field), false);
        }
        const unrelated = await call('get_tweets', { account: 'missing', tweetIds: ['502'] }, connection);
        assert.equal(unrelated.data.error.code, 'ACCOUNT_UNKNOWN');
        for (const field of ['stage', 'postAttempted', 'uploadAttempts']) assert.equal(Object.hasOwn(unrelated.data.error, field), false);
      } finally { await connection.close(); }
    }
  });
  for (const [name, args] of [ ['Empty text', { text: '' }], ['Japanese 141 characters', { text: 'あ'.repeat(141) }], ['Relative image', { text: 'image', images: ['relative.png'] }], ['Invalid image format', { text: 'image', images: [invalidPath] }], ['Invalid second image', { text: 'image', images: [pngPath, invalidPath] }], ['Oversized image', { text: 'image', images: [oversizedPath] }], ['More than four images', { text: 'image', images: Array(5).fill(pngPath) }] ]) await run(`${name} rejected before HTTP`, () => fails('post_tweet', { account: 'posting', ...args }, true));
  for (const [name, emoji] of [['Mixed skin handshake', '🫱🏻‍🫲🏽'], ['Skin tone heart hands', '🫶🏻'], ['Head shaking', '🙂‍↔️'], ['Phoenix', '🐦‍🔥'], ['Direction walking', '🚶‍➡️'], ['Sark flag', '🇨🇶']]) {
    await run(`${name} emoji reaches 280 weighted characters`, async () => {
      const before = trace.length;
      const data = await succeeds('post_tweet', { account: 'posting', text: emoji.repeat(140) });
      assert.equal(data.weightedLength, 280);
      assert.equal(trace.length - before, 1);
      const invalid = await fails('post_tweet', { account: 'posting', text: emoji.repeat(141) }, true);
      assert.equal(invalid.error.code, 'INVALID_TEXT');
    });
  }
  await run('Modern emoji preserves URL and Japanese weights', async () => {
    const data = await succeeds('post_tweet', { account: 'posting', text: 'あ🫱🏻‍🫲🏽 https://example.com/' + 'a'.repeat(400) });
    assert.equal(data.weightedLength, 28);
  });
  await run('Modern emoji does not hide invalid control characters', async () => {
    const data = await fails('post_tweet', { account: 'posting', text: '🫱🏻‍🫲🏽\uFFFE' }, true);
    assert.equal(data.error.code, 'INVALID_TEXT');
  });
  await run('Emoji weighting does not turn an invalid hostname into a URL', async () => {
    const data = await fails('post_tweet', { account: 'posting', text: 'https://#️⃣.com/' + 'a'.repeat(300) }, true);
    assert.equal(data.error.code, 'INVALID_TEXT');
  });
  for (const [name, text, length] of [['Japanese 140 characters', 'あ'.repeat(140), 280], ['Compound family emoji', '👨‍👩‍👧‍👦', 2], ['Long URL', `https://example.com/${'a'.repeat(300)}`, 23]]) await run(name, async () => { const data = await succeeds('post_tweet', { account: 'posting', text }); assert.equal(data.weightedLength, length); assert.ok(/^\d+$/.test(data.tweetId)); assert.ok(data.url.includes(data.tweetId)); });
  await run('Explicit duplicate posts send twice', async () => { const before = trace.filter((item) => item.path.endsWith('/create_tweet_v2')).length; await succeeds('post_tweet', { account: 'posting', text: 'explicit duplicate' }); await succeeds('post_tweet', { account: 'posting', text: 'explicit duplicate' }); assert.equal(trace.filter((item) => item.path.endsWith('/create_tweet_v2')).length - before, 2); });
  await run('Image uses multipart upload then one post', async () => { const before = trace.length; await succeeds('post_tweet', { account: 'posting', text: 'image', images: [pngPath] }); const requests = trace.slice(before); assert.equal(requests.filter((item) => item.path.endsWith('/upload_media_v2')).length, 1); const upload = requests.find((item) => item.path.endsWith('/upload_media_v2')); assert.match(upload.contentType, /^multipart\/form-data; boundary=/); assert.equal(upload.multipartPayloadValid, true); assert.equal(requests.filter((item) => item.path.endsWith('/create_tweet_v2')).length, 1); assert.deepEqual(requests.find((item) => item.path.endsWith('/create_tweet_v2')).mediaIds, ['700']); });
  // 失敗条件を先に固定: upload/createの段階、限定HTTPだけ同画像を一度再試行、全画像後に投稿一回。
  // 拒否・不正応答・通信不明・予算不足は再試行せず、取消後は次のupload/postへ進まない。
  await run('Posting upload recovery and stage safety', async () => {
    await restart(secretsPath, stateDirectory, 1500);
    const invoke = async (problem, expectedUploads, expectedPosts, code, images = [pngPath]) => {
      scenario = problem;
      scenarioUploads = 0;
      const before = trace.length;
      const args = { account: 'posting', text: 'upload recovery', images };
      const data = await (code ? fails('post_tweet', args) : succeeds('post_tweet', args));
      const requests = trace.slice(before);
      assert.equal(requests.filter((item) => item.path.endsWith('/upload_media_v2')).length, expectedUploads, problem);
      assert.equal(requests.filter((item) => item.path.endsWith('/create_tweet_v2')).length, expectedPosts, problem);
      if (code) {
        assert.equal(data.error.code, code, problem);
        assert.equal(data.error.stage, expectedPosts ? 'post_creation' : 'image_upload', problem);
        assert.equal(data.error.postAttempted, Boolean(expectedPosts), problem);
        assert.equal(data.error.uploadAttempts, expectedUploads, problem);
      }
      return requests;
    };
    try {
      await invoke('upload-jsonerror', 1, 0, 'API_REJECTED');
      for (const status of [502, 503, 504]) {
        await invoke(`upload-recover-${status}`, 2, 1);
        await invoke(`upload-always-${status}`, 2, 0, 'API_HTTP_ERROR');
      }
      await invoke('upload-stream502', 2, 1);
      let bodyCloseTimer;
      try {
        await Promise.race([failedUploadBody.closed, new Promise((_, reject) => { bodyCloseTimer = setTimeout(() => reject(new Error('HTTP error body was not released')), 1000); })]);
      } finally { clearTimeout(bodyCloseTimer); }
      assert.equal(failedUploadBody.cancelled, true, 'HTTP error body was not cancelled');
      for (const status of [401, 429, 500]) await invoke(`upload-always-${status}`, 1, 0, 'API_HTTP_ERROR');
      await invoke('upload-html', 1, 0, 'INVALID_RESPONSE');
      await invoke('upload-missing-id', 1, 0, 'UPLOAD_FAILED');
      await invoke('upload-timeout', 1, 0, 'API_UNAVAILABLE');
      await invoke('upload-network', 1, 0, 'API_UNAVAILABLE');
      const ordered = await invoke('upload-retry-order', 5, 1, undefined, orderedImagePaths);
      const hashes = orderedImages.map((bytes) => createHash('sha256').update(bytes).digest('hex'));
      assert.deepEqual(ordered.slice(0, -1).map((item) => item.uploadedImageHash), [hashes[0], hashes[1], hashes[1], hashes[2], hashes[3]]);
      assert.deepEqual(ordered.at(-1).mediaIds, ['701', '703', '704', '705']);
      await invoke('create502', 1, 1, 'POST_UNCERTAIN');
      await invoke('jsonerror', 0, 1, 'API_REJECTED', []);
      await invoke('missing-id', 0, 1, 'POST_UNCERTAIN', []);
      scenario = 'normal';
      const invalid = await fails('post_tweet', { account: 'posting', text: 'invalid image', images: [invalidPath] }, true);
      assert.equal(invalid.error.stage, 'input_validation');
      assert.equal(invalid.error.postAttempted, false);
      assert.equal(invalid.error.uploadAttempts, 0);
      await restart();
      await invoke('upload-always-502', 1, 0, 'API_HTTP_ERROR');
      await restart(secretsPath, stateDirectory, 1000);
      const budgetStart = performance.now();
      await invoke('upload-retry-time-budget', 2, 0, 'API_UNAVAILABLE');
      assert.ok(performance.now() - budgetStart < 1200, 'Retry received a fresh timeout budget');
    } finally { scenario = 'normal'; await restart(); }
  });
  for (const [phase, expectedUploads] of [['backoff', 1], ['second', 2]]) await run(`MCP cancellation during upload retry ${phase} prevents further requests`, async () => {
    await restart(secretsPath, stateDirectory, 1500);
    scenario = `upload-cancel-${phase}`;
    scenarioUploads = 0;
    const before = trace.length;
    const controller = new AbortController();
    let acceptanceTimer;
    const accepted = new Promise((resolve, reject) => {
      uploadAccepted = resolve;
      acceptanceTimer = setTimeout(() => reject(new Error('Mock did not reject image upload')), 1000);
    });
    const request = client.callTool({ name: 'post_tweet', arguments: { account: 'posting', text: 'cancel backoff', images: [pngPath] } }, undefined, { signal: controller.signal, timeout: 5000 });
    const cancelled = assert.rejects(request);
    try {
      await accepted;
      if (phase === 'backoff') await new Promise((resolve) => setTimeout(resolve, 60));
      controller.abort();
      await cancelled;
      await new Promise((resolve) => setTimeout(resolve, 350));
      assert.equal(trace.slice(before).filter((item) => item.path.endsWith('/upload_media_v2')).length, expectedUploads);
      assert.equal(trace.slice(before).filter((item) => item.path.endsWith('/create_tweet_v2')).length, 0);
    } finally { clearTimeout(acceptanceTimer); uploadAccepted = undefined; controller.abort(); await cancelled; scenario = 'normal'; await restart(); }
  });
  // 失敗条件を先に固定: 全画像の事前検証、4枚の内容と順序、後続upload失敗時の投稿・再送禁止。
  for (const problem of ['image-order', 'image-later-failure']) await run(`Four image snapshots preserve order and stop on ${problem}`, async () => {
    scenario = problem;
    scenarioUploads = 0;
    const before = trace.length;
    try {
      const args = { account: 'posting', text: 'four images', images: orderedImagePaths };
      if (problem === 'image-order') await succeeds('post_tweet', args);
      else {
        const { error } = await fails('post_tweet', args);
        assert.equal(error.code, 'UPLOAD_FAILED');
        assert.equal(error.stage, 'image_upload');
        assert.equal(error.postAttempted, false);
        assert.equal(error.uploadAttempts, 3);
      }
      const requests = trace.slice(before);
      const uploads = requests.filter((item) => item.path.endsWith('/upload_media_v2'));
      assert.deepEqual(uploads.map((item) => item.uploadedImageHash), orderedImages.slice(0, problem === 'image-order' ? 4 : 3).map((bytes) => createHash('sha256').update(bytes).digest('hex')));
      assert.ok(uploads.every((item) => item.multipartPayloadValid));
      const posts = requests.filter((item) => item.path.endsWith('/create_tweet_v2'));
      assert.equal(posts.length, problem === 'image-order' ? 1 : 0);
      if (posts.length) { assert.deepEqual(posts[0].mediaIds, ['700', '701', '702', '703']); assert.equal(requests.at(-1), posts[0]); }
    } finally { scenario = 'normal'; }
  });
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
  // 失敗条件を先に固定: 無効・欠落・epoch前の日時、同時刻のID順、同数値のID表記を従来どおり扱う。
  await run('Feed date keys preserve invalid epoch negative and equal-date ordering', async () => {
    scenario = 'feed-date-order';
    try {
      const data = await succeeds('get_following_feed', { account: 'reading', maxAccounts: 20 });
      assert.deepEqual(data.tweets.map((item) => item.id), ['70', '070', '60', '50', '40', '30', '20', '10', '80']);
    } finally { scenario = 'normal'; }
  });
  // 失敗条件を先に固定: protected=true優先、無効名、初出の大小文字重複、上限後の公開重複。
  await run('Following selection scans late duplicates and preserves selected unknown counts', async () => {
    scenario = 'following-selection';
    try {
      for (const [name, extra] of [['get_following_feed', {}], ['search_tweets', { scope: 'following', keyword: 'needle phrase' }]]) {
        const before = trace.length;
        const data = await succeeds(name, { account: 'reading', maxAccounts: 20, ...extra });
        assert.equal(data.coverage.accounts, 20);
        assert.equal(data.coverage.protectedAccountsExcluded, 3);
        assert.equal(data.coverage.protectionStatusUnknownAccounts, 17);
        assert.equal(data.coverage.followingHasNextPage, true);
        const requests = trace.slice(before).filter((item) => item.path.endsWith('/advanced_search'));
        assert.equal(requests.length, 1);
        const names = [...requests[0].query.query.matchAll(/from:([a-z0-9_]+)/g)].map((match) => match[1]);
        assert.deepEqual(names, ['firstunknown', 'knownfirst', ...Array.from({ length: 18 }, (_, index) => `chosen${index}`)]);
      }
    } finally { scenario = 'normal'; }
  });
  await run('Feed batches deduplicate and sort with coverage', async () => { const before = trace.length; const data = await succeeds('get_following_feed', { account: 'reading' }); assert.equal(data.mode, 'reconstructed_following_feed'); assert.equal(data.officialHomeEquivalent, false); assert.deepEqual(data.tweets.map((item) => item.id), ['602', '601', '603']); assert.equal(data.coverage.protectedAccountsExcluded, 1); assert.equal(data.hasNextPage, true); assert.ok(data.nextCursor); cursor = data.nextCursor; const search = trace.slice(before).filter((item) => item.path.endsWith('/advanced_search')); assert.equal(search.length, 2); for (const request of search) { assert.ok((request.query.query.match(/from:/g) || []).length <= 20); assert.ok(request.query.query.includes('include:nativeretweets')); assert.equal(request.query.queryType, 'Latest'); assert.ok(!request.query.query.includes('from:Private')); } });
  await run('Feed cursor account binding rejects before HTTP', async () => { assert.ok(cursor, 'Feed did not provide cursor'); await fails('get_following_feed', { account: 'posting', cursor }, true); });
  // G1/G2: 同一アカウントのcursor改変と保護情報欠落を、署名・coverageで区別する。
  await run('Feed signed cursor rejects payload signature and unsigned legacy tampering', async () => {
    assert.ok(cursor?.startsWith('plumivox-feed.'));
    const [, payload, signature] = cursor.split('.');
    const decoded = JSON.parse(Buffer.from(payload, 'base64url').toString('utf8'));
    assert.equal(decoded.version, 2);
    decoded.batches[0].users = ['Outside'];
    const changed = Buffer.from(JSON.stringify(decoded)).toString('base64url');
    await fails('get_following_feed', { account: 'reading', cursor: `plumivox-feed.${changed}.${signature}` }, true);
    await fails('get_following_feed', { account: 'reading', cursor: `plumivox-feed.${payload}.${signature[0] === 'a' ? 'b' : 'a'}${signature.slice(1)}` }, true);
    decoded.version = 1;
    await fails('get_following_feed', { account: 'reading', cursor: Buffer.from(JSON.stringify(decoded)).toString('base64url') }, true);
  });
  await run('Feed and search distinguish unknown protection flags from known private accounts', async () => {
    scenario = 'unknown-following';
    try {
      for (const [name, args] of [['get_following_feed', {}], ['search_tweets', { scope: 'following', keyword: 'needle phrase' }]]) {
        const before = trace.length;
        const data = await succeeds(name, { account: 'reading', ...args });
        assert.equal(data.coverage.protectedAccountsExcluded, 1);
        assert.equal(data.coverage.protectionStatusUnknownAccounts, 2);
        const query = trace.slice(before).find((item) => item.path.endsWith('/advanced_search')).query.query.toLowerCase();
        assert.ok(query.includes('from:knownpublic'));
        assert.ok(query.includes('from:unknownpublic'));
        assert.ok(query.includes('from:otherunknown'));
        assert.ok(!query.includes('from:private'));
      }
    } finally { scenario = 'normal'; }
  });
  await run('Malformed cursor rejects before HTTP', () => fails('get_following_feed', { account: 'reading', cursor: 'bad-cursor!' }, true));
  // 旧wire形式を独立に構成し、共通化後も既発行cursorとcanonical署名の契約を維持する。
  const legacySignedCursor = (prefix, data) => {
    const payload = Buffer.from(JSON.stringify(data)).toString('base64url');
    return `${prefix}${payload}.${createHmac('sha256', fakeKey).update(payload).digest('base64url')}`;
  };
  const legacyFeedData = {
    version: 2, account: 'reading', userName: 'reader', batches: [{ users: ['public0'], cursor: 'search-next' }],
    coverage: { accounts: 1, followingHasNextPage: false, followingNextCursor: null, protectedAccountsExcluded: 0,
      protectionStatusUnknownAccounts: 0, followingPage: 'first', followingContinuationAvailable: false, followingContinuationUnavailableReason: null },
  };
  const legacySearchConditions = { account: 'reading', userName: 'reader', scope: 'all', keyword: 'needle phrase', startEpoch: null, endEpoch: null, maxAccounts: null };
  const legacySearchData = { version: 1, kind: 'tweets', conditions: legacySearchConditions, inner: 'search-filter-next' };
  await run('Legacy feed and search cursors resume without changing signed wire format', async () => {
    const first = await succeeds('get_following_feed', { account: 'reading', maxAccounts: 20 });
    assert.equal(first.nextCursor, legacySignedCursor('plumivox-feed.', { ...legacyFeedData,
      batches: [{ users: Array.from({ length: 20 }, (_, index) => `public${index}`), cursor: 'search-next' }],
      coverage: { ...legacyFeedData.coverage, accounts: 20, followingHasNextPage: true, protectedAccountsExcluded: 1 },
    }));
    const beforeFeed = trace.length;
    await succeeds('get_following_feed', { account: 'reading', cursor: legacySignedCursor('plumivox-feed.', legacyFeedData) });
    assert.equal(trace.length - beforeFeed, 1);
    assert.equal(trace.at(-1).query.cursor, 'search-next');
    scenario = 'search-normal';
    try {
      const firstSearch = await succeeds('search_tweets', { account: 'reading', keyword: 'needle phrase' });
      assert.equal(firstSearch.nextCursor, legacySignedCursor('plumivox-search.', legacySearchData));
      const next = await succeeds('search_tweets', { account: 'reading', keyword: 'needle phrase', cursor: legacySignedCursor('plumivox-search.', legacySearchData) });
      assert.equal(next.hasNextPage, false);
      assert.equal(trace.at(-1).query.cursor, 'search-filter-next');
      const nested = legacySignedCursor('plumivox-search.', { ...legacySearchData,
        conditions: { ...legacySearchConditions, scope: 'following', maxAccounts: 100 }, inner: legacySignedCursor('plumivox-feed.', legacyFeedData),
      });
      await succeeds('search_tweets', { account: 'reading', keyword: 'needle phrase', scope: 'following', cursor: nested });
      assert.equal(trace.at(-1).query.cursor, 'search-next');
    } finally { scenario = 'normal'; }
  });
  await run('Signed cursors reject noncanonical signatures malformed envelopes and wrong versions before HTTP', async () => {
    const alphabet = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_';
    for (const [name, prefix, data, args, message] of [
      ['get_following_feed', 'plumivox-feed.', legacyFeedData, {}, 'フィードのcursorが不正、または別アカウントのものです。'],
      ['search_tweets', 'plumivox-search.', legacySearchData, { keyword: 'needle phrase' }, '検索のcursorが不正、または検索条件・アカウントが一致しません。'],
    ]) {
      const valid = legacySignedCursor(prefix, data);
      const noncanonical = valid.slice(0, -1) + alphabet[alphabet.indexOf(valid.at(-1)) + 1];
      for (const invalid of [noncanonical, `${valid}.extra`, valid.slice(0, -1), legacySignedCursor(prefix, { ...data, version: 99 })]) {
        const rejected = await fails(name, { account: 'reading', ...args, cursor: invalid }, true);
        assert.equal(rejected.error.code, 'INVALID_CURSOR');
        assert.equal(rejected.error.message, message);
      }
    }
  });
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
  // 失敗条件を先に固定: 入力拒否、条件の脱落、cursorの混同・改変、提供元失敗、取消の再送。
  const searchArgs = { account: 'reading', keyword: 'needle phrase', startTime: '2026-10-12T20:00:00+09:00', endTime: '2026-10-13T00:00:00+09:00' };
  const inSearchScenario = async (name, check) => { scenario = name; try { await check(); } finally { scenario = 'normal'; } };
  const assertSearchQuery = (request, keyword = 'needle phrase') => {
    assert.equal(request.path, '/twitter/tweet/advanced_search');
    assert.equal(request.query.queryType, 'Latest');
    assert.ok(request.query.query.includes(`"${keyword}"`));
    assert.ok(request.query.query.includes(`since_time:${Date.parse(searchArgs.startTime) / 1000}`));
    assert.ok(request.query.query.includes(`until_time:${Date.parse(searchArgs.endTime) / 1000}`));
  };
  // G4: provider単独・合計・署名ラッパーの上限で取得済み投稿を失わない。
  const assertUnavailableTweets = (data) => {
    assert.deepEqual(data.tweets.map((item) => item.id), ['1200']);
    assert.equal(data.hasNextPage, true);
    assert.equal(data.nextCursor, null);
    assert.equal(data.coverage.tweetContinuationAvailable, false);
    assert.equal(data.coverage.tweetContinuationUnavailableReason, 'cursor_too_long');
  };
  await run('Oversized provider cursors retain feed and global following search tweets', () => inSearchScenario('cursor-provider-over', async () => {
    for (const [name, args] of [['get_following_feed', {}], ['search_tweets', { keyword: 'needle phrase' }], ['search_tweets', { keyword: 'needle phrase', scope: 'following' }]]) assertUnavailableTweets(await succeeds(name, { account: 'reading', ...args }));
  }));
  await run('Oversized timeline cursor retains fetched posts', () => inSearchScenario('cursor-timeline-over', async () => {
    assertUnavailableTweets(await succeeds('get_my_posts', { account: 'reading' }));
  }));
  await run('Oversized following list cursors retain coverage and tweets', () => inSearchScenario('cursor-following-list-over', async () => {
    for (const [name, args] of [['get_following_feed', {}], ['search_tweets', { keyword: 'needle phrase', scope: 'following' }]]) {
      const data = await succeeds(name, { account: 'reading', ...args });
      assert.ok(data.tweets.length > 0);
      assert.equal(data.coverage.followingHasNextPage, true);
      assert.equal(data.coverage.followingNextCursor, null);
      assert.equal(data.coverage.followingContinuationAvailable, false);
      assert.equal(data.coverage.followingContinuationUnavailableReason, 'cursor_too_long');
    }
  }));
  await run('Aggregate feed cursors over budget retain feed and following search tweets', () => inSearchScenario('cursor-aggregate-over', async () => {
    for (const [name, args] of [['get_following_feed', {}], ['search_tweets', { keyword: 'needle phrase', scope: 'following' }]]) {
      const before = trace.length;
      assertUnavailableTweets(await succeeds(name, { account: 'reading', maxAccounts: 160, ...args }));
      const requests = trace.slice(before).filter((item) => item.path.endsWith('/advanced_search'));
      assert.equal(requests.length, 8);
      assert.ok(requests.every((item) => !item.query.cursor));
    }
  }));
  await run('Signed search wrapper over budget retains global tweets', () => inSearchScenario('cursor-wrapper-over', async () => {
    const data = await succeeds('search_tweets', { account: oversizedCursorAccountName, keyword: 'needle phrase' });
    assertUnavailableTweets(data);
  }));
  await run('Signed following list wrapper over budget retains following search tweets', () => inSearchScenario('cursor-following-wrapper-over', async () => {
    const data = await succeeds('search_tweets', { account: oversizedCursorAccountName, scope: 'following', keyword: 'needle phrase' });
    assert.ok(data.tweets.length > 0);
    assert.equal(data.coverage.followingHasNextPage, true);
    assert.equal(data.coverage.followingNextCursor, null);
    assert.equal(data.coverage.followingContinuationAvailable, false);
    assert.equal(data.coverage.followingContinuationUnavailableReason, 'cursor_too_long');
  }));
  await run('Signed following search wrapper expansion retains tweets when inner feed cursor fits', () => inSearchScenario('cursor-wrapper-following', async () => {
    const args = { account: expandedFeedCursorAccountName, maxAccounts: 20 };
    const feed = await succeeds('get_following_feed', args);
    assert.ok(feed.nextCursor && feed.nextCursor.length <= 100000);
    const data = await succeeds('search_tweets', { ...args, scope: 'following', keyword: 'needle phrase' });
    assertUnavailableTweets(data);
  }));
  await run('Provider cursors below budget remain resumable in global search', () => inSearchScenario('cursor-provider-below', async () => {
    const data = await succeeds('search_tweets', { account: 'reading', keyword: 'needle phrase' });
    assert.ok(data.nextCursor);
    assert.equal(data.coverage.tweetContinuationAvailable, true);
    assert.equal(data.coverage.tweetContinuationUnavailableReason, null);
    const next = await succeeds('search_tweets', { account: 'reading', keyword: 'needle phrase', cursor: data.nextCursor });
    assert.equal(next.hasNextPage, false);
    assert.equal(trace.at(-1).query.cursor.length, 9500);
  }));
  await run('Aggregate cursors below budget remain resumable in feed and following search', () => inSearchScenario('cursor-aggregate-below', async () => {
    for (const [name, args] of [['get_following_feed', {}], ['search_tweets', { keyword: 'needle phrase', scope: 'following' }]]) {
      const data = await succeeds(name, { account: 'reading', maxAccounts: 160, ...args });
      assert.ok(data.nextCursor);
      assert.equal(data.coverage.tweetContinuationAvailable, true);
      assert.equal(data.coverage.tweetContinuationUnavailableReason, null);
      const next = await succeeds(name, { account: 'reading', maxAccounts: 160, ...args, cursor: data.nextCursor });
      assert.equal(next.hasNextPage, false);
      assert.equal(trace.at(-1).query.cursor.length, 8000);
    }
  }));
  let globalSearchCursor;
  let followingSearchCursor;
  let followingListCursor;
  await run('Search global phrase and UTC period reach provider', () => inSearchScenario('search-normal', async () => {
    const before = trace.length;
    const data = await succeeds('search_tweets', { ...searchArgs, keyword: '  needle phrase  ' });
    assert.equal(data.scope, 'all');
    assert.equal(data.keyword, 'needle phrase');
    assert.equal(data.startTime, '2026-10-12T11:00:00Z');
    assert.equal(data.endTime, '2026-10-12T15:00:00Z');
    assert.deepEqual(data.tweets.map((item) => item.id).sort(), ['801', '802', '803', '804', '808']);
    assert.equal(trace.length - before, 1);
    assert.equal(data.apiRequests, 1);
    assertSearchQuery(trace.at(-1));
    assert.ok(!trace.at(-1).query.query.includes('from:'));
    assert.equal(data.hasNextPage, true);
    assert.ok(data.nextCursor);
    globalSearchCursor = data.nextCursor;
  }));
  await run('Search global continuation preserves filters and equivalent offsets', () => inSearchScenario('search-normal', async () => {
    assert.ok(globalSearchCursor);
    const data = await succeeds('search_tweets', { ...searchArgs, startTime: '2026-10-12T11:00:00Z', endTime: '2026-10-12T15:00:00Z', cursor: globalSearchCursor });
    assertSearchQuery(trace.at(-1));
    assert.equal(trace.at(-1).query.cursor, 'search-filter-next');
    assert.equal(data.hasNextPage, false);
  }));
  await run('Search following batches preserve filters deduplicate sort and exclude other authors', () => inSearchScenario('search-normal', async () => {
    const before = trace.length;
    const data = await succeeds('search_tweets', { ...searchArgs, scope: 'following' });
    assert.deepEqual(data.tweets.map((item) => item.id), ['802', '801', '803']);
    assert.ok(data.tweets.every((item) => /^Public\d+$/.test(item.author.userName)));
    assert.equal(data.coverage.protectedAccountsExcluded, 1);
    const requests = trace.slice(before).filter((item) => item.path.endsWith('/advanced_search'));
    assert.equal(requests.length, 2);
    for (const request of requests) { assertSearchQuery(request); assert.ok(request.query.query.includes('include:nativeretweets')); assert.ok(!request.query.query.includes('from:Private')); }
    assert.ok(data.nextCursor);
    followingSearchCursor = data.nextCursor;
  }));
  await run('Search following tweet continuation preserves both batch conditions', () => inSearchScenario('search-normal', async () => {
    assert.ok(followingSearchCursor);
    const before = trace.length;
    const data = await succeeds('search_tweets', { ...searchArgs, scope: 'following', cursor: followingSearchCursor });
    assert.equal(data.hasNextPage, false);
    const requests = trace.slice(before);
    assert.equal(requests.length, 2);
    for (const request of requests) { assertSearchQuery(request); assert.equal(request.query.cursor, 'search-filter-next'); }
  }));
  await run('Search empty following avoids advanced search', () => inSearchScenario('search-empty', async () => {
    const before = trace.length;
    const data = await succeeds('search_tweets', { ...searchArgs, scope: 'following' });
    assert.deepEqual(data.tweets, []);
    assert.equal(data.hasNextPage, false);
    assert.equal(trace.slice(before).filter((item) => item.path.endsWith('/advanced_search')).length, 0);
  }));
  await run('Search incomplete following list exposes bound continuation', () => inSearchScenario('search-following-page', async () => {
    const data = await succeeds('search_tweets', { ...searchArgs, scope: 'following' });
    assert.equal(data.coverage.followingHasNextPage, true);
    assert.ok(data.coverage.followingNextCursor);
    assert.notEqual(data.coverage.followingNextCursor, 'following-next');
    followingListCursor = data.coverage.followingNextCursor;
  }));
  await run('Search following list continuation keeps phrase and period', () => inSearchScenario('search-normal', async () => {
    assert.ok(followingListCursor);
    const before = trace.length;
    const data = await succeeds('search_tweets', { ...searchArgs, scope: 'following', followingCursor: followingListCursor });
    assert.deepEqual(data.tweets.map((item) => item.id), ['808']);
    const requests = trace.slice(before);
    assert.equal(requests[0].query.cursor, 'following-next');
    assertSearchQuery(requests[1]);
    assert.ok(requests[1].query.query.toLowerCase().includes('from:laterpublic'));
  }));
  await run('Search operator text stays a phrase inside following condition', () => inSearchScenario('search-normal', async () => {
    const data = await succeeds('search_tweets', { ...searchArgs, scope: 'following', keyword: 'needle OR from:Outside' });
    assert.deepEqual(data.tweets.map((item) => item.id), ['809']);
    assertSearchQuery(trace.at(-1), 'needle OR from:Outside');
  }));
  await run('Search invalid keywords reject without HTTP', async () => {
    for (const keyword of ['', '   ', 'x'.repeat(513), 'quote" OR from:Outside', 'back\\slash', 'line\nbreak', 'line\u2028break']) await fails('search_tweets', { ...searchArgs, keyword }, true);
  });
  await run('Search invalid dates and ranges reject without HTTP', async () => {
    for (const startTime of ['2026-10-12', '2026-02-30T11:00:00Z', '2026-10-12T25:00:00Z', '2026-10-12T11:00:00', '2026-10-12T11:00:00.5Z', '2026-10-12T11:00:00+24:00']) await fails('search_tweets', { ...searchArgs, startTime }, true);
    for (const endTime of [searchArgs.startTime, '2026-10-12T10:00:00Z']) await fails('search_tweets', { ...searchArgs, endTime }, true);
    await fails('search_tweets', { ...searchArgs, endTime: '2026-02-30T15:00:00Z' }, true);
    for (const maxAccounts of [19, 201, 20.5]) await fails('search_tweets', { ...searchArgs, scope: 'following', maxAccounts }, true);
  });
  await run('Search all rejects following-only arguments without HTTP', async () => {
    await fails('search_tweets', { ...searchArgs, maxAccounts: 100 }, true);
    await fails('search_tweets', { ...searchArgs, followingCursor: 'following-next' }, true);
  });
  await run('Search cursors bind account scope keyword period and following limit', async () => {
    assert.ok(followingSearchCursor);
    const bound = { ...searchArgs, scope: 'following', cursor: followingSearchCursor };
    for (const changed of [{ account: 'posting' }, { scope: 'all' }, { keyword: 'other phrase' }, { startTime: '2026-10-12T12:00:00Z' }, { endTime: '2026-10-12T16:00:00Z' }, { maxAccounts: 20 }]) await fails('search_tweets', { ...bound, ...changed }, true);
    await fails('search_tweets', { ...bound, cursor: `${followingSearchCursor[0] === 'a' ? 'b' : 'a'}${followingSearchCursor.slice(1)}` }, true);
  });
  await run('Search rejects mixed cursor kinds and old feed cursors', async () => {
    assert.ok(cursor && followingSearchCursor && followingListCursor);
    await fails('search_tweets', { ...searchArgs, scope: 'following', cursor }, true);
    await fails('get_following_feed', { account: 'reading', cursor: followingSearchCursor }, true);
    await fails('search_tweets', { ...searchArgs, scope: 'following', cursor: followingListCursor }, true);
    await fails('search_tweets', { ...searchArgs, scope: 'following', followingCursor: followingSearchCursor }, true);
    await fails('search_tweets', { ...searchArgs, scope: 'following', followingCursor: followingListCursor, cursor: followingSearchCursor }, true);
    for (const changed of [{ account: 'posting' }, { scope: 'all' }, { keyword: 'other phrase' }, { startTime: '2026-10-12T12:00:00Z' }, { endTime: '2026-10-12T16:00:00Z' }, { maxAccounts: 20 }]) await fails('search_tweets', { ...searchArgs, scope: 'following', followingCursor: followingListCursor, ...changed }, true);
    await fails('search_tweets', { ...searchArgs, scope: 'following', followingCursor: 'following-next' }, true);
  });
  await run('Search provider errors reject safely without retry', async () => {
    for (const name of ['http500', 'html', 'search-malformed']) await inSearchScenario(name, async () => {
      const before = trace.length;
      await fails('search_tweets', searchArgs);
      assert.equal(trace.length - before, 1);
    });
  });
  await run('Search cancellation does not resend', () => inSearchScenario('search-cancel', async () => {
    const before = trace.length;
    const accepted = new Promise((resolve) => { searchAccepted = resolve; });
    const controller = new AbortController();
    const request = client.callTool({ name: 'search_tweets', arguments: searchArgs }, undefined, { signal: controller.signal, timeout: 5000 });
    const cancelled = assert.rejects(request);
    let acceptanceTimer;
    try { await Promise.race([accepted, new Promise((_, reject) => { acceptanceTimer = setTimeout(() => reject(new Error('Search did not reach mock API')), 5000); })]); controller.abort(); await cancelled; await new Promise((resolve) => setTimeout(resolve, 200)); assert.equal(trace.length - before, 1); }
    finally { clearTimeout(acceptanceTimer); searchAccepted = undefined; controller.abort(); await cancelled; }
  }));
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
  await run('Reused login inspects only its selected account after configuration reread', async () => {
    const prefix = path.join(output, `account-inspection-${randomUUID()}`);
    const configPath = `${prefix}.json`, sessionRoot = `${prefix}-state`;
    const preloadPath = `${prefix}.mjs`, countsPath = `${prefix}-counts.json`;
    const profiles = Object.fromEntries(['selected', 'other1', 'other2'].map((name) => [name,
      { ...syntheticConfig.twitterapi_io.posting, user_name: name }]));
    const config = { twitterapi_io: { ...profiles, default_account: 'selected' } };
    await writeFile(configPath, JSON.stringify(config));
    for (const name of ['other1', 'other2']) {
      const directory = path.join(sessionRoot, createHash('sha256').update(`${name}\n${name}`).digest('hex'));
      await mkdir(directory, { recursive: true });
      await writeFile(path.join(directory, 'session.dpapi'), 'invalid synthetic encrypted session');
    }
    authGateFiles.push(configPath, sessionRoot, preloadPath, countsPath);
    await writeFile(preloadPath, `import fs from 'node:fs/promises';
import childProcess from 'node:child_process';
import { writeFileSync } from 'node:fs';
import { syncBuiltinESMExports } from 'node:module';
import { McpServer } from ${JSON.stringify(pathToFileURL(path.join(repo, 'node_modules/@modelcontextprotocol/sdk/dist/esm/server/mcp.js')).href)};
const counts = { configReads: 0, sessionChecks: 0, sessionReads: 0, decryptSpawns: 0 }, results = [];
const readFile = fs.readFile, lstat = fs.lstat, spawn = childProcess.spawn;
fs.readFile = async function(file, ...args) {
  const value = await readFile.call(this, file, ...args);
  if (String(file) === ${JSON.stringify(configPath)} && ++counts.configReads === 1) {
    const changed = JSON.parse(value); changed.twitterapi_io.default_account = 'other1';
    writeFileSync(file, JSON.stringify(changed));
  }
  if (String(file).endsWith('session.dpapi')) counts.sessionReads++;
  return value;
};
fs.lstat = function(file, ...args) { if (String(file).endsWith('session.dpapi')) counts.sessionChecks++; return lstat.call(this, file, ...args); };
childProcess.spawn = function(command, args, ...rest) {
  if (command === 'pwsh' && args?.includes('-EncodedCommand')) counts.decryptSpawns++;
  return spawn.call(this, command, args, ...rest);
};
syncBuiltinESMExports();
const registerTool = McpServer.prototype.registerTool;
McpServer.prototype.registerTool = function(name, definition, callback) {
  return registerTool.call(this, name, definition, async (...args) => {
    const before = { ...counts }, result = await callback(...args);
    results.push({ name, ...Object.fromEntries(Object.entries(counts).map(([key, value]) => [key, value - before[key]])) });
    writeFileSync(${JSON.stringify(countsPath)}, JSON.stringify(results));
    return result;
  });
};
`);
    const measuredTransport = makeTransport(configPath, sessionRoot, 250, preloadPath);
    const measuredClient = new Client({ name: 'plumivox-account-inspection-e2e', version: '1.0.0' });
    try {
      await measuredClient.connect(measuredTransport);
      const before = trace.length;
      const reused = await call('login_account', {}, measuredClient);
      assert.notEqual(reused.result.isError, true);
      assertLoginMetadata(reused.data, true);
      assert.equal(reused.data.account, 'selected');
      assert.equal(trace.length, before);
      const listed = await call('list_accounts', {}, measuredClient);
      assert.notEqual(listed.result.isError, true);
      assert.equal(listed.data.accounts.length, 3);
      assert.deepEqual(listed.data.accounts.filter((item) => item.isDefault).map((item) => item.account), ['other1']);
      assert.deepEqual(JSON.parse(await readFile(countsPath, 'utf8')), [
        { name: 'login_account', configReads: 2, sessionChecks: 2, sessionReads: 0, decryptSpawns: 0 },
        { name: 'list_accounts', configReads: 1, sessionChecks: 3, sessionReads: 2, decryptSpawns: 2 },
      ]);
    } finally { await measuredClient.close(); }
  });
  await run('DPAPI output byte limit preserves UTF8 across child stream chunks', async () => {
    const prefix = path.join(output, `session-output-${randomUUID()}`);
    const configPath = `${prefix}.json`, sessionRoot = `${prefix}-state`;
    const preloadPath = `${prefix}.mjs`, childPath = `${prefix}-child.mjs`;
    const profiles = {};
    for (const [name, offset] of [['below', -1], ['exact', 0], ['over', 1]]) {
      profiles[name] = { ...syntheticConfig.twitterapi_io.posting, user_name: name };
      const directory = path.join(sessionRoot, createHash('sha256').update(`${name}\n${name}`).digest('hex'));
      await mkdir(directory, { recursive: true });
      await writeFile(path.join(directory, 'session.dpapi'), JSON.stringify({ name, bytes: 2 * 1024 * 1024 + offset }));
    }
    await writeFile(configPath, JSON.stringify({ twitterapi_io: profiles }));
    authGateFiles.push(configPath, sessionRoot, preloadPath, childPath);
    await writeFile(childPath, `import { once } from 'node:events';
let input = ''; process.stdin.setEncoding('utf8');
for await (const chunk of process.stdin) input += chunk;
const { name, bytes } = JSON.parse(JSON.parse(input).value);
const session = JSON.stringify({ schema_version: 2, cookie: 'synthetic-🙂あe\\u0301-cookie',
  binding: ${JSON.stringify(createHash('sha256').update(`${fakeKey}\n${syntheticConfig.twitterapi_io.posting.proxy}`).digest('hex'))},
  user_name: name, source: 'plumivox_api', acquired_at: '2026-10-21T00:00:00.000Z' });
const body = Buffer.from(session + ' '.repeat(bytes - Buffer.byteLength(session)));
const split = body.indexOf(Buffer.from('🙂'));
for (const chunk of [body.subarray(0, split + 1), body.subarray(split + 1, split + 2), body.subarray(split + 2, split + 3), body.subarray(split + 3, split + 4)]) {
  process.stdout.write(chunk); await new Promise((resolve) => setTimeout(resolve, 15));
}
for (let offset = split + 4; offset < body.length; offset += 16381) {
  if (!process.stdout.write(body.subarray(offset, offset + 16381))) await once(process.stdout, 'drain');
  await new Promise(setImmediate);
}
`);
    await writeFile(preloadPath, `import childProcess from 'node:child_process';
import { syncBuiltinESMExports } from 'node:module';
const spawn = childProcess.spawn;
childProcess.spawn = function(command, args, options) {
  return command === 'pwsh' && args?.includes('-EncodedCommand')
    ? spawn.call(this, process.execPath, [${JSON.stringify(childPath)}], options)
    : spawn.apply(this, arguments);
};
syncBuiltinESMExports();
`);
    const boundedTransport = makeTransport(configPath, sessionRoot, 250, preloadPath);
    const boundedClient = new Client({ name: 'plumivox-session-output-e2e', version: '1.0.0' });
    try {
      await boundedClient.connect(boundedTransport);
      const { result, data } = await call('list_accounts', {}, boundedClient);
      assert.notEqual(result.isError, true);
      assert.deepEqual(data.accounts.map(({ account, postConfigured, sessionSource }) => ({ account, postConfigured, sessionSource })), [
        { account: 'below', postConfigured: true, sessionSource: 'plumivox_dpapi' },
        { account: 'exact', postConfigured: true, sessionSource: 'plumivox_dpapi' },
        { account: 'over', postConfigured: false, sessionSource: null },
      ]);
    } finally { await boundedClient.close(); }
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
  for (const problem of ['http401', 'http502', 'http503', 'http504', 'html', 'jsonerror', 'login-missing-cookie', 'timeout']) await run(`${problem} login failure retains encrypted session and does not retry`, async () => {
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
  await run('Invalid login cookies reject safely without changing saved session', async () => {
    const previous = await readFile(ownSessionPath);
    for (const name of ['login-cookie-newline', 'login-cookie-nul', 'login-cookie-utf8-over']) {
      scenario = name;
      try { const before = trace.length; const data = await fails('refresh_session', { account: 'auth' }); assert.equal(data.error.code, 'LOGIN_FAILED'); assert.deepEqual(await readFile(ownSessionPath), previous); assert.equal(trace.length - before, 1); }
      finally { scenario = 'normal'; }
    }
  });
  await run('Cancellation while login HTTP waits reports unchanged session before commit', async () => {
    const previous = await readFile(ownSessionPath);
    const gate = await authGate('login');
    const before = trace.length;
    let acceptanceTimer;
    const accepted = new Promise((resolve, reject) => { loginAccepted = resolve; acceptanceTimer = setTimeout(() => reject(new Error('Mock did not receive cancellable login')), 10000); });
    scenario = 'interprocess-login';
    releaseHeldLogin = undefined;
    const controller = new AbortController();
    const pending = call('refresh_session', { account: 'auth' }, gate.connection, { signal: controller.signal });
    const cancelled = assert.rejects(pending);
    try {
      await accepted;
      controller.abort();
      await cancelled;
      assert.equal((await gate.waitResult()).code, 'AUTH_CANCELLED');
      assert.deepEqual(await readFile(ownSessionPath), previous);
      assert.equal(trace.length - before, 1);
    } finally { clearTimeout(acceptanceTimer); loginAccepted = undefined; controller.abort(); releaseHeldLogin?.(); releaseHeldLogin = undefined; scenario = 'normal'; await gate.connection.close().catch(() => {}); await cancelled; }
  });
  await run('Cancellation during encryption reports unchanged session before commit', async () => {
    const previous = await readFile(ownSessionPath);
    const gate = await authGate('encrypt');
    const before = trace.length;
    const controller = new AbortController();
    const pending = call('refresh_session', { account: 'auth' }, gate.connection, { signal: controller.signal });
    const cancelled = assert.rejects(pending);
    try {
      const phase = await gate.wait();
      assert.equal(phase.phase, 'encrypt');
      controller.abort();
      await cancelled;
      await gate.resume();
      assert.equal((await gate.waitResult()).code, 'AUTH_CANCELLED');
      assert.deepEqual(await readFile(ownSessionPath), previous);
      assert.equal(trace.length - before, 1);
    } finally { controller.abort(); await gate.resume(); await gate.connection.close().catch(() => {}); await cancelled; }
  });
  await run('Lease loss during encryption reports unchanged session before commit', async () => {
    const previous = await readFile(ownSessionPath);
    const gate = await authGate('encrypt');
    const before = trace.length;
    const pending = call('refresh_session', { account: 'auth' }, gate.connection);
    pending.catch(() => {});
    try {
      const phase = await gate.wait();
      assert.equal(phase.phase, 'encrypt');
      const keeper = await verifiedKeeper(gate.transport.pid);
      assert.notEqual(keeper, phase.pid, 'Kill the verified mutex owner rather than the encryption process');
      process.kill(keeper);
      await gate.resume();
      const result = await pending;
      assert.equal(result.result.isError, true);
      assert.equal(result.data.error.code, 'AUTH_LOCK_LOST');
      assert.deepEqual(await readFile(ownSessionPath), previous);
      assert.equal(trace.length - before, 1);
    } finally { await gate.resume(); await gate.connection.close().catch(() => {}); await pending.catch(() => {}); }
  });
  await run('Lost saved acknowledgement reports uncertainty while committed session remains usable', async () => {
    const previous = await readFile(ownSessionPath);
    const gate = await authGate('saved');
    const before = trace.length;
    const pending = call('refresh_session', { account: 'auth' }, gate.connection);
    pending.catch(() => {});
    try {
      const phase = await gate.wait();
      assert.equal(phase.phase, 'saved');
      assert.notDeepEqual(await readFile(ownSessionPath), previous);
      const keeper = await verifiedKeeper(gate.transport.pid);
      assert.equal(keeper, phase.pid);
      process.kill(keeper);
      const result = await pending;
      assert.equal(result.result.isError, true);
      assert.equal(result.data.error.code, 'AUTH_SAVE_UNCERTAIN');
      const accounts = await succeeds('list_accounts');
      assert.equal(accounts.accounts.find((item) => item.account === 'auth').postConfigured, true);
      assertLoginMetadata(await succeeds('login_account', { account: 'auth' }), true);
      assert.equal(trace.length - before, 1, 'Uncertain save must not automatically log in again');
    } finally { await gate.connection.close().catch(() => {}); await pending.catch(() => {}); }
  });
  await run('Cancellation after commit reports uncertainty without undoing saved session', async () => {
    const previous = await readFile(ownSessionPath);
    const gate = await authGate('saved');
    const before = trace.length;
    const controller = new AbortController();
    const pending = call('refresh_session', { account: 'auth' }, gate.connection, { signal: controller.signal });
    const cancelled = assert.rejects(pending);
    try {
      const phase = await gate.wait();
      assert.equal(phase.phase, 'saved');
      const replacement = await readFile(ownSessionPath);
      assert.notDeepEqual(replacement, previous);
      controller.abort();
      await cancelled;
      assert.equal((await gate.waitResult()).code, 'AUTH_SAVE_UNCERTAIN');
      assert.deepEqual(await readFile(ownSessionPath), replacement);
      assert.equal(trace.length - before, 1);
    } finally { controller.abort(); await gate.connection.close().catch(() => {}); await cancelled; }
  });
  await run('Actual storage failure retains storage error classification', async () => {
    const blocked = path.join(output, 'auth-storage-blocked-file');
    await writeFile(blocked, 'This regular file cannot be a session directory');
    authGateFiles.push(blocked);
    const blockedTransport = makeTransport(secretsPath, blocked);
    const blockedClient = new Client({ name: 'plumivox-storage-failure-e2e', version: '1.0.0' });
    blockedTransport.stderr?.on('data', (data) => diagnostics.push(String(data)));
    try { await blockedClient.connect(blockedTransport); const result = await call('refresh_session', { account: 'auth' }, blockedClient); assert.equal(result.result.isError, true); assert.equal(result.data.error.code, 'SESSION_STORAGE_UNAVAILABLE'); }
    finally { await blockedClient.close().catch(() => {}); }
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
  await run('Losing mutex child while MCP survives prevents stale session overwrite', async () => {
    const ownerTransport = makeTransport(secretsPath, stateDirectory, 15000);
    const contenderTransport = makeTransport(secretsPath, stateDirectory, 15000);
    const owner = new Client({ name: 'plumivox-lost-mutex-owner', version: '1.0.0' });
    const contender = new Client({ name: 'plumivox-lost-mutex-contender', version: '1.0.0' });
    for (const stream of [ownerTransport.stderr, contenderTransport.stderr]) stream?.on('data', (data) => diagnostics.push(String(data)));
    let acceptanceTimer;
    let pending;
    try {
      await Promise.all([owner.connect(ownerTransport), contender.connect(contenderTransport)]);
      // 単独ケースでも旧保存の保持を検証できるよう、偽APIだけで初期保存を作る。
      assertLoginMetadata(await succeeds('refresh_session', { account: 'auth' }), false);
      const previous = await readFile(ownSessionPath);
      scenario = 'interprocess-login';
      releaseHeldLogin = undefined;
      const before = trace.length;
      const accepted = new Promise((resolve, reject) => { loginAccepted = resolve; acceptanceTimer = setTimeout(() => reject(new Error('Mock did not receive owner login')), 10000); });
      pending = call('refresh_session', { account: 'auth' }, owner);
      pending.catch(() => {});
      await accepted;
      const ownerPid = ownerTransport.pid;
      assert.ok(Number.isInteger(ownerPid) && ownerPid > 0);
      // 今回起動したMCPの直接子に限定し、Mutexスクリプトを確認してから終了する。
      const query = `$ids = @(Get-CimInstance Win32_Process -Filter 'ParentProcessId = ${ownerPid}' | Where-Object { $_.Name -eq 'pwsh.exe' } | ForEach-Object { if ($_.CommandLine -match '-EncodedCommand\\s+(\\S+)') { $scriptText = [Text.Encoding]::Unicode.GetString([Convert]::FromBase64String($Matches[1])); if ($scriptText.Contains('[Threading.Mutex]::new') -and $scriptText.Contains('Plumivox')) { [int]$_.ProcessId } } }); ConvertTo-Json -InputObject $ids -Compress`;
      const { stdout } = await promisify(execFile)('pwsh', ['-NoLogo', '-NoProfile', '-NonInteractive', '-Command', query], { windowsHide: true, timeout: 10000 });
      const children = JSON.parse(stdout.trim());
      assert.equal(children.length, 1, 'Exactly one verified mutex keeper must exist');
      assert.ok(children[0] !== ownerPid && children[0] !== process.pid);
      process.kill(children[0]);
      const lost = await pending;
      assert.equal(lost.result.isError, true);
      assert.equal(lost.data.error.code, 'AUTH_LOCK_LOST');
      assert.equal(ownerTransport.pid, ownerPid, 'MCP parent must remain alive');
      assert.equal((await owner.listTools()).tools.length, 11);
      assert.deepEqual(await readFile(ownSessionPath), previous);
      scenario = 'normal';
      const recovered = await call('refresh_session', { account: 'auth' }, contender);
      assert.notEqual(recovered.result.isError, true, JSON.stringify(recovered.data));
      assertLoginMetadata(recovered.data, false);
      const replacement = await readFile(ownSessionPath);
      assert.notDeepEqual(replacement, previous);
      releaseHeldLogin();
      assert.deepEqual(await readFile(ownSessionPath), replacement);
      assert.equal(trace.slice(before).filter((item) => item.path.endsWith('/user_login_v2')).length, 2);
      assert.equal(trace.slice(before).filter((item) => item.path.endsWith('/create_tweet_v2')).length, 0);
    } finally {
      clearTimeout(acceptanceTimer);
      loginAccepted = undefined;
      releaseHeldLogin?.();
      await Promise.all([owner.close().catch(() => {}), contender.close().catch(() => {})]);
      await pending?.catch(() => {});
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
const summary = { status: results.every((item) => item.status === 'pass') ? 'pass' : 'fail', passed: results.filter((item) => item.status === 'pass').length, failed: results.filter((item) => item.status === 'fail').length, results, retainedFiles: [secretsPath, mismatchedSecretsPath, singleAccountSecretsPath, pngPath, ...orderedImagePaths, invalidPath, oversizedPath, stateDirectory, corruptedStateDirectory, ...authGateFiles], retention: 'Synthetic input fixtures, E2E-only auth gates and encrypted sessions are retained for the caller to remove through the approved Windows recycle-bin workflow; summary and HTTP trace are reproducible evidence.' };
await writeFile(path.join(output, 'summary.json'), JSON.stringify(summary, null, 2));
await writeFile(path.join(output, 'http-trace.json'), JSON.stringify(trace, null, 2));
console.log(JSON.stringify({ status: summary.status, passed: summary.passed, failed: summary.failed, output }));
for (const failure of results.filter((item) => item.status === 'fail')) console.error(`${failure.name}: ${failure.message}`);
process.exitCode = summary.failed ? 1 : 0;
