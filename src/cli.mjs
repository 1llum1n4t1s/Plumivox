#!/usr/bin/env node
import { StdioServerTransport } from '@modelcontextprotocol/sdk/server/stdio.js';
import { createAccountStore } from './config.mjs';
import { createServer } from './server.mjs';
import { fileURLToPath } from 'node:url';
import { createSharedProxyManager, runProxyDaemon } from './proxy-shared.mjs';

const args = process.argv.slice(2);
if (args.length === 1 && args[0] === '--proxy-daemon') {
  try {
    const chunks = [];
    let size = 0;
    for await (const chunk of process.stdin) {
      size += chunk.length;
      if (size > 8192) throw new Error();
      chunks.push(chunk);
    }
    const bootstrap = JSON.parse(Buffer.concat(chunks).toString('utf8'));
    await runProxyDaemon(bootstrap);
  } catch {
    process.exitCode = 1;
  }
} else if (args.length && !(args.length === 1 && args[0] === '--stdio')) {
  console.error('使い方: plumivox [--stdio]');
  process.exitCode = 1;
} else {
  const proxyManager = createSharedProxyManager({ daemonEntry: fileURLToPath(import.meta.url) });
  const server = createServer({ accounts: createAccountStore(), proxyManager });
  process.stdin.once('end', () => { void proxyManager.close(); });
  for (const signal of ['SIGINT', 'SIGTERM']) {
    process.once(signal, () => { void Promise.allSettled([server.close(), proxyManager.close()]); });
  }
  try {
    await server.connect(new StdioServerTransport());
  } catch {
    await proxyManager.close();
    console.error('PlumivoxのMCP通信を開始できませんでした。');
    process.exitCode = 1;
  }
}
