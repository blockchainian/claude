import { execFileSync } from 'node:child_process';
import { parentPort, workerData } from 'node:worker_threads';

// Keep the syntax deadline and stdin delivery off the caller's event loop.
try {
  execFileSync(process.execPath, ['--input-type=module', '--check'], {
    input: workerData, stdio: ['pipe', 'pipe', 'pipe'], timeout: 10000,
  });
  parentPort.postMessage({});
} catch (error) {
  parentPort.postMessage({ error: {
    message: error.message, code: error.code, status: error.status,
    signal: error.signal, stderr: error.stderr?.toString(),
  } });
}
parentPort.close();
