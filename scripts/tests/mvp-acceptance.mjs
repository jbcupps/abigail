// Runs only built/installed daemons. Synthetic replies prove HTTP and storage
// contracts; they do not validate model quality or real-provider behavior.
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { createServer } from 'node:net';
import { createWriteStream, existsSync, mkdirSync, writeFileSync, appendFileSync } from 'node:fs';
import { resolve, dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { randomBytes, randomUUID } from 'node:crypto';
import { setTimeout as delay } from 'node:timers/promises';
import { startModelFixture } from './mvp-model-fixture.mjs';
import { stopIsolatedWindowsDaemons } from './mvp-owned-processes.mjs';

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const args = process.argv.slice(2);
function option(name, fallback) { const index = args.indexOf(name); return index < 0 ? fallback : args[index + 1]; }
const binaryRoot = resolve(option('--binary-dir', join(repoRoot, 'target/debug')));
const binaryDir = existsSync(join(binaryRoot, 'hive-daemon.exe')) ? binaryRoot : join(binaryRoot, 'resources');
const runId = new Date().toISOString().replace(/[:.]/g, '-') + '-' + randomUUID().slice(0, 8);
const artifactRoot = resolve(option('--output-dir', join(repoRoot, 'target/manual-test/mvp-acceptance')));
const runDir = join(artifactRoot, runId);
const dataDir = join(runDir, 'data');
const documentsDir = join(runDir, 'documents');
mkdirSync(dataDir, { recursive: true });
const timeline = join(runDir, 'timeline.jsonl');
const stages = [];
const ownedProcesses = new Set();
const environment = { ...process.env, LOCALAPPDATA: join(runDir, 'local-app-data'),
  APPDATA: join(runDir, 'roaming-app-data'), RUST_LOG: 'info', ABIGAIL_VAULT_RAW_KEY: randomBytes(32).toString('base64') };
for (const key of ['ABIGAIL_CI_MODE', 'ABIGAIL_HIVE_URL', 'ABIGAIL_DATA_DIR', 'ABIGAIL_ENTITY_DAEMON_PATH',
  'ABIGAIL_INTERNAL_BIN_DIR', 'ABIGAIL_VAULT_PASSPHRASE', 'ABIGAIL_VAULT_DATA_DIR', 'CLAUDECODE']) delete environment[key];
environment.ABIGAIL_ENTITY_DAEMON_PATH = join(binaryDir, 'entity-daemon.exe');
environment.ABIGAIL_DOCUMENTS_DIR = documentsDir;
let hive;
let fixture;
let hiveUrl;
let restartCount = 0;
const transport = [];

async function stage(name, action) {
  console.log(`CONTRACT ${name}`);
  const started = Date.now();
  try {
    const result = await action();
    const record = { name, passed: true, duration_ms: Date.now() - started };
    stages.push(record); appendFileSync(timeline, JSON.stringify(record) + '\n'); return result;
  } catch (error) {
    const record = { name, passed: false, duration_ms: Date.now() - started, error: error.message };
    stages.push(record); appendFileSync(timeline, JSON.stringify(record) + '\n'); throw error;
  }
}

async function raw(url, { method = 'GET', body, headers = {}, timeout = 60000 } = {}) {
  let response;
  let text;
  try {
    response = await fetch(url, { method, headers: { ...(body === undefined ? {} : { 'content-type': 'application/json' }), ...headers },
      body: body === undefined ? undefined : JSON.stringify(body), signal: AbortSignal.timeout(timeout) });
    text = await response.text();
  } catch (error) {
    transport.push({ method, url, error: error.message });
    throw error;
  }
  const record = { method, url, status: response.status, body: text };
  transport.push(record);
  let value;
  try { value = JSON.parse(text); } catch { value = text; }
  return { response, value };
}
async function api(url, options) {
  const { response, value } = await raw(url, options);
  assert(response.ok, `HTTP ${response.status} from ${url}: ${JSON.stringify(value)}`);
  assert(value?.ok === true, `API rejected ${url}: ${JSON.stringify(value)}`);
  return value.data;
}
async function until(action, message, timeout = 90000) {
  const deadline = Date.now() + timeout;
  let lastError;
  while (Date.now() < deadline) {
    try { const result = await action(); if (result) return result; } catch (error) { lastError = error; }
    await delay(300);
  }
  throw new Error(`${message}${lastError ? ': ' + lastError.message : ''}`);
}
async function freePort() {
  const server = createServer();
  await new Promise((resolvePromise, reject) => { server.once('error', reject); server.listen(0, '127.0.0.1', resolvePromise); });
  const port = server.address().port;
  await new Promise(resolvePromise => server.close(resolvePromise)); return port;
}
async function stopTree(child) {
  if (!child) return;
  assert(ownedProcesses.has(child), 'Refusing to stop an unowned process');
  if (process.platform === 'win32') {
    const stopped = await stopIsolatedWindowsDaemons({ binaryDir, dataDir });
    appendFileSync(timeline, JSON.stringify({ scoped_process_cleanup: stopped }) + '\n');
  } else if (child.exitCode === null && child.signalCode === null) { child.kill('SIGTERM'); }
  await until(() => child.exitCode !== null || child.signalCode !== null, `Owned Hive process ${child.pid} did not exit`, 10000);
  ownedProcesses.delete(child);
}
async function startHive() {
  const port = await freePort(); hiveUrl = `http://127.0.0.1:${port}`;
  const executable = join(binaryDir, 'hive-daemon.exe');
  assert(existsSync(executable), `Missing daemon ${executable}`);
  const child = spawn(executable, ['--port', String(port), '--data-dir', dataDir], { cwd: runDir, env: environment, windowsHide: true, stdio: ['ignore', 'pipe', 'pipe'] });
  child.stdout.pipe(createWriteStream(join(runDir, `hive-${restartCount}.stdout.txt`)));
  child.stderr.pipe(createWriteStream(join(runDir, `hive-${restartCount}.stderr.txt`)));
  child.on('error', error => appendFileSync(timeline, JSON.stringify({ spawn_error: error.message }) + '\n'));
  ownedProcesses.add(child); hive = child; restartCount++;
  await until(async () => {
    if (child.exitCode !== null) throw new Error(`Hive exited with code ${child.exitCode}`);
    const { response } = await raw(`${hiveUrl}/health`, { timeout: 2000 }); return response.ok;
  }, 'Hive did not become healthy');
  return until(async () => { const status = await api(`${hiveUrl}/v1/status`); return status.helper?.running && status.helper.local_url ? status : null; }, 'Hive helper did not become available');
}
async function streamChat(url, message) {
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), 60000);
  let events = ''; let tokens = ''; let done;
  try {
    const response = await fetch(`${url}/v1/chat/stream`, { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ message, session_id: 'main' }), signal: controller.signal });
    assert(response.ok, `Stream returned HTTP ${response.status}`);
    const reader = response.body.getReader(); const decoder = new TextDecoder();
    let pending = '';
    while (!done) {
      const chunk = await reader.read(); if (chunk.done) break;
      const text = decoder.decode(chunk.value, { stream: true }); events += text; pending += text;
      const frames = pending.split(/\r?\n\r?\n/); pending = frames.pop();
      for (const frame of frames) {
        const event = frame.match(/^event:\s*(.+)$/m)?.[1]?.trim();
        const data = frame.split(/\r?\n/).filter(line => line.startsWith('data:')).map(line => line.slice(5).trimStart()).join('\n');
        if (event === 'error') throw new Error(`Stream error: ${data}`);
        if (event === 'token') tokens += data;
        if (event === 'done') { done = JSON.parse(data); break; }
      }
    }
    await reader.cancel();
    assert(done, 'Stream ended without a done event');
    assert(done.reply.includes(message), `Stream reply did not come from fixture: ${done.reply}`);
    assert(tokens.includes(message), `Live stream did not deliver fixture text: ${tokens}`);
    return done;
  } finally {
    clearTimeout(timer); controller.abort();
    transport.push({ method: 'POST SSE', url, events });
  }
}
async function histories(entities) {
  return Promise.all(entities.map(entity => api(`${entity.url}/v1/chat/history?session_id=main&limit=200`)));
}
async function finishAll(tasks) {
  const results = await Promise.allSettled(tasks);
  const errors = results.filter(result => result.status === 'rejected').map(result => result.reason);
  if (errors.length) throw new AggregateError(errors, errors.map(error => error.message).join('; '));
  return results.map(result => result.value);
}
async function assertHistories(entities, expected) {
  const actual = await histories(entities);
  actual.forEach((history, index) => {
    assert.equal(history.session_id, 'main'); assert.deepEqual(history.messages, expected[index].messages, `Transcript changed for ${entities[index].name}`);
  }); return actual;
}

let result;
try {
  console.log('Synthetic contract acceptance; real-model quality is not evaluated.');
  fixture = await startModelFixture({ logPath: join(runDir, 'fixture-requests.jsonl') });
  const initial = await stage('start Hive and immortal helper with isolated disk store', startHive);
  const helper = { id: initial.entities.find(entity => entity.is_hive).id, name: 'Hive helper', url: initial.helper.local_url, marker: `MVP_HELPER_${runId}` };
  await stage('validate local provider discovery and completion heartbeat', async () => {
    const provider = await api(`${hiveUrl}/v1/providers/local`, { method: 'POST', body: { base_url: fixture.url } });
    assert.equal(provider.model, fixture.model);
  });
  const family = await stage('create and birth two Entities with distinct purposes', async () => Promise.all(['Alpha', 'Beta'].map(async name => {
    const entity = await api(`${hiveUrl}/v1/entities`, { method: 'POST', body: { name: `MVP ${name}` } });
    const birth = await api(`${hiveUrl}/v1/entities/${entity.id}/birth`, { method: 'POST', body: { path: 'quickstart', choices: [], purpose: `Help with ${name.toLowerCase()} household planning.` } });
    assert.equal(birth.certificate.entity_id, entity.id); assert(birth.certificate.signature);
    return { id: entity.id, name, marker: `MVP_${name.toUpperCase()}_${runId}` };
  })));
  await stage('Hive and Entity recovery Documents remain inside isolated run', async () => {
    for (const name of ['Abigail Hive', 'MVP Alpha', 'MVP Beta']) {
      assert(existsSync(join(documentsDir, 'Abigail', name)), `Missing isolated Documents folder for ${name}`);
    }
  });
  await stage('open both Entities while helper remains available', async () => {
    await Promise.all(family.map(async entity => {
      const open = await api(`${hiveUrl}/v1/entities/${entity.id}/open`, { method: 'POST', body: {} });
      entity.url = open.local_url;
      const status = await api(`${entity.url}/v1/status`); assert.equal(status.entity_id, entity.id);
    }));
    assert.equal(new Set([helper.url, ...family.map(entity => entity.url)]).size, 3);
    const status = await api(`${hiveUrl}/v1/status`); assert(status.helper.running);
  });
  const all = [helper, ...family];
  await stage('helper and two Entities complete simultaneous nonstream chat', async () => {
    await finishAll(all.map(async entity => {
      const response = await api(`${entity.url}/v1/chat`, { method: 'POST', body: { message: entity.marker, session_id: 'main' } });
      assert(response.reply.includes(entity.marker), `${entity.name} did not receive fixture reply: ${response.reply}`);
    }));
  });
  await stage('helper and two Entities deliver simultaneous token streams', () => finishAll(all.map(entity => streamChat(entity.url, `${entity.marker}_STREAM`))));
  const expected = await stage('durable main-session histories are isolated by Entity', async () => {
    const values = await histories(all);
    values.forEach((history, index) => {
      assert.equal(history.session_id, 'main'); assert.equal(history.messages.length, 4, `Expected two durable turns for ${all[index].name}`);
      const contents = history.messages.map(message => message.content).join('\n');
      assert(contents.includes(all[index].marker));
      all.forEach((entity, other) => { if (other !== index) assert(!contents.includes(entity.marker), `${all[index].name} can see ${entity.name} transcript`); });
    }); return values;
  });
  await stage('runtime leases cannot be minted or disclosed by unauthenticated clients', async () => {
    for (const entity of family) {
      const issued = await raw(`${hiveUrl}/v1/runtime/sessions`, { method: 'POST', body: { entity_id: entity.id, runtime_id: 'untrusted-contract-client' } });
      assert.equal(issued.response.status, 403, 'Unauthenticated client obtained a runtime lease');
      const disclosed = await raw(`${entity.url}/v1/session/status`);
      assert.equal(disclosed.response.status, 403, 'Unauthenticated client obtained runtime session metadata');
    }
  });
  await stage('persistence rejects invalid bearer for both Entity scopes', async () => {
    const operation = { operation: 'query_vec', sql: 'SELECT * FROM conversation_turn', bindings: [] };
    for (const entity of family) {
      const denied = await raw(`${hiveUrl}/v1/entities/${entity.id}/persistence`, { method: 'POST', body: operation, headers: { authorization: 'Bearer invalid-contract-lease' } });
      assert.equal(denied.response.status, 401); assert.equal(denied.value.ok, false);
    }
  });
  await stage('close and reopen both Entity daemons without transcript loss', async () => {
    await Promise.all(family.map(entity => api(`${hiveUrl}/v1/entities/${entity.id}/close`, { method: 'POST', body: {} })));
    await Promise.all(family.map(async entity => {
      const open = await api(`${hiveUrl}/v1/entities/${entity.id}/open`, { method: 'POST', body: {} }); entity.url = open.local_url;
    }));
    await assertHistories(all, expected);
  });
  await stage('stop entire owned Hive tree and restart from the same disk store', async () => {
    await stopTree(hive); await delay(500);
    const restarted = await startHive();
    helper.url = restarted.helper.local_url;
    assert(restarted.entities.some(entity => entity.id === family[0].id));
    await Promise.all(family.map(async entity => {
      const open = await api(`${hiveUrl}/v1/entities/${entity.id}/open`, { method: 'POST', body: {} }); entity.url = open.local_url;
    }));
    await assertHistories(all, expected);
    assert(existsSync(join(dataDir, 'memory.db')), 'Acceptance did not create the disk persistence root');
    assert(existsSync(join(dataDir, 'memory.db', 'wal')), 'Persistent database WAL is outside the requested isolated data directory');
  });
  result = { passed: true, kind: 'synthetic daemon contract acceptance', real_model_validation: false, binary_dir: binaryDir, data_dir: dataDir, documents_dir: documentsDir, entities: all, stages };
} catch (error) {
  result = { passed: false, kind: 'synthetic daemon contract acceptance', real_model_validation: false, binary_dir: binaryDir, data_dir: dataDir, documents_dir: documentsDir, stages, error: error.stack };
  console.error(error.stack); process.exitCode = 1;
} finally {
  for (const child of [...ownedProcesses]) {
    try { await stopTree(child); } catch (error) { result.cleanup_error = error.message; result.passed = false; process.exitCode = 1; }
  }
  if (fixture) await fixture.close();
  writeFileSync(join(runDir, 'http-traces.json'), JSON.stringify(transport, null, 2));
  writeFileSync(join(runDir, 'result.json'), JSON.stringify(result, null, 2));
  mkdirSync(artifactRoot, { recursive: true });
  writeFileSync(join(artifactRoot, 'latest.json'), JSON.stringify({ run_dir: runDir, passed: result.passed }, null, 2));
  console.log(`Contract result: ${result.passed ? 'PASS' : 'FAIL'}; ${join(runDir, 'result.json')}`);
}
