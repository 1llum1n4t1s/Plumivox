import { StdioServerTransport } from '@modelcontextprotocol/sdk/server/stdio.js';
import { createAccountStore } from '../src/config.mjs';
import { createServer } from '../src/server.mjs';

// テスト専用入口。通常のCLIにはAPI接続先の上書きを持ち込まない。
const [apiBaseUrl, secretsPath, stateDirectory, timeoutArgument = '250', profileArgument] = process.argv.slice(2);
const fixtureMode = ['--proxy-config-change', '--unknown-proxy-error'].includes(profileArgument) ? profileArgument : undefined;
const slowImageUpload = profileArgument === '--slow-image-upload';
const profile = fixtureMode || slowImageUpload ? undefined : profileArgument;
const url = new URL(apiBaseUrl);
if (url.protocol !== 'http:' || url.hostname !== '127.0.0.1' || !url.port || url.username || url.password || url.pathname !== '/' || url.search || url.hash) {
  throw new Error('Fixture requires an explicit loopback HTTP endpoint');
}
if (!secretsPath || !stateDirectory) throw new Error('Fixture requires synthetic configuration and isolated session state');
const timeoutMs = Number(timeoutArgument);
if (!Number.isInteger(timeoutMs) || timeoutMs < 250 || timeoutMs > 15000) throw new Error('Invalid fixture API timeout');
const accounts = createAccountStore({ secretsPath, stateDirectory, profile, sessionPaths: {} });
if (fixtureMode) {
  const getProxyConfiguration = accounts.getProxyConfiguration;
  let checks = 0;
  // 実際のensureProxyの準備後照合を通す。差し替えは偽設定を読むE2E入口だけに限定する。
  accounts.getProxyConfiguration = async (...args) => {
    const config = await getProxyConfiguration(...args);
    if (fixtureMode === '--unknown-proxy-error') throw new Error('Synthetic unexpected proxy error');
    return ++checks === 2 ? { ...config, fixtureChanged: true } : config;
  };
}
await createServer({ accounts, apiBaseUrl, timeoutMs, imageUploadTimeoutMs: slowImageUpload ? 1500 : undefined }).connect(new StdioServerTransport());
