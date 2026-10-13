#!/usr/bin/env node
import { StdioServerTransport } from '@modelcontextprotocol/sdk/server/stdio.js';
import { createAccountStore } from './config.mjs';
import { createServer } from './server.mjs';

const args = process.argv.slice(2);
if (args.length && !(args.length === 1 && args[0] === '--stdio')) {
  console.error('使い方: plumivox [--stdio]');
  process.exitCode = 1;
} else {
  const server = createServer({ accounts: createAccountStore() });
  try {
    await server.connect(new StdioServerTransport());
  } catch {
    console.error('PlumivoxのMCP通信を開始できませんでした。');
    process.exitCode = 1;
  }
}
