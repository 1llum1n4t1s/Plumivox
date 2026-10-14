import net from 'node:net';
import { readFile, appendFile } from 'node:fs/promises';
import { StdioServerTransport } from '@modelcontextprotocol/sdk/server/stdio.js';
import { createAccountStore } from '../src/config.mjs';
import { createServer } from '../src/server.mjs';
import { createManagedProxyManager } from '../src/proxy-manager.mjs';
import { createEgressServer } from '../src/proxy-egress.mjs';

// 差し替えはE2E専用起動点に限定し、本番の環境変数から呼び出さない。
const settings = JSON.parse(await readFile(process.argv[2], 'utf8'));
const record = event => appendFile(settings.egressTrace, JSON.stringify(event) + '\n');
const proxyManager = createManagedProxyManager({
  readyTimeoutMs: 2000,
  reconnectDelayMs: 80,
  egressFactory: options => createEgressServer({ ...options,
    lookup: async (host, options) => {
      await record({ type: 'lookup', all: options.all, verbatim: options.verbatim });
      if (host === 'private.x.com') return [{ address: '127.0.0.1', family: 4 }];
      if (host === 'mixed.x.com') return [{ address: '93.184.216.34', family: 4 }, { address: '10.0.0.1', family: 4 }];
      if (host === 'slow.x.com') {
        await new Promise(resolve => setTimeout(resolve, 350));
        return [{ address: '93.184.216.34', family: 4 }];
      }
      return [{ address: '93.184.216.34', family: 4 }];
    },
    connect: options => {
      void record({ type: 'connect', host: options.host, family: options.family, port: options.port });
      return net.connect({ host: '127.0.0.1', port: settings.echoPort });
    },
  }),
});
const accounts = createAccountStore({ secretsPath: settings.secretsPath, stateDirectory: settings.stateDirectory });
const server = createServer({ accounts, apiBaseUrl: settings.apiBaseUrl, proxyManager });
server.server.onclose = () => { void proxyManager.close(); };
const shutdown = async () => { await proxyManager.close(); await server.close(); };
process.on('SIGTERM', () => { void shutdown(); });
process.on('SIGINT', () => { void shutdown(); });
process.stdin.on('end', () => { void shutdown(); });
await server.connect(new StdioServerTransport());
