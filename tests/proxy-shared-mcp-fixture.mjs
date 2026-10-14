import { spawn } from 'node:child_process';
import net from 'node:net';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { StdioServerTransport } from '@modelcontextprotocol/sdk/server/stdio.js';
import { createAccountStore } from '../src/config.mjs';
import { createServer } from '../src/server.mjs';
import { createSharedProxyManager } from '../src/proxy-shared.mjs';

const settingsPath = process.argv[2];
const settings = JSON.parse(await readFile(settingsPath, 'utf8'));
const proxyManager = createSharedProxyManager({
  daemonEntry: path.join(path.dirname(fileURLToPath(import.meta.url)), 'proxy-shared-daemon-fixture.mjs'),
  daemonIdleMs: 450, ipcTimeoutMs: 2000, startupTimeoutMs: 4000,
  pollDelayMs: 40, maxPollAttempts: 30, runtimeDirectory: settings.runtimeDirectory,
  connect: options => net.connect(settings.attackPipe ? { path: settings.attackPipe } : options),
  spawnDaemon: async ({ daemonEntry, config, socketPath, idleMs, runtimeDirectory }) => {
    await mkdir(runtimeDirectory, { recursive: true });
    await writeFile(settings.ipcPathFile, JSON.stringify({ socketPath }));
    const child = spawn(process.execPath, [daemonEntry, settingsPath], { cwd: runtimeDirectory,
      detached: true, windowsHide: true, stdio: ['pipe', 'ignore', 'ignore'] });
    await new Promise((resolve, reject) => {
      child.once('error', reject); child.stdin.on('error', () => {});
      child.once('spawn', () => child.stdin.end(JSON.stringify({ config, socketPath, idleMs }) + '\n', () => { child.unref(); resolve(); }));
    });
    return child;
  },
});
const accounts = createAccountStore({ secretsPath: settings.secretsPath, stateDirectory: settings.stateDirectory });
const server = createServer({ accounts, apiBaseUrl: settings.apiBaseUrl, proxyManager });
server.server.onclose = () => { void proxyManager.close(); };
const shutdown = async () => { await proxyManager.close(); await server.close(); };
process.on('SIGTERM', () => { void shutdown(); });
process.on('SIGINT', () => { void shutdown(); });
process.stdin.on('end', () => { void shutdown(); });
await server.connect(new StdioServerTransport());
