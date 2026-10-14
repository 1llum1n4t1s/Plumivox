import net from 'node:net';
import { appendFileSync } from 'node:fs';
import { readFile, appendFile } from 'node:fs/promises';
import { runProxyDaemon } from '../src/proxy-shared.mjs';
import { createManagedProxyManager } from '../src/proxy-manager.mjs';
import { createEgressServer } from '../src/proxy-egress.mjs';

const settings = JSON.parse(await readFile(process.argv[2], 'utf8'));
let bootstrap = '';
for await (const chunk of process.stdin) { bootstrap += chunk; if (bootstrap.length > 8192) throw new Error('Fixture bootstrap too large'); }
const { config, socketPath, idleMs } = JSON.parse(bootstrap);
const record = event => appendFile(settings.egressTrace, JSON.stringify(event) + '\n');
let started = false;
process.once('beforeExit', () => appendFileSync(settings.daemonTrace, JSON.stringify({ event: started ? 'exit' : 'lost-race', pid: process.pid }) + '\n'));
try {
  const daemon = await runProxyDaemon({ config, socketPath, idleMs,
    managerFactory: () => createManagedProxyManager({ readyTimeoutMs: 2000, reconnectDelayMs: 80,
      egressFactory: options => createEgressServer({ ...options,
        lookup: async () => [{ address: '93.184.216.34', family: 4 }],
        connect: options => { void record({ type: 'connect', host: options.host, port: options.port });
          return net.connect({ host: '127.0.0.1', port: settings.echoPort }); },
      }),
    }),
  });
  started = true;
  await appendFile(settings.daemonTrace, JSON.stringify({ event: 'start', pid: process.pid }) + '\n');
  process.on('SIGTERM', () => { void daemon.close(); });
  process.on('SIGINT', () => { void daemon.close(); });
} catch (error) {
  if (error.code !== 'PROXY_PORT_BUSY') throw error;
}
