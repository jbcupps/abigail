// Exercises crash cleanup using copied Node executables, never app/user data.
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { copyFileSync, mkdirSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { randomUUID } from 'node:crypto';
import { setTimeout as delay } from 'node:timers/promises';
import { stopIsolatedWindowsDaemons } from './mvp-owned-processes.mjs';

assert.equal(process.platform, 'win32', 'This cleanup probe uses Windows process handles');
const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const runDir = join(repoRoot, 'target/manual-test/mvp-acceptance', 'cleanup-probe-' + randomUUID().slice(0, 8));
const binaryDir = join(runDir, 'bin');
const dataDir = join(runDir, 'owned data');
const otherDataDir = join(runDir, 'other data');
for (const directory of [binaryDir, dataDir, otherDataDir]) mkdirSync(directory, { recursive: true });
for (const filename of ['hive-daemon.exe', 'entity-daemon.exe']) copyFileSync(process.execPath, join(binaryDir, filename));
const loopPath = join(runDir, 'loop.mjs');
writeFileSync(loopPath, 'setInterval(() => {}, 1000);\n');
const parentPath = join(runDir, 'parent.mjs');
writeFileSync(parentPath, `import { spawn } from 'node:child_process';
const child = spawn(process.argv[2], [process.argv[3], ...process.argv.slice(4)], { detached: true, windowsHide: true, stdio: 'ignore' });
child.unref(); console.log(child.pid);
`);
const spectator = spawn(join(binaryDir, 'entity-daemon.exe'), [loopPath, '--data-dir', otherDataDir], { windowsHide: true, stdio: 'ignore' });
const parent = spawn(join(binaryDir, 'hive-daemon.exe'), [parentPath, join(binaryDir, 'entity-daemon.exe'), loopPath, '--data-dir', dataDir], { windowsHide: true, stdio: ['ignore', 'pipe', 'pipe'] });
let output = ''; let stderr = ''; let result;
parent.stdout.on('data', chunk => { output += chunk; });
parent.stderr.on('data', chunk => { stderr += chunk; });
try {
  const exit = await new Promise((resolvePromise, reject) => { parent.once('error', reject); parent.once('exit', resolvePromise); });
  assert.equal(exit, 0, stderr); const orphanPid = Number(output.trim()); assert(orphanPid > 0);
  await delay(300);
  const stopped = await stopIsolatedWindowsDaemons({ binaryDir, dataDir });
  assert(stopped.some(process => process.pid === orphanPid), 'Orphaned child was not found by exact scope');
  assert.equal(spectator.exitCode, null, 'Different-data-directory daemon was stopped');
  result = { passed: true, orphan_pid: orphanPid, stopped, other_scope_preserved: true };
} catch (error) { result = { passed: false, error: error.stack }; process.exitCode = 1; }
finally {
  for (const scope of [dataDir, otherDataDir]) {
    try { await stopIsolatedWindowsDaemons({ binaryDir, dataDir: scope }); }
    catch (error) { result.cleanup_error = error.message; result.passed = false; process.exitCode = 1; }
  }
  writeFileSync(join(runDir, 'result.json'), JSON.stringify(result, null, 2));
  console.log(`Orphan cleanup probe: ${result.passed ? 'PASS' : 'FAIL'}; ${join(runDir, 'result.json')}`);
}
