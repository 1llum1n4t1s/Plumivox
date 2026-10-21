import { open } from 'node:fs/promises';
import { isAbsolute } from 'node:path';

const MAX_KEY_BYTES = 1024 * 1024;

/** 表記やパスの正規化は呼び出し側に残し、共通の設定契約だけを検証する。 */
export function normalizeProxyConfig(config) {
  const port = value => Number.isInteger(value) && value >= 1024 && value <= 65535;
  if (!config || typeof config !== 'object' ||
    typeof config.sshHost !== 'string' || !config.sshHost || config.sshHost.length > 253 || /[\s\x00-\x1f]/.test(config.sshHost) ||
    typeof config.sshUser !== 'string' || !/^[A-Za-z0-9._-]{1,64}$/.test(config.sshUser) ||
    !Number.isInteger(config.sshPort) || config.sshPort < 1 || config.sshPort > 65535 ||
    !port(config.localPort) || !port(config.remotePort) ||
    typeof config.privateKeyFile !== 'string' || !isAbsolute(config.privateKeyFile) ||
    typeof config.hostKeySha256 !== 'string' || !/^SHA256:[A-Za-z0-9+/]{43}=?$/.test(config.hostKeySha256)) return null;
  const fingerprint = config.hostKeySha256.slice(7).replace(/=$/, '');
  const bytes = Buffer.from(fingerprint, 'base64');
  if (bytes.length !== 32 || bytes.toString('base64').replace(/=$/, '') !== fingerprint) return null;
  return {
    sshHost: config.sshHost, sshPort: config.sshPort, sshUser: config.sshUser,
    privateKeyFile: config.privateKeyFile, hostKeySha256: `SHA256:${fingerprint}`,
    localPort: config.localPort, remotePort: config.remotePort,
  };
}

/** consumeは同期処理に限る。借用する鍵バッファーは処理直後に消去する。 */
export async function withProxyPrivateKey(path, ErrorType, consume) {
  let file;
  let bytes;
  try {
    file = await open(path, 'r');
    const stat = await file.stat();
    if (!stat.isFile() || stat.size < 1) throw new ErrorType('PROXY_KEY_UNAVAILABLE');
    if (stat.size > MAX_KEY_BYTES) throw new ErrorType('PROXY_KEY_TOO_LARGE');
    // stat後に大きくなったファイルも上限+1バイトまでしか読まない。
    bytes = Buffer.alloc(MAX_KEY_BYTES + 1);
    let length = 0;
    while (length < bytes.length) {
      const result = await file.read(bytes, length, bytes.length - length, length);
      if (!result.bytesRead) break;
      length += result.bytesRead;
    }
    if (length > MAX_KEY_BYTES) throw new ErrorType('PROXY_KEY_TOO_LARGE');
    if (!length) throw new ErrorType('PROXY_KEY_UNAVAILABLE');
    return consume(bytes.subarray(0, length));
  } catch (error) {
    throw error instanceof ErrorType ? error : new ErrorType('PROXY_KEY_UNAVAILABLE');
  } finally {
    bytes?.fill(0);
    await file?.close().catch(() => {});
  }
}
