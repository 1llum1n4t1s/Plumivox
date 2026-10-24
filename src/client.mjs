import { open } from 'node:fs/promises';
import path from 'node:path';
import { createHmac, timingSafeEqual } from 'node:crypto';
import twitterText from 'twitter-text';
import emojiRegex from 'emoji-regex';
import { setTimeout as delay } from 'node:timers/promises';

const ID = /^\d{1,30}$/;
const USER = /^[A-Za-z0-9_]{1,15}$/;
const MAX_IMAGE = 5 * 1024 * 1024;
const MAX_RESPONSE_BYTES = 10 * 1024 * 1024;
const BATCH_SIZE = 20;
const FEED_CURSOR_PREFIX = 'plumivox-feed.';
const MAX_FEED_CURSOR = 100_000;
const MAX_PROVIDER_CURSOR = 10_000;
const SEARCH_CURSOR_PREFIX = 'plumivox-search.';
const MAX_SEARCH_CURSOR = 200_000;

export class PlumivoxError extends Error {
  constructor(code, message, details = {}) {
    super(message);
    this.code = code;
    this.details = details;
  }
}

const fail = (code, message, details) => { throw new PlumivoxError(code, message, details); };
const object = (value) => value !== null && typeof value === 'object' && !Array.isArray(value);
const numericId = (value) => typeof value === 'string' && ID.test(value);

async function responseText(response) {
  if (!response.body) return '';
  const reader = response.body.getReader();
  const decoder = new TextDecoder();
  const parts = [];
  let length = 0;
  try {
    for (;;) {
      const { value, done } = await reader.read();
      length += value?.byteLength ?? 0;
      // 受信バイト数を制限し、上限超過分を文字列へ変換・蓄積しない。
      if (length > MAX_RESPONSE_BYTES) {
        await reader.cancel().catch(() => {});
        fail('INVALID_RESPONSE', 'API応答のサイズが上限を超えました。');
      }
      const part = decoder.decode(value, { stream: !done });
      parts.push(part);
      if (done) return parts.join('');
    }
  } finally {
    reader.releaseLock();
  }
}

function parseTweetText(text) {
  const normalized = text.normalize('NFC');
  const urls = twitterText.extractUrlsWithIndices(normalized);
  // 本文とURL境界を変えずに評価し、文字数超過と無効文字を別々に判定する。
  const parsed = twitterText.parseTweet(normalized, { ...twitterText.configs.defaults, maxWeightedTweetLength: Number.MAX_SAFE_INTEGER });
  let weightedLength = parsed.weightedLength;
  for (const { 0: emoji, index } of normalized.matchAll(emojiRegex())) {
    if ([...emoji].length === 1 || urls.some(({ indices }) => index < indices[1] && index + emoji.length > indices[0])) continue;
    // Unicode 17の既知の複合絵文字だけを重み2へ補正する。
    weightedLength -= twitterText.parseTweet(emoji).weightedLength - 2;
  }
  return { weightedLength, valid: parsed.valid && weightedLength <= twitterText.configs.defaults.maxWeightedTweetLength };
}

function normalizeTweet(value) {
  if (!object(value) || !numericId(value.id)) fail('INVALID_RESPONSE', '投稿データの形式が不正です。');
  return {
    id: value.id,
    url: `https://x.com/i/status/${value.id}`,
    text: typeof value.text === 'string' ? value.text : '',
    createdAt: typeof value.createdAt === 'string' ? value.createdAt : null,
    author: object(value.author) ? {
      id: typeof value.author.id === 'string' ? value.author.id : null,
      userName: typeof value.author.userName === 'string' ? value.author.userName : null,
      name: typeof value.author.name === 'string' ? value.author.name : null,
    } : null,
    isReply: value.isReply === true,
    isRetweet: value.isRetweet === true || object(value.retweeted_tweet),
    isQuote: value.isQuote === true,
  };
}

function tweetPage(response, maxCursorLength = MAX_FEED_CURSOR) {
  if (!Array.isArray(response.tweets)) fail('INVALID_RESPONSE', '投稿一覧を確認できません。');
  const hasNextPage = response.has_next_page === true;
  const nextCursor = typeof response.next_cursor === 'string' && response.next_cursor ? response.next_cursor : null;
  if (hasNextPage && !nextCursor) fail('INVALID_RESPONSE', '続きの取得情報が不足しています。');
  const cursorTooLong = hasNextPage && nextCursor.length > maxCursorLength;
  return { tweets: response.tweets.map(normalizeTweet), hasNextPage, nextCursor: hasNextPage && !cursorTooLong ? nextCursor : null,
    ...(cursorTooLong ? { coverage: { tweetContinuationAvailable: false, tweetContinuationUnavailableReason: 'cursor_too_long' } } : {}) };
}

async function imageFile(filePath) {
  if (!path.isAbsolute(filePath)) fail('INVALID_IMAGE', '画像には絶対パスを指定してください。');
  let handle;
  try {
    handle = await open(filePath, 'r');
    const info = await handle.stat();
    if (!info.isFile() || info.size < 4 || info.size > MAX_IMAGE) fail('INVALID_IMAGE', '画像は5 MiB以下のPNGまたはJPEGファイルにしてください。');
    const bytes = await handle.readFile();
    if (bytes.length > MAX_IMAGE) fail('INVALID_IMAGE', '画像は5 MiB以下にしてください。');
    const png = bytes.subarray(0, 8).equals(Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]));
    const jpeg = bytes[0] === 0xff && bytes[1] === 0xd8 && bytes[2] === 0xff;
    if (!png && !jpeg) fail('INVALID_IMAGE', 'PNGまたはJPEG画像を指定してください。');
    return { bytes, type: png ? 'image/png' : 'image/jpeg', name: png ? 'image.png' : 'image.jpg' };
  } catch (error) {
    if (error instanceof PlumivoxError) throw error;
    fail('INVALID_IMAGE', '画像ファイルを読み込めません。');
  } finally {
    await handle?.close();
  }
}

function encodeSignedCursor(data, apiKey, prefix, maxLength) {
  const payload = Buffer.from(JSON.stringify(data)).toString('base64url');
  const signature = createHmac('sha256', apiKey).update(payload).digest('base64url');
  const cursor = `${prefix}${payload}.${signature}`;
  return cursor.length <= maxLength ? cursor : null;
}

function decodeSignedCursor(cursor, apiKey, prefix, maxLength) {
  if (typeof cursor !== 'string' || cursor.length > maxLength || !cursor.startsWith(prefix)) throw new Error();
  const [payload, signature, extra] = cursor.slice(prefix.length).split('.');
  if (extra !== undefined || !/^[A-Za-z0-9_-]+$/.test(payload ?? '') || !/^[A-Za-z0-9_-]{43}$/.test(signature ?? '')) throw new Error();
  const expected = createHmac('sha256', apiKey).update(payload).digest();
  const received = Buffer.from(signature, 'base64url');
  if (received.length !== expected.length || received.toString('base64url') !== signature || !timingSafeEqual(received, expected)) throw new Error();
  return JSON.parse(Buffer.from(payload, 'base64url').toString('utf8'));
}

function encodeFeedCursor(batches, coverage, account) {
  return encodeSignedCursor({
    version: 2, account: account.account, userName: account.userName.toLowerCase(), batches, coverage,
  }, account.apiKey, FEED_CURSOR_PREFIX, MAX_FEED_CURSOR);
}

function decodeFeedCursor(cursor, account) {
  try {
    const data = decodeSignedCursor(cursor, account.apiKey, FEED_CURSOR_PREFIX, MAX_FEED_CURSOR);
    if (data.version !== 2 || data.account !== account.account || data.userName !== account.userName.toLowerCase()
      || !Array.isArray(data.batches) || data.batches.length < 1 || data.batches.length > 10
      || !object(data.coverage) || !Number.isInteger(data.coverage.accounts)
      || data.coverage.accounts < 0 || data.coverage.accounts > 200
      || typeof data.coverage.followingHasNextPage !== 'boolean'
      || !Number.isInteger(data.coverage.protectedAccountsExcluded) || data.coverage.protectedAccountsExcluded < 0
      || data.coverage.protectedAccountsExcluded > 200
      || !Number.isInteger(data.coverage.protectionStatusUnknownAccounts) || data.coverage.protectionStatusUnknownAccounts < 0
      || data.coverage.protectionStatusUnknownAccounts > data.coverage.accounts) throw new Error();
    const batches = data.batches.map((batch) => {
      if (!object(batch) || !Array.isArray(batch.users) || batch.users.length < 1 || batch.users.length > BATCH_SIZE
        || !batch.users.every((name) => typeof name === 'string' && USER.test(name))
        || typeof batch.cursor !== 'string' || batch.cursor.length > MAX_PROVIDER_CURSOR || !batch.cursor) throw new Error();
      return { users: batch.users, cursor: batch.cursor };
    });
    return { batches, coverage: {
      accounts: data.coverage.accounts,
      followingHasNextPage: data.coverage.followingHasNextPage,
      followingNextCursor: typeof data.coverage.followingNextCursor === 'string' ? data.coverage.followingNextCursor : null,
      protectedAccountsExcluded: data.coverage.protectedAccountsExcluded,
      protectionStatusUnknownAccounts: data.coverage.protectionStatusUnknownAccounts,
      followingPage: data.coverage.followingPage === 'continuation' ? 'continuation' : 'first',
      followingContinuationAvailable: data.coverage.followingContinuationAvailable === true,
      followingContinuationUnavailableReason: data.coverage.followingContinuationUnavailableReason === 'cursor_too_long' ? 'cursor_too_long' : null,
    } };
  } catch { fail('INVALID_CURSOR', 'フィードのcursorが不正、または別アカウントのものです。'); }
}

function searchTime(value) {
  if (value === undefined) return null;
  const match = typeof value === 'string' && /^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2}):(\d{2})(Z|([+-])(\d{2}):(\d{2}))$/.exec(value);
  if (!match) fail('INVALID_SEARCH', '日時はタイムゾーン付きISO 8601の秒精度で指定してください。');
  const [, year, month, day, hour, minute, second, zone, sign, offsetHour, offsetMinute] = match;
  const parts = [year, month, day, hour, minute, second].map(Number);
  const date = new Date(0);
  date.setUTCFullYear(parts[0], parts[1] - 1, parts[2]);
  date.setUTCHours(parts[3], parts[4], parts[5], 0);
  if (date.getUTCFullYear() !== parts[0] || date.getUTCMonth() + 1 !== parts[1] || date.getUTCDate() !== parts[2]
    || date.getUTCHours() !== parts[3] || date.getUTCMinutes() !== parts[4] || date.getUTCSeconds() !== parts[5]
    || (zone !== 'Z' && (Number(offsetHour) > 23 || Number(offsetMinute) > 59))) {
    fail('INVALID_SEARCH', '検索日時が不正です。存在する日付と時刻を指定してください。');
  }
  const offset = zone === 'Z' ? 0 : (Number(offsetHour) * 60 + Number(offsetMinute)) * (sign === '+' ? 1 : -1);
  const epoch = date.getTime() / 1000 - offset * 60;
  return { epoch, normalized: new Date(epoch * 1000).toISOString().replace('.000Z', 'Z') };
}

function searchConditions(account, { scope = 'all', keyword, startTime, endTime, maxAccounts, cursor, followingCursor }) {
  if (typeof keyword !== 'string' || !keyword.trim() || keyword.trim().length > 512
    || /["\\\p{Cc}\u2028\u2029]/u.test(keyword)) {
    fail('INVALID_SEARCH', 'キーワードは空でない512文字以内の語句にしてください。引用符・バックスラッシュ・制御文字は使用できません。');
  }
  if (!['all', 'following'].includes(scope)) fail('INVALID_SEARCH', '検索対象はallまたはfollowingを指定してください。');
  if (cursor !== undefined && followingCursor !== undefined) fail('INVALID_CURSOR', 'cursorとfollowingCursorは同時に指定できません。');
  if (scope === 'all' && (maxAccounts !== undefined || followingCursor !== undefined)) {
    fail('INVALID_SEARCH', '全体検索ではmaxAccountsとfollowingCursorを指定できません。');
  }
  if (maxAccounts !== undefined && (!Number.isInteger(maxAccounts) || maxAccounts < 20 || maxAccounts > 200)) {
    fail('INVALID_SEARCH', 'maxAccountsは20から200までの整数にしてください。');
  }
  const start = searchTime(startTime);
  const end = searchTime(endTime);
  if (start && end && start.epoch >= end.epoch) fail('INVALID_SEARCH', '開始日時は終了日時より前にしてください。');
  const conditions = {
    account: account.account, userName: account.userName.toLowerCase(), scope, keyword: keyword.trim(),
    startEpoch: start?.epoch ?? null, endEpoch: end?.epoch ?? null,
    maxAccounts: scope === 'following' ? maxAccounts ?? 100 : null,
  };
  const query = [`"${conditions.keyword}"`, ...(start ? [`since_time:${start.epoch}`] : []), ...(end ? [`until_time:${end.epoch}`] : [])].join(' ');
  return { conditions, query, startTime: start?.normalized ?? null, endTime: end?.normalized ?? null };
}

function searchCursorLimit(conditions, kind) {
  return kind === 'tweets' && conditions.scope === 'following' ? MAX_FEED_CURSOR : MAX_PROVIDER_CURSOR;
}

function encodeSearchCursor(inner, account, conditions, kind) {
  if (typeof inner !== 'string' || !inner || inner.length > searchCursorLimit(conditions, kind)) {
    return null;
  }
  return encodeSignedCursor({ version: 1, kind, conditions, inner }, account.apiKey, SEARCH_CURSOR_PREFIX, MAX_SEARCH_CURSOR);
}

function decodeSearchCursor(cursor, account, conditions, kind) {
  try {
    const data = decodeSignedCursor(cursor, account.apiKey, SEARCH_CURSOR_PREFIX, MAX_SEARCH_CURSOR);
    if (data.version !== 1 || data.kind !== kind || JSON.stringify(data.conditions) !== JSON.stringify(conditions)
      || typeof data.inner !== 'string' || !data.inner || data.inner.length > searchCursorLimit(conditions, kind)) throw new Error();
    return data.inner;
  } catch { fail('INVALID_CURSOR', '検索のcursorが不正、または検索条件・アカウントが一致しません。'); }
}

export class TwitterApiClient {
  constructor({ apiBaseUrl = 'https://api.twitterapi.io', timeoutMs = 45_000 } = {}) {
    this.baseUrl = new URL(apiBaseUrl);
    if (this.baseUrl.origin !== 'https://api.twitterapi.io' &&
      !(this.baseUrl.protocol === 'http:' && ['127.0.0.1', '[::1]'].includes(this.baseUrl.hostname))) {
      throw new Error('Unsupported API origin');
    }
    this.timeoutMs = timeoutMs;
  }

  async request(account, endpoint, { query, body, form, signal, timeoutMs = this.timeoutMs } = {}) {
    const url = new URL(`/twitter/${endpoint}`, this.baseUrl);
    for (const [key, value] of Object.entries(query ?? {})) if (value !== undefined && value !== null) url.searchParams.set(key, String(value));
    let response;
    try {
      response = await fetch(url, {
        method: body || form ? 'POST' : 'GET',
        headers: { 'X-API-Key': account.apiKey, ...(body ? { 'Content-Type': 'application/json' } : {}) },
        body: form ?? (body ? JSON.stringify(body) : undefined),
        redirect: 'error',
        signal: AbortSignal.any([AbortSignal.timeout(timeoutMs), ...(signal ? [signal] : [])]),
      });
      if (!response.ok) {
        // エラー本文は読まず、再試行・次の処理の前に接続資源を解放する。
        await response.body?.cancel().catch(() => {});
        fail('API_HTTP_ERROR', 'APIが要求を受け付けませんでした。', { httpStatus: response.status });
      }
      const raw = await responseText(response);
      let result;
      try { result = JSON.parse(raw); } catch { fail('INVALID_RESPONSE', 'APIからJSON応答を確認できません。'); }
      if (!object(result)) fail('INVALID_RESPONSE', 'API応答の形式が不正です。');
      if (result.status === 'error' || result.status === 'failed' || result.success === false) {
        fail('API_REJECTED', 'APIが処理の失敗を返しました。');
      }
      return result;
    } catch (error) {
      if (error instanceof PlumivoxError) {
        if (endpoint === 'create_tweet_v2' && (error.code === 'INVALID_RESPONSE'
          || (error.code === 'API_HTTP_ERROR' && error.details.httpStatus >= 500))) {
          fail('POST_UNCERTAIN', '投稿の成否は未確定です。投稿済みの可能性があります。自動再送はしていません。', error.details);
        }
        throw error;
      }
      if (endpoint === 'create_tweet_v2') fail('POST_UNCERTAIN', '投稿の成否は未確定です。投稿済みの可能性があります。自動再送はしていません。');
      fail('API_UNAVAILABLE', 'API通信が完了しませんでした。自動再試行はしていません。');
    }
  }

  async getMyPosts(account, { cursor, includeReplies = false, signal } = {}) {
    const info = await this.request(account, 'user/info', { query: { userName: account.userName }, signal });
    if (!numericId(info.data?.id)) fail('INVALID_RESPONSE', 'アカウントIDを確認できません。');
    const result = await this.request(account, 'user/tweet_timeline', { query: {
      userId: info.data.id, includeReplies, includeParentTweet: false, cursor,
    }, signal });
    // タイムライン実応答はdata.tweetsの場合もある。ページ情報はトップを使う。
    const timeline = result.tweets === undefined && object(result.data)
      ? { ...result, tweets: result.data.tweets } : result;
    return { account: account.account, ...tweetPage(timeline) };
  }

  async loginAccount(credentials, signal) {
    const response = await this.request(credentials, 'user_login_v2', { body: {
      user_name: credentials.userName,
      email: credentials.email,
      password: credentials.password,
      proxy: credentials.proxy,
      ...(credentials.totpSecret ? { totp_secret: credentials.totpSecret } : {}),
    }, signal });
    const cookie = response.login_cookie ?? response.login_cookies;
    if (response.status !== 'success' || typeof cookie !== 'string' || !cookie.trim() || cookie.length > 1024 * 1024) {
      fail('LOGIN_FAILED', 'ログインCookieを確認できません。保存セッションは変更していません。');
    }
    return cookie;
  }

  async getTweets(account, tweetIds, signal) {
    const result = await this.request(account, 'tweets', { query: { tweet_ids: [...new Set(tweetIds)].join(',') }, signal });
    if (!Array.isArray(result.tweets)) fail('INVALID_RESPONSE', '投稿一覧を確認できません。');
    return { account: account.account, tweets: result.tweets.map(normalizeTweet) };
  }

  async getFollowingFeed(account, { maxAccounts = 100, cursor, followingCursor, signal, searchQuery } = {}) {
    if (cursor && followingCursor) fail('INVALID_CURSOR', 'cursorとfollowingCursorは同時に指定できません。');
    if (followingCursor?.startsWith(SEARCH_CURSOR_PREFIX)) fail('INVALID_CURSOR', '検索用のfollowingCursorはsearch_tweetsへ渡してください。');
    let batches;
    let coverage;
    let apiRequests = 0;
    if (cursor) ({ batches, coverage } = decodeFeedCursor(cursor, account));
    else {
      const result = await this.request(account, 'user/followings', { query: { userName: account.userName, pageSize: maxAccounts, cursor: followingCursor }, signal });
      apiRequests++;
      if (!Array.isArray(result.followings)) fail('INVALID_RESPONSE', 'フォロー先一覧を確認できません。');
      let protectedAccountsExcluded = 0;
      const users = [];
      const selectedUsers = new Set();
      const unknownUsers = new Set();
      for (const user of result.followings) {
        if (user.isProtected === true || user.protected === true) { protectedAccountsExcluded++; continue; }
        if (!USER.test(user.userName ?? '')) continue;
        const name = user.userName.toLowerCase();
        if (users.length < maxAccounts && !selectedUsers.has(name)) {
          selectedUsers.add(name);
          users.push(name);
          unknownUsers.add(name);
        }
        // 上限の後ろにある同名の公開情報も、選択済みアカウントの未知数へ反映する。
        if (user.isProtected === false || user.protected === false) unknownUsers.delete(name);
      }
      // 未知数の母数は、重複除去と上限適用後に実際に検索するアカウント。
      const protectionStatusUnknownAccounts = unknownUsers.size;
      const followingHasNextPage = result.has_next_page === true || result.followings.length > maxAccounts;
      const rawFollowingCursor = typeof result.next_cursor === 'string' && result.next_cursor ? result.next_cursor : null;
      const followingCursorTooLong = rawFollowingCursor !== null && rawFollowingCursor.length > MAX_PROVIDER_CURSOR;
      const followingNextCursor = followingCursorTooLong ? null : rawFollowingCursor;
      coverage = {
        accounts: users.length,
        followingHasNextPage, followingNextCursor,
        protectedAccountsExcluded,
        protectionStatusUnknownAccounts,
        followingPage: followingCursor ? 'continuation' : 'first',
        followingContinuationAvailable: followingHasNextPage && followingNextCursor !== null,
        followingContinuationUnavailableReason: followingCursorTooLong ? 'cursor_too_long' : null,
      };
      batches = [];
      for (let offset = 0; offset < users.length; offset += BATCH_SIZE) batches.push({ users: users.slice(offset, offset + BATCH_SIZE), cursor: null });
    }
    const pending = [];
    const uniqueTweets = new Map();
    let tweetHasNextPage = false;
    let tweetCursorTooLong = false;
    // 各バッチを順番に読み、1回のツール呼び出しで最大11回のAPI要求に制限する。
    for (const batch of batches) {
      const query = `(${batch.users.map((name) => `from:${name}`).join(' OR ')}) include:nativeretweets${searchQuery ? ` ${searchQuery}` : ''}`;
      const page = tweetPage(await this.request(account, 'tweet/advanced_search', { query: { query, queryType: 'Latest', cursor: batch.cursor }, signal }), MAX_PROVIDER_CURSOR);
      apiRequests++;
      for (const tweet of page.tweets) uniqueTweets.set(tweet.id, tweet);
      tweetHasNextPage ||= page.hasNextPage;
      tweetCursorTooLong ||= page.coverage?.tweetContinuationUnavailableReason === 'cursor_too_long';
      if (page.hasNextPage && page.nextCursor) pending.push({ users: batch.users, cursor: page.nextCursor });
    }
    const tweets = [...uniqueTweets.values()].map((tweet) => ({ tweet, time: Date.parse(tweet.createdAt ?? '') || 0 }))
      .sort((a, b) => (b.time - a.time) || (BigInt(a.tweet.id) < BigInt(b.tweet.id) ? 1 : -1))
      .map(({ tweet }) => tweet);
    // 1バッチでも安全に続けられない場合は、欠けた継続を全体の継続として返さない。
    const nextCursor = pending.length && !tweetCursorTooLong ? encodeFeedCursor(pending, coverage, account) : null;
    if (pending.length && !nextCursor) tweetCursorTooLong = true;
    return {
      account: account.account,
      mode: 'reconstructed_following_feed',
      officialHomeEquivalent: false,
      tweets,
      hasNextPage: tweetHasNextPage,
      nextCursor,
      coverage: { ...coverage, protectedTweetsAvailable: false, completeFollowingList: !coverage.followingHasNextPage && coverage.followingPage !== 'continuation',
        globalTimelineOrderGuaranteed: false, tweetContinuationAvailable: nextCursor !== null,
        tweetContinuationUnavailableReason: tweetCursorTooLong ? 'cursor_too_long' : null },
      apiRequests,
    };
  }

  async searchTweets(account, input = {}) {
    const { conditions, query, startTime, endTime } = searchConditions(account, input);
    const { scope, keyword, maxAccounts } = conditions;
    const cursor = input.cursor === undefined ? undefined : decodeSearchCursor(input.cursor, account, conditions, 'tweets');
    const followingCursor = input.followingCursor === undefined ? undefined : decodeSearchCursor(input.followingCursor, account, conditions, 'following');
    let result;
    if (scope === 'all') {
      const page = tweetPage(await this.request(account, 'tweet/advanced_search', { query: { query, queryType: 'Latest', cursor }, signal: input.signal }), MAX_PROVIDER_CURSOR);
      result = { ...page, apiRequests: 1, coverage: {
        publicSearch: true, protectedTweetsAvailable: false, completeSearch: false, globalTimelineOrderGuaranteed: false,
        tweetContinuationAvailable: page.nextCursor !== null,
        tweetContinuationUnavailableReason: page.coverage?.tweetContinuationUnavailableReason ?? null,
      } };
    } else {
      result = await this.getFollowingFeed(account, { maxAccounts, cursor, followingCursor, searchQuery: query, signal: input.signal });
      const nextFollowing = result.coverage.followingNextCursor;
      if (result.coverage.followingHasNextPage && !nextFollowing && !result.coverage.followingContinuationUnavailableReason) {
        fail('INVALID_RESPONSE', 'フォロー一覧の続き情報が不足しています。');
      }
      const nextFollowingCursor = nextFollowing ? encodeSearchCursor(nextFollowing, account, conditions, 'following') : null;
      result.coverage = { ...result.coverage, followingNextCursor: nextFollowingCursor,
        followingContinuationAvailable: result.coverage.followingHasNextPage && nextFollowingCursor !== null,
        followingContinuationUnavailableReason: result.coverage.followingContinuationUnavailableReason
          ?? (nextFollowing && !nextFollowingCursor ? 'cursor_too_long' : null) };
    }
    const nextCursor = result.nextCursor ? encodeSearchCursor(result.nextCursor, account, conditions, 'tweets') : null;
    const coverage = { ...result.coverage, tweetContinuationAvailable: nextCursor !== null,
      tweetContinuationUnavailableReason: result.coverage.tweetContinuationUnavailableReason
        ?? (result.nextCursor && !nextCursor ? 'cursor_too_long' : null) };
    return {
      account: account.account, scope, keyword, startTime, endTime,
      tweets: result.tweets, hasNextPage: result.hasNextPage,
      nextCursor,
      apiRequests: result.apiRequests, coverage,
    };
  }

  async postTweet(account, { text, images = [], signal, beforeSend }) {
    let stage = 'input_validation';
    let postAttempted = false;
    let uploadAttempts = 0;
    try {
      const parsed = parseTweetText(text);
      if (!text.trim() || !parsed.valid) fail('INVALID_TEXT', '投稿文は空でなく、Xの加重文字数280以内にしてください。');
      // 全ファイルの事前確認を終えてからアップロードを開始する。
      const files = await Promise.all(images.map(imageFile));
      if (signal?.aborted) fail('CANCELLED_BEFORE_POST', '処理は中止されました。投稿は送信していません。');
      stage = 'pre_send';
      if (beforeSend) await beforeSend();
      const mediaIds = [];
      stage = 'image_upload';
      for (const file of files) {
        if (signal?.aborted) fail('CANCELLED_BEFORE_POST', '処理は中止されました。投稿は送信していません。');
        const form = new FormData();
        form.set('login_cookies', account.loginCookies);
        form.set('proxy', account.proxy);
        const blob = new Blob([file.bytes], { type: file.type });
        file.bytes = null;
        form.set('file', blob, file.name);
        // 各画像の全要求と待機を従来の一要求分の時間枠に収める。
        const deadline = performance.now() + this.timeoutMs;
        let result;
        for (let attempt = 1; attempt <= 2; attempt++) {
          if (signal?.aborted) fail('CANCELLED_BEFORE_POST', '処理は中止されました。投稿は送信していません。');
          const remaining = Math.floor(deadline - performance.now());
          if (remaining <= 0) fail('API_UNAVAILABLE', 'API通信が完了しませんでした。');
          uploadAttempts++;
          try {
            result = await this.request(account, 'upload_media_v2', { form, signal, timeoutMs: remaining });
            break;
          } catch (error) {
            if (signal?.aborted) fail('CANCELLED_BEFORE_POST', '処理は中止されました。投稿は送信していません。');
            if (attempt !== 1 || !(error instanceof PlumivoxError) || error.code !== 'API_HTTP_ERROR'
              || ![502, 503, 504].includes(error.details.httpStatus) || deadline - performance.now() <= 250) throw error;
            try { await delay(250, undefined, { signal }); }
            catch { fail('CANCELLED_BEFORE_POST', '処理は中止されました。投稿は送信していません。'); }
          }
        }
        if (result.status !== 'success' || !numericId(result.media_id)) fail('UPLOAD_FAILED', '画像アップロードの成功を確認できません。投稿は送信していません。');
        mediaIds.push(result.media_id);
      }
      if (signal?.aborted) fail('CANCELLED_BEFORE_POST', '処理は中止されました。投稿は送信していません。');
      stage = 'post_creation';
      postAttempted = true;
      const result = await this.request(account, 'create_tweet_v2', { body: {
        login_cookies: account.loginCookies, proxy: account.proxy, tweet_text: text,
        ...(mediaIds.length ? { media_ids: mediaIds } : {}),
      }, signal });
      if (result.status !== 'success' || !numericId(result.tweet_id)) {
        fail('POST_UNCERTAIN', '投稿IDを確認できないため成否は未確定です。自動再送はしていません。');
      }
      return { account: account.account, tweetId: result.tweet_id, url: `https://x.com/i/status/${result.tweet_id}`, weightedLength: parsed.weightedLength };
    } catch (error) {
      if (error instanceof PlumivoxError) {
        if (stage === 'image_upload' && error.code === 'API_UNAVAILABLE') error.message = '画像アップロードの通信が完了しませんでした。投稿は送信していません。';
        error.details = { ...error.details, stage, postAttempted, uploadAttempts };
      }
      throw error;
    }
  }
}
