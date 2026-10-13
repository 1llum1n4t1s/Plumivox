import { StdioServerTransport } from '@modelcontextprotocol/sdk/server/stdio.js';
import { createAccountStore } from '../src/config.mjs';
import { createServer } from '../src/server.mjs';

// テスト専用入口。通常のCLIにはAPI接続先の上書きを持ち込まない。
const [apiBaseUrl, secretsPath, stateDirectory, timeoutArgument = '250'] = process.argv.slice(2);
const url = new URL(apiBaseUrl);
if (url.protocol !== 'http:' || url.hostname !== '127.0.0.1' || !url.port || url.username || url.password || url.pathname !== '/' || url.search || url.hash) {
  throw new Error('Fixture requires an explicit loopback HTTP endpoint');
}
if (!secretsPath || !stateDirectory) throw new Error('Fixture requires synthetic configuration and isolated session state');
const timeoutMs = Number(timeoutArgument);
if (!Number.isInteger(timeoutMs) || timeoutMs < 250 || timeoutMs > 15000) throw new Error('Invalid fixture API timeout');
await createServer({ accounts: createAccountStore({ secretsPath, stateDirectory, sessionPaths: {} }), apiBaseUrl, timeoutMs }).connect(new StdioServerTransport());
