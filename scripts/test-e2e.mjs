import path from 'node:path';
import { cargo, root, run } from './native.mjs';

if (process.env.PLUMIVOX_E2E_CASE) throw new Error('全件検証ではPLUMIVOX_E2E_CASEを未設定にしてください');
const target = path.resolve(process.env.CARGO_TARGET_DIR || path.join(root, 'target'));
const executable = name => path.join(target, 'debug', `${name}${process.platform === 'win32' ? '.exe' : ''}`);
const env = { ...process.env, PLUMIVOX_NATIVE_FIXTURE: executable('plumivox-fixture'), PLUMIVOX_NATIVE_BINARY: executable('plumivox') };
const test = file => run(process.execPath, [file], { env });

// 接続先差し替えと故障注入を含む実行ファイルは配布用と別ターゲット。
run(cargo, ['build', '--locked', '--features', 'test-fixture', '--bin', 'plumivox-fixture', '--bin', 'plumivox']);
run(cargo, ['test', '--locked', '--lib', ...(process.platform === 'darwin' ? ['--features', 'test-fixture'] : [])]);
test('tests/native-e2e.mjs');
test('tests/e2e.mjs');
test('tests/native-proxy-e2e.mjs');
if (process.platform === 'win32') test('tests/setup-e2e.mjs');
if (process.platform === 'darwin') test('tests/native-setup-e2e.mjs');
test('tests/setup-vps-e2e.mjs');
