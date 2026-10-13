import { open } from 'node:fs/promises';
import path from 'node:path';
import twitterText from 'twitter-text';

const ID = /^\d{1,30}$/;
const USER = /^[A-Za-z0-9_]{1,15}$/;
const MAX_IMAGE = 5 * 1024 * 1024;
const BATCH_SIZE = 20;

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

function tweetPage(response) {
  if (!Array.isArray(response.tweets)) fail('INVALID_RESPONSE', '投稿一覧を確認できません。');
  const hasNextPage = response.has_next_page === true;
  const nextCursor = typeof response.next_cursor === 'string' && response.next_cursor ? response.next_cursor : null;
  if (hasNextPage && !nextCursor) fail('INVALID_RESPONSE', '続きの取得情報が不足しています。');
  return { tweets: response.tweets.map(normalizeTweet), hasNextPage, nextCursor: hasNextPage ? nextCursor : null };
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

function decodeFeedCursor(cursor, account) {
  try {
    if (cursor.length > 100_000 || !/^[A-Za-z0-9_-]+$/.test(cursor)) throw new Error();
    const data = JSON.parse(Buffer.from(cursor, 'base64url').toString('utf8'));
    if (data.version !== 1 || data.account !== account.account || data.userName !== account.userName.toLowerCase()
      || !Array.isArray(data.batches) || data.batches.length < 1 || data.batches.length > 10
      || !object(data.coverage) || !Number.isInteger(data.coverage.accounts)
      || data.coverage.accounts < 0 || data.coverage.accounts > 200
      || typeof data.coverage.followingHasNextPage !== 'boolean'
      || !Number.isInteger(data.coverage.protectedAccountsExcluded) || data.coverage.protectedAccountsExcluded < 0
      || data.coverage.protectedAccountsExcluded > 200) throw new Error();
    const batches = data.batches.map((batch) => {
      if (!object(batch) || !Array.isArray(batch.users) || batch.users.length < 1 || batch.users.length > BATCH_SIZE
        || !batch.users.every((name) => typeof name === 'string' && USER.test(name))
        || typeof batch.cursor !== 'string' || batch.cursor.length > 10_000 || !batch.cursor) throw new Error();
      return { users: batch.users, cursor: batch.cursor };
    });
    return { batches, coverage: {
      accounts: data.coverage.accounts,
      followingHasNextPage: data.coverage.followingHasNextPage,
      followingNextCursor: typeof data.coverage.followingNextCursor === 'string' ? data.coverage.followingNextCursor : null,
      protectedAccountsExcluded: data.coverage.protectedAccountsExcluded,
      followingPage: data.coverage.followingPage === 'continuation' ? 'continuation' : 'first',
    } };
  } catch { fail('INVALID_CURSOR', 'フィードのcursorが不正、または別アカウントのものです。'); }
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

  async request(account, endpoint, { query, body, form, signal } = {}) {
    const url = new URL(`/twitter/${endpoint}`, this.baseUrl);
    for (const [key, value] of Object.entries(query ?? {})) if (value !== undefined && value !== null) url.searchParams.set(key, String(value));
    let response;
    try {
      response = await fetch(url, {
        method: body || form ? 'POST' : 'GET',
        headers: { 'X-API-Key': account.apiKey, ...(body ? { 'Content-Type': 'application/json' } : {}) },
        body: form ?? (body ? JSON.stringify(body) : undefined),
        redirect: 'error',
        signal: AbortSignal.any([AbortSignal.timeout(this.timeoutMs), ...(signal ? [signal] : [])]),
      });
      if (!response.ok) fail('API_HTTP_ERROR', 'APIが要求を受け付けませんでした。', { httpStatus: response.status });
      const raw = await response.text();
      if (raw.length > 10 * 1024 * 1024) fail('INVALID_RESPONSE', 'API応答のサイズが上限を超えました。');
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

  async getFollowingFeed(account, { maxAccounts = 100, cursor, followingCursor, signal } = {}) {
    if (cursor && followingCursor) fail('INVALID_CURSOR', 'cursorとfollowingCursorは同時に指定できません。');
    let batches;
    let coverage;
    let apiRequests = 0;
    if (cursor) ({ batches, coverage } = decodeFeedCursor(cursor, account));
    else {
      const result = await this.request(account, 'user/followings', { query: { userName: account.userName, pageSize: maxAccounts, cursor: followingCursor }, signal });
      apiRequests++;
      if (!Array.isArray(result.followings)) fail('INVALID_RESPONSE', 'フォロー先一覧を確認できません。');
      const protectedAccountsExcluded = result.followings.filter((user) => user.isProtected === true || user.protected === true).length;
      const users = [...new Set(result.followings
        .filter((user) => user.isProtected !== true && user.protected !== true && USER.test(user.userName ?? ''))
        .map((user) => user.userName.toLowerCase()))].slice(0, maxAccounts);
      coverage = {
        accounts: users.length,
        followingHasNextPage: result.has_next_page === true || result.followings.length > maxAccounts,
        followingNextCursor: typeof result.next_cursor === 'string' && result.next_cursor ? result.next_cursor : null,
        protectedAccountsExcluded,
        followingPage: followingCursor ? 'continuation' : 'first',
      };
      batches = [];
      for (let offset = 0; offset < users.length; offset += BATCH_SIZE) batches.push({ users: users.slice(offset, offset + BATCH_SIZE), cursor: null });
    }
    const pending = [];
    const uniqueTweets = new Map();
    // 各バッチを順番に読み、1回のツール呼び出しで最大11回のAPI要求に制限する。
    for (const batch of batches) {
      const query = `(${batch.users.map((name) => `from:${name}`).join(' OR ')}) include:nativeretweets`;
      const page = tweetPage(await this.request(account, 'tweet/advanced_search', { query: { query, queryType: 'Latest', cursor: batch.cursor }, signal }));
      apiRequests++;
      for (const tweet of page.tweets) uniqueTweets.set(tweet.id, tweet);
      if (page.hasNextPage) pending.push({ users: batch.users, cursor: page.nextCursor });
    }
    const tweets = [...uniqueTweets.values()].sort((a, b) => {
      const time = (Date.parse(b.createdAt ?? '') || 0) - (Date.parse(a.createdAt ?? '') || 0);
      return time || (BigInt(a.id) < BigInt(b.id) ? 1 : -1);
    });
    const nextCursor = pending.length ? Buffer.from(JSON.stringify({
      version: 1, account: account.account, userName: account.userName.toLowerCase(), batches: pending, coverage,
    })).toString('base64url') : null;
    return {
      account: account.account,
      mode: 'reconstructed_following_feed',
      officialHomeEquivalent: false,
      tweets,
      hasNextPage: pending.length > 0,
      nextCursor,
      coverage: { ...coverage, protectedTweetsAvailable: false, completeFollowingList: !coverage.followingHasNextPage && coverage.followingPage !== 'continuation',
        globalTimelineOrderGuaranteed: false },
      apiRequests,
    };
  }

  async postTweet(account, { text, images = [], signal }) {
    const parsed = twitterText.parseTweet(text);
    if (!text.trim() || !parsed.valid) fail('INVALID_TEXT', '投稿文は空でなく、Xの加重文字数280以内にしてください。');
    // 全ファイルの事前確認を終えてからアップロードを開始する。
    const files = await Promise.all(images.map(imageFile));
    const mediaIds = [];
    for (const file of files) {
      if (signal?.aborted) fail('CANCELLED_BEFORE_POST', '処理は中止されました。投稿は送信していません。');
      const form = new FormData();
      form.set('login_cookies', account.loginCookies);
      form.set('proxy', account.proxy);
      form.set('file', new Blob([file.bytes], { type: file.type }), file.name);
      const result = await this.request(account, 'upload_media_v2', { form, signal });
      if (result.status !== 'success' || !numericId(result.media_id)) fail('UPLOAD_FAILED', '画像アップロードの成功を確認できません。投稿は送信していません。');
      mediaIds.push(result.media_id);
    }
    if (signal?.aborted) fail('CANCELLED_BEFORE_POST', '処理は中止されました。投稿は送信していません。');
    const result = await this.request(account, 'create_tweet_v2', { body: {
      login_cookies: account.loginCookies, proxy: account.proxy, tweet_text: text,
      ...(mediaIds.length ? { media_ids: mediaIds } : {}),
    }, signal });
    if (result.status !== 'success' || !numericId(result.tweet_id)) {
      fail('POST_UNCERTAIN', '投稿IDを確認できないため成否は未確定です。自動再送はしていません。');
    }
    return { account: account.account, tweetId: result.tweet_id, url: `https://x.com/i/status/${result.tweet_id}`, weightedLength: parsed.weightedLength };
  }
}
