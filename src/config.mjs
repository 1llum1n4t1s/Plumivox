import { readFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import path from 'node:path';
import { createSessionStorage, readLegacyProtectedCookie } from './session.mjs';

const USER_NAME = /^[A-Za-z0-9_]{1,15}$/;
const DEFAULT_SECRETS = 'C:\\Users\\IMT\\dev\\Secret\\secrets.json';
const MAX_BYTES = 2 * 1024 * 1024;

/** 秘密値やファイルパスを含まない設定エラー。 */
export class ConfigurationError extends Error {
  /** @param {string} code @param {string} message */
  constructor(code, message) {
    super(message);
    this.name = 'ConfigurationError';
    this.code = code;
  }
}

const object = (value) => value !== null && typeof value === 'object' && !Array.isArray(value);
const nonempty = (value) => typeof value === 'string' && value.trim().length > 0;
const hash = (value) => createHash('sha256').update(value, 'utf8').digest('hex');
const validTotpSecret = (value) => value === undefined || value === null ||
  (typeof value === 'string' && (!value.trim() || (/^[A-Za-z2-7]+={0,6}$/.test(value) && !/^\d{6}$/.test(value))));

function validProxy(value) {
  if (!nonempty(value) || value !== value.trim() || /[\s\u0000-\u001f]/u.test(value)) return false;
  try {
    const url = new URL(value);
    return ['http:', 'https:'].includes(url.protocol) && Boolean(url.hostname) && !url.hash;
  } catch { return false; }
}

/**
 * @typedef {{account:string,userName:string,apiKey:string,proxy?:string,loginCookies?:string}} Account
 * @param {{secretsPath?:string,profile?:string,sessionPaths?:Record<string,string>,stateDirectory?:string}} [options]
 * sessionPaths はプロファイル名から既存の状態ディレクトリへの対応。
 */
export function createAccountStore({ secretsPath, profile, sessionPaths = {}, stateDirectory } = {}) {
  const source = process.env.PLUMIVOX_SECRETS_FILE || secretsPath || DEFAULT_SECRETS;
  const sessions = createSessionStorage({ stateDirectory });

  async function profiles() {
    try {
      const raw = await readFile(source, 'utf8');
      if (Buffer.byteLength(raw) > MAX_BYTES) throw new Error();
      const secrets = JSON.parse(raw);
      if (!object(secrets) || !object(secrets.twitterapi_io)) throw new Error();
      return Object.fromEntries(Object.entries(secrets.twitterapi_io).filter(([, value]) => object(value)));
    } catch {
      throw new ConfigurationError('CONFIG_UNAVAILABLE', 'アカウント設定を読み込めません。設定ファイルを確認してください。');
    }
  }

  async function select(account) {
    const configs = await profiles();
    const names = Object.keys(configs);
    const selected = account ?? process.env.PLUMIVOX_DEFAULT_ACCOUNT ?? profile ?? (names.length === 1 ? names[0] : undefined);
    if (!selected) throw new ConfigurationError('ACCOUNT_REQUIRED', '使用するアカウントを明示してください。');
    if (typeof selected !== 'string' || !Object.hasOwn(configs, selected)) {
      throw new ConfigurationError('ACCOUNT_UNKNOWN', '指定されたアカウント設定がありません。');
    }
    return { account: selected, config: configs[selected] };
  }

  const loginReady = (config, userName, apiKey, proxy) => Boolean(userName && apiKey && proxy &&
    nonempty(config.email) && nonempty(config.password) &&
    validTotpSecret(config.totp_secret));

  /** ログインに必要な項目だけを内部呼出元へ返す。MCP出力へ返さない。 */
  async function getLoginCredentials(account) {
    const selected = await select(account);
    const { config } = selected;
    const userName = typeof config.user_name === 'string' && USER_NAME.test(config.user_name) ? config.user_name : '';
    const apiKey = nonempty(config.api_key) ? config.api_key : '';
    const proxy = validProxy(config.proxy) ? config.proxy : undefined;
    if (!validTotpSecret(config.totp_secret)) {
      throw new ConfigurationError('LOGIN_CREDENTIALS_INVALID', 'totp_secretには認証コードではなくBase32の2FA秘密鍵を設定してください。');
    }
    if (!loginReady(config, userName, apiKey, proxy)) {
      throw new ConfigurationError('LOGIN_UNAVAILABLE', 'ログインにはAPIキー、ユーザー名、プロキシ、メールアドレス、パスワードを設定してください。');
    }
    return { account: selected.account, userName, apiKey, proxy, email: config.email, password: config.password,
      ...(nonempty(config.totp_secret) ? { totpSecret: config.totp_secret } : {}) };
  }

  /** @param {string} account @param {Record<string,unknown>} config */
  async function inspect(account, config) {
    const userName = typeof config.user_name === 'string' && USER_NAME.test(config.user_name) ? config.user_name : '';
    const apiKey = nonempty(config.api_key) ? config.api_key : '';
    const proxy = validProxy(config.proxy) ? config.proxy : undefined;
    const readConfigured = Boolean(userName && apiKey);
    const loginConfigured = loginReady(config, userName, apiKey, proxy);
    let loginCookies;
    let sessionSource = null;
    let sessionAcquiredAt = null;
    const own = readConfigured ? await sessions.read({ account, userName, apiKey, proxy }) : { status: 'missing' };
    if (own.status === 'valid') {
      loginCookies = own.cookie;
      sessionSource = 'plumivox_dpapi';
      sessionAcquiredAt = own.acquiredAt;
    } else if (own.status === 'missing' && readConfigured && nonempty(config.login_cookies)) {
      loginCookies = config.login_cookies;
      sessionSource = 'configuration';
    } else if (own.status === 'missing' && readConfigured && proxy) {
      const defaultFolder = { release_posting: 'TwitterApiRelease', blog_posting: 'TwitterApiBlog' };
      const folder = Object.hasOwn(defaultFolder, account) ? defaultFolder[account] : null;
      const stateDirectory = Object.hasOwn(sessionPaths, account) ? sessionPaths[account]
        : folder && process.env.LOCALAPPDATA ? path.join(process.env.LOCALAPPDATA, 'Kagayoi', folder) : null;
      if (nonempty(stateDirectory)) {
        loginCookies = await readLegacyProtectedCookie(path.join(stateDirectory, hash(userName.toLowerCase()), 'session.dpapi'),
          hash(`${apiKey}\n${proxy}`), userName) || undefined;
        if (loginCookies) sessionSource = 'windows_dpapi';
      }
    }
    return { account, userName, apiKey, proxy, loginCookies, readConfigured, loginConfigured, sessionAcquiredAt,
      postConfigured: Boolean(readConfigured && proxy && loginCookies), sessionSource };
  }

  return {
    async listAccounts() {
      const configs = await profiles();
      const accounts = [];
      for (const [account, config] of Object.entries(configs)) {
        const { userName, readConfigured, postConfigured, loginConfigured, sessionSource, sessionAcquiredAt } = await inspect(account, config);
        accounts.push({ account, userName, readConfigured, postConfigured, loginConfigured, sessionSource, sessionAcquiredAt });
      }
      return accounts;
    },
    /** @param {string} [account] @param {{requirePosting?:boolean}} [options] @returns {Promise<Account>} */
    async getAccount(account, { requirePosting = false } = {}) {
      const selected = await select(account);
      const result = await inspect(selected.account, selected.config);
      if (!result.readConfigured) {
        throw new ConfigurationError('ACCOUNT_INVALID', 'APIキーと有効なユーザー名を設定してください。');
      }
      if (requirePosting && !result.postConfigured) {
        throw new ConfigurationError('POSTING_UNAVAILABLE', '投稿には有効なプロキシと一致するv2 Cookieが必要です。');
      }
      const { account: name, userName, apiKey, proxy, loginCookies } = result;
      return { account: name, userName, apiKey, ...(proxy ? { proxy } : {}), ...(loginCookies ? { loginCookies } : {}) };
    },
    getLoginCredentials,
    async withSessionLock(account, callback) {
      const selected = await select(account);
      const userName = selected.config.user_name;
      if (typeof userName !== 'string' || !USER_NAME.test(userName) || !nonempty(selected.config.api_key)) {
        throw new ConfigurationError('ACCOUNT_INVALID', 'APIキーと有効なユーザー名を設定してください。');
      }
      const lockAccount = Object.freeze({ account: selected.account, userName });
      return sessions.withLock(lockAccount, callback);
    },
    async prepareSessionStorage(account) {
      const credentials = await getLoginCredentials(account);
      await sessions.prepare(credentials);
    },
    async saveSession(account, loginCookie, { expectedAccount, lease, signal } = {}) {
      let credentials;
      try { credentials = await getLoginCredentials(account); }
      catch (error) {
        if (!expectedAccount) throw error;
        throw new ConfigurationError('AUTH_CONFIG_CHANGED', 'ログイン中にアカウント設定が変更されました。設定を確認して再実行してください。');
      }
      if (expectedAccount && ['account', 'userName', 'apiKey', 'proxy'].some((key) => credentials[key] !== expectedAccount[key])) {
        throw new ConfigurationError('AUTH_CONFIG_CHANGED', 'ログイン中にアカウント設定が変更されました。設定を確認して再実行してください。');
      }
      return sessions.save(credentials, loginCookie, { lease, signal });
    },
  };
}
